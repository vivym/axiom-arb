use std::{collections::BTreeSet, error::Error};

use observability::{field_keys, span_names};

use crate::{
    bootstrap::BootstrapSource,
    config::NegRiskLiveTargetSet,
    negrisk_live::NegRiskLiveExecutionBackend,
    runtime::{
        load_durable_live_startup_state, load_durable_live_startup_state_for_strategy,
        operator_target_revision_for, persist_operator_target_revision_anchor_with_run_session_id,
        AppRunResult, AppRuntimeMode,
    },
    smoke::RealUserShadowSmokeConfig,
    AppInstrumentation, AppSupervisor, SupervisorError, SupervisorSummary,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonReport {
    pub startup_order: Vec<String>,
    pub ticks_run: usize,
    pub idle_reached: bool,
    pub real_user_shadow_smoke: bool,
    pub summary: SupervisorSummary,
}

pub struct AppDaemon {
    supervisor: AppSupervisor,
}

impl AppDaemon {
    pub fn new(supervisor: AppSupervisor) -> Self {
        Self { supervisor }
    }

    pub fn for_tests(supervisor: AppSupervisor) -> Self {
        Self::new(supervisor)
    }

    pub async fn run_until_idle_for_tests(
        &mut self,
        max_ticks: usize,
    ) -> Result<DaemonReport, SupervisorError> {
        let span = tracing::info_span!(
            span_names::APP_DAEMON_RUN,
            app_mode = tracing::field::Empty,
            global_posture = tracing::field::Empty,
            ingress_backlog = tracing::field::Empty,
            follow_up_backlog = tracing::field::Empty
        );
        let _span_guard = span.enter();
        span.record(field_keys::APP_MODE, self.supervisor.app_mode().as_str());

        let mut startup_order = vec![
            self.supervisor.startup_phase_label().to_owned(),
            "state".to_owned(),
            "decision".to_owned(),
        ];
        let mut ticks_run = 0usize;
        let mut idle_reached = false;
        let mut last_summary = None;
        let mut final_summary = None;

        if max_ticks == 0 {
            return Err(SupervisorError::new(
                "daemon test runner requires at least one tick",
            ));
        }

        while ticks_run < max_ticks {
            let summary = self.supervisor.run_once()?;
            ticks_run += 1;
            let no_more_progress_possible = last_summary.as_ref() == Some(&summary);
            idle_reached = self.supervisor.can_resume_ingest_loops();
            if idle_reached && !startup_order.iter().any(|step| step == "ingest") {
                startup_order.push("ingest".to_owned());
            }
            final_summary = Some(summary.clone());
            if idle_reached || no_more_progress_possible {
                break;
            }
            last_summary = Some(summary);
        }
        let summary = final_summary.expect("max_ticks > 0 should execute at least one tick");

        span.record(field_keys::GLOBAL_POSTURE, summary.global_posture.as_str());
        span.record(field_keys::INGRESS_BACKLOG, summary.ingress_backlog_count);
        span.record(
            field_keys::FOLLOW_UP_BACKLOG,
            summary.follow_up_backlog_count,
        );

        Ok(DaemonReport {
            startup_order,
            ticks_run,
            idle_reached,
            real_user_shadow_smoke: summary.real_user_shadow_smoke,
            summary,
        })
    }

    pub async fn run_until_shutdown(mut self) -> Result<DaemonReport, SupervisorError> {
        self.run_until_idle_for_tests(usize::MAX).await
    }

    pub fn run_startup(self) -> Result<AppRunResult, SupervisorError> {
        let span = tracing::info_span!(
            span_names::APP_DAEMON_RUN,
            app_mode = tracing::field::Empty,
            global_posture = tracing::field::Empty,
            ingress_backlog = tracing::field::Empty,
            follow_up_backlog = tracing::field::Empty
        );
        let _span_guard = span.enter();
        span.record(field_keys::APP_MODE, self.supervisor.app_mode().as_str());

        let result = self.supervisor.run_startup()?;
        span.record(
            field_keys::GLOBAL_POSTURE,
            result.summary.global_posture.as_str(),
        );
        span.record(
            field_keys::INGRESS_BACKLOG,
            result.summary.ingress_backlog_count,
        );
        span.record(
            field_keys::FOLLOW_UP_BACKLOG,
            result.summary.follow_up_backlog_count,
        );

        Ok(result)
    }
}

pub fn run_paper_daemon_instrumented<S>(
    source: &S,
    instrumentation: AppInstrumentation,
) -> Result<AppRunResult, Box<dyn Error>>
where
    S: BootstrapSource,
{
    let supervisor = match instrumentation.recorder() {
        Some(recorder) => {
            AppSupervisor::new_instrumented(AppRuntimeMode::Paper, source.snapshot(), recorder)
        }
        None => AppSupervisor::new(AppRuntimeMode::Paper, source.snapshot()),
    };

    AppDaemon::new(supervisor)
        .run_startup()
        .map_err(|error| -> Box<dyn Error> { Box::new(error) })
}

pub fn run_live_daemon_from_durable_store_with_neg_risk_live_targets_instrumented<S>(
    source: &S,
    instrumentation: AppInstrumentation,
    neg_risk_live_targets: NegRiskLiveTargetSet,
    neg_risk_live_approved_families: BTreeSet<String>,
    neg_risk_live_ready_families: BTreeSet<String>,
    real_user_shadow_smoke: Option<RealUserShadowSmokeConfig>,
) -> Result<AppRunResult, Box<dyn Error>>
where
    S: BootstrapSource,
{
    if real_user_shadow_smoke.is_none()
        && live_neg_risk_work_requested(
            &neg_risk_live_targets,
            &neg_risk_live_approved_families,
            &neg_risk_live_ready_families,
        )
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "live neg-risk startup requires an explicit execution backend; use the signer-gated run path",
        )
        .into());
    }

    run_live_daemon_from_durable_store_with_neg_risk_live_targets_and_session_instrumented(
        source,
        instrumentation,
        neg_risk_live_targets,
        neg_risk_live_approved_families,
        neg_risk_live_ready_families,
        real_user_shadow_smoke,
        None,
    )
}

