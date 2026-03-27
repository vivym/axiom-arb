use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use domain::{FamilyExclusionReason, IdentifierRecord, MarketRoute, NegRiskVariant};
use observability::{bootstrap_observability, span_names};
use strategy_negrisk::{
    build_family_graph, validate_family, validate_family_instrumented, FamilyValidationStatus,
    NegRiskGraph, NegRiskGraphFamily, NegRiskValidatorInstrumentation,
};
use tracing::{
    field::{Field, Visit},
    span::{Attributes, Id, Record},
    Event, Metadata, Subscriber,
};
use venue_polymarket::NegRiskMarketMetadata;

#[test]
fn graph_builder_groups_conditions_by_event_family() {
    let graph = build_family_graph(sample_identifier_records(), sample_metadata())
        .expect("sample graph should build");

    assert_eq!(graph.families().len(), 2);
    assert!(graph
        .families()
        .iter()
        .any(|family| family.neg_risk_variant == NegRiskVariant::Standard));
    assert!(graph.families().iter().any(|family| family
        .family
        .members
        .iter()
        .any(|member| member.is_placeholder)));
}

#[test]
fn validator_excludes_augmented_or_other_families_from_initial_scope_without_hiding_them() {
    let family = sample_augmented_family();
    let verdict = validate_family(&family, sample_discovery_revision(), "sha256:snapshot-a");

    assert_eq!(verdict.status, FamilyValidationStatus::Excluded);
    assert_eq!(
        verdict.reason,
        Some(FamilyExclusionReason::AugmentedVariant)
    );
}

#[test]
fn validator_recomputes_verdict_when_snapshot_hash_changes() {
    let first_family = sample_placeholder_family();
    let second_family = sample_named_family();

    let first = validate_family(&first_family, 7, "sha256:snapshot-a");
    let second = validate_family(&second_family, 8, "sha256:snapshot-b");

    assert_ne!(first.metadata_snapshot_hash, second.metadata_snapshot_hash);
}

#[test]
fn validator_records_verdict_without_emitting_discovered_family_count() {
    let observability = bootstrap_observability("validator-test");
    let instrumentation = NegRiskValidatorInstrumentation::enabled(observability.recorder());

    let (spans, verdict) = capture_spans(|| {
        validate_family_instrumented(
            &sample_named_family(),
            7,
            "sha256:snapshot-7",
            &instrumentation,
        )
    });

    assert_eq!(verdict.discovery_revision, 7);
    assert!(spans
        .iter()
        .any(|span| span.name == span_names::NEG_RISK_FAMILY_VALIDATION));
    assert_eq!(
        observability.registry().snapshot().gauge(
            observability
                .metrics()
                .neg_risk_family_discovered_count
                .key()
        ),
        None
    );
}

fn sample_identifier_records() -> Vec<IdentifierRecord> {
    vec![
        IdentifierRecord {
            event_id: "event-a".into(),
            event_family_id: "family-a".into(),
            market_id: "market-a".into(),
            condition_id: "condition-a".into(),
            token_id: "token-a".into(),
            outcome_label: "Alice".to_owned(),
            route: MarketRoute::NegRisk,
        },
        IdentifierRecord {
            event_id: "event-b".into(),
            event_family_id: "family-a".into(),
            market_id: "market-b".into(),
            condition_id: "condition-b".into(),
            token_id: "token-b".into(),
            outcome_label: "Other".to_owned(),
            route: MarketRoute::NegRisk,
        },
        IdentifierRecord {
            event_id: "event-c".into(),
            event_family_id: "family-b".into(),
            market_id: "market-c".into(),
            condition_id: "condition-c".into(),
            token_id: "token-c".into(),
            outcome_label: "Placeholder".to_owned(),
            route: MarketRoute::NegRisk,
        },
    ]
}

fn sample_metadata() -> Vec<NegRiskMarketMetadata> {
    vec![
        sample_metadata_row(MetadataRowInput {
            event_family_id: "family-a",
            event_id: "event-a",
            condition_id: "condition-a",
            token_id: "token-a",
            outcome_label: "Alice",
            neg_risk_variant: NegRiskVariant::Standard,
            is_placeholder: false,
            is_other: false,
        }),
        sample_metadata_row(MetadataRowInput {
            event_family_id: "family-a",
            event_id: "event-b",
            condition_id: "condition-b",
            token_id: "token-b",
            outcome_label: "Other",
            neg_risk_variant: NegRiskVariant::Standard,
            is_placeholder: false,
            is_other: true,
        }),
        sample_metadata_row(MetadataRowInput {
            event_family_id: "family-b",
            event_id: "event-c",
            condition_id: "condition-c",
            token_id: "token-c",
            outcome_label: "Placeholder",
            neg_risk_variant: NegRiskVariant::Standard,
            is_placeholder: true,
            is_other: false,
        }),
    ]
}

