use chrono::Utc;
use domain::{ExternalFactEvent, RuntimeMode};
use state::{ProjectionReadiness, PublishedSnapshot, StateApplier, StateStore};

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
