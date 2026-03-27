use domain::FamilyDiscoveryRecord;

use crate::StateStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateProjectionStatus {
    Ready,
    Lagging { reason: String },
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateProjectionReadiness {
    publication_id: String,
    status: CandidateProjectionStatus,
}

impl CandidateProjectionReadiness {
    pub fn ready(publication_id: impl Into<String>) -> Self {
        Self {
            publication_id: publication_id.into(),
            status: CandidateProjectionStatus::Ready,
        }
    }

    pub fn lagging(publication_id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            publication_id: publication_id.into(),
            status: CandidateProjectionStatus::Lagging {
                reason: reason.into(),
            },
        }
    }

    pub fn failed(publication_id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            publication_id: publication_id.into(),
            status: CandidateProjectionStatus::Failed {
                reason: reason.into(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateView {
    pub publication_id: String,
    pub state_version: u64,
    pub committed_journal_seq: i64,
    pub discovery_records: Vec<FamilyDiscoveryRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidatePublication {
    pub publication_id: String,
    pub state_version: u64,
    pub committed_journal_seq: i64,
    pub ready: bool,
    pub failure_reason: Option<String>,
    pub lag_reason: Option<String>,
    pub view: Option<CandidateView>,
}

impl CandidatePublication {
    pub fn from_store(store: &StateStore, readiness: CandidateProjectionReadiness) -> Self {
        let committed_journal_seq = store
            .last_applied_journal_seq()
            .expect("candidate publications require an applied journal anchor");
        let state_version = store.state_version();

        let (ready, failure_reason, lag_reason, view) = match readiness.status {
            CandidateProjectionStatus::Ready => (
                true,
                None,
                None,
                Some(CandidateView {
                    publication_id: readiness.publication_id.clone(),
                    state_version,
                    committed_journal_seq,
                    discovery_records: store.family_discovery_records(),
                }),
            ),
            CandidateProjectionStatus::Lagging { reason } => (false, None, Some(reason), None),
            CandidateProjectionStatus::Failed { reason } => (false, Some(reason), None, None),
        };

        Self {
            publication_id: readiness.publication_id,
            state_version,
            committed_journal_seq,
            ready,
            failure_reason,
            lag_reason,
            view,
        }
    }
}
