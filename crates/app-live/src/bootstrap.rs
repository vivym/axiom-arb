use state::{ReconcileReport, RemoteSnapshot, StateStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapStatus {
    CancelOnly,
    Ready,
}

pub fn bootstrap_status(store: &StateStore) -> BootstrapStatus {
    if store.first_reconcile_succeeded() {
        BootstrapStatus::Ready
    } else {
        BootstrapStatus::CancelOnly
    }
}

pub fn reconcile(store: &mut StateStore, snapshot: RemoteSnapshot) -> ReconcileReport {
    store.reconcile(snapshot)
}
