use std::sync::atomic::{AtomicU64, Ordering};

use persistence::{
    models::{AdoptableTargetRevisionRow, CandidateAdoptionProvenanceRow, CandidateTargetSetRow},
    run_migrations, CandidateAdoptionRepo, CandidateArtifactRepo,
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

#[tokio::test]
async fn adoption_provenance_round_trips_operator_target_revision() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let artifacts = CandidateArtifactRepo;
    let adoption = CandidateAdoptionRepo;

    let candidate_row = CandidateTargetSetRow {
        candidate_revision: "candidate-9".to_owned(),
        snapshot_id: "snapshot-9".to_owned(),
        source_revision: "discovery-9".to_owned(),
        payload: json!({
            "candidate_revision": "candidate-9",
            "snapshot_id": "snapshot-9",
        }),
    };
    artifacts
        .upsert_candidate_target_set(&db.pool, &candidate_row)
        .await
        .unwrap();

    let adoptable_row = AdoptableTargetRevisionRow {
        adoptable_revision: "adoptable-9".to_owned(),
        candidate_revision: "candidate-9".to_owned(),
        rendered_operator_target_revision: "targets-rev-9".to_owned(),
        payload: json!({
            "adoptable_revision": "adoptable-9",
            "candidate_revision": "candidate-9",
        }),
    };
    artifacts
        .upsert_adoptable_target_revision(&db.pool, &adoptable_row)
        .await
        .unwrap();

    let provenance_row = CandidateAdoptionProvenanceRow {
        operator_target_revision: "targets-rev-9".to_owned(),
        adoptable_revision: "adoptable-9".to_owned(),
        candidate_revision: "candidate-9".to_owned(),
    };
    adoption
        .upsert_provenance(&db.pool, &provenance_row)
        .await
        .unwrap();

    let loaded = adoption
        .get_by_operator_target_revision(&db.pool, "targets-rev-9")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded, provenance_row);

    db.cleanup().await;
}
