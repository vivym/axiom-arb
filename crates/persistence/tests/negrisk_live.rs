use std::{
    borrow::Cow,
    sync::atomic::{AtomicU64, Ordering},
};

use domain::ExecutionMode;
use persistence::{
    models::{ExecutionAttemptRow, LiveExecutionArtifactRow, LiveSubmissionRecordRow},
    run_migrations, ExecutionAttemptRepo, LiveArtifactRepo, LiveSubmissionRepo, PersistenceError,
};
use serde_json::json;
use sqlx::migrate::{Migration, MigrationType, Migrator};
use sqlx::{postgres::PgPoolOptions, PgPool};

static NEXT_SCHEMA_ID: AtomicU64 = AtomicU64::new(1);

struct TestDatabase {
    admin_pool: PgPool,
    pool: PgPool,
    schema: String,
}

impl TestDatabase {
    async fn new() -> Self {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for persistence tests");

        let admin_pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("test database should connect");

        let schema = format!(
            "persistence_negrisk_live_test_{}_{}",
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

fn sample_attempt(attempt_id: &str, mode: ExecutionMode, plan_id: &str) -> ExecutionAttemptRow {
    ExecutionAttemptRow {
        attempt_id: attempt_id.to_owned(),
        plan_id: plan_id.to_owned(),
        snapshot_id: "snapshot-7".to_owned(),
        route: "neg-risk".to_owned(),
        scope: "family:family-a".to_owned(),
        matched_rule_id: Some("rule-neg-risk-submit".to_owned()),
        execution_mode: mode,
        attempt_no: 1,
        idempotency_key: format!("idem-{attempt_id}"),
    }
}

fn sample_live_artifact(attempt_id: &str) -> LiveExecutionArtifactRow {
    LiveExecutionArtifactRow {
        attempt_id: attempt_id.to_owned(),
        stream: "negrisk.live".to_owned(),
        payload: json!({
            "attempt_id": attempt_id,
            "kind": "planned_order",
        }),
    }
}

fn sample_live_submission_record(
    attempt_id: &str,
    submission_ref: &str,
) -> LiveSubmissionRecordRow {
    LiveSubmissionRecordRow {
        submission_ref: submission_ref.to_owned(),
        attempt_id: attempt_id.to_owned(),
        route: "neg-risk".to_owned(),
        scope: "family-a".to_owned(),
        provider: "venue-polymarket".to_owned(),
        state: "submitted".to_owned(),
        payload: json!({
            "submission_ref": submission_ref,
            "family_id": "family-a",
            "route": "neg-risk",
            "reason": "submitted_for_execution",
        }),
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

#[tokio::test]
async fn live_artifacts_round_trip_with_attempt_anchor() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    ExecutionAttemptRepo
        .append(
            &db.pool,
            &sample_attempt(
                "attempt-live-1",
                ExecutionMode::Live,
                "negrisk-submit-family:family-a:member-1",
            ),
        )
        .await
        .unwrap();

    LiveArtifactRepo
        .append(&db.pool, sample_live_artifact("attempt-live-1"))
        .await
        .unwrap();

    let rows = LiveArtifactRepo
        .list_for_attempt(&db.pool, "attempt-live-1")
        .await
        .unwrap();
    assert_eq!(rows, vec![sample_live_artifact("attempt-live-1")]);

    db.cleanup().await;
}

#[tokio::test]
async fn legacy_negrisk_live_attempts_are_backfilled_from_plan_id() {
    let db = TestDatabase::new().await;
    create_legacy_live_schema(&db.pool).await;

    let attempt_id = "attempt-legacy-live-1";
    let plan_id = "request-bound:5:req-1:negrisk-submit-family:family-a:member-1";

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

    LiveArtifactRepo
        .append(&db.pool, sample_live_artifact(attempt_id))
        .await
        .unwrap();

    apply_audit_anchor_migration(&db.pool).await;

    let rows = ExecutionAttemptRepo
        .list_live_attempts(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        rows,
        vec![ExecutionAttemptRow {
            attempt_id: attempt_id.to_owned(),
            plan_id: plan_id.to_owned(),
            snapshot_id: "snapshot-7".to_owned(),
            route: "neg-risk".to_owned(),
            scope: "family-a".to_owned(),
            matched_rule_id: None,
            execution_mode: ExecutionMode::Live,
            attempt_no: 1,
            idempotency_key: "idem-legacy-live-1".to_owned(),
        }]
    );

    let artifacts = LiveArtifactRepo
        .list_for_attempt(&db.pool, attempt_id)
        .await
        .unwrap();
    assert_eq!(artifacts, vec![sample_live_artifact(attempt_id)]);

    db.cleanup().await;
}

#[tokio::test]
async fn live_artifacts_append_is_idempotent_per_attempt_and_stream() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    ExecutionAttemptRepo
        .append(
            &db.pool,
            &sample_attempt(
                "attempt-live-idempotent-1",
                ExecutionMode::Live,
                "negrisk-submit-family:family-a:member-1",
            ),
        )
        .await
        .unwrap();

    let artifact = sample_live_artifact("attempt-live-idempotent-1");

    LiveArtifactRepo
        .append(&db.pool, artifact.clone())
        .await
        .unwrap();
    LiveArtifactRepo
        .append(&db.pool, artifact.clone())
        .await
        .unwrap();

    let rows = LiveArtifactRepo
        .list_for_attempt(&db.pool, "attempt-live-idempotent-1")
        .await
        .unwrap();
    assert_eq!(rows, vec![artifact]);

    db.cleanup().await;
}

#[tokio::test]
async fn live_artifacts_reject_conflicting_payload_for_same_attempt_and_stream() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    ExecutionAttemptRepo
        .append(
            &db.pool,
            &sample_attempt(
                "attempt-live-conflict-1",
                ExecutionMode::Live,
                "negrisk-submit-family:family-a:member-1",
            ),
        )
        .await
        .unwrap();

    let original = sample_live_artifact("attempt-live-conflict-1");
    let conflicting = LiveExecutionArtifactRow {
        attempt_id: "attempt-live-conflict-1".to_owned(),
        stream: "negrisk.live".to_owned(),
        payload: json!({
            "attempt_id": "attempt-live-conflict-1",
            "kind": "signed_order",
        }),
    };

    LiveArtifactRepo
        .append(&db.pool, original.clone())
        .await
        .unwrap();

    let err = LiveArtifactRepo
        .append(&db.pool, conflicting)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        PersistenceError::ConflictingLiveArtifactPayload {
            ref attempt_id,
            ref stream,
        }
        if attempt_id == "attempt-live-conflict-1" && stream == "negrisk.live"
    ));

