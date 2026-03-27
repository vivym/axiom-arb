use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use app_live::{
    bootstrap::BootstrapStatus, instrumentation::emit_bootstrap_completion_observability,
    supervisor::NegRiskRolloutEvidenceSource, AppRunResult, AppRuntime, AppRuntimeMode,
    AppSupervisor, InputTaskEvent, NegRiskLiveStateSource, NegRiskRolloutEvidence,
    SupervisorPosture, SupervisorSummary,
};
use chrono::Utc;
use domain::{ConditionId, ExternalFactEvent, TokenId};
use observability::{bootstrap_observability, field_keys, span_names};
use state::{ReconcileAttention, RemoteSnapshot};
use tracing::{
    field::{Field, Visit},
    span::{Attributes, Id, Record},
    Event, Metadata, Subscriber,
};

#[test]
fn resume_records_supervisor_and_dispatch_spans_with_zero_rollout_gauges() {
    let observability = bootstrap_observability("app-live-test");
    let mut supervisor = AppSupervisor::for_tests_instrumented(observability.recorder());
    for journal_seq in 35..=41 {
        supervisor.seed_committed_input(sample_input_task_event(journal_seq));
    }
    supervisor.seed_runtime_progress(41, 7, Some("snapshot-7"));
    supervisor.seed_committed_state_version(7);
    supervisor.seed_pending_reconcile_count(0);
    supervisor.seed_neg_risk_rollout_evidence(sample_rollout_evidence("snapshot-7"));

    let (captured_spans, summary) = capture_spans(|| supervisor.resume_once().unwrap());
    let snapshot = observability.registry().snapshot();

    let resume_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::APP_SUPERVISOR_RESUME)
        .expect("resume span missing");
    assert_eq!(
        resume_span.field(field_keys::APP_MODE).map(String::as_str),
        Some("\"live\"")
    );
    assert_eq!(
        resume_span
            .field(field_keys::GLOBAL_POSTURE)
            .map(String::as_str),
        Some("\"healthy\"")
    );
    assert_eq!(
        resume_span
            .field(field_keys::INGRESS_BACKLOG)
            .map(String::as_str),
        Some("0")
    );
    assert_eq!(
        resume_span
            .field(field_keys::FOLLOW_UP_BACKLOG)
            .map(String::as_str),
        Some("0")
    );
    assert_eq!(
        resume_span
            .field(field_keys::BACKLOG_COUNT)
            .map(String::as_str),
        Some("0")
    );
    assert_eq!(
        resume_span
            .field(field_keys::LAST_JOURNAL_SEQ)
            .map(String::as_str),
        Some("41")
    );
    assert_eq!(
        resume_span
            .field(field_keys::STATE_VERSION)
            .map(String::as_str),
        Some("7")
    );
    assert_eq!(
        resume_span
            .field(field_keys::SNAPSHOT_ID)
            .map(String::as_str),
        Some("\"snapshot-7\"")
    );

    let flush_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::APP_DISPATCH_FLUSH)
        .expect("dispatch flush span missing");
    assert_eq!(
        flush_span
            .field(field_keys::BACKLOG_COUNT)
            .map(String::as_str),
        Some("0")
    );
    assert_eq!(
        flush_span
            .field(field_keys::SNAPSHOT_ID)
            .map(String::as_str),
        Some("\"snapshot-7\"")
    );

    assert_eq!(
        snapshot.gauge(observability.metrics().recovery_backlog_count.key()),
        Some(0.0)
    );
    assert_eq!(
        snapshot.gauge(observability.metrics().dispatcher_backlog_count.key()),
        Some(0.0)
    );
    assert_eq!(
        snapshot.gauge(
            observability
                .metrics()
                .neg_risk_live_ready_family_count
                .key()
        ),
        Some(0.0)
    );
    assert_eq!(
        snapshot.gauge(observability.metrics().neg_risk_live_gate_block_count.key()),
        Some(0.0)
    );
    assert_eq!(
        summary
            .neg_risk_rollout_evidence
            .as_ref()
            .map(|evidence| evidence.snapshot_id.as_str()),
        Some("snapshot-7")
    );
    assert_eq!(summary.pending_reconcile_count, 0);
    assert_eq!(summary.global_posture.as_str(), "healthy");
    assert_eq!(summary.ingress_backlog_count, 0);
    assert_eq!(summary.follow_up_backlog_count, 0);
}

