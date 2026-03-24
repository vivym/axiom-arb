use domain::ExecutionAttempt;
use recovery::{RecoveryCoordinator, RecoveryIntent, RecoveryOutputs, RecoveryScopeLock};

#[test]
fn recovery_scope_lock_blocks_strategy_expansion_for_same_family() {
    let family_lock = RecoveryScopeLock::family("family-a");
    let same_family = RecoveryScopeLock::family("family-a");
    let same_id_other_variant = RecoveryScopeLock::market("family-a");
    let different_family = RecoveryScopeLock::family("family-b");

    assert!(family_lock.blocks_expansion(&same_family));
    assert!(!family_lock.blocks_expansion(&same_id_other_variant));
    assert!(!family_lock.blocks_expansion(&different_family));
}

#[test]
fn ambiguous_attempt_emits_recovery_intent_or_pending_reconcile() {
    let coordinator = RecoveryCoordinator::default();
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

fn sample_ambiguous_attempt() -> ExecutionAttempt {
    ExecutionAttempt::new(
        "attempt-1",
        "request-9:redeem-resolved:condition-12",
        "snapshot-1",
        1,
    )
}
