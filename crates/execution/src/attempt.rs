use std::collections::HashMap;

use domain::{ExecutionAttempt, ExecutionAttemptContext, ExecutionMode, ExecutionRequest};
use sha2::{Digest, Sha256};

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

    pub fn request_bound_attempt_id(
        plan: &ExecutionPlan,
        request: &ExecutionRequest,
        attempt_no: u32,
    ) -> String {
        let plan_id = Self::request_bound_plan_id(plan, request);
        Self::attempt_id_for_plan_id(&plan_id, attempt_no)
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

        let attempt_id = Self::attempt_id_for_plan_id(&plan_id, *next_attempt_no);
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

    fn attempt_id_for_plan_id(plan_id: &str, attempt_no: u32) -> String {
        format!(
            "request-bound:{}:attempt-{}",
            stable_request_bound_digest(plan_id),
            attempt_no
        )
    }
}

fn stable_request_bound_digest(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}
