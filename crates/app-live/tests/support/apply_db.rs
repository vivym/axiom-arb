#![allow(dead_code)]

use std::{
    borrow::ToOwned,
    env,
    sync::atomic::{AtomicU64, Ordering},
};

use persistence::{
    models::{
        AdoptableTargetRevisionRow, CandidateAdoptionProvenanceRow, CandidateTargetSetRow,
        OperatorStrategyAdoptionHistoryRow, RuntimeProgressRow,
    },
    run_migrations, CandidateAdoptionRepo, CandidateArtifactRepo,
    RuntimeProgressRepo,
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
                "app_live_apply_{}_{}",
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

    pub fn seed_adoptable_revision(
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
                                            "condition_id": "condition-1",
                                            "token_id": "token-1",
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

            CandidateAdoptionRepo
                .upsert_provenance(
                    &self.pool,
                    &CandidateAdoptionProvenanceRow {
                        operator_target_revision: operator_target_revision.to_owned(),
                        adoptable_revision: adoptable_revision.to_owned(),
                        candidate_revision: candidate_revision.to_owned(),
                    },
                )
                .await
                .expect("candidate provenance should persist");
        });
    }

    pub fn seed_adopted_target_with_active_revision(
        &self,
        operator_target_revision: &str,
        active_operator_target_revision: Option<&str>,
    ) {
        self.seed_adoptable_revision("adoptable-9", "candidate-9", operator_target_revision);

        self.runtime.block_on(async {
            if let Some(active_operator_target_revision) = active_operator_target_revision {
                RuntimeProgressRepo
                    .record_progress(
                        &self.pool,
                        41,
                        7,
                        Some("snapshot-7"),
                        Some(active_operator_target_revision),
                        None,
                    )
                    .await
                    .expect("runtime progress should seed");
            }
        });
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

    pub fn latest_history(&self) -> Option<OperatorStrategyAdoptionHistoryRow> {
        self.runtime.block_on(async {
            sqlx::query(
                r#"
                SELECT
                    adoption_id,
                    action_kind,
                    operator_strategy_revision,
                    previous_operator_strategy_revision,
                    adoptable_strategy_revision,
                    strategy_candidate_revision,
                    adopted_at
                FROM operator_strategy_adoption_history
                ORDER BY history_seq DESC
                LIMIT 1
                "#,
            )
            .fetch_optional(&self.pool)
            .await
            .expect("history lookup should succeed")
            .map(|row| OperatorStrategyAdoptionHistoryRow {
                adoption_id: row.get("adoption_id"),
                action_kind: row.get("action_kind"),
                operator_strategy_revision: row.get("operator_strategy_revision"),
                previous_operator_strategy_revision: row.get("previous_operator_strategy_revision"),
                adoptable_strategy_revision: row.get("adoptable_strategy_revision"),
                strategy_candidate_revision: row.get("strategy_candidate_revision"),
                adopted_at: row.get("adopted_at"),
            })
        })
    }

    pub fn history_count(&self) -> i64 {
        self.runtime.block_on(async {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM operator_target_adoption_history")
                .fetch_one(&self.pool)
                .await
                .expect("history count should load")
        })
    }

    pub fn runtime_progress(&self) -> Option<RuntimeProgressRow> {
        self.runtime.block_on(async {
            RuntimeProgressRepo
                .current(&self.pool)
                .await
                .expect("runtime progress lookup should succeed")
        })
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
}

fn schema_scoped_database_url(database_url: &str, schema: &str) -> String {
    if let Some((base, query)) = database_url.split_once('?') {
        let mut params: Vec<String> = query
            .split('&')
            .filter(|entry| !entry.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        params.push(format!("options=-csearch_path%3D{schema}"));
        format!("{base}?{}", params.join("&"))
    } else {
        format!("{database_url}?options=-csearch_path%3D{schema}")
    }
}
