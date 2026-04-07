use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use async_trait::async_trait;
use domain::{SignatureType, WalletRoute};
use tokio::sync::Mutex as AsyncMutex;
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
    stream_api: Arc<LegacyStreamProbeApi>,
}

impl LivePolymarketProbe {
    pub(crate) fn new(source_config: PolymarketSourceConfig) -> Self {
        Self {
            stream_api: Arc::new(LegacyStreamProbeApi::new(source_config.clone())),
            source_config,
        }
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
        PolymarketGateway::from_stream_api(self.stream_api.clone())
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
                    .map_err(|error| map_gateway_error("authenticated REST", error))
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
                    .map_err(|error| map_gateway_error("market websocket", error))
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
                    .map_err(|error| map_gateway_error("user websocket", error))
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
                    .map_err(|error| map_gateway_error("heartbeat", error))
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
                    .map_err(|error| map_gateway_error("relayer reachability", error))
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
    ws_client: AsyncMutex<Option<PolymarketWsClient>>,
}

impl LegacyStreamProbeApi {
    fn new(source_config: PolymarketSourceConfig) -> Self {
        Self {
            source_config,
            ws_client: AsyncMutex::new(None),
        }
    }

    async fn ensure_ws_client(&self) -> Result<(), PolymarketGatewayError> {
        let mut guard = self.ws_client.lock().await;
        if guard.is_none() {
            let market_ws_url = self.source_config.market_ws_url.clone();
            let user_ws_url = self.source_config.user_ws_url.clone();
            let outbound_proxy_url = self.source_config.outbound_proxy_url.clone();
            let client = timeout_gateway_probe("websocket connection", async move {
                PolymarketWsClient::connect_with_proxy(
                    market_ws_url,
                    user_ws_url,
                    outbound_proxy_url,
                )
                .await
                .map_err(|error| PolymarketGatewayError::connectivity(error.to_string()))
            })
            .await?;
            *guard = Some(client);
        }
        Ok(())
    }
}

#[async_trait]
impl PolymarketStreamApi for LegacyStreamProbeApi {
    async fn market_events(
        &self,
        token_ids: &[String],
    ) -> Result<Vec<MarketWsEvent>, PolymarketGatewayError> {
        self.ensure_ws_client().await?;
        let mut guard = self.ws_client.lock().await;
        let client = guard
            .as_mut()
            .expect("websocket client should be initialized");

        timeout_gateway_probe("market websocket", async {
            client
                .subscribe_market_assets(token_ids, false)
                .await
                .map_err(|error| PolymarketGatewayError::connectivity(error.to_string()))?;
            let event = client
                .next_market_event()
                .await
                .map_err(|error| PolymarketGatewayError::connectivity(error.to_string()))?;
            Ok(vec![event])
        })
        .await
    }

