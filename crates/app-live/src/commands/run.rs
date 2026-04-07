use std::{collections::BTreeSet, error::Error, path::Path};

use config_schema::{load_raw_config_from_path, RuntimeModeToml, ValidatedConfig};
use observability::{bootstrap_observability, span_names};
use persistence::connect_pool_from_env;

use crate::cli::RunArgs;
use crate::config::{neg_risk_live_targets_from_route_artifacts, PolymarketSourceConfig};
use crate::daemon::run_live_daemon_from_durable_store_with_strategy_revision_and_session_instrumented;
use crate::negrisk_live::NegRiskLiveExecutionBackend;
use crate::polymarket_runtime_adapter::PolymarketLiveExecutionBackend;
use crate::{
    build_real_user_shadow_smoke_sources, instrumentation::emit_bootstrap_completion_observability,
    load_real_user_shadow_smoke_config, run_paper_instrumented, run_session::RunSessionHandle,
    startup::resolve_startup_strategy_revision, AppInstrumentation, ConfigError, LocalSignerConfig,
    NegRiskLiveTargetSet, PolymarketGatewayCredentials, SmokeSafeStartupSource,
    StaticSnapshotSource,
};

pub fn execute(args: RunArgs) -> Result<(), Box<dyn Error>> {
    run_from_config_path(&args.config)
}

pub(crate) fn run_from_config_path(config_path: &Path) -> Result<(), Box<dyn Error>> {
    run_from_config_path_for_source(config_path, RunInvocationSource::Run)
}

pub(crate) fn run_from_config_path_with_invoked_by(
    config_path: &Path,
    invoked_by: &'static str,
) -> Result<(), Box<dyn Error>> {
    run_from_config_path_for_source(config_path, RunInvocationSource::from_label(invoked_by)?)
}

