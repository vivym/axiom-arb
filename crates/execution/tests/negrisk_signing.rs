use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use domain::{ConditionId, EventFamilyId, TokenId};
use domain::{ExecutionAttemptContext, ExecutionMode};
use execution::plans::{ExecutionPlan, NegRiskMemberOrderPlan};
use execution::sink::{LiveVenueSink, VenueSink};
use execution::signing::{OrderSigner, TestOrderSigner};
use rust_decimal::Decimal;

#[test]
fn deterministic_test_signer_attaches_signed_identity_to_each_planned_member_order() {
    let signed = TestOrderSigner::default()
        .sign_family(&sample_family_plan())
        .unwrap();
    assert_eq!(signed.members.len(), 2);
    assert!(signed
        .members
        .iter()
        .all(|member| member.identity.signature.starts_with("test-sig:")));
}

#[test]
fn live_sink_signs_negrisk_family_submit_plans_when_signer_is_configured() {
    let called = Arc::new(AtomicUsize::new(0));
    let signer = Arc::new(SpySigner {
        inner: TestOrderSigner::default(),
        called: called.clone(),
    });
    let sink = LiveVenueSink::with_order_signer(signer);

    let receipt = sink.execute(&sample_family_plan(), &live_attempt()).unwrap();
    assert_eq!(receipt.outcome, domain::ExecutionAttemptOutcome::Succeeded);
    assert_eq!(called.load(Ordering::SeqCst), 1);
}

#[test]
fn live_sink_does_not_apply_signer_to_non_negrisk_plans() {
    let called = Arc::new(AtomicUsize::new(0));
    let sink = LiveVenueSink::with_order_signer(Arc::new(RejectingSigner {
        called: called.clone(),
    }));

    let receipt = sink
        .execute(
            &ExecutionPlan::RedeemResolved {
                condition_id: ConditionId::from("condition-x"),
            },
            &live_attempt(),
        )
        .unwrap();

    assert_eq!(receipt.outcome, domain::ExecutionAttemptOutcome::Succeeded);
    assert_eq!(called.load(Ordering::SeqCst), 0);
}

#[test]
fn live_sink_propagates_signer_error_for_negrisk_family_submit_plans() {
    let called = Arc::new(AtomicUsize::new(0));
    let sink = LiveVenueSink::with_order_signer(Arc::new(RejectingSigner {
        called: called.clone(),
    }));

    let err = sink
        .execute(&sample_family_plan(), &live_attempt())
        .unwrap_err();

    assert!(matches!(err, execution::VenueSinkError::Rejected { .. }));
    assert_eq!(called.load(Ordering::SeqCst), 1);
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
        attempt_id: "attempt-1".to_string(),
        snapshot_id: "snapshot-1".to_string(),
        execution_mode: ExecutionMode::Live,
        route: "route".to_string(),
        scope: "scope".to_string(),
        matched_rule_id: None,
    }
}

#[derive(Debug)]
struct SpySigner {
    inner: TestOrderSigner,
    called: Arc<AtomicUsize>,
}

impl OrderSigner for SpySigner {
    fn sign_family(
        &self,
        plan: &ExecutionPlan,
    ) -> Result<execution::signing::SignedFamilySubmission, execution::signing::SigningError> {
        self.called.fetch_add(1, Ordering::SeqCst);
        self.inner.sign_family(plan)
    }
}

#[derive(Debug)]
struct RejectingSigner {
    called: Arc<AtomicUsize>,
}

impl OrderSigner for RejectingSigner {
    fn sign_family(
        &self,
        plan: &ExecutionPlan,
    ) -> Result<execution::signing::SignedFamilySubmission, execution::signing::SigningError> {
        self.called.fetch_add(1, Ordering::SeqCst);
        Err(execution::signing::SigningError::UnsupportedPlan {
            plan_id: plan.plan_id(),
        })
    }
}