fn sample_augmented_family() -> NegRiskGraphFamily {
    family_from_graph(
        build_family_graph(
            vec![IdentifierRecord {
                event_id: "event-aug".into(),
                event_family_id: "family-aug".into(),
                market_id: "market-aug".into(),
                condition_id: "condition-aug".into(),
                token_id: "token-aug".into(),
                outcome_label: "Augmented".to_owned(),
                route: MarketRoute::NegRisk,
            }],
            vec![sample_metadata_row(MetadataRowInput {
                event_family_id: "family-aug",
                event_id: "event-aug",
                condition_id: "condition-aug",
                token_id: "token-aug",
                outcome_label: "Augmented",
                neg_risk_variant: NegRiskVariant::Augmented,
                is_placeholder: false,
                is_other: false,
            })],
        )
        .expect("augmented graph should build"),
    )
}

fn sample_placeholder_family() -> NegRiskGraphFamily {
    family_from_graph(
        build_family_graph(
            vec![IdentifierRecord {
                event_id: "event-placeholder".into(),
                event_family_id: "family-placeholder".into(),
                market_id: "market-placeholder".into(),
                condition_id: "condition-placeholder".into(),
                token_id: "token-placeholder".into(),
                outcome_label: "Placeholder".to_owned(),
                route: MarketRoute::NegRisk,
            }],
            vec![sample_metadata_row(MetadataRowInput {
                event_family_id: "family-placeholder",
                event_id: "event-placeholder",
                condition_id: "condition-placeholder",
                token_id: "token-placeholder",
                outcome_label: "Placeholder",
                neg_risk_variant: NegRiskVariant::Standard,
                is_placeholder: true,
                is_other: false,
            })],
        )
        .expect("placeholder graph should build"),
    )
}

fn sample_named_family() -> NegRiskGraphFamily {
    family_from_graph(
        build_family_graph(
            vec![IdentifierRecord {
                event_id: "event-named".into(),
                event_family_id: "family-named".into(),
                market_id: "market-named".into(),
                condition_id: "condition-named".into(),
                token_id: "token-named".into(),
                outcome_label: "Alice".to_owned(),
                route: MarketRoute::NegRisk,
            }],
            vec![sample_metadata_row(MetadataRowInput {
                event_family_id: "family-named",
                event_id: "event-named",
                condition_id: "condition-named",
                token_id: "token-named",
                outcome_label: "Alice",
                neg_risk_variant: NegRiskVariant::Standard,
                is_placeholder: false,
                is_other: false,
            })],
        )
        .expect("named graph should build"),
    )
}

fn family_from_graph(graph: NegRiskGraph) -> NegRiskGraphFamily {
    graph
        .families()
        .first()
        .expect("sample family should exist")
        .clone()
}

fn sample_discovery_revision() -> i64 {
    7
}

#[derive(Debug, Clone)]
struct CapturedSpan {
    name: String,
    fields: BTreeMap<String, String>,
}

#[allow(dead_code)]
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

struct MetadataRowInput<'a> {
    event_family_id: &'a str,
    event_id: &'a str,
    condition_id: &'a str,
    token_id: &'a str,
    outcome_label: &'a str,
    neg_risk_variant: NegRiskVariant,
    is_placeholder: bool,
    is_other: bool,
}

fn sample_metadata_row(input: MetadataRowInput<'_>) -> NegRiskMarketMetadata {
    NegRiskMarketMetadata {
        event_family_id: input.event_family_id.to_owned(),
        event_id: input.event_id.to_owned(),
        condition_id: input.condition_id.to_owned(),
        token_id: input.token_id.to_owned(),
        outcome_label: input.outcome_label.to_owned(),
        route: MarketRoute::NegRisk,
        enable_neg_risk: Some(true),
        neg_risk_augmented: Some(matches!(input.neg_risk_variant, NegRiskVariant::Augmented)),
        neg_risk_variant: input.neg_risk_variant,
        is_placeholder: input.is_placeholder,
        is_other: input.is_other,
        discovery_revision: 7,
        metadata_snapshot_hash: "sha256:test-snapshot".to_owned(),
    }
}
