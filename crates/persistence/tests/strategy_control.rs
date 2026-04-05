use std::sync::atomic::{AtomicU64, Ordering};

use chrono::Utc;
use domain::{
    canonical_strategy_artifact_semantic_digest, StrategyArtifactSemanticDigestInput, StrategyKey,
};
use persistence::{
    models::{
        OperatorStrategyAdoptionHistoryRow, StrategyAdoptionProvenanceRow, StrategyCandidateSetRow,
    },
    run_migrations, OperatorStrategyAdoptionHistoryRepo, StrategyAdoptionRepo,
    StrategyControlArtifactRepo,
};
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

#[tokio::test]
async fn strategy_control_repos_read_legacy_lineage_when_neutral_tables_are_empty() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    sqlx::query(
        r#"
        INSERT INTO candidate_target_sets (candidate_revision, snapshot_id, source_revision, payload)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind("candidate-legacy")
    .bind("snapshot-legacy")
    .bind("discovery-legacy")
    .bind(serde_json::json!({
        "candidate_revision": "candidate-legacy",
        "snapshot_id": "snapshot-legacy",
    }))
    .execute(&db.pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        INSERT INTO adoptable_target_revisions (
            adoptable_revision,
            candidate_revision,
            rendered_operator_target_revision,
            payload
        )
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind("adoptable-legacy")
    .bind("candidate-legacy")
    .bind("targets-rev-legacy")
    .bind(serde_json::json!({
        "adoptable_revision": "adoptable-legacy",
        "candidate_revision": "candidate-legacy",
    }))
    .execute(&db.pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        INSERT INTO candidate_adoption_provenance (
            operator_target_revision,
            adoptable_revision,
            candidate_revision
        )
        VALUES ($1, $2, $3)
        "#,
    )
    .bind("targets-rev-legacy")
    .bind("adoptable-legacy")
    .bind("candidate-legacy")
    .execute(&db.pool)
    .await
    .unwrap();

    let candidate = StrategyControlArtifactRepo
        .get_strategy_candidate_set(&db.pool, "candidate-legacy")
        .await
        .unwrap()
        .expect("neutral repo should read legacy candidate row");
    assert_eq!(
        candidate,
        StrategyCandidateSetRow {
            strategy_candidate_revision: "candidate-legacy".to_owned(),
            snapshot_id: "snapshot-legacy".to_owned(),
            source_revision: "discovery-legacy".to_owned(),
            payload: serde_json::json!({
                "candidate_revision": "candidate-legacy",
                "snapshot_id": "snapshot-legacy",
            }),
        }
    );

    let provenance = StrategyAdoptionRepo
        .get_by_operator_strategy_revision(&db.pool, "targets-rev-legacy")
        .await
        .unwrap()
        .expect("neutral repo should read legacy provenance row");
    assert_eq!(
        provenance,
        StrategyAdoptionProvenanceRow {
            operator_strategy_revision: "targets-rev-legacy".to_owned(),
            adoptable_strategy_revision: "adoptable-legacy".to_owned(),
            strategy_candidate_revision: "candidate-legacy".to_owned(),
        }
    );

    db.cleanup().await;
}

#[tokio::test]
async fn operator_strategy_adoption_history_repo_reads_legacy_history_when_neutral_table_is_empty()
{
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    sqlx::query(
        r#"
        INSERT INTO operator_target_adoption_history (
            adoption_id,
            action_kind,
            operator_target_revision,
            previous_operator_target_revision,
            adoptable_revision,
            candidate_revision,
            adopted_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind("legacy-adoption-1")
    .bind("adopt")
    .bind("targets-rev-11")
    .bind("targets-rev-10")
    .bind("adoptable-11")
    .bind("candidate-11")
    .bind(
        chrono::DateTime::parse_from_rfc3339("2026-04-06T01:00:00Z")
            .unwrap()
            .with_timezone(&Utc),
    )
    .execute(&db.pool)
    .await
    .unwrap();

    let latest = OperatorStrategyAdoptionHistoryRepo
        .latest(&db.pool)
        .await
        .unwrap()
        .expect("neutral history repo should read legacy history row");
    assert_eq!(
        latest,
        OperatorStrategyAdoptionHistoryRow {
            adoption_id: "legacy-adoption-1".to_owned(),
            action_kind: "adopt".to_owned(),
            operator_strategy_revision: "targets-rev-11".to_owned(),
            previous_operator_strategy_revision: Some("targets-rev-10".to_owned()),
            adoptable_strategy_revision: Some("adoptable-11".to_owned()),
            strategy_candidate_revision: Some("candidate-11".to_owned()),
            adopted_at: chrono::DateTime::parse_from_rfc3339("2026-04-06T01:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        }
    );

    let previous = OperatorStrategyAdoptionHistoryRepo
        .previous_distinct_revision(&db.pool, "targets-rev-11")
        .await
        .unwrap();
    assert_eq!(previous.as_deref(), Some("targets-rev-10"));

    db.cleanup().await;
}
