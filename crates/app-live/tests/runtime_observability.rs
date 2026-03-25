use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use app_live::{AppInstrumentation, AppRuntime, AppRuntimeMode, InputTaskEvent};
use chrono::Utc;
use domain::{ConditionId, ExternalFactEvent, TokenId};
use observability::{
    bootstrap_observability, field_keys, metric_dimensions::ReconcileReason, span_names,
    MetricDimension, MetricDimensions,
};
use state::{ReconcileAttention, RemoteSnapshot};
use tracing::{
    field::{Field, Visit},
    span::{Attributes, Id, Record},
    Event, Metadata, Subscriber,
};

#[test]
fn instrumentation_maps_reconcile_attention_into_repo_owned_dimensions() {
    let observability = bootstrap_observability("app-live-test");
    let instrumentation = AppInstrumentation::enabled(observability.recorder());

    let (captured_spans, report) = capture_spans(|| {
        let mut runtime = AppRuntime::new_instrumented(AppRuntimeMode::Live, instrumentation);
        runtime.reconcile(RemoteSnapshot::empty().with_attention(
            ReconcileAttention::IdentifierMismatch {
                token_id: TokenId::from("token-yes"),
                expected_condition_id: ConditionId::from("condition-a"),
                remote_condition_id: ConditionId::from("condition-b"),
            },
        ))
    });

    assert!(!report.succeeded);

    let dims = MetricDimensions::new([MetricDimension::ReconcileReason(
        ReconcileReason::IdentifierMismatch,
    )]);
    assert_eq!(
        observability
            .registry()
            .snapshot()
            .counter_with_dimensions(observability.metrics().reconcile_attention_total.key(), &dims),
        Some(1)
    );

    let reconcile_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::APP_RUNTIME_RECONCILE)
        .expect("reconcile span missing");
    assert_eq!(
        reconcile_span.field(field_keys::APP_MODE).map(String::as_str),
        Some("\"live\"")
    );
    assert_eq!(
        reconcile_span
            .field(field_keys::PENDING_RECONCILE_COUNT)
            .map(String::as_str),
        Some("0")
    );
}

#[test]
fn publish_snapshot_records_span_identity_and_anchor_fields() {
    let (captured_spans, snapshot) = capture_spans(|| {
        let mut runtime = AppRuntime::new(AppRuntimeMode::Paper);
        assert!(runtime.reconcile(RemoteSnapshot::empty()).succeeded);

        runtime
            .publish_snapshot("snapshot-0")
            .expect("snapshot should publish after reconcile")
    });

    assert_eq!(snapshot.snapshot_id, "snapshot-0");
    assert_eq!(snapshot.state_version, 0);
    assert_eq!(snapshot.committed_journal_seq, 0);

    let publish_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::APP_RUNTIME_PUBLISH_SNAPSHOT)
        .expect("publish snapshot span missing");
    assert_eq!(
        publish_span
            .field(field_keys::SNAPSHOT_ID)
            .map(String::as_str),
        Some("\"snapshot-0\"")
    );
    assert_eq!(
        publish_span
            .field(field_keys::STATE_VERSION)
            .map(String::as_str),
        Some("0")
    );
    assert_eq!(
        publish_span
            .field("committed_journal_seq")
            .map(String::as_str),
        Some("0")
    );
}

#[test]
fn apply_input_records_span_identity_and_result_field() {
    let (captured_spans, apply_result) = capture_spans(|| {
        let mut runtime = AppRuntime::new(AppRuntimeMode::Live);
        assert!(runtime.reconcile(RemoteSnapshot::empty()).succeeded);

        runtime
            .apply_input(InputTaskEvent::out_of_order_user_trade(
                1,
                ExternalFactEvent::new("market_ws", "session-2", "trade-1", "v1", Utc::now()),
            ))
            .expect("apply should succeed")
    });

    assert!(matches!(
        apply_result,
        state::ApplyResult::ReconcileRequired { .. }
    ));

    let apply_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::APP_RUNTIME_APPLY_INPUT)
        .expect("apply input span missing");
    assert_eq!(
        apply_span
            .field(field_keys::APPLY_RESULT)
            .map(String::as_str),
        Some("\"reconcile_required\"")
    );
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

    fn register_callsite(&self, _metadata: &'static Metadata<'static>) -> tracing::subscriber::Interest {
        tracing::subscriber::Interest::always()
    }

    fn new_span(&self, attrs: &Attributes<'_>) -> Id {
        let raw_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let id = Id::from_u64(raw_id);
        let mut fields = BTreeMap::new();
        let mut visitor = FieldVisitor { fields: &mut fields };
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
