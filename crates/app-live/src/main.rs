use std::{collections::BTreeSet, process};

use app_live::{
    build_real_user_shadow_smoke_sources, cli::AppLiveCli,
    instrumentation::emit_bootstrap_completion_observability,
    run_live_daemon_from_durable_store_with_neg_risk_live_targets_instrumented,
    run_paper_instrumented, AppInstrumentation, ConfigError, LocalSignerConfig,
    NegRiskLiveTargetSet, SmokeSafeStartupSource, StaticSnapshotSource,
};
use clap::Parser;
use config_schema::{load_raw_config_from_path, RuntimeModeToml, ValidatedConfig};
use observability::{bootstrap_observability, span_names};
use persistence::PersistenceError;

fn main() {
    if let Err(error) = run() {
        tracing::error!(error = %error, "app-live bootstrap failed");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = AppLiveCli::parse();
    let observability = bootstrap_observability("app-live");
    let bootstrap_span = tracing::info_span!(span_names::APP_BOOTSTRAP);
    let _bootstrap_guard = bootstrap_span.enter();
    let raw = load_raw_config_from_path(&cli.config)?;
    let validated = ValidatedConfig::new(raw)?;
    let config = validated.for_app_live()?;
    let real_user_shadow_smoke = app_live::load_real_user_shadow_smoke_config(&config)?;
    let neg_risk_live_targets = NegRiskLiveTargetSet::try_from(&config)?;
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
    require_database_url_env()?;
    let instrumentation = AppInstrumentation::enabled(observability.recorder());
    let result = match config.mode() {
        RuntimeModeToml::Paper => {
            let source = StaticSnapshotSource::empty();
            run_paper_instrumented(&source, instrumentation.clone())
        }
        RuntimeModeToml::Live => {
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
                            value: cli.config.display().to_string(),
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

fn require_database_url_env() -> Result<(), Box<dyn std::error::Error>> {
    std::env::var("DATABASE_URL")
        .map(|_| ())
        .map_err(|_| PersistenceError::MissingDatabaseUrl.into())
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
