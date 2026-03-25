use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use domain::{ExecutionAttemptContext, ExecutionAttemptOutcome, ExecutionMode, ExecutionReceipt};

use crate::plans::ExecutionPlan;
use crate::providers::{
    LiveSubmissionRecord, LiveSubmitOutcome, SignerProvider, SubmitProviderError,
    VenueExecutionProvider,
};
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
    signer: Option<Arc<dyn SignerProvider>>,
    submit_provider: Option<Arc<dyn VenueExecutionProvider>>,
}

impl fmt::Debug for LiveVenueSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LiveVenueSink")
            .field("signer", &self.signer.is_some())
            .field("submit_provider", &self.submit_provider.is_some())
            .finish()
    }
}

impl LiveVenueSink {
    pub fn noop() -> Self {
        Self::default()
    }

    pub fn with_submit_provider(
        signer: Arc<dyn SignerProvider>,
        submit_provider: Arc<dyn VenueExecutionProvider>,
    ) -> Self {
        Self {
            signer: Some(signer),
            submit_provider: Some(submit_provider),
        }
    }

    pub fn with_order_signer(order_signer: Arc<dyn OrderSigner>) -> Self {
        Self {
            signer: Some(Arc::new(OrderSignerAdapter {
                inner: order_signer,
            })),
            submit_provider: None,
        }
    }

    pub fn with_order_signer_and_hook(
        order_signer: Arc<dyn OrderSigner>,
        hook: Arc<dyn SignedFamilyHook>,
    ) -> Self {
        Self::with_submit_provider(
            Arc::new(OrderSignerAdapter {
                inner: order_signer,
            }),
            Arc::new(HookSubmitProvider { hook }),
        )
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
            let signer = self
                .signer
                .as_ref()
                .ok_or_else(|| VenueSinkError::Rejected {
                    reason: "missing signer provider for NegRiskSubmitFamily".to_owned(),
                })?;
            let submit_provider =
                self.submit_provider
                    .as_ref()
                    .ok_or_else(|| VenueSinkError::Rejected {
                        reason: "missing submit provider for NegRiskSubmitFamily".to_owned(),
                    })?;

            // Sign the planned orders before handing them to the venue provider.
            let signed = signer
                .sign_family(plan)
                .map_err(|err| VenueSinkError::Rejected {
                    reason: format!("signing error: {err:?}"),
                })?;

            let live_outcome = submit_provider
                .submit_family(&signed, attempt)
                .map_err(|err| VenueSinkError::Rejected {
                    reason: format!("submit provider error: {err:?}"),
                })?;

            return Ok(receipt_from_live_submit_outcome(attempt, live_outcome));
        }

        Ok(ExecutionReceipt::new(
            attempt.attempt_id.clone(),
            ExecutionAttemptOutcome::Succeeded,
        ))
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

        Ok(ExecutionReceipt::new(
            attempt.attempt_id.clone(),
            ExecutionAttemptOutcome::ShadowRecorded,
        ))
    }
}

#[derive(Clone)]
struct OrderSignerAdapter {
    inner: Arc<dyn OrderSigner>,
}

impl SignerProvider for OrderSignerAdapter {
    fn sign_family(
        &self,
        plan: &ExecutionPlan,
    ) -> Result<crate::signing::SignedFamilySubmission, crate::signing::SigningError> {
        OrderSigner::sign_family(self.inner.as_ref(), plan)
    }
}

#[derive(Clone)]
struct HookSubmitProvider {
    hook: Arc<dyn SignedFamilyHook>,
}

impl VenueExecutionProvider for HookSubmitProvider {
    fn submit_family(
        &self,
        signed: &SignedFamilySubmission,
        attempt: &ExecutionAttemptContext,
    ) -> Result<LiveSubmitOutcome, SubmitProviderError> {
        self.hook.on_signed_family(signed, attempt).map_err(|err| {
            SubmitProviderError::new(format!("signed-family hook error: {err:?}"))
        })?;

        Ok(LiveSubmitOutcome::Accepted {
            submission_record: LiveSubmissionRecord {
                submission_ref: format!("hook-submit:{}", attempt.attempt_id),
                attempt_id: attempt.attempt_id.clone(),
                route: attempt.route.clone(),
                scope: attempt.scope.clone(),
                provider: "signed-family-hook".to_owned(),
            },
        })
    }
}

fn receipt_from_live_submit_outcome(
    attempt: &ExecutionAttemptContext,
    outcome: LiveSubmitOutcome,
) -> ExecutionReceipt {
    match outcome {
        LiveSubmitOutcome::Accepted { submission_record }
        | LiveSubmitOutcome::AcceptedButUnconfirmed { submission_record } => ExecutionReceipt::new(
            attempt.attempt_id.clone(),
            ExecutionAttemptOutcome::Succeeded,
        )
        .with_submission_ref(submission_record.submission_ref),
        LiveSubmitOutcome::RejectedDefinitive { .. } => ExecutionReceipt::new(
            attempt.attempt_id.clone(),
            ExecutionAttemptOutcome::FailedDefinitive,
        ),
        LiveSubmitOutcome::Ambiguous { pending_ref, .. } => ExecutionReceipt::new(
            attempt.attempt_id.clone(),
            ExecutionAttemptOutcome::FailedAmbiguous,
        )
        .with_pending_ref(pending_ref),
    }
}
