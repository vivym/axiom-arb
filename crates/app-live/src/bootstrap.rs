use state::{ReconcileReport, RemoteSnapshot, StateStore};

use crate::{runtime::AppRuntimeMode, supervisor::AppSupervisor};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapStatus {
    CancelOnly,
    Ready,
}

pub trait BootstrapSource {
    fn snapshot(&self) -> RemoteSnapshot;
}

#[derive(Debug, Clone, Default)]
pub struct StaticSnapshotSource {
    snapshot: RemoteSnapshot,
}

impl StaticSnapshotSource {
    pub fn new(snapshot: RemoteSnapshot) -> Self {
        Self { snapshot }
    }

    pub fn empty() -> Self {
        Self::default()
    }
}

impl BootstrapSource for StaticSnapshotSource {
    fn snapshot(&self) -> RemoteSnapshot {
        self.snapshot.clone()
    }
}

pub fn supervisor_with_source<S>(app_mode: AppRuntimeMode, source: &S) -> AppSupervisor
where
    S: BootstrapSource,
{
    AppSupervisor::new(app_mode, source.snapshot())
}

pub fn bootstrap_status(store: &StateStore) -> BootstrapStatus {
    if store.first_reconcile_succeeded() {
        BootstrapStatus::Ready
    } else {
        BootstrapStatus::CancelOnly
    }
}

pub fn bootstrap_once<S>(store: &mut StateStore, source: &S) -> ReconcileReport
where
    S: BootstrapSource,
{
    reconcile(store, source.snapshot())
}

pub fn reconcile(store: &mut StateStore, snapshot: RemoteSnapshot) -> ReconcileReport {
    store.reconcile(snapshot)
}