fn run_from_config_path_for_source(
    config_path: &Path,
    invoked_by: RunInvocationSource,
) -> Result<(), Box<dyn Error>> {
    let observability = bootstrap_observability("app-live");
    let bootstrap_span = tracing::info_span!(span_names::APP_BOOTSTRAP);
    let _bootstrap_guard = bootstrap_span.enter();
    let raw = load_raw_config_from_path(config_path)?;
    let validated = ValidatedConfig::new(raw)?;
    let config = validated.for_app_live()?;
    let real_user_shadow_smoke = load_real_user_shadow_smoke_config(&config)?;
    require_database_url_env()?;
    let run_session = RunSessionHandle::create_starting(config_path, &config, invoked_by.as_str())?;
    let instrumentation = AppInstrumentation::enabled(observability.recorder());
    let result = (|| -> Result<crate::runtime::AppRunResult, Box<dyn Error>> {
        match config.mode() {
            RuntimeModeToml::Paper => {
                let source = StaticSnapshotSource::empty();
                Ok(run_paper_instrumented(&source, instrumentation.clone()))
            }
            RuntimeModeToml::Live => {
                let resolved_strategy = load_resolved_strategy_revision_from_config(&config)?;
                let allow_legacy_target_source_resume = config
                    .target_source()
                    .map(|source| source.is_adopted())
                    .unwrap_or(false);
                let neg_risk_live_targets = neg_risk_live_targets_from_route_artifacts(
                    &resolved_strategy.route_artifacts,
                    resolved_strategy.operator_strategy_revision.as_deref(),
                )?;
                let neg_risk_live_approved_families = rollout_approved_families(&config);
                let neg_risk_live_ready_families = rollout_ready_families(&config);
                let signer_config = if real_user_shadow_smoke.is_some()
                    || live_neg_risk_work_requested(
                        &neg_risk_live_targets,
                        &neg_risk_live_approved_families,
                        &neg_risk_live_ready_families,
                    ) {
                    Some(LocalSignerConfig::try_from(&config)?)
                } else {
                    None
                };
                let live_execution_backend = build_live_neg_risk_execution_backend(
                    &config,
                    real_user_shadow_smoke.is_some(),
                    &neg_risk_live_targets,
                    &neg_risk_live_approved_families,
                    &neg_risk_live_ready_families,
                )?;
                let source = match real_user_shadow_smoke.as_ref() {
                    Some(smoke) => SmokeSafeStartupSource::RealUserShadowSmoke(Box::new(
                        build_real_user_shadow_smoke_sources(
                            smoke.source_config.clone(),
                            signer_config
                                .clone()
                                .expect("smoke startup should require signer config"),
                            run_session.run_session_id(),
                        )
                        .map_err(|error| {
                            ConfigError::InvalidPolymarketSourceConfig {
                                value: config_path.display().to_string(),
                                message: error,
                            }
                        })?,
                    )),
                    None => SmokeSafeStartupSource::Static(StaticSnapshotSource::empty()),
                };

                run_live_daemon_from_durable_store_with_strategy_revision_and_session_instrumented(
                    &source,
                    instrumentation,
                    resolved_strategy.operator_strategy_revision.as_deref(),
                    allow_legacy_target_source_resume,
                    neg_risk_live_targets,
                    neg_risk_live_approved_families,
                    neg_risk_live_ready_families,
                    live_execution_backend,
                    real_user_shadow_smoke,
                    Some(run_session.run_session_id()),
                )
            }
        }
    })();

    match result {
        Ok(result) => {
            run_session.mark_running()?;
            run_session.refresh_last_seen()?;
            run_session.mark_exited()?;
            emit_bootstrap_completion_observability(&observability.recorder(), &result);
            Ok(())
        }
        Err(error) => {
            run_session.mark_failed(&error.to_string())?;
            Err(error)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunInvocationSource {
    Run,
    Bootstrap,
    Apply,
}

impl RunInvocationSource {
    fn from_label(label: &'static str) -> Result<Self, Box<dyn Error>> {
        match label {
            "run" => Ok(Self::Run),
            "bootstrap" => Ok(Self::Bootstrap),
            "apply" => Ok(Self::Apply),
            other => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unsupported run invocation source: {other}"),
            )
            .into()),
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Bootstrap => "bootstrap",
            Self::Apply => "apply",
        }
    }
}

fn require_database_url_env() -> Result<(), Box<dyn Error>> {
    std::env::var("DATABASE_URL")
        .map(|_| ())
        .map_err(|_| persistence::PersistenceError::MissingDatabaseUrl.into())
}

fn load_resolved_strategy_revision_from_config(
    config: &config_schema::AppLiveConfigView<'_>,
) -> Result<crate::startup::ResolvedStrategyRevision, Box<dyn Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    Ok(runtime.block_on(async {
        let pool = connect_pool_from_env().await?;
        resolve_startup_strategy_revision(&pool, config).await
    })?)
}

fn rollout_approved_families(config: &config_schema::AppLiveConfigView<'_>) -> BTreeSet<String> {
    config
        .negrisk_rollout()
        .map(|rollout| rollout.approved_families().iter().cloned().collect())
        .unwrap_or_default()
}

fn rollout_ready_families(config: &config_schema::AppLiveConfigView<'_>) -> BTreeSet<String> {
    config
        .negrisk_rollout()
        .map(|rollout| rollout.ready_families().iter().cloned().collect())
        .unwrap_or_default()
}

fn live_neg_risk_work_requested(
    targets: &NegRiskLiveTargetSet,
    approved_families: &BTreeSet<String>,
    ready_families: &BTreeSet<String>,
) -> bool {
    targets.targets().keys().any(|family_id| {
        approved_families.contains(family_id) && ready_families.contains(family_id)
    })
}

