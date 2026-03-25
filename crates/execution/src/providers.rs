use domain::ExecutionAttemptContext;

use crate::signing::SignedFamilySubmission;

pub use domain::{LiveSubmissionRecord, LiveSubmitOutcome, PendingReconcileWork, ReconcileOutcome};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveSubmitRequest<'a> {
    pub attempt: &'a ExecutionAttemptContext,
    pub signed_submission: &'a SignedFamilySubmission,
}

pub trait LiveSubmitProvider {
    fn submit_live(&self, request: &LiveSubmitRequest<'_>) -> LiveSubmitOutcome;
}

pub trait ReconcileProvider {
    fn reconcile_live(&self, work: &PendingReconcileWork) -> ReconcileOutcome;
}
