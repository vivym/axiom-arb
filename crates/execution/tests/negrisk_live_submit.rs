use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use domain::{
    ConditionId, EventFamilyId, ExecutionAttemptContext, ExecutionAttemptOutcome, ExecutionMode,
    SignedOrderIdentity, TokenId,
};
use execution::providers::{
    ReconcileProvider, SignerProvider, SubmitProviderError, VenueExecutionProvider,
};
use execution::signing::SignedFamilyMember;
use execution::sink::{LiveVenueSink, VenueSink};
use execution::{
    plans::{ExecutionPlan, NegRiskMemberOrderPlan},
    LiveSubmissionRecord, LiveSubmitOutcome, PendingReconcileWork, ReconcileOutcome,
    ReconcileProviderError, SignedFamilySubmission, TestOrderSigner,
};
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

    let outcome = provider.submit_family(&signed, &attempt).unwrap();

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

impl VenueExecutionProvider for InspectingSubmitProvider {
    fn submit_family(
        &self,
        signed: &SignedFamilySubmission,
        attempt: &ExecutionAttemptContext,
    ) -> Result<LiveSubmitOutcome, SubmitProviderError> {
        assert_eq!(attempt.attempt_id, "attempt-1");
        assert_eq!(signed.plan_id, "plan-1");

        Ok(LiveSubmitOutcome::Accepted {
            submission_record: LiveSubmissionRecord {
                submission_ref: "submission-1".to_owned(),
                attempt_id: attempt.attempt_id.clone(),
                route: attempt.route.clone(),
                scope: attempt.scope.clone(),
                provider: "inspecting-submit".to_owned(),
            },
        })
    }
}

#[test]
fn live_sink_anchors_submission_refs_from_submit_provider_acceptance() {
    let signer: Arc<dyn SignerProvider> = Arc::new(TestOrderSigner);
    let provider = Arc::new(RecordingSubmitProvider::accepted("submission-2"));
    let sink = LiveVenueSink::with_submit_provider(signer, provider.clone());

    let receipt = sink
        .execute(&sample_family_plan(), &live_attempt())
        .unwrap();

    assert_eq!(receipt.outcome, ExecutionAttemptOutcome::Succeeded);
    assert_eq!(receipt.submission_ref.as_deref(), Some("submission-2"));
    assert_eq!(receipt.pending_ref, None);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        provider.last_attempt_id.lock().unwrap().as_deref(),
        Some("attempt-1")
    );
}

#[test]
fn live_sink_anchors_pending_refs_from_ambiguous_submit_provider_outcomes() {
    let signer: Arc<dyn SignerProvider> = Arc::new(TestOrderSigner);
    let provider = Arc::new(RecordingSubmitProvider::ambiguous("pending-2"));
    let sink = LiveVenueSink::with_submit_provider(signer, provider);

    let receipt = sink
        .execute(&sample_family_plan(), &live_attempt())
        .unwrap();

    assert_eq!(receipt.outcome, ExecutionAttemptOutcome::FailedAmbiguous);
    assert_eq!(receipt.submission_ref, None);
    assert_eq!(receipt.pending_ref.as_deref(), Some("pending-2"));
}

#[test]
fn live_sink_preserves_reconcile_anchor_for_unconfirmed_acceptance() {
    let signer: Arc<dyn SignerProvider> = Arc::new(TestOrderSigner);
    let provider = Arc::new(RecordingSubmitProvider::accepted_but_unconfirmed(
        "submission-3",
        "pending-reconcile-3",
    ));
    let sink = LiveVenueSink::with_submit_provider(signer, provider);

    let receipt = sink
        .execute(&sample_family_plan(), &live_attempt())
        .unwrap();

    assert_eq!(receipt.outcome, ExecutionAttemptOutcome::Succeeded);
    assert_eq!(receipt.submission_ref.as_deref(), Some("submission-3"));
    assert_eq!(
        receipt.pending_ref.as_deref(),
        Some("pending-reconcile-3")
    );
}

