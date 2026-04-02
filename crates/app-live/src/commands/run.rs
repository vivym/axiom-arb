use std::{collections::BTreeSet, error::Error, path::Path};

use config_schema::{load_raw_config_from_path, RuntimeModeToml, ValidatedConfig};
use observability::{bootstrap_observability, span_names};
use persistence::connect_pool_from_env;

use crate::cli::RunArgs;
use crate::{
    build_real_user_shadow_smoke_sources, instrumentation::emit_bootstrap_completion_observability,
    load_real_user_shadow_smoke_config,
    run_live_daemon_from_durable_store_with_neg_risk_live_targets_instrumented,
    run_paper_instrumented, startup::resolve_startup_targets, AppInstrumentation, ConfigError,
    LocalSignerConfig, NegRiskLiveTargetSet, SmokeSafeStartupSource, StaticSnapshotSource,
};

pub fn execute(args: RunArgs) -> Result<(), Box<dyn Error>> {
    run_from_config_path(&args.config)
}

pub fn run_from_config_path(config_path: &Path) -> Result<(), Box<dyn Error>> {
    let observability = bootstrap_observability("app-live");
    let bootstrap_span = tracing::info_span!(span_names::APP_BOOTSTRAP);
    let _bootstrap_guard = bootstrap_span.enter();
    let raw = load_raw_config_from_path(config_path)?;
    let validated = ValidatedConfig::new(raw)?;
    let config = validated.for_app_live()?;
    let real_user_shadow_smoke = load_real_user_shadow_smoke_config(&config)?;
    require_database_url_env()?;
    let instrumentation = AppInstrumentation::enabled(observability.recorder());
    let result = match config.mode() {
        RuntimeModeToml::Paper => {
            let source = StaticSnapshotSource::empty();
            run_paper_instrumented(&source, instrumentation.clone())
        }
        RuntimeModeToml::Live => {
            let resolved_targets = load_resolved_targets_from_config(&config)?;
            let neg_risk_live_targets = resolved_targets.targets;
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
            let source = match real_user_shadow_smoke.as_ref() {
                Some(smoke) => SmokeSafeStartupSource::RealUserShadowSmoke(Box::new(
                    build_real_user_shadow_smoke_sources(
                        smoke.source_config.clone(),
                        signer_config
                            .clone()
                            .expect("smoke startup should require signer config"),
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

            if live_neg_risk_work_requested(
                &neg_risk_live_targets,
                &neg_risk_live_approved_families,
                &neg_risk_live_ready_families,
            ) {
                let _ = signer_config;
            }

            run_live_daemon_from_durable_store_with_neg_risk_live_targets_instrumented(
                &source,
                instrumentation,
                neg_risk_live_targets,
                neg_risk_live_approved_families,
                neg_risk_live_ready_families,
                real_user_shadow_smoke,
            )?
        }
    };
    emit_bootstrap_completion_observability(&observability.recorder(), &result);

    Ok(())
}

fn require_database_url_env() -> Result<(), Box<dyn Error>> {
    std::env::var("DATABASE_URL")
        .map(|_| ())
        .map_err(|_| persistence::PersistenceError::MissingDatabaseUrl.into())
}

fn load_resolved_targets_from_config(
    config: &config_schema::AppLiveConfigView<'_>,
) -> Result<crate::ResolvedTargets, Box<dyn Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    Ok(runtime.block_on(async {
        let pool = connect_pool_from_env().await?;
        resolve_startup_targets(&pool, config).await
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
