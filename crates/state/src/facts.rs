use std::collections::BTreeSet;

use domain::{ExternalFactEvent, ExternalFactPayloadData, RuntimeAttentionObservedPayload};

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
pub struct PendingReconcileAnchor {
    pub pending_ref: String,
    pub submission_ref: String,
    pub family_id: String,
    pub route: String,
    pub reason: String,
}

impl PendingReconcileAnchor {
    pub fn new(
        pending_ref: impl Into<String>,
        submission_ref: impl Into<String>,
        family_id: impl Into<String>,
        route: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            pending_ref: pending_ref.into(),
            submission_ref: submission_ref.into(),
            family_id: family_id.into(),
            route: route.into(),
            reason: reason.into(),
        }
    }

    fn from_pending_ref_and_reason(
        pending_ref: PendingRef,
        family_id: impl Into<String>,
        route: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        let pending_ref_value = pending_ref.0;
        Self::new(
            pending_ref_value.clone(),
            pending_ref_value,
            family_id,
            route,
            reason,
        )
    }

    fn from_live_submit(
        fact_key: &FactKey,
        payload: &domain::NegRiskLiveSubmitObservedPayload,
    ) -> Self {
        Self::new(
            PendingRef::from_fact_key(fact_key).0,
            payload.submission_ref.clone(),
            payload.scope.clone(),
            fact_key.source_kind.clone(),
            "live submit observed",
        )
    }

    fn from_runtime_attention(
        pending_ref: PendingRef,
        payload: &RuntimeAttentionObservedPayload,
    ) -> Self {
        Self::from_pending_ref_and_reason(
            pending_ref,
            payload.scope_id.clone(),
            payload.source.clone(),
            payload.reason.clone(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeAttentionAnchor {
    pub attention_ref: String,
    pub source: String,
    pub scope_id: String,
    pub attention_kind: String,
    pub reason: String,
}

impl RuntimeAttentionAnchor {
    fn from_fact_key_and_payload(
        fact_key: &FactKey,
        payload: &RuntimeAttentionObservedPayload,
    ) -> Self {
        Self {
            attention_ref: format!(
                "attention:{}:{}:{}:{}",
                fact_key.source_kind,
                fact_key.source_session_id,
                fact_key.source_event_id,
                fact_key.normalizer_version
            ),
            source: payload.source.clone(),
            scope_id: payload.scope_id.clone(),
            attention_kind: payload.attention_kind.clone(),
            reason: payload.reason.clone(),
        }
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
        let fact_key = FactKey::from_event(&event);
        let hint = match event.payload.as_ref() {
            Some(ExternalFactPayloadData::NegRiskLiveSubmitObserved(payload)) => {
                FactApplyHint::PendingReconcile {
                    anchor: PendingReconcileAnchor::from_live_submit(&fact_key, payload),
                }
            }
            Some(ExternalFactPayloadData::NegRiskLiveReconcileObserved(payload)) => {
                FactApplyHint::LiveReconcileObserved {
                    pending_ref: PendingRef(payload.pending_ref.clone()),
                    terminal: payload.terminal,
                }
            }
            Some(ExternalFactPayloadData::RuntimeAttentionObserved(payload)) => {
                if payload.attention_kind == "metadata_stale" {
                    FactApplyHint::RuntimeAttention {
                        anchor: RuntimeAttentionAnchor::from_fact_key_and_payload(
                            &fact_key, payload,
                        ),
                    }
                } else {
                    FactApplyHint::PendingReconcile {
                        anchor: PendingReconcileAnchor::from_runtime_attention(
                            PendingRef::from_fact_key(&fact_key),
                            payload,
                        ),
                    }
                }
            }
            None => FactApplyHint::None,
        };

        Self { event, hint }
    }

    pub fn out_of_order_user_trade(event: ExternalFactEvent) -> Self {
        let fact_key = FactKey::from_event(&event);

        Self {
            event,
            hint: FactApplyHint::PendingReconcile {
                anchor: PendingReconcileAnchor::from_pending_ref_and_reason(
                    PendingRef::from_fact_key(&fact_key),
                    fact_key.source_session_id.clone(),
                    fact_key.source_kind.clone(),
                    "user trade arrived out of authoritative order",
                ),
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
    PendingReconcile {
        anchor: PendingReconcileAnchor,
    },
    RuntimeAttention {
        anchor: RuntimeAttentionAnchor,
    },
    LiveReconcileObserved {
        pending_ref: PendingRef,
        terminal: bool,
    },
}