    async fn user_events(
        &self,
        auth: &PolymarketUserStreamAuth,
        condition_ids: &[String],
    ) -> Result<Vec<UserWsEvent>, PolymarketGatewayError> {
        self.ensure_ws_client().await?;
        let mut guard = self.ws_client.lock().await;
        let client = guard
            .as_mut()
            .expect("websocket client should be initialized");

        timeout_gateway_probe("user websocket", async {
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
        })
        .await
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

async fn timeout_gateway_probe<F, T>(label: &str, future: F) -> Result<T, PolymarketGatewayError>
where
    F: Future<Output = Result<T, PolymarketGatewayError>>,
{
    tokio::time::timeout(CONNECTIVITY_TIMEOUT, future)
        .await
        .map_err(|_| {
            PolymarketGatewayError::connectivity(format!(
                "{label} probe timed out after {}s",
                CONNECTIVITY_TIMEOUT.as_secs()
            ))
        })?
}

fn map_gateway_error(label: &str, error: PolymarketGatewayError) -> PolymarketProbeError {
    PolymarketProbeError::new(
        "ConnectivityError",
        format!("{label} probe failed: {error}"),
    )
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

#[cfg(test)]
mod tests {
    use std::{net::TcpListener as StdTcpListener, sync::OnceLock, thread, time::Duration};

    use tokio::sync::Mutex;
    use tokio_tungstenite::tungstenite::{accept as accept_websocket, Message as WsMessage};

    use super::*;

    #[tokio::test]
    async fn legacy_stream_probe_reuses_single_connection_pair_across_market_and_user_events() {
        let _guard = env_lock().lock().await;
        let _proxy_guard = ProxyEnvGuard::clear();

        let market_ws = ProbeWsServer::spawn(WsProbeKind::Market);
        let user_ws = ProbeWsServer::spawn(WsProbeKind::User);
        let api = LegacyStreamProbeApi::new(sample_source_config(market_ws.url(), user_ws.url()));

        let market_events = api
            .market_events(&["token-1".to_owned()])
            .await
            .expect("market events should succeed");
        assert_eq!(market_events.len(), 1);

        let user_events = api
            .user_events(&sample_user_stream_auth(), &["condition-1".to_owned()])
            .await
            .expect("user events should succeed");
        assert!(matches!(user_events[0], UserWsEvent::Trade(_)));
    }

    #[test]
    fn gateway_failures_keep_the_probe_label_in_the_error_message() {
        let error = map_gateway_error(
            "authenticated REST",
            PolymarketGatewayError::upstream_response("401 Unauthorized"),
        );

        assert_eq!(error.category, "ConnectivityError");
        assert!(error.message.contains("authenticated REST probe failed"));
        assert!(error.message.contains("401 Unauthorized"));
    }

    fn sample_source_config(market_ws_url: &str, user_ws_url: &str) -> PolymarketSourceConfig {
        PolymarketSourceConfig {
            clob_host: "http://127.0.0.1:1".parse().expect("clob host"),
            data_api_host: "http://127.0.0.1:1".parse().expect("data api host"),
            relayer_host: "http://127.0.0.1:1".parse().expect("relayer host"),
            market_ws_url: market_ws_url.parse().expect("market ws url"),
            user_ws_url: user_ws_url.parse().expect("user ws url"),
            outbound_proxy_url: None,
            heartbeat_interval_seconds: 15,
            relayer_poll_interval_seconds: 5,
            metadata_refresh_interval_seconds: 60,
        }
    }

    fn sample_user_stream_auth() -> PolymarketUserStreamAuth {
        PolymarketUserStreamAuth {
            address: "0x1111111111111111111111111111111111111111".to_owned(),
            api_key: "poly-api-key".to_owned(),
            secret: "poly-secret".to_owned(),
            passphrase: "poly-passphrase".to_owned(),
        }
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct ProxyEnvGuard {
        all_proxy: Option<String>,
        all_proxy_lower: Option<String>,
    }

    impl ProxyEnvGuard {
        fn clear() -> Self {
            let guard = Self {
                all_proxy: std::env::var("ALL_PROXY").ok(),
                all_proxy_lower: std::env::var("all_proxy").ok(),
            };
            std::env::remove_var("ALL_PROXY");
            std::env::remove_var("all_proxy");
            guard
        }
    }

    impl Drop for ProxyEnvGuard {
        fn drop(&mut self) {
            restore_env_var("ALL_PROXY", self.all_proxy.take());
            restore_env_var("all_proxy", self.all_proxy_lower.take());
        }
    }

    fn restore_env_var(key: &str, value: Option<String>) {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    struct ProbeWsServer {
        url: String,
        shutdown_tx: Option<std::sync::mpsc::Sender<()>>,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl ProbeWsServer {
        fn spawn(kind: WsProbeKind) -> Self {
            let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind ws probe server");
            let address = listener.local_addr().expect("ws probe server address");
            listener
                .set_nonblocking(true)
                .expect("ws probe server should be nonblocking");
            let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();
            let handle = thread::spawn(move || loop {
                if shutdown_rx.try_recv().is_ok() {
                    break;
                }

                match listener.accept() {
                    Ok((stream, _)) => {
                        stream
                            .set_nonblocking(false)
                            .expect("accepted ws stream should be blocking");
                        let mut websocket =
                            accept_websocket(stream).expect("accept ws probe websocket");
                        let mut responded = false;
                        loop {
                            match websocket.read() {
                                Ok(WsMessage::Text(_)) if !responded => {
                                    websocket
                                        .send(WsMessage::Text(kind.response_payload().into()))
                                        .expect("send ws probe response");
                                    responded = true;
                                }
                                Ok(_) => {}
                                Err(_) => break,
                            }
                        }
                        break;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("ws probe server accept failed: {error}"),
                }
            });

            Self {
                url: format!("ws://{address}"),
                shutdown_tx: Some(shutdown_tx),
                handle: Some(handle),
            }
        }

        fn url(&self) -> &str {
            &self.url
        }
    }

    impl Drop for ProbeWsServer {
        fn drop(&mut self) {
            if let Some(shutdown_tx) = self.shutdown_tx.take() {
                let _ = shutdown_tx.send(());
            }
            if let Some(handle) = self.handle.take() {
                handle.join().expect("ws probe server should join");
            }
        }
    }

    #[derive(Clone, Copy)]
    enum WsProbeKind {
        Market,
        User,
    }

    impl WsProbeKind {
        fn response_payload(self) -> &'static str {
            match self {
                Self::Market => {
                    r#"{"event":"book","asset_id":"token-1","best_bid":"0.40","best_ask":"0.41"}"#
                }
                Self::User => {
                    r#"{"event":"trade","trade_id":"trade-1","order_id":"order-1","status":"MATCHED","condition_id":"condition-1","price":"0.41","size":"100","fee_rate_bps":"15","transaction_hash":"0xtrade"}"#
                }
            }
        }
    }
}