pub(crate) fn run_live_daemon_from_durable_store_with_neg_risk_live_targets_and_session_instrumented<
    S,
>(
    source: &S,
    instrumentation: AppInstrumentation,
    neg_risk_live_targets: NegRiskLiveTargetSet,
    neg_risk_live_approved_families: BTreeSet<String>,
    neg_risk_live_ready_families: BTreeSet<String>,
    real_user_shadow_smoke: Option<RealUserShadowSmokeConfig>,
    run_session_id: Option<&str>,
) -> Result<AppRunResult, Box<dyn Error>>
where
    S: BootstrapSource,
{
    run_live_daemon_from_durable_store_with_strategy_revision_and_session_instrumented(
        source,
        instrumentation,
        None,
        false,
        neg_risk_live_targets,
        neg_risk_live_approved_families,
        neg_risk_live_ready_families,
        None,
        real_user_shadow_smoke,
        run_session_id,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_live_daemon_from_durable_store_with_strategy_revision_and_session_instrumented<
    S,
>(
    source: &S,
    instrumentation: AppInstrumentation,
    operator_strategy_revision: Option<&str>,
    allow_legacy_target_source_resume: bool,
    neg_risk_live_targets: NegRiskLiveTargetSet,
    neg_risk_live_approved_families: BTreeSet<String>,
    neg_risk_live_ready_families: BTreeSet<String>,
    neg_risk_live_execution_backend: Option<Box<dyn NegRiskLiveExecutionBackend>>,
    real_user_shadow_smoke: Option<RealUserShadowSmokeConfig>,
    run_session_id: Option<&str>,
) -> Result<AppRunResult, Box<dyn Error>>
where
    S: BootstrapSource,
{
    let load_shadow_state = real_user_shadow_smoke.is_some();
    let operator_target_revision =
        operator_target_revision_for(&neg_risk_live_targets).map(str::to_owned);
    let durable_state = if operator_strategy_revision.is_some() {
        load_durable_live_startup_state_for_strategy(
            operator_strategy_revision,
            operator_target_revision.as_deref(),
            allow_legacy_target_source_resume,
            load_shadow_state,
        )?
    } else {
        load_durable_live_startup_state(operator_target_revision.as_deref(), load_shadow_state)?
    };
    validate_real_user_shadow_smoke_restore(&durable_state, real_user_shadow_smoke.as_ref())?;
    let mut supervisor = match instrumentation.recorder() {
        Some(recorder) => {
            AppSupervisor::new_instrumented(AppRuntimeMode::Live, source.snapshot(), recorder)
        }
        None => AppSupervisor::new(AppRuntimeMode::Live, source.snapshot()),
    };
    if let Some(run_session_id) = run_session_id {
        supervisor.set_run_session_id(run_session_id);
    }
    seed_live_supervisor_from_durable_state(
        &mut supervisor,
        durable_state,
        neg_risk_live_targets,
        neg_risk_live_approved_families,
        neg_risk_live_ready_families,
        real_user_shadow_smoke,
        true,
    );
    if let Some(backend) = neg_risk_live_execution_backend {
        supervisor.set_neg_risk_live_execution_backend_boxed(backend);
    }

    let result = AppDaemon::new(supervisor)
        .run_startup()
        .map_err(|error| -> Box<dyn Error> { Box::new(error) })?;
    persist_operator_target_revision_anchor_with_run_session_id(
        &result.summary,
        operator_target_revision.as_deref(),
        operator_strategy_revision,
        run_session_id,
    )?;
    Ok(result)
}

fn validate_real_user_shadow_smoke_restore(
    durable_state: &crate::runtime::DurableLiveStartupState,
    real_user_shadow_smoke: Option<&RealUserShadowSmokeConfig>,
) -> Result<(), Box<dyn Error>> {
    if real_user_shadow_smoke.is_some() && !durable_state.live_execution_records.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "real-user shadow smoke cannot resume with durable live execution records",
        )
        .into());
    }

    Ok(())
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