    let rows = LiveArtifactRepo
        .list_for_attempt(&db.pool, "attempt-live-conflict-1")
        .await
        .unwrap();
    assert_eq!(rows, vec![original]);

    db.cleanup().await;
}

#[tokio::test]
async fn live_artifacts_support_bulk_listing_for_replay() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let attempt_a = sample_attempt(
        "attempt-live-bulk-a",
        ExecutionMode::Live,
        "negrisk-submit-family:family-a:member-1",
    );
    let attempt_b = sample_attempt(
        "attempt-live-bulk-b",
        ExecutionMode::Live,
        "negrisk-submit-family:family-b:member-1",
    );
    ExecutionAttemptRepo
        .append(&db.pool, &attempt_a)
        .await
        .unwrap();
    ExecutionAttemptRepo
        .append(&db.pool, &attempt_b)
        .await
        .unwrap();

    let artifact_a = sample_live_artifact("attempt-live-bulk-a");
    let artifact_b = LiveExecutionArtifactRow {
        attempt_id: "attempt-live-bulk-b".to_owned(),
        stream: "negrisk.audit".to_owned(),
        payload: json!({
            "attempt_id": "attempt-live-bulk-b",
            "kind": "audit_trail",
        }),
    };
    LiveArtifactRepo
        .append(&db.pool, artifact_a.clone())
        .await
        .unwrap();
    LiveArtifactRepo
        .append(&db.pool, artifact_b.clone())
        .await
        .unwrap();

    let rows = LiveArtifactRepo
        .list_for_attempts(
            &db.pool,
            &[
                "attempt-live-bulk-b".to_owned(),
                "attempt-live-bulk-a".to_owned(),
            ],
        )
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows["attempt-live-bulk-a"], vec![artifact_a]);
    assert_eq!(rows["attempt-live-bulk-b"], vec![artifact_b]);

    db.cleanup().await;
}

