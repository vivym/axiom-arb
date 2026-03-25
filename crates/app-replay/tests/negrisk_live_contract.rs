use std::sync::atomic::{AtomicU64, Ordering};

use domain::ExecutionMode;
use persistence::{
    models::{ExecutionAttemptRow, LiveExecutionArtifactRow},
    run_migrations, ExecutionAttemptRepo, LiveArtifactRepo,
};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool};

use app_replay::{load_negrisk_live_attempt_artifacts, NegRiskLiveAttemptArtifacts};

static NEXT_SCHEMA_ID: AtomicU64 = AtomicU64::new(1);

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
            "app_replay_negrisk_live_contract_{}_{}",
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

fn request_bound_plan_id(request_id: &str, inner_plan_id: &str) -> String {
    format!(
        "request-bound:{}:{}:{}",
        request_id.len(),
        request_id,
        inner_plan_id
    )
}

fn artifact(attempt_id: &str, stream: &str, kind: &str) -> LiveExecutionArtifactRow {
    LiveExecutionArtifactRow {
        attempt_id: attempt_id.to_owned(),
        stream: stream.to_owned(),
        payload: json!({
            "kind": kind,
            "attempt_id": attempt_id,
        }),
    }
}

#[tokio::test]
async fn negrisk_live_contract_lists_only_negrisk_live_attempts_with_artifacts() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    ExecutionAttemptRepo
        .append(
            &db.pool,
            &sample_attempt(
                "attempt-negrisk-live-1",
                ExecutionMode::Live,
                &request_bound_plan_id(
                    "req-1",
                    "negrisk-submit-family:family-a:condition-1:token-1:0.4:5",
                ),
            ),
        )
        .await
        .unwrap();

    ExecutionAttemptRepo
        .append(
            &db.pool,
            &sample_attempt(
                "attempt-non-negrisk-live-1",
                ExecutionMode::Live,
                &request_bound_plan_id("req-2", "fullset-buy-merge:condition-1"),
            ),
        )
        .await
        .unwrap();

    ExecutionAttemptRepo
        .append(
            &db.pool,
            &sample_attempt(
                "attempt-negrisk-shadow-1",
                ExecutionMode::Shadow,
                &request_bound_plan_id(
                    "req-3",
                    "negrisk-submit-family:family-b:condition-2:token-2:0.4:5",
                ),
            ),
        )
        .await
        .unwrap();

    LiveArtifactRepo
        .append(
            &db.pool,
            artifact("attempt-negrisk-live-1", "negrisk.live", "planned_order"),
        )
        .await
        .unwrap();

    let rows = load_negrisk_live_attempt_artifacts(&db.pool).await.unwrap();

    assert_eq!(
        rows,
        vec![NegRiskLiveAttemptArtifacts {
            attempt: sample_attempt(
                "attempt-negrisk-live-1",
                ExecutionMode::Live,
                &request_bound_plan_id(
                    "req-1",
                    "negrisk-submit-family:family-a:condition-1:token-1:0.4:5",
                ),
            ),
            artifacts: vec![artifact(
                "attempt-negrisk-live-1",
                "negrisk.live",
                "planned_order"
            )],
        }]
    );

    db.cleanup().await;
}

#[tokio::test]
async fn negrisk_live_contract_returns_single_row_per_attempt_stream_after_retries() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let plan_id = request_bound_plan_id(
        "req-4",
        "negrisk-submit-family:family-c:condition-3:token-3:0.4:5",
    );
    let attempt = sample_attempt("attempt-negrisk-live-dup-1", ExecutionMode::Live, &plan_id);

    ExecutionAttemptRepo
        .append(&db.pool, &attempt)
        .await
        .unwrap();

    LiveArtifactRepo
        .append(
            &db.pool,
            artifact(
                "attempt-negrisk-live-dup-1",
                "negrisk.live",
                "planned_order",
            ),
        )
        .await
        .unwrap();
    LiveArtifactRepo
        .append(
            &db.pool,
            artifact("attempt-negrisk-live-dup-1", "negrisk.live", "signed_order"),
        )
        .await
        .unwrap();

    let rows = load_negrisk_live_attempt_artifacts(&db.pool).await.unwrap();

    assert_eq!(
        rows,
        vec![NegRiskLiveAttemptArtifacts {
            attempt,
            artifacts: vec![artifact(
                "attempt-negrisk-live-dup-1",
                "negrisk.live",
                "signed_order",
            )],
        }]
    );

    db.cleanup().await;
}
