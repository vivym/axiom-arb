use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use domain::{ExecutionMode, ExecutionRequest};
use execution::{
    orchestrator::ExecutionPlanningInput,
    plans::ExecutionPlan,
    sink::{VenueSink, VenueSinkError},
};
use tracing::{
    field::{Field, Visit},
    span::{Attributes, Id, Record},
    Event, Metadata, Subscriber,
};

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CapturedSpan {
    pub name: String,
    pub fields: BTreeMap<String, String>,
}

#[allow(dead_code)]
impl CapturedSpan {
    pub fn field(&self, key: &str) -> Option<&String> {
        self.fields.get(key)
    }
}

#[allow(dead_code)]
pub fn capture_spans<T>(f: impl FnOnce() -> T) -> (Vec<CapturedSpan>, T) {
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

pub fn sample_planning_input(execution_mode: ExecutionMode) -> ExecutionPlanningInput {
    ExecutionPlanningInput::new(
        ExecutionRequest {
            request_id: "request-1".to_owned(),
            decision_input_id: "intent-1".to_owned(),
            snapshot_id: "snapshot-1".to_owned(),
            route: "full-set".to_owned(),
            scope: "default".to_owned(),
            activation_mode: execution_mode,
            matched_rule_id: None,
        },
        execution_mode,
        ExecutionPlan::FullSetBuyThenMerge {
            condition_id: domain::ConditionId::from("condition-1"),
        },
    )
}

#[derive(Debug, Clone, Copy)]
pub struct FailingVenueSink;

impl VenueSink for FailingVenueSink {
    fn sink_kind(&self) -> &'static str {
        "test_failing"
    }

    fn execute(
        &self,
        _plan: &ExecutionPlan,
        _attempt: &domain::ExecutionAttemptContext,
    ) -> Result<domain::ExecutionReceipt, VenueSinkError> {
        Err(VenueSinkError::Rejected {
            reason: "planned sink failure".to_owned(),
        })
    }
}

#[derive(Clone)]
#[allow(dead_code)]
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

#[allow(dead_code)]
struct FieldVisitor<'a> {
    fields: &'a mut BTreeMap<String, String>,
}

impl Visit for FieldVisitor<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.fields
            .insert(field.name().to_owned(), format!("{value:?}"));
    }
}
