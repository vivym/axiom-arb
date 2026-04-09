use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    str::FromStr,
};

use domain::{RuntimeMode, RuntimeOverlay};
use observability::{field_keys, span_names};
use persistence::{
    append_shadow_execution_batch, connect_pool_from_env,
    models::{
        ExecutionAttemptRow, LiveSubmissionRecordRow, PendingReconcileRow, RuntimeProgressRow,
        ShadowExecutionArtifactRow,
    },
    CandidateAdoptionRepo, CandidateArtifactRepo, ExecutionAttemptRepo, LiveSubmissionRepo,
    PendingReconcileRepo, RuntimeProgressRepo, ShadowArtifactRepo,
};
use serde_json::json;
use state::{
    ApplyError, ApplyResult, CandidateProjectionReadiness, CandidatePublication,
    PendingReconcileAnchor, PublishedSnapshot, ReconcileReport, RemoteSnapshot, StateApplier,
    StateStore,
};
use tracing::field;

use crate::bootstrap::{self, BootstrapSource, BootstrapStatus};
use crate::config::NegRiskLiveTargetSet;
use crate::input_tasks::InputTaskEvent;
use crate::instrumentation::AppInstrumentation;
use crate::negrisk_live::NegRiskLiveExecutionRecord;
use crate::smoke::RealUserShadowSmokeConfig;
use crate::supervisor::{AppSupervisor, CandidateRestoreStatus, SupervisorSummary};

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
            "unsupported runtime mode '{}'; expected 'paper' or 'live'",
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

