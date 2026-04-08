use std::collections::BTreeSet;

use config_schema::{AppLiveConfigView, RuntimeModeToml};
use persistence::connect_pool_from_env;
use venue_polymarket::PolymarketL2ProbeCredentials;

use crate::{
    config::{PolymarketGatewayCredentials, PolymarketSourceConfig},
    polymarket_probe::{
        LivePolymarketProbe, PolymarketProbeError, PolymarketProbeFacade, UserWsProbeAuth,
    },
    LocalRelayerRuntimeConfig, ResolvedTargets,
};

use super::{
    report::{DoctorCheckStatus, DoctorReport},
    DoctorFailure, DoctorLiveContext,
};

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
    let l2_probe_credentials = PolymarketGatewayCredentials::try_from(config)
        .map(|credentials| PolymarketL2ProbeCredentials {
            address: credentials.address,
            api_key: credentials.api_key,
            secret: credentials.secret,
            passphrase: credentials.passphrase,
        })
        .map_err(|error| {
            report.push_check(
                "Connectivity",
                DoctorCheckStatus::Fail,
                "ConnectivityError",
                error.to_string(),
            );
            DoctorFailure::new("ConnectivityError", error.to_string())
        })?;
    let relayer_runtime_config = if wallet_kind_requires_relayer(config) {
        Some(
            LocalRelayerRuntimeConfig::required_from(config).map_err(|error| {
                report.push_check(
                    "Connectivity",
                    DoctorCheckStatus::Fail,
                    "ConnectivityError",
                    error.to_string(),
                );
                DoctorFailure::new("ConnectivityError", error.to_string())
            })?,
        )
    } else {
        None
    };
    let user_ws_auth = user_ws_probe_auth_from_config(config);
    let mut backend = LivePolymarketProbe::new(source_config);

    let checks = live_context
        .runtime
        .block_on(run_live_probes(
            &mut backend,
            resolved_targets,
            &l2_probe_credentials,
            relayer_runtime_config.as_ref(),
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
        address: account.address(),
        api_key: account.api_key(),
        secret: account.secret(),
        passphrase: account.passphrase(),
    })
}

fn wallet_kind_requires_relayer(config: &AppLiveConfigView<'_>) -> bool {
    config
        .account()
        .map(|account| account.signature_type_label() != "Eoa")
        .unwrap_or(false)
}