#[test]
fn supervisor_records_bootstrap_rollout_metrics_with_explicit_provenance() {
    let observability = bootstrap_observability("app-live-test");
    let mut supervisor = AppSupervisor::for_tests_instrumented(observability.recorder());
    for journal_seq in 35..=41 {
        supervisor.seed_committed_input(sample_input_task_event(journal_seq));
    }
    supervisor.seed_runtime_progress(41, 7, Some("snapshot-7"));
    supervisor.seed_committed_state_version(7);
    supervisor.seed_pending_reconcile_count(0);
    supervisor.seed_neg_risk_rollout_evidence(sample_bootstrap_rollout_evidence("snapshot-7"));

    let (captured_spans, summary) = capture_spans(|| supervisor.resume_once().unwrap());

    let snapshot = observability.registry().snapshot();
    assert_eq!(
        snapshot.gauge(
            observability
                .metrics()
                .neg_risk_live_ready_family_count
                .key()
        ),
        Some(
            summary
                .neg_risk_rollout_evidence
                .as_ref()
                .unwrap()
                .live_ready_family_count as f64
        )
    );
    let completion_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::APP_SUPERVISOR_RESUME)
        .expect("resume span missing");
    assert_eq!(
        completion_span
            .field(field_keys::EVIDENCE_SOURCE)
            .map(String::as_str),
        Some("\"bootstrap\"")
    );
}

#[test]
fn bootstrap_completion_forwarder_does_not_reemit_neg_risk_producer_metrics() {
    let observability = bootstrap_observability("app-live-test");
    let recorder = observability.recorder();
    recorder.record_neg_risk_live_attempt_count(9.0);
    recorder.record_neg_risk_live_ready_family_count(4.0);
    recorder.record_neg_risk_live_gate_block_count(2.0);
    recorder.increment_neg_risk_rollout_parity_mismatch_count(3);

    let result = sample_bootstrap_result_with_rollout_evidence();
    let (captured_spans, ()) =
        capture_spans(|| emit_bootstrap_completion_observability(&recorder, &result));

    let snapshot = observability.registry().snapshot();
    assert_eq!(
        snapshot.gauge(observability.metrics().neg_risk_live_attempt_count.key()),
        Some(9.0)
    );
    assert_eq!(
        snapshot.gauge(
            observability
                .metrics()
                .neg_risk_live_ready_family_count
                .key()
        ),
        Some(4.0)
    );
    assert_eq!(
        snapshot.gauge(observability.metrics().neg_risk_live_gate_block_count.key()),
        Some(2.0)
    );
    assert_eq!(
        snapshot.counter(
            observability
                .metrics()
                .neg_risk_rollout_parity_mismatch_count
                .key()
        ),
        Some(3)
    );
    let completion_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::APP_BOOTSTRAP_COMPLETE)
        .expect("bootstrap completion span missing");
    assert_eq!(
        completion_span
            .field(field_keys::EVIDENCE_SOURCE)
            .map(String::as_str),
        Some("\"bootstrap\"")
    );
}

#[test]
fn bootstrap_completion_forwarder_emits_neutral_rollout_provenance_without_snapshot_claim() {
    let observability = bootstrap_observability("app-live-test");
    let recorder = observability.recorder();
    let result = sample_neutral_rollout_result();

    let (captured_spans, ()) =
        capture_spans(|| emit_bootstrap_completion_observability(&recorder, &result));

    let completion_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::APP_BOOTSTRAP_COMPLETE)
        .expect("bootstrap completion span missing");
    assert_eq!(
        completion_span
            .field(field_keys::EVIDENCE_SOURCE)
            .map(String::as_str),
        Some("\"neutral\"")
    );
}

