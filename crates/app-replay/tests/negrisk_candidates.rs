use std::sync::atomic::{AtomicU64, Ordering};

use app_replay::{
    load_negrisk_adoptable_target_revisions, load_negrisk_candidate_adoption_provenance,
    load_negrisk_candidate_summary, load_negrisk_candidate_target_sets,
    summarize_negrisk_candidate_chain, NegRiskCandidateSummary,
};
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
    async fn new() -> Option<Self> {
        let Some(database_url) = std::env::var_os("DATABASE_URL") else {
            return None;
        };
        let database_url = database_url
            .into_string()
            .expect("DATABASE_URL should be valid utf8");

        let admin_pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("test database should connect");

        let schema = format!(
            "app_replay_negrisk_candidates_{}_{}",
            std::process::id(),
            NEXT_SCHEMA_ID.fetch_add(1, Ordering::Relaxed)
        );
        sqlx::query(&format!(r#"CREATE SCHEMA "{schema}""#))
            .execute(&admin_pool)
            .await
            .expect("schema should create");

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

        Some(Self {
            admin_pool,
            pool,
            schema,
        })
    }

    async fn cleanup(self) {
        self.pool.close().await;
        sqlx::query(&format!(
            r#"DROP SCHEMA IF EXISTS "{schema}" CASCADE"#,
            schema = self.schema
        ))
        .execute(&self.admin_pool)
        .await
        .expect("schema should drop");
        self.admin_pool.close().await;
    }
}

#[tokio::test]
async fn replay_loads_candidate_and_adoption_provenance_chain() {
    let Some(db) = TestDatabase::new().await else {
        return;
    };
    run_migrations(&db.pool).await.unwrap();
    seed_candidate_chain(&db.pool, "candidate-9", "adoptable-9", "targets-rev-9").await;

    let summary = load_negrisk_candidate_summary(&db.pool).await.unwrap();
    let candidates = load_negrisk_candidate_target_sets(&db.pool).await.unwrap();
    let adoptable = load_negrisk_adoptable_target_revisions(&db.pool)
        .await
        .unwrap();
    let provenance = load_negrisk_candidate_adoption_provenance(&db.pool)
        .await
        .unwrap();

    assert_eq!(
        summary,
        NegRiskCandidateSummary {
            candidate_target_set_count: 1,
            adoptable_target_revision_count: 1,
            adoption_provenance_count: 1,
            latest_candidate_revision: Some("candidate-9".to_owned()),
            latest_adoptable_revision: Some("adoptable-9".to_owned()),
            operator_target_revision: Some("targets-rev-9".to_owned()),
        }
    );
    assert_eq!(candidates.len(), 1);
    assert_eq!(adoptable.len(), 1);
    assert_eq!(provenance.len(), 1);

    db.cleanup().await;
}

#[tokio::test]
async fn replay_keeps_candidate_generation_advisory_without_provenance() {
    let Some(db) = TestDatabase::new().await else {
        return;
    };
    run_migrations(&db.pool).await.unwrap();

    let artifacts = CandidateArtifactRepo;
    artifacts
        .upsert_candidate_target_set(
            &db.pool,
            &CandidateTargetSetRow {
                candidate_revision: "candidate-advisory-1".to_owned(),
                snapshot_id: "snapshot-21".to_owned(),
                source_revision: "discovery-21".to_owned(),
                payload: json!({
                    "candidate_revision": "candidate-advisory-1",
                    "snapshot_id": "snapshot-21",
                }),
            },
        )
        .await
        .unwrap();
    artifacts
        .upsert_adoptable_target_revision(
            &db.pool,
            &AdoptableTargetRevisionRow {
                adoptable_revision: "adoptable-advisory-1".to_owned(),
                candidate_revision: "candidate-advisory-1".to_owned(),
                rendered_operator_target_revision: "targets-rev-advisory".to_owned(),
                payload: json!({
                    "adoptable_revision": "adoptable-advisory-1",
                    "candidate_revision": "candidate-advisory-1",
                    "rendered_operator_target_revision": "targets-rev-advisory",
                }),
            },
        )
        .await
        .unwrap();

    let summary = load_negrisk_candidate_summary(&db.pool).await.unwrap();

    assert_eq!(summary.candidate_target_set_count, 1);
    assert_eq!(summary.adoptable_target_revision_count, 1);
    assert_eq!(summary.adoption_provenance_count, 0);
    assert_eq!(
        summary.latest_candidate_revision.as_deref(),
        Some("candidate-advisory-1")
    );
    assert_eq!(
        summary.latest_adoptable_revision.as_deref(),
        Some("adoptable-advisory-1")
    );
    assert_eq!(summary.operator_target_revision, None);

    db.cleanup().await;
}

#[test]
fn summary_is_anchored_to_latest_candidate_chain_only() {
    let summary = summarize_negrisk_candidate_chain(
        &[
            CandidateTargetSetRow {
                candidate_revision: "candidate-9".to_owned(),
                snapshot_id: "snapshot-9".to_owned(),
                source_revision: "discovery-9".to_owned(),
                payload: json!({ "candidate_revision": "candidate-9" }),
            },
            CandidateTargetSetRow {
                candidate_revision: "candidate-10".to_owned(),
                snapshot_id: "snapshot-10".to_owned(),
                source_revision: "discovery-10".to_owned(),
                payload: json!({ "candidate_revision": "candidate-10" }),
            },
        ],
        &[AdoptableTargetRevisionRow {
            adoptable_revision: "adoptable-9".to_owned(),
            candidate_revision: "candidate-9".to_owned(),
            rendered_operator_target_revision: "targets-rev-9".to_owned(),
            payload: json!({
                "adoptable_revision": "adoptable-9",
                "candidate_revision": "candidate-9",
                "rendered_operator_target_revision": "targets-rev-9",
            }),
        }],
        &[CandidateAdoptionProvenanceRow {
            operator_target_revision: "targets-rev-9".to_owned(),
            adoptable_revision: "adoptable-9".to_owned(),
            candidate_revision: "candidate-9".to_owned(),
        }],
    );

    assert_eq!(
        summary,
        NegRiskCandidateSummary {
            candidate_target_set_count: 2,
            adoptable_target_revision_count: 1,
            adoption_provenance_count: 1,
            latest_candidate_revision: Some("candidate-10".to_owned()),
            latest_adoptable_revision: None,
            operator_target_revision: None,
        }
    );
}

async fn seed_candidate_chain(
    pool: &PgPool,
    candidate_revision: &str,
    adoptable_revision: &str,
    operator_target_revision: &str,
) {
    let artifacts = CandidateArtifactRepo;
    let adoption = CandidateAdoptionRepo;

    artifacts
        .upsert_candidate_target_set(
            pool,
            &CandidateTargetSetRow {
                candidate_revision: candidate_revision.to_owned(),
                snapshot_id: "snapshot-9".to_owned(),
                source_revision: "discovery-9".to_owned(),
                payload: json!({
                    "candidate_revision": candidate_revision,
                    "snapshot_id": "snapshot-9",
                }),
            },
        )
        .await
        .unwrap();
    artifacts
        .upsert_adoptable_target_revision(
            pool,
            &AdoptableTargetRevisionRow {
                adoptable_revision: adoptable_revision.to_owned(),
                candidate_revision: candidate_revision.to_owned(),
                rendered_operator_target_revision: operator_target_revision.to_owned(),
                payload: json!({
                    "adoptable_revision": adoptable_revision,
                    "candidate_revision": candidate_revision,
                    "rendered_operator_target_revision": operator_target_revision,
                }),
            },
        )
        .await
        .unwrap();
    adoption
        .upsert_provenance(
            pool,
            &CandidateAdoptionProvenanceRow {
                operator_target_revision: operator_target_revision.to_owned(),
                adoptable_revision: adoptable_revision.to_owned(),
                candidate_revision: candidate_revision.to_owned(),
            },
        )
        .await
        .unwrap();
}
