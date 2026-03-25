use std::collections::{BTreeMap, BTreeSet};

use domain::{ExecutionMode, RuntimeMode};
use state::{ApplyResult, PublishedSnapshot, RemoteSnapshot};

use crate::{
    bootstrap::{BootstrapStatus, StaticSnapshotSource},
    config::NegRiskFamilyLiveTarget,
    dispatch::{DispatchLoop, DispatchSummary},
    input_tasks::{InputTaskEvent, InputTaskQueue},
    negrisk_live::{eligible_live_records, NegRiskLiveExecutionRecord},
    runtime::{AppRunResult, AppRuntime, AppRuntimeMode},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorSummary {
    pub fullset_mode: ExecutionMode,
    pub negrisk_mode: ExecutionMode,
    pub neg_risk_live_attempt_count: usize,
    pub neg_risk_live_state_source: NegRiskLiveStateSource,
    pub bootstrap_status: BootstrapStatus,
    pub runtime_mode: RuntimeMode,
    pub pending_reconcile_count: usize,
    pub last_journal_seq: i64,
    pub last_state_version: u64,
    pub published_snapshot_id: Option<String>,
    pub published_snapshot_committed_journal_seq: Option<i64>,
    pub neg_risk_rollout_evidence: Option<NegRiskRolloutEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NegRiskLiveStateSource {
    #[default]
    None,
    SyntheticBootstrap,
    DurableRestore,
}

impl NegRiskLiveStateSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::SyntheticBootstrap => "synthetic_bootstrap",
            Self::DurableRestore => "durable_restore",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NegRiskRolloutEvidence {
    pub snapshot_id: String,
    pub live_ready_family_count: usize,
    pub blocked_family_count: usize,
    pub parity_mismatch_count: u64,
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
    pending_reconcile_count: Option<usize>,
    neg_risk_rollout_evidence: Option<NegRiskRolloutEvidence>,
    neg_risk_live_execution_records: Vec<NegRiskLiveExecutionRecord>,
}

pub struct AppSupervisor {
    dispatcher: DispatchLoop,
    runtime: AppRuntime,
    bootstrap_snapshot: RemoteSnapshot,
    committed_log: Vec<InputTaskEvent>,
    input_tasks: InputTaskQueue,
    seed: RuntimeSeed,
    neg_risk_live_targets: BTreeMap<String, NegRiskFamilyLiveTarget>,
    neg_risk_live_approved_families: BTreeSet<String>,
    neg_risk_live_ready_families: BTreeSet<String>,
    neg_risk_rollout_evidence: Option<NegRiskRolloutEvidence>,
    neg_risk_live_execution_records: Vec<NegRiskLiveExecutionRecord>,
    neg_risk_live_state_source: NegRiskLiveStateSource,
}

impl AppSupervisor {
    pub fn new(app_mode: AppRuntimeMode, bootstrap_snapshot: RemoteSnapshot) -> Self {
        Self {
            dispatcher: DispatchLoop::default(),
            runtime: AppRuntime::new(app_mode),
            bootstrap_snapshot,
            committed_log: Vec::new(),
            input_tasks: InputTaskQueue::default(),
            seed: RuntimeSeed::default(),
            neg_risk_live_targets: BTreeMap::new(),
            neg_risk_live_approved_families: BTreeSet::new(),
            neg_risk_live_ready_families: BTreeSet::new(),
            neg_risk_rollout_evidence: None,
            neg_risk_live_execution_records: Vec::new(),
            neg_risk_live_state_source: NegRiskLiveStateSource::None,
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

        let allow_operator_synthesis = self.allow_operator_rollout_evidence_synthesis();
        self.publish_current_snapshot(allow_operator_synthesis);
        self.refresh_neg_risk_live_execution_records(allow_operator_synthesis)?;
        let _ = self.dispatcher.flush();

        Ok(self.summary())
    }

    pub fn run_bootstrap(self) -> AppRunResult {
        let mut supervisor = self;
        let source = StaticSnapshotSource::new(supervisor.bootstrap_snapshot.clone());
        let report = supervisor.runtime.bootstrap_once(&source);
        let allow_operator_synthesis = supervisor.allow_operator_rollout_evidence_synthesis();
        supervisor.publish_current_snapshot(allow_operator_synthesis);
        supervisor
            .refresh_neg_risk_live_execution_records(allow_operator_synthesis)
            .expect("bootstrap should build neg-risk live execution records");
        let _ = supervisor.dispatcher.flush();
        let summary = supervisor.summary();

        AppRunResult {
            runtime: supervisor.runtime,
            report,
            summary,
        }
    }

    pub fn push_dirty_snapshot(
        &mut self,
        state_version: u64,
        fullset_ready: bool,
        negrisk_ready: bool,
    ) {
        self.dispatcher
            .push_test_snapshot(state_version, fullset_ready, negrisk_ready);
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

    pub fn seed_pending_reconcile_count(&mut self, pending_reconcile_count: usize) {
        self.seed.pending_reconcile_count = Some(pending_reconcile_count);
    }

    pub fn seed_neg_risk_rollout_evidence(&mut self, evidence: NegRiskRolloutEvidence) {
        self.seed.neg_risk_rollout_evidence = Some(evidence);
    }

    pub fn seed_neg_risk_live_targets(
        &mut self,
        targets: BTreeMap<String, NegRiskFamilyLiveTarget>,
    ) {
        self.neg_risk_live_targets = targets;
    }

    pub fn seed_neg_risk_live_approval(&mut self, family_id: &str) {
        self.neg_risk_live_approved_families
            .insert(family_id.to_owned());
    }

    pub fn seed_neg_risk_live_ready_family(&mut self, family_id: &str) {
        self.neg_risk_live_ready_families
            .insert(family_id.to_owned());
    }

    pub fn seed_neg_risk_live_execution_record(&mut self, record: NegRiskLiveExecutionRecord) {
        self.seed.neg_risk_live_execution_records.push(record);
    }

    pub fn neg_risk_live_execution_records(&self) -> &[NegRiskLiveExecutionRecord] {
        &self.neg_risk_live_execution_records
    }

    pub fn seed_committed_input(&mut self, input: InputTaskEvent) {
        self.record_committed_input(input);
    }

    pub fn seed_unapplied_journal_entry(&mut self, journal_seq: i64, input: InputTaskEvent) {
        let mut input = input;
        input.journal_seq = journal_seq;
        self.input_tasks.push(input);
    }

    pub fn pending_input_count(&self) -> usize {
        self.input_tasks.len()
    }

    pub fn resume_once(&mut self) -> Result<SupervisorSummary, SupervisorError> {
        self.runtime = AppRuntime::new(self.runtime.app_mode());
        self.neg_risk_rollout_evidence = None;
        self.neg_risk_live_execution_records = Vec::new();
        self.neg_risk_live_state_source = NegRiskLiveStateSource::None;

        let committed_state_version = self
            .seed
            .committed_state_version
            .unwrap_or(self.seed.last_state_version);
        let last_journal_seq = match self.seed.last_journal_seq {
            Some(last_journal_seq) => last_journal_seq,
            None if committed_state_version == 0 && self.committed_log.is_empty() => 0,
            None => {
                return Err(SupervisorError::new(
                    "durable last journal sequence is required to resume committed state",
                ));
            }
        };
        self.runtime.replay_committed_history(&self.committed_log)?;
        match self.seed.pending_reconcile_count {
            Some(0) => self.runtime.clear_pending_reconcile_after_restore(),
            Some(expected) if self.runtime.pending_reconcile_count() != expected => {
                return Err(SupervisorError::new(format!(
                    "durable pending reconcile count {} did not match rebuilt count {}",
                    expected,
                    self.runtime.pending_reconcile_count()
                )));
            }
            Some(_) => {}
            None if self.runtime.pending_reconcile_count() == 0 => {}
            None => {
                return Err(SupervisorError::new(
                    "durable pending reconcile count is required to resume pending follow-up work",
                ));
            }
        }
        if self.runtime.state_version() != committed_state_version {
            return Err(SupervisorError::new(format!(
                "durable history rebuilt state version {} but expected {}",
                self.runtime.state_version(),
                committed_state_version
            )));
        }
        if self.runtime.last_journal_seq() != Some(last_journal_seq) {
            return Err(SupervisorError::new(format!(
                "durable history rebuilt last journal seq {} but expected {}",
                self.runtime.last_journal_seq().unwrap_or_default(),
                last_journal_seq
            )));
        }

        if self.runtime.state_version() > 0
            || self.seed.published_snapshot_id.is_some()
            || self.seed.last_state_version == 0
        {
            self.publish_current_snapshot(false);
        }
        self.neg_risk_live_execution_records = self.seed.neg_risk_live_execution_records.clone();
        self.neg_risk_live_state_source = if self.neg_risk_live_execution_records.is_empty() {
            NegRiskLiveStateSource::None
        } else {
            NegRiskLiveStateSource::DurableRestore
        };
        self.retain_current_neg_risk_live_execution_records();
        self.validate_rollout_evidence_anchor()?;

        while let Some(input) = self.input_tasks.next_after(self.seed.last_journal_seq) {
            match self.runtime.apply_input(input.clone())? {
                ApplyResult::Applied {
                    state_version,
                    dirty_set,
                    ..
                } => {
                    self.dispatcher.record_apply(state_version, dirty_set);
                    self.record_committed_input(input.clone());
                    let _ = self.input_tasks.remove(&input);
                    self.publish_current_snapshot(false);
                }
                ApplyResult::Duplicate { .. }
                | ApplyResult::Deferred { .. }
                | ApplyResult::ReconcileRequired { .. } => {
                    self.record_committed_input(input.clone());
                    let _ = self.input_tasks.remove(&input);
                }
            }
        }

        if self.runtime.published_snapshot_id().is_none() && self.runtime.state_version() > 0 {
            self.publish_current_snapshot(false);
        }

        self.validate_neg_risk_live_execution_anchor()?;
        let _ = self.dispatcher.flush();

        Ok(self.summary())
    }

    fn summary(&self) -> SupervisorSummary {
        let neg_risk_live_attempt_count = self.neg_risk_live_execution_records.len();
        SupervisorSummary {
            fullset_mode: ExecutionMode::Live,
            negrisk_mode: if neg_risk_live_attempt_count > 0 {
                ExecutionMode::Live
            } else {
                ExecutionMode::Shadow
            },
            neg_risk_live_attempt_count,
            neg_risk_live_state_source: self.neg_risk_live_state_source,
            bootstrap_status: self.runtime.bootstrap_status(),
            runtime_mode: self.runtime.runtime_mode(),
            pending_reconcile_count: self.runtime.pending_reconcile_count(),
            last_journal_seq: self.runtime.last_journal_seq().unwrap_or_default(),
            last_state_version: self.runtime.state_version(),
            published_snapshot_id: self.runtime.published_snapshot_id().map(str::to_owned),
            published_snapshot_committed_journal_seq: self
                .runtime
                .published_snapshot_committed_journal_seq(),
            neg_risk_rollout_evidence: self.neg_risk_rollout_evidence.clone(),
        }
    }

    fn publish_current_snapshot(&mut self, allow_operator_synthesis: bool) {
        if let Some(snapshot) = self
            .runtime
            .publish_snapshot(&snapshot_id_for(self.runtime.state_version()))
        {
            self.neg_risk_rollout_evidence =
                Some(self.rollout_evidence_for_snapshot(&snapshot, allow_operator_synthesis));
            self.retain_current_neg_risk_live_execution_records();
            self.dispatcher.observe_snapshot(snapshot);
        } else {
            self.neg_risk_rollout_evidence = None;
            self.retain_current_neg_risk_live_execution_records();
        }
    }

    fn validate_rollout_evidence_anchor(&self) -> Result<(), SupervisorError> {
        if self.runtime.app_mode() != AppRuntimeMode::Live {
            return Ok(());
        }

        let restoring_empty_history = self.seed.last_journal_seq.is_none()
            && self
                .seed
                .committed_state_version
                .unwrap_or(self.seed.last_state_version)
                == 0
            && self.runtime.last_journal_seq() == Some(0)
            && self.runtime.state_version() == 0;
        if restoring_empty_history && self.seed.neg_risk_rollout_evidence.is_none() {
            return Ok(());
        }

        if self.runtime.state_version() == 0
            && self.seed.published_snapshot_id.is_none()
            && self.neg_risk_rollout_evidence.is_none()
        {
            return Ok(());
        }

        match (
            self.seed.neg_risk_rollout_evidence.as_ref(),
            self.neg_risk_rollout_evidence.as_ref(),
        ) {
            (Some(expected), Some(actual)) if expected == actual => Ok(()),
            (Some(expected), Some(actual)) => Err(SupervisorError::new(format!(
                "durable rollout gate evidence {:?} did not match rebuilt evidence {:?}",
                expected, actual
            ))),
            (Some(expected), None) => Err(SupervisorError::new(format!(
                "durable rollout gate evidence {:?} could not be rebuilt",
                expected
            ))),
            (None, Some(_)) => Err(SupervisorError::new(
                "durable rollout gate evidence is required to resume live state",
            )),
            (None, None) => Ok(()),
        }
    }

    fn record_committed_input(&mut self, input: InputTaskEvent) {
        if self.committed_log.iter().any(|entry| entry == &input) {
            return;
        }

        self.committed_log.push(input);
        self.committed_log.sort_by_key(|entry| entry.journal_seq);
    }

    fn allow_operator_rollout_evidence_synthesis(&self) -> bool {
        self.seed.last_journal_seq.is_none()
            && self.seed.committed_state_version.is_none()
            && self.seed.published_snapshot_id.is_none()
            && self.committed_log.is_empty()
            && self.runtime.last_journal_seq() == Some(0)
            && self.runtime.state_version() == 0
    }

    fn refresh_neg_risk_live_execution_records(
        &mut self,
        allow_operator_synthesis: bool,
    ) -> Result<(), SupervisorError> {
        if self.runtime.app_mode() != AppRuntimeMode::Live
            || !allow_operator_synthesis
            || !self.neg_risk_live_execution_records.is_empty()
        {
            return Ok(());
        }

        let Some(snapshot_id) = self.runtime.published_snapshot_id() else {
            return Ok(());
        };
        let Some(rollout_evidence) = self.neg_risk_rollout_evidence.as_ref() else {
            return Ok(());
        };
        if rollout_evidence.live_ready_family_count == 0 {
            return Ok(());
        }

        self.neg_risk_live_execution_records = eligible_live_records(
            snapshot_id,
            &self.neg_risk_live_targets,
            &self.neg_risk_live_approved_families,
            &self.neg_risk_live_ready_families,
        )
        .map_err(|err| SupervisorError::new(err.to_string()))?;
        self.neg_risk_live_state_source = if self.neg_risk_live_execution_records.is_empty() {
            NegRiskLiveStateSource::None
        } else {
            NegRiskLiveStateSource::SyntheticBootstrap
        };
        Ok(())
    }

    fn retain_current_neg_risk_live_execution_records(&mut self) {
        let Some(snapshot_id) = self.runtime.published_snapshot_id() else {
            self.neg_risk_live_execution_records.clear();
            self.neg_risk_live_state_source = NegRiskLiveStateSource::None;
            return;
        };
        let evidence_snapshot_id = self
            .neg_risk_rollout_evidence
            .as_ref()
            .map(|evidence| evidence.snapshot_id.as_str())
            .unwrap_or(snapshot_id);

        self.neg_risk_live_execution_records.retain(|record| {
            record.snapshot_id == snapshot_id && record.snapshot_id == evidence_snapshot_id
        });
        if self.neg_risk_live_execution_records.is_empty() {
            self.neg_risk_live_state_source = NegRiskLiveStateSource::None;
        }
    }

    fn rollout_evidence_for_snapshot(
        &self,
        snapshot: &PublishedSnapshot,
        allow_operator_synthesis: bool,
    ) -> NegRiskRolloutEvidence {
        if let Some(evidence) = rollout_evidence_from_snapshot(snapshot) {
            return evidence;
        }

        if !allow_operator_synthesis {
            if let Some(evidence) = self.seed.neg_risk_rollout_evidence.as_ref() {
                if evidence.snapshot_id == snapshot.snapshot_id {
                    return evidence.clone();
                }
            }
        }

        if allow_operator_synthesis {
            let live_ready_family_count = self.synthetic_live_ready_family_count();
            return NegRiskRolloutEvidence {
                snapshot_id: snapshot.snapshot_id.clone(),
                live_ready_family_count,
                blocked_family_count: self
                    .neg_risk_live_targets
                    .len()
                    .saturating_sub(live_ready_family_count),
                parity_mismatch_count: 0,
            };
        }

        NegRiskRolloutEvidence {
            snapshot_id: snapshot.snapshot_id.clone(),
            ..NegRiskRolloutEvidence::default()
        }
    }

    fn validate_neg_risk_live_execution_anchor(&self) -> Result<(), SupervisorError> {
        if self.runtime.app_mode() != AppRuntimeMode::Live {
            return Ok(());
        }

        let live_ready_count = self
            .neg_risk_rollout_evidence
            .as_ref()
            .map(|evidence| evidence.live_ready_family_count)
            .unwrap_or_default();
        if live_ready_count == 0 {
            return Ok(());
        }

        if self.neg_risk_live_execution_records.is_empty() {
            return Err(SupervisorError::new(
                "durable neg-risk live attempt anchors are required to resume live state",
            ));
        }

        let Some(snapshot_id) = self.runtime.published_snapshot_id() else {
            return Err(SupervisorError::new(
                "durable neg-risk live attempt anchors require a published snapshot",
            ));
        };
        if self
            .neg_risk_live_execution_records
            .iter()
            .any(|record| record.snapshot_id != snapshot_id)
        {
            return Err(SupervisorError::new(
                "durable neg-risk live attempt anchors did not match the rebuilt snapshot",
            ));
        }

        Ok(())
    }

    fn synthetic_live_ready_family_count(&self) -> usize {
        self.neg_risk_live_targets
            .keys()
            .filter(|family_id| {
                self.neg_risk_live_ready_families.contains(*family_id)
                    && self.neg_risk_live_approved_families.contains(*family_id)
            })
            .count()
    }
}

fn snapshot_id_for(state_version: u64) -> String {
    format!("snapshot-{state_version}")
}

fn rollout_evidence_from_snapshot(snapshot: &PublishedSnapshot) -> Option<NegRiskRolloutEvidence> {
    let negrisk = snapshot.negrisk.as_ref()?;

    let live_ready_family_count = negrisk
        .families
        .iter()
        .filter(|family| {
            family.shadow_parity_ready
                && family.recovery_ready
                && family.replay_drift_ready
                && family.fault_injection_ready
                && family.conversion_path_ready
                && family.halt_semantics_ready
        })
        .count();
    let parity_mismatch_count = negrisk
        .families
        .iter()
        .filter(|family| !family.shadow_parity_ready)
        .count() as u64;

    Some(NegRiskRolloutEvidence {
        snapshot_id: snapshot.snapshot_id.clone(),
        live_ready_family_count,
        blocked_family_count: negrisk
            .families
            .len()
            .saturating_sub(live_ready_family_count),
        parity_mismatch_count,
    })
}
