use chrono::Utc;
use domain::ExternalFactEvent;
use state::{ApplyResult, StateApplier, StateConfidence, StateFactInput, StateStore};

#[test]
fn duplicate_fact_returns_duplicate_anchor_without_mutating_state_version() {
    let mut store = StateStore::new();
    let mut applier = StateApplier::new(&mut store);
    let event = ExternalFactEvent::new("market_ws", "session-1", "evt-1", "v1", Utc::now());

    let first = applier.apply(17, event.clone()).unwrap();
    let duplicate = applier.apply(18, event).unwrap();

    assert!(matches!(
        first,
        ApplyResult::Applied {
            journal_seq: 17,
            state_version: 1,
            ..
        }
    ));
    assert!(matches!(
        duplicate,
        ApplyResult::Duplicate {
            journal_seq: 18,
            duplicate_of_journal_seq: 17,
            state_version: 1,
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
fn decreasing_journal_seq_is_rejected() {
    let mut store = StateStore::new();
    let mut applier = StateApplier::new(&mut store);
    let first = ExternalFactEvent::new("market_ws", "session-1", "evt-1", "v1", Utc::now());
    let second = ExternalFactEvent::new("market_ws", "session-1", "evt-2", "v1", Utc::now());

    applier.apply(17, first).unwrap();
    let err = applier.apply(16, second).unwrap_err();

    assert_eq!(
        err.to_string(),
        "journal sequence 16 must be greater than last consumed sequence 17"
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
            journal_seq: 19,
            pending_ref: Some(_),
            ..
        }
    ));
}

#[test]
fn reconcile_required_does_not_advance_state_version() {
    let mut store = StateStore::new();
    let mut applier = StateApplier::new(&mut store);

    let applied = applier
        .apply(
            17,
            ExternalFactEvent::new("market_ws", "session-1", "evt-1", "v1", Utc::now()),
        )
        .unwrap();
    let reconcile_required = applier.apply(19, sample_out_of_order_user_trade()).unwrap();

    assert!(matches!(
        applied,
        ApplyResult::Applied {
            state_version: 1,
            ..
        }
    ));
    assert!(matches!(
        reconcile_required,
        ApplyResult::ReconcileRequired {
            journal_seq: 19,
            pending_ref: Some(_),
            ..
        }
    ));
    assert_eq!(store.state_version(), 1);
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

#[test]
fn live_submit_fact_enters_reconcile_first_posture_without_advancing_state_version() {
    let mut store = StateStore::new();
    let mut applier = StateApplier::new(&mut store);

    let result = applier
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

    assert!(matches!(
        result,
        ApplyResult::ReconcileRequired {
            journal_seq: 19,
            pending_ref: Some(_),
            ..
        }
    ));
    assert_eq!(store.state_version(), 0);
    assert_eq!(store.pending_reconcile_count(), 1);
    assert_eq!(store.mode(), domain::RuntimeMode::Reconciling);
    assert_eq!(store.first_reconcile_succeeded(), true);
    assert_eq!(
        store.scope_confidence("family-a"),
        StateConfidence::Uncertain
    );
    let anchors = store.pending_reconcile_anchors();
    assert_eq!(anchors.len(), 1);
    assert_eq!(anchors[0].submission_ref, "submission-family-a-1");
    assert_eq!(anchors[0].family_id, "family-a");
    assert_eq!(anchors[0].route, "negrisk_live_submit");
    assert_eq!(anchors[0].reason, "live submit observed");
    assert!(!store.allows_automatic_repair());
}

#[test]
fn terminal_live_reconcile_clears_pending_anchor_and_restores_healthy_posture() {
    let mut store = StateStore::new();
    let mut applier = StateApplier::new(&mut store);

    applier
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

    drop(applier);
    let pending_ref = store.pending_reconcile_anchors()[0].pending_ref.clone();
    let mut applier = StateApplier::new(&mut store);
    let result = applier
        .apply(
            20,
            ExternalFactEvent::negrisk_live_reconcile_observed(
                "session-live",
                "evt-2",
                pending_ref,
                "family-a",
                true,
                Utc::now(),
            ),
        )
        .unwrap();

    assert!(matches!(
        result,
        ApplyResult::Applied {
            journal_seq: 20,
            state_version: 1,
            ..
        }
    ));
    assert_eq!(store.pending_reconcile_count(), 0);
    assert_eq!(store.mode(), domain::RuntimeMode::Healthy);
    assert_eq!(store.scope_confidence("family-a"), StateConfidence::Certain);
    assert!(store.allows_automatic_repair());
}

#[test]
fn state_confidence_reports_scope_uncertainty_from_live_submit_anchor() {
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

    assert_eq!(store.state_confidence("family-a"), StateConfidence::Uncertain);
    assert_eq!(store.state_confidence("family-b"), StateConfidence::Certain);
}

fn sample_out_of_order_user_trade() -> StateFactInput {
    StateFactInput::out_of_order_user_trade(ExternalFactEvent::new(
        "market_ws",
        "session-2",
        "evt-2",
        "v1",
        Utc::now(),
    ))
}
