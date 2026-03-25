use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use domain::ExecutionMode;
use persistence::{
    models::{ExecutionAttemptRow, PendingReconcileRow, ShadowExecutionArtifactRow},
    run_migrations, ExecutionAttemptRepo, PendingReconcileRepo, PersistenceError,
    RuntimeProgressRepo, ShadowArtifactRepo,
};
use serde_json::json;
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
            "persistence_runtime_test_{}_{}",
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

fn sample_shadow_artifact(attempt_id: &str) -> ShadowExecutionArtifactRow {
    ShadowExecutionArtifactRow {
        attempt_id: attempt_id.to_owned(),
        stream: "shadow.execution".to_owned(),
        payload: json!({
            "attempt_id": attempt_id,
            "kind": "planned_order",
        }),
    }
}

fn sample_attempt(attempt_id: &str, mode: ExecutionMode) -> ExecutionAttemptRow {
    ExecutionAttemptRow {
        attempt_id: attempt_id.to_owned(),
        plan_id: format!("plan-{attempt_id}"),
        snapshot_id: "snapshot-7".to_owned(),
        route: if mode == ExecutionMode::Live {
            "neg-risk".to_owned()
        } else {
            "shadow".to_owned()
        },
        scope: "family:family-1".to_owned(),
        matched_rule_id: Some("rule-family-anchor".to_owned()),
        execution_mode: mode,
        attempt_no: 1,
        idempotency_key: format!("idem-{attempt_id}"),
    }
}

fn sample_pending_reconcile(pending_ref: &str) -> PendingReconcileRow {
    PendingReconcileRow {
        pending_ref: pending_ref.to_owned(),
        scope_kind: "family".to_owned(),
        scope_id: "family-1".to_owned(),
        reason: "ambiguous_attempt".to_owned(),
    }
}

#[tokio::test]
async fn runtime_progress_persists_journal_state_snapshot_triplet() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    RuntimeProgressRepo
        .record_progress(&db.pool, 41, 7, Some("snapshot-7"))
        .await
        .unwrap();

    let progress = RuntimeProgressRepo
        .current(&db.pool)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(progress.last_journal_seq, 41);
    assert_eq!(progress.last_state_version, 7);
    assert_eq!(progress.last_snapshot_id.as_deref(), Some("snapshot-7"));

    db.cleanup().await;
}

#[tokio::test]
async fn shadow_artifacts_are_isolated_from_live_attempt_rows() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    ExecutionAttemptRepo
        .append(
            &db.pool,
            &sample_attempt("attempt-shadow-1", ExecutionMode::Shadow),
        )
        .await
        .unwrap();

    ShadowArtifactRepo
        .append(&db.pool, sample_shadow_artifact("attempt-shadow-1"))
        .await
        .unwrap();

    assert!(ExecutionAttemptRepo
        .list_live_attempts(&db.pool)
        .await
        .unwrap()
        .is_empty());

    db.cleanup().await;
}

#[tokio::test]
async fn shadow_artifacts_reject_live_attempt_ids() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    ExecutionAttemptRepo
        .append(
            &db.pool,
            &sample_attempt("attempt-live-1", ExecutionMode::Live),
        )
        .await
        .unwrap();

    let err = ShadowArtifactRepo
        .append(&db.pool, sample_shadow_artifact("attempt-live-1"))
        .await
        .unwrap_err();

    assert!(matches!(
        err,
        PersistenceError::ShadowArtifactRequiresShadowAttempt { ref attempt_id }
        if attempt_id == "attempt-live-1"
    ));

    db.cleanup().await;
}

