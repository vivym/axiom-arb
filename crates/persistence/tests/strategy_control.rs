use std::sync::atomic::{AtomicU64, Ordering};

use chrono::Utc;
use domain::{
    canonical_strategy_artifact_semantic_digest, StrategyArtifactSemanticDigestInput, StrategyKey,
};
use persistence::run_migrations;
use sqlx::{postgres::PgPoolOptions, PgPool, Row};

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
            "persistence_strategy_control_test_{}_{}",
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

async fn table_exists(pool: &PgPool, table_name: &str) -> bool {
    sqlx::query(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM information_schema.tables
            WHERE table_schema = current_schema() AND table_name = $1
        ) AS exists
        "#,
    )
    .bind(table_name)
    .fetch_one(pool)
    .await
    .expect("table lookup should succeed")
    .get("exists")
}

async fn column_exists(pool: &PgPool, table_name: &str, column_name: &str) -> bool {
    sqlx::query(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM information_schema.columns
            WHERE table_schema = current_schema() AND table_name = $1 AND column_name = $2
        ) AS exists
        "#,
    )
    .bind(table_name)
    .bind(column_name)
    .fetch_one(pool)
    .await
    .expect("column lookup should succeed")
    .get("exists")
}

#[tokio::test]
async fn strategy_control_migration_creates_neutral_lineage_tables() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    assert!(table_exists(&db.pool, "strategy_candidate_sets").await);
    assert!(table_exists(&db.pool, "adoptable_strategy_revisions").await);
    assert!(table_exists(&db.pool, "strategy_adoption_provenance").await);
    assert!(table_exists(&db.pool, "operator_strategy_adoption_history").await);

    assert!(
        column_exists(
            &db.pool,
            "strategy_adoption_provenance",
            "operator_strategy_revision"
        )
        .await
    );
    assert!(
        column_exists(
            &db.pool,
            "operator_strategy_adoption_history",
            "operator_strategy_revision"
        )
        .await
    );

    db.cleanup().await;
}

#[test]
fn semantic_digest_ignores_provenance_only_metadata() {
    let semantic_payload = "route=neg-risk;scope=family-42;legs=2".to_owned();
    let key = StrategyKey::new("neg-risk", "family-42");
    let baseline = StrategyArtifactSemanticDigestInput {
        key: key.clone(),
        route_policy_version: "route-policy-v1".to_owned(),
        canonical_semantic_payload: semantic_payload.clone(),
        source_snapshot_id: Some("snapshot-1".to_owned()),
        source_session_id: Some("source-session-1".to_owned()),
        observed_at: Some(Utc::now()),
        strategy_candidate_revision: Some("candidate-strategy-1".to_owned()),
        adoptable_strategy_revision: Some("adoptable-strategy-1".to_owned()),
        provenance_explanation: Some("initial lineage".to_owned()),
    };
    let provenance_only_change = StrategyArtifactSemanticDigestInput {
        key,
        route_policy_version: "route-policy-v1".to_owned(),
        canonical_semantic_payload: semantic_payload,
        source_snapshot_id: Some("snapshot-2".to_owned()),
        source_session_id: Some("source-session-2".to_owned()),
        observed_at: Some(Utc::now() + chrono::Duration::minutes(5)),
        strategy_candidate_revision: Some("candidate-strategy-2".to_owned()),
        adoptable_strategy_revision: Some("adoptable-strategy-2".to_owned()),
        provenance_explanation: Some("migrated from legacy target lineage".to_owned()),
    };

    let baseline_digest = canonical_strategy_artifact_semantic_digest(&baseline);
    let changed_digest = canonical_strategy_artifact_semantic_digest(&provenance_only_change);

    assert_eq!(baseline_digest, changed_digest);
}
