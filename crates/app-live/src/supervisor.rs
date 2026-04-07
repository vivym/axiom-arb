use std::collections::{BTreeMap, BTreeSet};

use chrono::Utc;
use domain::{ExecutionMode, RuntimeMode};
use observability::{field_keys, span_names, RuntimeMetricsRecorder};
use persistence::models::{ExecutionAttemptRow, ShadowExecutionArtifactRow};
use state::{ApplyResult, PendingReconcileAnchor, PublishedSnapshot, RemoteSnapshot};
use tracing::field;

use crate::{
    bootstrap::{BootstrapStatus, StaticSnapshotSource},
    config::{neg_risk_live_target_revision_from_targets, NegRiskFamilyLiveTarget},
    discovery::DiscoverySupervisor,
    dispatch::{DispatchLoop, DispatchSummary},
    input_tasks::InputTaskEvent,
    instrumentation::AppInstrumentation,
    negrisk_live::{
        eligible_live_records, eligible_live_records_with_backend, NegRiskLiveExecutionBackend,
        NegRiskLiveExecutionRecord,
    },
    negrisk_shadow::eligible_shadow_records_with_run_session_id,
    posture::SupervisorPosture,
    queues::{CandidateNotice, CandidateRestrictionTruth, IngressQueue},
    runtime::{
        persist_live_execution_records_with_run_session_id,
        persist_shadow_execution_records_with_run_session_id, AppRunResult, AppRuntime,
        AppRuntimeMode,
    },
    snapshot_meta::{rollout_evidence_from_snapshot, snapshot_id_for},
    task_groups::MetadataDiscoveryBatch,
};
use state::DirtyDomain;

const DIVERGENCE_PENDING_RECONCILE_COUNT_MISMATCH: &str = "pending_reconcile_count_mismatch";
const DIVERGENCE_STATE_VERSION_MISMATCH: &str = "state_version_mismatch";
const DIVERGENCE_LAST_JOURNAL_SEQ_MISMATCH: &str = "last_journal_seq_mismatch";
const DIVERGENCE_ROLLOUT_EVIDENCE_MISMATCH: &str = "rollout_evidence_mismatch";
const DIVERGENCE_ROLLOUT_EVIDENCE_MISSING: &str = "rollout_evidence_missing";
const DIVERGENCE_NEG_RISK_LIVE_EXECUTION_SNAPSHOT_MISMATCH: &str =
    "neg_risk_live_execution_snapshot_mismatch";
