use std::{
    borrow::Cow,
    str::FromStr,
    sync::{Arc, Mutex},
};

use alloy::dyn_abi::Eip712Domain;
use alloy::hex::ToHexExt as _;
use alloy::signers::{local::PrivateKeySigner, Signer as _};
use alloy::sol_types::SolStruct as _;
use domain::ExecutionMode;
use execution::negrisk::plan_family_submission;
use execution::plans::ExecutionPlan;
use execution::providers::{SignerProvider, SubmitProviderError, VenueExecutionProvider};
use execution::signing::{OrderSigner, SignedFamilySubmission, SigningError};
use execution::sink::LiveVenueSink;
use execution::ExecutionInstrumentation;
use polymarket_client_sdk::auth::{state::Authenticated, Normal};
use polymarket_client_sdk::auth::{Credentials as SdkCredentials, Uuid};
use polymarket_client_sdk::clob::types::{
    OrderType as SdkOrderType, Side as SdkSide, SignatureType as SdkSignatureType,
};
use polymarket_client_sdk::clob::{Client as SdkClobClient, Config as SdkClobConfig};
use polymarket_client_sdk::gamma::Client as SdkGammaClient;
use polymarket_client_sdk::types::{Address as SdkAddress, U256};
use polymarket_client_sdk::{contract_config, POLYGON, PRIVATE_KEY_VAR};
use tokio::runtime::Runtime;
use venue_polymarket::{
    build_post_order_request_from_signed_member, LiveClobSdkApi, LiveMetadataSdkApi,
    PolymarketGateway, PolymarketNegRiskSubmitProvider, PostOrderTransport,
};

