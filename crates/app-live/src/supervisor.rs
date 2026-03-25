use std::collections::{BTreeMap, BTreeSet};

use domain::{ExecutionMode, RuntimeMode};
use observability::{field_keys, span_names, RuntimeMetricsRecorder};
use state::{ApplyResult, PublishedSnapshot, RemoteSnapshot};
use tracing::field;

use crate::{
    bootstrap::{BootstrapStatus, StaticSnapshotSource},
    config::NegRiskFamilyLiveTarget,
    dispatch::{DispatchLoop, DispatchSummary},
    input_tasks::{InputTaskEvent, InputTaskQueue},
    instrumentation::AppInstrumentation,
    negrisk_live::{eligible_live_records, NegRiskLiveExecutionRecord},
    runtime::{AppRunResult, AppRuntime, AppRuntimeMode},
    snapshot_meta::{rollout_evidence_from_snapshot, snapshot_id_for},
};

const DIVERGENCE_PENDING_RECONCILE_COUNT_MISMATCH: &str = "pending_reconcile_count_mismatch";
const DIVERGENCE_STATE_VERSION_MISMATCH: &str = "state_version_mismatch";
const DIVERGENCE_LAST_JOURNAL_SEQ_MISMATCH: &str = "last_journal_seq_mismatch";
const DIVERGENCE_ROLLOUT_EVIDENCE_MISMATCH: &str = "rollout_evidence_mismatch";
const DIVERGENCE_ROLLOUT_EVIDENCE_MISSING: &str = "rollout_evidence_missing";
const DIVERGENCE_ROLLOUT_EVIDENCE_UNEXPECTED: &str = "rollout_evidence_unexpected";
const DIVERGENCE_NEG_RISK_LIVE_EXECUTION_ANCHORS_MISSING: &str =
    "neg_risk_live_execution_anchors_missing";
const DIVERGENCE_NEG_RISK_LIVE_EXECUTION_SNAPSHOT_MISSING: &str =
    "neg_risk_live_execution_snapshot_missing";
