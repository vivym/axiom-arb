use std::cell::RefCell;
use std::rc::Rc;

use domain::{ExecutionAttemptContext, ExecutionAttemptOutcome, ExecutionReceipt};

use crate::{orchestrator::ExecutionError, plans::ExecutionPlan};

pub trait VenueSink {
    fn execute(
        &self,
        plan: &ExecutionPlan,
        attempt: &ExecutionAttemptContext,
    ) -> Result<ExecutionReceipt, ExecutionError>;
}

#[derive(Debug, Clone, Default)]
pub struct LiveVenueSink;

impl LiveVenueSink {
    pub fn noop() -> Self {
        Self
    }
}

impl VenueSink for LiveVenueSink {
    fn execute(
        &self,
        _plan: &ExecutionPlan,
        attempt: &ExecutionAttemptContext,
    ) -> Result<ExecutionReceipt, ExecutionError> {
        Ok(ExecutionReceipt {
            attempt_id: attempt.attempt_id.clone(),
            outcome: ExecutionAttemptOutcome::Succeeded,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct ShadowVenueSink {
    recorded_attempt_ids: Rc<RefCell<Vec<String>>>,
}

impl ShadowVenueSink {
    pub fn noop() -> Self {
        Self::default()
    }

    pub fn recorded_attempt_ids(&self) -> Vec<String> {
        self.recorded_attempt_ids.borrow().clone()
    }
}

impl VenueSink for ShadowVenueSink {
    fn execute(
        &self,
        _plan: &ExecutionPlan,
        attempt: &ExecutionAttemptContext,
    ) -> Result<ExecutionReceipt, ExecutionError> {
        self.recorded_attempt_ids
            .borrow_mut()
            .push(attempt.attempt_id.clone());

        Ok(ExecutionReceipt {
            attempt_id: attempt.attempt_id.clone(),
            outcome: ExecutionAttemptOutcome::Succeeded,
        })
    }
}
