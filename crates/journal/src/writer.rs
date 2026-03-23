use std::sync::{
    atomic::{AtomicI64, Ordering},
    Arc, Mutex,
};

use chrono::{DateTime, Utc};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    WsMarket,
    WsUser,
    RestHeartbeat,
    Internal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JournalEvent {
    pub stream: String,
    pub source_kind: SourceKind,
    pub source_session_id: String,
    pub source_event_id: Option<String>,
    pub dedupe_key: String,
    pub causal_parent_id: Option<String>,
    pub event_type: String,
    pub event_ts: DateTime<Utc>,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JournalEntry {
    pub journal_seq: i64,
    pub stream: String,
    pub source_kind: SourceKind,
    pub source_session_id: String,
    pub source_event_id: Option<String>,
    pub dedupe_key: String,
    pub causal_parent_id: Option<String>,
    pub event_type: String,
    pub event_ts: DateTime<Utc>,
    pub payload: Value,
    pub ingested_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalError {
    Poisoned,
}

impl std::fmt::Display for JournalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Poisoned => write!(f, "journal writer state poisoned"),
        }
    }
}

impl std::error::Error for JournalError {}

#[derive(Debug, Clone)]
pub struct JournalWriter {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    next_seq: AtomicI64,
    entries: Mutex<Vec<JournalEntry>>,
}

impl JournalWriter {
    pub fn in_memory() -> Self {
        Self {
            inner: Arc::new(Inner {
                next_seq: AtomicI64::new(1),
                entries: Mutex::new(Vec::new()),
            }),
        }
    }

    pub async fn append(&self, event: JournalEvent) -> Result<JournalEntry, JournalError> {
        let journal_seq = self.inner.next_seq.fetch_add(1, Ordering::SeqCst);
        let entry = JournalEntry {
            journal_seq,
            stream: event.stream,
            source_kind: event.source_kind,
            source_session_id: event.source_session_id,
            source_event_id: event.source_event_id,
            dedupe_key: event.dedupe_key,
            causal_parent_id: event.causal_parent_id,
            event_type: event.event_type,
            event_ts: event.event_ts,
            payload: event.payload,
            ingested_at: Utc::now(),
        };

        let mut entries = self.inner.entries.lock().map_err(|_| JournalError::Poisoned)?;
        entries.push(entry.clone());

        Ok(entry)
    }

    pub fn entries(&self) -> Result<Vec<JournalEntry>, JournalError> {
        let entries = self.inner.entries.lock().map_err(|_| JournalError::Poisoned)?;
        Ok(entries.clone())
    }
}
