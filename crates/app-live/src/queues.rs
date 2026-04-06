use std::collections::{BTreeMap, BTreeSet, VecDeque};

use state::{CandidatePublication, DirtyDomain};

use crate::config::NegRiskFamilyLiveTarget;
use crate::input_tasks::InputTaskEvent;

const DEFAULT_FULL_SET_BASIS_DIGEST: &str = "full-set-basis-default";

pub fn default_full_set_basis_digest() -> String {
    DEFAULT_FULL_SET_BASIS_DIGEST.to_owned()
}

#[derive(Debug, Default)]
pub struct IngressQueue {
    backlog: VecDeque<InputTaskEvent>,
}

impl IngressQueue {
    pub fn push(&mut self, input: InputTaskEvent) {
        self.backlog.push_back(input);
        self.backlog
            .make_contiguous()
            .sort_by_key(|entry| entry.journal_seq);
    }

    pub fn next_after(&self, last_journal_seq: Option<i64>) -> Option<InputTaskEvent> {
        self.backlog
            .iter()
            .find(|entry| last_journal_seq.is_none_or(|last| entry.journal_seq > last))
            .cloned()
    }

    pub fn remove(&mut self, input: &InputTaskEvent) -> Option<InputTaskEvent> {
        let index = self.backlog.iter().position(|entry| entry == input)?;
        self.backlog.remove(index)
    }

    pub fn len(&self) -> usize {
        self.backlog.len()
    }

