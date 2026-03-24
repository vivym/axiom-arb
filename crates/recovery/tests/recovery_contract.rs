use domain::ExecutionAttempt;
use recovery::{RecoveryCoordinator, RecoveryScopeLock};

#[test]
fn recovery_scope_lock_blocks_strategy_expansion_for_same_family() {
    let lock = RecoveryScopeLock::family("family-a");
    assert!(lock.blocks_expansion("family-a"));
    assert!(!lock.blocks_expansion("family-b"));
}

#[test]
fn ambiguous_attempt_emits_recovery_intent_or_pending_reconcile() {
    let coordinator = RecoveryCoordinator::default();
    let outputs = coordinator.on_failed_ambiguous(sample_ambiguous_attempt());

    assert!(outputs.recovery_intent.is_some() || outputs.pending_reconcile.is_some());
}

fn sample_ambiguous_attempt() -> ExecutionAttempt {
    ExecutionAttempt::new("attempt-1", "plan-1", "snapshot-1", 1)
}