#[test]
fn flush_dispatch_records_dispatcher_backlog_from_pending_dirty_records() {
    let observability = bootstrap_observability("app-live-test");
    let mut supervisor = AppSupervisor::for_tests_instrumented(observability.recorder());
    supervisor.push_dirty_snapshot(5, false, false);

    let (captured_spans, summary) = capture_spans(|| supervisor.flush_dispatch());
    let snapshot = observability.registry().snapshot();

    let flush_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::APP_DISPATCH_FLUSH)
        .expect("dispatch flush span missing");
    assert_eq!(
        flush_span
            .field(field_keys::BACKLOG_COUNT)
            .map(String::as_str),
        Some("1")
    );
    assert_eq!(
        flush_span
            .field(field_keys::STATE_VERSION)
            .map(String::as_str),
        Some("5")
    );

    assert_eq!(summary.coalesced_versions, vec![5]);
    assert_eq!(
        snapshot.gauge(observability.metrics().dispatcher_backlog_count.key()),
        Some(1.0)
    );
}

#[test]
fn run_once_resets_rollout_gauges_when_bootstrap_publication_fails() {
    let observability = bootstrap_observability("app-live-test");
    let recorder = observability.recorder();
    recorder.record_neg_risk_live_ready_family_count(7.0);
    recorder.record_neg_risk_live_gate_block_count(11.0);

    let failing_snapshot =
        RemoteSnapshot::empty().with_attention(ReconcileAttention::IdentifierMismatch {
            token_id: TokenId::from("token-yes"),
            expected_condition_id: ConditionId::from("condition-a"),
            remote_condition_id: ConditionId::from("condition-b"),
        });
    let mut supervisor =
        AppSupervisor::new_instrumented(AppRuntimeMode::Live, failing_snapshot, recorder);
    let summary = supervisor.run_once().unwrap();
    let snapshot = observability.registry().snapshot();

    assert!(summary.neg_risk_rollout_evidence.is_none());
    assert_eq!(
        snapshot.gauge(
            observability
                .metrics()
                .neg_risk_live_ready_family_count
                .key()
        ),
        Some(0.0)
    );
    assert_eq!(
        snapshot.gauge(observability.metrics().neg_risk_live_gate_block_count.key()),
        Some(0.0)
    );
}

#[test]
fn resume_once_resets_parity_mismatch_dedupe_cache_between_attempts() {
    let observability = bootstrap_observability("app-live-test");
    let mut supervisor = AppSupervisor::for_tests_instrumented(observability.recorder());
    for journal_seq in 35..=41 {
        supervisor.seed_committed_input(sample_input_task_event(journal_seq));
    }
    supervisor.seed_runtime_progress(41, 7, Some("snapshot-7"));
    supervisor.seed_committed_state_version(7);
    supervisor.seed_pending_reconcile_count(0);
    supervisor.seed_neg_risk_rollout_evidence(sample_parity_rollout_evidence("snapshot-7"));

    supervisor.resume_once().unwrap();
    supervisor.resume_once().unwrap();

    assert_eq!(
        observability.registry().snapshot().counter(
            observability
                .metrics()
                .neg_risk_rollout_parity_mismatch_count
                .key()
        ),
        Some(4)
    );
}

#[test]
fn resume_pending_reconcile_mismatch_records_divergence_span_and_counter() {
    let observability = bootstrap_observability("app-live-test");
    let mut supervisor = AppSupervisor::for_tests_instrumented(observability.recorder());
    for journal_seq in 35..=41 {
        supervisor.seed_committed_input(sample_input_task_event(journal_seq));
    }
    supervisor.seed_runtime_progress(41, 7, Some("snapshot-7"));
    supervisor.seed_committed_state_version(7);
    supervisor.seed_pending_reconcile_count(3);

    let (captured_spans, err) = capture_spans(|| supervisor.resume_once().unwrap_err());

    assert!(err.to_string().contains("pending reconcile count"));
    assert_eq!(
        observability
            .registry()
            .snapshot()
            .counter(observability.metrics().divergence_count.key()),
        Some(1)
    );

    let divergence_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::APP_RECOVERY_DIVERGENCE)
        .expect("divergence span missing");
    assert_eq!(
        divergence_span
            .field(field_keys::DIVERGENCE_KIND)
            .map(String::as_str),
        Some("\"pending_reconcile_count_mismatch\"")
    );
}

