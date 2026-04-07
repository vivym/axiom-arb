use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use async_trait::async_trait;
use domain::{SignatureType, WalletRoute};
use venue_polymarket::{
    L2AuthHeaders, LiveRelayerApi, MarketWsEvent, PolymarketClobApi, PolymarketGateway,
    PolymarketGatewayError, PolymarketHeartbeatStatus, PolymarketOpenOrderSummary,
    PolymarketOrderQuery, PolymarketRestClient, PolymarketSignedOrder, PolymarketStreamApi,
    PolymarketSubmitResponse, PolymarketUserStreamAuth, PolymarketWsClient, RelayerAuth,
    RestClientBuildError, RestError, SignerContext, UserWsEvent, WsUserChannelAuth,
};

use crate::{config::PolymarketSourceConfig, LocalRelayerAuth, LocalSignerConfig};

const CONNECTIVITY_TIMEOUT: Duration = Duration::from_secs(5);
const HEARTBEAT_PREVIOUS_ID: &str = "doctor-preflight-heartbeat";

pub(crate) type ProbeFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), PolymarketProbeError>> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PolymarketProbeError {
    pub(crate) category: &'static str,
    pub(crate) message: String,
}

impl PolymarketProbeError {
    pub(crate) fn new(category: &'static str, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct UserWsProbeAuth<'a> {
    pub(crate) address: &'a str,
    pub(crate) api_key: &'a str,
    pub(crate) secret: &'a str,
    pub(crate) passphrase: &'a str,
}

pub(crate) trait PolymarketProbeFacade {
    fn fetch_open_orders<'a>(&'a mut self, signer_config: &'a LocalSignerConfig)
        -> ProbeFuture<'a>;
    fn subscribe_market_assets<'a>(&'a mut self, token_ids: &'a [String]) -> ProbeFuture<'a>;
    fn subscribe_user_markets<'a>(
        &'a mut self,
        auth: UserWsProbeAuth<'a>,
        condition_ids: &'a [String],
    ) -> ProbeFuture<'a>;
    fn post_order_heartbeat<'a>(
        &'a mut self,
        signer_config: &'a LocalSignerConfig,
    ) -> ProbeFuture<'a>;
    fn fetch_recent_transactions<'a>(
        &'a mut self,
        signer_config: &'a LocalSignerConfig,
    ) -> ProbeFuture<'a>;
}

pub(crate) struct LivePolymarketProbe {
    source_config: PolymarketSourceConfig,
}

impl LivePolymarketProbe {
    pub(crate) fn new(source_config: PolymarketSourceConfig) -> Self {
        Self { source_config }
    }

    fn rest_client(&self) -> Result<PolymarketRestClient, RestClientBuildError> {
        PolymarketRestClient::new(
            self.source_config.clob_host.clone(),
            self.source_config.data_api_host.clone(),
            self.source_config.relayer_host.clone(),
            self.source_config.outbound_proxy_url.clone(),
            None,
        )
    }

    fn clob_gateway(
        &self,
        signer_config: &LocalSignerConfig,
    ) -> Result<PolymarketGateway, PolymarketProbeError> {
        let client = self
            .rest_client()
            .map_err(|error| PolymarketProbeError::new("ConnectivityError", error.to_string()))?;
        Ok(PolymarketGateway::from_clob_api(Arc::new(
            LegacyClobProbeApi::new(client, signer_config.clone()),
        )))
    }

    fn stream_gateway(&self) -> PolymarketGateway {
        PolymarketGateway::from_stream_api(Arc::new(LegacyStreamProbeApi::new(
            self.source_config.clone(),
        )))
    }

    fn relayer_gateway(&self) -> Result<PolymarketGateway, PolymarketProbeError> {
        let client = self
            .rest_client()
            .map_err(|error| PolymarketProbeError::new("ConnectivityError", error.to_string()))?;
        Ok(PolymarketGateway::from_relayer_api(Arc::new(
            LiveRelayerApi::new(client),
        )))
    }
}

