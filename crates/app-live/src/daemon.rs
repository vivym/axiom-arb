use std::{collections::BTreeSet, error::Error};

use observability::{field_keys, span_names};

use crate::{
    bootstrap::BootstrapSource,
    config::NegRiskLiveTargetSet,
    runtime::{
        load_durable_live_startup_state, operator_target_revision_for,
        persist_operator_target_revision_anchor, AppRunResult, AppRuntimeMode,
    },
    smoke::RealUserShadowSmokeConfig,
    AppInstrumentation, AppSupervisor, SupervisorError, SupervisorSummary,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonReport {
    pub startup_order: Vec<String>,
    pub ticks_run: usize,
    pub idle_reached: bool,
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
    let operator_target_revision =
        operator_target_revision_for(&neg_risk_live_targets).map(str::to_owned);
    let durable_state = load_durable_live_startup_state(operator_target_revision.as_deref())?;
    let mut supervisor = match instrumentation.recorder() {
        Some(recorder) => {
            AppSupervisor::new_instrumented(AppRuntimeMode::Live, source.snapshot(), recorder)
        }
        None => AppSupervisor::new(AppRuntimeMode::Live, source.snapshot()),
    };
    seed_live_supervisor_from_durable_state(
        &mut supervisor,
        durable_state,
        neg_risk_live_targets,
        neg_risk_live_approved_families,
        neg_risk_live_ready_families,
        real_user_shadow_smoke,
    );

    let result = AppDaemon::new(supervisor)
        .run_startup()
        .map_err(|error| -> Box<dyn Error> { Box::new(error) })?;
    persist_operator_target_revision_anchor(&result.summary, operator_target_revision.as_deref())?;
    Ok(result)
}

fn seed_live_supervisor_from_durable_state(
    supervisor: &mut AppSupervisor,
    durable_state: crate::runtime::DurableLiveStartupState,
    neg_risk_live_targets: NegRiskLiveTargetSet,
    neg_risk_live_approved_families: BTreeSet<String>,
    neg_risk_live_ready_families: BTreeSet<String>,
    real_user_shadow_smoke: Option<RealUserShadowSmokeConfig>,
) {
    if real_user_shadow_smoke.is_some() {
        supervisor.enable_real_user_shadow_smoke();
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
    for attempt in durable_state.shadow_execution_attempts {
        supervisor.seed_neg_risk_shadow_execution_attempt(attempt);
    }
    for artifact in durable_state.shadow_execution_artifacts {
        supervisor.seed_neg_risk_shadow_execution_artifact(artifact);
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

    use domain::ExecutionMode;

    use super::seed_live_supervisor_from_durable_state;
    use crate::{load_neg_risk_live_targets, load_real_user_shadow_smoke_config, AppSupervisor};

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
            load_neg_risk_live_targets(Some(valid_neg_risk_live_targets_json()))
                .expect("targets should parse"),
            BTreeSet::from(["family-a".to_owned()]),
            BTreeSet::from(["family-a".to_owned()]),
            load_real_user_shadow_smoke_config(
                Some("1"),
                Some(
                    r#"{
                      "clob_host": "https://clob.polymarket.com",
                      "data_api_host": "https://data-api.polymarket.com",
                      "relayer_host": "https://relayer-v2.polymarket.com",
                      "market_ws_url": "wss://ws-subscriptions-clob.polymarket.com/ws/market",
                      "user_ws_url": "wss://ws-subscriptions-clob.polymarket.com/ws/user",
                      "heartbeat_interval_seconds": 15,
                      "relayer_poll_interval_seconds": 5,
                      "metadata_refresh_interval_seconds": 60
                    }"#,
                ),
            )
            .expect("smoke config should parse"),
        );

        let summary = supervisor.run_once().expect("supervisor should run");

        assert_eq!(summary.negrisk_mode, ExecutionMode::Shadow);
        assert_eq!(summary.neg_risk_live_attempt_count, 0);
        assert_eq!(supervisor.neg_risk_shadow_execution_attempts().len(), 1);
        assert_eq!(supervisor.neg_risk_shadow_execution_artifacts().len(), 1);
    }

    fn valid_neg_risk_live_targets_json() -> &'static str {
        r#"
        [
          {
            "family_id": "family-a",
            "members": [
              {
                "condition_id": "condition-1",
                "token_id": "token-1",
                "price": "0.45",
                "quantity": "10"
              }
            ]
          }
        ]
        "#
    }
}
