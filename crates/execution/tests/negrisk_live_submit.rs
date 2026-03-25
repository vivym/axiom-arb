use execution::{LiveSubmissionRecord, LiveSubmitOutcome, PendingReconcileWork, ReconcileOutcome};

#[test]
fn live_submit_outcome_distinguishes_accepted_and_ambiguous() {
    let accepted = LiveSubmitOutcome::Accepted {
        submission_record: sample_submission_record("attempt-1"),
    };
    let ambiguous = LiveSubmitOutcome::Ambiguous {
        pending_ref: "pending-attempt-1".to_owned(),
        reason: "timeout".to_owned(),
    };

    assert!(accepted.is_accepted());
    assert!(ambiguous.is_ambiguous());
}

#[test]
fn reconcile_outcome_exposes_terminal_and_pending_states() {
    let confirmed = ReconcileOutcome::ConfirmedAuthoritative {
        submission_ref: "submission-1".to_owned(),
    };
    let pending = ReconcileOutcome::StillPending;
    let recovery = ReconcileOutcome::NeedsRecovery {
        pending_ref: "pending-1".to_owned(),
        reason: "timeout".to_owned(),
    };

    assert!(confirmed.is_confirmed());
    assert!(pending.is_pending());
    assert!(recovery.needs_recovery());
}

#[test]
fn pending_reconcile_work_captures_route_and_scope() {
    let work = PendingReconcileWork {
        pending_ref: "pending-1".to_owned(),
        route: "neg-risk".to_owned(),
        scope: "family-a".to_owned(),
    };

    assert_eq!(work.pending_ref, "pending-1");
    assert_eq!(work.route, "neg-risk");
    assert_eq!(work.scope, "family-a");
}

fn sample_submission_record(attempt_id: &str) -> LiveSubmissionRecord {
    LiveSubmissionRecord {
        submission_ref: "submission-1".to_owned(),
        attempt_id: attempt_id.to_owned(),
        route: "neg-risk".to_owned(),
        scope: "family-a".to_owned(),
        provider: "negrisk-live".to_owned(),
    }
}