#[tokio::test]
async fn live_submission_records_round_trip_with_attempt_anchor() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    ExecutionAttemptRepo
        .append(
            &db.pool,
            &sample_attempt(
                "attempt-live-submit-1",
                ExecutionMode::Live,
                "negrisk-submit-family:family-a:member-1",
            ),
        )
        .await
        .unwrap();

    let row = sample_live_submission_record("attempt-live-submit-1", "submission-ref-1");
    LiveSubmissionRepo
        .append(&db.pool, row.clone())
        .await
        .unwrap();

    let rows = LiveSubmissionRepo
        .list_for_attempt(&db.pool, "attempt-live-submit-1")
        .await
        .unwrap();
    assert_eq!(rows, vec![row]);

    db.cleanup().await;
}

#[tokio::test]
async fn live_submission_records_carry_resume_truth_payload() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    ExecutionAttemptRepo
        .append(
            &db.pool,
            &sample_attempt(
                "attempt-live-submit-2",
                ExecutionMode::Live,
                "negrisk-submit-family:family-a:member-1",
            ),
        )
        .await
        .unwrap();

    let row = LiveSubmissionRecordRow {
        submission_ref: "submission-ref-2".to_owned(),
        attempt_id: "attempt-live-submit-2".to_owned(),
        route: "neg-risk".to_owned(),
        scope: "family-a".to_owned(),
        provider: "venue-polymarket".to_owned(),
        state: "pending_reconcile".to_owned(),
        payload: json!({
            "submission_ref": "submission-ref-2",
            "family_id": "family-a",
            "route": "neg-risk",
            "reason": "awaiting_resolve",
        }),
    };

    LiveSubmissionRepo
        .append(&db.pool, row.clone())
        .await
        .unwrap();

    let rows = LiveSubmissionRepo
        .list_for_attempts(&db.pool, &["attempt-live-submit-2".to_owned()])
        .await
        .unwrap();
    assert_eq!(rows["attempt-live-submit-2"], vec![row]);

    db.cleanup().await;
}

#[tokio::test]
async fn live_submission_records_reject_malformed_anchor_state() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    ExecutionAttemptRepo
        .append(
            &db.pool,
            &sample_attempt(
                "attempt-live-submit-bad-state",
                ExecutionMode::Live,
                "negrisk-submit-family:family-a:member-1",
            ),
        )
        .await
        .unwrap();

    let row = LiveSubmissionRecordRow {
        submission_ref: "submission-ref-bad-state".to_owned(),
        attempt_id: "attempt-live-submit-bad-state".to_owned(),
        route: "neg-risk".to_owned(),
        scope: "family-a".to_owned(),
        provider: "venue-polymarket".to_owned(),
        state: "in_review".to_owned(),
        payload: json!({
            "submission_ref": "submission-ref-bad-state",
            "family_id": "family-a",
            "route": "neg-risk",
            "reason": "awaiting_resolve",
        }),
    };

    let err = LiveSubmissionRepo.append(&db.pool, row).await.unwrap_err();

    assert!(matches!(
        err,
        PersistenceError::InvalidValue { ref kind, .. }
        if *kind == "live_submission_records.state"
    ));

    db.cleanup().await;
}

#[tokio::test]
async fn live_submission_records_reject_shadow_attempt_ids() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    ExecutionAttemptRepo
        .append(
            &db.pool,
            &sample_attempt(
                "attempt-shadow-submit-1",
                ExecutionMode::Shadow,
                "negrisk-submit-family:family-a:member-1",
            ),
        )
        .await
        .unwrap();

    let err = LiveSubmissionRepo
        .append(
            &db.pool,
            sample_live_submission_record("attempt-shadow-submit-1", "submission-ref-shadow"),
        )
        .await
        .unwrap_err();

    assert!(matches!(
        err,
        PersistenceError::LiveSubmissionRequiresLiveAttempt { ref attempt_id, .. }
        if attempt_id == "attempt-shadow-submit-1"
    ));

    db.cleanup().await;
}

#[tokio::test]
async fn live_artifacts_reject_shadow_attempt_ids() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    ExecutionAttemptRepo
        .append(
            &db.pool,
            &sample_attempt(
                "attempt-shadow-1",
                ExecutionMode::Shadow,
                "negrisk-submit-family:family-a:member-1",
            ),
        )
        .await
        .unwrap();

    let err = LiveArtifactRepo
        .append(&db.pool, sample_live_artifact("attempt-shadow-1"))
        .await
        .unwrap_err();

    assert!(matches!(
        err,
        PersistenceError::LiveArtifactRequiresLiveAttempt { ref attempt_id }
        if attempt_id == "attempt-shadow-1"
    ));

    db.cleanup().await;
}
