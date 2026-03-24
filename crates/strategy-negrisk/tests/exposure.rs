use domain::{
    IdentifierMap, IdentifierRecord, InventoryBucket, MarketRoute, NegRiskExposureError,
    NegRiskVariant,
};
use strategy_negrisk::{build_family_graph, reconstruct_family_exposure, NegRiskGraphFamily};
use venue_polymarket::NegRiskMarketMetadata;

#[test]
fn family_exposure_reconstructs_member_vectors_and_rollups() {
    let records = sample_identifier_records();
    let family = sample_family(&records);
    let identifier_map = IdentifierMap::from_records(records).expect("identifier map should build");

    let exposure =
        reconstruct_family_exposure(&family, sample_inventory_rows(), &identifier_map).unwrap();

    assert_eq!(exposure.family_id.as_str(), "family-1");
    assert_eq!(exposure.rollup.member_count, 2);
    assert_eq!(
        exposure
            .rollup
            .bucket_quantities
            .get(&InventoryBucket::Free),
        Some(&decimal("4"))
    );
    assert_eq!(
        exposure
            .rollup
            .bucket_quantities
            .get(&InventoryBucket::Redeemable),
        Some(&decimal("1"))
    );
    assert_eq!(exposure.members.len(), 2);

    let first = exposure
        .members
        .iter()
        .find(|member| member.token_id.as_str() == "token-1")
        .unwrap();
    assert_eq!(first.condition_id.as_str(), "condition-1");
    assert_eq!(first.outcome_label, "Alice");
    assert_eq!(
        first.bucket_quantities.get(&InventoryBucket::Free),
        Some(&decimal("3"))
    );
    assert_eq!(
        first.bucket_quantities.get(&InventoryBucket::Redeemable),
        Some(&decimal("1"))
    );

    let second = exposure
        .members
        .iter()
        .find(|member| member.token_id.as_str() == "token-2")
        .unwrap();
    assert_eq!(second.condition_id.as_str(), "condition-2");
    assert_eq!(second.outcome_label, "Bob");
    assert_eq!(
        second.bucket_quantities.get(&InventoryBucket::Free),
        Some(&decimal("1"))
    );
}

#[test]
fn family_exposure_ignores_tokens_not_in_the_target_family() {
    let records = sample_identifier_records();
    let family = sample_family(&records);
    let identifier_map = IdentifierMap::from_records(records).expect("identifier map should build");

    let exposure =
        reconstruct_family_exposure(&family, inventory_with_unrelated_token(), &identifier_map)
            .expect("unrelated family token should be ignored");

    assert_eq!(exposure.family_id.as_str(), "family-1");
    assert_eq!(exposure.members.len(), 2);
    assert!(exposure
        .members
        .iter()
        .all(|member| member.token_id.as_str() != "token-3"));
}

#[test]
fn family_exposure_errors_when_family_token_cannot_resolve_condition() {
    let records = sample_identifier_records();
    let family = sample_family(&records);
    let identifier_map = IdentifierMap::from_records(vec![records[1].clone()])
        .expect("identifier map should build from partial records");

    let err = reconstruct_family_exposure(&family, sample_inventory_rows(), &identifier_map)
        .expect_err("missing mapping should fail");

    assert!(matches!(
        err,
        NegRiskExposureError::MissingTokenCondition { token_id } if token_id.as_str() == "token-1"
    ));
}

fn sample_family(records: &[IdentifierRecord]) -> NegRiskGraphFamily {
    build_family_graph(records.to_vec(), sample_metadata())
        .expect("graph should build")
        .families()
        .first()
        .expect("family should exist")
        .clone()
}

fn sample_identifier_records() -> Vec<IdentifierRecord> {
    vec![
        IdentifierRecord {
            event_id: "event-1".into(),
            event_family_id: "family-1".into(),
            market_id: "market-1".into(),
            condition_id: "condition-1".into(),
            token_id: "token-1".into(),
            outcome_label: "Alice".to_owned(),
            route: MarketRoute::NegRisk,
        },
        IdentifierRecord {
            event_id: "event-2".into(),
            event_family_id: "family-1".into(),
            market_id: "market-2".into(),
            condition_id: "condition-2".into(),
            token_id: "token-2".into(),
            outcome_label: "Bob".to_owned(),
            route: MarketRoute::NegRisk,
        },
        IdentifierRecord {
            event_id: "event-3".into(),
            event_family_id: "family-2".into(),
            market_id: "market-3".into(),
            condition_id: "condition-3".into(),
            token_id: "token-3".into(),
            outcome_label: "Carol".to_owned(),
            route: MarketRoute::NegRisk,
        },
    ]
}

fn sample_metadata() -> Vec<NegRiskMarketMetadata> {
    vec![
        metadata_row("family-1", "event-1", "condition-1", "token-1", "Alice"),
        metadata_row("family-1", "event-2", "condition-2", "token-2", "Bob"),
        metadata_row("family-2", "event-3", "condition-3", "token-3", "Carol"),
    ]
}

fn metadata_row(
    event_family_id: &str,
    event_id: &str,
    condition_id: &str,
    token_id: &str,
    outcome_label: &str,
) -> NegRiskMarketMetadata {
    NegRiskMarketMetadata {
        event_family_id: event_family_id.to_owned(),
        event_id: event_id.to_owned(),
        condition_id: condition_id.to_owned(),
        token_id: token_id.to_owned(),
        outcome_label: outcome_label.to_owned(),
        route: MarketRoute::NegRisk,
        enable_neg_risk: Some(true),
        neg_risk_augmented: Some(false),
        neg_risk_variant: NegRiskVariant::Standard,
        is_placeholder: false,
        is_other: false,
        discovery_revision: 7,
        metadata_snapshot_hash: "sha256:test".to_owned(),
    }
}

fn sample_inventory_rows() -> Vec<(domain::TokenId, InventoryBucket, rust_decimal::Decimal)> {
    vec![
        ("token-1".into(), InventoryBucket::Free, decimal("3")),
        ("token-1".into(), InventoryBucket::Redeemable, decimal("1")),
        ("token-2".into(), InventoryBucket::Free, decimal("1")),
    ]
}

fn inventory_with_unrelated_token() -> Vec<(domain::TokenId, InventoryBucket, rust_decimal::Decimal)>
{
    let mut rows = sample_inventory_rows();
    rows.push(("token-3".into(), InventoryBucket::Quarantined, decimal("9")));
    rows
}

fn decimal(value: &str) -> rust_decimal::Decimal {
    value.parse().expect("decimal should parse")
}