const DIVERGENCE_NEG_RISK_LIVE_EXECUTION_SNAPSHOT_MISMATCH: &str =
    "neg_risk_live_execution_snapshot_mismatch";

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
    metrics_recorder: Option<RuntimeMetricsRecorder>,
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
        Self::new_with_metrics(app_mode, bootstrap_snapshot, None)
    }

    pub fn new_instrumented(
        app_mode: AppRuntimeMode,
        bootstrap_snapshot: RemoteSnapshot,
        recorder: RuntimeMetricsRecorder,
    ) -> Self {
        Self::new_with_metrics(app_mode, bootstrap_snapshot, Some(recorder))
    }

    fn new_with_metrics(
        app_mode: AppRuntimeMode,
        bootstrap_snapshot: RemoteSnapshot,
        metrics_recorder: Option<RuntimeMetricsRecorder>,
    ) -> Self {
        Self {
            dispatcher: DispatchLoop::default(),
            runtime: AppRuntime::new_instrumented(
                app_mode,
                runtime_instrumentation(metrics_recorder.as_ref()),
            ),
            metrics_recorder,
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

    pub fn for_tests_instrumented(recorder: RuntimeMetricsRecorder) -> Self {
        Self::new_instrumented(AppRuntimeMode::Live, RemoteSnapshot::empty(), recorder)
    }

    pub fn run_once(&mut self) -> Result<SupervisorSummary, SupervisorError> {
        if self.runtime.bootstrap_status() != BootstrapStatus::Ready {
            let source = StaticSnapshotSource::new(self.bootstrap_snapshot.clone());
            self.runtime.bootstrap_once(&source);
        }

        let allow_operator_synthesis = self.allow_operator_rollout_evidence_synthesis();
        self.publish_current_snapshot(allow_operator_synthesis);
        self.refresh_neg_risk_live_execution_records(allow_operator_synthesis)?;
        let _ = self.flush_dispatch_instrumented();

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
        let _ = supervisor.flush_dispatch_instrumented();
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
        self.flush_dispatch_instrumented()
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
        let span = tracing::info_span!(
            span_names::APP_SUPERVISOR_RESUME,
            app_mode = field::Empty,
            backlog_count = field::Empty,
            processed_count = field::Empty,
            last_journal_seq = field::Empty,
            state_version = field::Empty,
            snapshot_id = field::Empty,
            pending_reconcile_count = field::Empty
        );
        let _span_guard = span.enter();
        span.record(field_keys::APP_MODE, self.runtime.app_mode().as_str());

        self.runtime = AppRuntime::new_instrumented(
            self.runtime.app_mode(),
            runtime_instrumentation(self.metrics_recorder.as_ref()),
        );
        self.neg_risk_rollout_evidence = None;
        self.neg_risk_live_execution_records = Vec::new();
        self.neg_risk_live_state_source = NegRiskLiveStateSource::None;
        self.record_recovery_backlog(self.input_tasks.len());

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
                return Err(self.divergence_error(
                    DIVERGENCE_PENDING_RECONCILE_COUNT_MISMATCH,
                    format!(
                        "durable pending reconcile count {} did not match rebuilt count {}",
                        expected,
                        self.runtime.pending_reconcile_count()
                    ),
                ));
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
            return Err(self.divergence_error(
                DIVERGENCE_STATE_VERSION_MISMATCH,
                format!(
                    "durable history rebuilt state version {} but expected {}",
                    self.runtime.state_version(),
                    committed_state_version
                ),
            ));
        }
        if self.runtime.last_journal_seq() != Some(last_journal_seq) {
            return Err(self.divergence_error(
                DIVERGENCE_LAST_JOURNAL_SEQ_MISMATCH,
                format!(
                    "durable history rebuilt last journal seq {} but expected {}",
                    self.runtime.last_journal_seq().unwrap_or_default(),
                    last_journal_seq
                ),
            ));
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

        let mut processed_count = 0usize;
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
                    processed_count += 1;
                    self.record_recovery_backlog(self.input_tasks.len());
                }
                ApplyResult::Duplicate { .. }
                | ApplyResult::Deferred { .. }
                | ApplyResult::ReconcileRequired { .. } => {
                    self.record_committed_input(input.clone());
                    let _ = self.input_tasks.remove(&input);
                    processed_count += 1;
                    self.record_recovery_backlog(self.input_tasks.len());
                }
            }
        }

        if self.runtime.published_snapshot_id().is_none() && self.runtime.state_version() > 0 {
            self.publish_current_snapshot(false);
        }

        self.validate_neg_risk_live_execution_anchor()?;
        span.record(field_keys::PROCESSED_COUNT, processed_count);
        let _ = self.flush_dispatch_instrumented();

        let summary = self.summary();
        span.record(field_keys::BACKLOG_COUNT, self.input_tasks.len());
        span.record(field_keys::LAST_JOURNAL_SEQ, summary.last_journal_seq);
        span.record(field_keys::STATE_VERSION, summary.last_state_version);
        span.record(
            field_keys::PENDING_RECONCILE_COUNT,
            summary.pending_reconcile_count,
        );
        if let Some(snapshot_id) = summary.published_snapshot_id.as_deref() {
            span.record(field_keys::SNAPSHOT_ID, snapshot_id);
        }

        Ok(summary)
    }

    fn summary(&self) -> SupervisorSummary {
        let neg_risk_live_attempt_count = self.neg_risk_live_execution_records.len();
        if let Some(evidence) = self.neg_risk_rollout_evidence.as_ref() {
            self.record_rollout_evidence(evidence);
        }
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
            let evidence = self.rollout_evidence_for_snapshot(&snapshot, allow_operator_synthesis);
            self.record_rollout_evidence(&evidence);
            self.neg_risk_rollout_evidence = Some(evidence);
            self.retain_current_neg_risk_live_execution_records();
            self.dispatcher.observe_snapshot(snapshot);
        } else {
            self.record_zero_rollout_evidence();
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
            (Some(expected), Some(actual)) => Err(self.divergence_error(
                DIVERGENCE_ROLLOUT_EVIDENCE_MISMATCH,
                format!(
                    "durable rollout gate evidence {:?} did not match rebuilt evidence {:?}",
                    expected, actual
                ),
            )),
            (Some(expected), None) => Err(self.divergence_error(
                DIVERGENCE_ROLLOUT_EVIDENCE_MISSING,
                format!(
                    "durable rollout gate evidence {:?} could not be rebuilt",
                    expected
                ),
            )),
            (None, Some(_)) => Err(self.divergence_error(
                DIVERGENCE_ROLLOUT_EVIDENCE_UNEXPECTED,
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
            self.metrics_recorder.clone(),
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
        if snapshot.negrisk.is_some() {
            return rollout_evidence_from_snapshot(snapshot);
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
            return Err(self.divergence_error(
                DIVERGENCE_NEG_RISK_LIVE_EXECUTION_ANCHORS_MISSING,
                "durable neg-risk live attempt anchors are required to resume live state",
            ));
        }

        let Some(snapshot_id) = self.runtime.published_snapshot_id() else {
            return Err(self.divergence_error(
                DIVERGENCE_NEG_RISK_LIVE_EXECUTION_SNAPSHOT_MISSING,
                "durable neg-risk live attempt anchors require a published snapshot",
            ));
        };
        if self
            .neg_risk_live_execution_records
            .iter()
            .any(|record| record.snapshot_id != snapshot_id)
        {
            return Err(self.divergence_error(
                DIVERGENCE_NEG_RISK_LIVE_EXECUTION_SNAPSHOT_MISMATCH,
                "durable neg-risk live attempt anchors did not match the rebuilt snapshot",
            ));
        }

        Ok(())
    }

    fn divergence_error(
        &self,
        divergence_kind: &'static str,
        message: impl Into<String>,
    ) -> SupervisorError {
        runtime_instrumentation(self.metrics_recorder.as_ref()).record_divergence(divergence_kind);
        SupervisorError::new(message)
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

    fn record_recovery_backlog(&self, backlog_count: usize) {
        let Some(recorder) = &self.metrics_recorder else {
            return;
        };

        recorder.record_recovery_backlog_count(backlog_count as f64);
    }

    fn record_rollout_evidence(&self, evidence: &NegRiskRolloutEvidence) {
        let Some(recorder) = &self.metrics_recorder else {
            return;
        };

        recorder.record_neg_risk_live_ready_family_count(evidence.live_ready_family_count as f64);
        recorder.record_neg_risk_live_gate_block_count(evidence.blocked_family_count as f64);
    }

    fn record_zero_rollout_evidence(&self) {
        let Some(recorder) = &self.metrics_recorder else {
            return;
        };

        recorder.record_neg_risk_live_ready_family_count(0.0);
        recorder.record_neg_risk_live_gate_block_count(0.0);
    }

    fn flush_dispatch_instrumented(&mut self) -> DispatchSummary {
        let span = tracing::info_span!(
            span_names::APP_DISPATCH_FLUSH,
            backlog_count = field::Empty,
            processed_count = field::Empty,
            state_version = field::Empty,
            snapshot_id = field::Empty
        );
        let _span_guard = span.enter();

        let backlog_count = self.dispatcher.pending_backlog_count();
        span.record(field_keys::BACKLOG_COUNT, backlog_count);

        let summary = self.dispatcher.flush();
        if let Some(recorder) = &self.metrics_recorder {
            recorder
                .record_dispatcher_backlog_count(self.dispatcher.pending_backlog_count() as f64);
        }
        span.record(
            field_keys::PROCESSED_COUNT,
            summary.coalesced_versions.len(),
        );

        let state_version = summary
            .fullset_last_ready_state_version
            .or(summary.negrisk_last_ready_state_version)
            .or_else(|| summary.coalesced_versions.last().copied());
        if let Some(state_version) = state_version {
            span.record(field_keys::STATE_VERSION, state_version);
        }

        let snapshot_id = summary
            .fullset_last_ready_snapshot_id
            .as_deref()
            .or(summary.negrisk_last_ready_snapshot_id.as_deref())
            .or(summary.last_stable_snapshot_id.as_deref());
        if let Some(snapshot_id) = snapshot_id {
            span.record(field_keys::SNAPSHOT_ID, snapshot_id);
        }

        summary
    }
}

fn runtime_instrumentation(recorder: Option<&RuntimeMetricsRecorder>) -> AppInstrumentation {
    match recorder.cloned() {
        Some(recorder) => AppInstrumentation::enabled(recorder),
        None => AppInstrumentation::disabled(),
    }
}
