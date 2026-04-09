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
        AdoptableStrategyRevisionRow, AdoptableTargetRevisionRow, CandidateAdoptionProvenanceRow,
        CandidateTargetSetRow, ExecutionAttemptRow, JournalEntryInput, LiveExecutionArtifactRow,
        LiveSubmissionRecordRow, RunSessionRow, RunSessionState, ShadowExecutionArtifactRow,
        StrategyAdoptionProvenanceRow, StrategyCandidateSetRow,
    },
    run_migrations, CandidateAdoptionRepo, CandidateArtifactRepo, ExecutionAttemptRepo,
    JournalRepo, LiveArtifactRepo, LiveSubmissionRepo, RunSessionRepo, RuntimeProgressRepo,
    ShadowArtifactRepo, StrategyAdoptionRepo, StrategyControlArtifactRepo,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
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
                        None,
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
                    None,
                )
                .await
                .expect("runtime progress should seed");
        });
    }

    pub fn seed_strategy_runtime_progress(&self, operator_strategy_revision: &str) {
        self.runtime.block_on(async {
            RuntimeProgressRepo
                .record_progress_with_strategy_revision(
                    &self.pool,
                    41,
                    7,
                    Some("snapshot-verify-7"),
                    None,
                    Some(operator_strategy_revision),
                    None,
                )
                .await
                .expect("strategy runtime progress should seed");
        });
    }

    pub fn seed_adopted_strategy_revision_with_routes(
        &self,
        operator_strategy_revision: &str,
        include_full_set: bool,
    ) {
        self.runtime.block_on(async {
            let strategy_candidate_revision =
                format!("strategy-candidate-{operator_strategy_revision}");
            let adoptable_strategy_revision = format!("adoptable-{operator_strategy_revision}");
            let mut route_artifacts = Vec::new();
            if include_full_set {
                route_artifacts.push(json!({
                    "key": {
                        "route": "full-set",
                        "scope": "default",
                    },
                    "route_policy_version": "full-set-route-policy-v1",
                    "semantic_digest": "full-set-basis-default",
                    "content": {
                        "config_basis_digest": "full-set-basis-default",
                        "mode": "static-default",
                    },
                }));
            }
            route_artifacts.push(json!({
                "key": {
                    "route": "neg-risk",
                    "scope": "family-a",
                },
                "route_policy_version": "neg-risk-route-policy-v1",
                "semantic_digest": "family-a",
                "content": {
                    "family_id": "family-a",
                    "rendered_live_target": {
                        "family_id": "family-a",
                        "members": [
                            {
                                "condition_id": "condition-1",
                                "token_id": "token-1",
                                "price": "0.43",
                                "quantity": "5",
                            }
                        ]
                    },
                    "target_id": "candidate-target-family-a",
                    "validation": {
                        "status": "adoptable",
                    },
                },
            }));

            StrategyControlArtifactRepo
                .upsert_strategy_candidate_set(
                    &self.pool,
                    &StrategyCandidateSetRow {
                        strategy_candidate_revision: strategy_candidate_revision.clone(),
                        snapshot_id: format!("snapshot-{operator_strategy_revision}"),
                        source_revision: format!("discovery-{operator_strategy_revision}"),
                        payload: json!({
                            "strategy_candidate_revision": strategy_candidate_revision,
                            "snapshot_id": format!("snapshot-{operator_strategy_revision}"),
                        }),
                    },
                )
                .await
                .expect("strategy candidate should seed");

            StrategyControlArtifactRepo
                .upsert_adoptable_strategy_revision(
                    &self.pool,
                    &AdoptableStrategyRevisionRow {
                        adoptable_strategy_revision: adoptable_strategy_revision.clone(),
                        strategy_candidate_revision: strategy_candidate_revision.clone(),
                        rendered_operator_strategy_revision: operator_strategy_revision.to_owned(),
                        payload: json!({
                            "adoptable_strategy_revision": adoptable_strategy_revision,
                            "strategy_candidate_revision": strategy_candidate_revision,
                            "rendered_operator_strategy_revision": operator_strategy_revision,
                            "route_artifacts": route_artifacts,
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
                .expect("adoptable strategy should seed");

            StrategyAdoptionRepo
                .upsert_provenance(
                    &self.pool,
                    &StrategyAdoptionProvenanceRow {
                        operator_strategy_revision: operator_strategy_revision.to_owned(),
                        adoptable_strategy_revision,
                        strategy_candidate_revision,
                    },
                )
                .await
                .expect("strategy provenance should seed");
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
                        last_seen_at: row.started_at,
                        ..row.clone()
                    },
                )
                .await
                .expect("run session should seed as starting");

            match row.state {
                RunSessionState::Starting => {}
                RunSessionState::Running => {
                    RunSessionRepo
                        .mark_running(&self.pool, &row.run_session_id, row.last_seen_at)
                        .await
                        .expect("run session should transition to running");
                }
                RunSessionState::Exited => {
                    RunSessionRepo
                        .mark_exited(
                            &self.pool,
                            &row.run_session_id,
                            row.ended_at.unwrap_or(row.last_seen_at),
                            row.exit_status.as_deref().unwrap_or("success"),
                            row.exit_reason.as_deref(),
                        )
                        .await
                        .expect("run session should transition to exited");
                }
                RunSessionState::Failed => {
                    RunSessionRepo
                        .mark_failed(
                            &self.pool,
                            &row.run_session_id,
                            row.ended_at.unwrap_or(row.last_seen_at),
                            row.exit_reason.as_deref().unwrap_or("failed"),
                        )
                        .await
                        .expect("run session should transition to failed");
                }
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn sample_run_session(
        &self,
        run_session_id: &str,
        invoked_by: &str,
        mode: &str,
        config_path: &std::path::Path,
        target_source_kind: &str,
        startup_target_revision_at_start: &str,
        configured_operator_target_revision: Option<&str>,
        active_operator_target_revision_at_start: Option<&str>,
        rollout_state_at_start: Option<&str>,
        real_user_shadow_smoke: bool,
        state: RunSessionState,
        started_at: DateTime<Utc>,
        last_seen_at: DateTime<Utc>,
    ) -> RunSessionRow {
        RunSessionRow {
            run_session_id: run_session_id.to_owned(),
            invoked_by: invoked_by.to_owned(),
            mode: mode.to_owned(),
            state,
            started_at,
            last_seen_at,
            ended_at: None,
            exit_status: None,
            exit_reason: None,
            config_path: config_path.display().to_string(),
            config_fingerprint: config_fingerprint(config_path),
            target_source_kind: target_source_kind.to_owned(),
            startup_target_revision_at_start: startup_target_revision_at_start.to_owned(),
            configured_operator_target_revision: configured_operator_target_revision
                .map(ToOwned::to_owned),
            active_operator_target_revision_at_start: active_operator_target_revision_at_start
                .map(ToOwned::to_owned),
            configured_operator_strategy_revision: None,
            active_operator_strategy_revision_at_start: None,
            rollout_state_at_start: rollout_state_at_start.map(ToOwned::to_owned),
            real_user_shadow_smoke,
        }
    }

    pub fn seed_live_attempt(&self, attempt_id: &str) {
        self.seed_attempt(sample_attempt(attempt_id, ExecutionMode::Live));
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

    pub fn seed_live_attempt_with_artifacts(&self, attempt_id: &str) {
        self.seed_live_attempt(attempt_id);
        self.seed_live_artifact(sample_live_artifact(attempt_id));
        self.seed_live_submission(sample_live_submission(
            attempt_id,
            &format!("{attempt_id}-submission"),
        ));
    }

    pub fn seed_live_attempt_with_artifacts_for_run_session(
        &self,
        attempt_id: &str,
        run_session_id: &str,
    ) {
        let mut attempt = sample_attempt(attempt_id, ExecutionMode::Live);
        attempt.run_session_id = Some(run_session_id.to_owned());
        self.seed_attempt(attempt);
        self.seed_live_artifact(sample_live_artifact(attempt_id));
        self.seed_live_submission(sample_live_submission(
            attempt_id,
            &format!("{attempt_id}-submission"),
        ));
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

pub fn live_ready_strategy_config() -> String {
    live_ready_strategy_config_for("strategy-rev-12")
}

pub fn smoke_ready_strategy_config_for(operator_strategy_revision: &str) -> String {
    format!(
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

[polymarket.source_overrides]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://gamma-api.polymarket.com"
relayer_host = "https://relayer-v2.polymarket.com"
market_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
user_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60

[strategy_control]
source = "adopted"
operator_strategy_revision = "{operator_strategy_revision}"

[strategies.full_set]
enabled = true

[strategies.neg_risk]
enabled = true

[strategies.neg_risk.rollout]
approved_scopes = ["family-a"]
ready_scopes = ["family-a"]
"#
    )
}

pub fn live_ready_strategy_config_for(operator_strategy_revision: &str) -> String {
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

[strategy_control]
source = "adopted"
operator_strategy_revision = "{operator_strategy_revision}"

[strategies.full_set]
enabled = true

[strategies.neg_risk]
enabled = true

[strategies.neg_risk.rollout]
approved_scopes = ["family-a"]
ready_scopes = ["family-a"]
"#
    )
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

[negrisk.target_source]
source = "adopted"
operator_target_revision = "{operator_target_revision}"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-a"]
"#
    )
}

pub fn live_rollout_required_config_for(operator_target_revision: &str) -> String {
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

[negrisk.target_source]
source = "adopted"
operator_target_revision = "{operator_target_revision}"

[negrisk.rollout]
approved_families = []
ready_families = []
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

[polymarket.source_overrides]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://gamma-api.polymarket.com"
relayer_host = "https://relayer-v2.polymarket.com"
market_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
user_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"

[negrisk.rollout]
approved_families = []
ready_families = []
"#
    .to_owned()
}

pub fn smoke_ready_config() -> String {
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

[polymarket.source_overrides]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://gamma-api.polymarket.com"
relayer_host = "https://relayer-v2.polymarket.com"
market_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
user_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-a"]
"#
    .to_owned()
}

pub mod config_shapes {
    pub fn live_ready_config() -> String {
        super::live_ready_config()
    }

    pub fn live_ready_strategy_config() -> String {
        super::live_ready_strategy_config()
    }

    pub fn smoke_ready_strategy_config_for(operator_strategy_revision: &str) -> String {
        super::smoke_ready_strategy_config_for(operator_strategy_revision)
    }

    pub fn live_ready_strategy_config_for(operator_strategy_revision: &str) -> String {
        super::live_ready_strategy_config_for(operator_strategy_revision)
    }

    pub fn live_ready_config_for(operator_target_revision: &str) -> String {
        super::live_ready_config_for(operator_target_revision)
    }

    pub fn live_rollout_required_config_for(operator_target_revision: &str) -> String {
        super::live_rollout_required_config_for(operator_target_revision)
    }

    pub fn smoke_rollout_required_config() -> String {
        super::smoke_rollout_required_config()
    }

    pub fn smoke_ready_config() -> String {
        super::smoke_ready_config()
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
    sample_attempt_for_route(attempt_id, execution_mode, "neg-risk")
}

pub fn sample_attempt_for_route(
    attempt_id: &str,
    execution_mode: ExecutionMode,
    route: &str,
) -> ExecutionAttemptRow {
    ExecutionAttemptRow {
        attempt_id: attempt_id.to_owned(),
        plan_id: "negrisk-submit-family:family-a".to_owned(),
        snapshot_id: "snapshot-verify-7".to_owned(),
        route: route.to_owned(),
        scope: "family-a".to_owned(),
        matched_rule_id: Some("rule-neg-risk-submit".to_owned()),
        execution_mode,
        attempt_no: 1,
        idempotency_key: format!("idem-{attempt_id}"),
        run_session_id: None,
    }
}

pub fn sample_attempt_in_snapshot(
    attempt_id: &str,
    execution_mode: ExecutionMode,
    snapshot_id: &str,
) -> ExecutionAttemptRow {
    let mut row = sample_attempt(attempt_id, execution_mode);
    row.snapshot_id = snapshot_id.to_owned();
    row
}

pub fn sample_attempt_in_snapshot_for_route(
    attempt_id: &str,
    execution_mode: ExecutionMode,
    snapshot_id: &str,
    route: &str,
) -> ExecutionAttemptRow {
    let mut row = sample_attempt_for_route(attempt_id, execution_mode, route);
    row.snapshot_id = snapshot_id.to_owned();
    row
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

impl TestDatabase {
    pub fn seed_smoke_runtime_progress(&self, operator_target_revision: &str) {
        self.seed_adopted_target_with_active_revision(
            operator_target_revision,
            Some(operator_target_revision),
        );
    }

    pub fn seed_shadow_attempt_with_artifacts(&self, attempt_id: &str) {
        self.seed_attempt(sample_attempt(attempt_id, ExecutionMode::Shadow));
        self.seed_shadow_artifact(sample_shadow_artifact(attempt_id));
    }

    pub fn seed_shadow_attempt_with_artifacts_for_run_session(
        &self,
        attempt_id: &str,
        run_session_id: &str,
    ) {
        let mut attempt = sample_attempt(attempt_id, ExecutionMode::Shadow);
        attempt.run_session_id = Some(run_session_id.to_owned());
        self.seed_attempt(attempt);
        self.seed_shadow_artifact(sample_shadow_artifact(attempt_id));
    }

    pub fn seed_shadow_attempt_with_artifacts_in_snapshot(
        &self,
        attempt_id: &str,
        snapshot_id: &str,
    ) {
        self.seed_attempt(sample_attempt_in_snapshot(
            attempt_id,
            ExecutionMode::Shadow,
            snapshot_id,
        ));
        self.seed_shadow_artifact(sample_shadow_artifact(attempt_id));
    }

    pub fn seed_non_working_smoke_run_window(&self) {
        self.seed_attempt(sample_attempt(
            "attempt-shadow-preflight-1",
            ExecutionMode::Shadow,
        ));
    }

    pub fn seed_non_working_smoke_run_window_in_snapshot(&self, snapshot_id: &str) {
        self.seed_attempt(sample_attempt_in_snapshot(
            "attempt-shadow-preflight-1",
            ExecutionMode::Shadow,
            snapshot_id,
        ));
    }

    pub fn seed_live_attempt_in_snapshot(&self, attempt_id: &str, snapshot_id: &str) {
        self.seed_attempt(sample_attempt_in_snapshot(
            attempt_id,
            ExecutionMode::Live,
            snapshot_id,
        ));
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

fn config_fingerprint(config_path: &std::path::Path) -> String {
    let raw = fs::read(config_path).expect("config should be readable");
    format!("{:x}", Sha256::digest(raw))
}