async fn run_live_probes<B: PolymarketProbeFacade>(
    backend: &mut B,
    resolved_targets: &ResolvedTargets,
    l2_probe_credentials: &PolymarketL2ProbeCredentials,
    relayer_runtime_config: Option<&LocalRelayerRuntimeConfig>,
    user_ws_auth: Option<UserWsProbeAuth<'_>>,
) -> Result<Vec<ConnectivityCheck>, PolymarketProbeError> {
    let mut checks = Vec::new();

    backend.fetch_open_orders(l2_probe_credentials).await?;
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

    backend.post_order_heartbeat(l2_probe_credentials).await?;
    checks.push(ConnectivityCheck::pass("heartbeat probe succeeded"));

    if let Some(relayer_runtime_config) = relayer_runtime_config {
        backend
            .fetch_recent_transactions(relayer_runtime_config)
            .await?;
        checks.push(ConnectivityCheck::pass(
            "relayer reachability probe succeeded",
        ));
    }

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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use rust_decimal::Decimal;

    use crate::polymarket_probe::ProbeFuture;
    use crate::{
        LocalRelayerAuth, NegRiskFamilyLiveTarget, NegRiskLiveTargetSet, NegRiskMemberLiveTarget,
    };
    use venue_polymarket::PolymarketL2ProbeCredentials;

    use super::*;

    #[tokio::test]
    async fn connectivity_probe_uses_resolved_target_ids_for_ws_scope() {
        let mut backend = FakeBackend::default();
        let resolved_targets =
            sample_resolved_targets(vec![("condition-2", "token-2"), ("condition-1", "token-1")]);

        let checks = run_live_probes(
            &mut backend,
            &resolved_targets,
            &sample_l2_probe_credentials(),
            Some(&sample_relayer_runtime_config()),
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
                ProbeCall::FetchOpenOrders(sample_l2_probe_credentials()),
                ProbeCall::SubscribeMarketAssets(vec!["token-1".to_owned(), "token-2".to_owned()]),
                ProbeCall::SubscribeUserMarkets(vec![
                    "condition-1".to_owned(),
                    "condition-2".to_owned(),
                ]),
                ProbeCall::PostOrderHeartbeat(sample_l2_probe_credentials()),
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
            &sample_l2_probe_credentials(),
            Some(&sample_relayer_runtime_config()),
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
                ProbeCall::FetchOpenOrders(sample_l2_probe_credentials()),
                ProbeCall::PostOrderHeartbeat(sample_l2_probe_credentials()),
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
            &sample_l2_probe_credentials(),
            Some(&sample_relayer_runtime_config()),
            Some(sample_user_ws_auth()),
        )
        .await
        .expect("probe execution should succeed");

        assert!(backend
            .calls
            .contains(&ProbeCall::PostOrderHeartbeat(sample_l2_probe_credentials())));
    }

    #[tokio::test]
    async fn relayer_probe_does_not_call_submit_like_logic() {
        let mut backend = FakeBackend::default();

        run_live_probes(
            &mut backend,
            &sample_resolved_targets(vec![("condition-1", "token-1")]),
            &sample_l2_probe_credentials(),
            Some(&sample_relayer_runtime_config()),
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
            &sample_l2_probe_credentials(),
            Some(&sample_relayer_runtime_config()),
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
                ProbeCall::FetchOpenOrders(sample_l2_probe_credentials()),
                ProbeCall::SubscribeMarketAssets(vec!["token-1".to_owned()]),
                ProbeCall::PostOrderHeartbeat(sample_l2_probe_credentials()),
                ProbeCall::FetchRecentTransactions,
            ]
        );
    }

    #[tokio::test]
    async fn eoa_live_probe_set_skips_relayer_reachability() {
        let mut backend = FakeBackend::default();

        let checks = run_live_probes(
            &mut backend,
            &sample_resolved_targets(vec![("condition-1", "token-1")]),
            &sample_l2_probe_credentials(),
            None,
            Some(sample_user_ws_auth()),
        )
        .await
        .expect("probe execution should succeed");

        assert!(checks
            .iter()
            .all(|check| check.label != "relayer reachability probe succeeded"));
        assert!(!backend.calls.contains(&ProbeCall::FetchRecentTransactions));
    }

    #[tokio::test]
    async fn run_live_probes_passes_full_account_credentials_to_user_ws_probe() {
        let mut backend = FakeBackend::default();

        run_live_probes(
            &mut backend,
            &sample_resolved_targets(vec![("condition-1", "token-1")]),
            &sample_l2_probe_credentials(),
            Some(&sample_relayer_runtime_config()),
            Some(sample_user_ws_auth()),
        )
        .await
        .expect("probe execution should succeed");

        assert_eq!(
            backend.user_ws_auth,
            Some(RecordedUserWsAuth {
                address: "0x1111111111111111111111111111111111111111".to_owned(),
                api_key: "poly-api-key".to_owned(),
                secret: "poly-secret".to_owned(),
                passphrase: "poly-passphrase".to_owned(),
            })
        );
    }

    #[tokio::test]
    async fn connectivity_probe_passes_only_narrow_l2_credentials_to_clob_checks() {
        let mut backend = FakeBackend::default();
        let resolved_targets = sample_resolved_targets(vec![("condition-1", "token-1")]);
        let l2_credentials = sample_l2_probe_credentials();

        run_live_probes(
            &mut backend,
            &resolved_targets,
            &l2_credentials,
            Some(&sample_relayer_runtime_config()),
            Some(sample_user_ws_auth()),
        )
        .await
        .expect("probe execution should succeed");

        assert_eq!(
            backend.l2_probe_credentials,
            Some(l2_credentials),
            "doctor CLOB probes should be driven by the narrow L2 credential DTO"
        );
    }

    #[tokio::test]
    async fn run_live_probes_propagates_facade_error_category_and_message() {
        let mut backend = FailingBackend;

        let error = run_live_probes(
            &mut backend,
            &sample_resolved_targets(vec![("condition-1", "token-1")]),
            &sample_l2_probe_credentials(),
            Some(&sample_relayer_runtime_config()),
            Some(sample_user_ws_auth()),
        )
        .await
        .expect_err("probe execution should fail");

        assert_eq!(error.category, "ConnectivityError");
        assert!(error.message.contains("forced failure"));
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum ProbeCall {
        FetchOpenOrders(PolymarketL2ProbeCredentials),
        SubscribeMarketAssets(Vec<String>),
        SubscribeUserMarkets(Vec<String>),
        PostOrderHeartbeat(PolymarketL2ProbeCredentials),
        FetchRecentTransactions,
        SubmitLike,
    }

    #[derive(Default)]
    struct FakeBackend {
        calls: Vec<ProbeCall>,
        user_ws_auth: Option<RecordedUserWsAuth>,
        l2_probe_credentials: Option<PolymarketL2ProbeCredentials>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct RecordedUserWsAuth {
        address: String,
        api_key: String,
        secret: String,
        passphrase: String,
    }

    impl PolymarketProbeFacade for FakeBackend {
        fn fetch_open_orders<'a>(
            &'a mut self,
            l2_probe_credentials: &'a PolymarketL2ProbeCredentials,
        ) -> ProbeFuture<'a> {
            self.l2_probe_credentials = Some(l2_probe_credentials.clone());
            self.calls
                .push(ProbeCall::FetchOpenOrders(l2_probe_credentials.clone()));
            Box::pin(async { Ok(()) })
        }

        fn subscribe_market_assets<'a>(&'a mut self, token_ids: &'a [String]) -> ProbeFuture<'a> {
            self.calls
                .push(ProbeCall::SubscribeMarketAssets(token_ids.to_vec()));
            Box::pin(async { Ok(()) })
        }

        fn subscribe_user_markets<'a>(
            &'a mut self,
            auth: UserWsProbeAuth<'a>,
            condition_ids: &'a [String],
        ) -> ProbeFuture<'a> {
            self.user_ws_auth = Some(RecordedUserWsAuth {
                address: auth.address.to_owned(),
                api_key: auth.api_key.to_owned(),
                secret: auth.secret.to_owned(),
                passphrase: auth.passphrase.to_owned(),
            });
            self.calls
                .push(ProbeCall::SubscribeUserMarkets(condition_ids.to_vec()));
            Box::pin(async { Ok(()) })
        }

        fn post_order_heartbeat<'a>(
            &'a mut self,
            l2_probe_credentials: &'a PolymarketL2ProbeCredentials,
        ) -> ProbeFuture<'a> {
            self.l2_probe_credentials = Some(l2_probe_credentials.clone());
            self.calls
                .push(ProbeCall::PostOrderHeartbeat(l2_probe_credentials.clone()));
            Box::pin(async { Ok(()) })
        }

        fn fetch_recent_transactions<'a>(
            &'a mut self,
            _relayer_runtime_config: &'a LocalRelayerRuntimeConfig,
        ) -> ProbeFuture<'a> {
            self.calls.push(ProbeCall::FetchRecentTransactions);
            Box::pin(async { Ok(()) })
        }
    }

    struct FailingBackend;

    impl PolymarketProbeFacade for FailingBackend {
        fn fetch_open_orders<'a>(
            &'a mut self,
            _l2_probe_credentials: &'a PolymarketL2ProbeCredentials,
        ) -> ProbeFuture<'a> {
            Box::pin(async {
                Err(PolymarketProbeError::new(
                    "ConnectivityError",
                    "forced failure",
                ))
            })
        }

        fn subscribe_market_assets<'a>(&'a mut self, _token_ids: &'a [String]) -> ProbeFuture<'a> {
            Box::pin(async { Ok(()) })
        }

        fn subscribe_user_markets<'a>(
            &'a mut self,
            _auth: UserWsProbeAuth<'a>,
            _condition_ids: &'a [String],
        ) -> ProbeFuture<'a> {
            Box::pin(async { Ok(()) })
        }

        fn post_order_heartbeat<'a>(
            &'a mut self,
            _l2_probe_credentials: &'a PolymarketL2ProbeCredentials,
        ) -> ProbeFuture<'a> {
            Box::pin(async { Ok(()) })
        }

        fn fetch_recent_transactions<'a>(
            &'a mut self,
            _relayer_runtime_config: &'a LocalRelayerRuntimeConfig,
        ) -> ProbeFuture<'a> {
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

    fn sample_relayer_runtime_config() -> LocalRelayerRuntimeConfig {
        LocalRelayerRuntimeConfig {
            auth: LocalRelayerAuth::RelayerApiKey {
                api_key: "relay-key".to_owned(),
                address: "0x1111111111111111111111111111111111111111".to_owned(),
            },
        }
    }

    fn sample_l2_probe_credentials() -> PolymarketL2ProbeCredentials {
        PolymarketL2ProbeCredentials {
            address: "0x1111111111111111111111111111111111111111".to_owned(),
            api_key: "poly-api-key".to_owned(),
            secret: "poly-secret".to_owned(),
            passphrase: "poly-passphrase".to_owned(),
        }
    }

    fn sample_user_ws_auth() -> UserWsProbeAuth<'static> {
        UserWsProbeAuth {
            address: "0x1111111111111111111111111111111111111111",
            api_key: "poly-api-key",
            secret: "poly-secret",
            passphrase: "poly-passphrase",
        }
    }
}
