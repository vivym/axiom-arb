#![allow(dead_code)]

use std::{
    env, fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use chrono::{DateTime, Utc};
use domain::ExecutionMode;
use persistence::{
    models::{
        AdoptableTargetRevisionRow, CandidateAdoptionProvenanceRow, CandidateTargetSetRow,
        ExecutionAttemptRow, JournalEntryInput, LiveExecutionArtifactRow, LiveSubmissionRecordRow,
        ShadowExecutionArtifactRow,
    },
    run_migrations, CandidateAdoptionRepo, CandidateArtifactRepo, ExecutionAttemptRepo,
    JournalRepo, LiveArtifactRepo, LiveSubmissionRepo, RuntimeProgressRepo, ShadowArtifactRepo,
};
use serde_json::{json, Value};
use sqlx::{postgres::PgPoolOptions, PgPool};

use super::cli::default_test_database_url;

static NEXT_SCHEMA_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_TEMP_CONFIG_ID: AtomicU64 = AtomicU64::new(1);

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
                "app_live_verify_{}_{}",
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
                        candidate_revision: "candidate-verify-9".to_owned(),
                        snapshot_id: "snapshot-verify-9".to_owned(),
                        source_revision: "discovery-verify-9".to_owned(),
                        payload: json!({
                            "candidate_revision": "candidate-verify-9",
                            "snapshot_id": "snapshot-verify-9",
                        }),
                    },
                )
                .await
                .expect("candidate row should persist");

            CandidateArtifactRepo
                .upsert_adoptable_target_revision(
                    &self.pool,
                    &AdoptableTargetRevisionRow {
                        adoptable_revision: "adoptable-verify-9".to_owned(),
                        candidate_revision: "candidate-verify-9".to_owned(),
                        rendered_operator_target_revision: operator_target_revision.to_owned(),
                        payload: json!({
                            "adoptable_revision": "adoptable-verify-9",
                            "candidate_revision": "candidate-verify-9",
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
                        adoptable_revision: "adoptable-verify-9".to_owned(),
                        candidate_revision: "candidate-verify-9".to_owned(),
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
                        Some("snapshot-verify-7"),
                        Some(active_operator_target_revision),
                    )
                    .await
                    .expect("runtime progress should seed");
            }
        });
    }

    pub fn seed_runtime_progress(
        &self,
        last_journal_seq: i64,
        last_state_version: i64,
        last_snapshot_id: Option<&str>,
        operator_target_revision: Option<&str>,
    ) {
        self.runtime.block_on(async {
            RuntimeProgressRepo
                .record_progress(
                    &self.pool,
                    last_journal_seq,
                    last_state_version,
                    last_snapshot_id,
                    operator_target_revision,
                )
                .await
                .expect("runtime progress should seed");
        });
    }

    pub fn seed_attempt(&self, row: ExecutionAttemptRow) {
        self.runtime.block_on(async {
            ExecutionAttemptRepo
                .append(&self.pool, &row)
                .await
                .expect("execution attempt should seed");
        });
    }

    pub fn seed_shadow_artifact(&self, row: ShadowExecutionArtifactRow) {
        self.runtime.block_on(async {
            ShadowArtifactRepo
                .append(&self.pool, row)
                .await
                .expect("shadow artifact should seed");
        });
    }

    pub fn seed_live_artifact(&self, row: LiveExecutionArtifactRow) {
        self.runtime.block_on(async {
            LiveArtifactRepo
                .append(&self.pool, row)
                .await
                .expect("live artifact should seed");
        });
    }

    pub fn seed_live_submission(&self, row: LiveSubmissionRecordRow) {
        self.runtime.block_on(async {
            LiveSubmissionRepo
                .append(&self.pool, row)
                .await
                .expect("live submission should seed");
        });
    }

    pub fn seed_journal(&self, row: JournalEntryInput) {
        self.runtime.block_on(async {
            JournalRepo
                .append(&self.pool, &row)
                .await
                .expect("journal row should seed");
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

pub fn live_ready_config() -> String {
    live_ready_config_for("targets-rev-9")
}

pub fn live_ready_config_for(operator_target_revision: &str) -> String {
    format!(
        r#"[runtime]
mode = "live"
real_user_shadow_smoke = false

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key"
secret = "poly-secret"
passphrase = "poly-passphrase"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key"
address = "0x1111111111111111111111111111111111111111"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "{operator_target_revision}"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-a"]
"#
    )
}

pub fn smoke_rollout_required_config() -> String {
    r#"[runtime]
mode = "live"
real_user_shadow_smoke = true

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key"
secret = "poly-secret"
passphrase = "poly-passphrase"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key"
address = "0x1111111111111111111111111111111111111111"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"

[negrisk.rollout]
approved_families = []
ready_families = []
"#
    .to_owned()
}

pub mod config_shapes {
    pub fn live_ready_config() -> String {
        super::live_ready_config()
    }

    pub fn live_ready_config_for(operator_target_revision: &str) -> String {
        super::live_ready_config_for(operator_target_revision)
    }

    pub fn smoke_rollout_required_config() -> String {
        super::smoke_rollout_required_config()
    }
}

pub fn temp_config_path(prefix: &str, contents: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "{prefix}-{}-{}.toml",
        std::process::id(),
        NEXT_TEMP_CONFIG_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, contents).expect("temp config should be writable");
    path
}

pub fn sample_attempt(attempt_id: &str, execution_mode: ExecutionMode) -> ExecutionAttemptRow {
    ExecutionAttemptRow {
        attempt_id: attempt_id.to_owned(),
        plan_id: "negrisk-submit-family:family-a".to_owned(),
        snapshot_id: "snapshot-verify-7".to_owned(),
        route: "neg-risk".to_owned(),
        scope: "family-a".to_owned(),
        matched_rule_id: Some("rule-neg-risk-submit".to_owned()),
        execution_mode,
        attempt_no: 1,
        idempotency_key: format!("idem-{attempt_id}"),
    }
}

pub fn sample_shadow_artifact(attempt_id: &str) -> ShadowExecutionArtifactRow {
    ShadowExecutionArtifactRow {
        attempt_id: attempt_id.to_owned(),
        stream: "negrisk.shadow".to_owned(),
        payload: json!({
            "attempt_id": attempt_id,
            "kind": "shadow_replay",
        }),
    }
}

pub fn sample_live_artifact(attempt_id: &str) -> LiveExecutionArtifactRow {
    LiveExecutionArtifactRow {
        attempt_id: attempt_id.to_owned(),
        stream: "negrisk.live".to_owned(),
        payload: json!({
            "attempt_id": attempt_id,
            "kind": "planned_order",
        }),
    }
}

pub fn sample_live_submission(attempt_id: &str, submission_ref: &str) -> LiveSubmissionRecordRow {
    LiveSubmissionRecordRow {
        submission_ref: submission_ref.to_owned(),
        attempt_id: attempt_id.to_owned(),
        route: "neg-risk".to_owned(),
        scope: "family-a".to_owned(),
        provider: "venue-polymarket".to_owned(),
        state: "submitted".to_owned(),
        payload: json!({
            "submission_ref": submission_ref,
            "family_id": "family-a",
            "route": "neg-risk",
            "reason": "submitted_for_execution",
        }),
    }
}

pub fn sample_journal(
    source_event_id: &str,
    journal_seq_hint: i64,
    event_ts: DateTime<Utc>,
    payload: Value,
) -> JournalEntryInput {
    JournalEntryInput {
        stream: "verify.test".to_owned(),
        source_kind: "integration-test".to_owned(),
        source_session_id: "session-verify-1".to_owned(),
        source_event_id: source_event_id.to_owned(),
        dedupe_key: format!("verify-{journal_seq_hint}-{source_event_id}"),
        causal_parent_id: None,
        event_type: "verify.seeded".to_owned(),
        event_ts,
        payload,
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
