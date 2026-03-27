use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExternalFactPayload(Option<ExternalFactPayloadData>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalFactPayloadData {
    NegRiskLiveSubmitObserved(NegRiskLiveSubmitObservedPayload),
    NegRiskLiveReconcileObserved(NegRiskLiveReconcileObservedPayload),
    RuntimeAttentionObserved(RuntimeAttentionObservedPayload),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegRiskLiveSubmitObservedPayload {
    pub attempt_id: String,
    pub scope: String,
    pub submission_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegRiskLiveReconcileObservedPayload {
    pub pending_ref: String,
    pub scope: String,
    pub terminal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeAttentionObservedPayload {
    pub source: String,
    pub scope_id: String,
    pub attention_kind: String,
    pub reason: String,
}

impl ExternalFactPayload {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn negrisk_live_submit_observed(
        attempt_id: impl Into<String>,
        scope: impl Into<String>,
        submission_ref: impl Into<String>,
    ) -> Self {
        Self(Some(ExternalFactPayloadData::NegRiskLiveSubmitObserved(
            NegRiskLiveSubmitObservedPayload {
                attempt_id: attempt_id.into(),
                scope: scope.into(),
                submission_ref: submission_ref.into(),
            },
        )))
    }

    pub fn negrisk_live_reconcile_observed(
        pending_ref: impl Into<String>,
        scope: impl Into<String>,
        terminal: bool,
    ) -> Self {
        Self(Some(ExternalFactPayloadData::NegRiskLiveReconcileObserved(
            NegRiskLiveReconcileObservedPayload {
                pending_ref: pending_ref.into(),
                scope: scope.into(),
                terminal,
            },
        )))
    }

    pub fn runtime_attention_observed(
        source: impl Into<String>,
        scope_id: impl Into<String>,
        attention_kind: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self(Some(ExternalFactPayloadData::RuntimeAttentionObserved(
            RuntimeAttentionObservedPayload {
                source: source.into(),
                scope_id: scope_id.into(),
                attention_kind: attention_kind.into(),
                reason: reason.into(),
            },
        )))
    }

    pub fn as_ref(&self) -> Option<&ExternalFactPayloadData> {
        self.0.as_ref()
    }

    pub fn kind(&self) -> &'static str {
        match &self.0 {
            None => "none",
            Some(ExternalFactPayloadData::NegRiskLiveSubmitObserved(_)) => {
                "negrisk_live_submit_observed"
            }
            Some(ExternalFactPayloadData::NegRiskLiveReconcileObserved(_)) => {
                "negrisk_live_reconcile_observed"
            }
            Some(ExternalFactPayloadData::RuntimeAttentionObserved(_)) => {
                "runtime_attention_observed"
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalFactEvent {
    pub source_kind: String,
    pub source_session_id: String,
    pub source_event_id: String,
    pub normalizer_version: String,
    pub observed_at: DateTime<Utc>,
    pub raw_payload_hash: Option<String>,
    pub payload: ExternalFactPayload,
}

impl ExternalFactEvent {
    pub fn new(
        source_kind: impl Into<String>,
        source_session_id: impl Into<String>,
        source_event_id: impl Into<String>,
        normalizer_version: impl Into<String>,
        observed_at: DateTime<Utc>,
    ) -> Self {
        Self {
            source_kind: source_kind.into(),
            source_session_id: source_session_id.into(),
            source_event_id: source_event_id.into(),
            normalizer_version: normalizer_version.into(),
            observed_at,
            raw_payload_hash: None,
            payload: ExternalFactPayload::none(),
        }
    }

    pub fn negrisk_live_submit_observed(
        source_session_id: impl Into<String>,
        source_event_id: impl Into<String>,
        attempt_id: impl Into<String>,
        scope: impl Into<String>,
        submission_ref: impl Into<String>,
        observed_at: DateTime<Utc>,
    ) -> Self {
        Self {
            source_kind: "negrisk_live_submit".to_owned(),
            source_session_id: source_session_id.into(),
            source_event_id: source_event_id.into(),
            normalizer_version: "v1-negrisk-live-submit".to_owned(),
            observed_at,
            raw_payload_hash: None,
            payload: ExternalFactPayload::negrisk_live_submit_observed(
                attempt_id,
                scope,
                submission_ref,
            ),
        }
    }

    pub fn negrisk_live_reconcile_observed(
        source_session_id: impl Into<String>,
        source_event_id: impl Into<String>,
        pending_ref: impl Into<String>,
        scope: impl Into<String>,
        terminal: bool,
        observed_at: DateTime<Utc>,
    ) -> Self {
        Self {
            source_kind: "negrisk_live_reconcile".to_owned(),
            source_session_id: source_session_id.into(),
            source_event_id: source_event_id.into(),
            normalizer_version: "v1-negrisk-live-reconcile".to_owned(),
            observed_at,
            raw_payload_hash: None,
            payload: ExternalFactPayload::negrisk_live_reconcile_observed(
                pending_ref,
                scope,
                terminal,
            ),
        }
    }

    pub fn runtime_attention_observed(
        source: impl Into<String>,
        source_session_id: impl Into<String>,
        source_event_id: impl Into<String>,
        scope_id: impl Into<String>,
        attention_kind: impl Into<String>,
        reason: impl Into<String>,
        observed_at: DateTime<Utc>,
    ) -> Self {
        let source = source.into();

        Self {
            source_kind: "runtime_attention".to_owned(),
            source_session_id: source_session_id.into(),
            source_event_id: source_event_id.into(),
            normalizer_version: format!("v1-runtime-attention-{source}"),
            observed_at,
            raw_payload_hash: None,
            payload: ExternalFactPayload::runtime_attention_observed(
                source,
                scope_id,
                attention_kind,
                reason,
            ),
        }
    }
}
