use std::{fmt, str::FromStr};

use domain::{RuntimeMode, RuntimeOverlay};
use observability::{field_keys, span_names};
use state::{
    ApplyError, ApplyResult, PublishedSnapshot, ReconcileReport, RemoteSnapshot, StateApplier,
    StateStore,
};
use tracing::field;

use crate::bootstrap::{self, BootstrapSource, BootstrapStatus};
use crate::instrumentation::AppInstrumentation;
use crate::input_tasks::InputTaskEvent;
use crate::supervisor::SupervisorSummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppRuntimeMode {
    Paper,
    Live,
}

impl AppRuntimeMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::Live => "live",
        }
    }
}

impl FromStr for AppRuntimeMode {
    type Err = ParseAppRuntimeModeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "paper" => Ok(Self::Paper),
            "live" => Ok(Self::Live),
            other => Err(ParseAppRuntimeModeError {
                received: other.to_owned(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseAppRuntimeModeError {
    received: String,
}

impl ParseAppRuntimeModeError {
    pub fn received(&self) -> &str {
        &self.received
    }
}

impl fmt::Display for ParseAppRuntimeModeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unsupported AXIOM_MODE '{}'; expected 'paper' or 'live'",
            self.received
        )
    }
}

impl std::error::Error for ParseAppRuntimeModeError {}

#[derive(Debug)]
pub struct AppRuntime {
    store: StateStore,
    app_mode: AppRuntimeMode,
    instrumentation: AppInstrumentation,
    published_snapshot: Option<PublishedSnapshot>,
}

#[derive(Debug)]
pub struct AppRunResult {
    pub runtime: AppRuntime,
    pub report: ReconcileReport,
    pub summary: SupervisorSummary,
}

impl AppRuntime {
    pub fn new(app_mode: AppRuntimeMode) -> Self {
        Self::new_instrumented(app_mode, AppInstrumentation::disabled())
    }

    pub fn new_instrumented(app_mode: AppRuntimeMode, instrumentation: AppInstrumentation) -> Self {
        Self {
            store: StateStore::new(),
            app_mode,
            instrumentation,
            published_snapshot: None,
        }
    }

    pub fn app_mode(&self) -> AppRuntimeMode {
        self.app_mode
    }

    pub fn bootstrap_status(&self) -> BootstrapStatus {
        bootstrap::bootstrap_status(&self.store)
    }

    pub fn runtime_mode(&self) -> RuntimeMode {
        self.store.mode()
    }

    pub fn state_version(&self) -> u64 {
        self.store.state_version()
    }

    pub fn last_journal_seq(&self) -> Option<i64> {
        self.store.last_consumed_journal_seq()
    }

    pub fn published_snapshot_id(&self) -> Option<&str> {
        self.published_snapshot
            .as_ref()
            .map(|snapshot| snapshot.snapshot_id.as_str())
    }

    pub fn published_snapshot_committed_journal_seq(&self) -> Option<i64> {
        self.published_snapshot
            .as_ref()
            .map(|snapshot| snapshot.committed_journal_seq)
    }

    pub fn runtime_overlay(&self) -> Option<RuntimeOverlay> {
        self.store.mode_overlay()
    }

    pub fn pending_reconcile_count(&self) -> usize {
        self.store.pending_reconcile_count()
    }

    pub fn reconcile(&mut self, snapshot: RemoteSnapshot) -> ReconcileReport {
        let span = tracing::info_span!(
            span_names::APP_RUNTIME_RECONCILE,
            app_mode = field::Empty,
            pending_reconcile_count = field::Empty
        );
        let _span_guard = span.enter();
        span.record(field_keys::APP_MODE, &self.app_mode.as_str());

        let report = bootstrap::reconcile(&mut self.store, snapshot);
        if !report.attention.is_empty() {
            for attention in &report.attention {
                self.instrumentation.record_reconcile_attention(attention);
            }
        }
        self.anchor_baseline_if_ready(report.succeeded);
        let pending_reconcile_count = self.store.pending_reconcile_count();
        span.record(field_keys::PENDING_RECONCILE_COUNT, &pending_reconcile_count);
        report
    }

    pub fn bootstrap_once<S>(&mut self, source: &S) -> ReconcileReport
    where
        S: BootstrapSource,
    {
        let report = bootstrap::bootstrap_once(&mut self.store, source);
        self.anchor_baseline_if_ready(report.succeeded);
        report
    }

