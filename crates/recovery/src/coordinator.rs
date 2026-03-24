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
        let stable_plan_scope = stable_plan_scope(&attempt.plan_id);
        RecoveryOutputs {
            recovery_intent: Some(RecoveryIntent::new(
                format!("recovery-{}", attempt.attempt_id),
                attempt.snapshot_id,
                format!("execution_path:{}", stable_plan_scope),
            )),
            pending_reconcile: None,
        }
    }
}

fn stable_plan_scope(plan_id: &str) -> &str {
    plan_id
        .split_once(':')
        .map(|(_, scope)| scope)
        .unwrap_or(plan_id)
}
