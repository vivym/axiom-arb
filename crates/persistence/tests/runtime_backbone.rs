use std::sync::atomic::{AtomicU64, Ordering};

use persistence::{
    models::ShadowExecutionArtifactRow, run_migrations, ExecutionAttemptRepo, RuntimeProgressRepo,
    ShadowArtifactRepo,
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
