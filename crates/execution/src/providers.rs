use domain::ExecutionAttemptContext;

use crate::plans::ExecutionPlan;
use crate::signing::{OrderSigner, SignedFamilySubmission, SigningError};

pub use domain::{LiveSubmissionRecord, LiveSubmitOutcome, PendingReconcileWork, ReconcileOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmitProviderError {
    pub reason: String,
}

impl SubmitProviderError {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileProviderError {
    pub reason: String,
}

impl ReconcileProviderError {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

pub trait SignerProvider: Send + Sync {
    fn sign_family(&self, plan: &ExecutionPlan) -> Result<SignedFamilySubmission, SigningError>;
}

impl<T> SignerProvider for T
where
    T: OrderSigner + ?Sized,
{
    fn sign_family(&self, plan: &ExecutionPlan) -> Result<SignedFamilySubmission, SigningError> {
        OrderSigner::sign_family(self, plan)
    }
}

pub trait VenueExecutionProvider: Send + Sync {
    fn submit_family(
        &self,
        signed: &SignedFamilySubmission,
        attempt: &ExecutionAttemptContext,
    ) -> Result<LiveSubmitOutcome, SubmitProviderError>;
}

pub trait ReconcileProvider: Send + Sync {
    fn reconcile_live(
        &self,
        work: &PendingReconcileWork,
    ) -> Result<ReconcileOutcome, ReconcileProviderError>;
}

pub use VenueExecutionProvider as LiveSubmitProvider;
