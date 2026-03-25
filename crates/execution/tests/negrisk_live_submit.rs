use execution::{
    LiveSubmissionRecord, LiveSubmitOutcome, PendingReconcileWork, ReconcileOutcome,
    SignedFamilySubmission,
};
use execution::providers::{LiveSubmitProvider, LiveSubmitRequest};
use execution::signing::SignedFamilyMember;
use domain::{ExecutionAttemptContext, ExecutionMode, SignedOrderIdentity};
use rust_decimal::Decimal;

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

#[test]
fn live_submit_provider_receives_attempt_and_signed_submission_context() {
    let provider = InspectingSubmitProvider;
    let attempt = ExecutionAttemptContext {
        attempt_id: "attempt-1".to_owned(),
        snapshot_id: "snapshot-1".to_owned(),
        execution_mode: ExecutionMode::Live,
        route: "neg-risk".to_owned(),
        scope: "family-a".to_owned(),
        matched_rule_id: Some("rule-1".to_owned()),
    };
    let signed = sample_signed_submission("plan-1");

    let request = LiveSubmitRequest {
        attempt: &attempt,
        signed_submission: &signed,
    };
    let outcome = provider.submit_live(&request);

    assert_eq!(
        outcome,
        LiveSubmitOutcome::Accepted {
            submission_record: LiveSubmissionRecord {
                submission_ref: "submission-1".to_owned(),
                attempt_id: "attempt-1".to_owned(),
                route: "neg-risk".to_owned(),
                scope: "family-a".to_owned(),
                provider: "inspecting-submit".to_owned(),
            },
        }
    );
}

struct InspectingSubmitProvider;

impl LiveSubmitProvider for InspectingSubmitProvider {
    fn submit_live(
        &self,
        request: &LiveSubmitRequest<'_>,
    ) -> LiveSubmitOutcome {
        assert_eq!(request.attempt.attempt_id, "attempt-1");
        assert_eq!(request.signed_submission.plan_id, "plan-1");

        LiveSubmitOutcome::Accepted {
            submission_record: LiveSubmissionRecord {
                submission_ref: "submission-1".to_owned(),
                attempt_id: request.attempt.attempt_id.clone(),
                route: request.attempt.route.clone(),
                scope: request.attempt.scope.clone(),
                provider: "inspecting-submit".to_owned(),
            },
        }
    }
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

fn sample_signed_submission(plan_id: &str) -> SignedFamilySubmission {
    SignedFamilySubmission {
        plan_id: plan_id.to_owned(),
        members: vec![SignedFamilyMember {
            condition_id: domain::ConditionId::from("condition-1"),
            token_id: domain::TokenId::from("token-1"),
            price: Decimal::new(1, 0),
            quantity: Decimal::new(2, 0),
            maker: "0xmaker".to_owned(),
            signer: "0xsigner".to_owned(),
            taker: "0x0000000000000000000000000000000000000000".to_owned(),
            maker_amount: "2".to_owned(),
            taker_amount: "2".to_owned(),
            side: "BUY".to_owned(),
            expiration: "0".to_owned(),
            fee_rate_bps: "30".to_owned(),
            signature_type: 0,
            identity: SignedOrderIdentity {
                signed_order_hash: "hash-1".to_owned(),
                salt: "123".to_owned(),
                nonce: "0".to_owned(),
                signature: "sig-1".to_owned(),
            },
        }],
    }
}
