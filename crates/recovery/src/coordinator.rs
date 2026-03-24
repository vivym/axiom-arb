use domain::{ExecutionAttempt, RecoveryIntent};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecoveryOutputs {
    pub recovery_intent: Option<RecoveryIntent>,
    pub pending_reconcile: Option<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RecoveryCoordinator;

impl RecoveryCoordinator {
    pub fn on_failed_ambiguous(&self, attempt: ExecutionAttempt) -> RecoveryOutputs {
        RecoveryOutputs {
            recovery_intent: Some(RecoveryIntent::new(
                format!("recovery-{}", attempt.attempt_id),
                attempt.snapshot_id,
                format!("execution_path:{}", attempt.plan_id),
            )),
            pending_reconcile: None,
        }
    }
}
