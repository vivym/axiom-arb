#![allow(dead_code)]

use std::{
    env,
    sync::atomic::{AtomicU64, Ordering},
};

use persistence::{
    models::{
        AdoptableStrategyRevisionRow, AdoptableTargetRevisionRow, CandidateTargetSetRow,
        StrategyCandidateSetRow,
    },
    run_migrations, CandidateArtifactRepo,
};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool, Row};

use super::cli::default_test_database_url;

static NEXT_SCHEMA_ID: AtomicU64 = AtomicU64::new(1);

pub struct TestDatabase {
    runtime: tokio::runtime::Runtime,
    admin_pool: PgPool,
    pool: PgPool,
    schema: String,
    database_url: String,
}

impl TestDatabase {
    pub fn new() -> Self {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build");

        let (admin_pool, pool, schema, database_url) = runtime.block_on(async {
            let admin_database_url =
                env::var("DATABASE_URL").unwrap_or_else(|_| default_test_database_url().to_owned());
            let admin_pool = PgPoolOptions::new()
                .max_connections(8)
                .connect(&admin_database_url)
                .await
                .expect("test database should connect");
            let schema = format!(
                "app_live_discover_{}_{}",
                std::process::id(),
                NEXT_SCHEMA_ID.fetch_add(1, Ordering::Relaxed)
            );
            sqlx::query(&format!(r#"CREATE SCHEMA "{schema}""#))
                .execute(&admin_pool)
                .await
                .expect("test schema should create");

            let database_url = schema_scoped_database_url(&admin_database_url, &schema);
            let pool = PgPoolOptions::new()
                .max_connections(8)
                .connect(&database_url)
                .await
                .expect("schema-scoped test pool should connect");
            run_migrations(&pool)
                .await
                .expect("test migrations should run");

            (admin_pool, pool, schema, database_url)
        });

        Self {
            runtime,
            admin_pool,
            pool,
            schema,
            database_url,
        }
    }

    pub fn database_url(&self) -> &str {
        &self.database_url
    }

    pub fn has_candidate_rows(&self) -> bool {
        self.count_rows("candidate_target_sets") > 0
    }

    pub fn has_strategy_candidate_rows(&self) -> bool {
        self.count_rows("strategy_candidate_sets") > 0
    }

    pub fn has_adoptable_rows(&self) -> bool {
        self.count_rows("adoptable_target_revisions") > 0
    }

    pub fn has_strategy_adoptable_rows(&self) -> bool {
        self.count_rows("adoptable_strategy_revisions") > 0
    }

    pub fn has_candidate_provenance_rows(&self) -> bool {
        self.count_rows("candidate_adoption_provenance") > 0
    }

    pub fn has_strategy_provenance_rows(&self) -> bool {
        self.count_rows("strategy_adoption_provenance") > 0
    }

    pub fn strategy_candidate_row_count(&self) -> i64 {
        self.count_rows("strategy_candidate_sets")
    }

    pub fn strategy_adoptable_row_count(&self) -> i64 {
        self.count_rows("adoptable_strategy_revisions")
    }

    pub fn strategy_candidate_rows(&self) -> Vec<StrategyCandidateSetRow> {
        self.runtime.block_on(async {
            sqlx::query(
                r#"
                SELECT strategy_candidate_revision, snapshot_id, source_revision, payload
                FROM strategy_candidate_sets
                ORDER BY strategy_candidate_revision
                "#,
            )
            .fetch_all(&self.pool)
            .await
            .expect("strategy candidate rows should load")
            .into_iter()
            .map(|row| StrategyCandidateSetRow {
                strategy_candidate_revision: row
                    .try_get("strategy_candidate_revision")
                    .expect("strategy candidate revision"),
                snapshot_id: row.try_get("snapshot_id").expect("snapshot id"),
                source_revision: row.try_get("source_revision").expect("source revision"),
                payload: row.try_get("payload").expect("payload"),
            })
            .collect()
        })
    }

    pub fn strategy_adoptable_rows(&self) -> Vec<AdoptableStrategyRevisionRow> {
        self.runtime.block_on(async {
            sqlx::query(
                r#"
                SELECT
                    adoptable_strategy_revision,
                    strategy_candidate_revision,
                    rendered_operator_strategy_revision,
                    payload
                FROM adoptable_strategy_revisions
                ORDER BY adoptable_strategy_revision
                "#,
            )
            .fetch_all(&self.pool)
            .await
            .expect("strategy adoptable rows should load")
            .into_iter()
            .map(|row| AdoptableStrategyRevisionRow {
                adoptable_strategy_revision: row
                    .try_get("adoptable_strategy_revision")
                    .expect("adoptable strategy revision"),
                strategy_candidate_revision: row
                    .try_get("strategy_candidate_revision")
                    .expect("strategy candidate revision"),
                rendered_operator_strategy_revision: row
                    .try_get("rendered_operator_strategy_revision")
                    .expect("rendered operator strategy revision"),
                payload: row.try_get("payload").expect("payload"),
            })
            .collect()
        })
    }

    pub fn seed_advisory_candidate(&self, candidate_revision: &str, reason: &str) {
        self.runtime.block_on(async {
            CandidateArtifactRepo
                .upsert_candidate_target_set(
                    &self.pool,
                    &CandidateTargetSetRow {
                        candidate_revision: candidate_revision.to_owned(),
                        snapshot_id: format!("snapshot-{candidate_revision}"),
                        source_revision: format!("discovery-{candidate_revision}"),
                        payload: json!({
                            "candidate_revision": candidate_revision,
                            "snapshot_id": format!("snapshot-{candidate_revision}"),
                            "targets": [
                                {
                                    "target_id": format!("target-{candidate_revision}"),
                                    "family_id": "family-a",
                                    "validation": {
                                        "status": "deferred",
                                        "reason": reason,
                                    }
                                }
                            ]
                        }),
                    },
                )
                .await
                .expect("advisory candidate row should persist");
        });
    }

    pub fn seed_adoptable_revision_without_provenance(
        &self,
        adoptable_revision: &str,
        candidate_revision: &str,
        operator_target_revision: &str,
    ) {
        self.runtime.block_on(async {
            CandidateArtifactRepo
                .upsert_candidate_target_set(
                    &self.pool,
                    &CandidateTargetSetRow {
                        candidate_revision: candidate_revision.to_owned(),
                        snapshot_id: format!("snapshot-{candidate_revision}"),
                        source_revision: format!("discovery-{candidate_revision}"),
                        payload: json!({
                            "candidate_revision": candidate_revision,
                            "snapshot_id": format!("snapshot-{candidate_revision}"),
                            "targets": [
                                {
                                    "target_id": format!("target-{candidate_revision}"),
                                    "family_id": "family-a",
                                }
                            ]
                        }),
                    },
                )
                .await
                .expect("candidate row should persist");

            CandidateArtifactRepo
                .upsert_adoptable_target_revision(
                    &self.pool,
                    &AdoptableTargetRevisionRow {
                        adoptable_revision: adoptable_revision.to_owned(),
                        candidate_revision: candidate_revision.to_owned(),
                        rendered_operator_target_revision: operator_target_revision.to_owned(),
                        payload: json!({
                            "adoptable_revision": adoptable_revision,
                            "candidate_revision": candidate_revision,
                            "rendered_operator_target_revision": operator_target_revision,
                            "rendered_live_targets": {
                                "family-a": {
                                    "family_id": "family-a",
                                    "members": [
                                        {
                                            "condition_id": "0x0000000000000000000000000000000000000000000000000000000000000001",
                                            "token_id": "29",
                                            "price": "0.43",
                                            "quantity": "5",
                                        }
                                    ]
                                }
                            }
                        }),
                    },
                )
                .await
                .expect("adoptable row should persist");
        });
    }

    pub fn cleanup(self) {
        self.runtime.block_on(async {
            self.pool.close().await;
            let drop_schema = format!(
                r#"DROP SCHEMA IF EXISTS "{schema}" CASCADE"#,
                schema = self.schema
            );
            let _ = sqlx::query(&drop_schema).execute(&self.admin_pool).await;
            self.admin_pool.close().await;
        });
    }

    fn count_rows(&self, table: &str) -> i64 {
        self.runtime.block_on(async {
            sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&self.pool)
                .await
                .expect("row count should load")
        })
    }
}

fn schema_scoped_database_url(database_url: &str, schema: &str) -> String {
    let options = format!("options=-csearch_path%3D{schema}");
    if database_url.contains('?') {
        format!("{database_url}&{options}")
    } else {
        format!("{database_url}?{options}")
    }
}