#[derive(Debug, Clone)]
pub(crate) struct DurableLiveStartupState {
    pub(crate) last_journal_seq: i64,
    pub(crate) last_state_version: u64,
    pub(crate) published_snapshot_id: Option<String>,
    pub(crate) pending_reconcile_anchors: Vec<PendingReconcileAnchor>,
    pub(crate) live_execution_records: Vec<NegRiskLiveExecutionRecord>,
    pub(crate) shadow_execution_attempts: Vec<ExecutionAttemptRow>,
    pub(crate) shadow_execution_artifacts: Vec<ShadowExecutionArtifactRow>,
    pub(crate) candidate_restore_status: CandidateRestoreStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DurableShadowExecutionState {
    pub(crate) attempts: Vec<ExecutionAttemptRow>,
    pub(crate) artifacts: Vec<ShadowExecutionArtifactRow>,
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

    pub fn follow_up_backlog_count(&self) -> usize {
        self.store.pending_reconcile_count()
    }

    pub fn restore_committed_anchor(
        &mut self,
        committed_state_version: u64,
        committed_journal_seq: i64,
    ) {
        self.store
            .restore_committed_anchor(committed_state_version, committed_journal_seq);
    }

    pub fn restore_pending_reconcile_anchor(&mut self, anchor: PendingReconcileAnchor) {
        self.store.restore_pending_reconcile_anchor(anchor);
    }

    pub fn reconcile(&mut self, snapshot: RemoteSnapshot) -> ReconcileReport {
        let span = tracing::info_span!(
            span_names::APP_RUNTIME_RECONCILE,
            app_mode = field::Empty,
            pending_reconcile_count = field::Empty
        );
        let _span_guard = span.enter();
        span.record(field_keys::APP_MODE, self.app_mode.as_str());

        let report = bootstrap::reconcile(&mut self.store, snapshot);
        if !report.attention.is_empty() {
            for attention in &report.attention {
                self.instrumentation.record_reconcile_attention(attention);
            }
        }
        if report.succeeded {
            self.store.clear_pending_reconcile_after_restore();
        }
        self.anchor_baseline_if_ready(report.succeeded);
        let pending_reconcile_count = self.store.pending_reconcile_count();
        span.record(field_keys::PENDING_RECONCILE_COUNT, pending_reconcile_count);
        report
    }

    pub fn bootstrap_once<S>(&mut self, source: &S) -> ReconcileReport
    where
        S: BootstrapSource,
    {
        let report = bootstrap::bootstrap_once(&mut self.store, source);
        if !report.attention.is_empty() {
            for attention in &report.attention {
                self.instrumentation.record_reconcile_attention(attention);
            }
        }
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
        let result = match StateApplier::new(&mut self.store)
            .apply(journal_seq, input.into_state_fact_input())
        {
            Ok(result) => result,
            Err(error) => {
                span.record(field_keys::APPLY_RESULT, "error");
                return Err(error);
            }
        };
        span.record(field_keys::APPLY_RESULT, apply_result_label(&result));
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
        span.record(field_keys::STATE_VERSION, snapshot.state_version);
        span.record(
            field_keys::COMMITTED_JOURNAL_SEQ,
            snapshot.committed_journal_seq,
        );
        self.published_snapshot = Some(snapshot.clone());
        Some(snapshot)
    }

    pub fn candidate_publication(&self) -> Option<CandidatePublication> {
        self.store.last_applied_journal_seq()?;
        Some(CandidatePublication::from_store(
            &self.store,
            CandidateProjectionReadiness::ready(format!("candidate-pub-{}", self.state_version())),
        ))
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

pub fn run_paper_instrumented<S>(source: &S, instrumentation: AppInstrumentation) -> AppRunResult
where
    S: BootstrapSource,
{
    run_with_mode(AppRuntimeMode::Paper, source, instrumentation)
}

pub fn run_live<S>(source: &S) -> AppRunResult
where
    S: BootstrapSource,
{
    run_live_instrumented(source, AppInstrumentation::disabled())
}

pub fn run_live_with_neg_risk_live_targets<S>(
    source: &S,
    neg_risk_live_targets: NegRiskLiveTargetSet,
    neg_risk_live_approved_families: BTreeSet<String>,
    neg_risk_live_ready_families: BTreeSet<String>,
) -> AppRunResult
where
    S: BootstrapSource,
{
    run_live_with_neg_risk_live_targets_instrumented(
        source,
        AppInstrumentation::disabled(),
        neg_risk_live_targets,
        neg_risk_live_approved_families,
        neg_risk_live_ready_families,
    )
}

pub fn run_live_with_neg_risk_live_targets_instrumented<S>(
    source: &S,
    instrumentation: AppInstrumentation,
    neg_risk_live_targets: NegRiskLiveTargetSet,
    neg_risk_live_approved_families: BTreeSet<String>,
    neg_risk_live_ready_families: BTreeSet<String>,
) -> AppRunResult
where
    S: BootstrapSource,
{
    let mut supervisor = match instrumentation.recorder() {
        Some(recorder) => {
            AppSupervisor::new_instrumented(AppRuntimeMode::Live, source.snapshot(), recorder)
        }
        None => AppSupervisor::new(AppRuntimeMode::Live, source.snapshot()),
    };
    supervisor.seed_neg_risk_live_targets(neg_risk_live_targets.into_targets());
    for family_id in neg_risk_live_approved_families {
        supervisor.seed_neg_risk_live_approval(&family_id);
    }
    for family_id in neg_risk_live_ready_families {
        supervisor.seed_neg_risk_live_ready_family(&family_id);
    }
    supervisor.run_bootstrap()
}

pub fn run_live_from_durable_store_instrumented<S>(
    source: &S,
    instrumentation: AppInstrumentation,
) -> Result<AppRunResult, Box<dyn std::error::Error>>
where
    S: BootstrapSource,
{
    run_live_from_durable_store_with_neg_risk_live_targets_instrumented(
        source,
        instrumentation,
        NegRiskLiveTargetSet::empty(),
        BTreeSet::new(),
        BTreeSet::new(),
        None,
    )
}

pub fn run_live_from_durable_store_with_neg_risk_live_targets_instrumented<S>(
    source: &S,
    instrumentation: AppInstrumentation,
    neg_risk_live_targets: NegRiskLiveTargetSet,
    neg_risk_live_approved_families: BTreeSet<String>,
    neg_risk_live_ready_families: BTreeSet<String>,
    real_user_shadow_smoke: Option<RealUserShadowSmokeConfig>,
) -> Result<AppRunResult, Box<dyn std::error::Error>>
where
    S: BootstrapSource,
{
    run_live_from_durable_store_with_strategy_revision_and_neg_risk_live_targets_instrumented(
        source,
        instrumentation,
        None,
        false,
        neg_risk_live_targets,
        neg_risk_live_approved_families,
        neg_risk_live_ready_families,
        real_user_shadow_smoke,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn run_live_from_durable_store_with_strategy_revision_and_neg_risk_live_targets_instrumented<
    S,
>(
    source: &S,
    instrumentation: AppInstrumentation,
    operator_strategy_revision: Option<&str>,
    allow_legacy_target_source_resume: bool,
    neg_risk_live_targets: NegRiskLiveTargetSet,
    neg_risk_live_approved_families: BTreeSet<String>,
    neg_risk_live_ready_families: BTreeSet<String>,
    real_user_shadow_smoke: Option<RealUserShadowSmokeConfig>,
) -> Result<AppRunResult, Box<dyn std::error::Error>>
where
    S: BootstrapSource,
{
    let load_shadow_state = real_user_shadow_smoke.is_some();
    let operator_target_revision =
        operator_target_revision_for(&neg_risk_live_targets).map(str::to_owned);
    let durable_state = load_durable_live_startup_state_for_strategy(
        operator_strategy_revision,
        operator_target_revision.as_deref(),
        allow_legacy_target_source_resume,
        load_shadow_state,
    )?;
    validate_real_user_shadow_smoke_restore(&durable_state, real_user_shadow_smoke.as_ref())?;
    let mut supervisor = match instrumentation.recorder() {
        Some(recorder) => {
            AppSupervisor::new_instrumented(AppRuntimeMode::Live, source.snapshot(), recorder)
        }
        None => AppSupervisor::new(AppRuntimeMode::Live, source.snapshot()),
    };
    supervisor.enable_durable_live_persistence();
    if real_user_shadow_smoke.is_some() {
        supervisor.enable_real_user_shadow_smoke();
        supervisor.enable_durable_shadow_persistence();
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

    let result = supervisor.run_startup()?;
    persist_operator_startup_anchor(
        &result.summary,
        operator_target_revision.as_deref(),
        operator_strategy_revision,
    )?;
    Ok(result)
}

pub fn run_live_instrumented<S>(source: &S, instrumentation: AppInstrumentation) -> AppRunResult
where
    S: BootstrapSource,
{
    run_with_mode(AppRuntimeMode::Live, source, instrumentation)
}

fn run_with_mode<S>(
    app_mode: AppRuntimeMode,
    source: &S,
    instrumentation: AppInstrumentation,
) -> AppRunResult
where
    S: BootstrapSource,
{
    let supervisor = match instrumentation.recorder() {
        Some(recorder) => AppSupervisor::new_instrumented(app_mode, source.snapshot(), recorder),
        None => AppSupervisor::new(app_mode, source.snapshot()),
    };

    supervisor.run_bootstrap()
}

fn apply_result_label(result: &ApplyResult) -> &'static str {
    match result {
        ApplyResult::Applied { .. } => "applied",
        ApplyResult::Duplicate { .. } => "duplicate",
        ApplyResult::Deferred { .. } => "deferred",
        ApplyResult::ReconcileRequired { .. } => "reconcile_required",
    }
}

pub(crate) fn load_durable_live_startup_state(
    operator_target_revision: Option<&str>,
    load_shadow_state: bool,
) -> Result<DurableLiveStartupState, Box<dyn std::error::Error>> {
    load_durable_live_startup_state_for_strategy(
        None,
        operator_target_revision,
        false,
        load_shadow_state,
    )
}

pub(crate) fn load_durable_live_startup_state_for_strategy(
    operator_strategy_revision: Option<&str>,
    operator_target_revision: Option<&str>,
    allow_legacy_target_source_resume: bool,
    load_shadow_state: bool,
) -> Result<DurableLiveStartupState, Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let pool = connect_pool_from_env().await?;
        let progress = RuntimeProgressRepo.current(&pool).await?;
        let live_attempts = ExecutionAttemptRepo.list_live_attempts(&pool).await?;
        let submissions_by_attempt = LiveSubmissionRepo
            .list_for_attempts(
                &pool,
                &live_attempts
                    .iter()
                    .map(|attempt| attempt.attempt_id.clone())
                    .collect::<Vec<_>>(),
            )
            .await?;
        let pending_rows = PendingReconcileRepo.list_all(&pool).await?;
        let pending_reconcile_anchors = pending_rows
            .iter()
            .map(pending_reconcile_anchor_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        let live_execution_records =
            durable_live_execution_records(live_attempts, submissions_by_attempt, &pending_rows)?;
        let shadow_execution_state = if load_shadow_state {
            let shadow_attempts = ExecutionAttemptRepo.list_shadow_attempts(&pool).await?;
            let shadow_artifacts = ShadowArtifactRepo
                .list_for_attempts(
                    &pool,
                    &shadow_attempts
                        .iter()
                        .map(|attempt| attempt.attempt_id.clone())
                        .collect::<Vec<_>>(),
                )
                .await?;
            durable_shadow_execution_state(shadow_attempts, shadow_artifacts)?
        } else {
            DurableShadowExecutionState {
                attempts: Vec::new(),
                artifacts: Vec::new(),
            }
        };
        let has_durable_follow_up_work =
            !pending_reconcile_anchors.is_empty()
                || !live_execution_records.is_empty()
                || !shadow_execution_state.attempts.is_empty();
        let progress_operator_target_revision = progress
            .as_ref()
            .and_then(|row| row.operator_target_revision.as_deref());
        let candidate_restore_status = if let Some(progress_operator_target_revision) =
            progress_operator_target_revision
        {
            match CandidateAdoptionRepo
                .get_by_operator_target_revision(&pool, progress_operator_target_revision)
                .await
            {
                Ok(Some(provenance)) => {
                    let artifacts = CandidateArtifactRepo;
                    let candidate = artifacts
                        .get_candidate_target_set(&pool, &provenance.candidate_revision)
                        .await?
                        .ok_or_else(|| {
                            boxed_error(format!(
                                "candidate adoption provenance {} could not load candidate {}",
                                progress_operator_target_revision, provenance.candidate_revision
                            ))
                        })?;
                    let adoptable = artifacts
                        .get_adoptable_target_revision(&pool, &provenance.adoptable_revision)
                        .await?
                        .ok_or_else(|| {
                            boxed_error(format!(
                                "candidate adoption provenance {} could not load adoptable {}",
                                progress_operator_target_revision, provenance.adoptable_revision
                            ))
                        })?;
                    if adoptable.candidate_revision != provenance.candidate_revision
                        || adoptable.rendered_operator_target_revision
                            != progress_operator_target_revision
                    {
                        return Err(boxed_error(format!(
                            "candidate adoption provenance chain mismatch for operator target revision {progress_operator_target_revision}"
                        )));
                    }

                    CandidateRestoreStatus {
                        latest_candidate_revision: Some(candidate.candidate_revision),
                        latest_adoptable_revision: Some(adoptable.adoptable_revision),
                        latest_candidate_operator_target_revision: Some(
                            progress_operator_target_revision.to_owned(),
                        ),
                        adoption_provenance_resolved: true,
                    }
                }
                Ok(None) => {
                    CandidateRestoreStatus::default()
                }
                Err(error) => return Err(error.into()),
            }
        } else {
            CandidateRestoreStatus::default()
        };
        validate_operator_revision(
            progress.as_ref(),
            operator_strategy_revision,
            operator_target_revision,
            allow_legacy_target_source_resume,
            has_durable_follow_up_work,
        )?;
        let (last_journal_seq, last_state_version, published_snapshot_id) =
            durable_progress_anchor(progress, has_durable_follow_up_work)?;

        Ok(DurableLiveStartupState {
            last_journal_seq,
            last_state_version,
            published_snapshot_id,
            pending_reconcile_anchors,
            live_execution_records,
            shadow_execution_attempts: shadow_execution_state.attempts,
            shadow_execution_artifacts: shadow_execution_state.artifacts,
            candidate_restore_status,
        })
    })
}

pub(crate) fn operator_target_revision_for(targets: &NegRiskLiveTargetSet) -> Option<&str> {
    (!targets.is_empty()).then_some(targets.revision())
}

fn validate_real_user_shadow_smoke_restore(
    durable_state: &DurableLiveStartupState,
    real_user_shadow_smoke: Option<&RealUserShadowSmokeConfig>,
) -> Result<(), Box<dyn std::error::Error>> {
    if real_user_shadow_smoke.is_some() && !durable_state.live_execution_records.is_empty() {
        return Err(boxed_error(
            "real-user shadow smoke cannot resume with durable live execution records",
        ));
    }

    Ok(())
}

#[cfg(test)]
fn validate_operator_target_revision(
    progress: Option<&RuntimeProgressRow>,
    expected_revision: Option<&str>,
    has_durable_follow_up_work: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    validate_operator_revision(
        progress,
        None,
        expected_revision,
        false,
        has_durable_follow_up_work,
    )
}

fn validate_operator_revision(
    progress: Option<&RuntimeProgressRow>,
    expected_strategy_revision: Option<&str>,
    expected_target_revision: Option<&str>,
    allow_legacy_target_source_resume: bool,
    has_durable_follow_up_work: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(expected_strategy_revision) = expected_strategy_revision {
        if !has_durable_follow_up_work {
            return Ok(());
        }

        let Some(progress) = progress else {
            return Err(boxed_error(
                "operator strategy revision anchor is required when a startup strategy bundle is supplied",
            ));
        };

        return match progress.operator_strategy_revision.as_deref() {
            Some(actual_revision) if actual_revision == expected_strategy_revision => Ok(()),
            Some(actual_revision) => Err(boxed_error(format!(
                "operator strategy revision anchor mismatch: persisted={actual_revision} configured={expected_strategy_revision}"
            ))),
            None => {
                if allow_legacy_target_source_resume
                    && progress.operator_target_revision.as_deref()
                        == Some(expected_strategy_revision)
                {
                    return Ok(());
                }
                Err(boxed_error(
                    "operator strategy revision anchor is required when a startup strategy bundle is supplied",
                ))
            }
        };
    }

    let Some(expected_target_revision) = expected_target_revision else {
        return Ok(());
    };

    if !has_durable_follow_up_work {
        return Ok(());
    }

    let Some(progress) = progress else {
        return Err(boxed_error(
            "operator target revision anchor is required when live operator targets are supplied",
        ));
    };

    match progress.operator_target_revision.as_deref() {
        Some(actual_revision) if actual_revision == expected_target_revision => Ok(()),
        Some(actual_revision) => Err(boxed_error(format!(
            "operator target revision anchor mismatch: persisted={actual_revision} configured={expected_target_revision}"
        ))),
        None => Err(boxed_error(
            "operator target revision anchor is required when live operator targets are supplied",
        )),
    }
}

pub(crate) fn persist_operator_startup_anchor(
    summary: &SupervisorSummary,
    operator_target_revision: Option<&str>,
    operator_strategy_revision: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    persist_operator_target_revision_anchor_with_run_session_id(
        summary,
        operator_target_revision,
        operator_strategy_revision,
        None,
    )
}

pub(crate) fn persist_operator_target_revision_anchor_with_run_session_id(
    summary: &SupervisorSummary,
    operator_target_revision: Option<&str>,
    operator_strategy_revision: Option<&str>,
    active_run_session_id: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    if operator_target_revision.is_none() && operator_strategy_revision.is_none() {
        return Ok(());
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let pool = connect_pool_from_env().await?;
        RuntimeProgressRepo
            .record_progress_with_strategy_revision(
                &pool,
                summary.last_journal_seq,
                i64::try_from(summary.last_state_version).map_err(|_| {
                    boxed_error(format!(
                        "startup state version {} does not fit in i64",
                        summary.last_state_version
                    ))
                })?,
                summary.published_snapshot_id.as_deref(),
                operator_target_revision,
                operator_strategy_revision,
                active_run_session_id,
            )
            .await
            .map_err(Into::into)
    })
}

fn durable_live_execution_records(
    attempts: Vec<ExecutionAttemptRow>,
    submissions_by_attempt: BTreeMap<String, Vec<LiveSubmissionRecordRow>>,
    pending_rows: &[PendingReconcileRow],
) -> Result<Vec<NegRiskLiveExecutionRecord>, Box<dyn std::error::Error>> {
    let pending_by_submission_ref = pending_rows
        .iter()
        .map(|row| {
            let submission_ref =
                payload_string(&row.payload, "submission_ref").map_err(boxed_error)?;
            Ok((submission_ref, row.pending_ref.clone()))
        })
        .collect::<Result<BTreeMap<_, _>, Box<dyn std::error::Error>>>()?;

    attempts
        .into_iter()
        .map(|attempt| {
            let attempt_no = u32::try_from(attempt.attempt_no).map_err(|_| {
                boxed_error(format!(
                    "durable live attempt {} has negative attempt_no {}",
                    attempt.attempt_id, attempt.attempt_no
                ))
            })?;
            let submissions = submissions_by_attempt
                .get(&attempt.attempt_id)
                .cloned()
                .unwrap_or_default();
            if submissions.is_empty() {
                return Err(boxed_error(format!(
                    "missing durable live submission record for attempt {}",
                    attempt.attempt_id
                )));
            }
            let submission_ref = submissions.first().map(|row| row.submission_ref.clone());
            let pending_ref = submission_ref
                .as_deref()
                .and_then(|submission_ref| pending_by_submission_ref.get(submission_ref))
                .cloned();

            Ok(NegRiskLiveExecutionRecord {
                attempt_id: attempt.attempt_id,
                plan_id: attempt.plan_id,
                snapshot_id: attempt.snapshot_id,
                execution_mode: attempt.execution_mode,
                attempt_no,
                idempotency_key: attempt.idempotency_key,
                route: attempt.route,
                scope: attempt.scope,
                matched_rule_id: attempt.matched_rule_id,
                submission_ref,
                pending_ref,
                artifacts: Vec::new(),
                order_requests: submissions.into_iter().map(|row| row.payload).collect(),
            })
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()
}

fn durable_progress_anchor(
    progress: Option<RuntimeProgressRow>,
    has_follow_up_work: bool,
) -> Result<(i64, u64, Option<String>), Box<dyn std::error::Error>> {
    match progress {
        Some(progress) => Ok((
            progress.last_journal_seq,
            u64::try_from(progress.last_state_version).map_err(|_| {
                boxed_error(format!(
                    "durable runtime progress state version {} is negative",
                    progress.last_state_version
                ))
            })?,
            progress.last_snapshot_id,
        )),
        None if has_follow_up_work => Err(boxed_error(
            "durable runtime progress is required when live follow-up work exists",
        )),
        None => Ok((0, 0, Some("snapshot-0".to_owned()))),
    }
}

fn durable_shadow_execution_state(
    attempts: Vec<ExecutionAttemptRow>,
    artifacts: Vec<ShadowExecutionArtifactRow>,
) -> Result<DurableShadowExecutionState, Box<dyn std::error::Error>> {
    let mut artifact_count_by_attempt = BTreeMap::new();
    let known_attempt_ids = attempts
        .iter()
        .map(|attempt| attempt.attempt_id.as_str())
        .collect::<BTreeSet<_>>();
    for artifact in &artifacts {
        if !known_attempt_ids.contains(artifact.attempt_id.as_str()) {
            return Err(boxed_error(format!(
                "shadow artifact {} is missing durable shadow attempt {}",
                artifact.stream, artifact.attempt_id
            )));
        }
        *artifact_count_by_attempt
            .entry(artifact.attempt_id.as_str())
            .or_insert(0usize) += 1;
    }
    for attempt in &attempts {
        if !artifact_count_by_attempt.contains_key(attempt.attempt_id.as_str()) {
            return Err(boxed_error(format!(
                "missing durable shadow artifact for attempt {}",
                attempt.attempt_id
            )));
        }
    }

    Ok(DurableShadowExecutionState {
        attempts,
        artifacts,
    })
}

#[allow(dead_code)]
pub(crate) fn persist_shadow_execution_records(
    attempts: &[ExecutionAttemptRow],
    artifacts: &[ShadowExecutionArtifactRow],
) -> Result<(), Box<dyn std::error::Error>> {
    persist_shadow_execution_records_with_run_session_id(attempts, artifacts, None)
}

pub(crate) fn persist_shadow_execution_records_with_run_session_id(
    attempts: &[ExecutionAttemptRow],
    artifacts: &[ShadowExecutionArtifactRow],
    run_session_id: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let attempts = attempts
        .iter()
        .map(|attempt| attempt_row_with_run_session_id(attempt, run_session_id))
        .collect::<Vec<_>>();
    let mut seen_attempt_ids = std::collections::BTreeMap::<String, Vec<String>>::new();
    for attempt in &attempts {
        seen_attempt_ids
            .entry(attempt.attempt_id.clone())
            .or_default()
            .push(format!(
                "scope={} plan_id={} run_session_id={:?}",
                attempt.scope, attempt.plan_id, attempt.run_session_id
            ));
    }
    if let Some((attempt_id, entries)) = seen_attempt_ids
        .iter()
        .find(|(_, entries)| entries.len() > 1)
    {
        return Err(boxed_error(format!(
            "duplicate shadow attempt rows reached persistence boundary: attempt_id={} entries={:?}",
            attempt_id, entries
        )));
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let pool = connect_pool_from_env().await?;
        append_shadow_execution_batch(&pool, &attempts, artifacts).await?;
        Ok(())
    })
}

#[allow(dead_code)]
pub(crate) fn persist_live_execution_records(
    records: &[NegRiskLiveExecutionRecord],
) -> Result<(), Box<dyn std::error::Error>> {
    persist_live_execution_records_with_run_session_id(records, None)
}

pub(crate) fn persist_live_execution_records_with_run_session_id(
    records: &[NegRiskLiveExecutionRecord],
    run_session_id: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let pool = connect_pool_from_env().await?;
        for record in records {
            let submission_ref = record
                .submission_ref
                .as_deref()
                .or(record.pending_ref.as_deref())
                .ok_or_else(|| {
                    boxed_error(format!(
                        "missing durable live submission ref for attempt {}",
                        record.attempt_id
                    ))
                })?;
            let attempt = execution_attempt_row_for_live_record(record, run_session_id)?;
            ExecutionAttemptRepo.append(&pool, &attempt).await?;
            LiveSubmissionRepo
                .append(
                    &pool,
                    LiveSubmissionRecordRow {
                        submission_ref: submission_ref.to_owned(),
                        attempt_id: record.attempt_id.clone(),
                        route: record.route.clone(),
                        scope: record.scope.clone(),
                        provider: "venue-polymarket".to_owned(),
                        state: if record.pending_ref.is_some() {
                            "pending_reconcile".to_owned()
                        } else {
                            "submitted".to_owned()
                        },
                        payload: json!({
                            "submission_ref": submission_ref,
                            "family_id": record.scope.clone(),
                            "route": record.route.clone(),
                            "reason": "submitted_for_execution",
                            "requests": record.order_requests.clone(),
                        }),
                    },
                )
                .await?;
        }
        Ok(())
    })
}

fn execution_attempt_row_for_live_record(
    record: &NegRiskLiveExecutionRecord,
    run_session_id: Option<&str>,
) -> Result<ExecutionAttemptRow, Box<dyn std::error::Error>> {
    Ok(ExecutionAttemptRow {
        attempt_id: record.attempt_id.clone(),
        plan_id: record.plan_id.clone(),
        snapshot_id: record.snapshot_id.clone(),
        route: record.route.clone(),
        scope: record.scope.clone(),
        matched_rule_id: record.matched_rule_id.clone(),
        execution_mode: record.execution_mode,
        attempt_no: i32::try_from(record.attempt_no).map_err(|_| {
            boxed_error(format!(
                "live attempt {} attempt_no {} does not fit in i32",
                record.attempt_id, record.attempt_no
            ))
        })?,
        idempotency_key: record.idempotency_key.clone(),
        run_session_id: run_session_id.map(str::to_owned),
    })
}

fn attempt_row_with_run_session_id(
    attempt: &ExecutionAttemptRow,
    run_session_id: Option<&str>,
) -> ExecutionAttemptRow {
    let mut attempt = attempt.clone();
    if let Some(run_session_id) = run_session_id {
        attempt.run_session_id = Some(run_session_id.to_owned());
    }
    attempt
}

fn pending_reconcile_anchor_from_row(
    row: &PendingReconcileRow,
) -> Result<PendingReconcileAnchor, Box<dyn std::error::Error>> {
    Ok(PendingReconcileAnchor::new(
        row.pending_ref.clone(),
        payload_string(&row.payload, "submission_ref").map_err(boxed_error)?,
        payload_string(&row.payload, "family_id").unwrap_or_else(|_| row.scope_id.clone()),
        payload_string(&row.payload, "route").unwrap_or_else(|_| "neg-risk".to_owned()),
        payload_string(&row.payload, "reason").unwrap_or_else(|_| row.reason.clone()),
    ))
}

fn payload_string(payload: &serde_json::Value, key: &str) -> Result<String, String> {
    payload
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("missing durable payload field {key}"))
}

