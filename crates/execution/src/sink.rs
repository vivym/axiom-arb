use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use domain::{ExecutionAttemptContext, ExecutionAttemptOutcome, ExecutionMode, ExecutionReceipt};

use crate::plans::ExecutionPlan;
use crate::signing::{OrderSigner, SignedFamilySubmission};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VenueSinkError {
    Rejected {
        reason: String,
    },
    ModeMismatch {
        sink: &'static str,
        expected: ExecutionMode,
        actual: ExecutionMode,
    },
}

pub trait VenueSink {
    fn execute(
        &self,
        plan: &ExecutionPlan,
        attempt: &ExecutionAttemptContext,
    ) -> Result<ExecutionReceipt, VenueSinkError>;
}

pub trait SignedFamilyHook: Send + Sync {
    fn on_signed_family(
        &self,
        signed: &SignedFamilySubmission,
        attempt: &ExecutionAttemptContext,
    ) -> Result<(), SignedFamilyHookError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedFamilyHookError {
    pub reason: String,
}

#[derive(Clone, Default)]
pub struct LiveVenueSink {
    order_signer: Option<Arc<dyn OrderSigner>>,
    signed_family_hook: Option<Arc<dyn SignedFamilyHook>>,
}

impl fmt::Debug for LiveVenueSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LiveVenueSink")
            .field("order_signer", &self.order_signer.is_some())
            .field("signed_family_hook", &self.signed_family_hook.is_some())
            .finish()
    }
}

impl LiveVenueSink {
    pub fn noop() -> Self {
        Self::default()
    }

    pub fn with_order_signer(order_signer: Arc<dyn OrderSigner>) -> Self {
        Self {
            order_signer: Some(order_signer),
            signed_family_hook: None,
        }
    }

    pub fn with_order_signer_and_hook(
        order_signer: Arc<dyn OrderSigner>,
        hook: Arc<dyn SignedFamilyHook>,
    ) -> Self {
        Self {
            order_signer: Some(order_signer),
            signed_family_hook: Some(hook),
        }
    }
}

fn ensure_sink_mode(
    sink: &'static str,
    expected: ExecutionMode,
    actual: ExecutionMode,
) -> Result<(), VenueSinkError> {
    if actual == expected {
        Ok(())
    } else {
        Err(VenueSinkError::ModeMismatch {
            sink,
            expected,
            actual,
        })
    }
}

fn ensure_live_sink_mode(
    plan: &ExecutionPlan,
    actual: ExecutionMode,
) -> Result<(), VenueSinkError> {
    match actual {
        ExecutionMode::Live | ExecutionMode::RecoveryOnly => Ok(()),
        ExecutionMode::ReduceOnly if !plan.is_risk_expanding() => Ok(()),
        other => Err(VenueSinkError::ModeMismatch {
            sink: "live",
            expected: ExecutionMode::Live,
            actual: other,
        }),
    }
}

impl VenueSink for LiveVenueSink {
    fn execute(
        &self,
        plan: &ExecutionPlan,
        attempt: &ExecutionAttemptContext,
    ) -> Result<ExecutionReceipt, VenueSinkError> {
        ensure_live_sink_mode(plan, attempt.execution_mode)?;

        if let ExecutionPlan::NegRiskSubmitFamily { .. } = plan {
            // Fail-closed: neg-risk family submit plans must never reach "Succeeded" without a signer.
            let signer = self.order_signer.as_ref().ok_or_else(|| VenueSinkError::Rejected {
                reason: "missing order signer for NegRiskSubmitFamily".to_owned(),
            })?;

            // Narrow plumbing hook for Phase 3b neg-risk live submit: sign the planned orders
            // deterministically. Non-neg-risk plans must remain unaffected by signer/hook configuration.
            let signed = signer.sign_family(plan).map_err(|err| VenueSinkError::Rejected {
                reason: format!("signing error: {err:?}"),
            })?;

            if let Some(hook) = &self.signed_family_hook {
                hook.on_signed_family(&signed, attempt)
                    .map_err(|err| VenueSinkError::Rejected {
                        reason: format!("signed-family hook error: {err:?}"),
                    })?;
            }
        }

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
    ) -> Result<ExecutionReceipt, VenueSinkError> {
        ensure_sink_mode("shadow", ExecutionMode::Shadow, attempt.execution_mode)?;
        self.recorded_attempt_ids
            .borrow_mut()
            .push(attempt.attempt_id.clone());

        Ok(ExecutionReceipt {
            attempt_id: attempt.attempt_id.clone(),
            outcome: ExecutionAttemptOutcome::ShadowRecorded,
        })
    }
}