    pub fn is_empty(&self) -> bool {
        self.backlog.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotNotice {
    pub snapshot_id: String,
    pub state_version: u64,
    pub dirty_domains: BTreeSet<DirtyDomain>,
    pub fullset_ready: bool,
    pub negrisk_ready: bool,
}

impl SnapshotNotice {
    pub fn new(
        snapshot_id: impl Into<String>,
        state_version: u64,
        dirty_domains: impl IntoIterator<Item = DirtyDomain>,
    ) -> Self {
        Self {
            snapshot_id: snapshot_id.into(),
            state_version,
            dirty_domains: dirty_domains.into_iter().collect(),
            fullset_ready: false,
            negrisk_ready: false,
        }
    }

    pub fn with_projection_readiness(mut self, fullset_ready: bool, negrisk_ready: bool) -> Self {
        self.fullset_ready = fullset_ready;
        self.negrisk_ready = negrisk_ready;
        self
    }
}

#[derive(Debug, Default)]
pub struct SnapshotDispatchQueue {
    notices: VecDeque<SnapshotNotice>,
}

impl SnapshotDispatchQueue {
    pub fn push(&mut self, notice: SnapshotNotice) {
        self.notices.push_back(notice);
    }

    pub fn coalesced(&self) -> Vec<SnapshotNotice> {
        let latest_fullset = self
            .notices
            .iter()
            .filter(|notice| {
                notice
                    .dirty_domains
                    .iter()
                    .any(|domain| is_fullset_domain(*domain))
            })
            .max_by_key(|notice| notice.state_version)
            .cloned();
        let latest_negrisk = self
            .notices
            .iter()
            .filter(|notice| notice.dirty_domains.contains(&DirtyDomain::NegRiskFamilies))
            .max_by_key(|notice| notice.state_version)
            .cloned();

        let mut coalesced = BTreeMap::<u64, SnapshotNotice>::new();
        for notice in [latest_fullset, latest_negrisk].into_iter().flatten() {
            coalesced.insert(notice.state_version, notice);
        }

        coalesced.into_values().collect()
    }

    pub fn len(&self) -> usize {
        self.notices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notices.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CandidateRestrictionTruth {
    Eligible,
    Restricted { reason: String },
}

impl CandidateRestrictionTruth {
    pub fn eligible() -> Self {
        Self::Eligible
    }

    pub fn restricted(reason: impl Into<String>) -> Self {
        Self::Restricted {
            reason: reason.into(),
        }
    }

    pub fn restriction_reason(&self) -> Option<&str> {
        match self {
            Self::Eligible => None,
            Self::Restricted { reason } => Some(reason.as_str()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateNotice {
    pub publication: CandidatePublication,
    pub dirty_domains: BTreeSet<DirtyDomain>,
    pub operator_target_revision: Option<String>,
    pub rendered_live_targets: BTreeMap<String, NegRiskFamilyLiveTarget>,
    pub restriction: CandidateRestrictionTruth,
    // Authoritative discovery may render adoptable output without observed backfill completion.
    pub authoritative: bool,
    pub full_set_basis_digest: String,
}

impl CandidateNotice {
    pub fn from_publication(
        publication: &CandidatePublication,
        dirty_domains: impl IntoIterator<Item = DirtyDomain>,
        operator_target_revision: Option<&str>,
        rendered_live_targets: BTreeMap<String, NegRiskFamilyLiveTarget>,
        restriction: CandidateRestrictionTruth,
    ) -> Self {
        Self::from_publication_with_authority(
            publication,
            dirty_domains,
            operator_target_revision,
            rendered_live_targets,
            restriction,
            false,
        )
    }

    pub fn authoritative_from_publication(
        publication: &CandidatePublication,
        dirty_domains: impl IntoIterator<Item = DirtyDomain>,
        operator_target_revision: Option<&str>,
        rendered_live_targets: BTreeMap<String, NegRiskFamilyLiveTarget>,
        restriction: CandidateRestrictionTruth,
    ) -> Self {
        Self::from_publication_with_authority(
            publication,
            dirty_domains,
            operator_target_revision,
            rendered_live_targets,
            restriction,
            true,
        )
    }

    fn from_publication_with_authority(
        publication: &CandidatePublication,
        dirty_domains: impl IntoIterator<Item = DirtyDomain>,
        operator_target_revision: Option<&str>,
        rendered_live_targets: BTreeMap<String, NegRiskFamilyLiveTarget>,
        restriction: CandidateRestrictionTruth,
        authoritative: bool,
    ) -> Self {
        let notice = Self {
            publication: publication.clone(),
            dirty_domains: dirty_domains.into_iter().collect(),
            operator_target_revision: operator_target_revision.map(str::to_owned),
            rendered_live_targets,
            restriction,
            authoritative,
            full_set_basis_digest: default_full_set_basis_digest(),
        };

        notice
    }

    pub fn with_full_set_basis_digest(mut self, full_set_basis_digest: impl Into<String>) -> Self {
        self.full_set_basis_digest = full_set_basis_digest.into();
        self
    }
}

#[derive(Debug, Default)]
pub struct CandidateNoticeQueue {
    backlog: VecDeque<CandidateNotice>,
}

impl CandidateNoticeQueue {
    pub fn push(&mut self, notice: CandidateNotice) {
        self.backlog.push_back(notice);
    }

    pub fn pop_front(&mut self) -> Option<CandidateNotice> {
        self.backlog.pop_front()
    }

    pub fn coalesced(&self) -> Vec<CandidateNotice> {
        let mut coalesced = BTreeMap::new();
        for notice in self
            .backlog
            .iter()
            .filter(|notice| notice.dirty_domains.contains(&DirtyDomain::Candidates))
        {
            coalesced.insert(
                (
                    notice.publication.publication_id.clone(),
                    notice.publication.state_version,
                    notice.operator_target_revision.clone(),
                    notice.restriction.clone(),
                    notice.authoritative,
                    notice.full_set_basis_digest.clone(),
                ),
                notice.clone(),
            );
        }

        coalesced.into_values().collect()
    }

    pub fn len(&self) -> usize {
        self.backlog.len()
    }

    pub fn is_empty(&self) -> bool {
        self.backlog.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FollowUpWork {
    PendingReconcile {
        scope_id: String,
        pending_ref: String,
        reason: String,
    },
    Recovery {
        scope_id: String,
        reason: String,
    },
}

impl FollowUpWork {
    pub fn pending_reconcile(
        scope_id: impl Into<String>,
        pending_ref: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::PendingReconcile {
            scope_id: scope_id.into(),
            pending_ref: pending_ref.into(),
            reason: reason.into(),
        }
    }

    pub fn recovery(scope_id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Recovery {
            scope_id: scope_id.into(),
            reason: reason.into(),
        }
    }
}

#[derive(Debug, Default)]
pub struct FollowUpQueue {
    backlog: VecDeque<FollowUpWork>,
}

impl FollowUpQueue {
    pub fn push(&mut self, work: FollowUpWork) {
        self.backlog.push_back(work);
    }

    pub fn pop_front(&mut self) -> Option<FollowUpWork> {
        self.backlog.pop_front()
    }

    pub fn len(&self) -> usize {
        self.backlog.len()
    }

    pub fn is_empty(&self) -> bool {
        self.backlog.is_empty()
    }
}

pub(crate) fn fullset_domains() -> BTreeSet<DirtyDomain> {
    BTreeSet::from([
        DirtyDomain::Runtime,
        DirtyDomain::Orders,
        DirtyDomain::Inventory,
        DirtyDomain::Approvals,
        DirtyDomain::Resolution,
        DirtyDomain::Relayer,
    ])
}

pub(crate) fn negrisk_domains() -> BTreeSet<DirtyDomain> {
    BTreeSet::from([DirtyDomain::NegRiskFamilies])
}

pub(crate) fn is_fullset_domain(domain: DirtyDomain) -> bool {
    fullset_domains().contains(&domain)
}