#[test]
fn resume_missing_durable_rollout_evidence_does_not_record_divergence_signal() {
    let observability = bootstrap_observability("app-live-test");
    let mut supervisor = AppSupervisor::for_tests_instrumented(observability.recorder());
    for journal_seq in 35..=41 {
        supervisor.seed_committed_input(sample_input_task_event(journal_seq));
    }
    supervisor.seed_runtime_progress(41, 7, Some("snapshot-7"));
    supervisor.seed_committed_state_version(7);
    supervisor.seed_pending_reconcile_count(0);

    let (captured_spans, err) = capture_spans(|| supervisor.resume_once().unwrap_err());

    assert!(err.to_string().contains("rollout gate evidence"));
    assert_eq!(
        observability
            .registry()
            .snapshot()
            .counter(observability.metrics().divergence_count.key()),
        None
    );
    assert!(captured_spans
        .iter()
        .all(|span| span.name != span_names::APP_RECOVERY_DIVERGENCE));
}

#[test]
fn resume_missing_live_attempt_anchors_does_not_record_divergence_signal() {
    let observability = bootstrap_observability("app-live-test");
    let mut supervisor = AppSupervisor::for_tests_instrumented(observability.recorder());
    supervisor.seed_runtime_progress(0, 0, Some("snapshot-0"));
    supervisor.seed_committed_state_version(0);
    supervisor.seed_pending_reconcile_count(0);
    supervisor.seed_neg_risk_rollout_evidence(NegRiskRolloutEvidence {
        snapshot_id: "snapshot-0".to_owned(),
        live_ready_family_count: 1,
        blocked_family_count: 0,
        parity_mismatch_count: 0,
    });

    let (captured_spans, err) = capture_spans(|| supervisor.resume_once().unwrap_err());

    assert!(err.to_string().contains("live attempt anchors"));
    assert_eq!(
        observability
            .registry()
            .snapshot()
            .counter(observability.metrics().divergence_count.key()),
        None
    );
    assert!(captured_spans
        .iter()
        .all(|span| span.name != span_names::APP_RECOVERY_DIVERGENCE));
}

#[derive(Debug, Clone)]
struct CapturedSpan {
    name: String,
    fields: BTreeMap<String, String>,
}

impl CapturedSpan {
    fn field(&self, key: &str) -> Option<&String> {
        self.fields.get(key)
    }
}

fn capture_spans<T>(f: impl FnOnce() -> T) -> (Vec<CapturedSpan>, T) {
    let spans = Arc::new(Mutex::new(BTreeMap::<u64, CapturedSpan>::new()));
    let subscriber = CaptureSubscriber {
        spans: Arc::clone(&spans),
        next_id: Arc::new(AtomicU64::new(1)),
    };

    let result = tracing::subscriber::with_default(subscriber, f);
    let captured = spans
        .lock()
        .expect("capture lock poisoned")
        .values()
        .cloned()
        .collect::<Vec<_>>();

    (captured, result)
}

#[derive(Clone)]
struct CaptureSubscriber {
    spans: Arc<Mutex<BTreeMap<u64, CapturedSpan>>>,
    next_id: Arc<AtomicU64>,
}

impl Subscriber for CaptureSubscriber {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn register_callsite(
        &self,
        _metadata: &'static Metadata<'static>,
    ) -> tracing::subscriber::Interest {
        tracing::subscriber::Interest::always()
    }

    fn new_span(&self, attrs: &Attributes<'_>) -> Id {
        let raw_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let id = Id::from_u64(raw_id);
        let mut fields = BTreeMap::new();
        let mut visitor = FieldVisitor {
            fields: &mut fields,
        };
        attrs.record(&mut visitor);

        self.spans.lock().expect("capture lock poisoned").insert(
            raw_id,
            CapturedSpan {
                name: attrs.metadata().name().to_owned(),
                fields,
            },
        );

        id
    }

    fn record(&self, span: &Id, values: &Record<'_>) {
        let span_id = span.clone().into_u64();
        let mut spans = self.spans.lock().expect("capture lock poisoned");
        if let Some(captured) = spans.get_mut(&span_id) {
            let mut visitor = FieldVisitor {
                fields: &mut captured.fields,
            };
            values.record(&mut visitor);
        }
    }

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, _event: &Event<'_>) {}

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}

    fn clone_span(&self, id: &Id) -> Id {
        id.clone()
    }

    fn try_close(&self, _id: Id) -> bool {
        true
    }
}

struct FieldVisitor<'a> {
    fields: &'a mut BTreeMap<String, String>,
}

impl Visit for FieldVisitor<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.fields
            .insert(field.name().to_owned(), format!("{value:?}"));
    }
}

