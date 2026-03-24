use chrono::Utc;
use domain::ExternalFactEvent;
use state::{ApplyResult, StateApplier, StateStore};

#[test]
fn duplicate_fact_returns_duplicate_anchor_without_mutating_state_version() {
    let mut store = StateStore::new();
    let mut applier = StateApplier::new(&mut store);
    let event = ExternalFactEvent::new("market_ws", "session-1", "evt-1", "v1", Utc::now());

    let first = applier.apply(17, event.clone()).unwrap();
    let duplicate = applier.apply(18, event).unwrap();

    assert!(matches!(first, ApplyResult::Applied { .. }));
    assert!(matches!(
        duplicate,
        ApplyResult::Duplicate {
            duplicate_of_journal_seq: 17,
            ..
        }
    ));
}

#[test]
fn replaying_same_fact_at_same_journal_seq_returns_duplicate() {
    let mut store = StateStore::new();
    let mut applier = StateApplier::new(&mut store);
    let event = ExternalFactEvent::new("market_ws", "session-1", "evt-1", "v1", Utc::now());

    let first = applier.apply(17, event.clone()).unwrap();
    let replay = applier.apply(17, event).unwrap();

    assert!(matches!(first, ApplyResult::Applied { .. }));
    assert!(matches!(
        replay,
        ApplyResult::Duplicate {
            journal_seq: 17,
            duplicate_of_journal_seq: 17,
            state_version: 1,
        }
    ));
}

#[test]
fn consumed_journal_seq_cannot_be_reused_after_duplicate() {
    let mut store = StateStore::new();
    let mut applier = StateApplier::new(&mut store);
    let event_a = ExternalFactEvent::new("market_ws", "session-1", "evt-1", "v1", Utc::now());
    let event_b = ExternalFactEvent::new("market_ws", "session-1", "evt-2", "v1", Utc::now());

    applier.apply(17, event_a.clone()).unwrap();
    let duplicate = applier.apply(18, event_a).unwrap();
    assert!(matches!(duplicate, ApplyResult::Duplicate { .. }));

    let err = applier.apply(18, event_b).unwrap_err();
    assert_eq!(
        err.to_string(),
        "journal sequence 18 is already bound to a different fact"
    );
}

#[test]
fn out_of_order_fact_creates_reconcile_required_pending_ref() {
    let mut store = StateStore::new();
    let mut applier = StateApplier::new(&mut store);
    let event = sample_out_of_order_user_trade();

    let result = applier.apply(19, event).unwrap();
    assert!(matches!(
        result,
        ApplyResult::ReconcileRequired {
            pending_ref: Some(_),
            ..
        }
    ));
}

#[test]
fn consumed_journal_seq_cannot_be_reused_after_reconcile_required() {
    let mut store = StateStore::new();
    let mut applier = StateApplier::new(&mut store);

    let result = applier.apply(19, sample_out_of_order_user_trade()).unwrap();
    assert!(matches!(result, ApplyResult::ReconcileRequired { .. }));

    let other_event = ExternalFactEvent::new("market_ws", "session-2", "evt-3", "v1", Utc::now());
    let err = applier.apply(19, other_event).unwrap_err();
    assert_eq!(
        err.to_string(),
        "journal sequence 19 is already bound to a different fact"
    );
}

fn sample_out_of_order_user_trade() -> ExternalFactEvent {
    ExternalFactEvent::new(
        "user_trade_out_of_order",
        "session-2",
        "evt-2",
        "v1",
        Utc::now(),
    )
}
