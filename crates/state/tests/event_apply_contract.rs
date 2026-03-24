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

fn sample_out_of_order_user_trade() -> ExternalFactEvent {
    ExternalFactEvent::new(
        "user_trade_out_of_order",
        "session-2",
        "evt-2",
        "v1",
        Utc::now(),
    )
}
