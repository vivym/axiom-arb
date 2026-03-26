use chrono::Utc;
use domain::{ExternalFactEvent, RuntimeMode};
use state::{
    PendingReconcileAnchor, ProjectionReadiness, PublishedSnapshot, StateApplier, StateConfidence,
    StateStore,
};

#[test]
fn restore_committed_anchor_rehydrates_snapshot_and_next_apply_progress() {
    let mut store = StateStore::new();
    store.restore_committed_anchor(7, 41);

    assert_eq!(store.mode(), RuntimeMode::Healthy);
    assert_eq!(store.mode_overlay(), None);
    assert!(store.first_reconcile_succeeded());
    assert_eq!(store.state_version(), 7);
    assert_eq!(store.last_applied_journal_seq(), Some(41));
    assert_eq!(store.last_consumed_journal_seq(), Some(41));

    let snapshot = PublishedSnapshot::from_store(
        &store,
        ProjectionReadiness::ready_fullset_pending_negrisk("snapshot-7"),
    );
    assert_eq!(snapshot.snapshot_id, "snapshot-7");
    assert_eq!(snapshot.state_version, 7);
    assert_eq!(snapshot.committed_journal_seq, 41);

    let applied = StateApplier::new(&mut store)
        .apply(
            42,
            ExternalFactEvent::new("market_ws", "session-1", "evt-42", "v1", Utc::now()),
        )
        .unwrap();

    assert!(matches!(
        applied,
        state::ApplyResult::Applied {
            journal_seq: 42,
            state_version: 8,
            ..
        }
    ));
    assert_eq!(store.state_version(), 8);
    assert_eq!(store.last_applied_journal_seq(), Some(42));
}

#[test]
fn restore_committed_anchor_preserves_durable_pending_reconcile_state() {
    let mut store = StateStore::new();

    StateApplier::new(&mut store)
        .apply(
            19,
            ExternalFactEvent::negrisk_live_submit_observed(
                "session-live",
                "evt-1",
                "attempt-family-a-1",
                "family-a",
                "submission-family-a-1",
                Utc::now(),
            ),
        )
        .unwrap();

    assert_eq!(store.pending_reconcile_count(), 1);
    assert_eq!(store.mode(), RuntimeMode::Reconciling);
    assert_eq!(
        store.scope_confidence("family-a"),
        StateConfidence::Uncertain
    );
    assert_eq!(store.pending_reconcile_anchors().len(), 1);

    store.restore_committed_anchor(7, 41);

    assert_eq!(store.state_version(), 7);
    assert_eq!(store.last_applied_journal_seq(), Some(41));
    assert_eq!(store.last_consumed_journal_seq(), Some(41));
    assert_eq!(store.pending_reconcile_count(), 1);
    assert_eq!(store.mode(), RuntimeMode::Reconciling);
    assert_eq!(
        store.mode_overlay(),
        Some(domain::RuntimeOverlay::CancelOnly)
    );
    assert_eq!(
        store.scope_confidence("family-a"),
        StateConfidence::Uncertain
    );
    assert!(store.first_reconcile_succeeded());
    assert!(!store.allows_automatic_repair());
}

#[test]
fn restore_pending_reconcile_anchor_rehydrates_exact_anchor_fields_and_state_confidence() {
    let mut store = StateStore::new();
    store.restore_committed_anchor(7, 41);

    let anchor = PendingReconcileAnchor::new(
        "pending-family-a-1",
        "submission-family-a-1",
        "family-a",
        "live_submit_provider",
        "accepted but unconfirmed on restore",
    );

    store.restore_pending_reconcile_anchor(anchor.clone());

    assert_eq!(store.pending_reconcile_count(), 1);
    assert_eq!(store.pending_reconcile_anchors(), vec![anchor]);
    assert_eq!(store.mode(), RuntimeMode::Reconciling);
    assert_eq!(
        store.mode_overlay(),
        Some(domain::RuntimeOverlay::CancelOnly)
    );
    assert_eq!(
        store.state_confidence("family-a"),
        StateConfidence::Uncertain
    );
    assert_eq!(store.state_confidence("family-b"), StateConfidence::Certain);
    assert!(!store.allows_automatic_repair());
}

#[test]
fn terminal_live_reconcile_clears_restored_pending_anchor_and_scope_uncertainty() {
    let mut store = StateStore::new();
    store.restore_committed_anchor(7, 41);
    store.restore_pending_reconcile_anchor(PendingReconcileAnchor::new(
        "pending-family-a-1",
        "submission-family-a-1",
        "family-a",
        "live_submit_provider",
        "accepted but unconfirmed on restore",
    ));

    let result = StateApplier::new(&mut store)
        .apply(
            42,
            ExternalFactEvent::negrisk_live_reconcile_observed(
                "session-live",
                "evt-2",
                "pending-family-a-1",
                "family-a",
                true,
                Utc::now(),
            ),
        )
        .unwrap();

    assert!(matches!(
        result,
        state::ApplyResult::Applied {
            journal_seq: 42,
            state_version: 8,
            ..
        }
    ));
    assert_eq!(store.pending_reconcile_count(), 0);
    assert_eq!(store.mode(), RuntimeMode::Healthy);
    assert_eq!(store.state_confidence("family-a"), StateConfidence::Certain);
    assert!(store.allows_automatic_repair());
}