#[test]
fn reconcile_provider_can_return_provider_errors_separately_from_domain_outcomes() {
    let provider: Arc<dyn ReconcileProvider> = Arc::new(FailingReconcileProvider);
    let work = PendingReconcileWork {
        pending_ref: "pending-3".to_owned(),
        route: "neg-risk".to_owned(),
        scope: "family-a".to_owned(),
    };

    let err = provider.reconcile_live(&work).unwrap_err();

    assert_eq!(
        err,
        ReconcileProviderError::new("reconcile provider unavailable")
    );
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

fn sample_family_plan() -> ExecutionPlan {
    ExecutionPlan::NegRiskSubmitFamily {
        family_id: EventFamilyId::from("family-a"),
        members: vec![
            NegRiskMemberOrderPlan {
                condition_id: ConditionId::from("condition-1"),
                token_id: TokenId::from("token-1"),
                price: Decimal::new(45, 2),
                quantity: Decimal::new(10, 0),
            },
            NegRiskMemberOrderPlan {
                condition_id: ConditionId::from("condition-2"),
                token_id: TokenId::from("token-2"),
                price: Decimal::new(55, 2),
                quantity: Decimal::new(8, 0),
            },
        ],
    }
}

fn live_attempt() -> ExecutionAttemptContext {
    ExecutionAttemptContext {
        attempt_id: "attempt-1".to_owned(),
        snapshot_id: "snapshot-1".to_owned(),
        execution_mode: ExecutionMode::Live,
        route: "neg-risk".to_owned(),
        scope: "family-a".to_owned(),
        matched_rule_id: None,
    }
}

#[derive(Debug, Clone)]
struct RecordingSubmitProvider {
    calls: Arc<AtomicUsize>,
    last_attempt_id: Arc<Mutex<Option<String>>>,
    outcome: LiveSubmitOutcome,
}

impl RecordingSubmitProvider {
    fn accepted(submission_ref: &str) -> Self {
        Self {
            calls: Arc::new(AtomicUsize::new(0)),
            last_attempt_id: Arc::new(Mutex::new(None)),
            outcome: LiveSubmitOutcome::Accepted {
                submission_record: LiveSubmissionRecord {
                    submission_ref: submission_ref.to_owned(),
                    attempt_id: "attempt-1".to_owned(),
                    route: "neg-risk".to_owned(),
                    scope: "family-a".to_owned(),
                    provider: "recording-submit".to_owned(),
                },
            },
        }
    }

    fn ambiguous(pending_ref: &str) -> Self {
        Self {
            calls: Arc::new(AtomicUsize::new(0)),
            last_attempt_id: Arc::new(Mutex::new(None)),
            outcome: LiveSubmitOutcome::Ambiguous {
                pending_ref: pending_ref.to_owned(),
                reason: "timeout".to_owned(),
            },
        }
    }

    fn accepted_but_unconfirmed(submission_ref: &str, pending_ref: &str) -> Self {
        Self {
            calls: Arc::new(AtomicUsize::new(0)),
            last_attempt_id: Arc::new(Mutex::new(None)),
            outcome: LiveSubmitOutcome::AcceptedButUnconfirmed {
                submission_record: LiveSubmissionRecord {
                    submission_ref: submission_ref.to_owned(),
                    attempt_id: "attempt-1".to_owned(),
                    route: "neg-risk".to_owned(),
                    scope: "family-a".to_owned(),
                    provider: "recording-submit".to_owned(),
                },
                pending_ref: pending_ref.to_owned(),
            },
        }
    }
}

impl VenueExecutionProvider for RecordingSubmitProvider {
    fn submit_family(
        &self,
        signed: &SignedFamilySubmission,
        attempt: &ExecutionAttemptContext,
    ) -> Result<LiveSubmitOutcome, SubmitProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.last_attempt_id.lock().unwrap() = Some(attempt.attempt_id.clone());
        assert_eq!(signed.plan_id, sample_family_plan().plan_id());
        Ok(self.outcome.clone())
    }
}

struct FailingReconcileProvider;

impl ReconcileProvider for FailingReconcileProvider {
    fn reconcile_live(
        &self,
        work: &PendingReconcileWork,
    ) -> Result<ReconcileOutcome, ReconcileProviderError> {
        assert_eq!(work.pending_ref, "pending-3");
        assert_eq!(work.route, "neg-risk");
        assert_eq!(work.scope, "family-a");

        Err(ReconcileProviderError::new(
            "reconcile provider unavailable",
        ))
    }
}