impl PolymarketProbeFacade for LivePolymarketProbe {
    fn fetch_open_orders<'a>(
        &'a mut self,
        signer_config: &'a LocalSignerConfig,
    ) -> ProbeFuture<'a> {
        Box::pin(async move {
            let gateway = self.clob_gateway(signer_config)?;
            timeout_probe("authenticated REST", async move {
                gateway
                    .open_orders(PolymarketOrderQuery::open_orders())
                    .await
                    .map(|_| ())
                    .map_err(map_gateway_error)
            })
            .await
        })
    }

    fn subscribe_market_assets<'a>(&'a mut self, token_ids: &'a [String]) -> ProbeFuture<'a> {
        Box::pin(async move {
            let gateway = self.stream_gateway();
            timeout_probe("market websocket", async move {
                gateway
                    .collect_market_events(token_ids.to_vec())
                    .await
                    .map(|_| ())
                    .map_err(map_gateway_error)
            })
            .await
        })
    }

    fn subscribe_user_markets<'a>(
        &'a mut self,
        auth: UserWsProbeAuth<'a>,
        condition_ids: &'a [String],
    ) -> ProbeFuture<'a> {
        Box::pin(async move {
            let gateway = self.stream_gateway();
            let auth = PolymarketUserStreamAuth {
                address: auth.address.to_owned(),
                api_key: auth.api_key.to_owned(),
                secret: auth.secret.to_owned(),
                passphrase: auth.passphrase.to_owned(),
            };
            timeout_probe("user websocket", async move {
                gateway
                    .collect_user_events(auth, condition_ids.to_vec())
                    .await
                    .map(|_| ())
                    .map_err(map_gateway_error)
            })
            .await
        })
    }

    fn post_order_heartbeat<'a>(
        &'a mut self,
        signer_config: &'a LocalSignerConfig,
    ) -> ProbeFuture<'a> {
        Box::pin(async move {
            let gateway = self.clob_gateway(signer_config)?;
            timeout_probe("heartbeat", async move {
                gateway
                    .post_heartbeat(Some(HEARTBEAT_PREVIOUS_ID))
                    .await
                    .map(|_| ())
                    .map_err(map_gateway_error)
            })
            .await
        })
    }

    fn fetch_recent_transactions<'a>(
        &'a mut self,
        signer_config: &'a LocalSignerConfig,
    ) -> ProbeFuture<'a> {
        Box::pin(async move {
            let gateway = self.relayer_gateway()?;
            let auth = relayer_auth_from_signer_config(signer_config)?;
            timeout_probe("relayer reachability", async move {
                gateway
                    .recent_transactions(&auth)
                    .await
                    .map(|_| ())
                    .map_err(map_gateway_error)
            })
            .await
        })
    }
}

struct LegacyClobProbeApi {
    client: PolymarketRestClient,
    signer_config: LocalSignerConfig,
}

impl LegacyClobProbeApi {
    fn new(client: PolymarketRestClient, signer_config: LocalSignerConfig) -> Self {
        Self {
            client,
            signer_config,
        }
    }
}

#[async_trait]
impl PolymarketClobApi for LegacyClobProbeApi {
    async fn open_orders(
        &self,
        query: &PolymarketOrderQuery,
    ) -> Result<Vec<PolymarketOpenOrderSummary>, PolymarketGatewayError> {
        match query {
            PolymarketOrderQuery::OpenOrders => {
                let auth = l2_auth_headers_from_signer_config(&self.signer_config)
                    .map_err(map_probe_protocol_error)?;
                self.client
                    .fetch_open_orders(&auth)
                    .await
                    .map(|orders| {
                        orders
                            .into_iter()
                            .map(|order| PolymarketOpenOrderSummary {
                                order_id: order.order_id,
                            })
                            .collect()
                    })
                    .map_err(map_rest_error)
            }
        }
    }

    async fn submit_order(
        &self,
        _order: &PolymarketSignedOrder,
    ) -> Result<PolymarketSubmitResponse, PolymarketGatewayError> {
        Err(PolymarketGatewayError::protocol(
            "submit_order is not part of doctor connectivity probes",
        ))
    }

    async fn post_heartbeat(
        &self,
        previous_heartbeat_id: Option<&str>,
    ) -> Result<PolymarketHeartbeatStatus, PolymarketGatewayError> {
        let auth = l2_auth_headers_from_signer_config(&self.signer_config)
            .map_err(map_probe_protocol_error)?;
        let previous_heartbeat_id = previous_heartbeat_id.unwrap_or(HEARTBEAT_PREVIOUS_ID);
        self.client
            .post_order_heartbeat(&auth, previous_heartbeat_id)
            .await
            .map(|heartbeat| PolymarketHeartbeatStatus {
                heartbeat_id: heartbeat.heartbeat_id,
                valid: heartbeat.valid,
            })
            .map_err(map_rest_error)
    }
}

struct LegacyStreamProbeApi {
    source_config: PolymarketSourceConfig,
}

impl LegacyStreamProbeApi {
    fn new(source_config: PolymarketSourceConfig) -> Self {
        Self { source_config }
    }

    async fn connect(&self) -> Result<PolymarketWsClient, PolymarketGatewayError> {
        PolymarketWsClient::connect_with_proxy(
            self.source_config.market_ws_url.clone(),
            self.source_config.user_ws_url.clone(),
            self.source_config.outbound_proxy_url.clone(),
        )
        .await
        .map_err(|error| PolymarketGatewayError::connectivity(error.to_string()))
    }
}

#[async_trait]
impl PolymarketStreamApi for LegacyStreamProbeApi {
    async fn market_events(
        &self,
        token_ids: &[String],
    ) -> Result<Vec<MarketWsEvent>, PolymarketGatewayError> {
        let mut client = self.connect().await?;
        client
            .subscribe_market_assets(token_ids, false)
            .await
            .map_err(|error| PolymarketGatewayError::connectivity(error.to_string()))?;
        let event = client
            .next_market_event()
            .await
            .map_err(|error| PolymarketGatewayError::connectivity(error.to_string()))?;
        Ok(vec![event])
    }

