use std::collections::HashMap;

use domain::{ExecutionAttempt, ExecutionAttemptContext, ExecutionMode, ExecutionRequest};

use crate::plans::ExecutionPlan;

#[derive(Debug, Default)]
pub struct ExecutionAttemptFactory {
    next_attempt_no_by_plan: HashMap<String, u32>,
}

impl ExecutionAttemptFactory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_seeded_attempt_numbers(next_attempt_no_by_plan: HashMap<String, u32>) -> Self {
        Self {
            next_attempt_no_by_plan,
        }
    }

    pub fn request_bound_plan_id(plan: &ExecutionPlan, request: &ExecutionRequest) -> String {
        format!(
            "request-bound:{}:{}:{}",
            request.request_id.len(),
            request.request_id,
            plan.plan_id()
        )
    }

    pub fn next_for_plan(
        &mut self,
        plan: &ExecutionPlan,
        request: &ExecutionRequest,
        execution_mode: ExecutionMode,
    ) -> (ExecutionAttempt, ExecutionAttemptContext) {
        let plan_id = Self::request_bound_plan_id(plan, request);
        let next_attempt_no = self
            .next_attempt_no_by_plan
            .entry(plan_id.clone())
            .or_insert(0);
        *next_attempt_no += 1;

        let attempt_id = format!("{}:attempt-{}", plan_id, *next_attempt_no);
        let attempt = ExecutionAttempt::new(
            attempt_id.clone(),
            plan_id,
            request.snapshot_id.clone(),
            *next_attempt_no,
        );
        let context = ExecutionAttemptContext {
            attempt_id,
            snapshot_id: request.snapshot_id.clone(),
            execution_mode,
            route: request.route.clone(),
            scope: request.scope.clone(),
            matched_rule_id: request.matched_rule_id.clone(),
        };

        (attempt, context)
    }
}
