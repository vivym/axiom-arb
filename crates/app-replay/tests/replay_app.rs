use std::{
    borrow::Cow,
    panic::AssertUnwindSafe,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use chrono::Utc;
use futures_util::future::FutureExt;
use journal::{JournalEntry, SourceKind};
use persistence::{
    models::{ExecutionAttemptRow, JournalEntryInput, LiveExecutionArtifactRow},
    run_migrations, ExecutionAttemptRepo, JournalRepo,
};
use sqlx::migrate::{Migration, MigrationType, Migrator};
use sqlx::{postgres::PgPoolOptions, PgPool};

use app_replay::{
    load_negrisk_live_attempt_artifacts, parse_args, replay_event_journal_from_pool,
    replay_from_source, replay_journal, NegRiskLiveAttemptArtifacts, ReplayConsumer, ReplayRange,
    ReplaySource, SummaryReplayConsumer,
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

#[test]
fn summary_consumer_tracks_phase3c_live_submit_closure_events() {
    let mut consumer = SummaryReplayConsumer::default();

    replay_journal(
        vec![
            sample_entry_with_kind(
                3,
                "event-3",
                "neg-risk-live",
                SourceKind::Internal,
                "reconcile_confirmed_authoritative",
            ),
            sample_entry_with_kind(
                1,
                "event-1",
                "neg-risk-live",
                SourceKind::Internal,
                "live_submission_recorded",
            ),
            sample_entry_with_kind(
                2,
                "event-2",
                "neg-risk-live",
                SourceKind::Internal,
                "pending_reconcile_created",
            ),
        ],
        &mut consumer,
    )
    .unwrap();

    let summary = consumer.summary();
    assert_eq!(summary.processed_count, 3);
    assert_eq!(summary.last_journal_seq, Some(3));
    assert_eq!(summary.per_stream.get("neg-risk-live"), Some(&3));
    assert_eq!(summary.per_source_kind.get("internal"), Some(&3));
    assert_eq!(
        summary.per_event_type.get("live_submission_recorded"),
        Some(&1)
    );
    assert_eq!(
        summary.per_event_type.get("pending_reconcile_created"),
        Some(&1)
    );
    assert_eq!(
        summary
            .per_event_type
            .get("reconcile_confirmed_authoritative"),
        Some(&1)
    );
}

#[tokio::test]
async fn replay_event_journal_from_pool_materializes_summary_from_db_rows() {
    if database_url().is_none() {
        return;
    }

    with_test_database(|db| async move {
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
    })
    .await;
}

#[tokio::test]
async fn live_attempt_artifact_loader_returns_empty_when_no_live_attempts_exist() {
    if database_url().is_none() {
        return;
    }

    with_test_database(|db| async move {
        run_migrations(&db.pool).await.unwrap();
        seed_journal(&db.pool).await;

        let rows = load_negrisk_live_attempt_artifacts(&db.pool).await.unwrap();
        assert!(rows.is_empty());
    })
    .await;
}

#[tokio::test]
async fn live_attempt_artifact_loader_ignores_non_negrisk_routes_even_with_negrisk_plan_ids() {
    if database_url().is_none() {
        return;
    }

    with_test_database(|db| async move {
        run_migrations(&db.pool).await.unwrap();
        ExecutionAttemptRepo
            .append(
                &db.pool,
                &sample_attempt(
                    "attempt-fullset-live-1",
                    "request-bound:5:req-1:negrisk-submit-family:family-a",
                    "fullset",
                ),
            )
            .await
            .unwrap();

        let rows = load_negrisk_live_attempt_artifacts(&db.pool).await.unwrap();
        assert!(rows.is_empty());
    })
    .await;
}

#[tokio::test]
async fn live_attempt_artifact_loader_includes_backfilled_legacy_negrisk_attempts() {
    if database_url().is_none() {
        return;
    }

    with_test_database(|db| async move {
        create_legacy_live_schema(&db.pool).await;

        let attempt_id = "attempt-legacy-live-1";
        let plan_id =
            "request-bound:5:req-1:negrisk-submit-family:family-a:condition-1:token-1:0.4:5";

        sqlx::query(
            r#"
            INSERT INTO execution_attempts (
                attempt_id,
                plan_id,
                snapshot_id,
                execution_mode,
                attempt_no,
                idempotency_key
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(attempt_id)
        .bind(plan_id)
        .bind("snapshot-7")
        .bind("live")
        .bind(1_i32)
        .bind("idem-legacy-live-1")
        .execute(&db.pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            INSERT INTO live_execution_artifacts (attempt_id, stream, payload)
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(attempt_id)
        .bind("negrisk.live")
        .bind(serde_json::json!({
            "attempt_id": attempt_id,
            "kind": "planned_order",
        }))
        .execute(&db.pool)
        .await
        .unwrap();

        apply_audit_anchor_migration(&db.pool).await;

        let rows = load_negrisk_live_attempt_artifacts(&db.pool).await.unwrap();
        assert_eq!(
            rows,
            vec![NegRiskLiveAttemptArtifacts {
                attempt: ExecutionAttemptRow {
                    attempt_id: attempt_id.to_owned(),
                    plan_id: plan_id.to_owned(),
                    snapshot_id: "snapshot-7".to_owned(),
                    route: "neg-risk".to_owned(),
                    scope: "family-a".to_owned(),
                    matched_rule_id: None,
                    execution_mode: domain::ExecutionMode::Live,
                    attempt_no: 1,
                    idempotency_key: "idem-legacy-live-1".to_owned(),
                },
                artifacts: vec![LiveExecutionArtifactRow {
                    attempt_id: attempt_id.to_owned(),
                    stream: "negrisk.live".to_owned(),
                    payload: serde_json::json!({
                        "attempt_id": attempt_id,
                        "kind": "planned_order",
                    }),
                }],
            }]
        );
    })
    .await;
}

#[tokio::test]
async fn with_test_database_cleans_up_schema_after_panicking_body() {
    let Some(database_url) = database_url() else {
        return;
    };

    let schema_name = Arc::new(Mutex::new(None::<String>));
    let captured = Arc::clone(&schema_name);

    let panic = AssertUnwindSafe(with_test_database(move |db| {
        let captured = Arc::clone(&captured);
        async move {
            *captured.lock().unwrap() = Some(db.schema().to_owned());
            panic!("intentional panic to verify cleanup");
        }
    }))
    .catch_unwind()
    .await;

    assert!(panic.is_err());

    let admin_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await
        .unwrap();
    let schema = schema_name.lock().unwrap().clone().unwrap();
    assert!(!schema_exists(&admin_pool, &schema).await);
    admin_pool.close().await;
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

async fn create_legacy_live_schema(pool: &PgPool) {
    sqlx::query(
        r#"
        CREATE TABLE execution_attempts (
            attempt_id TEXT PRIMARY KEY,
            plan_id TEXT NOT NULL,
            snapshot_id TEXT NOT NULL,
            execution_mode TEXT NOT NULL CHECK (
                execution_mode IN ('disabled', 'shadow', 'live', 'reduce_only', 'recovery_only')
            ),
            attempt_no INTEGER NOT NULL,
            idempotency_key TEXT NOT NULL,
            outcome TEXT,
            payload JSONB NOT NULL DEFAULT '{}'::JSONB,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
        "#,
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        CREATE TABLE live_execution_artifacts (
            attempt_id TEXT NOT NULL REFERENCES execution_attempts (attempt_id),
            stream TEXT NOT NULL,
            payload JSONB NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            PRIMARY KEY (attempt_id, stream)
        )
        "#,
    )
    .execute(pool)
    .await
    .unwrap();
}

fn audit_anchor_migrator() -> Migrator {
    Migrator {
        migrations: Cow::Owned(vec![Migration::new(
            8,
            Cow::Borrowed("execution_attempt_audit_anchor"),
            MigrationType::Simple,
            Cow::Borrowed(include_str!(
                "../../../migrations/0008_execution_attempt_audit_anchor.sql"
            )),
            false,
        )]),
        ignore_missing: false,
        locking: true,
        no_tx: false,
    }
}

async fn apply_audit_anchor_migration(pool: &PgPool) {
    audit_anchor_migrator().run(pool).await.unwrap();
}

#[derive(Clone)]
struct TestDatabase {
    admin_pool: PgPool,
    pool: PgPool,
    schema: String,
}

impl TestDatabase {
    async fn new(database_url: &str) -> Self {
        let admin_pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(database_url)
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
            .connect(database_url)
            .await
            .expect("isolated test pool should connect");

        Self {
            admin_pool,
            pool,
            schema,
        }
    }

    fn schema(&self) -> &str {
        &self.schema
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

fn database_url() -> Option<String> {
    std::env::var("DATABASE_URL").ok()
}

fn sample_attempt(attempt_id: &str, plan_id: &str, route: &str) -> ExecutionAttemptRow {
    ExecutionAttemptRow {
        attempt_id: attempt_id.to_owned(),
        plan_id: plan_id.to_owned(),
        snapshot_id: "snapshot-7".to_owned(),
        route: route.to_owned(),
        scope: "family:family-a".to_owned(),
        matched_rule_id: Some("rule-replay-loader".to_owned()),
        execution_mode: domain::ExecutionMode::Live,
        attempt_no: 1,
        idempotency_key: format!("idem-{attempt_id}"),
    }
}

async fn with_test_database<F, Fut>(test: F)
where
    F: FnOnce(TestDatabase) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let database_url = database_url().expect("DATABASE_URL must be set for app-replay tests");
    let db = TestDatabase::new(&database_url).await;
    let result = AssertUnwindSafe(test(db.clone())).catch_unwind().await;
    db.cleanup().await;

    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

async fn schema_exists(pool: &PgPool, schema: &str) -> bool {
    let row: (bool,) = sqlx::query_as(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM information_schema.schemata
            WHERE schema_name = $1
        )
        "#,
    )
    .bind(schema)
    .fetch_one(pool)
    .await
    .unwrap();

    row.0
}
