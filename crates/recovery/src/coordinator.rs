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
    if is_stable_business_plan_id(plan_id) {
        return plan_id;
    }

    if let Some((request_prefix, stable_scope)) = plan_id.split_once(':') {
        if is_request_bound_plan_prefix(request_prefix) && is_stable_business_plan_id(stable_scope) {
            return stable_scope;
        }
    }

    plan_id
}

fn is_request_bound_plan_prefix(prefix: &str) -> bool {
    prefix.starts_with("request-")
}

fn is_stable_business_plan_id(plan_id: &str) -> bool {
    plan_id.starts_with("fullset-buy-merge:")
        || plan_id.starts_with("fullset-split-sell:")
        || plan_id.starts_with("cancel-stale:")
        || plan_id.starts_with("redeem-resolved:")
}
