pub use domain::{LiveSubmissionRecord, LiveSubmitOutcome, PendingReconcileWork, ReconcileOutcome};

pub trait LiveSubmitProvider {
    fn submit_live(&self, work: &PendingReconcileWork) -> LiveSubmitOutcome;
}

pub trait ReconcileProvider {
    fn reconcile_live(&self, work: &PendingReconcileWork) -> ReconcileOutcome;
}
