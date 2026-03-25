use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use domain::{ConditionId, EventFamilyId, TokenId};
use domain::{ExecutionAttemptContext, ExecutionMode};
use execution::plans::{ExecutionPlan, NegRiskMemberOrderPlan};
use execution::sink::{LiveVenueSink, SignedFamilyHook, SignedFamilyHookError, VenueSink};
use execution::signing::{OrderSigner, TestOrderSigner};
use rust_decimal::Decimal;
use std::collections::HashMap;

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
fn deterministic_test_signer_canonicalizes_member_ordering_for_equivalent_family_plans() {
    let plan_a = sample_family_plan();
    let mut plan_b = sample_family_plan();
    if let ExecutionPlan::NegRiskSubmitFamily { members, .. } = &mut plan_b {
        members.reverse();
    }

    let signed_a = TestOrderSigner::default().sign_family(&plan_a).unwrap();
    let signed_b = TestOrderSigner::default().sign_family(&plan_b).unwrap();

    assert_eq!(signed_a.plan_id, signed_b.plan_id);
    assert_eq!(signed_a.members, signed_b.members);

    fn to_map(
        signed: &execution::signing::SignedFamilySubmission,
    ) -> HashMap<String, domain::SignedOrderIdentity> {
        signed
            .members
            .iter()
            .map(|member| {
                let key = format!(
                    "{}:{}:{}:{}",
                    member.condition_id.as_str(),
                    member.token_id.as_str(),
                    member.price.normalize(),
                    member.quantity.normalize()
                );
                (key, member.identity.clone())
            })
            .collect()
    }

    assert_eq!(to_map(&signed_a), to_map(&signed_b));
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
fn live_sink_rejects_negrisk_family_submit_plans_when_signer_is_missing_in_live_mode() {
    let sink = LiveVenueSink::noop();
    let err = sink
        .execute(&sample_family_plan(), &live_attempt())
        .unwrap_err();
    assert!(matches!(err, execution::VenueSinkError::Rejected { .. }));
}

#[test]
fn live_sink_rejects_negrisk_family_submit_plans_when_signer_is_missing_in_recovery_only_mode() {
    let sink = LiveVenueSink::noop();
    let err = sink
        .execute(&sample_family_plan(), &recovery_attempt())
        .unwrap_err();
    assert!(matches!(err, execution::VenueSinkError::Rejected { .. }));
}

#[test]
fn live_sink_forwards_signed_family_submission_to_hook_for_negrisk_family_submits() {
    let called = Arc::new(AtomicUsize::new(0));
    let hook = Arc::new(SpySignedFamilyHook {
        called: called.clone(),
        last_plan_id: Arc::new(std::sync::Mutex::new(None)),
        last_member_count: Arc::new(std::sync::Mutex::new(None)),
    });
    let signer = Arc::new(TestOrderSigner::default());
    let sink = LiveVenueSink::with_order_signer_and_hook(signer, hook.clone());

    let plan = sample_family_plan();
    let plan_id = plan.plan_id();
    let receipt = sink.execute(&plan, &live_attempt()).unwrap();
    assert_eq!(receipt.outcome, domain::ExecutionAttemptOutcome::Succeeded);
    assert_eq!(called.load(Ordering::SeqCst), 1);
    assert_eq!(
        hook.last_plan_id.lock().unwrap().clone(),
        Some(plan_id)
    );
    assert_eq!(*hook.last_member_count.lock().unwrap(), Some(2));
}

#[test]
fn live_sink_propagates_hook_errors_for_negrisk_family_submits() {
    let hook_called = Arc::new(AtomicUsize::new(0));
    let sign_called = Arc::new(AtomicUsize::new(0));

    let hook = Arc::new(RejectingSignedFamilyHook {
        called: hook_called.clone(),
    });
    let signer = Arc::new(SpySigner {
        inner: TestOrderSigner::default(),
        called: sign_called.clone(),
    });
    let sink = LiveVenueSink::with_order_signer_and_hook(signer, hook);

    let err = sink
        .execute(&sample_family_plan(), &live_attempt())
        .unwrap_err();

    assert!(matches!(err, execution::VenueSinkError::Rejected { .. }));
    assert_eq!(sign_called.load(Ordering::SeqCst), 1);
    assert_eq!(hook_called.load(Ordering::SeqCst), 1);
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
fn live_sink_does_not_forward_signed_family_submission_for_non_negrisk_plans() {
    let called = Arc::new(AtomicUsize::new(0));
    let hook = Arc::new(SpySignedFamilyHook {
        called: called.clone(),
        last_plan_id: Arc::new(std::sync::Mutex::new(None)),
        last_member_count: Arc::new(std::sync::Mutex::new(None)),
    });
    let signer = Arc::new(RejectingSigner {
        called: Arc::new(AtomicUsize::new(0)),
    });
    let sink = LiveVenueSink::with_order_signer_and_hook(signer, hook);

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

fn recovery_attempt() -> ExecutionAttemptContext {
    ExecutionAttemptContext {
        attempt_id: "attempt-1".to_string(),
        snapshot_id: "snapshot-1".to_string(),
        execution_mode: ExecutionMode::RecoveryOnly,
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

#[derive(Debug)]
struct SpySignedFamilyHook {
    called: Arc<AtomicUsize>,
    last_plan_id: Arc<std::sync::Mutex<Option<String>>>,
    last_member_count: Arc<std::sync::Mutex<Option<usize>>>,
}

impl SignedFamilyHook for SpySignedFamilyHook {
    fn on_signed_family(
        &self,
        signed: &execution::signing::SignedFamilySubmission,
        _attempt: &ExecutionAttemptContext,
    ) -> Result<(), SignedFamilyHookError> {
        self.called.fetch_add(1, Ordering::SeqCst);
        *self.last_plan_id.lock().unwrap() = Some(signed.plan_id.clone());
        *self.last_member_count.lock().unwrap() = Some(signed.members.len());
        Ok(())
    }
}

#[derive(Debug)]
struct RejectingSignedFamilyHook {
    called: Arc<AtomicUsize>,
}

impl SignedFamilyHook for RejectingSignedFamilyHook {
    fn on_signed_family(
        &self,
        _signed: &execution::signing::SignedFamilySubmission,
        _attempt: &ExecutionAttemptContext,
    ) -> Result<(), SignedFamilyHookError> {
        self.called.fetch_add(1, Ordering::SeqCst);
        Err(SignedFamilyHookError {
            reason: "hook failure".to_owned(),
        })
    }
}
