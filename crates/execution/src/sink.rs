use domain::{ExecutionAttemptContext, ExecutionAttemptOutcome, ExecutionMode};

use crate::{orchestrator::ExecutionError, plans::ExecutionPlan};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionReceipt {
    inner: domain::ExecutionReceipt,
    shadow_recorded: bool,
    authoritative_fill_effect: bool,
}

impl ExecutionReceipt {
    pub fn live(attempt_id: impl Into<String>) -> Self {
        Self {
            inner: domain::ExecutionReceipt {
                attempt_id: attempt_id.into(),
                outcome: ExecutionAttemptOutcome::Succeeded,
            },
            shadow_recorded: false,
            authoritative_fill_effect: true,
        }
    }

    pub fn shadow(attempt_id: impl Into<String>) -> Self {
        Self {
            inner: domain::ExecutionReceipt {
                attempt_id: attempt_id.into(),
                outcome: ExecutionAttemptOutcome::Succeeded,
            },
            shadow_recorded: true,
            authoritative_fill_effect: false,
        }
    }

    pub fn attempt_id(&self) -> &str {
        &self.inner.attempt_id
    }

    pub fn outcome(&self) -> ExecutionAttemptOutcome {
        self.inner.outcome
    }

    pub fn is_shadow_recorded(&self) -> bool {
        self.shadow_recorded
    }

    pub fn has_authoritative_fill_effect(&self) -> bool {
        self.authoritative_fill_effect
    }

    pub fn into_inner(self) -> domain::ExecutionReceipt {
        self.inner
    }
}

pub trait VenueSink {
    fn execution_mode(&self) -> ExecutionMode;

    fn execute(
        &self,
        plan: &ExecutionPlan,
        attempt: &ExecutionAttemptContext,
    ) -> Result<ExecutionReceipt, ExecutionError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LiveVenueSink;

impl LiveVenueSink {
    pub fn noop() -> Self {
        Self
    }
}

impl VenueSink for LiveVenueSink {
    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Live
    }

    fn execute(
        &self,
        _plan: &ExecutionPlan,
        attempt: &ExecutionAttemptContext,
    ) -> Result<ExecutionReceipt, ExecutionError> {
        Ok(ExecutionReceipt::live(attempt.attempt_id.clone()))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ShadowVenueSink;

impl ShadowVenueSink {
    pub fn noop() -> Self {
        Self
    }
}

impl VenueSink for ShadowVenueSink {
    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Shadow
    }

    fn execute(
        &self,
        _plan: &ExecutionPlan,
        attempt: &ExecutionAttemptContext,
    ) -> Result<ExecutionReceipt, ExecutionError> {
        Ok(ExecutionReceipt::shadow(attempt.attempt_id.clone()))
    }
}
