use chrono::{TimeZone, Utc};
use domain::{ExecutionMode, ExternalFactEvent, RuntimeMode};
use state::{ApplyResult, ProjectionReadiness, RemoteSnapshot};

use crate::{
    bootstrap::{BootstrapStatus, StaticSnapshotSource},
    dispatch::{DispatchLoop, DispatchSummary},
    input_tasks::{InputTaskEvent, InputTaskQueue},
    runtime::{AppRunResult, AppRuntime, AppRuntimeMode},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorSummary {
    pub fullset_mode: ExecutionMode,
    pub negrisk_mode: ExecutionMode,
    pub bootstrap_status: BootstrapStatus,
    pub runtime_mode: RuntimeMode,
    pub last_journal_seq: i64,
    pub last_state_version: u64,
    pub published_snapshot_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorError {
    message: String,
}

impl SupervisorError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for SupervisorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SupervisorError {}

impl From<state::ApplyError> for SupervisorError {
    fn from(value: state::ApplyError) -> Self {
        Self::new(value.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct RuntimeSeed {
    last_journal_seq: Option<i64>,
    last_state_version: u64,
    published_snapshot_id: Option<String>,
    committed_state_version: Option<u64>,
}

pub struct AppSupervisor {
    dispatcher: DispatchLoop,
    runtime: AppRuntime,
    bootstrap_snapshot: RemoteSnapshot,
    input_tasks: InputTaskQueue,
    seed: RuntimeSeed,
}

impl AppSupervisor {
    pub fn new(app_mode: AppRuntimeMode, bootstrap_snapshot: RemoteSnapshot) -> Self {
        Self {
            dispatcher: DispatchLoop::default(),
            runtime: AppRuntime::new(app_mode),
            bootstrap_snapshot,
            input_tasks: InputTaskQueue::default(),
            seed: RuntimeSeed::default(),
        }
    }

    pub fn for_tests() -> Self {
        Self::new(AppRuntimeMode::Live, RemoteSnapshot::empty())
    }

    pub fn run_once(&mut self) -> Result<SupervisorSummary, SupervisorError> {
        if self.runtime.bootstrap_status() != BootstrapStatus::Ready {
            let source = StaticSnapshotSource::new(self.bootstrap_snapshot.clone());
            self.runtime.bootstrap_once(&source);
        }

        if let Some(snapshot) = self
            .runtime
            .publish_snapshot(&snapshot_id_for(self.runtime.state_version()))
        {
            self.dispatcher.observe_snapshot(snapshot);
        }
        let _ = self.dispatcher.flush();

        Ok(self.summary())
    }

    pub fn run_bootstrap(self) -> AppRunResult {
        let mut supervisor = self;
        let source = StaticSnapshotSource::new(supervisor.bootstrap_snapshot.clone());
        let report = supervisor.runtime.bootstrap_once(&source);
        let summary = supervisor.summary();

        AppRunResult {
            runtime: supervisor.runtime,
            report,
            summary,
        }
    }

    pub fn push_dirty_snapshot(&mut self, state_version: u64) {
        self.dispatcher.push_test_snapshot(state_version);
    }

    pub fn flush_dispatch(&mut self) -> DispatchSummary {
        self.dispatcher.flush()
    }

    pub fn seed_runtime_progress(
        &mut self,
        last_journal_seq: i64,
        last_state_version: u64,
        published_snapshot_id: Option<&str>,
    ) {
        self.seed.last_journal_seq = Some(last_journal_seq);
        self.seed.last_state_version = last_state_version;
        self.seed.published_snapshot_id = published_snapshot_id.map(str::to_owned);
    }

    pub fn seed_committed_state_version(&mut self, committed_state_version: u64) {
        self.seed.committed_state_version = Some(committed_state_version);
    }

    pub fn seed_unapplied_journal_entry(&mut self, journal_seq: i64, input: InputTaskEvent) {
        let mut input = input;
        input.journal_seq = journal_seq;
        self.input_tasks.push(input);
    }

    pub fn resume_once(&mut self) -> Result<SupervisorSummary, SupervisorError> {
        self.runtime = AppRuntime::new(self.runtime.app_mode());

        let committed_state_version = self
            .seed
            .committed_state_version
            .unwrap_or(self.seed.last_state_version);
        self.runtime.restore_state(committed_state_version)?;
        self.runtime.set_runtime_progress(
            self.seed.last_journal_seq,
            self.seed.published_snapshot_id.clone(),
        );

        let pending_entries = self.input_tasks.drain_after(self.seed.last_journal_seq);
        for input in pending_entries {
            match self.runtime.apply_input(input.clone())? {
                ApplyResult::Applied { state_version, .. } => {
                    if let Some(snapshot) = self
                        .runtime
                        .publish_snapshot(&snapshot_id_for(state_version))
                    {
                        self.dispatcher.observe_snapshot(snapshot);
                    }
                }
                ApplyResult::Duplicate { .. }
                | ApplyResult::Deferred { .. }
                | ApplyResult::ReconcileRequired { .. } => {}
            }
        }

        if self.runtime.published_snapshot_id().is_none() && self.runtime.state_version() > 0 {
            if let Some(snapshot) = self
                .runtime
                .publish_snapshot(&snapshot_id_for(self.runtime.state_version()))
            {
                self.dispatcher.observe_snapshot(snapshot);
            }
        }

        let _ = self.dispatcher.flush();

        Ok(self.summary())
    }

    fn summary(&self) -> SupervisorSummary {
        SupervisorSummary {
            fullset_mode: ExecutionMode::Live,
            negrisk_mode: ExecutionMode::Shadow,
            bootstrap_status: self.runtime.bootstrap_status(),
            runtime_mode: self.runtime.runtime_mode(),
            last_journal_seq: self.runtime.last_journal_seq().unwrap_or_default(),
            last_state_version: self.runtime.state_version(),
            published_snapshot_id: self.runtime.published_snapshot_id().map(str::to_owned),
        }
    }
}

fn snapshot_id_for(state_version: u64) -> String {
    format!("snapshot-{state_version}")
}

pub(crate) fn synthetic_event(journal_seq: i64) -> ExternalFactEvent {
    ExternalFactEvent::new(
        "replay",
        "supervisor-seed",
        &format!("evt-{journal_seq}"),
        "v1",
        Utc.with_ymd_and_hms(2026, 3, 25, 0, 0, 0)
            .single()
            .expect("static replay timestamp should be valid"),
    )
}

pub(crate) fn readiness_for(snapshot_id: &str) -> ProjectionReadiness {
    ProjectionReadiness::ready_fullset_pending_negrisk(snapshot_id)
}
