use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    str::FromStr,
};

use domain::{RuntimeMode, RuntimeOverlay};
use observability::{field_keys, span_names};
use persistence::{
    connect_pool_from_env,
    models::{ExecutionAttemptRow, LiveSubmissionRecordRow, PendingReconcileRow},
    ExecutionAttemptRepo, LiveSubmissionRepo, PendingReconcileRepo, RuntimeProgressRepo,
};
use state::{
    ApplyError, ApplyResult, PendingReconcileAnchor, PublishedSnapshot, ReconcileReport,
    RemoteSnapshot, StateApplier, StateStore,
};
use tracing::field;

use crate::bootstrap::{self, BootstrapSource, BootstrapStatus};
use crate::config::NegRiskFamilyLiveTarget;
use crate::input_tasks::InputTaskEvent;
use crate::instrumentation::AppInstrumentation;
use crate::negrisk_live::NegRiskLiveExecutionRecord;
use crate::supervisor::{AppSupervisor, SupervisorSummary};

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

#[derive(Debug, Clone)]
struct DurableLiveStartupState {
    last_journal_seq: i64,
    last_state_version: u64,
    published_snapshot_id: Option<String>,
    pending_reconcile_anchors: Vec<PendingReconcileAnchor>,
    live_execution_records: Vec<NegRiskLiveExecutionRecord>,
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
    neg_risk_live_targets: BTreeMap<String, NegRiskFamilyLiveTarget>,
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
    neg_risk_live_targets: BTreeMap<String, NegRiskFamilyLiveTarget>,
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
    supervisor.seed_neg_risk_live_targets(neg_risk_live_targets);
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
    let durable_state = load_durable_live_startup_state()?;
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

    supervisor.run_startup().map_err(Into::into)
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

fn load_durable_live_startup_state() -> Result<DurableLiveStartupState, Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let pool = connect_pool_from_env().await?;
        let progress = RuntimeProgressRepo.current(&pool).await?;
        let attempts = ExecutionAttemptRepo.list_live_attempts(&pool).await?;
        let submissions_by_attempt = LiveSubmissionRepo
            .list_for_attempts(
                &pool,
                &attempts
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
            durable_live_execution_records(attempts, submissions_by_attempt, &pending_rows)?;

        let (last_journal_seq, last_state_version, published_snapshot_id) = match progress {
            Some(progress) => (
                progress.last_journal_seq,
                u64::try_from(progress.last_state_version).map_err(|_| {
                    boxed_error(format!(
                        "durable runtime progress state version {} is negative",
                        progress.last_state_version
                    ))
                })?,
                progress.last_snapshot_id,
            ),
            None => (0, 0, Some("snapshot-0".to_owned())),
        };

        Ok(DurableLiveStartupState {
            last_journal_seq,
            last_state_version,
            published_snapshot_id,
            pending_reconcile_anchors,
            live_execution_records,
        })
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
