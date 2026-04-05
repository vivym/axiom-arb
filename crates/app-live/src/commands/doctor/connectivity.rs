use std::{collections::BTreeSet, future::Future, pin::Pin, time::Duration};

use config_schema::{AppLiveConfigView, RuntimeModeToml};
use domain::{SignatureType, WalletRoute};
use persistence::connect_pool_from_env;
use venue_polymarket::{
    L2AuthHeaders, PolymarketRestClient, PolymarketWsClient, RelayerAuth, RestClientBuildError,
    SignerContext, WsUserChannelAuth,
};

use crate::{config::PolymarketSourceConfig, LocalRelayerAuth, LocalSignerConfig, ResolvedTargets};

use super::{
    report::{DoctorCheckStatus, DoctorReport},
    DoctorFailure, DoctorLiveContext,
};

const CONNECTIVITY_TIMEOUT: Duration = Duration::from_secs(5);
const HEARTBEAT_PREVIOUS_ID: &str = "doctor-preflight-heartbeat";

type ProbeFuture<'a> = Pin<Box<dyn Future<Output = Result<(), ProbeBackendError>> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConnectivityCheck {
    status: DoctorCheckStatus,
    label: String,
}

impl ConnectivityCheck {
    fn pass(label: impl Into<String>) -> Self {
        Self {
            status: DoctorCheckStatus::Pass,
            label: label.into(),
        }
    }