#[tokio::test]
async fn shadow_artifact_table_rejects_live_attempt_ids_via_direct_sql() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    ExecutionAttemptRepo
        .append(
            &db.pool,
            &sample_attempt("attempt-live-sql", ExecutionMode::Live),
        )
        .await
        .unwrap();

    let err = sqlx::query(
        r#"
        INSERT INTO shadow_execution_artifacts (attempt_id, stream, payload)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind("attempt-live-sql")
    .bind("shadow.execution")
    .bind(json!({ "attempt_id": "attempt-live-sql", "kind": "planned_order" }))
    .execute(&db.pool)
    .await
    .unwrap_err();

    let message = err.to_string();
    assert!(
        message.contains("shadow_execution_artifacts requires a shadow execution attempt"),
        "unexpected database error: {message}"
    );

    db.cleanup().await;
}

#[tokio::test]
async fn execution_attempt_table_rejects_mode_change_when_shadow_artifacts_exist() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    ExecutionAttemptRepo
        .append(
            &db.pool,
            &sample_attempt("attempt-shadow-sql", ExecutionMode::Shadow),
        )
        .await
        .unwrap();

    sqlx::query(
        r#"
        INSERT INTO shadow_execution_artifacts (attempt_id, stream, payload)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind("attempt-shadow-sql")
    .bind("shadow.execution")
    .bind(json!({ "attempt_id": "attempt-shadow-sql", "kind": "planned_order" }))
    .execute(&db.pool)
    .await
    .unwrap();

    let err = sqlx::query(
        r#"
        UPDATE execution_attempts
        SET execution_mode = 'live'
        WHERE attempt_id = $1
        "#,
    )
    .bind("attempt-shadow-sql")
    .execute(&db.pool)
    .await
    .unwrap_err();

    let message = err.to_string();
    assert!(
        message.contains("execution_attempts with shadow artifacts cannot change away from shadow"),
        "unexpected database error: {message}"
    );

    db.cleanup().await;
}

#[tokio::test]
async fn live_artifact_table_rejects_shadow_attempt_ids_via_direct_sql() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    ExecutionAttemptRepo
        .append(
            &db.pool,
            &sample_attempt("attempt-shadow-live-sql", ExecutionMode::Shadow),
        )
        .await
        .unwrap();

    let err = sqlx::query(
        r#"
        INSERT INTO live_execution_artifacts (attempt_id, stream, payload)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind("attempt-shadow-live-sql")
    .bind("live.execution")
    .bind(json!({ "attempt_id": "attempt-shadow-live-sql", "kind": "planned_order" }))
    .execute(&db.pool)
    .await
    .unwrap_err();

    let message = err.to_string();
    assert!(
        message.contains("live_execution_artifacts requires a live execution attempt"),
        "unexpected database error: {message}"
    );

    db.cleanup().await;
}

#[tokio::test]
async fn execution_attempt_table_rejects_mode_change_when_live_artifacts_exist() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    ExecutionAttemptRepo
        .append(
            &db.pool,
            &sample_attempt("attempt-live-sql", ExecutionMode::Live),
        )
        .await
        .unwrap();

    sqlx::query(
        r#"
        INSERT INTO live_execution_artifacts (attempt_id, stream, payload)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind("attempt-live-sql")
    .bind("live.execution")
    .bind(json!({ "attempt_id": "attempt-live-sql", "kind": "planned_order" }))
    .execute(&db.pool)
    .await
    .unwrap();

    let err = sqlx::query(
        r#"
        UPDATE execution_attempts
        SET execution_mode = 'shadow'
        WHERE attempt_id = $1
        "#,
    )
    .bind("attempt-live-sql")
    .execute(&db.pool)
    .await
    .unwrap_err();

    let message = err.to_string();
    assert!(
        message.contains("execution_attempts with live artifacts cannot change away from live"),
        "unexpected database error: {message}"
    );

    db.cleanup().await;
}

#[tokio::test]
async fn live_artifact_schema_matches_task5_primary_key() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let primary_key_columns: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT att.attname
        FROM pg_constraint con
        JOIN pg_class rel ON rel.oid = con.conrelid
        JOIN pg_namespace nsp ON nsp.oid = rel.relnamespace
        JOIN unnest(con.conkey) WITH ORDINALITY AS cols(attnum, ordinality) ON TRUE
        JOIN pg_attribute att ON att.attrelid = rel.oid AND att.attnum = cols.attnum
        WHERE rel.relname = 'live_execution_artifacts'
          AND nsp.nspname = current_schema()
          AND con.contype = 'p'
        ORDER BY cols.ordinality
        "#,
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert_eq!(primary_key_columns, vec!["attempt_id", "stream"]);

    db.cleanup().await;
}

