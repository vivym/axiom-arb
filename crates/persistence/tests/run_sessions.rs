use std::sync::atomic::{AtomicU64, Ordering};

use persistence::{run_migrations, RunSessionState};
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
            "persistence_run_session_test_{}_{}",
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

#[tokio::test]
async fn run_sessions_migration_creates_table_and_session_link_columns() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let tables: Vec<String> = sqlx::query_scalar(
        "select table_name from information_schema.tables where table_schema = current_schema()",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();

    assert!(tables.iter().any(|name| name == "run_sessions"));

    let progress_columns: Vec<String> = sqlx::query_scalar(
        "select column_name from information_schema.columns where table_name = 'runtime_apply_progress'",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert!(progress_columns
        .iter()
        .any(|name| name == "active_run_session_id"));

    let attempt_columns: Vec<String> = sqlx::query_scalar(
        "select column_name from information_schema.columns where table_name = 'execution_attempts'",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert!(attempt_columns.iter().any(|name| name == "run_session_id"));

    db.cleanup().await;
}

#[test]
fn run_session_state_labels_are_stable() {
    assert_eq!(RunSessionState::Starting.as_str(), "starting");
    assert_eq!(RunSessionState::Running.as_str(), "running");
    assert_eq!(RunSessionState::Exited.as_str(), "exited");
    assert_eq!(RunSessionState::Failed.as_str(), "failed");
}

#[tokio::test]
async fn run_sessions_reject_invalid_state_labels() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let err = sqlx::query(
        r#"
        INSERT INTO run_sessions (
            run_session_id,
            invoked_by,
            mode,
            state,
            started_at,
            last_seen_at,
            ended_at,
            exit_status,
            exit_reason,
            config_path,
            config_fingerprint,
            target_source_kind,
            startup_target_revision_at_start,
            configured_operator_target_revision,
            active_operator_target_revision_at_start,
            rollout_state_at_start,
            real_user_shadow_smoke
        )
        VALUES (
            $1,
            $2,
            $3,
            $4,
            NOW(),
            NOW(),
            NULL,
            NULL,
            NULL,
            $5,
            $6,
            $7,
            $8,
            NULL,
            NULL,
            NULL,
            false
        )
        "#,
    )
    .bind("run-session-invalid-1")
    .bind("tester")
    .bind("daemon")
    .bind("paused")
    .bind("/tmp/run-session-config.toml")
    .bind("fingerprint-1")
    .bind("source")
    .bind("rev-start")
    .execute(&db.pool)
    .await
    .unwrap_err();

    assert!(
        err.to_string().contains("state") || err.to_string().contains("check constraint"),
        "unexpected database error: {err}"
    );

    db.cleanup().await;
}