fn sample_input_task_event(journal_seq: i64) -> InputTaskEvent {
    InputTaskEvent::new(
        journal_seq,
        ExternalFactEvent::new(
            "market_ws",
            "session-1",
            format!("evt-{journal_seq}"),
            "v1",
            Utc::now(),
        ),
    )
}

fn sample_rollout_evidence(snapshot_id: &str) -> NegRiskRolloutEvidence {
    NegRiskRolloutEvidence {
        snapshot_id: snapshot_id.to_owned(),
        live_ready_family_count: 0,
        blocked_family_count: 0,
        parity_mismatch_count: 0,
    }
}

fn sample_bootstrap_rollout_evidence(snapshot_id: &str) -> NegRiskRolloutEvidence {
    NegRiskRolloutEvidence {
        snapshot_id: snapshot_id.to_owned(),
        live_ready_family_count: 0,
        blocked_family_count: 0,
        parity_mismatch_count: 2,
    }
}

fn sample_parity_rollout_evidence(snapshot_id: &str) -> NegRiskRolloutEvidence {
    NegRiskRolloutEvidence {
        snapshot_id: snapshot_id.to_owned(),
        live_ready_family_count: 0,
        blocked_family_count: 0,
        parity_mismatch_count: 2,
    }
}

fn sample_bootstrap_result_with_rollout_evidence() -> AppRunResult {
    AppRunResult {
        runtime: AppRuntime::new(AppRuntimeMode::Paper),
        report: state::ReconcileReport {
            succeeded: true,
            promoted_from_bootstrap: true,
            remote_applied: false,
            attention: Vec::new(),
        },
        summary: SupervisorSummary {
            fullset_mode: domain::ExecutionMode::Live,
            negrisk_mode: domain::ExecutionMode::Live,
            neg_risk_live_attempt_count: 5,
            neg_risk_live_state_source: NegRiskLiveStateSource::None,
            bootstrap_status: BootstrapStatus::Ready,
            runtime_mode: domain::RuntimeMode::Healthy,
            pending_reconcile_count: 0,
            last_journal_seq: 12,
            last_state_version: 7,
            published_snapshot_id: Some("snapshot-7".to_owned()),
            published_snapshot_committed_journal_seq: Some(12),
            latest_candidate_revision: None,
            latest_adoptable_revision: None,
            latest_candidate_operator_target_revision: None,
            adoption_provenance_resolved: false,
            neg_risk_rollout_evidence: Some(sample_bootstrap_rollout_evidence("snapshot-7")),
            neg_risk_rollout_evidence_source: NegRiskRolloutEvidenceSource::Bootstrap,
            global_posture: SupervisorPosture::Healthy,
            ingress_backlog_count: 0,
            follow_up_backlog_count: 0,
        },
    }
}

fn sample_neutral_rollout_result() -> AppRunResult {
    AppRunResult {
        runtime: AppRuntime::new(AppRuntimeMode::Paper),
        report: state::ReconcileReport {
            succeeded: true,
            promoted_from_bootstrap: false,
            remote_applied: false,
            attention: Vec::new(),
        },
        summary: SupervisorSummary {
            fullset_mode: domain::ExecutionMode::Live,
            negrisk_mode: domain::ExecutionMode::Shadow,
            neg_risk_live_attempt_count: 0,
            neg_risk_live_state_source: NegRiskLiveStateSource::DurableRestore,
            bootstrap_status: BootstrapStatus::Ready,
            runtime_mode: domain::RuntimeMode::Healthy,
            pending_reconcile_count: 0,
            last_journal_seq: 12,
            last_state_version: 7,
            published_snapshot_id: Some("snapshot-7".to_owned()),
            published_snapshot_committed_journal_seq: Some(12),
            latest_candidate_revision: None,
            latest_adoptable_revision: None,
            latest_candidate_operator_target_revision: None,
            adoption_provenance_resolved: false,
            neg_risk_rollout_evidence: Some(sample_rollout_evidence("snapshot-7")),
            neg_risk_rollout_evidence_source: NegRiskRolloutEvidenceSource::Neutral,
            global_posture: SupervisorPosture::Healthy,
            ingress_backlog_count: 0,
            follow_up_backlog_count: 0,
        },
    }
}