#[tokio::test]
async fn followup_migration_upgrades_legacy_live_artifacts_to_composite_primary_key() {
    let db = TestDatabase::new().await;

    apply_migration_file(&db.pool, "0005_unified_runtime_backbone.sql")
        .await
        .unwrap();
    apply_migration_file(&db.pool, "0006_phase3b_negrisk_live.sql")
        .await
        .unwrap();

    insert_legacy_execution_attempt(
        &db.pool,
        "attempt-live-upgrade-1",
        "plan-attempt-live-upgrade-1",
        "snapshot-7",
        "live",
        1,
        "idem-attempt-live-upgrade-1",
    )
    .await;

    sqlx::query(
        r#"
        INSERT INTO live_execution_artifacts (attempt_id, stream, payload)
        VALUES
            ($1, $2, $3),
            ($1, $2, $4)
        "#,
    )
    .bind("attempt-live-upgrade-1")
    .bind("live.execution")
    .bind(json!({ "kind": "planned_order", "seq": 1 }))
    .bind(json!({ "kind": "planned_order", "seq": 1 }))
    .execute(&db.pool)
    .await
    .unwrap();

    apply_migration_file(&db.pool, "0007_phase3b_negrisk_live_followup.sql")
        .await
        .unwrap();

    let primary_key_columns: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT att.attname
        FROM pg_constraint con
        JOIN pg_class rel ON rel.oid = con.conrelid
        JOIN pg_namespace nsp ON nsp.oid = rel.relnamespace
        JOIN unnest(con.conkey) WITH ORDINALITY AS cols(attnum, ordinality) ON TRUE
        JOIN pg_attribute att ON att.attrelid = rel.oid AND att.attnum = cols.attnum
        WHERE rel.relname = 'live_execution_artifacts'
          AND nsp.nspname = current_schema()
          AND con.contype = 'p'
        ORDER BY cols.ordinality
        "#,
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert_eq!(primary_key_columns, vec!["attempt_id", "stream"]);

    let payloads: Vec<serde_json::Value> = sqlx::query_scalar(
        r#"
        SELECT payload
        FROM live_execution_artifacts
        WHERE attempt_id = $1 AND stream = $2
        "#,
    )
    .bind("attempt-live-upgrade-1")
    .bind("live.execution")
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert_eq!(payloads, vec![json!({ "kind": "planned_order", "seq": 1 })]);

    db.cleanup().await;
}

#[tokio::test]
async fn followup_migration_rejects_conflicting_payload_groups() {
    let db = TestDatabase::new().await;

    apply_migration_file(&db.pool, "0005_unified_runtime_backbone.sql")
        .await
        .unwrap();
    apply_migration_file(&db.pool, "0006_phase3b_negrisk_live.sql")
        .await
        .unwrap();

    insert_legacy_execution_attempt(
        &db.pool,
        "attempt-live-upgrade-conflict-1",
        "plan-attempt-live-upgrade-conflict-1",
        "snapshot-7",
        "live",
        1,
        "idem-attempt-live-upgrade-conflict-1",
    )
    .await;

    sqlx::query(
        r#"
        INSERT INTO live_execution_artifacts (attempt_id, stream, payload)
        VALUES
            ($1, $2, $3),
            ($1, $2, $4)
        "#,
    )
    .bind("attempt-live-upgrade-conflict-1")
    .bind("live.execution")
    .bind(json!({ "kind": "planned_order", "seq": 1 }))
    .bind(json!({ "kind": "signed_order", "seq": 2 }))
    .execute(&db.pool)
    .await
    .unwrap();

    let err = apply_migration_file(&db.pool, "0007_phase3b_negrisk_live_followup.sql")
        .await
        .unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains("conflicting payloads"),
        "unexpected migration error: {message}"
    );

    db.cleanup().await;
}

