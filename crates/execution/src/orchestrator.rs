use std::cell::RefCell;

use domain::{ConditionId, ExecutionMode, ExecutionRequest, OrderId};

use crate::{
    attempt::ExecutionAttemptFactory,
    plans::ExecutionPlan,
    sink::VenueSink,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionPlanningRequest {
    pub request: ExecutionRequest,
    pub execution_mode: ExecutionMode,
}

impl ExecutionPlanningRequest {
    pub fn new(request: ExecutionRequest, execution_mode: ExecutionMode) -> Self {
        Self {
            request,
            execution_mode,
        }
    }
}

pub trait ExecutionPlanInput {
    fn request(&self) -> &ExecutionRequest;

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Live
    }
}

impl ExecutionPlanInput for ExecutionRequest {
    fn request(&self) -> &ExecutionRequest {
        self
    }
}

impl ExecutionPlanInput for ExecutionPlanningRequest {
    fn request(&self) -> &ExecutionRequest {
        &self.request
    }

    fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionError {
    ModeViolation {
        execution_mode: ExecutionMode,
        plan: ExecutionPlan,
    },
}

impl ExecutionError {
    pub fn is_mode_violation(&self) -> bool {
        matches!(self, Self::ModeViolation { .. })
    }
}

#[derive(Debug)]
pub struct ExecutionOrchestrator<S> {
    sink: S,
    attempt_factory: RefCell<ExecutionAttemptFactory>,
}

impl<S: VenueSink> ExecutionOrchestrator<S> {
    pub fn new(sink: S) -> Self {
        Self {
            sink,
            attempt_factory: RefCell::new(ExecutionAttemptFactory::new()),
        }
    }

    pub fn plan<I: ExecutionPlanInput>(&self, input: &I) -> Result<ExecutionPlan, ExecutionError> {
        let plan = Self::plan_for_request(input.request());

        if input.execution_mode() == ExecutionMode::ReduceOnly && plan.is_risk_expanding() {
            return Err(ExecutionError::ModeViolation {
                execution_mode: input.execution_mode(),
                plan,
            });
        }

        Ok(plan)
    }

    pub fn execute<I: ExecutionPlanInput>(
        &self,
        input: &I,
    ) -> Result<domain::ExecutionReceipt, ExecutionError> {
        let plan = self.plan(input)?;
        let request = input.request();
        let execution_mode = input.execution_mode();
        let (_attempt, attempt_context) = self.attempt_factory.borrow_mut().next_for_plan(
            &plan,
            &request.snapshot_id,
            execution_mode,
        );

        self.sink.execute(&plan, &attempt_context)
    }

    fn plan_for_request(request: &ExecutionRequest) -> ExecutionPlan {
        let condition_id = ConditionId::from(request.snapshot_id.clone());
        let order_id = OrderId::from(request.decision_input_id.clone());

        if request.request_id.contains("redeem") || request.decision_input_id.contains("redeem") {
            ExecutionPlan::RedeemResolved { condition_id }
        } else if request.request_id.contains("cancel")
            || request.decision_input_id.contains("cancel")
        {
            ExecutionPlan::CancelStale { order_id }
        } else if request.request_id.contains("split")
            || request.decision_input_id.contains("split")
        {
            ExecutionPlan::FullSetSplitThenSell { condition_id }
        } else {
            ExecutionPlan::FullSetBuyThenMerge { condition_id }
        }
    }
}
