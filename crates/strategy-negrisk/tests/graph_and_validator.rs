use domain::{FamilyExclusionReason, IdentifierRecord, MarketRoute, NegRiskVariant};
use strategy_negrisk::{
    build_family_graph, validate_family, FamilyValidationStatus, NegRiskGraph, NegRiskGraphFamily,
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
        sample_metadata_row(
            "family-a",
            "event-a",
            "condition-a",
            "token-a",
            "Alice",
            NegRiskVariant::Standard,
            false,
            false,
        ),
        sample_metadata_row(
            "family-a",
            "event-b",
            "condition-b",
            "token-b",
            "Other",
            NegRiskVariant::Standard,
            false,
            true,
        ),
        sample_metadata_row(
            "family-b",
            "event-c",
            "condition-c",
            "token-c",
            "Placeholder",
            NegRiskVariant::Standard,
            true,
            false,
        ),
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
            vec![sample_metadata_row(
                "family-aug",
                "event-aug",
                "condition-aug",
                "token-aug",
                "Augmented",
                NegRiskVariant::Augmented,
                false,
                false,
            )],
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
            vec![sample_metadata_row(
                "family-placeholder",
                "event-placeholder",
                "condition-placeholder",
                "token-placeholder",
                "Placeholder",
                NegRiskVariant::Standard,
                true,
                false,
            )],
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
            vec![sample_metadata_row(
                "family-named",
                "event-named",
                "condition-named",
                "token-named",
                "Alice",
                NegRiskVariant::Standard,
                false,
                false,
            )],
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

fn sample_metadata_row(
    event_family_id: &str,
    event_id: &str,
    condition_id: &str,
    token_id: &str,
    outcome_label: &str,
    neg_risk_variant: NegRiskVariant,
    is_placeholder: bool,
    is_other: bool,
) -> NegRiskMarketMetadata {
    NegRiskMarketMetadata {
        event_family_id: event_family_id.to_owned(),
        event_id: event_id.to_owned(),
        condition_id: condition_id.to_owned(),
        token_id: token_id.to_owned(),
        outcome_label: outcome_label.to_owned(),
        route: MarketRoute::NegRisk,
        enable_neg_risk: Some(true),
        neg_risk_augmented: Some(matches!(neg_risk_variant, NegRiskVariant::Augmented)),
        neg_risk_variant,
        is_placeholder,
        is_other,
        discovery_revision: 7,
        metadata_snapshot_hash: "sha256:test-snapshot".to_owned(),
    }
}