fn seed_live_supervisor_from_durable_state(
    supervisor: &mut AppSupervisor,
    durable_state: crate::runtime::DurableLiveStartupState,
    neg_risk_live_targets: NegRiskLiveTargetSet,
    neg_risk_live_approved_families: BTreeSet<String>,
    neg_risk_live_ready_families: BTreeSet<String>,
    real_user_shadow_smoke: Option<RealUserShadowSmokeConfig>,
    enable_durable_shadow_persistence: bool,
) {
    supervisor.enable_durable_live_persistence();
    if real_user_shadow_smoke.is_some() {
        supervisor.enable_real_user_shadow_smoke();
        if enable_durable_shadow_persistence {
            supervisor.enable_durable_shadow_persistence();
        }
    }
    supervisor.seed_runtime_progress(
        durable_state.last_journal_seq,
        durable_state.last_state_version,
        durable_state.published_snapshot_id.as_deref(),
    );
    supervisor.seed_committed_state_version(durable_state.last_state_version);
    supervisor.seed_pending_reconcile_count(durable_state.pending_reconcile_anchors.len());
    supervisor.seed_candidate_restore_status(
        durable_state
            .candidate_restore_status
            .latest_candidate_revision
            .as_deref(),
        durable_state
            .candidate_restore_status
            .latest_adoptable_revision
            .as_deref(),
        durable_state
            .candidate_restore_status
            .latest_candidate_operator_target_revision
            .as_deref(),
        durable_state
            .candidate_restore_status
            .adoption_provenance_resolved,
    );
    for anchor in durable_state.pending_reconcile_anchors {
        supervisor.seed_pending_reconcile_anchor(anchor);
    }
    for record in durable_state.live_execution_records {
        supervisor.seed_neg_risk_live_execution_record(record);
    }
    if real_user_shadow_smoke.is_some() {
        for attempt in durable_state.shadow_execution_attempts {
            supervisor.seed_neg_risk_shadow_execution_attempt(attempt);
        }
        for artifact in durable_state.shadow_execution_artifacts {
            supervisor.seed_neg_risk_shadow_execution_artifact(artifact);
        }
    }
    supervisor.seed_neg_risk_live_targets(neg_risk_live_targets.into_targets());
    for family_id in neg_risk_live_approved_families {
        supervisor.seed_neg_risk_live_approval(&family_id);
    }
    for family_id in neg_risk_live_ready_families {
        supervisor.seed_neg_risk_live_ready_family(&family_id);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use config_schema::{load_raw_config_from_str, ValidatedConfig};
    use domain::ExecutionMode;

    use super::seed_live_supervisor_from_durable_state;
    use crate::{load_real_user_shadow_smoke_config, AppSupervisor, NegRiskLiveTargetSet};

    #[test]
    fn smoke_config_enables_shadow_path_when_seeding_live_daemon_startup() {
        let mut supervisor = AppSupervisor::for_tests();
        seed_live_supervisor_from_durable_state(
            &mut supervisor,
            crate::runtime::DurableLiveStartupState {
                last_journal_seq: 0,
                last_state_version: 0,
                published_snapshot_id: Some("snapshot-0".to_owned()),
                pending_reconcile_anchors: Vec::new(),
                live_execution_records: Vec::new(),
                shadow_execution_attempts: Vec::new(),
                shadow_execution_artifacts: Vec::new(),
                candidate_restore_status: crate::supervisor::CandidateRestoreStatus::default(),
            },
            NegRiskLiveTargetSet::try_from(&smoke_config_view()).expect("targets should parse"),
            BTreeSet::from(["family-a".to_owned()]),
            BTreeSet::from(["family-a".to_owned()]),
            load_real_user_shadow_smoke_config(&smoke_config_view())
                .expect("smoke config should parse"),
            false,
        );

        let summary = supervisor.run_once().expect("supervisor should run");

        assert_eq!(summary.negrisk_mode, ExecutionMode::Shadow);
        assert_eq!(summary.neg_risk_live_attempt_count, 0);
        assert_eq!(supervisor.neg_risk_shadow_execution_attempts().len(), 1);
        assert_eq!(supervisor.neg_risk_shadow_execution_artifacts().len(), 1);
    }

    fn smoke_config_view() -> config_schema::AppLiveConfigView<'static> {
        let raw = load_raw_config_from_str(
            r#"
[runtime]
mode = "live"
real_user_shadow_smoke = true

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
api_key = "poly-api-key-1"
secret = "poly-secret-1"
passphrase = "poly-passphrase-1"

[polymarket.relayer_auth]
kind = "builder_api_key"
api_key = "builder-api-key-1"
timestamp = "1700000001"
passphrase = "builder-passphrase-1"
signature = "builder-signature-1"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-a"]

[[negrisk.targets]]
family_id = "family-a"

[[negrisk.targets.members]]
condition_id = "condition-1"
token_id = "token-1"
price = "0.45"
quantity = "10"
"#,
        )
        .expect("config should parse");
        let raw = Box::leak(Box::new(raw));
        let validated = Box::leak(Box::new(
            ValidatedConfig::new(raw.clone()).expect("config should validate"),
        ));

        validated
            .for_app_live()
            .expect("live config should validate")
    }
}
