use std::sync::atomic::{AtomicU64, Ordering};

use persistence::{
    models::{
        AdoptableStrategyRevisionRow, StrategyAdoptionProvenanceRow, StrategyCandidateSetRow,
    },
    run_migrations, StrategyAdoptionRepo, StrategyControlArtifactRepo,
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
        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://axiom:axiom@localhost:5432/axiom_arb".to_owned());

        let admin_pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("test database should connect");

        let schema = format!(
            "persistence_phase3e_test_{}_{}",
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

async fn seed_candidate_artifact(
    pool: &PgPool,
    artifacts: &StrategyControlArtifactRepo,
    strategy_candidate_revision: &str,
    snapshot_id: &str,
    source_revision: &str,
    rendered_operator_strategy_revision: &str,
) {
    artifacts
        .upsert_strategy_candidate_set(
            pool,
            &StrategyCandidateSetRow {
                strategy_candidate_revision: strategy_candidate_revision.to_owned(),
                snapshot_id: snapshot_id.to_owned(),
                source_revision: source_revision.to_owned(),
                payload: json!({
                    "strategy_candidate_revision": strategy_candidate_revision,
                    "snapshot_id": snapshot_id,
                    "source_revision": source_revision,
                }),
            },
        )
        .await
        .unwrap();

    artifacts
        .upsert_adoptable_strategy_revision(
            pool,
            &AdoptableStrategyRevisionRow {
                adoptable_strategy_revision: format!("adoptable-{strategy_candidate_revision}"),
                strategy_candidate_revision: strategy_candidate_revision.to_owned(),
                rendered_operator_strategy_revision: rendered_operator_strategy_revision.to_owned(),
                payload: json!({
                    "adoptable_strategy_revision": format!("adoptable-{strategy_candidate_revision}"),
                    "strategy_candidate_revision": strategy_candidate_revision,
                    "rendered_operator_strategy_revision": rendered_operator_strategy_revision,
                }),
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn adoption_provenance_round_trips_operator_strategy_revision() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let artifacts = StrategyControlArtifactRepo;
    let adoption = StrategyAdoptionRepo;

    let candidate_row = StrategyCandidateSetRow {
        strategy_candidate_revision: "candidate-9".to_owned(),
        snapshot_id: "snapshot-9".to_owned(),
        source_revision: "discovery-9".to_owned(),
        payload: json!({
            "strategy_candidate_revision": "candidate-9",
            "snapshot_id": "snapshot-9",
        }),
    };
    artifacts
        .upsert_strategy_candidate_set(&db.pool, &candidate_row)
        .await
        .unwrap();

    let adoptable_row = AdoptableStrategyRevisionRow {
        adoptable_strategy_revision: "adoptable-9".to_owned(),
        strategy_candidate_revision: "candidate-9".to_owned(),
        rendered_operator_strategy_revision: "targets-rev-9".to_owned(),
        payload: json!({
            "adoptable_strategy_revision": "adoptable-9",
            "strategy_candidate_revision": "candidate-9",
        }),
    };
    artifacts
        .upsert_adoptable_strategy_revision(&db.pool, &adoptable_row)
        .await
        .unwrap();

    let provenance_row = StrategyAdoptionProvenanceRow {
        operator_strategy_revision: "targets-rev-9".to_owned(),
        adoptable_strategy_revision: "adoptable-9".to_owned(),
        strategy_candidate_revision: "candidate-9".to_owned(),
    };
    adoption
        .upsert_provenance(&db.pool, &provenance_row)
        .await
        .unwrap();

    let loaded = adoption
        .get_by_operator_strategy_revision(&db.pool, "targets-rev-9")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded, provenance_row);

    db.cleanup().await;
}

#[tokio::test]
async fn strategy_artifact_rewrites_are_rejected() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let artifacts = StrategyControlArtifactRepo;
    let candidate_row = StrategyCandidateSetRow {
        strategy_candidate_revision: "candidate-1".to_owned(),
        snapshot_id: "snapshot-1".to_owned(),
        source_revision: "discovery-1".to_owned(),
        payload: json!({
            "strategy_candidate_revision": "candidate-1",
            "snapshot_id": "snapshot-1",
            "source_revision": "discovery-1",
        }),
    };

    artifacts
        .upsert_strategy_candidate_set(&db.pool, &candidate_row)
        .await
        .unwrap();
    artifacts
        .upsert_strategy_candidate_set(&db.pool, &candidate_row)
        .await
        .unwrap();

    let conflicting_candidate_row = StrategyCandidateSetRow {
        snapshot_id: "snapshot-2".to_owned(),
        ..candidate_row.clone()
    };
    assert!(artifacts
        .upsert_strategy_candidate_set(&db.pool, &conflicting_candidate_row)
        .await
        .is_err());

    let adoptable_row = AdoptableStrategyRevisionRow {
        adoptable_strategy_revision: "adoptable-1".to_owned(),
        strategy_candidate_revision: "candidate-1".to_owned(),
        rendered_operator_strategy_revision: "targets-rev-1".to_owned(),
        payload: json!({
            "adoptable_strategy_revision": "adoptable-1",
            "strategy_candidate_revision": "candidate-1",
            "rendered_operator_strategy_revision": "targets-rev-1",
        }),
    };
    artifacts
        .upsert_adoptable_strategy_revision(&db.pool, &adoptable_row)
        .await
        .unwrap();
    artifacts
        .upsert_adoptable_strategy_revision(&db.pool, &adoptable_row)
        .await
        .unwrap();

    let conflicting_adoptable_row = AdoptableStrategyRevisionRow {
        rendered_operator_strategy_revision: "targets-rev-2".to_owned(),
        ..adoptable_row.clone()
    };
    assert!(artifacts
        .upsert_adoptable_strategy_revision(&db.pool, &conflicting_adoptable_row)
        .await
        .is_err());

    db.cleanup().await;
}

#[tokio::test]
async fn provenance_pairing_must_match_the_adoptable_revision_candidate() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let artifacts = StrategyControlArtifactRepo;
    let adoption = StrategyAdoptionRepo;

    seed_candidate_artifact(
        &db.pool,
        &artifacts,
        "candidate-1",
        "snapshot-1",
        "discovery-1",
        "targets-rev-1",
    )
    .await;
    seed_candidate_artifact(
        &db.pool,
        &artifacts,
        "candidate-2",
        "snapshot-2",
        "discovery-2",
        "targets-rev-2",
    )
    .await;

    let mismatched_provenance = StrategyAdoptionProvenanceRow {
        operator_strategy_revision: "targets-rev-1".to_owned(),
        adoptable_strategy_revision: "adoptable-candidate-1".to_owned(),
        strategy_candidate_revision: "candidate-2".to_owned(),
    };
    assert!(adoption
        .upsert_provenance(&db.pool, &mismatched_provenance)
        .await
        .is_err());

    db.cleanup().await;
}

#[tokio::test]
async fn lookup_rejects_mismatched_rendered_operator_strategy_revision() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let artifacts = StrategyControlArtifactRepo;
    let adoption = StrategyAdoptionRepo;

    seed_candidate_artifact(
        &db.pool,
        &artifacts,
        "candidate-3",
        "snapshot-3",
        "discovery-3",
        "targets-rev-other",
    )
    .await;

    let provenance_row = StrategyAdoptionProvenanceRow {
        operator_strategy_revision: "targets-rev-3".to_owned(),
        adoptable_strategy_revision: "adoptable-candidate-3".to_owned(),
        strategy_candidate_revision: "candidate-3".to_owned(),
    };
    adoption
        .upsert_provenance(&db.pool, &provenance_row)
        .await
        .unwrap();

    assert!(adoption
        .get_by_operator_strategy_revision(&db.pool, "targets-rev-3")
        .await
        .is_err());

    db.cleanup().await;
}
