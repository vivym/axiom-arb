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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateFactInput {
    event: ExternalFactEvent,
    hint: FactApplyHint,
}

impl StateFactInput {
    pub fn new(event: ExternalFactEvent) -> Self {
        Self {
            event,
            hint: FactApplyHint::None,
        }
    }

    pub fn out_of_order_user_trade(event: ExternalFactEvent) -> Self {
        let fact_key = FactKey::from_event(&event);

        Self {
            event,
            hint: FactApplyHint::ReconcileRequired {
                pending_ref: PendingRef::from_fact_key(&fact_key),
                reason: "user trade arrived out of authoritative order",
            },
        }
    }

    pub(crate) fn fact_key(&self) -> FactKey {
        FactKey::from_event(&self.event)
    }

    pub(crate) fn apply_hint(&self) -> &FactApplyHint {
        &self.hint
    }
}

impl From<ExternalFactEvent> for StateFactInput {
    fn from(event: ExternalFactEvent) -> Self {
        Self::new(event)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FactApplyHint {
    None,
    ReconcileRequired {
        pending_ref: PendingRef,
        reason: &'static str,
    },
}
