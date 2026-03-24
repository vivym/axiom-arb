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
    if let Some(remainder) = plan_id.strip_prefix("request-bound:") {
        if let Some((request_len, trailing)) = remainder.split_once(':') {
            if let Ok(request_len) = request_len.parse::<usize>() {
                if trailing.len() > request_len && trailing.as_bytes()[request_len] == b':' {
                    return &trailing[request_len + 1..];
                }
            }
        }
    }

    plan_id
}