fn boxed_error(message: impl Into<String>) -> Box<dyn std::error::Error> {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.into()).into()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use domain::ExecutionMode;
    use persistence::models::{
        ExecutionAttemptRow, RuntimeProgressRow, ShadowExecutionArtifactRow,
    };
    use serde_json::json;

    use super::{
        attempt_row_with_run_session_id, durable_live_execution_records, durable_progress_anchor,
        durable_shadow_execution_state, execution_attempt_row_for_live_record,
        validate_operator_revision, validate_operator_target_revision,
    };
    use crate::negrisk_live::NegRiskLiveExecutionRecord;

    #[test]
    fn durable_live_attempt_requires_submission_record() {
        let err = durable_live_execution_records(
            vec![ExecutionAttemptRow {
                attempt_id: "attempt-live-1".to_owned(),
                plan_id: "negrisk-submit-family:family-a".to_owned(),
                snapshot_id: "snapshot-7".to_owned(),
                route: "neg-risk".to_owned(),
                scope: "family-a".to_owned(),
                matched_rule_id: Some("family-a-live".to_owned()),
                execution_mode: ExecutionMode::Live,
                attempt_no: 1,
                idempotency_key: "idem-attempt-live-1".to_owned(),
                run_session_id: None,
            }],
            BTreeMap::new(),
            &[],
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("missing durable live submission record"),
            "{err}"
        );
    }

    #[test]
    fn live_attempt_row_uses_run_session_id_when_provided() {
        let attempt = execution_attempt_row_for_live_record(
            &NegRiskLiveExecutionRecord {
                attempt_id: "attempt-live-1".to_owned(),
                plan_id: "negrisk-submit-family:family-a".to_owned(),
                snapshot_id: "snapshot-7".to_owned(),
                execution_mode: ExecutionMode::Live,
                attempt_no: 1,
                idempotency_key: "idem-attempt-live-1".to_owned(),
                route: "neg-risk".to_owned(),
                scope: "family-a".to_owned(),
                matched_rule_id: Some("family-a-live".to_owned()),
                submission_ref: Some("submission-1".to_owned()),
                pending_ref: None,
                artifacts: Vec::new(),
                order_requests: vec![json!({"submission_ref": "submission-1"})],
            },
            Some("run-session-1"),
        )
        .unwrap();

        assert_eq!(attempt.run_session_id.as_deref(), Some("run-session-1"));
    }

    #[test]
    fn durable_follow_up_work_requires_runtime_progress_anchor() {
        let err = durable_progress_anchor(None, true).unwrap_err();

        assert!(
            err.to_string()
                .contains("durable runtime progress is required"),
            "{err}"
        );
    }

    #[test]
    fn durable_progress_anchor_allows_empty_baseline_without_follow_up_work() {
        let (last_journal_seq, last_state_version, snapshot_id) =
            durable_progress_anchor(None, false).unwrap();

        assert_eq!(last_journal_seq, 0);
        assert_eq!(last_state_version, 0);
        assert_eq!(snapshot_id.as_deref(), Some("snapshot-0"));
    }

    #[test]
    fn durable_progress_anchor_converts_persisted_runtime_progress() {
        let (last_journal_seq, last_state_version, snapshot_id) = durable_progress_anchor(
            Some(RuntimeProgressRow {
                last_journal_seq: 41,
                last_state_version: 7,
                last_snapshot_id: Some("snapshot-7".to_owned()),
                operator_target_revision: None,
                operator_strategy_revision: None,
                active_run_session_id: None,
            }),
            true,
        )
        .unwrap();

        assert_eq!(last_journal_seq, 41);
        assert_eq!(last_state_version, 7);
        assert_eq!(snapshot_id.as_deref(), Some("snapshot-7"));
    }

    #[test]
    fn operator_targets_require_matching_persisted_revision_anchor() {
        let err = validate_operator_target_revision(
            Some(&RuntimeProgressRow {
                last_journal_seq: 41,
                last_state_version: 7,
                last_snapshot_id: Some("snapshot-7".to_owned()),
                operator_target_revision: Some("targets-rev-stale".to_owned()),
                operator_strategy_revision: Some("targets-rev-stale".to_owned()),
                active_run_session_id: None,
            }),
            Some("targets-rev-current"),
            true,
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("operator target revision"),
            "{err}"
        );
    }

    #[test]
    fn operator_targets_allow_missing_revision_anchor_without_follow_up_work() {
        validate_operator_target_revision(None, Some("targets-rev-current"), false).unwrap();
    }

    #[test]
    fn strategy_resume_accepts_legacy_target_only_anchor_for_adopted_target_source() {
        validate_operator_revision(
            Some(&RuntimeProgressRow {
                last_journal_seq: 41,
                last_state_version: 7,
                last_snapshot_id: Some("snapshot-7".to_owned()),
                operator_target_revision: Some("targets-rev-9".to_owned()),
                operator_strategy_revision: None,
                active_run_session_id: None,
            }),
            Some("targets-rev-9"),
            Some("targets-rev-9"),
            true,
            true,
        )
        .expect("legacy adopted target-source resume should accept target-only anchor");
    }

    #[test]
    fn strategy_resume_requires_strategy_anchor_when_legacy_fallback_is_disabled() {
        let err = validate_operator_revision(
            Some(&RuntimeProgressRow {
                last_journal_seq: 41,
                last_state_version: 7,
                last_snapshot_id: Some("snapshot-7".to_owned()),
                operator_target_revision: Some("targets-rev-9".to_owned()),
                operator_strategy_revision: None,
                active_run_session_id: None,
            }),
            Some("targets-rev-9"),
            Some("targets-rev-9"),
            false,
            true,
        )
        .expect_err(
            "strategy bundles should still require strategy anchor without legacy fallback",
        );

        assert!(
            err.to_string()
                .contains("operator strategy revision anchor is required"),
            "{err}"
        );
    }

    #[test]
    fn durable_shadow_state_round_trips_attempts_and_artifacts() {
        let attempts = vec![ExecutionAttemptRow {
            attempt_id: "attempt-shadow-1".to_owned(),
            plan_id: "negrisk-submit-family:family-a".to_owned(),
            snapshot_id: "snapshot-7".to_owned(),
            route: "neg-risk".to_owned(),
            scope: "family-a".to_owned(),
            matched_rule_id: Some("family-a-live".to_owned()),
            execution_mode: ExecutionMode::Shadow,
            attempt_no: 1,
            idempotency_key: "idem-attempt-shadow-1".to_owned(),
            run_session_id: None,
        }];
        let artifacts = vec![ShadowExecutionArtifactRow {
            attempt_id: "attempt-shadow-1".to_owned(),
            stream: "neg-risk-shadow-plan".to_owned(),
            payload: serde_json::json!({
                "attempt_id": "attempt-shadow-1",
                "scope": "family-a",
            }),
        }];

        let state = durable_shadow_execution_state(attempts.clone(), artifacts.clone()).unwrap();

        assert_eq!(state.attempts, attempts);
        assert_eq!(state.artifacts, artifacts);
    }

    #[test]
    fn shadow_attempt_row_uses_run_session_id_when_provided() {
        let attempt = attempt_row_with_run_session_id(
            &ExecutionAttemptRow {
                attempt_id: "attempt-shadow-1".to_owned(),
                plan_id: "negrisk-submit-family:family-a".to_owned(),
                snapshot_id: "snapshot-7".to_owned(),
                route: "neg-risk".to_owned(),
                scope: "family-a".to_owned(),
                matched_rule_id: Some("family-a-live".to_owned()),
                execution_mode: ExecutionMode::Shadow,
                attempt_no: 1,
                idempotency_key: "idem-attempt-shadow-1".to_owned(),
                run_session_id: None,
            },
            Some("run-session-1"),
        );

        assert_eq!(attempt.run_session_id.as_deref(), Some("run-session-1"));
    }

    #[test]
    fn durable_shadow_state_requires_artifact_for_every_attempt() {
        let err = durable_shadow_execution_state(
            vec![ExecutionAttemptRow {
                attempt_id: "attempt-shadow-1".to_owned(),
                plan_id: "negrisk-submit-family:family-a".to_owned(),
                snapshot_id: "snapshot-7".to_owned(),
                route: "neg-risk".to_owned(),
                scope: "family-a".to_owned(),
                matched_rule_id: Some("family-a-live".to_owned()),
                execution_mode: ExecutionMode::Shadow,
                attempt_no: 1,
                idempotency_key: "idem-attempt-shadow-1".to_owned(),
                run_session_id: None,
            }],
            Vec::new(),
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("missing durable shadow artifact for attempt"),
            "{err}"
        );
    }
}