    async fn user_events(
        &self,
        auth: &PolymarketUserStreamAuth,
        condition_ids: &[String],
    ) -> Result<Vec<UserWsEvent>, PolymarketGatewayError> {
        let mut client = self.connect().await?;
        client
            .subscribe_user_markets(
                &WsUserChannelAuth {
                    api_key: &auth.api_key,
                    secret: &auth.secret,
                    passphrase: &auth.passphrase,
                },
                condition_ids,
            )
            .await
            .map_err(|error| PolymarketGatewayError::connectivity(error.to_string()))?;
        let event = client
            .next_user_event()
            .await
            .map_err(|error| PolymarketGatewayError::connectivity(error.to_string()))?;
        Ok(vec![event])
    }
}

async fn timeout_probe<F, T>(label: &str, future: F) -> Result<T, PolymarketProbeError>
where
    F: Future<Output = Result<T, PolymarketProbeError>>,
{
    tokio::time::timeout(CONNECTIVITY_TIMEOUT, future)
        .await
        .map_err(|_| {
            PolymarketProbeError::new(
                "ConnectivityError",
                format!(
                    "{label} probe timed out after {}s",
                    CONNECTIVITY_TIMEOUT.as_secs()
                ),
            )
        })?
}

fn map_gateway_error(error: PolymarketGatewayError) -> PolymarketProbeError {
    PolymarketProbeError::new("ConnectivityError", error.to_string())
}

fn map_probe_protocol_error(error: PolymarketProbeError) -> PolymarketGatewayError {
    PolymarketGatewayError::protocol(error.message)
}

fn map_rest_error(error: RestError) -> PolymarketGatewayError {
    match error {
        RestError::Auth(error) => PolymarketGatewayError::auth(error.to_string()),
        RestError::Http(error) => PolymarketGatewayError::connectivity(error.to_string()),
        RestError::HttpResponse { status, body } => {
            PolymarketGatewayError::upstream_response(format!("{status}: {body}"))
        }
        RestError::Gateway(error) => error,
        RestError::Metadata(error) => PolymarketGatewayError::protocol(error.to_string()),
        RestError::Url(error) => PolymarketGatewayError::protocol(error.to_string()),
        RestError::MissingField(field) => {
            PolymarketGatewayError::relayer(format!("missing response field: {field}"))
        }
    }
}

fn l2_auth_headers_from_signer_config<'a>(
    signer_config: &'a LocalSignerConfig,
) -> Result<L2AuthHeaders<'a>, PolymarketProbeError> {
    Ok(L2AuthHeaders {
        signer: SignerContext {
            address: &signer_config.signer.address,
            funder_address: &signer_config.signer.funder_address,
            signature_type: parse_signature_type(&signer_config.signer.signature_type)?,
            wallet_route: parse_wallet_route(&signer_config.signer.wallet_route)?,
        },
        api_key: &signer_config.l2_auth.api_key,
        passphrase: &signer_config.l2_auth.passphrase,
        timestamp: &signer_config.l2_auth.timestamp,
        signature: &signer_config.l2_auth.signature,
    })
}

fn relayer_auth_from_signer_config<'a>(
    signer_config: &'a LocalSignerConfig,
) -> Result<RelayerAuth<'a>, PolymarketProbeError> {
    Ok(match &signer_config.relayer_auth {
        LocalRelayerAuth::BuilderApiKey {
            api_key,
            timestamp,
            passphrase,
            signature,
        } => RelayerAuth::BuilderApiKey {
            api_key,
            timestamp,
            passphrase,
            signature,
        },
        LocalRelayerAuth::RelayerApiKey { api_key, address } => {
            RelayerAuth::RelayerApiKey { api_key, address }
        }
    })
}

fn parse_signature_type(label: &str) -> Result<SignatureType, PolymarketProbeError> {
    match label.trim().to_ascii_lowercase().as_str() {
        "eoa" => Ok(SignatureType::Eoa),
        "proxy" | "poly_proxy" => Ok(SignatureType::Proxy),
        "safe" | "gnosis_safe" => Ok(SignatureType::Safe),
        other => Err(PolymarketProbeError::new(
            "ConnectivityError",
            format!("unsupported signature type label {other}"),
        )),
    }
}

fn parse_wallet_route(label: &str) -> Result<WalletRoute, PolymarketProbeError> {
    match label.trim().to_ascii_lowercase().as_str() {
        "eoa" => Ok(WalletRoute::Eoa),
        "proxy" => Ok(WalletRoute::Proxy),
        "safe" => Ok(WalletRoute::Safe),
        other => Err(PolymarketProbeError::new(
            "ConnectivityError",
            format!("unsupported wallet route label {other}"),
        )),
    }
}
