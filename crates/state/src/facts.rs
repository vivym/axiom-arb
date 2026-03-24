use std::collections::BTreeSet;

use domain::ExternalFactEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DirtyDomain {
    Runtime,
    Orders,
    Inventory,
    Approvals,
    Resolution,
    Relayer,
    NegRiskFamilies,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRef(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirtySet {
    pub domains: BTreeSet<DirtyDomain>,
}

impl DirtySet {
    pub fn new(domains: impl IntoIterator<Item = DirtyDomain>) -> Self {
        Self {
            domains: domains.into_iter().collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FactKey {
    pub source_kind: String,
    pub source_session_id: String,
    pub source_event_id: String,
    pub normalizer_version: String,
}

impl FactKey {
    pub(crate) fn from_event(event: &ExternalFactEvent) -> Self {
        Self {
            source_kind: event.source_kind.clone(),
            source_session_id: event.source_session_id.clone(),
            source_event_id: event.source_event_id.clone(),
            normalizer_version: event.normalizer_version.clone(),
        }
    }
}
