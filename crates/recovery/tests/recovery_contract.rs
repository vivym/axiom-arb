use domain::ExecutionAttempt;
use recovery::{
    PendingReconcilePayload, RecoveryCoordinator, RecoveryIntent, RecoveryOutputs,
    RecoveryScopeLock,
};

#[test]
fn recovery_scope_lock_blocks_nested_child_scopes_without_cross_variant_aliasing() {
    let family_lock = RecoveryScopeLock::family("family-a");
    let nested_family = RecoveryScopeLock::family("family-a:condition-12");
    let nested_condition = RecoveryScopeLock::condition("family-a:condition-12");
    let nested_market = RecoveryScopeLock::market("family-a:condition-12:market-1");
    let nested_path = RecoveryScopeLock::execution_path("family-a:condition-12:market-1:path-1");
    let same_id_other_variant = RecoveryScopeLock::market("family-a");
    let different_family = RecoveryScopeLock::family("family-b");

    assert!(family_lock.blocks_expansion(&nested_family));
    assert!(family_lock.blocks_expansion(&nested_condition));
    assert!(family_lock.blocks_expansion(&nested_market));
    assert!(family_lock.blocks_expansion(&nested_path));
    assert!(!family_lock.blocks_expansion(&same_id_other_variant));
    assert!(!family_lock.blocks_expansion(&different_family));
}

#[test]
fn ambiguous_attempt_emits_recovery_intent_or_pending_reconcile() {
    let coordinator = RecoveryCoordinator;
    let outputs = coordinator.on_failed_ambiguous(sample_ambiguous_attempt());

    assert_eq!(
        outputs,
        RecoveryOutputs {
            recovery_intent: Some(RecoveryIntent::new(
                "recovery-attempt-1",
                "snapshot-1",
                "execution_path:redeem-resolved:condition-12",
            )),
            pending_reconcile: None,
        }
    );
}

#[test]
fn stable_business_plan_id_is_preserved_in_recovery_scope() {
    let coordinator = RecoveryCoordinator;
    let outputs = coordinator.on_failed_ambiguous(sample_stable_attempt());

    assert_eq!(
        outputs,
        RecoveryOutputs {
            recovery_intent: Some(RecoveryIntent::new(
                "recovery-attempt-2",
                "snapshot-2",
                "execution_path:redeem-resolved:condition-12",
            )),
            pending_reconcile: None,
        }
    );
}

#[test]
fn pending_reconcile_payload_carries_persistence_fields() {
    let payload = PendingReconcilePayload {
        submission_ref: "submission-1".to_owned(),
        family_id: "family-1".to_owned(),
        route: "execution_path:redeem-resolved:condition-12".to_owned(),
        reason: "ambiguous_attempt".to_owned(),
    };

    assert_eq!(payload.submission_ref, "submission-1");
    assert_eq!(payload.family_id, "family-1");
    assert_eq!(payload.route, "execution_path:redeem-resolved:condition-12");
    assert_eq!(payload.reason, "ambiguous_attempt");
}

fn sample_ambiguous_attempt() -> ExecutionAttempt {
    ExecutionAttempt::new(
        "attempt-1",
        "request-bound:9:request-9:redeem-resolved:condition-12",
        "snapshot-1",
        1,
    )
}

fn sample_stable_attempt() -> ExecutionAttempt {
    ExecutionAttempt::new("attempt-2", "redeem-resolved:condition-12", "snapshot-2", 2)
}