#[tokio::test]
async fn execution_attempt_append_rejects_duplicate_attempt_ids() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let row = sample_attempt("attempt-shadow-dup", ExecutionMode::Shadow);
    ExecutionAttemptRepo.append(&db.pool, &row).await.unwrap();

    let err = ExecutionAttemptRepo
        .append(&db.pool, &row)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        PersistenceError::DuplicateExecutionAttempt { ref attempt_id }
        if attempt_id == "attempt-shadow-dup"
    ));

    db.cleanup().await;
}

#[tokio::test]
async fn execution_attempt_round_trips_durable_audit_anchor_fields() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let row = sample_attempt("attempt-live-anchor-1", ExecutionMode::Live);
    ExecutionAttemptRepo.append(&db.pool, &row).await.unwrap();

    let rows = ExecutionAttemptRepo
        .list_live_attempts(&db.pool)
        .await
        .unwrap();
    assert_eq!(rows, vec![row]);

    db.cleanup().await;
}

#[tokio::test]
async fn execution_attempt_anchor_migration_backfills_existing_rows_safely() {
    let db = TestDatabase::new().await;

    apply_migration_file(&db.pool, "0005_unified_runtime_backbone.sql")
        .await
        .unwrap();
    apply_migration_file(&db.pool, "0006_phase3b_negrisk_live.sql")
        .await
        .unwrap();
    apply_migration_file(&db.pool, "0007_phase3b_negrisk_live_followup.sql")
        .await
        .unwrap();

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
    .bind("attempt-pre-anchor-1")
    .bind("request-bound:5:req-1:negrisk-submit-family:family-a")
    .bind("snapshot-legacy")
    .bind("live")
    .bind(1_i32)
    .bind("idem-legacy")
    .execute(&db.pool)
    .await
    .unwrap();

    apply_migration_file(&db.pool, "0008_execution_attempt_audit_anchor.sql")
        .await
        .unwrap();

    let row = sqlx::query_as::<_, (String, String, Option<String>)>(
        r#"
        SELECT route, scope, matched_rule_id
        FROM execution_attempts
        WHERE attempt_id = $1
        "#,
    )
    .bind("attempt-pre-anchor-1")
    .fetch_one(&db.pool)
    .await
    .unwrap();

    assert_eq!(row.0, "neg-risk");
    assert_eq!(row.1, "family-a");
    assert_eq!(row.2, None);

    db.cleanup().await;
}

async fn insert_legacy_execution_attempt(
    pool: &PgPool,
    attempt_id: &str,
    plan_id: &str,
    snapshot_id: &str,
    execution_mode: &str,
    attempt_no: i32,
    idempotency_key: &str,
) {
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
    .bind(snapshot_id)
    .bind(execution_mode)
    .bind(attempt_no)
    .bind(idempotency_key)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn pending_reconcile_append_rejects_duplicate_refs() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let row = sample_pending_reconcile("pending-1");
    PendingReconcileRepo
        .append(
            &db.pool,
            &row,
            &json!({ "attempt_id": "attempt-shadow-dup" }),
        )
        .await
        .unwrap();

    let err = PendingReconcileRepo
        .append(
            &db.pool,
            &row,
            &json!({ "attempt_id": "attempt-shadow-dup" }),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        PersistenceError::DuplicatePendingReconcile { ref pending_ref }
        if pending_ref == "pending-1"
    ));

    db.cleanup().await;
}

fn migration_file(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../migrations")
        .join(name)
}

async fn apply_migration_file(pool: &PgPool, file_name: &str) -> Result<(), sqlx::Error> {
    let sql = std::fs::read_to_string(migration_file(file_name))
        .expect("migration file should exist for runtime backbone tests");
    sqlx::raw_sql(&sql).execute(pool).await?;
    Ok(())
}
