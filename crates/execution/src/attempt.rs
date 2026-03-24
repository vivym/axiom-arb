use domain::{ExecutionAttempt, ExecutionAttemptContext, ExecutionMode};

use crate::plans::ExecutionPlan;

#[derive(Debug, Default)]
pub struct ExecutionAttemptFactory {
    next_attempt_no: u32,
}

impl ExecutionAttemptFactory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn next_for_plan(
        &mut self,
        plan: &ExecutionPlan,
        snapshot_id: &str,
        execution_mode: ExecutionMode,
    ) -> (ExecutionAttempt, ExecutionAttemptContext) {
        let attempt_no = self.next_attempt_no + 1;
        self.next_attempt_no = attempt_no;

        let attempt_id = format!("{}:attempt-{}", plan.plan_id(), attempt_no);
        let attempt = ExecutionAttempt::new(
            attempt_id.clone(),
            plan.plan_id(),
            snapshot_id.to_owned(),
            attempt_no,
        );
        let context = ExecutionAttemptContext {
            attempt_id,
            snapshot_id: snapshot_id.to_owned(),
            execution_mode,
        };

        (attempt, context)
    }
}
