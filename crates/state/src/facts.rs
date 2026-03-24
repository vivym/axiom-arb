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

impl PendingRef {
    pub(crate) fn from_fact_key(fact_key: &FactKey) -> Self {
        Self(format!(
            "pending:{}:{}:{}:{}",
            fact_key.source_kind,
            fact_key.source_session_id,
            fact_key.source_event_id,
            fact_key.normalizer_version
        ))
    }
}

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

pub(crate) enum FactApplyHint {
    None,
    ReconcileRequired {
        pending_ref: PendingRef,
        reason: &'static str,
    },
}

pub(crate) fn classify_fact_for_apply(fact_key: &FactKey) -> FactApplyHint {
    if fact_key.source_kind == "user_trade_out_of_order" {
        return FactApplyHint::ReconcileRequired {
            pending_ref: PendingRef::from_fact_key(fact_key),
            reason: "user trade arrived out of authoritative order",
        };
    }

    FactApplyHint::None
}