fn build_live_neg_risk_execution_backend(
    config: &config_schema::AppLiveConfigView<'_>,
    real_user_shadow_smoke: bool,
    targets: &NegRiskLiveTargetSet,
    approved_families: &BTreeSet<String>,
    ready_families: &BTreeSet<String>,
) -> Result<Option<Box<dyn NegRiskLiveExecutionBackend>>, Box<dyn Error>> {
    if real_user_shadow_smoke
        || !live_neg_risk_work_requested(targets, approved_families, ready_families)
    {
        return Ok(None);
    }

    let source_config = PolymarketSourceConfig::try_from(config)?;
    let signer_config = LocalSignerConfig::try_from(config)?;
    let credentials = PolymarketGatewayCredentials::try_from(config)?;
    let backend = PolymarketLiveExecutionBackend::from_runtime_inputs(
        &source_config,
        &signer_config,
        &credentials,
    )?;
    Ok(Some(Box::new(backend)))
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        sync::{Mutex, OnceLock},
    };

    use config_schema::{load_raw_config_from_str, ValidatedConfig};
    use polymarket_client_sdk::PRIVATE_KEY_VAR;
    use rust_decimal::Decimal;

    use super::build_live_neg_risk_execution_backend;
    use crate::{
        config::{NegRiskFamilyLiveTarget, NegRiskLiveTargetSet, NegRiskMemberLiveTarget},
        load_real_user_shadow_smoke_config,
    };

    #[test]
    fn live_neg_risk_backend_requires_private_key_for_real_live_work() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        std::env::remove_var(PRIVATE_KEY_VAR);

        let config = live_view(false);
        let smoke = load_real_user_shadow_smoke_config(&config).expect("smoke config should load");
        let error = build_live_neg_risk_execution_backend(
            &config,
            smoke.is_some(),
            &sample_live_targets(),
            &sample_rollout_families(),
            &sample_rollout_families(),
        )
        .err()
        .expect("missing private key should fail closed for real live work");

        assert!(error.to_string().contains(PRIVATE_KEY_VAR));
    }

    #[test]
    fn live_neg_risk_backend_is_skipped_for_real_user_shadow_smoke() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        std::env::remove_var(PRIVATE_KEY_VAR);

        let config = live_view(true);
        let smoke = load_real_user_shadow_smoke_config(&config).expect("smoke config should load");
        let backend = build_live_neg_risk_execution_backend(
            &config,
            smoke.is_some(),
            &sample_live_targets(),
            &sample_rollout_families(),
            &sample_rollout_families(),
        )
        .expect("shadow smoke should not require a live execution backend");

        assert!(backend.is_none());
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn sample_live_targets() -> NegRiskLiveTargetSet {
        NegRiskLiveTargetSet::from_targets_with_revision(
            "rev-1",
            BTreeMap::from([(
                "family-a".to_owned(),
                NegRiskFamilyLiveTarget {
                    family_id: "family-a".to_owned(),
                    members: vec![NegRiskMemberLiveTarget {
                        condition_id: "condition-a".to_owned(),
                        token_id:
                            "15871154585880608648532107628464183779895785213830018178010423617714102767076"
                                .to_owned(),
                        price: Decimal::new(41, 2),
                        quantity: Decimal::new(5, 0),
                    }],
                },
            )]),
        )
    }

    fn sample_rollout_families() -> BTreeSet<String> {
        BTreeSet::from(["family-a".to_owned()])
    }

    fn live_view(real_user_shadow_smoke: bool) -> config_schema::AppLiveConfigView<'static> {
        let smoke_flag = if real_user_shadow_smoke {
            "true"
        } else {
            "false"
        };
        let raw = Box::leak(Box::new(
            load_raw_config_from_str(&format!(
                r#"
[runtime]
mode = "live"
real_user_shadow_smoke = {smoke_flag}

[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-12"

[polymarket.source]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://gamma-api.polymarket.com"
relayer_host = "https://relayer-v2.polymarket.com"
market_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
user_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "550e8400-e29b-41d4-a716-446655440000"
secret = "poly-secret"
passphrase = "poly-passphrase"

[polymarket.relayer_auth]
kind = "builder_api_key"
api_key = "builder-api-key"
timestamp = "1700000001"
passphrase = "builder-passphrase"
signature = "builder-signature"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-a"]
"#
            ))
            .expect("config should parse"),
        ));
        let validated = Box::leak(Box::new(
            ValidatedConfig::new(raw.clone()).expect("config should validate"),
        ));
        validated.for_app_live().expect("live view should validate")
    }
}
