use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use domain::{ExecutionAttemptContext, ExecutionAttemptOutcome, ExecutionMode, ExecutionReceipt};

use crate::plans::ExecutionPlan;
use crate::providers::{
    LiveSubmitOutcome, RouteExecutionAdapter, SignerProvider, SubmitProviderError,
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
    fn sink_kind(&self) -> &'static str;

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
    route_execution_adapters: BTreeMap<&'static str, Arc<dyn RouteExecutionAdapter>>,
}

impl fmt::Debug for LiveVenueSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LiveVenueSink")
            .field(
                "route_execution_adapters",
                &self
                    .route_execution_adapters
                    .keys()
                    .copied()
                    .collect::<Vec<_>>(),
            )
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
        Self::default().register_route_execution_adapter(Arc::new(
            NegRiskRouteExecutionAdapter::with_submit_provider(signer, submit_provider),
        ))
    }

    pub fn with_order_signer(order_signer: Arc<dyn OrderSigner>) -> Self {
        Self::default().register_route_execution_adapter(Arc::new(
            NegRiskRouteExecutionAdapter::with_order_signer(order_signer),
        ))
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

    pub fn with_route_execution_adapter(
        route_execution_adapter: Arc<dyn RouteExecutionAdapter>,
    ) -> Self {
        Self::default().register_route_execution_adapter(route_execution_adapter)
    }

    pub fn register_route_execution_adapter(
        mut self,
        route_execution_adapter: Arc<dyn RouteExecutionAdapter>,
    ) -> Self {
        self.route_execution_adapters
            .insert(route_execution_adapter.route(), route_execution_adapter);
        self
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
    fn sink_kind(&self) -> &'static str {
        "live"
    }

    fn execute(
        &self,
        plan: &ExecutionPlan,
        attempt: &ExecutionAttemptContext,
    ) -> Result<ExecutionReceipt, VenueSinkError> {
        ensure_live_sink_mode(plan, attempt.execution_mode)?;

        if plan.is_risk_expanding() {
            let route = plan.route().unwrap_or(attempt.route.as_str());
            let adapter = self.route_execution_adapters.get(route).ok_or_else(|| {
                VenueSinkError::Rejected {
                    reason: format!("missing live execution adapter for route {route}"),
                }
            })?;
            let live_outcome =
                adapter
                    .submit_live(plan, attempt)
                    .map_err(|err| VenueSinkError::Rejected {
                        reason: format!("route execution adapter error: {}", err.reason),
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
    fn sink_kind(&self) -> &'static str {
        "shadow"
    }

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

        Ok(LiveSubmitOutcome::AcceptedButUnconfirmed {
            submission_record: None,
            pending_ref: format!("pending-hook:{}", attempt.attempt_id),
        })
    }
}

#[derive(Clone)]
pub struct NegRiskRouteExecutionAdapter {
    signer: Option<Arc<dyn SignerProvider>>,
    submit_provider: Option<Arc<dyn VenueExecutionProvider>>,
}

impl NegRiskRouteExecutionAdapter {
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

impl RouteExecutionAdapter for NegRiskRouteExecutionAdapter {
    fn route(&self) -> &'static str {
        "neg-risk"
    }

    fn submit_live(
        &self,
        plan: &ExecutionPlan,
        attempt: &ExecutionAttemptContext,
    ) -> Result<LiveSubmitOutcome, SubmitProviderError> {
        let signer = self
            .signer
            .as_ref()
            .ok_or_else(|| SubmitProviderError::new("missing signer provider for neg-risk"))?;
        let submit_provider = self
            .submit_provider
            .as_ref()
            .ok_or_else(|| SubmitProviderError::new("missing submit provider for neg-risk"))?;

        let signed = signer
            .sign_family(plan)
            .map_err(|err| SubmitProviderError::new(format!("signing error: {err:?}")))?;

        submit_provider.submit_family(&signed, attempt)
    }
}

fn receipt_from_live_submit_outcome(
    attempt: &ExecutionAttemptContext,
    outcome: LiveSubmitOutcome,
) -> ExecutionReceipt {
    match outcome {
        LiveSubmitOutcome::Accepted { submission_record } => ExecutionReceipt::new(
            attempt.attempt_id.clone(),
            ExecutionAttemptOutcome::Succeeded,
        )
        .with_submission_ref(submission_record.submission_ref),
        LiveSubmitOutcome::AcceptedButUnconfirmed {
            submission_record,
            pending_ref,
        } => ExecutionReceipt::new(
            attempt.attempt_id.clone(),
            ExecutionAttemptOutcome::Succeeded,
        )
        .with_pending_ref(pending_ref)
        .tap_if_some(submission_record.map(|record| record.submission_ref)),
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

trait ExecutionReceiptExt {
    fn tap_if_some(self, submission_ref: Option<String>) -> Self;
}

impl ExecutionReceiptExt for ExecutionReceipt {
    fn tap_if_some(self, submission_ref: Option<String>) -> Self {
        match submission_ref {
            Some(submission_ref) => self.with_submission_ref(submission_ref),
            None => self,
        }
    }
}
