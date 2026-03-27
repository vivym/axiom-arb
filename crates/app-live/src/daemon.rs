use std::{collections::BTreeSet, error::Error};

use observability::{field_keys, span_names};

use crate::{
    bootstrap::BootstrapSource,
    config::NegRiskLiveTargetSet,
    runtime::{
        load_durable_live_startup_state, operator_target_revision_for,
        persist_operator_target_revision_anchor, AppRunResult, AppRuntimeMode,
    },
    AppInstrumentation, AppSupervisor, SupervisorError, SupervisorSummary,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonReport {
    pub startup_order: Vec<String>,
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
        let summary = self.supervisor.run_once()?;
        if max_ticks > 0 && self.supervisor.can_resume_ingest_loops() {
            startup_order.push("ingest".to_owned());
        }

        span.record(field_keys::GLOBAL_POSTURE, summary.global_posture.as_str());
        span.record(field_keys::INGRESS_BACKLOG, summary.ingress_backlog_count);
        span.record(
            field_keys::FOLLOW_UP_BACKLOG,
            summary.follow_up_backlog_count,
        );

        Ok(DaemonReport {
            startup_order,
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
    supervisor.seed_runtime_progress(
        durable_state.last_journal_seq,
        durable_state.last_state_version,
        durable_state.published_snapshot_id.as_deref(),
    );
    supervisor.seed_committed_state_version(durable_state.last_state_version);
    supervisor.seed_pending_reconcile_count(durable_state.pending_reconcile_anchors.len());
    for anchor in durable_state.pending_reconcile_anchors {
        supervisor.seed_pending_reconcile_anchor(anchor);
    }
    for record in durable_state.live_execution_records {
        supervisor.seed_neg_risk_live_execution_record(record);
    }
    supervisor.seed_neg_risk_live_targets(neg_risk_live_targets.into_targets());
    for family_id in neg_risk_live_approved_families {
        supervisor.seed_neg_risk_live_approval(&family_id);
    }
    for family_id in neg_risk_live_ready_families {
        supervisor.seed_neg_risk_live_ready_family(&family_id);
    }

    let result = AppDaemon::new(supervisor)
        .run_startup()
        .map_err(|error| -> Box<dyn Error> { Box::new(error) })?;
    persist_operator_target_revision_anchor(&result.summary, operator_target_revision.as_deref())?;
    Ok(result)
}