const DIVERGENCE_NEG_RISK_SHADOW_EXECUTION_SNAPSHOT_MISMATCH: &str =
    "neg_risk_shadow_execution_snapshot_mismatch";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorSummary {
    pub fullset_mode: ExecutionMode,
    pub negrisk_mode: ExecutionMode,
    pub real_user_shadow_smoke: bool,
    pub neg_risk_live_attempt_count: usize,
    pub neg_risk_live_state_source: NegRiskLiveStateSource,
    pub neg_risk_rollout_evidence_source: NegRiskRolloutEvidenceSource,
    pub bootstrap_status: BootstrapStatus,
    pub runtime_mode: RuntimeMode,
    pub pending_reconcile_count: usize,
    pub last_journal_seq: i64,
    pub last_state_version: u64,
    pub published_snapshot_id: Option<String>,
    pub published_snapshot_committed_journal_seq: Option<i64>,
    pub latest_candidate_revision: Option<String>,
    pub latest_adoptable_revision: Option<String>,
    pub latest_candidate_operator_target_revision: Option<String>,
    pub adoption_provenance_resolved: bool,
    pub neg_risk_rollout_evidence: Option<NegRiskRolloutEvidence>,
    pub global_posture: SupervisorPosture,
    pub ingress_backlog_count: usize,
    pub follow_up_backlog_count: usize,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NegRiskRolloutEvidenceSource {
    #[default]
    None,
    Bootstrap,
    Neutral,
    Snapshot,
}

impl NegRiskRolloutEvidenceSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Bootstrap => "bootstrap",
            Self::Neutral => "neutral",
            Self::Snapshot => "snapshot",
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CandidateRestoreStatus {
    pub latest_candidate_revision: Option<String>,
    pub latest_adoptable_revision: Option<String>,
    pub latest_candidate_operator_target_revision: Option<String>,
    pub adoption_provenance_resolved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorError {
    message: String,
}

impl SupervisorError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
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

#[derive(Debug, Clone, PartialEq, Default)]
struct RuntimeSeed {
    last_journal_seq: Option<i64>,
    last_state_version: u64,
    published_snapshot_id: Option<String>,
    committed_state_version: Option<u64>,
    pending_reconcile_count: Option<usize>,
    pending_reconcile_anchors: Vec<PendingReconcileAnchor>,
    candidate_restore_status: CandidateRestoreStatus,
    neg_risk_rollout_evidence: Option<NegRiskRolloutEvidence>,
    neg_risk_live_execution_records: Vec<NegRiskLiveExecutionRecord>,
    neg_risk_shadow_execution_attempts: Vec<ExecutionAttemptRow>,
    neg_risk_shadow_execution_artifacts: Vec<ShadowExecutionArtifactRow>,
}

pub struct AppSupervisor {
    dispatcher: DispatchLoop,
    posture: SupervisorPosture,
    runtime: AppRuntime,
    metrics_recorder: Option<RuntimeMetricsRecorder>,
    bootstrap_snapshot: RemoteSnapshot,
    committed_log: Vec<InputTaskEvent>,
    input_tasks: IngressQueue,
    seed: RuntimeSeed,
    real_user_shadow_smoke_enabled: bool,
    durable_live_persistence_enabled: bool,
    durable_shadow_persistence_enabled: bool,
    run_session_id: Option<String>,
    neg_risk_live_targets: BTreeMap<String, NegRiskFamilyLiveTarget>,
    neg_risk_live_target_revision: Option<String>,
    neg_risk_live_approved_families: BTreeSet<String>,
    neg_risk_live_ready_families: BTreeSet<String>,
    neg_risk_live_execution_backend: Option<Box<dyn NegRiskLiveExecutionBackend>>,
    neg_risk_rollout_evidence: Option<NegRiskRolloutEvidence>,
    neg_risk_rollout_evidence_source: NegRiskRolloutEvidenceSource,
    last_emitted_rollout_evidence: Option<NegRiskRolloutEvidence>,
    candidate_restore_status: CandidateRestoreStatus,
    neg_risk_live_execution_records: Vec<NegRiskLiveExecutionRecord>,
    neg_risk_live_state_source: NegRiskLiveStateSource,
    neg_risk_shadow_execution_attempts: Vec<ExecutionAttemptRow>,
    neg_risk_shadow_execution_artifacts: Vec<ShadowExecutionArtifactRow>,
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
            posture: SupervisorPosture::Healthy,
            runtime: AppRuntime::new_instrumented(
                app_mode,
                runtime_instrumentation(metrics_recorder.as_ref()),
            ),
            metrics_recorder,
            bootstrap_snapshot,
            committed_log: Vec::new(),
            input_tasks: IngressQueue::default(),
            seed: RuntimeSeed::default(),
            real_user_shadow_smoke_enabled: false,
            durable_live_persistence_enabled: false,
            durable_shadow_persistence_enabled: false,
            run_session_id: None,
            neg_risk_live_targets: BTreeMap::new(),
            neg_risk_live_target_revision: None,
            neg_risk_live_approved_families: BTreeSet::new(),
            neg_risk_live_ready_families: BTreeSet::new(),
            neg_risk_live_execution_backend: None,
            neg_risk_rollout_evidence: None,
            neg_risk_rollout_evidence_source: NegRiskRolloutEvidenceSource::None,
            last_emitted_rollout_evidence: None,
            candidate_restore_status: CandidateRestoreStatus::default(),
            neg_risk_live_execution_records: Vec::new(),
            neg_risk_live_state_source: NegRiskLiveStateSource::None,
            neg_risk_shadow_execution_attempts: Vec::new(),
            neg_risk_shadow_execution_artifacts: Vec::new(),
        }
    }

    pub fn for_tests() -> Self {
        Self::new(AppRuntimeMode::Live, RemoteSnapshot::empty())
    }

    pub fn for_tests_instrumented(recorder: RuntimeMetricsRecorder) -> Self {
        Self::new_instrumented(AppRuntimeMode::Live, RemoteSnapshot::empty(), recorder)
    }

    pub fn posture(&self) -> SupervisorPosture {
        self.posture
    }

    pub fn app_mode(&self) -> AppRuntimeMode {
        self.runtime.app_mode()
    }

    pub fn can_resume_ingest_loops(&self) -> bool {
        self.runtime.follow_up_backlog_count() == 0 && self.input_tasks.is_empty()
    }

    pub(crate) fn startup_phase_label(&self) -> &'static str {
        let restoring_seeded_startup = self.runtime.bootstrap_status() != BootstrapStatus::Ready
            && self.has_seeded_startup_state();
        if restoring_seeded_startup {
            "restore"
        } else {
            "bootstrap"
        }
    }

    pub fn run_once(&mut self) -> Result<SupervisorSummary, SupervisorError> {
        self.last_emitted_rollout_evidence = None;
        let restoring_seeded_startup = self.runtime.bootstrap_status() != BootstrapStatus::Ready
            && self.has_seeded_startup_state();
        if restoring_seeded_startup {
            self.restore_seeded_startup_state()?;
        } else if self.runtime.bootstrap_status() != BootstrapStatus::Ready {
            let source = StaticSnapshotSource::new(self.bootstrap_snapshot.clone());
            self.runtime.bootstrap_once(&source);
        }

        let allow_operator_synthesis = self.allow_operator_rollout_evidence_synthesis();
        self.publish_current_snapshot(allow_operator_synthesis);
        self.refresh_neg_risk_live_execution_records(allow_operator_synthesis)?;
        if restoring_seeded_startup {
            self.validate_seeded_startup_restore()?;
        }
        self.drain_input_tasks()?;
        self.validate_neg_risk_live_execution_anchor()?;
        let _ = self.flush_dispatch_instrumented();

        Ok(self.summary())
    }

    pub fn run_startup(self) -> Result<AppRunResult, SupervisorError> {
        let mut supervisor = self;
        let summary = supervisor.run_once()?;

        Ok(AppRunResult {
            runtime: supervisor.runtime,
            report: state::ReconcileReport {
                succeeded: true,
                promoted_from_bootstrap: false,
                remote_applied: false,
                attention: Vec::new(),
            },
            summary,
        })
    }

    pub fn run_bootstrap(self) -> AppRunResult {
        let mut supervisor = self;
        supervisor.last_emitted_rollout_evidence = None;
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

    pub fn materialize_authoritative_discovery_batch(
        batch: MetadataDiscoveryBatch,
        run_session_id: &str,
    ) -> Result<SupervisorSummary, SupervisorError> {
        let mut supervisor = Self::new(AppRuntimeMode::Live, RemoteSnapshot::empty());
        supervisor.set_run_session_id(run_session_id);
        supervisor.run_authoritative_discovery_once(batch.rendered_live_targets, batch.inputs)
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

    pub fn seed_pending_reconcile_anchor(&mut self, anchor: PendingReconcileAnchor) {
        self.seed.pending_reconcile_anchors.push(anchor);
    }

    pub fn seed_neg_risk_rollout_evidence(&mut self, evidence: NegRiskRolloutEvidence) {
        self.seed.neg_risk_rollout_evidence = Some(evidence);
    }

    pub fn seed_candidate_restore_status(
        &mut self,
        latest_candidate_revision: Option<&str>,
        latest_adoptable_revision: Option<&str>,
        latest_candidate_operator_target_revision: Option<&str>,
        adoption_provenance_resolved: bool,
    ) {
        self.seed.candidate_restore_status = CandidateRestoreStatus {
            latest_candidate_revision: latest_candidate_revision.map(str::to_owned),
            latest_adoptable_revision: latest_adoptable_revision.map(str::to_owned),
            latest_candidate_operator_target_revision: latest_candidate_operator_target_revision
                .map(str::to_owned),
            adoption_provenance_resolved,
        };
        self.candidate_restore_status = self.seed.candidate_restore_status.clone();
    }

    pub fn seed_neg_risk_live_targets(
        &mut self,
        targets: BTreeMap<String, NegRiskFamilyLiveTarget>,
    ) {
        self.neg_risk_live_target_revision =
            (!targets.is_empty()).then(|| neg_risk_live_target_revision_from_targets(&targets));
        self.neg_risk_live_targets = targets;
    }

    pub fn run_authoritative_discovery_once(
        &mut self,
        rendered_live_targets: BTreeMap<String, NegRiskFamilyLiveTarget>,
        inputs: Vec<InputTaskEvent>,
    ) -> Result<SupervisorSummary, SupervisorError> {
        self.seed_neg_risk_live_targets(rendered_live_targets);
        for input in inputs {
            self.seed_unapplied_journal_entry(input.journal_seq, input);
        }
        self.last_emitted_rollout_evidence = None;
        if self.runtime.bootstrap_status() != BootstrapStatus::Ready {
            let source = StaticSnapshotSource::new(self.bootstrap_snapshot.clone());
            self.runtime.bootstrap_once(&source);
        }

        let allow_operator_synthesis = self.allow_operator_rollout_evidence_synthesis();
        self.publish_current_snapshot(allow_operator_synthesis);
        self.refresh_neg_risk_live_execution_records(allow_operator_synthesis)?;
        self.drain_input_tasks_authoritative_discovery()?;
        self.validate_neg_risk_live_execution_anchor()?;
        let _ = self.flush_dispatch_instrumented();

        Ok(self.summary())
    }

    pub fn enable_real_user_shadow_smoke(&mut self) {
        self.real_user_shadow_smoke_enabled = true;
    }

    pub fn enable_durable_shadow_persistence(&mut self) {
        self.durable_shadow_persistence_enabled = true;
    }

    pub fn enable_durable_live_persistence(&mut self) {
        self.durable_live_persistence_enabled = true;
    }

    pub fn set_run_session_id(&mut self, run_session_id: &str) {
        self.run_session_id = Some(run_session_id.to_owned());
    }

    pub fn seed_neg_risk_live_approval(&mut self, family_id: &str) {
        self.neg_risk_live_approved_families
            .insert(family_id.to_owned());
    }

    pub fn seed_neg_risk_live_ready_family(&mut self, family_id: &str) {
        self.neg_risk_live_ready_families
            .insert(family_id.to_owned());
    }

    #[cfg(test)]
    pub(crate) fn set_neg_risk_live_execution_backend(
        &mut self,
        backend: impl NegRiskLiveExecutionBackend + 'static,
    ) {
        self.neg_risk_live_execution_backend = Some(Box::new(backend));
    }

    pub(crate) fn set_neg_risk_live_execution_backend_boxed(
        &mut self,
        backend: Box<dyn NegRiskLiveExecutionBackend>,
    ) {
        self.neg_risk_live_execution_backend = Some(backend);
    }

    pub fn seed_neg_risk_live_execution_record(&mut self, record: NegRiskLiveExecutionRecord) {
        self.seed.neg_risk_live_execution_records.push(record);
    }

    pub fn seed_neg_risk_shadow_execution_attempt(&mut self, attempt: ExecutionAttemptRow) {
        self.seed.neg_risk_shadow_execution_attempts.push(attempt);
    }

    pub fn seed_neg_risk_shadow_execution_artifact(
        &mut self,
        artifact: ShadowExecutionArtifactRow,
    ) {
        self.seed.neg_risk_shadow_execution_artifacts.push(artifact);
    }

    pub fn neg_risk_live_execution_records(&self) -> &[NegRiskLiveExecutionRecord] {
        &self.neg_risk_live_execution_records
    }

    pub fn neg_risk_shadow_execution_attempts(&self) -> &[ExecutionAttemptRow] {
        &self.neg_risk_shadow_execution_attempts
    }

    pub fn neg_risk_shadow_execution_artifacts(&self) -> &[ShadowExecutionArtifactRow] {
        &self.neg_risk_shadow_execution_artifacts
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
        self.last_emitted_rollout_evidence = None;
        let span = tracing::info_span!(
            span_names::APP_SUPERVISOR_RESUME,
            app_mode = field::Empty,
            backlog_count = field::Empty,
            processed_count = field::Empty,
            last_journal_seq = field::Empty,
            state_version = field::Empty,
            snapshot_id = field::Empty,
            pending_reconcile_count = field::Empty,
            global_posture = field::Empty,
            ingress_backlog = field::Empty,
            follow_up_backlog = field::Empty,
            evidence_source = field::Empty
        );
        let _span_guard = span.enter();
        span.record(field_keys::APP_MODE, self.runtime.app_mode().as_str());

        self.runtime = AppRuntime::new_instrumented(
            self.runtime.app_mode(),
            runtime_instrumentation(self.metrics_recorder.as_ref()),
        );
        self.neg_risk_rollout_evidence = None;
        self.neg_risk_rollout_evidence_source = NegRiskRolloutEvidenceSource::None;
        self.candidate_restore_status = CandidateRestoreStatus::default();
        self.neg_risk_live_execution_records = Vec::new();
        self.neg_risk_live_state_source = NegRiskLiveStateSource::None;
        self.neg_risk_shadow_execution_attempts = Vec::new();
        self.neg_risk_shadow_execution_artifacts = Vec::new();
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
        self.candidate_restore_status = self.seed.candidate_restore_status.clone();
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
        self.neg_risk_shadow_execution_attempts =
            self.seed.neg_risk_shadow_execution_attempts.clone();
        self.neg_risk_shadow_execution_artifacts =
            self.seed.neg_risk_shadow_execution_artifacts.clone();
        self.retain_current_neg_risk_live_execution_records();
        self.retain_current_neg_risk_shadow_execution_records();
        self.validate_neg_risk_shadow_execution_anchor()?;
        self.validate_rollout_evidence_anchor()?;

        let processed_count = self.drain_input_tasks()?;

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
        span.record(field_keys::GLOBAL_POSTURE, summary.global_posture.as_str());
        span.record(field_keys::INGRESS_BACKLOG, summary.ingress_backlog_count);
        span.record(
            field_keys::FOLLOW_UP_BACKLOG,
            summary.follow_up_backlog_count,
        );
        span.record(
            field_keys::EVIDENCE_SOURCE,
            summary.neg_risk_rollout_evidence_source.as_str(),
        );
        if let Some(snapshot_id) = summary.published_snapshot_id.as_deref() {
            span.record(field_keys::SNAPSHOT_ID, snapshot_id);
        }

        Ok(summary)
    }

    fn drain_input_tasks(&mut self) -> Result<usize, SupervisorError> {
        let mut processed_count = 0usize;
        let durable_anchor = self.runtime.last_journal_seq();

        while let Some(input) = self.input_tasks.next_after(durable_anchor) {
            match self.runtime.apply_input(input.clone())? {
                ApplyResult::Applied {
                    state_version,
                    dirty_set,
                    ..
                } => {
                    let candidate_dirty = dirty_set.domains.contains(&DirtyDomain::Candidates);
                    self.dispatcher.record_apply(state_version, dirty_set);
                    self.record_committed_input(input.clone());
                    let _ = self.input_tasks.remove(&input);
                    self.publish_current_snapshot(false);
                    if candidate_dirty {
                        self.materialize_candidate_artifacts();
                    }
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

        Ok(processed_count)
    }

    fn drain_input_tasks_authoritative_discovery(&mut self) -> Result<usize, SupervisorError> {
        let mut processed_count = 0usize;
        let mut candidate_dirty_seen = false;
        let durable_anchor = self.runtime.last_journal_seq();

        while let Some(input) = self.input_tasks.next_after(durable_anchor) {
            match self.runtime.apply_input(input.clone())? {
                ApplyResult::Applied {
                    state_version,
                    dirty_set,
                    ..
                } => {
                    let candidate_dirty = dirty_set.domains.contains(&DirtyDomain::Candidates);
                    self.dispatcher.record_apply(state_version, dirty_set);
                    self.record_committed_input(input.clone());
                    let _ = self.input_tasks.remove(&input);
                    self.publish_current_snapshot(false);
                    candidate_dirty_seen |= candidate_dirty;
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

        if candidate_dirty_seen {
            self.materialize_candidate_artifacts_authoritative();
        }

        Ok(processed_count)
    }

    fn summary(&self) -> SupervisorSummary {
        let neg_risk_live_attempt_count = self.neg_risk_live_execution_records.len();
        let ingress_backlog_count = self.input_tasks.len();
        let follow_up_backlog_count = self.runtime.follow_up_backlog_count();
        self.record_status_surface(ingress_backlog_count, follow_up_backlog_count);
        SupervisorSummary {
            fullset_mode: ExecutionMode::Live,
            negrisk_mode: if neg_risk_live_attempt_count > 0 {
                ExecutionMode::Live
            } else {
                ExecutionMode::Shadow
            },
            real_user_shadow_smoke: self.real_user_shadow_smoke_enabled,
            neg_risk_live_attempt_count,
            neg_risk_live_state_source: self.neg_risk_live_state_source,
            neg_risk_rollout_evidence_source: self.neg_risk_rollout_evidence_source,
            bootstrap_status: self.runtime.bootstrap_status(),
            runtime_mode: self.runtime.runtime_mode(),
            pending_reconcile_count: self.runtime.pending_reconcile_count(),
            last_journal_seq: self.runtime.last_journal_seq().unwrap_or_default(),
            last_state_version: self.runtime.state_version(),
            published_snapshot_id: self.runtime.published_snapshot_id().map(str::to_owned),
            published_snapshot_committed_journal_seq: self
                .runtime
                .published_snapshot_committed_journal_seq(),
            latest_candidate_revision: self
                .candidate_restore_status
                .latest_candidate_revision
                .clone(),
            latest_adoptable_revision: self
                .candidate_restore_status
                .latest_adoptable_revision
                .clone(),
            latest_candidate_operator_target_revision: self
                .candidate_restore_status
                .latest_candidate_operator_target_revision
                .clone(),
            adoption_provenance_resolved: self
                .candidate_restore_status
                .adoption_provenance_resolved,
            neg_risk_rollout_evidence: self.neg_risk_rollout_evidence.clone(),
            global_posture: self.posture,
            ingress_backlog_count,
            follow_up_backlog_count,
        }
    }

    fn publish_current_snapshot(&mut self, allow_operator_synthesis: bool) {
        if let Some(snapshot) = self
            .runtime
            .publish_snapshot(&snapshot_id_for(self.runtime.state_version()))
        {
            let (evidence, evidence_source) =
                self.rollout_evidence_for_snapshot(&snapshot, allow_operator_synthesis);
            self.record_rollout_evidence(&evidence);
            self.neg_risk_rollout_evidence = Some(evidence);
            self.neg_risk_rollout_evidence_source = evidence_source;
            self.retain_current_neg_risk_live_execution_records();
            self.retain_current_neg_risk_shadow_execution_records();
            self.dispatcher.observe_snapshot(snapshot);
        } else {
            self.record_zero_rollout_evidence();
            self.neg_risk_rollout_evidence = None;
            self.neg_risk_rollout_evidence_source = NegRiskRolloutEvidenceSource::None;
            self.retain_current_neg_risk_live_execution_records();
            self.retain_current_neg_risk_shadow_execution_records();
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
        let pristine_bootstrap = self.seed.last_journal_seq.is_none()
            && self.seed.committed_state_version.is_none()
            && self.seed.published_snapshot_id.is_none()
            && self.seed.pending_reconcile_count.is_none()
            && self.seed.pending_reconcile_anchors.is_empty()
            && self.seed.neg_risk_rollout_evidence.is_none()
            && self.seed.neg_risk_live_execution_records.is_empty()
            && self.seed.neg_risk_shadow_execution_attempts.is_empty()
            && self.committed_log.is_empty()
            && self.runtime.last_journal_seq() == Some(0)
            && self.runtime.state_version() == 0;
        if pristine_bootstrap {
            return true;
        }

        self.has_seeded_startup_state()
            && self.runtime.pending_reconcile_count() == 0
            && self.neg_risk_live_execution_records.is_empty()
            && self.neg_risk_shadow_execution_attempts.is_empty()
            && self.seed.pending_reconcile_anchors.is_empty()
            && self.seed.neg_risk_rollout_evidence.is_none()
    }

    fn refresh_neg_risk_live_execution_records(
        &mut self,
        allow_operator_synthesis: bool,
    ) -> Result<(), SupervisorError> {
        if self.runtime.app_mode() != AppRuntimeMode::Live || !allow_operator_synthesis {
            return Ok(());
        }

        if self.real_user_shadow_smoke_enabled {
            if !self.neg_risk_shadow_execution_attempts.is_empty() {
                return Ok(());
            }
        } else if !self.neg_risk_live_execution_records.is_empty() {
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

        if self.real_user_shadow_smoke_enabled {
            let records = eligible_shadow_records_with_run_session_id(
                snapshot_id,
                &self.neg_risk_live_targets,
                &self.neg_risk_live_approved_families,
                &self.neg_risk_live_ready_families,
                self.metrics_recorder.clone(),
                self.run_session_id.as_deref(),
            )
            .map_err(|err| SupervisorError::new(err.to_string()))?;
            let attempts = records
                .iter()
                .map(|record| record.attempt.clone())
                .collect::<Vec<_>>();
            let artifacts = records
                .into_iter()
                .flat_map(|record| record.artifacts)
                .collect::<Vec<_>>();
            if self.durable_shadow_persistence_enabled {
                persist_shadow_execution_records_with_run_session_id(
                    &attempts,
                    &artifacts,
                    self.run_session_id.as_deref(),
                )
                .map_err(|err| SupervisorError::new(err.to_string()))?;
            }
            self.neg_risk_shadow_execution_attempts = attempts;
            self.neg_risk_shadow_execution_artifacts = artifacts;
            self.neg_risk_live_execution_records.clear();
            self.neg_risk_live_state_source = NegRiskLiveStateSource::None;
            return Ok(());
        }

        let records = match self.neg_risk_live_execution_backend.as_deref() {
            Some(backend) => eligible_live_records_with_backend(
                snapshot_id,
                &self.neg_risk_live_targets,
                &self.neg_risk_live_approved_families,
                &self.neg_risk_live_ready_families,
                self.metrics_recorder.clone(),
                backend,
            ),
            None => eligible_live_records(
                snapshot_id,
                &self.neg_risk_live_targets,
                &self.neg_risk_live_approved_families,
                &self.neg_risk_live_ready_families,
                self.metrics_recorder.clone(),
            ),
        }
        .map_err(|err| SupervisorError::new(err.to_string()))?;
        let applied_record_count = self.apply_live_submit_facts(&records)?;
        let applied_records = records
            .into_iter()
            .take(applied_record_count)
            .collect::<Vec<_>>();
        if self.durable_live_persistence_enabled {
            persist_live_execution_records_with_run_session_id(
                &applied_records,
                self.run_session_id.as_deref(),
            )
            .map_err(|err| SupervisorError::new(err.to_string()))?;
        }
        self.neg_risk_live_execution_records = applied_records;
        self.neg_risk_live_state_source = if self.neg_risk_live_execution_records.is_empty() {
            NegRiskLiveStateSource::None
        } else {
            NegRiskLiveStateSource::SyntheticBootstrap
        };
        Ok(())
    }

    fn apply_live_submit_facts(
        &mut self,
        records: &[NegRiskLiveExecutionRecord],
    ) -> Result<usize, SupervisorError> {
        let mut next_journal_seq = self.runtime.last_journal_seq().unwrap_or(0);
        let mut applied_record_count = 0usize;

        for record in records {
            let submission_ref = record
                .submission_ref
                .as_deref()
                .or(record.pending_ref.as_deref());
            let Some(submission_ref) = submission_ref else {
                continue;
            };
            applied_record_count += 1;
            next_journal_seq += 1;
            let input = InputTaskEvent::new(
                next_journal_seq,
                domain::ExternalFactEvent::negrisk_live_submit_observed(
                    "app-live-bootstrap",
                    format!("live-submit-{}", record.attempt_id),
                    record.attempt_id.clone(),
                    record.scope.clone(),
                    submission_ref.to_owned(),
                    Utc::now(),
                ),
            );
            match self.runtime.apply_input(input.clone())? {
                ApplyResult::Applied {
                    state_version,
                    dirty_set,
                    ..
                } => {
                    self.dispatcher.record_apply(state_version, dirty_set);
                    self.publish_current_snapshot(false);
                }
                ApplyResult::Duplicate { .. }
                | ApplyResult::Deferred { .. }
                | ApplyResult::ReconcileRequired { .. } => {}
            }
            self.record_committed_input(input);

            if self.runtime.pending_reconcile_count() > 0 {
                break;
            }
        }

        Ok(applied_record_count)
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

    fn retain_current_neg_risk_shadow_execution_records(&mut self) {
        let Some(snapshot_id) = self.runtime.published_snapshot_id() else {
            self.neg_risk_shadow_execution_attempts.clear();
            self.neg_risk_shadow_execution_artifacts.clear();
            return;
        };

        self.neg_risk_shadow_execution_attempts
            .retain(|attempt| attempt.snapshot_id == snapshot_id);
        let retained_attempt_ids = self
            .neg_risk_shadow_execution_attempts
            .iter()
            .map(|attempt| attempt.attempt_id.as_str())
            .collect::<BTreeSet<_>>();
        self.neg_risk_shadow_execution_artifacts
            .retain(|artifact| retained_attempt_ids.contains(artifact.attempt_id.as_str()));
    }

    fn validate_neg_risk_shadow_execution_anchor(&self) -> Result<(), SupervisorError> {
        if !self.real_user_shadow_smoke_enabled
            || (self.seed.neg_risk_shadow_execution_attempts.is_empty()
                && self.seed.neg_risk_shadow_execution_artifacts.is_empty())
        {
            return Ok(());
        }

        let Some(snapshot_id) = self.runtime.published_snapshot_id() else {
            return Err(self.divergence_error(
                DIVERGENCE_NEG_RISK_SHADOW_EXECUTION_SNAPSHOT_MISMATCH,
                "real-user shadow smoke requires a published snapshot to retain shadow execution records",
            ));
        };

        if self.neg_risk_shadow_execution_attempts.is_empty()
            || self.neg_risk_shadow_execution_artifacts.is_empty()
        {
            return Err(self.divergence_error(
                DIVERGENCE_NEG_RISK_SHADOW_EXECUTION_SNAPSHOT_MISMATCH,
                "real-user shadow smoke requires retained shadow execution records after restore",
            ));
        }

        if self
            .neg_risk_shadow_execution_attempts
            .iter()
            .any(|attempt| attempt.snapshot_id != snapshot_id)
        {
            return Err(self.divergence_error(
                DIVERGENCE_NEG_RISK_SHADOW_EXECUTION_SNAPSHOT_MISMATCH,
                "real-user shadow smoke retained shadow execution records did not match the restored snapshot",
            ));
        }

        Ok(())
    }

    fn materialize_candidate_artifacts(&mut self) {
        self.materialize_candidate_artifacts_with_authority(false);
    }

    fn materialize_candidate_artifacts_authoritative(&mut self) {
        self.materialize_candidate_artifacts_with_authority(true);
    }

    fn materialize_candidate_artifacts_with_authority(&mut self, authoritative: bool) {
        let Some(publication) = self.runtime.candidate_publication() else {
            return;
        };
        let Some(view) = publication.view.as_ref() else {
            return;
        };
        if view.discovery_records.is_empty() {
            return;
        }

        let notice = if authoritative {
            CandidateNotice::authoritative_from_publication(
                &publication,
                [DirtyDomain::Candidates],
                self.neg_risk_live_target_revision.as_deref(),
                self.neg_risk_live_targets.clone(),
                CandidateRestrictionTruth::eligible(),
            )
        } else {
            CandidateNotice::from_publication(
                &publication,
                [DirtyDomain::Candidates],
                self.neg_risk_live_target_revision.as_deref(),
                self.neg_risk_live_targets.clone(),
                CandidateRestrictionTruth::eligible(),
            )
        };
        let Ok(report) = DiscoverySupervisor::persist_notice_blocking(notice) else {
            return;
        };

        let adoption_provenance_resolved =
            self.candidate_restore_status.adoption_provenance_resolved
                && report.operator_target_revision.as_deref()
                    == self.neg_risk_live_target_revision.as_deref();

        self.candidate_restore_status = CandidateRestoreStatus {
            latest_candidate_revision: report.candidate_revision,
            latest_adoptable_revision: report.adoptable_revision,
            latest_candidate_operator_target_revision: report.operator_target_revision,
            adoption_provenance_resolved,
        };
    }

    fn rollout_evidence_for_snapshot(
        &self,
        snapshot: &PublishedSnapshot,
        allow_operator_synthesis: bool,
    ) -> (NegRiskRolloutEvidence, NegRiskRolloutEvidenceSource) {
        if snapshot.negrisk.is_some() {
            return (
                rollout_evidence_from_snapshot(snapshot),
                NegRiskRolloutEvidenceSource::Snapshot,
            );
        }

        if !allow_operator_synthesis {
            if let Some(evidence) = self.seed.neg_risk_rollout_evidence.as_ref() {
                if evidence.snapshot_id == snapshot.snapshot_id {
                    return (evidence.clone(), NegRiskRolloutEvidenceSource::Bootstrap);
                }
            }
        }

        if allow_operator_synthesis {
            let live_ready_family_count = self.synthetic_live_ready_family_count();
            return (
                NegRiskRolloutEvidence {
                    snapshot_id: snapshot.snapshot_id.clone(),
                    live_ready_family_count,
                    blocked_family_count: self
                        .neg_risk_live_targets
                        .len()
                        .saturating_sub(live_ready_family_count),
                    parity_mismatch_count: 0,
                },
                NegRiskRolloutEvidenceSource::Bootstrap,
            );
        }

        (
            NegRiskRolloutEvidence {
                snapshot_id: snapshot.snapshot_id.clone(),
                ..NegRiskRolloutEvidence::default()
            },
            NegRiskRolloutEvidenceSource::Neutral,
        )
    }

    fn validate_neg_risk_live_execution_anchor(&self) -> Result<(), SupervisorError> {
        if self.runtime.app_mode() != AppRuntimeMode::Live {
            return Ok(());
        }
        if self.real_user_shadow_smoke_enabled {
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

    fn has_seeded_startup_state(&self) -> bool {
        self.seed.last_journal_seq.is_some()
            || self.seed.committed_state_version.is_some()
            || self.seed.published_snapshot_id.is_some()
            || self.seed.pending_reconcile_count.is_some()
            || !self.seed.pending_reconcile_anchors.is_empty()
            || !self.seed.neg_risk_live_execution_records.is_empty()
            || !self.seed.neg_risk_shadow_execution_attempts.is_empty()
    }

    fn validate_seeded_startup_restore(&self) -> Result<(), SupervisorError> {
        if self.seed.neg_risk_rollout_evidence.is_some() {
            self.validate_rollout_evidence_anchor()?;
        }

        if !self.seed.neg_risk_live_execution_records.is_empty() {
            self.validate_neg_risk_live_execution_anchor()?;
        }

        Ok(())
    }

    fn restore_seeded_startup_state(&mut self) -> Result<(), SupervisorError> {
        self.runtime = AppRuntime::new_instrumented(
            self.runtime.app_mode(),
            runtime_instrumentation(self.metrics_recorder.as_ref()),
        );
        self.neg_risk_rollout_evidence = None;
        self.neg_risk_rollout_evidence_source = NegRiskRolloutEvidenceSource::None;
        self.last_emitted_rollout_evidence = None;
        self.candidate_restore_status = self.seed.candidate_restore_status.clone();
        self.neg_risk_live_execution_records = Vec::new();
        self.neg_risk_live_state_source = NegRiskLiveStateSource::None;
        self.neg_risk_shadow_execution_attempts = Vec::new();
        self.neg_risk_shadow_execution_artifacts = Vec::new();

        let committed_state_version = self
            .seed
            .committed_state_version
            .unwrap_or(self.seed.last_state_version);
        let Some(last_journal_seq) = self.seed.last_journal_seq else {
            return Err(SupervisorError::new(
                "durable last journal sequence is required to restore seeded startup state",
            ));
        };
        self.runtime
            .restore_committed_anchor(committed_state_version, last_journal_seq);

        for anchor in self.seed.pending_reconcile_anchors.iter().cloned() {
            self.runtime.restore_pending_reconcile_anchor(anchor);
        }
        match self.seed.pending_reconcile_count {
            Some(expected) if self.runtime.pending_reconcile_count() != expected => {
                return Err(SupervisorError::new(format!(
                    "durable pending reconcile count {} did not match restored count {}",
                    expected,
                    self.runtime.pending_reconcile_count()
                )));
            }
            Some(0) | Some(_) | None => {}
        }

        self.publish_current_snapshot(false);
        self.neg_risk_live_execution_records = self.seed.neg_risk_live_execution_records.clone();
        self.neg_risk_live_state_source = if self.neg_risk_live_execution_records.is_empty() {
            NegRiskLiveStateSource::None
        } else {
            NegRiskLiveStateSource::DurableRestore
        };
        self.neg_risk_shadow_execution_attempts =
            self.seed.neg_risk_shadow_execution_attempts.clone();
        self.neg_risk_shadow_execution_artifacts =
            self.seed.neg_risk_shadow_execution_artifacts.clone();
        self.retain_current_neg_risk_live_execution_records();
        self.retain_current_neg_risk_shadow_execution_records();
        self.validate_neg_risk_shadow_execution_anchor()?;
        Ok(())
    }

    fn record_recovery_backlog(&self, backlog_count: usize) {
        let Some(recorder) = &self.metrics_recorder else {
            return;
        };

        recorder.record_recovery_backlog_count(backlog_count as f64);
    }

    fn record_rollout_evidence(&mut self, evidence: &NegRiskRolloutEvidence) {
        let Some(recorder) = &self.metrics_recorder else {
            return;
        };

        recorder.record_neg_risk_live_ready_family_count(evidence.live_ready_family_count as f64);
        recorder.record_neg_risk_live_gate_block_count(evidence.blocked_family_count as f64);
        if self.last_emitted_rollout_evidence.as_ref() != Some(evidence) {
            recorder
                .increment_neg_risk_rollout_parity_mismatch_count(evidence.parity_mismatch_count);
            self.last_emitted_rollout_evidence = Some(evidence.clone());
        }
    }

    fn record_zero_rollout_evidence(&self) {
        let Some(recorder) = &self.metrics_recorder else {
            return;
        };

        recorder.record_neg_risk_live_ready_family_count(0.0);
        recorder.record_neg_risk_live_gate_block_count(0.0);
    }

    fn record_status_surface(&self, ingress_backlog_count: usize, follow_up_backlog_count: usize) {
        let Some(recorder) = &self.metrics_recorder else {
            return;
        };

        recorder.record_daemon_posture(self.posture.as_str());
        recorder.record_ingress_backlog(ingress_backlog_count as f64);
        recorder.record_follow_up_backlog(follow_up_backlog_count as f64);
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use domain::ExecutionMode;
    use rust_decimal::Decimal;

    use super::AppSupervisor;
    use crate::{
        negrisk_live::{NegRiskLiveError, NegRiskLiveExecutionBackend},
        NegRiskFamilyLiveTarget, NegRiskLiveExecutionRecord, NegRiskMemberLiveTarget,
    };

    struct StubNegRiskLiveExecutionBackend;

    impl NegRiskLiveExecutionBackend for StubNegRiskLiveExecutionBackend {
        fn execute_live_family(
            &self,
            snapshot_id: &str,
            target: &NegRiskFamilyLiveTarget,
            matched_rule_id: &str,
            _instrumentation: execution::ExecutionInstrumentation,
        ) -> Result<NegRiskLiveExecutionRecord, NegRiskLiveError> {
            Ok(NegRiskLiveExecutionRecord {
                attempt_id: format!("attempt-{}", target.family_id),
                plan_id: format!("plan-{}", target.family_id),
                snapshot_id: snapshot_id.to_owned(),
                execution_mode: domain::ExecutionMode::Live,
                attempt_no: 1,
                idempotency_key: format!("idem-{}", target.family_id),
                route: "neg-risk".to_owned(),
                scope: target.family_id.clone(),
                matched_rule_id: Some(matched_rule_id.to_owned()),
                submission_ref: Some(format!("submission-{}", target.family_id)),
                pending_ref: Some(format!("tx:{}", target.family_id)),
                artifacts: Vec::new(),
                order_requests: Vec::new(),
            })
        }
    }

    #[test]
    fn smoke_supervisor_without_durable_persistence_capability_ignores_ambient_database_url() {
        let mut supervisor = AppSupervisor::for_tests();
        supervisor.enable_real_user_shadow_smoke();
        supervisor.seed_neg_risk_live_targets(BTreeMap::from([(
            "family-a".to_owned(),
            NegRiskFamilyLiveTarget {
                family_id: "family-a".to_owned(),
                members: vec![NegRiskMemberLiveTarget {
                    condition_id: "condition-1".to_owned(),
                    token_id: "token-1".to_owned(),
                    price: Decimal::new(45, 2),
                    quantity: Decimal::new(10, 0),
                }],
            },
        )]));
        supervisor.seed_neg_risk_live_approval("family-a");
        supervisor.seed_neg_risk_live_ready_family("family-a");

        let summary = supervisor.run_once().expect("supervisor should run");

        assert_eq!(summary.negrisk_mode, ExecutionMode::Shadow);
        assert_eq!(summary.neg_risk_live_attempt_count, 0);
        assert_eq!(supervisor.neg_risk_shadow_execution_attempts().len(), 1);
        assert_eq!(supervisor.neg_risk_shadow_execution_artifacts().len(), 1);
    }

    #[test]
    fn supervisor_uses_injected_neg_risk_backend_for_synthetic_live_bootstrap() {
        let mut supervisor = AppSupervisor::for_tests();
        supervisor.set_neg_risk_live_execution_backend(StubNegRiskLiveExecutionBackend);
        supervisor.seed_neg_risk_live_targets(BTreeMap::from([(
            "family-a".to_owned(),
            NegRiskFamilyLiveTarget {
                family_id: "family-a".to_owned(),
                members: vec![NegRiskMemberLiveTarget {
                    condition_id: "condition-1".to_owned(),
                    token_id: "token-1".to_owned(),
                    price: Decimal::new(45, 2),
                    quantity: Decimal::new(10, 0),
                }],
            },
        )]));
        supervisor.seed_neg_risk_live_approval("family-a");
        supervisor.seed_neg_risk_live_ready_family("family-a");

        let summary = supervisor.run_once().expect("supervisor should run");

        assert_eq!(summary.negrisk_mode, ExecutionMode::Live);
        assert_eq!(summary.neg_risk_live_attempt_count, 1);
        assert_eq!(
            supervisor.neg_risk_live_execution_records()[0]
                .submission_ref
                .as_deref(),
            Some("submission-family-a")
        );
        assert_eq!(
            supervisor.neg_risk_live_execution_records()[0]
                .pending_ref
                .as_deref(),
            Some("tx:family-a")
        );
        assert_eq!(
            summary.neg_risk_live_state_source,
            crate::supervisor::NegRiskLiveStateSource::SyntheticBootstrap
        );
    }

    #[test]
    fn app_supervisor_remains_send_and_sync_after_backend_injection_seam() {
        fn assert_send_sync<T: Send + Sync>() {}

        assert_send_sync::<AppSupervisor>();
    }
}
