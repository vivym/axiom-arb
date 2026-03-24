use domain::{
    EventFamilyId, FamilyHaltPolicy, FamilyHaltState, FamilyHaltStatus, HaltPriority,
    IdentifierMap, IdentifierRecord, InventoryBucket, MarketRoute, NegRiskExposureError,
    NegRiskExposureVector, NegRiskFamily, NegRiskNode, TokenId,
};
use rust_decimal::Decimal;

#[test]
fn standard_family_keeps_placeholder_and_other_members_visible_for_validation() {
    let family = sample_family(["Alice", "Bob", "Other"]);

    assert_eq!(family.members.len(), 3);
    assert!(family.members.iter().any(|member| member.is_other));
}

#[test]
fn member_level_exposure_preserves_bucket_breakdown() {
    let exposure =
        NegRiskExposureVector::from_inventory(sample_inventory(), sample_identifier_map());
    let exposure = exposure.expect("sample inventory should be accepted");

    assert_eq!(exposure.members.len(), 2);
    assert!(exposure.members[0]
        .bucket_quantities
        .contains_key(&InventoryBucket::MatchedUnsettled));
}

#[test]
fn duplicate_token_bucket_rows_are_accumulated_safely() {
    let exposure = NegRiskExposureVector::from_inventory(
        sample_inventory_with_duplicate(),
        sample_identifier_map(),
    )
    .expect("duplicate rows in the same family should be accumulated");

    let member = exposure
        .members
        .iter()
        .find(|member| member.token_id == TokenId::from("token-a"))
        .expect("token-a should exist");

    assert_eq!(
        member
            .bucket_quantities
            .get(&InventoryBucket::MatchedUnsettled),
        Some(&Decimal::new(5, 0))
    );
}

#[test]
fn mixed_family_inventory_input_is_rejected() {
    let err = NegRiskExposureVector::from_inventory(
        sample_mixed_family_inventory(),
        sample_mixed_family_identifier_map(),
    )
    .expect_err("mixed family inventory should be rejected");

    assert!(matches!(err, NegRiskExposureError::MixedFamily { .. }));
}

#[test]
fn non_negrisk_route_rows_are_rejected() {
    let err = NegRiskExposureVector::from_inventory(
        sample_standard_route_inventory(),
        sample_standard_route_identifier_map(),
    )
    .expect_err("standard route inventory should be rejected");

    assert!(matches!(
        err,
        NegRiskExposureError::InvalidRoute {
            route: Some(MarketRoute::Standard),
            ..
        }
    ));
}

#[test]
fn family_halt_precedence_stays_below_global_halt_and_above_strategy_filters() {
    let policy = FamilyHaltPolicy::default_block_new_risk();

    assert_eq!(policy.priority(), HaltPriority::Family);
}

#[test]
fn stale_family_halt_remains_blocking_until_reconfirmed_or_cleared() {
    let mut halt = FamilyHaltState::active("family-1", "sha256:snapshot-a");
    let status = halt.reconcile_against_snapshot_hash("sha256:snapshot-b");

    assert_eq!(status, FamilyHaltStatus::StaleBlocking);
    assert_eq!(halt.status(), FamilyHaltStatus::StaleBlocking);

    let revalidated = halt.revalidate_against_snapshot_hash("sha256:snapshot-b");
    assert_eq!(revalidated, FamilyHaltStatus::ActiveBlocking);
    assert_eq!(halt.status(), FamilyHaltStatus::ActiveBlocking);
    assert_eq!(halt.metadata_snapshot_hash(), "sha256:snapshot-b");

    let cleared = halt.clear();
    assert_eq!(cleared, FamilyHaltStatus::Cleared);
    assert_eq!(halt.status(), FamilyHaltStatus::Cleared);

    let still_cleared = halt.reconcile_against_snapshot_hash("sha256:snapshot-c");
    assert_eq!(still_cleared, FamilyHaltStatus::Cleared);
    assert_eq!(halt.status(), FamilyHaltStatus::Cleared);
}

fn sample_family(outcomes: [&str; 3]) -> NegRiskFamily {
    NegRiskFamily {
        family_id: EventFamilyId::from("family-a"),
        route: MarketRoute::NegRisk,
        members: outcomes
            .into_iter()
            .enumerate()
            .map(|(index, outcome_label)| NegRiskNode {
                token_id: TokenId::from(format!("token-{index}")),
                outcome_label: outcome_label.to_owned(),
                is_placeholder: outcome_label == "Placeholder",
                is_other: outcome_label == "Other",
                route: MarketRoute::NegRisk,
            })
            .collect(),
    }
}

fn sample_inventory() -> Vec<(TokenId, InventoryBucket, Decimal)> {
    vec![
        (
            TokenId::from("token-a"),
            InventoryBucket::MatchedUnsettled,
            Decimal::new(2, 0),
        ),
        (
            TokenId::from("token-b"),
            InventoryBucket::Free,
            Decimal::new(1, 0),
        ),
    ]
}

fn sample_inventory_with_duplicate() -> Vec<(TokenId, InventoryBucket, Decimal)> {
    vec![
        (
            TokenId::from("token-a"),
            InventoryBucket::MatchedUnsettled,
            Decimal::new(2, 0),
        ),
        (
            TokenId::from("token-a"),
            InventoryBucket::MatchedUnsettled,
            Decimal::new(3, 0),
        ),
        (
            TokenId::from("token-b"),
            InventoryBucket::Free,
            Decimal::new(1, 0),
        ),
    ]
}

fn sample_identifier_map() -> IdentifierMap {
    IdentifierMap::from_records([
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
            event_id: "event-a".into(),
            event_family_id: "family-a".into(),
            market_id: "market-a".into(),
            condition_id: "condition-a".into(),
            token_id: "token-b".into(),
            outcome_label: "Bob".to_owned(),
            route: MarketRoute::NegRisk,
        },
    ])
    .expect("identifier map should build")
}

fn sample_mixed_family_inventory() -> Vec<(TokenId, InventoryBucket, Decimal)> {
    vec![
        (
            TokenId::from("token-a"),
            InventoryBucket::MatchedUnsettled,
            Decimal::new(2, 0),
        ),
        (
            TokenId::from("token-b"),
            InventoryBucket::Free,
            Decimal::new(1, 0),
        ),
    ]
}

fn sample_mixed_family_identifier_map() -> IdentifierMap {
    IdentifierMap::from_records([
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
            event_family_id: "family-b".into(),
            market_id: "market-b".into(),
            condition_id: "condition-b".into(),
            token_id: "token-b".into(),
            outcome_label: "Bob".to_owned(),
            route: MarketRoute::NegRisk,
        },
    ])
    .expect("identifier map should build")
}

fn sample_standard_route_inventory() -> Vec<(TokenId, InventoryBucket, Decimal)> {
    vec![(
        TokenId::from("token-a"),
        InventoryBucket::MatchedUnsettled,
        Decimal::new(2, 0),
    )]
}

fn sample_standard_route_identifier_map() -> IdentifierMap {
    IdentifierMap::from_records([IdentifierRecord {
        event_id: "event-a".into(),
        event_family_id: "family-a".into(),
        market_id: "market-a".into(),
        condition_id: "condition-a".into(),
        token_id: "token-a".into(),
        outcome_label: "Alice".to_owned(),
        route: MarketRoute::Standard,
    }])
    .expect("identifier map should build")
}