    pub fn apply_input(&mut self, input: InputTaskEvent) -> Result<ApplyResult, ApplyError> {
        let span = tracing::info_span!(
            span_names::APP_RUNTIME_APPLY_INPUT,
            apply_result = field::Empty
        );
        let _span_guard = span.enter();
        let journal_seq = input.journal_seq;
        let result = match StateApplier::new(&mut self.store).apply(journal_seq, input.into_state_fact_input()) {
            Ok(result) => result,
            Err(error) => {
                span.record(field_keys::APPLY_RESULT, &"error");
                return Err(error);
            }
        };
        span.record(field_keys::APPLY_RESULT, &apply_result_label(&result));
        match &result {
            ApplyResult::Applied { .. } => {
                self.published_snapshot = None;
            }
            ApplyResult::ReconcileRequired { .. } => {
                self.store.mark_reconcile_required();
            }
            ApplyResult::Duplicate { .. } | ApplyResult::Deferred { .. } => {}
        }
        Ok(result)
    }

    pub fn publish_snapshot(&mut self, snapshot_id: &str) -> Option<PublishedSnapshot> {
        let span = tracing::info_span!(
            span_names::APP_RUNTIME_PUBLISH_SNAPSHOT,
            snapshot_id = field::Empty,
            state_version = field::Empty,
            committed_journal_seq = field::Empty
        );
        let _span_guard = span.enter();
        self.store.last_applied_journal_seq()?;
        let snapshot = PublishedSnapshot::from_store(
            &self.store,
            state::ProjectionReadiness::ready_fullset_pending_negrisk(snapshot_id),
        );
        span.record(field_keys::SNAPSHOT_ID, snapshot.snapshot_id.as_str());
        span.record(field_keys::STATE_VERSION, &snapshot.state_version);
        span.record("committed_journal_seq", &snapshot.committed_journal_seq);
        self.published_snapshot = Some(snapshot.clone());
        Some(snapshot)
    }

    pub fn replay_committed_history(
        &mut self,
        history: &[InputTaskEvent],
    ) -> Result<(), ApplyError> {
        self.store = StateStore::new();
        self.published_snapshot = None;
        let mut reconcile_required = false;

        for input in history.iter().cloned() {
            let result = StateApplier::new(&mut self.store)
                .apply(input.journal_seq, input.into_state_fact_input())?;
            if matches!(result, ApplyResult::ReconcileRequired { .. }) {
                reconcile_required = true;
            }
        }

        if reconcile_required {
            self.store.restore_reconciled_policy();
            self.store.mark_reconcile_required();
        } else if self.store.last_applied_journal_seq().is_none() {
            // An empty committed history still needs a baseline anchor so restart can
            // validate journal progress and re-publish the synthetic snapshot-0 boundary.
            self.store.mark_reconciled_after_restore(0);
        } else {
            self.store.restore_reconciled_policy();
        }
        Ok(())
    }

    pub fn clear_pending_reconcile_after_restore(&mut self) {
        self.store.clear_pending_reconcile_after_restore();
    }

    fn anchor_baseline_if_ready(&mut self, reconcile_succeeded: bool) {
        if reconcile_succeeded && self.store.last_applied_journal_seq().is_none() {
            self.store.mark_reconciled_after_restore(0);
        }
    }
}

pub fn run_paper<S>(source: &S) -> AppRunResult
where
    S: BootstrapSource,
{
    run_paper_instrumented(source, AppInstrumentation::disabled())
}

pub fn run_paper_instrumented<S>(source: &S, _instrumentation: AppInstrumentation) -> AppRunResult
where
    S: BootstrapSource,
{
    run_with_mode(AppRuntimeMode::Paper, source)
}

pub fn run_live<S>(source: &S) -> AppRunResult
where
    S: BootstrapSource,
{
    run_live_instrumented(source, AppInstrumentation::disabled())
}

pub fn run_live_instrumented<S>(source: &S, _instrumentation: AppInstrumentation) -> AppRunResult
where
    S: BootstrapSource,
{
    run_with_mode(AppRuntimeMode::Live, source)
}

fn run_with_mode<S>(app_mode: AppRuntimeMode, source: &S) -> AppRunResult
where
    S: BootstrapSource,
{
    crate::supervisor::AppSupervisor::new(app_mode, source.snapshot()).run_bootstrap()
}

fn apply_result_label(result: &ApplyResult) -> &'static str {
    match result {
        ApplyResult::Applied { .. } => "applied",
        ApplyResult::Duplicate { .. } => "duplicate",
        ApplyResult::Deferred { .. } => "deferred",
        ApplyResult::ReconcileRequired { .. } => "reconcile_required",
    }
}
