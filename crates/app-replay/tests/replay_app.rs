use chrono::Utc;
use journal::{JournalEntry, SourceKind};

use app_replay::{replay_journal, ReplayConsumer};

#[derive(Default)]
struct RecordingConsumer {
    seen: Vec<i64>,
}

impl ReplayConsumer for RecordingConsumer {
    type Error = std::convert::Infallible;

    fn consume(&mut self, entry: JournalEntry) -> Result<(), Self::Error> {
        self.seen.push(entry.journal_seq);
        Ok(())
    }
}

#[test]
fn replay_entrypoint_consumes_entries_in_journal_sequence_order() {
    let mut consumer = RecordingConsumer::default();

    replay_journal(
        vec![
            sample_entry(3, "event-3"),
            sample_entry(1, "event-1"),
            sample_entry(2, "event-2"),
        ],
        &mut consumer,
    )
    .unwrap();

    assert_eq!(consumer.seen, vec![1, 2, 3]);
}

fn sample_entry(journal_seq: i64, event_id: &str) -> JournalEntry {
    JournalEntry {
        journal_seq,
        stream: "journal".to_owned(),
        source_kind: SourceKind::Internal,
        source_session_id: "replay-session".to_owned(),
        source_event_id: event_id.to_owned(),
        dedupe_key: format!("journal:{event_id}"),
        causal_parent_id: None,
        event_type: "event".to_owned(),
        event_ts: Utc::now(),
        payload: serde_json::json!({ "event_id": event_id }),
        ingested_at: Utc::now(),
    }
}