use crate::negrisk_live::{
    to_execution_target, NegRiskLiveArtifact, NegRiskLiveError, NegRiskLiveExecutionBackend,
    NegRiskLiveExecutionRecord,
};
use crate::{
    config::{NegRiskFamilyLiveTarget, PolymarketSourceConfig},
    PolymarketGatewayCredentials,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PolymarketRuntimeAdapterError {
    MissingPrivateKeyEnv { env_var: &'static str },
    InvalidPrivateKey(String),
    InvalidApiKey(String),
    InvalidAddress { field: &'static str, value: String },
    InvalidSignatureType(String),
    Sdk(String),
}

impl std::fmt::Display for PolymarketRuntimeAdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingPrivateKeyEnv { env_var } => {
                write!(f, "missing required environment variable {env_var}")
            }
            Self::InvalidPrivateKey(message) => {
                write!(f, "invalid polymarket private key: {message}")
            }
            Self::InvalidApiKey(value) => write!(f, "invalid polymarket api key UUID: {value}"),
            Self::InvalidAddress { field, value } => {
                write!(f, "invalid {field}: {value}")
            }
            Self::InvalidSignatureType(value) => {
                write!(f, "unsupported polymarket signature_type: {value}")
            }
            Self::Sdk(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for PolymarketRuntimeAdapterError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PolymarketMetadataGatewayBackend {
    Sdk,
}

pub(crate) fn polymarket_metadata_gateway_backend(
    _source: &PolymarketSourceConfig,
) -> PolymarketMetadataGatewayBackend {
    PolymarketMetadataGatewayBackend::Sdk
}

pub(crate) fn build_polymarket_metadata_gateway(
    source: &PolymarketSourceConfig,
) -> Result<PolymarketGateway, PolymarketRuntimeAdapterError> {
    let client = SdkGammaClient::new(source.data_api_host.as_str()).map_err(|error| {
        PolymarketRuntimeAdapterError::Sdk(format!(
            "polymarket metadata sdk client build failed: {error}"
        ))
    })?;
    Ok(PolymarketGateway::from_metadata_api(Arc::new(
        LiveMetadataSdkApi::new(client),
    )))
}

#[derive(Debug, Clone)]
pub(crate) struct PolymarketOrderSigner {
    runtime: Arc<Runtime>,
    client: SdkClobClient<Authenticated<Normal>>,
    signer: PrivateKeySigner,
}

impl PolymarketOrderSigner {
    pub(crate) fn from_runtime_inputs(
        source: &PolymarketSourceConfig,
        credentials: &PolymarketGatewayCredentials,
    ) -> Result<Self, PolymarketRuntimeAdapterError> {
        let private_key = std::env::var(PRIVATE_KEY_VAR).map_err(|_| {
            PolymarketRuntimeAdapterError::MissingPrivateKeyEnv {
                env_var: PRIVATE_KEY_VAR,
            }
        })?;
        let signer = PrivateKeySigner::from_str(&private_key)
            .map_err(|error| PolymarketRuntimeAdapterError::InvalidPrivateKey(error.to_string()))?
            .with_chain_id(Some(POLYGON));
        let api_key = Uuid::parse_str(&credentials.api_key).map_err(|_| {
            PolymarketRuntimeAdapterError::InvalidApiKey(credentials.api_key.clone())
        })?;
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .map_err(|error| PolymarketRuntimeAdapterError::Sdk(error.to_string()))?,
        );
        let client = runtime.block_on(async {
            let base = SdkClobClient::new(source.clob_host.as_str(), SdkClobConfig::default())
                .map_err(|error| PolymarketRuntimeAdapterError::Sdk(error.to_string()))?;
            let mut auth = base
                .authentication_builder(&signer)
                .credentials(SdkCredentials::new(
                    api_key,
                    credentials.secret.clone(),
                    credentials.passphrase.clone(),
                ))
                .signature_type(parse_signature_type(&credentials.signature_type)?);
            if !matches!(
                parse_signature_type(&credentials.signature_type)?,
                SdkSignatureType::Eoa
            ) {
                auth = auth.funder(parse_address(
                    "polymarket.account.funder_address",
                    &credentials.funder_address,
                )?);
            }
            auth.authenticate()
                .await
                .map_err(|error| PolymarketRuntimeAdapterError::Sdk(error.to_string()))
        })?;
        Ok(Self {
            runtime,
            client,
            signer,
        })
    }
}

impl OrderSigner for PolymarketOrderSigner {
    fn sign_family(&self, plan: &ExecutionPlan) -> Result<SignedFamilySubmission, SigningError> {
        match plan {
            ExecutionPlan::NegRiskSubmitFamily { members, .. } => {
                let plan_id = plan.plan_id();
                let mut canonical_members: Vec<_> = members.iter().collect();
                canonical_members.sort_by(|left, right| {
                    left.condition_id
                        .as_str()
                        .cmp(right.condition_id.as_str())
                        .then_with(|| left.token_id.as_str().cmp(right.token_id.as_str()))
                        .then_with(|| {
                            left.price
                                .normalize()
                                .to_string()
                                .cmp(&right.price.normalize().to_string())
                        })
                        .then_with(|| {
                            left.quantity
                                .normalize()
                                .to_string()
                                .cmp(&right.quantity.normalize().to_string())
                        })
                });

                let signed_members = canonical_members
                    .into_iter()
                    .map(|member| {
                        let token_id =
                            U256::from_str(member.token_id.as_str()).map_err(|error| {
                                SigningError::SignerFailure {
                                    plan_id: plan_id.clone(),
                                    reason: format!(
                                        "invalid token_id {}: {error}",
                                        member.token_id.as_str()
                                    ),
                                }
                            })?;

                        let (signed_order, is_neg_risk) = self
                            .runtime
                            .block_on(async {
                                let signable = self
                                    .client
                                    .limit_order()
                                    .token_id(token_id)
                                    .price(member.price)
                                    .size(member.quantity)
                                    .side(SdkSide::Buy)
                                    .order_type(SdkOrderType::GTC)
                                    .post_only(false)
                                    .build()
                                    .await
                                    .map_err(|error| error.to_string())?;
                                let signed_order = self
                                    .client
                                    .sign(&self.signer, signable)
                                    .await
                                    .map_err(|error| error.to_string())?;
                                let is_neg_risk = self
                                    .client
                                    .neg_risk(token_id)
                                    .await
                                    .map_err(|error| error.to_string())?
                                    .neg_risk;
                                Ok::<_, String>((signed_order, is_neg_risk))
                            })
                            .map_err(|reason| SigningError::SignerFailure {
                                plan_id: plan_id.clone(),
                                reason,
                            })?;

                        let signed_order_hash = signed_order_hash(
                            &signed_order,
                            self.signer.chain_id().unwrap_or(POLYGON),
                            is_neg_risk,
                        )
                        .map_err(|reason| SigningError::SignerFailure {
                            plan_id: plan_id.clone(),
                            reason,
                        })?;

                        Ok(execution::signing::SignedFamilyMember {
                            condition_id: member.condition_id.clone(),
                            token_id: member.token_id.clone(),
                            price: member.price,
                            quantity: member.quantity,
                            maker: signed_order.order.maker.to_string(),
                            signer: signed_order.order.signer.to_string(),
                            taker: signed_order.order.taker.to_string(),
                            maker_amount: signed_order.order.makerAmount.to_string(),
                            taker_amount: signed_order.order.takerAmount.to_string(),
                            side: match SdkSide::try_from(signed_order.order.side) {
                                Ok(SdkSide::Buy) => "BUY".to_owned(),
                                Ok(SdkSide::Sell) => "SELL".to_owned(),
                                Ok(SdkSide::Unknown) | Ok(_) | Err(_) => {
                                    return Err(SigningError::SignerFailure {
                                        plan_id: plan_id.clone(),
                                        reason: format!(
                                            "unsupported sdk side value: {}",
                                            signed_order.order.side
                                        ),
                                    });
                                }
                            },
                            expiration: signed_order.order.expiration.to_string(),
                            fee_rate_bps: signed_order.order.feeRateBps.to_string(),
                            signature_type: signed_order.order.signatureType,
                            identity: domain::SignedOrderIdentity {
                                signed_order_hash,
                                salt: signed_order.order.salt.to_string(),
                                nonce: signed_order.order.nonce.to_string(),
                                signature: signed_order.signature.to_string(),
                            },
                        })
                    })
                    .collect::<Result<Vec<_>, SigningError>>()?;

                Ok(SignedFamilySubmission {
                    plan_id,
                    members: signed_members,
                })
            }
            other => Err(SigningError::UnsupportedPlan {
                plan_id: other.plan_id(),
            }),
        }
    }
}

#[derive(Clone)]
pub(crate) struct PolymarketLiveExecutionBackend {
    signer: Arc<PolymarketOrderSigner>,
    gateway: PolymarketGateway,
    runtime: Arc<Runtime>,
    transport: PostOrderTransport,
}

impl PolymarketLiveExecutionBackend {
    pub(crate) fn from_runtime_inputs(
        source: &PolymarketSourceConfig,
        credentials: &PolymarketGatewayCredentials,
    ) -> Result<Self, PolymarketRuntimeAdapterError> {
        let signer = Arc::new(PolymarketOrderSigner::from_runtime_inputs(
            source,
            credentials,
        )?);
        let gateway =
            PolymarketGateway::from_clob_api(Arc::new(LiveClobSdkApi::new(signer.client.clone())));
        let runtime = signer.runtime.clone();

        Ok(Self {
            signer,
            gateway,
            runtime,
            transport: PostOrderTransport {
                owner: credentials.api_key.clone(),
                order_type: venue_polymarket::OrderType::Gtc,
                defer_exec: false,
            },
        })
    }
}

impl NegRiskLiveExecutionBackend for PolymarketLiveExecutionBackend {
    fn execute_live_family(
        &self,
        snapshot_id: &str,
        target: &NegRiskFamilyLiveTarget,
        matched_rule_id: &str,
        instrumentation: ExecutionInstrumentation,
    ) -> Result<NegRiskLiveExecutionRecord, NegRiskLiveError> {
        let request = domain::ExecutionRequest {
            request_id: format!("negrisk-live-request:{snapshot_id}:{}", target.family_id),
            decision_input_id: format!("negrisk-live-intent:{snapshot_id}:{}", target.family_id),
            snapshot_id: snapshot_id.to_owned(),
            route: execution::negrisk::ROUTE.to_owned(),
            scope: target.family_id.clone(),
            activation_mode: ExecutionMode::Live,
            matched_rule_id: Some(matched_rule_id.to_owned()),
        };
        let target = to_execution_target(target);
        let plan = plan_family_submission(&request, &target).map_err(|error| {
            NegRiskLiveError::Planning(format!("neg-risk live planning failed: {error:?}"))
        })?;

        let signed_capture = Arc::new(Mutex::new(None::<SignedFamilySubmission>));
        let sink = LiveVenueSink::with_submit_provider(
            Arc::new(RecordingSignerProvider {
                inner: self.signer.clone(),
                last_signed: signed_capture.clone(),
            }),
            Arc::new(GatewayBackedSubmitProvider {
                gateway: self.gateway.clone(),
                runtime: self.runtime.clone(),
                transport: self.transport.clone(),
            }),
        );
        let execution_record =
            execution::ExecutionOrchestrator::new_instrumented(sink, instrumentation)
                .execute_with_attempt(&execution::ExecutionPlanningInput::new(
                    request.clone(),
                    request.activation_mode,
                    plan.clone(),
                ))
                .map_err(|error| {
                    NegRiskLiveError::Sink(format!("neg-risk live sink failed: {error:?}"))
                })?;
        ensure_live_execution_succeeded(&execution_record.receipt)?;

        let signed = signed_capture
            .lock()
            .expect("signed capture lock should not be poisoned")
            .clone()
            .ok_or_else(|| {
                NegRiskLiveError::Sink(
                    "neg-risk live signer did not produce a signed family submission".to_owned(),
                )
            })?;
        let order_requests = signed
            .members
            .iter()
            .map(|member| {
                build_post_order_request_from_signed_member(member, &self.transport).map_err(
                    |error| {
                        NegRiskLiveError::Sink(format!(
                            "neg-risk live post-order build failed: {error:?}"
                        ))
                    },
                )
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|request| {
                serde_json::to_value(request).expect("post order request should serialize")
            })
            .collect::<Vec<_>>();
        let attempt_context = execution_record.attempt_context.clone();
        let artifact_requests = order_requests.clone();

        Ok(NegRiskLiveExecutionRecord {
            idempotency_key: format!("idem-{}", execution_record.attempt.attempt_id),
            attempt_id: execution_record.receipt.attempt_id,
            plan_id: execution_record.attempt.plan_id,
            snapshot_id: execution_record.attempt.snapshot_id,
            execution_mode: attempt_context.execution_mode,
            attempt_no: execution_record.attempt.attempt_no,
            route: attempt_context.route.clone(),
            scope: attempt_context.scope.clone(),
            matched_rule_id: attempt_context.matched_rule_id.clone(),
            submission_ref: execution_record.receipt.submission_ref,
            pending_ref: execution_record.receipt.pending_ref,
            artifacts: vec![NegRiskLiveArtifact {
                stream: "neg-risk-live-orders".to_owned(),
                payload: serde_json::json!({
                    "attempt_id": attempt_context.attempt_id,
                    "route": attempt_context.route,
                    "scope": attempt_context.scope,
                    "matched_rule_id": attempt_context.matched_rule_id,
                    "plan_id": signed.plan_id,
                    "requests": artifact_requests,
                }),
            }],
            order_requests,
        })
    }
}

fn parse_signature_type(value: &str) -> Result<SdkSignatureType, PolymarketRuntimeAdapterError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "eoa" => Ok(SdkSignatureType::Eoa),
        "proxy" => Ok(SdkSignatureType::Proxy),
        "gnosis_safe" => Ok(SdkSignatureType::GnosisSafe),
        other => Err(PolymarketRuntimeAdapterError::InvalidSignatureType(
            other.to_owned(),
        )),
    }
}

fn parse_address(
    field: &'static str,
    value: &str,
) -> Result<SdkAddress, PolymarketRuntimeAdapterError> {
    SdkAddress::from_str(value).map_err(|_| PolymarketRuntimeAdapterError::InvalidAddress {
        field,
        value: value.to_owned(),
    })
}

fn signed_order_hash(
    signed_order: &polymarket_client_sdk::clob::types::SignedOrder,
    chain_id: u64,
    is_neg_risk: bool,
) -> Result<String, String> {
    let exchange = contract_config(chain_id, is_neg_risk)
        .ok_or_else(|| format!("missing polymarket contract config for chain {chain_id}"))?
        .exchange;
    let domain = Eip712Domain {
        name: Some(Cow::Borrowed("Polymarket CTF Exchange")),
        version: Some(Cow::Borrowed("1")),
        chain_id: Some(U256::from(chain_id)),
        verifying_contract: Some(exchange),
        ..Eip712Domain::default()
    };
    Ok(signed_order
        .order
        .eip712_signing_hash(&domain)
        .encode_hex_with_prefix())
}

#[derive(Clone)]
struct RecordingSignerProvider {
    inner: Arc<PolymarketOrderSigner>,
    last_signed: Arc<Mutex<Option<SignedFamilySubmission>>>,
}

impl SignerProvider for RecordingSignerProvider {
    fn sign_family(&self, plan: &ExecutionPlan) -> Result<SignedFamilySubmission, SigningError> {
        let signed = OrderSigner::sign_family(self.inner.as_ref(), plan)?;
        *self
            .last_signed
            .lock()
            .expect("signed capture lock should not be poisoned") = Some(signed.clone());
        Ok(signed)
    }
}

#[derive(Clone)]
struct GatewayBackedSubmitProvider {
    gateway: PolymarketGateway,
    runtime: Arc<Runtime>,
    transport: PostOrderTransport,
}

impl VenueExecutionProvider for GatewayBackedSubmitProvider {
    fn submit_family(
        &self,
        signed: &SignedFamilySubmission,
        attempt: &domain::ExecutionAttemptContext,
    ) -> Result<domain::LiveSubmitOutcome, SubmitProviderError> {
        let provider = PolymarketNegRiskSubmitProvider::with_gateway_runtime(
            self.transport.clone(),
            self.gateway.clone(),
            self.runtime.handle().clone(),
        );
        provider.submit_family(signed, attempt)
    }
}

fn ensure_live_execution_succeeded(
    receipt: &domain::ExecutionReceipt,
) -> Result<(), NegRiskLiveError> {
    if receipt.outcome == domain::ExecutionAttemptOutcome::Succeeded {
        Ok(())
    } else {
        Err(NegRiskLiveError::Sink(format!(
            "unexpected neg-risk live receipt outcome: {:?}",
            receipt.outcome
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        str::FromStr,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Mutex, OnceLock,
        },
        thread,
        time::{Duration, Instant},
    };

    use alloy::primitives::U256;
    use async_trait::async_trait;
    use domain::{ExecutionAttemptContext, ExecutionMode};
    use execution::providers::VenueExecutionProvider;
    use execution::signing::OrderSigner;
    use execution::{
        plans::{ExecutionPlan, NegRiskMemberOrderPlan},
        LiveSubmitOutcome, SignedFamilySubmission,
    };
    use httpmock::{Method::GET, MockServer};
    use polymarket_client_sdk::PRIVATE_KEY_VAR;
    use rust_decimal::Decimal;
    use serde_json::json;
    use venue_polymarket::{
        OrderType, PolymarketClobApi, PolymarketGateway, PolymarketGatewayError,
        PolymarketHeartbeatStatus, PolymarketOpenOrderSummary, PolymarketOrderQuery,
        PolymarketSignedOrder, PolymarketSubmitResponse, PostOrderTransport,
    };

    use super::{
        polymarket_metadata_gateway_backend, GatewayBackedSubmitProvider,
        PolymarketMetadataGatewayBackend, PolymarketOrderSigner,
    };
    use crate::{config::PolymarketSourceConfig, PolymarketGatewayCredentials};

    const TEST_PRIVATE_KEY: &str =
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    const TEST_TOKEN_ID: &str =
        "15871154585880608648532107628464183779895785213830018178010423617714102767076";

    #[test]
    fn production_polymarket_signer_requires_private_key_env() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        std::env::remove_var(PRIVATE_KEY_VAR);

        let error = PolymarketOrderSigner::from_runtime_inputs(
            &sample_source_config(),
            &sample_gateway_credentials(),
        )
        .expect_err("missing env should fail closed");

        assert!(error.to_string().contains(PRIVATE_KEY_VAR));
    }

    #[test]
    fn production_polymarket_signer_builds_real_signed_identity() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        std::env::set_var(PRIVATE_KEY_VAR, TEST_PRIVATE_KEY);
        let server = MockServer::start();
        let token_id = U256::from_str(TEST_TOKEN_ID).expect("token id should parse");

        server.mock(|when, then| {
            when.method(GET)
                .path("/fee-rate")
                .query_param("token_id", token_id.to_string());
            then.status(200).json_body(json!({ "base_fee": 17 }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/tick-size")
                .query_param("token_id", token_id.to_string());
            then.status(200)
                .json_body(json!({ "minimum_tick_size": Decimal::new(1, 2) }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/neg-risk")
                .query_param("token_id", token_id.to_string());
            then.status(200).json_body(json!({ "neg_risk": false }));
        });

        let signer = PolymarketOrderSigner::from_runtime_inputs(
            &sample_source_config_with_host(&server.base_url()),
            &sample_gateway_credentials(),
        )
        .expect("signer should build with private key env");

        let signed = signer
            .sign_family(&sample_single_member_plan())
            .expect("signer should sign family plan");

        assert_eq!(signed.members.len(), 1);
        assert_ne!(signed.members[0].maker, "0xmaker");
        assert_ne!(signed.members[0].signer, "0xsigner");
        assert_eq!(signed.members[0].fee_rate_bps, "17");
        assert!(signed.members[0].identity.signature.starts_with("0x"));
        assert!(signed.members[0]
            .identity
            .signed_order_hash
            .starts_with("0x"));
        assert_ne!(signed.members[0].identity.signature, "test-sig:plan-1:0");
    }

    #[test]
    fn metadata_gateway_backend_defaults_to_sdk() {
        let source = sample_source_config();

        assert_eq!(
            polymarket_metadata_gateway_backend(&source),
            PolymarketMetadataGatewayBackend::Sdk
        );
    }

    #[test]
    fn adapter_submit_provider_uses_gateway_runtime_without_proxy() {
        let server = SingleRequestServer::spawn(
            "200 OK",
            r#"{"success":true,"orderID":"rest-order-1","status":"live","makingAmount":"10","takingAmount":"5","errorMsg":""}"#,
        );
        let submit_calls = Arc::new(AtomicUsize::new(0));
        let provider = sample_adapter_submit_provider(submit_calls.clone());

        let outcome = provider
            .submit_family(&sample_signed_submission(), &sample_attempt())
            .expect("gateway-backed submit should succeed");

        match outcome {
            LiveSubmitOutcome::Accepted { submission_record } => {
                assert_eq!(submission_record.submission_ref, "gateway-order-1");
            }
            other => panic!("unexpected outcome: {other:?}"),
        }

        assert_eq!(submit_calls.load(Ordering::SeqCst), 1);
        server.finish_without_request();
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn sample_source_config() -> PolymarketSourceConfig {
        sample_source_config_with_host("https://clob.polymarket.com")
    }

    fn sample_source_config_with_host(base: &str) -> PolymarketSourceConfig {
        PolymarketSourceConfig {
            clob_host: base.parse().expect("clob host should parse"),
            data_api_host: base.parse().expect("data host should parse"),
            relayer_host: base.parse().expect("relayer host should parse"),
            market_ws_url: "wss://ws-subscriptions-clob.polymarket.com/ws/market"
                .parse()
                .expect("market ws should parse"),
            user_ws_url: "wss://ws-subscriptions-clob.polymarket.com/ws/user"
                .parse()
                .expect("user ws should parse"),
            heartbeat_interval_seconds: 15,
            relayer_poll_interval_seconds: 5,
            metadata_refresh_interval_seconds: 60,
        }
    }

    fn sample_gateway_credentials() -> PolymarketGatewayCredentials {
        PolymarketGatewayCredentials {
            address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266".to_owned(),
            funder_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266".to_owned(),
            signature_type: "eoa".to_owned(),
            wallet_route: "eoa".to_owned(),
            api_key: "00000000-0000-0000-0000-000000000000".to_owned(),
            secret: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_owned(),
            passphrase: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .to_owned(),
        }
    }

    fn sample_single_member_plan() -> ExecutionPlan {
        ExecutionPlan::NegRiskSubmitFamily {
            family_id: "family-a".into(),
            members: vec![NegRiskMemberOrderPlan {
                condition_id: "condition-1".into(),
                token_id: TEST_TOKEN_ID.into(),
                price: Decimal::new(45, 2),
                quantity: Decimal::new(10, 0),
            }],
        }
    }

    fn sample_adapter_submit_provider(
        submit_calls: Arc<AtomicUsize>,
    ) -> GatewayBackedSubmitProvider {
        GatewayBackedSubmitProvider {
            gateway: PolymarketGateway::from_clob_api(Arc::new(RecordingSubmitClobApi {
                submit_calls,
            })),
            runtime: Arc::new(
                tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(1)
                    .enable_all()
                    .build()
                    .expect("submit runtime should build"),
            ),
            transport: sample_post_order_transport(),
        }
    }

    fn sample_attempt() -> ExecutionAttemptContext {
        ExecutionAttemptContext {
            attempt_id: "attempt-1".to_owned(),
            snapshot_id: "snapshot-1".to_owned(),
            execution_mode: ExecutionMode::Live,
            route: "neg-risk".to_owned(),
            scope: "family-a".to_owned(),
            matched_rule_id: None,
        }
    }

    fn sample_signed_submission() -> SignedFamilySubmission {
        SignedFamilySubmission {
            plan_id: "plan-1".to_owned(),
            members: vec![execution::signing::SignedFamilyMember {
                condition_id: domain::ConditionId::from("condition-1"),
                token_id: domain::TokenId::from("token-1"),
                price: Decimal::new(45, 2),
                quantity: Decimal::new(10, 0),
                maker: "0xmaker".to_owned(),
                signer: "0xsigner".to_owned(),
                taker: "0x0000000000000000000000000000000000000000".to_owned(),
                maker_amount: "10".to_owned(),
                taker_amount: "5".to_owned(),
                side: "BUY".to_owned(),
                expiration: "0".to_owned(),
                fee_rate_bps: "30".to_owned(),
                signature_type: 0,
                identity: domain::SignedOrderIdentity {
                    signed_order_hash: "hash-1".to_owned(),
                    salt: "123".to_owned(),
                    nonce: "0".to_owned(),
                    signature: "sig-1".to_owned(),
                },
            }],
        }
    }

    fn sample_post_order_transport() -> PostOrderTransport {
        PostOrderTransport {
            owner: "owner-uuid".to_owned(),
            order_type: OrderType::Gtc,
            defer_exec: false,
        }
    }

    #[derive(Debug)]
    struct RecordingSubmitClobApi {
        submit_calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl PolymarketClobApi for RecordingSubmitClobApi {
        async fn open_orders(
            &self,
            _query: &PolymarketOrderQuery,
        ) -> Result<Vec<PolymarketOpenOrderSummary>, PolymarketGatewayError> {
            Ok(Vec::new())
        }

        async fn submit_order(
            &self,
            _order: &PolymarketSignedOrder,
        ) -> Result<PolymarketSubmitResponse, PolymarketGatewayError> {
            self.submit_calls.fetch_add(1, Ordering::SeqCst);
            Ok(PolymarketSubmitResponse {
                order_id: "gateway-order-1".to_owned(),
                status: "LIVE".to_owned(),
                success: true,
                error_message: None,
                transaction_hashes: Vec::new(),
            })
        }

        async fn post_heartbeat(
            &self,
            _previous_heartbeat_id: Option<&str>,
        ) -> Result<PolymarketHeartbeatStatus, PolymarketGatewayError> {
            Ok(PolymarketHeartbeatStatus {
                heartbeat_id: "hb-1".to_owned(),
                valid: true,
            })
        }
    }

    struct SingleRequestServer {
        request: Arc<Mutex<Option<String>>>,
        join: thread::JoinHandle<()>,
    }

    impl SingleRequestServer {
        fn spawn(status: &'static str, body: &'static str) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
            listener
                .set_nonblocking(true)
                .expect("set listener nonblocking");
            let request = Arc::new(Mutex::new(None));
            let captured = request.clone();
            let deadline = Instant::now() + Duration::from_secs(2);

            let join = thread::spawn(move || loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut buf = [0_u8; 8192];
                        let mut request_text = Vec::new();
                        loop {
                            match stream.read(&mut buf) {
                                Ok(0) => break,
                                Ok(n) => {
                                    request_text.extend_from_slice(&buf[..n]);
                                    if request_text.windows(4).any(|window| window == b"\r\n\r\n") {
                                        break;
                                    }
                                }
                                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                                    thread::sleep(Duration::from_millis(10));
                                }
                                Err(err) => panic!("request read failed: {err}"),
                            }
                        }

                        *captured.lock().expect("request capture lock") =
                            Some(String::from_utf8_lossy(&request_text).into_owned());

                        let response = format!(
                            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        stream
                            .write_all(response.as_bytes())
                            .expect("write response");
                        break;
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        if Instant::now() >= deadline {
                            break;
                        }
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(err) => panic!("accept failed: {err}"),
                }
            });

            Self { request, join }
        }

        fn finish_without_request(self) {
            self.join.join().expect("server thread should finish");
            assert!(self.request.lock().expect("request lock").is_none());
        }
    }
}
