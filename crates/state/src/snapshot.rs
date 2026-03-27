use crate::StateStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionReadiness {
    snapshot_id: String,
    fullset_ready: bool,
    negrisk_ready: bool,
}

impl ProjectionReadiness {
    pub fn new(snapshot_id: impl Into<String>, fullset_ready: bool, negrisk_ready: bool) -> Self {
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
    // Candidate publication readiness is carried separately via CandidatePublication.
    pub fullset: Option<FullSetView>,
    pub negrisk: Option<NegRiskView>,
}

impl PublishedSnapshot {
    pub fn from_store(store: &StateStore, readiness: ProjectionReadiness) -> Self {
        let state_version = store.state_version();
        let committed_journal_seq = store
            .last_applied_journal_seq()
            .expect("published snapshots require an applied journal anchor");
        let fullset = store
            .anchored_fullset()
            .filter(|_| readiness.fullset_ready)
            .map(|anchor| FullSetView {
                snapshot_id: readiness.snapshot_id.clone(),
                state_version: anchor.state_version,
                open_orders: anchor.open_orders.clone(),
            });
        let fullset_ready = readiness.fullset_ready && fullset.is_some();
        let negrisk_ready = false;
        let negrisk = None;

        Self {
            snapshot_id: readiness.snapshot_id,
            state_version,
            committed_journal_seq,
            fullset_ready,
            negrisk_ready,
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
pub struct NegRiskFamilyRolloutReadiness {
    pub family_id: String,
    pub shadow_parity_ready: bool,
    pub recovery_ready: bool,
    pub replay_drift_ready: bool,
    pub fault_injection_ready: bool,
    pub conversion_path_ready: bool,
    pub halt_semantics_ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegRiskView {
    pub snapshot_id: String,
    pub state_version: u64,
    pub families: Vec<NegRiskFamilyRolloutReadiness>,
}

impl NegRiskView {
    pub fn family_ids(&self) -> Vec<String> {
        self.families
            .iter()
            .map(|family| family.family_id.clone())
            .collect()
    }
}
