use chrono::Utc;
use journal::{JournalEntry, SourceKind};

use app_replay::{
    parse_args, replay_from_source, replay_journal, ReplayConsumer, ReplayRange, ReplaySource,
};

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

#[test]
fn replay_from_source_uses_requested_range_before_consuming() {
    let source = InMemoryReplaySource::new(vec![
        sample_entry(1, "event-1"),
        sample_entry(2, "event-2"),
        sample_entry(3, "event-3"),
    ]);
    let mut consumer = RecordingConsumer::default();

    replay_from_source(&source, ReplayRange::new(1, Some(1)), &mut consumer).unwrap();

    assert_eq!(
        source.requested_ranges(),
        vec![ReplayRange::new(1, Some(1))]
    );
    assert_eq!(consumer.seen, vec![2]);
}

#[test]
fn parse_args_reads_from_seq_and_optional_limit() {
    let range = parse_args(["app-replay", "--from-seq", "42", "--limit", "5"]).unwrap();

    assert_eq!(range, ReplayRange::new(42, Some(5)));
}

#[derive(Default)]
struct InMemoryReplaySource {
    entries: Vec<JournalEntry>,
    requested: std::cell::RefCell<Vec<ReplayRange>>,
}

impl InMemoryReplaySource {
    fn new(entries: Vec<JournalEntry>) -> Self {
        Self {
            entries,
            requested: std::cell::RefCell::new(Vec::new()),
        }
    }

    fn requested_ranges(&self) -> Vec<ReplayRange> {
        self.requested.borrow().clone()
    }
}

impl ReplaySource for InMemoryReplaySource {
    type Error = std::convert::Infallible;

    fn list(&self, range: ReplayRange) -> Result<Vec<JournalEntry>, Self::Error> {
        self.requested.borrow_mut().push(range);
        Ok(self
            .entries
            .iter()
            .filter(|entry| entry.journal_seq > range.after_seq)
            .take(range.limit.unwrap_or(usize::MAX as i64) as usize)
            .cloned()
            .collect())
    }
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
