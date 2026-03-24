use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalFactEvent {
    pub source_kind: String,
    pub source_session_id: String,
    pub source_event_id: String,
    pub normalizer_version: String,
    pub observed_at: DateTime<Utc>,
    pub raw_payload_hash: Option<String>,
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
        }
    }
}
