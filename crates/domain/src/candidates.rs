use chrono::{DateTime, Utc};

use crate::EventFamilyId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverySourceAnchor {
    pub source_kind: String,
    pub source_session_id: String,
    pub source_event_id: String,
    pub normalizer_version: String,
}

impl DiscoverySourceAnchor {
    pub fn new(
        source_kind: impl Into<String>,
        source_session_id: impl Into<String>,
        source_event_id: impl Into<String>,
        normalizer_version: impl Into<String>,
    ) -> Self {
        Self {
            source_kind: source_kind.into(),
            source_session_id: source_session_id.into(),
            source_event_id: source_event_id.into(),
            normalizer_version: normalizer_version.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyDiscoveryRecord {
    pub family_id: EventFamilyId,
    pub source: DiscoverySourceAnchor,
    pub discovered_at: DateTime<Utc>,
    pub backfill_cursor: Option<String>,
    pub backfill_completed_at: Option<DateTime<Utc>>,
}

impl FamilyDiscoveryRecord {
    pub fn new(
        family_id: EventFamilyId,
        source: DiscoverySourceAnchor,
        discovered_at: DateTime<Utc>,
    ) -> Self {
        Self {
            family_id,
            source,
            discovered_at,
            backfill_cursor: None,
            backfill_completed_at: None,
        }
    }

    pub fn with_backfill_cursor(mut self, cursor: impl Into<String>) -> Self {
        self.backfill_cursor = Some(cursor.into());
        self
    }

    pub fn record_backfill(
        &mut self,
        cursor: impl Into<String>,
        completed_at: Option<DateTime<Utc>>,
    ) {
        self.backfill_cursor = Some(cursor.into());
        if completed_at.is_some() {
            self.backfill_completed_at = completed_at;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidatePolicyAnchor {
    pub policy_name: String,
    pub policy_version: String,
}

impl CandidatePolicyAnchor {
    pub fn new(policy_name: impl Into<String>, policy_version: impl Into<String>) -> Self {
        Self {
            policy_name: policy_name.into(),
            policy_version: policy_version.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateValidationResult {
    Adoptable,
    Deferred { reason: String },
    Rejected { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateTarget {
    pub target_id: String,
    pub family_id: EventFamilyId,
    pub validation: CandidateValidationResult,
}

impl CandidateTarget {
    pub fn new(
        target_id: impl Into<String>,
        family_id: EventFamilyId,
        validation: CandidateValidationResult,
    ) -> Self {
        Self {
            target_id: target_id.into(),
            family_id,
            validation,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdoptableTargetRevision {
    pub revision_id: String,
    pub source_snapshot_id: String,
    pub policy_version: String,
}

impl AdoptableTargetRevision {
    pub fn new(
        revision_id: impl Into<String>,
        source_snapshot_id: impl Into<String>,
        policy_version: impl Into<String>,
    ) -> Self {
        Self {
            revision_id: revision_id.into(),
            source_snapshot_id: source_snapshot_id.into(),
            policy_version: policy_version.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateTargetSet {
    pub target_set_id: String,
    pub source_snapshot_id: String,
    pub discovery_record: FamilyDiscoveryRecord,
    pub policy: CandidatePolicyAnchor,
    pub targets: Vec<CandidateTarget>,
    pub adoptable_revision: Option<AdoptableTargetRevision>,
}

impl CandidateTargetSet {
    pub fn new(
        target_set_id: impl Into<String>,
        source_snapshot_id: impl Into<String>,
        discovery_record: FamilyDiscoveryRecord,
        policy: CandidatePolicyAnchor,
        targets: Vec<CandidateTarget>,
    ) -> Self {
        Self {
            target_set_id: target_set_id.into(),
            source_snapshot_id: source_snapshot_id.into(),
            discovery_record,
            policy,
            targets,
            adoptable_revision: None,
        }
    }

    pub fn with_adoptable_revision(mut self, revision: AdoptableTargetRevision) -> Self {
        self.adoptable_revision = Some(revision);
        self
    }
}
