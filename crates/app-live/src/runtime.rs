use std::{fmt, str::FromStr};

use domain::{RuntimeMode, RuntimeOverlay};
use state::{
    ApplyError, ApplyResult, PublishedSnapshot, ReconcileReport, RemoteSnapshot, StateApplier,
    StateStore,
};

use crate::bootstrap::{self, BootstrapSource, BootstrapStatus};
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
        Self {
            store: StateStore::new(),
            app_mode,
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

    pub fn reconcile(&mut self, snapshot: RemoteSnapshot) -> ReconcileReport {
        let report = bootstrap::reconcile(&mut self.store, snapshot);
        self.anchor_baseline_if_ready(report.succeeded);
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
        let journal_seq = input.journal_seq;
        let result =
            StateApplier::new(&mut self.store).apply(journal_seq, input.into_state_fact_input())?;
        if matches!(result, ApplyResult::Applied { .. }) {
            self.published_snapshot = None;
        }
        Ok(result)
    }

    pub fn publish_snapshot(&mut self, snapshot_id: &str) -> Option<PublishedSnapshot> {
        self.store.last_applied_journal_seq()?;
        let snapshot = PublishedSnapshot::from_store(
            &self.store,
            state::ProjectionReadiness::ready_fullset_pending_negrisk(snapshot_id),
        );
        self.published_snapshot = Some(snapshot.clone());
        Some(snapshot)
    }

    pub fn replay_committed_history(
        &mut self,
        history: &[InputTaskEvent],
    ) -> Result<(), ApplyError> {
        self.store = StateStore::new();
        self.published_snapshot = None;

        for input in history.iter().cloned() {
            let _ = StateApplier::new(&mut self.store)
                .apply(input.journal_seq, input.into_state_fact_input())?;
        }

        self.store.mark_reconciled_after_restore(0);
        Ok(())
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
    run_with_mode(AppRuntimeMode::Paper, source)
}

pub fn run_live<S>(source: &S) -> AppRunResult
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
