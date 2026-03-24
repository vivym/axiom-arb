use std::sync::atomic::{AtomicU64, Ordering};

use chrono::Utc;
use journal::{JournalEntry, SourceKind};
use persistence::{models::JournalEntryInput, run_migrations, JournalRepo};
use sqlx::{postgres::PgPoolOptions, PgPool};

use app_replay::{
    parse_args, replay_event_journal_from_pool, replay_from_source, replay_journal, ReplayConsumer,
    ReplayRange, ReplaySource, SummaryReplayConsumer,
};

static NEXT_SCHEMA_ID: AtomicU64 = AtomicU64::new(1);

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

#[test]
fn summary_consumer_materializes_deterministic_replay_state() {
    let mut consumer = SummaryReplayConsumer::default();

    replay_journal(
        vec![
            sample_entry_with_kind(3, "event-3", "orders", SourceKind::WsUser, "trade"),
            sample_entry_with_kind(
                1,
                "event-1",
                "markets",
                SourceKind::WsMarket,
                "orderbook_snapshot",
            ),
            sample_entry_with_kind(2, "event-2", "runtime", SourceKind::Internal, "heartbeat"),
        ],
        &mut consumer,
    )
    .unwrap();

    let summary = consumer.summary();
    assert_eq!(summary.processed_count, 3);
    assert_eq!(summary.last_journal_seq, Some(3));
    assert_eq!(summary.per_stream.get("markets"), Some(&1));
    assert_eq!(summary.per_stream.get("orders"), Some(&1));
    assert_eq!(summary.per_stream.get("runtime"), Some(&1));
    assert_eq!(summary.per_source_kind.get("ws_market"), Some(&1));
    assert_eq!(summary.per_source_kind.get("ws_user"), Some(&1));
    assert_eq!(summary.per_source_kind.get("internal"), Some(&1));
    assert_eq!(summary.per_event_type.get("orderbook_snapshot"), Some(&1));
    assert_eq!(summary.per_event_type.get("trade"), Some(&1));
    assert_eq!(summary.per_event_type.get("heartbeat"), Some(&1));
}

#[tokio::test]
async fn replay_event_journal_from_pool_materializes_summary_from_db_rows() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();
    seed_journal(&db.pool).await;
    let mut consumer = SummaryReplayConsumer::default();

    replay_event_journal_from_pool(&db.pool, ReplayRange::new(1, Some(2)), &mut consumer)
        .await
        .unwrap();

    let summary = consumer.summary();
    assert_eq!(summary.processed_count, 2);
    assert_eq!(summary.last_journal_seq, Some(3));
    assert_eq!(summary.per_stream.get("orders"), Some(&1));
    assert_eq!(summary.per_stream.get("runtime"), Some(&1));
    assert_eq!(summary.per_stream.get("markets"), None);
    assert_eq!(summary.per_source_kind.get("ws_user"), Some(&1));
    assert_eq!(summary.per_source_kind.get("internal"), Some(&1));
    assert_eq!(summary.per_event_type.get("trade"), Some(&1));
    assert_eq!(summary.per_event_type.get("heartbeat"), Some(&1));

    db.cleanup().await;
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
    sample_entry_with_kind(
        journal_seq,
        event_id,
        "journal",
        SourceKind::Internal,
        "event",
    )
}

fn sample_entry_with_kind(
    journal_seq: i64,
    event_id: &str,
    stream: &str,
    source_kind: SourceKind,
    event_type: &str,
) -> JournalEntry {
    JournalEntry {
        journal_seq,
        stream: stream.to_owned(),
        source_kind,
        source_session_id: "replay-session".to_owned(),
        source_event_id: event_id.to_owned(),
        dedupe_key: format!("journal:{event_id}"),
        causal_parent_id: None,
        event_type: event_type.to_owned(),
        event_ts: Utc::now(),
        payload: serde_json::json!({ "event_id": event_id }),
        ingested_at: Utc::now(),
    }
}

struct TestDatabase {
    admin_pool: PgPool,
    pool: PgPool,
    schema: String,
}

impl TestDatabase {
    async fn new() -> Self {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for app-replay tests");

        let admin_pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("test database should connect");

        let schema = format!(
            "app_replay_test_{}_{}",
            std::process::id(),
            NEXT_SCHEMA_ID.fetch_add(1, Ordering::Relaxed)
        );
        let create_schema = format!(r#"CREATE SCHEMA "{schema}""#);

        sqlx::query(&create_schema)
            .execute(&admin_pool)
            .await
            .expect("test schema should create");

        let search_path_sql = format!(r#"SET search_path TO "{schema}""#);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .after_connect(move |conn, _meta| {
                let search_path_sql = search_path_sql.clone();
                Box::pin(async move {
                    sqlx::query(&search_path_sql).execute(conn).await?;
                    Ok(())
                })
            })
            .connect(&database_url)
            .await
            .expect("isolated test pool should connect");

        Self {
            admin_pool,
            pool,
            schema,
        }
    }

    async fn cleanup(self) {
        self.pool.close().await;

        let drop_schema = format!(
            r#"DROP SCHEMA IF EXISTS "{schema}" CASCADE"#,
            schema = self.schema
        );
        sqlx::query(&drop_schema)
            .execute(&self.admin_pool)
            .await
            .expect("test schema should drop");

        self.admin_pool.close().await;
    }
}

async fn seed_journal(pool: &PgPool) {
    let repo = JournalRepo;

    repo.append(
        pool,
        &JournalEntryInput {
            stream: "markets".to_owned(),
            source_kind: "ws_market".to_owned(),
            source_session_id: "market-session-1".to_owned(),
            source_event_id: "book-1".to_owned(),
            dedupe_key: "markets:book-1".to_owned(),
            causal_parent_id: None,
            event_type: "orderbook_snapshot".to_owned(),
            event_ts: Utc::now(),
            payload: serde_json::json!({ "event_id": "book-1" }),
        },
    )
    .await
    .unwrap();

    repo.append(
        pool,
        &JournalEntryInput {
            stream: "orders".to_owned(),
            source_kind: "ws_user".to_owned(),
            source_session_id: "user-session-1".to_owned(),
            source_event_id: "trade-1".to_owned(),
            dedupe_key: "orders:trade-1".to_owned(),
            causal_parent_id: Some(1),
            event_type: "trade".to_owned(),
            event_ts: Utc::now(),
            payload: serde_json::json!({ "event_id": "trade-1" }),
        },
    )
    .await
    .unwrap();

    repo.append(
        pool,
        &JournalEntryInput {
            stream: "runtime".to_owned(),
            source_kind: "internal".to_owned(),
            source_session_id: "runtime-session-1".to_owned(),
            source_event_id: "heartbeat-1".to_owned(),
            dedupe_key: "runtime:heartbeat-1".to_owned(),
            causal_parent_id: None,
            event_type: "heartbeat".to_owned(),
            event_ts: Utc::now(),
            payload: serde_json::json!({ "event_id": "heartbeat-1" }),
        },
    )
    .await
    .unwrap();
}
