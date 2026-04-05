#![allow(dead_code)]

use std::{
    borrow::ToOwned,
    env,
    sync::atomic::{AtomicU64, Ordering},
};

use persistence::{
    models::{
        AdoptableTargetRevisionRow, CandidateAdoptionProvenanceRow, CandidateTargetSetRow,
        RunSessionRow, RunSessionState,
    },
    run_migrations, CandidateAdoptionRepo, CandidateArtifactRepo, RunSessionRepo,
    RuntimeProgressRepo,
};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool};

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
                "app_live_status_{}_{}",
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

    pub fn seed_adopted_target_with_active_revision(
        &self,
        operator_target_revision: &str,
        active_operator_target_revision: Option<&str>,
    ) {
        self.runtime.block_on(async {
            CandidateArtifactRepo
                .upsert_candidate_target_set(
                    &self.pool,
                    &CandidateTargetSetRow {
                        candidate_revision: "candidate-9".to_owned(),
                        snapshot_id: "snapshot-9".to_owned(),
                        source_revision: "discovery-9".to_owned(),
                        payload: json!({
                            "candidate_revision": "candidate-9",
                            "snapshot_id": "snapshot-9",
                        }),
                    },
                )
                .await
                .expect("candidate row should persist");

            CandidateArtifactRepo
                .upsert_adoptable_target_revision(
                    &self.pool,
                    &AdoptableTargetRevisionRow {
                        adoptable_revision: "adoptable-9".to_owned(),
                        candidate_revision: "candidate-9".to_owned(),
                        rendered_operator_target_revision: operator_target_revision.to_owned(),
                        payload: json!({
                            "adoptable_revision": "adoptable-9",
                            "candidate_revision": "candidate-9",
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
                        adoptable_revision: "adoptable-9".to_owned(),
                        candidate_revision: "candidate-9".to_owned(),
                    },
                )
                .await
                .expect("candidate provenance should persist");

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

    pub fn seed_adopted_target_without_provenance(&self, operator_target_revision: &str) {
        self.runtime.block_on(async {
            CandidateArtifactRepo
                .upsert_candidate_target_set(
                    &self.pool,
                    &CandidateTargetSetRow {
                        candidate_revision: "candidate-9".to_owned(),
                        snapshot_id: "snapshot-9".to_owned(),
                        source_revision: "discovery-9".to_owned(),
                        payload: json!({
                            "candidate_revision": "candidate-9",
                            "snapshot_id": "snapshot-9",
                        }),
                    },
                )
                .await
                .expect("candidate row should persist");

            CandidateArtifactRepo
                .upsert_adoptable_target_revision(
                    &self.pool,
                    &AdoptableTargetRevisionRow {
                        adoptable_revision: "adoptable-9".to_owned(),
                        candidate_revision: "candidate-9".to_owned(),
                        rendered_operator_target_revision: operator_target_revision.to_owned(),
                        payload: json!({
                            "adoptable_revision": "adoptable-9",
                            "candidate_revision": "candidate-9",
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
        });
    }

    pub fn seed_runtime_progress(
        &self,
        operator_target_revision: Option<&str>,
        active_run_session_id: Option<&str>,
    ) {
        self.runtime.block_on(async {
            RuntimeProgressRepo
                .record_progress(
                    &self.pool,
                    41,
                    7,
                    Some("snapshot-7"),
                    operator_target_revision,
                    None,
                )
                .await
                .expect("runtime progress should seed");

            if let Some(active_run_session_id) = active_run_session_id {
                RuntimeProgressRepo
                    .set_active_run_session_id(&self.pool, active_run_session_id)
                    .await
                    .expect("active run session should seed");
            }
        });
    }

    pub fn seed_run_session(&self, row: RunSessionRow) {
        self.runtime.block_on(async {
            RunSessionRepo
                .create_starting(
                    &self.pool,
                    &RunSessionRow {
                        state: RunSessionState::Starting,
                        ended_at: None,
                        exit_status: None,
                        exit_reason: None,
                        ..row.clone()
                    },
                )
                .await
                .expect("run session should seed");

            match row.state {
                RunSessionState::Starting => {}
                RunSessionState::Running => {
                    RunSessionRepo
                        .mark_running(&self.pool, &row.run_session_id, row.started_at)
                        .await
                        .expect("running session should seed");
                }
                RunSessionState::Exited => {
                    RunSessionRepo
                        .mark_running(&self.pool, &row.run_session_id, row.started_at)
                        .await
                        .expect("running session should seed");
                    RunSessionRepo
                        .mark_exited(
                            &self.pool,
                            &row.run_session_id,
                            row.ended_at.unwrap_or(row.last_seen_at),
                            row.exit_status.as_deref().unwrap_or("success"),
                            row.exit_reason.as_deref(),
                        )
                        .await
                        .expect("exited session should seed");
                }
                RunSessionState::Failed => {
                    RunSessionRepo
                        .mark_running(&self.pool, &row.run_session_id, row.started_at)
                        .await
                        .expect("running session should seed");
                    RunSessionRepo
                        .mark_failed(
                            &self.pool,
                            &row.run_session_id,
                            row.ended_at.unwrap_or(row.last_seen_at),
                            row.exit_reason.as_deref().unwrap_or("seeded failure"),
                        )
                        .await
                        .expect("failed session should seed");
                }
            }
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
