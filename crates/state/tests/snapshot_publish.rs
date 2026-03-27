use chrono::{TimeZone, Utc};
use domain::{
    ConditionId, ExternalFactEvent, MarketId, Order, OrderId, SettlementState, SignedOrderIdentity,
    SubmissionState, TokenId, VenueOrderState,
};
use state::{
    CandidateProjectionReadiness, CandidatePublication, NegRiskFamilyRolloutReadiness, NegRiskView,
    ProjectionReadiness, PublishedSnapshot, StateApplier, StateStore,
};

#[test]
fn successful_reconcile_reanchors_fullset_before_publication() {
    let mut store = sample_store_with_anchored_fullset();
    let report = store.reconcile(state::RemoteSnapshot {
        open_orders: vec![sample_order("order-1", "hash-1")],
        ..state::RemoteSnapshot::empty()
    });
    assert!(report.succeeded);

    let snapshot = PublishedSnapshot::from_store(
        &store,
        ProjectionReadiness::ready_fullset_pending_negrisk("snapshot-17"),
    );

    assert_eq!(snapshot.snapshot_id, "snapshot-17");
    assert_eq!(snapshot.state_version, 1);
    assert_eq!(snapshot.committed_journal_seq, 17);
    assert!(snapshot.fullset_ready);
    assert_eq!(
        snapshot
            .fullset
            .as_ref()
            .map(|view| view.open_orders.clone()),
        Some(vec!["order-1".to_owned()])
    );
}

#[test]
fn fullset_snapshot_publish_does_not_wait_for_negrisk_projection() {
    let store = sample_store_with_anchored_fullset();
    let snapshot = PublishedSnapshot::from_store(
        &store,
        ProjectionReadiness::ready_fullset_pending_negrisk("snapshot-7"),
    );

    assert_eq!(snapshot.snapshot_id, "snapshot-7");
    assert_eq!(snapshot.state_version, 1);
    assert_eq!(snapshot.committed_journal_seq, 17);
    assert_eq!(
        snapshot
            .fullset
            .as_ref()
            .map(|view| view.open_orders.clone()),
        Some(vec!["order-1".to_owned()])
    );
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
    assert_eq!(
        snapshot
            .fullset
            .as_ref()
            .map(|view| view.open_orders.clone()),
        Some(vec!["order-9".to_owned()])
    );
}

#[test]
fn unanchored_fullset_mutation_is_downgraded_before_publication() {
    let mut store = sample_store_with_anchored_fullset();
    store.record_local_order(sample_order("order-2", "hash-2"));

    let snapshot = PublishedSnapshot::from_store(
        &store,
        ProjectionReadiness::ready_fullset_pending_negrisk("snapshot-8"),
    );

    assert_eq!(snapshot.snapshot_id, "snapshot-8");
    assert_eq!(snapshot.state_version, 1);
    assert_eq!(snapshot.committed_journal_seq, 17);
    assert!(!snapshot.fullset_ready);
    assert!(snapshot.fullset.is_none());
}

#[test]
fn unsupported_negrisk_readiness_is_downgraded_before_publication() {
    let store = sample_store_with_anchored_fullset();
    let snapshot =
        PublishedSnapshot::from_store(&store, ProjectionReadiness::new("snapshot-10", true, true));

    assert_eq!(snapshot.snapshot_id, "snapshot-10");
    assert!(snapshot.fullset_ready);
    assert!(snapshot.fullset.is_some());
    assert!(!snapshot.negrisk_ready);
    assert!(snapshot.negrisk.is_none());
}

#[test]
fn candidate_publication_uses_separate_readiness_path_from_published_snapshot() {
    let store = sample_store_with_anchored_fullset();
    let snapshot = PublishedSnapshot::from_store(
        &store,
        ProjectionReadiness::ready_fullset_pending_negrisk("snapshot-11"),
    );
    let candidate_publication = CandidatePublication::from_store(
        &store,
        CandidateProjectionReadiness::failed("candidate-pub-11", "projection timeout"),
    );

    assert!(snapshot.fullset_ready);
    assert!(!snapshot.negrisk_ready);
    assert_eq!(candidate_publication.publication_id, "candidate-pub-11");
    assert!(!candidate_publication.ready);
    assert!(candidate_publication.view.is_none());
    assert_eq!(
        candidate_publication.failure_reason.as_deref(),
        Some("projection timeout")
    );
}

#[test]
fn published_snapshot_exposes_family_level_rollout_readiness() {
    let snapshot = PublishedSnapshot {
        snapshot_id: "snapshot-12".to_owned(),
        state_version: 12,
        committed_journal_seq: 44,
        fullset_ready: true,
        negrisk_ready: true,
        fullset: None,
        negrisk: Some(NegRiskView {
            snapshot_id: "snapshot-12".to_owned(),
            state_version: 12,
            families: vec![NegRiskFamilyRolloutReadiness {
                family_id: "family-a".to_owned(),
                shadow_parity_ready: true,
                recovery_ready: true,
                replay_drift_ready: false,
                fault_injection_ready: true,
                conversion_path_ready: true,
                halt_semantics_ready: true,
            }],
        }),
    };

    assert!(!snapshot.negrisk.as_ref().unwrap().families[0].replay_drift_ready);
}

fn sample_snapshot(snapshot_id: &str) -> PublishedSnapshot {
    let mut store = StateStore::new();
    store.record_local_order(sample_order("order-9", "hash-9"));

    for journal_seq in 1..=9 {
        apply_event(&mut store, journal_seq);
    }

    PublishedSnapshot::from_store(
        &store,
        ProjectionReadiness::ready_fullset_pending_negrisk(snapshot_id),
    )
}

fn sample_store_with_anchored_fullset() -> StateStore {
    let mut store = StateStore::new();
    store.record_local_order(sample_order("order-1", "hash-1"));
    apply_event(&mut store, 17);
    store
}

fn apply_event(store: &mut StateStore, journal_seq: i64) {
    let event = ExternalFactEvent::new(
        "market_ws",
        "session-1",
        format!("evt-{journal_seq}"),
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
