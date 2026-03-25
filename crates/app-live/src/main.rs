use std::{
    collections::{BTreeMap, BTreeSet},
    env, process,
    str::FromStr,
};

use app_live::{
    load_neg_risk_live_targets, run_live_with_neg_risk_live_targets_instrumented,
    run_paper_instrumented, AppInstrumentation, AppRuntimeMode, NegRiskFamilyLiveTarget,
    StaticSnapshotSource,
};
use domain::RuntimeMode;
use observability::{bootstrap_observability, span_names};

const NEG_RISK_LIVE_TARGETS_ENV: &str = "AXIOM_NEG_RISK_LIVE_TARGETS";
const NEG_RISK_LIVE_APPROVED_FAMILIES_ENV: &str = "AXIOM_NEG_RISK_LIVE_APPROVED_FAMILIES";
const NEG_RISK_LIVE_READY_FAMILIES_ENV: &str = "AXIOM_NEG_RISK_LIVE_READY_FAMILIES";

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
            run_live_with_neg_risk_live_targets_instrumented(
                &source,
                instrumentation,
                neg_risk_live_targets,
                neg_risk_live_approved_families,
                neg_risk_live_ready_families,
            )
        }
    };
    let recorder = observability.recorder();
    recorder.record_runtime_mode(runtime_mode_label(result.runtime.runtime_mode()));
    recorder.record_neg_risk_live_attempt_count(result.summary.neg_risk_live_attempt_count as f64);
    if let Some(evidence) = result.summary.neg_risk_rollout_evidence.as_ref() {
        recorder.record_neg_risk_live_ready_family_count(evidence.live_ready_family_count as f64);
        recorder.record_neg_risk_live_gate_block_count(evidence.blocked_family_count as f64);
        recorder.increment_neg_risk_rollout_parity_mismatch_count(evidence.parity_mismatch_count);
    }

    let completion_span = tracing::info_span!(
        span_names::APP_BOOTSTRAP_COMPLETE,
        app_mode = %result.runtime.app_mode().as_str(),
        bootstrap_status = ?result.runtime.bootstrap_status(),
        promoted_from_bootstrap = result.report.promoted_from_bootstrap,
        runtime_mode = ?result.runtime.runtime_mode(),
        fullset_mode = ?result.summary.fullset_mode,
        negrisk_mode = ?result.summary.negrisk_mode,
        neg_risk_live_attempt_count = result.summary.neg_risk_live_attempt_count,
        neg_risk_live_state_source = result.summary.neg_risk_live_state_source.as_str(),
        pending_reconcile_count = result.summary.pending_reconcile_count,
        published_snapshot_id = %result
            .summary
            .published_snapshot_id
            .as_deref()
            .unwrap_or("none")
    );
    let _completion_guard = completion_span.enter();
    tracing::info!("app-live bootstrap complete");

    Ok(())
}

fn runtime_mode_label(mode: RuntimeMode) -> &'static str {
    match mode {
        RuntimeMode::Bootstrapping => "bootstrapping",
        RuntimeMode::Healthy => "healthy",
        RuntimeMode::Reconciling => "reconciling",
        RuntimeMode::Degraded => "degraded",
        RuntimeMode::NoNewRisk => "no_new_risk",
        RuntimeMode::GlobalHalt => "global_halt",
    }
}

fn load_neg_risk_live_targets_env(
) -> Result<BTreeMap<String, NegRiskFamilyLiveTarget>, Box<dyn std::error::Error>> {
    match env::var(NEG_RISK_LIVE_TARGETS_ENV) {
        Ok(value) => Ok(load_neg_risk_live_targets(Some(value.as_str()))?),
        Err(env::VarError::NotPresent) => Ok(BTreeMap::new()),
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
