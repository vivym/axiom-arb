use std::cell::RefCell;

use domain::{
    ExecutionAttempt, ExecutionAttemptContext, ExecutionMode, ExecutionReceipt, ExecutionRequest,
};
use observability::field_keys;

use crate::{
    attempt::ExecutionAttemptFactory,
    instrumentation::ExecutionInstrumentation,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionAttemptRecord {
    pub attempt: ExecutionAttempt,
    pub attempt_context: ExecutionAttemptContext,
    pub receipt: ExecutionReceipt,
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
    instrumentation: ExecutionInstrumentation,
}

impl<S: VenueSink> ExecutionOrchestrator<S> {
    pub fn new(sink: S) -> Self {
        Self::with_attempt_factory_instrumented(
            sink,
            ExecutionAttemptFactory::new(),
            ExecutionInstrumentation::disabled(),
        )
    }

    pub fn new_instrumented(sink: S, instrumentation: ExecutionInstrumentation) -> Self {
        Self::with_attempt_factory_instrumented(
            sink,
            ExecutionAttemptFactory::new(),
            instrumentation,
        )
    }

    pub fn with_attempt_factory(sink: S, attempt_factory: ExecutionAttemptFactory) -> Self {
        Self::with_attempt_factory_instrumented(
            sink,
            attempt_factory,
            ExecutionInstrumentation::disabled(),
        )
    }

    pub fn with_attempt_factory_instrumented(
        sink: S,
        attempt_factory: ExecutionAttemptFactory,
        instrumentation: ExecutionInstrumentation,
    ) -> Self {
        Self {
            sink,
            attempt_factory: RefCell::new(attempt_factory),
            instrumentation,
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

        Ok(input.plan.clone())
    }

    pub fn execute(
        &self,
        input: &ExecutionPlanningInput,
    ) -> Result<ExecutionReceipt, ExecutionError> {
        self.execute_with_attempt(input)
            .map(|record| record.receipt)
    }

    pub fn execute_with_attempt(
        &self,
        input: &ExecutionPlanningInput,
    ) -> Result<ExecutionAttemptRecord, ExecutionError> {
        let plan = self.plan(input)?;
        let (attempt, attempt_context) = self.attempt_factory.borrow_mut().next_for_plan(
            &plan,
            &input.request,
            input.execution_mode,
        );
        let attempt_span =
            self.instrumentation
                .attempt_span(self.sink.sink_kind(), &attempt, &attempt_context);
        let _attempt_span_guard = attempt_span.as_ref().map(|span| span.enter());

        let result = self.sink.execute(&plan, &attempt_context);

        if let Some(span) = &attempt_span {
            span.record(field_keys::ATTEMPT_OUTCOME, attempt_outcome_label(&result));
        }
        if matches!(
            result.as_ref(),
            Ok(receipt) if receipt.outcome == domain::ExecutionAttemptOutcome::ShadowRecorded
        ) {
            self.instrumentation.increment_shadow_attempt_count(1);
        }

        let receipt = result.map_err(|error| ExecutionError::Sink { error })?;

        Ok(ExecutionAttemptRecord {
            attempt,
            attempt_context,
            receipt,
        })
    }
}

fn attempt_outcome_label(result: &Result<ExecutionReceipt, VenueSinkError>) -> &'static str {
    match result {
        Ok(receipt) => match receipt.outcome {
            domain::ExecutionAttemptOutcome::Succeeded => "succeeded",
            domain::ExecutionAttemptOutcome::ShadowRecorded => "shadow_recorded",
            domain::ExecutionAttemptOutcome::FailedDefinitive => "failed_definitive",
            domain::ExecutionAttemptOutcome::FailedAmbiguous => "failed_ambiguous",
            domain::ExecutionAttemptOutcome::RetryExhausted => "retry_exhausted",
        },
        Err(_) => "sink_error",
    }
}
