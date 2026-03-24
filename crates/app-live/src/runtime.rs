use std::{fmt, str::FromStr};

use domain::{RuntimeMode, RuntimeOverlay};
use state::{
    ApplyError, ApplyResult, DirtyDomain, DirtySet, FullSetView, PublishedSnapshot,
    ReconcileReport, RemoteSnapshot, StateApplier, StateStore,
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
    last_journal_seq: Option<i64>,
    durable_anchor: Option<DurableStateAnchor>,
    published_snapshot: Option<PublishedSnapshot>,
}

#[derive(Debug)]
pub struct AppRunResult {
    pub runtime: AppRuntime,
    pub report: ReconcileReport,
    pub summary: SupervisorSummary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DurableStateAnchor {
    committed_state_version: u64,
    committed_journal_seq: i64,
}

impl AppRuntime {
    pub fn new(app_mode: AppRuntimeMode) -> Self {
        Self {
            store: StateStore::new(),
            app_mode,
            last_journal_seq: None,
            durable_anchor: None,
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
        self.durable_anchor
            .as_ref()
            .map(|anchor| anchor.committed_state_version)
            .unwrap_or_else(|| self.store.state_version())
    }

    pub fn last_journal_seq(&self) -> Option<i64> {
        self.last_journal_seq
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
        bootstrap::reconcile(&mut self.store, snapshot)
    }

    pub fn bootstrap_once<S>(&mut self, source: &S) -> ReconcileReport
    where
        S: BootstrapSource,
    {
        bootstrap::bootstrap_once(&mut self.store, source)
    }

    pub fn apply_input(&mut self, input: InputTaskEvent) -> Result<ApplyResult, ApplyError> {
        if let Some(anchor) = self.durable_anchor.as_mut() {
            anchor.committed_state_version += 1;
            anchor.committed_journal_seq = input.journal_seq;
            self.last_journal_seq = Some(input.journal_seq);
            self.published_snapshot = None;

            return Ok(ApplyResult::Applied {
                journal_seq: input.journal_seq,
                state_version: anchor.committed_state_version,
                dirty_set: DirtySet::new([
                    DirtyDomain::Runtime,
                    DirtyDomain::Orders,
                    DirtyDomain::Inventory,
                    DirtyDomain::Approvals,
                    DirtyDomain::Resolution,
                    DirtyDomain::Relayer,
                    DirtyDomain::NegRiskFamilies,
                ]),
            });
        }

        let result = StateApplier::new(&mut self.store).apply(input.journal_seq, input.event)?;
        self.last_journal_seq = Some(input.journal_seq);
        self.published_snapshot = None;
        Ok(result)
    }

    pub fn publish_snapshot(&mut self, snapshot_id: &str) -> Option<PublishedSnapshot> {
        let snapshot = if let Some(anchor) = self.durable_anchor.as_ref() {
            anchored_snapshot(
                snapshot_id,
                anchor.committed_state_version,
                anchor.committed_journal_seq,
            )
        } else {
            self.store.last_applied_journal_seq()?;
            PublishedSnapshot::from_store(
                &self.store,
                state::ProjectionReadiness::ready_fullset_pending_negrisk(snapshot_id),
            )
        };
        self.published_snapshot = Some(snapshot.clone());
        Some(snapshot)
    }

    pub fn restore_durable_anchor(
        &mut self,
        committed_state_version: u64,
        last_journal_seq: i64,
        published_snapshot_id: Option<String>,
    ) {
        self.store = StateStore::new();
        self.last_journal_seq = Some(last_journal_seq);
        self.durable_anchor = (committed_state_version > 0).then_some(DurableStateAnchor {
            committed_state_version,
            committed_journal_seq: last_journal_seq,
        });
        self.published_snapshot = published_snapshot_id
            .filter(|snapshot_id| snapshot_id == &format!("snapshot-{committed_state_version}"))
            .and_then(|snapshot_id| {
                self.durable_anchor.as_ref().map(|anchor| {
                    anchored_snapshot(
                        &snapshot_id,
                        anchor.committed_state_version,
                        anchor.committed_journal_seq,
                    )
                })
            });
    }
}

fn anchored_snapshot(
    snapshot_id: &str,
    state_version: u64,
    committed_journal_seq: i64,
) -> PublishedSnapshot {
    PublishedSnapshot {
        snapshot_id: snapshot_id.to_owned(),
        state_version,
        committed_journal_seq,
        fullset_ready: true,
        negrisk_ready: false,
        fullset: Some(FullSetView {
            snapshot_id: snapshot_id.to_owned(),
            state_version,
            open_orders: Vec::new(),
        }),
        negrisk: None,
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
