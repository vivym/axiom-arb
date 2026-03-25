use std::sync::atomic::{AtomicU64, Ordering};

use domain::ExecutionMode;
use persistence::{
    models::{ExecutionAttemptRow, LiveExecutionArtifactRow},
    run_migrations, ExecutionAttemptRepo, LiveArtifactRepo, PersistenceError,
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