    fn skip(label: impl Into<String>) -> Self {
        Self {
            status: DoctorCheckStatus::Skip,
            label: label.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProbeBackendError {
    category: &'static str,
    message: String,
}

impl ProbeBackendError {
    fn new(category: &'static str, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct UserWsProbeAuth<'a> {
    api_key: &'a str,
    secret: &'a str,
    passphrase: &'a str,
}

trait ConnectivityProbeBackend {
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

pub(super) fn evaluate(
    config: &AppLiveConfigView<'_>,
    live_context: Option<&DoctorLiveContext>,
    resolved_targets: Option<&ResolvedTargets>,
    report: &mut DoctorReport,
) -> Result<(), DoctorFailure> {
    require_database_url(report)?;

    match config.mode() {
        RuntimeModeToml::Paper => {
            report.push_check(
                "Connectivity",
                DoctorCheckStatus::Skip,
                "live REST, websocket, heartbeat, and relayer probes not required in paper mode",
                "",
            );
            Ok(())
        }
        RuntimeModeToml::Live => evaluate_live(config, live_context, resolved_targets, report),
    }
}

fn evaluate_live(
    config: &AppLiveConfigView<'_>,
    live_context: Option<&DoctorLiveContext>,
    resolved_targets: Option<&ResolvedTargets>,
    report: &mut DoctorReport,
) -> Result<(), DoctorFailure> {
    let live_context = live_context.expect("live context should exist for live doctor probes");
    verify_database_connectivity(live_context, report)?;
    let resolved_targets =
        resolved_targets.expect("resolved startup targets should exist for live doctor probes");
    let source_config: PolymarketSourceConfig =
        PolymarketSourceConfig::try_from(config).map_err(|error| {
            report.push_check(
                "Connectivity",
                DoctorCheckStatus::Fail,
                "ConnectivityError",
                error.to_string(),
            );
            DoctorFailure::new("ConnectivityError", error.to_string())
        })?;
    let signer_config = LocalSignerConfig::try_from(config).map_err(|error| {
        report.push_check(
            "Connectivity",
            DoctorCheckStatus::Fail,
            "ConnectivityError",
            error.to_string(),
        );
        DoctorFailure::new("ConnectivityError", error.to_string())
    })?;
    let user_ws_auth = user_ws_probe_auth_from_config(config);
    let mut backend = RealConnectivityProbeBackend::new(source_config);

    let checks = live_context
        .runtime
        .block_on(run_live_probes(
            &mut backend,
            resolved_targets,
            &signer_config,
            user_ws_auth,
        ))
        .map_err(|error| {
            report.push_check(
                "Connectivity",
                DoctorCheckStatus::Fail,
                error.category,
                error.message.clone(),
            );
            DoctorFailure::new(error.category, error.message)
        })?;

    for check in checks {
        report.push_check("Connectivity", check.status, check.label, "");
    }

    Ok(())
}

fn verify_database_connectivity(
    live_context: &DoctorLiveContext,
    report: &mut DoctorReport,
) -> Result<(), DoctorFailure> {
    live_context
        .runtime
        .block_on(connect_pool_from_env())
        .map(|_| {
            report.push_check(
                "Connectivity",
                DoctorCheckStatus::Pass,
                "database connectivity probe succeeded",
                "",
            );
        })
        .map_err(|error| {
            report.push_check(
                "Connectivity",
                DoctorCheckStatus::Fail,
                "ConnectivityError",
                error.to_string(),
            );
            DoctorFailure::new("ConnectivityError", error.to_string())
        })
}

fn require_database_url(report: &mut DoctorReport) -> Result<(), DoctorFailure> {
    match std::env::var("DATABASE_URL") {
        Ok(_) => {
            report.push_check(
                "Connectivity",
                DoctorCheckStatus::Pass,
                "DATABASE_URL is set",
                "",
            );
            Ok(())
        }
        Err(_) => {
            let message = "DATABASE_URL is not set";
            report.push_check(
                "Connectivity",
                DoctorCheckStatus::Fail,
                "ConnectivityError",
                message,
            );
            Err(DoctorFailure::new("ConnectivityError", message))
        }
    }
}

fn user_ws_probe_auth_from_config<'a>(
    config: &'a AppLiveConfigView<'a>,
) -> Option<UserWsProbeAuth<'a>> {
    config.account().map(|account| UserWsProbeAuth {
        api_key: account.api_key(),
        secret: account.secret(),
        passphrase: account.passphrase(),
    })
}

async fn run_live_probes<B: ConnectivityProbeBackend>(
    backend: &mut B,
    resolved_targets: &ResolvedTargets,
    signer_config: &LocalSignerConfig,
    user_ws_auth: Option<UserWsProbeAuth<'_>>,
) -> Result<Vec<ConnectivityCheck>, ProbeBackendError> {
    let mut checks = Vec::new();

    backend.fetch_open_orders(signer_config).await?;
    checks.push(ConnectivityCheck::pass(
        "authenticated REST probe succeeded",
    ));

    let token_ids = collect_token_ids(resolved_targets);
    if token_ids.is_empty() {
        checks.push(ConnectivityCheck::skip(
            "market websocket probe skipped because resolved startup targets did not contain usable token IDs",
        ));
    } else {
        backend.subscribe_market_assets(&token_ids).await?;
        checks.push(ConnectivityCheck::pass("market websocket probe succeeded"));
    }

    let condition_ids = collect_condition_ids(resolved_targets);
    if condition_ids.is_empty() {
        checks.push(ConnectivityCheck::skip(
            "user websocket probe skipped because resolved startup targets did not contain usable condition IDs",
        ));
    } else if let Some(user_ws_auth) = user_ws_auth {
        backend
            .subscribe_user_markets(user_ws_auth, &condition_ids)
            .await?;
        checks.push(ConnectivityCheck::pass("user websocket probe succeeded"));
    } else {
        checks.push(ConnectivityCheck::skip(
            "user websocket probe skipped because the current config does not expose reusable account credentials",
        ));
    }

    backend.post_order_heartbeat(signer_config).await?;
    checks.push(ConnectivityCheck::pass("heartbeat probe succeeded"));

    backend.fetch_recent_transactions(signer_config).await?;
    checks.push(ConnectivityCheck::pass(
        "relayer reachability probe succeeded",
    ));

    Ok(checks)
}

fn collect_token_ids(resolved_targets: &ResolvedTargets) -> Vec<String> {
    resolved_targets
        .targets
        .targets()
        .values()
        .flat_map(|family| family.members.iter())
        .map(|member| member.token_id.trim())
        .filter(|token_id| !token_id.is_empty())
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn collect_condition_ids(resolved_targets: &ResolvedTargets) -> Vec<String> {
    resolved_targets
        .targets
        .targets()
        .values()
        .flat_map(|family| family.members.iter())
        .map(|member| member.condition_id.trim())
        .filter(|condition_id| !condition_id.is_empty())
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

struct RealConnectivityProbeBackend {
    source_config: PolymarketSourceConfig,
    ws_client: Option<PolymarketWsClient>,
}

impl RealConnectivityProbeBackend {
    fn new(source_config: PolymarketSourceConfig) -> Self {
        Self {
            source_config,
            ws_client: None,
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

    async fn ws_client(&mut self) -> Result<&mut PolymarketWsClient, ProbeBackendError> {
        if self.ws_client.is_none() {
            let market_ws_url = self.source_config.market_ws_url.clone();
            let user_ws_url = self.source_config.user_ws_url.clone();
            let outbound_proxy_url = self.source_config.outbound_proxy_url.clone();
            let client = timeout_probe("websocket connection", async move {
                PolymarketWsClient::connect_with_proxy(
                    market_ws_url,
                    user_ws_url,
                    outbound_proxy_url,
                )
                .await
                .map_err(|error| ProbeBackendError::new("ConnectivityError", error.to_string()))
            })
            .await?;
            self.ws_client = Some(client);
        }

        Ok(self
            .ws_client
            .as_mut()
            .expect("ws client should be initialized"))
    }
}

impl ConnectivityProbeBackend for RealConnectivityProbeBackend {
    fn fetch_open_orders<'a>(
        &'a mut self,
        signer_config: &'a LocalSignerConfig,
    ) -> ProbeFuture<'a> {
        Box::pin(async move {
            let rest = self
                .rest_client()
                .map_err(|error| ProbeBackendError::new("ConnectivityError", error.to_string()))?;
            let auth = l2_auth_headers_from_signer_config(signer_config)?;
            timeout_probe("authenticated REST", async move {
                rest.fetch_open_orders(&auth)
                    .await
                    .map(|_| ())
                    .map_err(|error| ProbeBackendError::new("ConnectivityError", error.to_string()))
            })
            .await
        })
    }

    fn subscribe_market_assets<'a>(&'a mut self, token_ids: &'a [String]) -> ProbeFuture<'a> {
        Box::pin(async move {
            let client = self.ws_client().await?;
            timeout_probe("market websocket", async {
                client
                    .subscribe_market_assets(token_ids, false)
                    .await
                    .map_err(|error| {
                        ProbeBackendError::new("ConnectivityError", error.to_string())
                    })?;
                client
                    .next_market_event()
                    .await
                    .map(|_| ())
                    .map_err(|error| ProbeBackendError::new("ConnectivityError", error.to_string()))
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
            let client = self.ws_client().await?;
            timeout_probe("user websocket", async {
                client
                    .subscribe_user_markets(
                        &WsUserChannelAuth {
                            api_key: auth.api_key,
                            secret: auth.secret,
                            passphrase: auth.passphrase,
                        },
                        condition_ids,
                    )
                    .await
                    .map_err(|error| {
                        ProbeBackendError::new("ConnectivityError", error.to_string())
                    })?;
                client
                    .next_user_event()
                    .await
                    .map(|_| ())
                    .map_err(|error| ProbeBackendError::new("ConnectivityError", error.to_string()))
            })
            .await
        })
    }

    fn post_order_heartbeat<'a>(
        &'a mut self,
        signer_config: &'a LocalSignerConfig,
    ) -> ProbeFuture<'a> {
        Box::pin(async move {
            let rest = self
                .rest_client()
                .map_err(|error| ProbeBackendError::new("ConnectivityError", error.to_string()))?;
            let auth = l2_auth_headers_from_signer_config(signer_config)?;
            timeout_probe("heartbeat", async move {
                rest.post_order_heartbeat(&auth, HEARTBEAT_PREVIOUS_ID)
                    .await
                    .map(|_| ())
                    .map_err(|error| ProbeBackendError::new("ConnectivityError", error.to_string()))
            })
            .await
        })
    }

    fn fetch_recent_transactions<'a>(
        &'a mut self,
        signer_config: &'a LocalSignerConfig,
    ) -> ProbeFuture<'a> {
        Box::pin(async move {
            let rest = self
                .rest_client()
                .map_err(|error| ProbeBackendError::new("ConnectivityError", error.to_string()))?;
            let auth = relayer_auth_from_signer_config(signer_config)?;
            timeout_probe("relayer reachability", async move {
                rest.fetch_recent_transactions(&auth)
                    .await
                    .map(|_| ())
                    .map_err(|error| ProbeBackendError::new("ConnectivityError", error.to_string()))
            })
            .await
        })
    }
}

async fn timeout_probe<F, T>(label: &str, future: F) -> Result<T, ProbeBackendError>
where
    F: Future<Output = Result<T, ProbeBackendError>>,
{
    tokio::time::timeout(CONNECTIVITY_TIMEOUT, future)
        .await
        .map_err(|_| {
            ProbeBackendError::new(
                "ConnectivityError",
                format!(
                    "{label} probe timed out after {}s",
                    CONNECTIVITY_TIMEOUT.as_secs()
                ),
            )
        })?
}

fn l2_auth_headers_from_signer_config<'a>(
    signer_config: &'a LocalSignerConfig,
) -> Result<L2AuthHeaders<'a>, ProbeBackendError> {
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
) -> Result<RelayerAuth<'a>, ProbeBackendError> {
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

fn parse_signature_type(label: &str) -> Result<SignatureType, ProbeBackendError> {
    match label.trim().to_ascii_lowercase().as_str() {
        "eoa" => Ok(SignatureType::Eoa),
        "proxy" | "poly_proxy" => Ok(SignatureType::Proxy),
        "safe" | "gnosis_safe" => Ok(SignatureType::Safe),
        other => Err(ProbeBackendError::new(
            "ConnectivityError",
            format!("unsupported signature type label {other}"),
        )),
    }
}

fn parse_wallet_route(label: &str) -> Result<WalletRoute, ProbeBackendError> {
    match label.trim().to_ascii_lowercase().as_str() {
        "eoa" => Ok(WalletRoute::Eoa),
        "proxy" => Ok(WalletRoute::Proxy),
        "safe" => Ok(WalletRoute::Safe),
        other => Err(ProbeBackendError::new(
            "ConnectivityError",
            format!("unsupported wallet route label {other}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use rust_decimal::Decimal;

    use crate::{
        LocalL2AuthHeaders, LocalRelayerAuth, LocalSignerIdentity, NegRiskFamilyLiveTarget,
        NegRiskLiveTargetSet, NegRiskMemberLiveTarget,
    };

    use super::*;

    #[tokio::test]
    async fn connectivity_probe_uses_resolved_target_ids_for_ws_scope() {
        let mut backend = FakeBackend::default();
        let resolved_targets =
            sample_resolved_targets(vec![("condition-2", "token-2"), ("condition-1", "token-1")]);

        let checks = run_live_probes(
            &mut backend,
            &resolved_targets,
            &sample_signer_config(),
            Some(sample_user_ws_auth()),
        )
        .await
        .expect("probe execution should succeed");

        assert!(checks
            .iter()
            .all(|check| check.status != DoctorCheckStatus::Skip));
        assert_eq!(
            backend.calls,
            vec![
                ProbeCall::FetchOpenOrders,
                ProbeCall::SubscribeMarketAssets(vec!["token-1".to_owned(), "token-2".to_owned()]),
                ProbeCall::SubscribeUserMarkets(vec![
                    "condition-1".to_owned(),
                    "condition-2".to_owned(),
                ]),
                ProbeCall::PostOrderHeartbeat,
                ProbeCall::FetchRecentTransactions,
            ]
        );
    }

    #[tokio::test]
    async fn no_usable_token_or_condition_ids_causes_ws_probes_to_skip() {
        let mut backend = FakeBackend::default();
        let resolved_targets = sample_resolved_targets(vec![("", ""), ("   ", "   ")]);

        let checks = run_live_probes(
            &mut backend,
            &resolved_targets,
            &sample_signer_config(),
            Some(sample_user_ws_auth()),
        )
        .await
        .expect("probe execution should succeed");

        assert_eq!(
            checks
                .iter()
                .filter(|check| check.status == DoctorCheckStatus::Skip)
                .count(),
            2
        );
        assert_eq!(
            backend.calls,
            vec![
                ProbeCall::FetchOpenOrders,
                ProbeCall::PostOrderHeartbeat,
                ProbeCall::FetchRecentTransactions,
            ]
        );
    }

    #[tokio::test]
    async fn heartbeat_probe_uses_the_allowed_request_path() {
        let mut backend = FakeBackend::default();

        run_live_probes(
            &mut backend,
            &sample_resolved_targets(vec![("condition-1", "token-1")]),
            &sample_signer_config(),
            Some(sample_user_ws_auth()),
        )
        .await
        .expect("probe execution should succeed");

        assert!(backend.calls.contains(&ProbeCall::PostOrderHeartbeat));
    }

    #[tokio::test]
    async fn relayer_probe_does_not_call_submit_like_logic() {
        let mut backend = FakeBackend::default();

        run_live_probes(
            &mut backend,
            &sample_resolved_targets(vec![("condition-1", "token-1")]),
            &sample_signer_config(),
            Some(sample_user_ws_auth()),
        )
        .await
        .expect("probe execution should succeed");

        assert!(!backend.calls.contains(&ProbeCall::SubmitLike));
        assert!(backend.calls.contains(&ProbeCall::FetchRecentTransactions));
    }

    #[tokio::test]
    async fn missing_reusable_account_secret_skips_only_the_user_ws_probe() {
        let mut backend = FakeBackend::default();

        let checks = run_live_probes(
            &mut backend,
            &sample_resolved_targets(vec![("condition-1", "token-1")]),
            &sample_signer_config(),
            None,
        )
        .await
        .expect("probe execution should succeed");

        assert_eq!(
            checks
                .iter()
                .filter(|check| check.status == DoctorCheckStatus::Skip)
                .count(),
            1
        );
        assert_eq!(
            backend.calls,
            vec![
                ProbeCall::FetchOpenOrders,
                ProbeCall::SubscribeMarketAssets(vec!["token-1".to_owned()]),
                ProbeCall::PostOrderHeartbeat,
                ProbeCall::FetchRecentTransactions,
            ]
        );
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum ProbeCall {
        FetchOpenOrders,
        SubscribeMarketAssets(Vec<String>),
        SubscribeUserMarkets(Vec<String>),
        PostOrderHeartbeat,
        FetchRecentTransactions,
        SubmitLike,
    }

    #[derive(Default)]
    struct FakeBackend {
        calls: Vec<ProbeCall>,
    }

    impl ConnectivityProbeBackend for FakeBackend {
        fn fetch_open_orders<'a>(
            &'a mut self,
            _signer_config: &'a LocalSignerConfig,
        ) -> ProbeFuture<'a> {
            self.calls.push(ProbeCall::FetchOpenOrders);
            Box::pin(async { Ok(()) })
        }

        fn subscribe_market_assets<'a>(&'a mut self, token_ids: &'a [String]) -> ProbeFuture<'a> {
            self.calls
                .push(ProbeCall::SubscribeMarketAssets(token_ids.to_vec()));
            Box::pin(async { Ok(()) })
        }

        fn subscribe_user_markets<'a>(
            &'a mut self,
            _auth: UserWsProbeAuth<'a>,
            condition_ids: &'a [String],
        ) -> ProbeFuture<'a> {
            self.calls
                .push(ProbeCall::SubscribeUserMarkets(condition_ids.to_vec()));
            Box::pin(async { Ok(()) })
        }

        fn post_order_heartbeat<'a>(
            &'a mut self,
            _signer_config: &'a LocalSignerConfig,
        ) -> ProbeFuture<'a> {
            self.calls.push(ProbeCall::PostOrderHeartbeat);
            Box::pin(async { Ok(()) })
        }

        fn fetch_recent_transactions<'a>(
            &'a mut self,
            _signer_config: &'a LocalSignerConfig,
        ) -> ProbeFuture<'a> {
            self.calls.push(ProbeCall::FetchRecentTransactions);
            Box::pin(async { Ok(()) })
        }
    }

    fn sample_resolved_targets(members: Vec<(&str, &str)>) -> ResolvedTargets {
        let members = members
            .into_iter()
            .map(|(condition_id, token_id)| NegRiskMemberLiveTarget {
                condition_id: condition_id.to_owned(),
                token_id: token_id.to_owned(),
                price: Decimal::new(43, 2),
                quantity: Decimal::new(5, 0),
            })
            .collect();
        let targets = BTreeMap::from([(
            "family-a".to_owned(),
            NegRiskFamilyLiveTarget {
                family_id: "family-a".to_owned(),
                members,
            },
        )]);

        ResolvedTargets {
            targets: NegRiskLiveTargetSet::from_targets_with_revision("targets-rev-9", targets),
            operator_target_revision: Some("targets-rev-9".to_owned()),
        }
    }

    fn sample_signer_config() -> LocalSignerConfig {
        LocalSignerConfig {
            signer: LocalSignerIdentity {
                address: "0x1111111111111111111111111111111111111111".to_owned(),
                funder_address: "0x2222222222222222222222222222222222222222".to_owned(),
                signature_type: "eoa".to_owned(),
                wallet_route: "eoa".to_owned(),
            },
            l2_auth: LocalL2AuthHeaders {
                api_key: "poly-api-key".to_owned(),
                passphrase: "poly-passphrase".to_owned(),
                timestamp: "1700000000".to_owned(),
                signature: "poly-signature".to_owned(),
            },
            relayer_auth: LocalRelayerAuth::RelayerApiKey {
                api_key: "relay-key".to_owned(),
                address: "0x1111111111111111111111111111111111111111".to_owned(),
            },
        }
    }

    fn sample_user_ws_auth() -> UserWsProbeAuth<'static> {
        UserWsProbeAuth {
            api_key: "poly-api-key",
            secret: "poly-secret",
            passphrase: "poly-passphrase",
        }
    }
}
