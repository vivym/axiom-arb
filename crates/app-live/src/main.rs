use std::{collections::BTreeSet, env, process, str::FromStr};

use app_live::{
    instrumentation::emit_bootstrap_completion_observability, load_local_signer_config,
    load_neg_risk_live_targets, load_real_user_shadow_smoke_config,
    run_live_daemon_from_durable_store_with_neg_risk_live_targets_instrumented,
    run_paper_instrumented, AppInstrumentation, AppRuntimeMode, ConfigError, LocalSignerConfig,
    NegRiskLiveTargetSet, StaticSnapshotSource,
};
use observability::{bootstrap_observability, span_names};
use persistence::PersistenceError;

const NEG_RISK_LIVE_TARGETS_ENV: &str = "AXIOM_NEG_RISK_LIVE_TARGETS";
const NEG_RISK_LIVE_APPROVED_FAMILIES_ENV: &str = "AXIOM_NEG_RISK_LIVE_APPROVED_FAMILIES";
const NEG_RISK_LIVE_READY_FAMILIES_ENV: &str = "AXIOM_NEG_RISK_LIVE_READY_FAMILIES";
const LOCAL_SIGNER_CONFIG_ENV: &str = "AXIOM_LOCAL_SIGNER_CONFIG";
const REAL_USER_SHADOW_SMOKE_ENV: &str = "AXIOM_REAL_USER_SHADOW_SMOKE";
const POLYMARKET_SOURCE_CONFIG_ENV: &str = "AXIOM_POLYMARKET_SOURCE_CONFIG";

fn main() {
    if let Err(error) = run() {
        tracing::error!(error = %error, "app-live bootstrap failed");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let observability = bootstrap_observability("app-live");
    let bootstrap_span = tracing::info_span!(span_names::APP_BOOTSTRAP);
    let _bootstrap_guard = bootstrap_span.enter();
    let app_mode = env::var("AXIOM_MODE").unwrap_or_else(|_| "paper".to_owned());
    let app_mode = AppRuntimeMode::from_str(&app_mode)?;
    let real_user_shadow_smoke_guard = load_real_user_shadow_smoke_guard();
    if app_mode == AppRuntimeMode::Paper && real_user_shadow_smoke_guard.as_deref() == Some("1") {
        return Err("real-user shadow smoke is not supported in paper mode".into());
    }
    let _real_user_shadow_smoke = load_real_user_shadow_smoke_config_env(
        real_user_shadow_smoke_guard.as_deref(),
    )?;
    let source = StaticSnapshotSource::empty();
    let instrumentation = AppInstrumentation::enabled(observability.recorder());
    let result = match app_mode {
        AppRuntimeMode::Paper => run_paper_instrumented(&source, instrumentation.clone()),
        AppRuntimeMode::Live => {
            let neg_risk_live_targets = load_neg_risk_live_targets_env()?;
            let neg_risk_live_approved_families =
                load_family_scope_env(NEG_RISK_LIVE_APPROVED_FAMILIES_ENV)?;
            let neg_risk_live_ready_families =
                load_family_scope_env(NEG_RISK_LIVE_READY_FAMILIES_ENV)?;
            if live_neg_risk_work_requested(
                &neg_risk_live_targets,
                &neg_risk_live_approved_families,
                &neg_risk_live_ready_families,
            ) {
                let _local_signer_config = load_local_signer_config_env()?;
            }
            require_database_url_env()?;
            run_live_daemon_from_durable_store_with_neg_risk_live_targets_instrumented(
                &source,
                instrumentation,
                neg_risk_live_targets,
                neg_risk_live_approved_families,
                neg_risk_live_ready_families,
            )?
        }
    };
    emit_bootstrap_completion_observability(&observability.recorder(), &result);

    Ok(())
}

fn load_real_user_shadow_smoke_guard() -> Option<String> {
    env::var_os(REAL_USER_SHADOW_SMOKE_ENV).and_then(|value| value.into_string().ok())
}

fn load_real_user_shadow_smoke_config_env(
    guard: Option<&str>,
) -> Result<Option<app_live::RealUserShadowSmokeConfig>, Box<dyn std::error::Error>> {
    if guard != Some("1") {
        return Ok(None);
    }

    let source_json = match env::var(POLYMARKET_SOURCE_CONFIG_ENV) {
        Ok(value) => Some(value),
        Err(env::VarError::NotPresent) => None,
        Err(env::VarError::NotUnicode(_)) => {
            return Err(format!(
                "invalid value for {POLYMARKET_SOURCE_CONFIG_ENV}: value is not valid UTF-8"
            )
            .into());
        }
    };

    Ok(load_real_user_shadow_smoke_config(guard, source_json.as_deref())?)
}

fn load_neg_risk_live_targets_env() -> Result<NegRiskLiveTargetSet, Box<dyn std::error::Error>> {
    match env::var(NEG_RISK_LIVE_TARGETS_ENV) {
        Ok(value) => Ok(load_neg_risk_live_targets(Some(value.as_str()))?),
        Err(env::VarError::NotPresent) => Ok(NegRiskLiveTargetSet::empty()),
        Err(env::VarError::NotUnicode(_)) => Err(format!(
            "invalid value for {NEG_RISK_LIVE_TARGETS_ENV}: value is not valid UTF-8"
        )
        .into()),
    }
}

fn load_family_scope_env(var_name: &str) -> Result<BTreeSet<String>, Box<dyn std::error::Error>> {
    match env::var(var_name) {
        Ok(value) => Ok(value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect()),
        Err(env::VarError::NotPresent) => Ok(BTreeSet::new()),
        Err(env::VarError::NotUnicode(_)) => {
            Err(format!("invalid value for {var_name}: value is not valid UTF-8").into())
        }
    }
}

fn load_local_signer_config_env() -> Result<LocalSignerConfig, Box<dyn std::error::Error>> {
    match env::var(LOCAL_SIGNER_CONFIG_ENV) {
        Ok(value) => Ok(load_local_signer_config(Some(value.as_str()))?),
        Err(env::VarError::NotPresent) => Err(ConfigError::MissingLocalSignerConfig.into()),
        Err(env::VarError::NotUnicode(_)) => Err(format!(
            "invalid value for {LOCAL_SIGNER_CONFIG_ENV}: value is not valid UTF-8"
        )
        .into()),
    }
}

fn require_database_url_env() -> Result<(), Box<dyn std::error::Error>> {
    std::env::var("DATABASE_URL")
        .map(|_| ())
        .map_err(|_| PersistenceError::MissingDatabaseUrl.into())
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
