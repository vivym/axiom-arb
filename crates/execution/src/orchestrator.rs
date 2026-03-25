use std::cell::RefCell;

use domain::{ExecutionMode, ExecutionReceipt, ExecutionRequest};

use crate::{
    attempt::ExecutionAttemptFactory,
    plans::ExecutionPlan,
    sink::{VenueSink, VenueSinkError},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionPlanningInput {
    pub request: ExecutionRequest,
    pub execution_mode: ExecutionMode,
    pub plan: ExecutionPlan,
}

impl ExecutionPlanningInput {
    pub fn new(
        request: ExecutionRequest,
        execution_mode: ExecutionMode,
        plan: ExecutionPlan,
    ) -> Self {
        Self {
            request,
            execution_mode,
            plan,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionError {
    ModeViolation {
        execution_mode: ExecutionMode,
        plan: ExecutionPlan,
    },
    Sink {
        error: VenueSinkError,
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

    pub fn with_attempt_factory(sink: S, attempt_factory: ExecutionAttemptFactory) -> Self {
        Self {
            sink,
            attempt_factory: RefCell::new(attempt_factory),
        }
    }

    pub fn plan(&self, input: &ExecutionPlanningInput) -> Result<ExecutionPlan, ExecutionError> {
        if input.execution_mode != input.request.activation_mode {
            return Err(ExecutionError::ModeViolation {
                execution_mode: input.request.activation_mode,
                plan: input.plan.clone(),
            });
        }

        if input.request.activation_mode == ExecutionMode::ReduceOnly
            && input.plan.is_risk_expanding()
        {
            return Err(ExecutionError::ModeViolation {
                execution_mode: input.request.activation_mode,
                plan: input.plan.clone(),
            });
        }

        if input.request.activation_mode == ExecutionMode::RecoveryOnly
            && matches!(input.plan, ExecutionPlan::NegRiskSubmitFamily { .. })
        {
            return Err(ExecutionError::ModeViolation {
                execution_mode: input.request.activation_mode,
                plan: input.plan.clone(),
            });
        }

        Ok(input.plan.clone())
    }

    pub fn execute(
        &self,
        input: &ExecutionPlanningInput,
    ) -> Result<ExecutionReceipt, ExecutionError> {
        let plan = self.plan(input)?;
        let (_attempt, attempt_context) = self
            .attempt_factory
            .borrow_mut()
            .next_for_plan(&plan, &input.request);

        self.sink
            .execute(&plan, &attempt_context)
            .map_err(|error| ExecutionError::Sink { error })
    }
}
