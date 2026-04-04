use std::sync::atomic::{AtomicU64, Ordering};

use domain::ExecutionMode;
use persistence::{
    models::{ExecutionAttemptRow, ShadowExecutionArtifactRow},
    run_migrations, ExecutionAttemptRepo, ShadowArtifactRepo,
};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool};

use app_replay::{load_negrisk_shadow_attempt_artifacts, NegRiskShadowAttemptArtifacts};

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
            "app_replay_negrisk_shadow_contract_{}_{}",
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

fn sample_attempt(
    attempt_id: &str,
    mode: ExecutionMode,
    plan_id: &str,
    route: &str,
) -> ExecutionAttemptRow {
    let scope = if route == "neg-risk" {
        plan_id
            .split("negrisk-shadow-family:")
            .nth(1)
            .and_then(|suffix| suffix.split(':').next())
            .map(|family_id| format!("family:{family_id}"))
            .unwrap_or_else(|| "family:family-a".to_owned())
    } else {
        "default".to_owned()
    };

    ExecutionAttemptRow {
        attempt_id: attempt_id.to_owned(),
        plan_id: plan_id.to_owned(),
        snapshot_id: "snapshot-7".to_owned(),
        route: route.to_owned(),
        scope,
        matched_rule_id: Some("rule-negrisk-shadow".to_owned()),
        execution_mode: mode,
        attempt_no: 1,
        idempotency_key: format!("idem-{attempt_id}"),
        run_session_id: None,
    }
}

fn artifact(attempt_id: &str, stream: &str) -> ShadowExecutionArtifactRow {
    ShadowExecutionArtifactRow {
        attempt_id: attempt_id.to_owned(),
        stream: stream.to_owned(),
        payload: json!({
            "kind": "shadow",
            "attempt_id": attempt_id,
        }),
    }
}

#[tokio::test]
async fn negrisk_shadow_contract_lists_only_negrisk_shadow_attempts_with_artifacts() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    ExecutionAttemptRepo
        .append(
            &db.pool,
            &sample_attempt(
                "attempt-negrisk-shadow-1",
                ExecutionMode::Shadow,
                "request-bound:5:req-1:negrisk-shadow-family:family-a",
                "neg-risk",
            ),
        )
        .await
        .unwrap();

    ExecutionAttemptRepo
        .append(
            &db.pool,
            &sample_attempt(
                "attempt-fullset-shadow-1",
                ExecutionMode::Shadow,
                "request-bound:5:req-2:fullset-shadow-family:family-b",
                "fullset",
            ),
        )
        .await
        .unwrap();

    ShadowArtifactRepo
        .append(
            &db.pool,
            artifact("attempt-negrisk-shadow-1", "neg-risk-shadow-plan"),
        )
        .await
        .unwrap();

    ShadowArtifactRepo
        .append(
            &db.pool,
            artifact("attempt-negrisk-shadow-1", "neg-risk-shadow-result"),
        )
        .await
        .unwrap();

    ShadowArtifactRepo
        .append(
            &db.pool,
            artifact("attempt-fullset-shadow-1", "fullset-shadow-plan"),
        )
        .await
        .unwrap();

    let rows = load_negrisk_shadow_attempt_artifacts(&db.pool)
        .await
        .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows,
        vec![NegRiskShadowAttemptArtifacts {
            attempt: sample_attempt(
                "attempt-negrisk-shadow-1",
                ExecutionMode::Shadow,
                "request-bound:5:req-1:negrisk-shadow-family:family-a",
                "neg-risk",
            ),
            artifacts: vec![
                artifact("attempt-negrisk-shadow-1", "neg-risk-shadow-plan"),
                artifact("attempt-negrisk-shadow-1", "neg-risk-shadow-result"),
            ],
        }]
    );

    db.cleanup().await;
}
