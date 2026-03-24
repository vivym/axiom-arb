use chrono::{TimeZone, Utc};
use domain::{
    ConditionId, ExternalFactEvent, MarketId, Order, OrderId, SettlementState,
    SignedOrderIdentity, SubmissionState, TokenId, VenueOrderState,
};
use state::{ProjectionReadiness, PublishedSnapshot, StateApplier, StateStore};

#[test]
fn fullset_snapshot_publish_does_not_wait_for_negrisk_projection() {
    let store = sample_store_with_fullset_only();
    let snapshot = PublishedSnapshot::from_store(
        &store,
        ProjectionReadiness::ready_fullset_pending_negrisk("snapshot-7"),
    );

    assert_eq!(snapshot.snapshot_id, "snapshot-7");
    assert_eq!(snapshot.state_version, 1);
    assert_eq!(snapshot.committed_journal_seq, 17);
    assert!(snapshot.fullset.is_some());
    assert!(snapshot.negrisk.is_none());
    assert!(snapshot.fullset_ready);
    assert!(!snapshot.negrisk_ready);
}

#[test]
fn published_snapshot_keeps_projection_readiness_flags_explicit() {
    let snapshot = sample_snapshot("snapshot-9");

    assert_eq!(snapshot.snapshot_id, "snapshot-9");
    assert_eq!(snapshot.state_version, 9);
    assert_eq!(snapshot.committed_journal_seq, 9);
    assert!(snapshot.fullset_ready);
    assert!(!snapshot.negrisk_ready);
}

fn sample_snapshot(snapshot_id: &str) -> PublishedSnapshot {
    let mut store = StateStore::new();

    for journal_seq in 1..=9 {
        apply_event(&mut store, journal_seq);
    }

    store.record_local_order(sample_order("order-9", "hash-9"));

    PublishedSnapshot::from_store(
        &store,
        ProjectionReadiness::ready_fullset_pending_negrisk(snapshot_id),
    )
}

fn sample_store_with_fullset_only() -> StateStore {
    let mut store = StateStore::new();
    apply_event(&mut store, 17);
    store.record_local_order(sample_order("order-1", "hash-1"));
    store
}

fn apply_event(store: &mut StateStore, journal_seq: i64) {
    let event = ExternalFactEvent::new(
        "market_ws",
        "session-1",
        &format!("evt-{journal_seq}"),
        "v1",
        Utc.with_ymd_and_hms(2026, 3, 24, 10, 0, 0).unwrap(),
    );

    StateApplier::new(store).apply(journal_seq, event).unwrap();
}

fn sample_order(order_id: &str, signed_order_hash: &str) -> Order {
    Order {
        order_id: OrderId::from(order_id),
        market_id: MarketId::from("market-a"),
        condition_id: ConditionId::from("condition-1"),
        token_id: TokenId::from("token-1"),
        price: rust_decimal::Decimal::new(45, 2),
        quantity: rust_decimal::Decimal::new(10, 0),
        submission_state: SubmissionState::Acked,
        venue_state: VenueOrderState::Live,
        settlement_state: SettlementState::Matched,
        signed_order: Some(SignedOrderIdentity {
            signed_order_hash: signed_order_hash.to_owned(),
            salt: format!("salt-{order_id}"),
            nonce: format!("nonce-{order_id}"),
            signature: format!("sig-{order_id}"),
        }),
    }
}
