use crate::StateStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionReadiness {
    snapshot_id: String,
    fullset_ready: bool,
    negrisk_ready: bool,
}

impl ProjectionReadiness {
    pub fn new(
        snapshot_id: impl Into<String>,
        fullset_ready: bool,
        negrisk_ready: bool,
    ) -> Self {
        Self {
            snapshot_id: snapshot_id.into(),
            fullset_ready,
            negrisk_ready,
        }
    }

    pub fn ready_fullset_pending_negrisk(snapshot_id: impl Into<String>) -> Self {
        Self::new(snapshot_id, true, false)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedSnapshot {
    pub snapshot_id: String,
    pub state_version: u64,
    pub committed_journal_seq: i64,
    pub fullset_ready: bool,
    pub negrisk_ready: bool,
    pub fullset: Option<FullSetView>,
    pub negrisk: Option<NegRiskView>,
}

impl PublishedSnapshot {
    pub fn from_store(store: &StateStore, readiness: ProjectionReadiness) -> Self {
        let state_version = store.state_version();
        let committed_journal_seq = store.last_applied_journal_seq().unwrap_or_default();
        let fullset = readiness.fullset_ready.then(|| FullSetView {
            snapshot_id: readiness.snapshot_id.clone(),
            state_version,
            open_orders: store.fullset_open_order_ids(),
        });
        let negrisk = readiness.negrisk_ready.then(|| NegRiskView {
            snapshot_id: readiness.snapshot_id.clone(),
            state_version,
            family_ids: store.negrisk_family_ids(),
        });

        Self {
            snapshot_id: readiness.snapshot_id,
            state_version,
            committed_journal_seq,
            fullset_ready: readiness.fullset_ready,
            negrisk_ready: readiness.negrisk_ready,
            fullset,
            negrisk,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FullSetView {
    pub snapshot_id: String,
    pub state_version: u64,
    pub open_orders: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegRiskView {
    pub snapshot_id: String,
    pub state_version: u64,
    pub family_ids: Vec<String>,
}
