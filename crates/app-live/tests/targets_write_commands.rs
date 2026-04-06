use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

use app_live::commands::targets::state::normalize_active_operator_strategy_revision;
use persistence::{
    models::{
        AdoptableStrategyRevisionRow, AdoptableTargetRevisionRow, CandidateAdoptionProvenanceRow,
        CandidateTargetSetRow, OperatorStrategyAdoptionHistoryRow, StrategyAdoptionProvenanceRow,
        StrategyCandidateSetRow,
    },
    run_migrations, CandidateAdoptionRepo, CandidateArtifactRepo,
    OperatorTargetAdoptionHistoryRepo, RuntimeProgressRepo, StrategyAdoptionRepo,
    StrategyControlArtifactRepo,
};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{postgres::PgPoolOptions, PgPool, Row};

static NEXT_SCHEMA_ID: AtomicU64 = AtomicU64::new(1);

const MINIMAL_TARGET_SOURCE_CONFIG: &str = r#"
[runtime]
mode = "live"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-7"
"#;

const MINIMAL_TARGET_SOURCE_CONFIG_REV_9: &str = r#"
[runtime]
mode = "live"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"
"#;

const LEGACY_EXPLICIT_LIVE_CONFIG: &str = r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[polymarket.source]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://gamma-api.polymarket.com"
relayer_host = "https://relayer-v2.polymarket.com"
market_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
user_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60

[polymarket.signer]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key-1"
passphrase = "poly-passphrase-1"
timestamp = "1700000000"
signature = "poly-signature-1"

[polymarket.relayer_auth]
kind = "builder_api_key"
api_key = "builder-api-key-1"
timestamp = "1700000001"
passphrase = "builder-passphrase-1"
signature = "builder-signature-1"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-a"]

[[negrisk.targets]]
family_id = "family-a"

[[negrisk.targets.members]]
condition_id = "condition-1"
token_id = "token-1"
price = "0.43"
quantity = "5"
"#;

#[test]
fn targets_adopt_requires_exactly_one_selector_flag() {
    let config = temp_config(MINIMAL_TARGET_SOURCE_CONFIG);

    let output = Command::new(app_live_binary())
        .arg("targets")
        .arg("adopt")
        .arg("--config")
        .arg(&config)
        .output()
        .expect("app-live targets adopt should execute");

    let text = combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(
        text.contains("exactly one of --operator-strategy-revision or --adoptable-revision"),
        "{text}"
    );

    let _ = fs::remove_file(config);
}

#[test]
fn targets_adopt_from_fresh_adoptable_revision_writes_canonical_provenance_and_records_history() {
    let database = TestDatabase::new();
    database.seed_fresh_adoptable_revision_with_active_runtime(
        "adoptable-9",
        "candidate-9",
        "targets-rev-9",
    );
    let config = temp_config(MINIMAL_TARGET_SOURCE_CONFIG);

    let output = Command::new(app_live_binary())
        .arg("targets")
        .arg("adopt")
        .arg("--config")
        .arg(&config)
        .arg("--adoptable-revision")
        .arg("adoptable-9")
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live targets adopt should execute");

    let text = combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("operator_strategy_revision = targets-rev-9"),
        "{text}"
    );
    assert!(
        text.contains("previous_operator_strategy_revision = targets-rev-7"),
        "{text}"
    );
    assert!(text.contains("restart_required = true"), "{text}");

    let rewritten = fs::read_to_string(&config).expect("rewritten config should load");
    assert_rewritten_strategy_control(&rewritten, "targets-rev-9");

    let latest = database.latest_history().expect("history row should exist");
    assert_eq!(latest.action_kind, "adopt");
    assert_eq!(latest.operator_strategy_revision, "targets-rev-9");
    assert_eq!(
        latest.previous_operator_strategy_revision.as_deref(),
        Some("targets-rev-7")
    );
    assert_eq!(
        latest.adoptable_strategy_revision.as_deref(),
        Some("adoptable-9")
    );
    assert_eq!(
        latest.strategy_candidate_revision.as_deref(),
        Some("candidate-9")
    );
    assert_eq!(database.history_count(), 1);

    let provenance = database
        .provenance_for("targets-rev-9")
        .expect("provenance lookup should succeed")
        .expect("canonical provenance should be written");
    assert_eq!(provenance.adoptable_strategy_revision, "adoptable-9");
    assert_eq!(provenance.strategy_candidate_revision, "candidate-9");

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn targets_adopt_same_operator_target_revision_preserves_canonical_provenance_and_appends_history()
{
    let database = TestDatabase::new();
    database.seed_adoptable_revision_with_active_runtime(
        "adoptable-7",
        "candidate-7",
        "targets-rev-7",
    );
    database.seed_fresh_adoptable_revision_with_active_runtime(
        "adoptable-9",
        "candidate-9",
        "targets-rev-7",
    );
    let config = temp_config(MINIMAL_TARGET_SOURCE_CONFIG);

    let output = Command::new(app_live_binary())
        .arg("targets")
        .arg("adopt")
        .arg("--config")
        .arg(&config)
        .arg("--adoptable-revision")
        .arg("adoptable-9")
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live targets adopt should execute");

    let text = combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("operator_strategy_revision = targets-rev-7"),
        "{text}"
    );
    assert_eq!(database.history_count(), 1);

    let latest = database.latest_history().expect("history row should exist");
    assert_eq!(latest.action_kind, "adopt");
    assert_eq!(latest.operator_strategy_revision, "targets-rev-7");
    assert_eq!(
        latest.previous_operator_strategy_revision.as_deref(),
        Some("targets-rev-7")
    );
    assert_eq!(
        latest.adoptable_strategy_revision.as_deref(),
        Some("adoptable-9")
    );
    assert_eq!(
        latest.strategy_candidate_revision.as_deref(),
        Some("candidate-9")
    );

    let provenance = database
        .provenance_for("targets-rev-7")
        .expect("provenance lookup should succeed")
        .expect("canonical provenance should remain available");
    assert_eq!(provenance.adoptable_strategy_revision, "adoptable-7");
    assert_eq!(provenance.strategy_candidate_revision, "candidate-7");

    let rewritten = fs::read_to_string(&config).expect("rewritten config should load");
    assert_rewritten_strategy_control(&rewritten, "targets-rev-7");

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn targets_adopt_allows_direct_operator_target_revision_from_history_lineage() {
    let database = TestDatabase::new();
    database.seed_history_only_revision("adoptable-7", "candidate-7", "targets-rev-7");
    let config = temp_config(MINIMAL_TARGET_SOURCE_CONFIG);

    let output = Command::new(app_live_binary())
        .arg("targets")
        .arg("adopt")
        .arg("--config")
        .arg(&config)
        .arg("--operator-target-revision")
        .arg("targets-rev-7")
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live targets adopt should execute");

    let text = combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("operator_strategy_revision = targets-rev-7"),
        "{text}"
    );
    assert!(
        text.contains("previous_operator_strategy_revision = targets-rev-7"),
        "{text}"
    );

    let latest = database.latest_history().expect("history row should exist");
    assert_eq!(database.history_count(), 2);
    assert_eq!(latest.action_kind, "adopt");
    assert_eq!(latest.operator_strategy_revision, "targets-rev-7");
    assert_eq!(
        latest.previous_operator_strategy_revision.as_deref(),
        Some("targets-rev-7")
    );
    assert_eq!(
        latest.adoptable_strategy_revision.as_deref(),
        Some("adoptable-7")
    );
    assert_eq!(
        latest.strategy_candidate_revision.as_deref(),
        Some("candidate-7")
    );

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn targets_adopt_allows_direct_operator_target_revision_after_rollback_row_removes_latest_lineage()
{
    let database = TestDatabase::new();
    database.seed_history_only_revision("adoptable-7", "candidate-7", "targets-rev-7");
    database.append_rollback_history_row("targets-rev-7", Some("targets-rev-9"));
    let config = temp_config(MINIMAL_TARGET_SOURCE_CONFIG_REV_9);

    let output = Command::new(app_live_binary())
        .arg("targets")
        .arg("adopt")
        .arg("--config")
        .arg(&config)
        .arg("--operator-target-revision")
        .arg("targets-rev-7")
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live targets adopt should execute");

    let text = combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("operator_strategy_revision = targets-rev-7"),
        "{text}"
    );
    assert!(
        text.contains("previous_operator_strategy_revision = targets-rev-9"),
        "{text}"
    );

    let latest = database.latest_history().expect("history row should exist");
    assert_eq!(latest.action_kind, "adopt");
    assert_eq!(latest.operator_strategy_revision, "targets-rev-7");
    assert_eq!(
        latest.previous_operator_strategy_revision.as_deref(),
        Some("targets-rev-9")
    );
    assert_eq!(
        latest.adoptable_strategy_revision.as_deref(),
        Some("adoptable-7")
    );
    assert_eq!(
        latest.strategy_candidate_revision.as_deref(),
        Some("candidate-7")
    );

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn targets_adopt_fails_closed_when_adoptable_revision_lacks_rendered_live_targets() {
    let database = TestDatabase::new();
    database.seed_adoptable_revision_without_rendered_live_targets(
        "adoptable-bad",
        "candidate-bad",
        "targets-rev-bad",
    );
    let config = temp_config(MINIMAL_TARGET_SOURCE_CONFIG);

    let output = Command::new(app_live_binary())
        .arg("targets")
        .arg("adopt")
        .arg("--config")
        .arg(&config)
        .arg("--adoptable-revision")
        .arg("adoptable-bad")
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live targets adopt should execute");

    let text = combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("rendered_live_targets"), "{text}");

    let rewritten = fs::read_to_string(&config).expect("config should still load");
    assert!(
        rewritten.contains("operator_target_revision = \"targets-rev-7\""),
        "{rewritten}"
    );
    let latest = database.latest_history();
    assert!(latest.is_none());

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn targets_adopt_rejects_invalid_route_artifacts_in_neutral_adoptable_payload() {
    let database = TestDatabase::new();
    database.seed_neutral_adoptable_revision_with_invalid_route_artifacts(
        "adoptable-neutral-bad",
        "strategy-candidate-bad",
        "strategy-rev-bad",
    );
    let config = temp_config(MINIMAL_TARGET_SOURCE_CONFIG);

    let output = Command::new(app_live_binary())
        .arg("targets")
        .arg("adopt")
        .arg("--config")
        .arg(&config)
        .arg("--adoptable-revision")
        .arg("adoptable-neutral-bad")
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live targets adopt should execute");

    let text = combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("route_artifacts"), "{text}");

    let rewritten = fs::read_to_string(&config).expect("config should still load");
    assert!(
        rewritten.contains("operator_target_revision = \"targets-rev-7\""),
        "{rewritten}"
    );
    assert!(database.latest_history().is_none());

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn targets_adopt_migrates_legacy_explicit_config_into_first_neutral_revision() {
    let database = TestDatabase::new();
    let config = database.legacy_explicit_config_path();

    let output = Command::new(app_live_binary())
        .arg("targets")
        .arg("adopt")
        .arg("--config")
        .arg(&config)
        .arg("--adopt-compatibility")
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live targets adopt should execute");

    let text = combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("operator_strategy_revision = strategy-rev-"), "{text}");
    assert!(text.contains("migration_source = legacy-explicit"), "{text}");

    let operator_strategy_revision = text
        .lines()
        .find_map(|line| {
            line.strip_prefix("operator_strategy_revision = ")
                .map(str::to_owned)
        })
        .expect("migration should print the synthetic operator strategy revision");
    let rewritten = fs::read_to_string(&config).expect("rewritten config should load");
    assert_rewritten_strategy_control(&rewritten, &operator_strategy_revision);

    let latest = database.latest_history().expect("history row should exist");
    let expected_adoptable_revision = format!(
        "adoptable-strategy-{}",
        operator_strategy_revision.trim_start_matches("strategy-rev-")
    );
    assert_eq!(latest.action_kind, "adopt");
    assert_eq!(latest.operator_strategy_revision, operator_strategy_revision);
    assert_eq!(
        latest.adoptable_strategy_revision.as_deref(),
        Some(expected_adoptable_revision.as_str())
    );

    let provenance = database
        .provenance_for(&operator_strategy_revision)
        .expect("provenance lookup should succeed")
        .expect("canonical provenance should be written");
    assert_eq!(provenance.operator_strategy_revision, operator_strategy_revision);

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn targets_adopt_compatibility_treats_matching_legacy_runtime_digest_as_no_restart() {
    let database = TestDatabase::new();
    database.seed_legacy_explicit_runtime_progress();
    let config = database.legacy_explicit_config_path();
    let expected_strategy_revision = legacy_explicit_strategy_revision();
    assert_eq!(
        normalize_active_operator_strategy_revision(
            Some(expected_strategy_revision.as_str()),
            Some(legacy_explicit_operator_target_revision().as_str()),
            None,
        )
        .as_deref(),
        Some(expected_strategy_revision.as_str())
    );

    let output = Command::new(app_live_binary())
        .arg("targets")
        .arg("adopt")
        .arg("--config")
        .arg(&config)
        .arg("--adopt-compatibility")
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live targets adopt should execute");

    let text = combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("restart_required = false"), "{text}");

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn targets_rollback_rejects_compatibility_mode_without_neutral_history() {
    let database = TestDatabase::new();
    let config = database.legacy_explicit_config_path();

    let output = Command::new(app_live_binary())
        .arg("targets")
        .arg("rollback")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live targets rollback should execute");

    let text = combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("neutral adoption history"), "{text}");

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn targets_rollback_allows_compatibility_mode_after_neutral_history_exists() {
    let database = TestDatabase::new();
    database.seed_history_only_revision("adoptable-8", "candidate-8", "targets-rev-8");

    let seed_config = database.legacy_explicit_config_path();
    let seed_output = Command::new(app_live_binary())
        .arg("targets")
        .arg("adopt")
        .arg("--config")
        .arg(&seed_config)
        .arg("--adopt-compatibility")
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live targets adopt should execute");
    assert!(seed_output.status.success(), "{}", combined(&seed_output));

    let config = database.legacy_explicit_config_path();
    let output = Command::new(app_live_binary())
        .arg("targets")
        .arg("rollback")
        .arg("--config")
        .arg(&config)
        .arg("--to-operator-target-revision")
        .arg("targets-rev-8")
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live targets rollback should execute");

    let text = combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("operator_strategy_revision = targets-rev-8"),
        "{text}"
    );
    assert!(text.contains("previous_operator_strategy_revision = strategy-rev-"), "{text}");
    assert!(text.contains("restart_required = false"), "{text}");

    let rewritten = fs::read_to_string(&config).expect("rewritten config should load");
    assert_rewritten_strategy_control(&rewritten, "targets-rev-8");

    database.cleanup();
    let _ = fs::remove_file(seed_config);
    let _ = fs::remove_file(config);
}

#[test]
fn targets_rollback_defaults_to_previous_distinct_revision() {
    let database = TestDatabase::new();
    database.seed_rollback_history();
    let config = temp_config(MINIMAL_TARGET_SOURCE_CONFIG_REV_9);

    let output = Command::new(app_live_binary())
        .arg("targets")
        .arg("rollback")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live targets rollback should execute");

    let text = combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("operator_strategy_revision = targets-rev-8"),
        "{text}"
    );
    assert!(
        text.contains("previous_operator_strategy_revision = targets-rev-9"),
        "{text}"
    );
    assert!(text.contains("restart_required = true"), "{text}");

    let rewritten = fs::read_to_string(&config).expect("rewritten config should load");
    assert_rewritten_strategy_control(&rewritten, "targets-rev-8");

    let latest = database.latest_history().expect("history row should exist");
    assert_eq!(latest.action_kind, "rollback");
    assert_eq!(latest.operator_strategy_revision, "targets-rev-8");
    assert_eq!(
        latest.previous_operator_strategy_revision.as_deref(),
        Some("targets-rev-9")
    );
    assert_eq!(latest.adoptable_strategy_revision, None);
    assert_eq!(latest.strategy_candidate_revision, None);

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn targets_rollback_to_explicit_operator_target_revision_rewrites_config_and_records_history() {
    let database = TestDatabase::new();
    database.seed_rollback_history();
    let config = temp_config(MINIMAL_TARGET_SOURCE_CONFIG_REV_9);

    let output = Command::new(app_live_binary())
        .arg("targets")
        .arg("rollback")
        .arg("--config")
        .arg(&config)
        .arg("--to-operator-target-revision")
        .arg("targets-rev-8")
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live targets rollback should execute");

    let text = combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("operator_strategy_revision = targets-rev-8"),
        "{text}"
    );
    assert!(
        text.contains("previous_operator_strategy_revision = targets-rev-9"),
        "{text}"
    );
    assert!(text.contains("restart_required = true"), "{text}");

    let rewritten = fs::read_to_string(&config).expect("rewritten config should load");
    assert_rewritten_strategy_control(&rewritten, "targets-rev-8");

    let latest = database.latest_history().expect("history row should exist");
    assert_eq!(latest.action_kind, "rollback");
    assert_eq!(latest.operator_strategy_revision, "targets-rev-8");
    assert_eq!(
        latest.previous_operator_strategy_revision.as_deref(),
        Some("targets-rev-9")
    );
    assert_eq!(latest.adoptable_strategy_revision, None);
    assert_eq!(latest.strategy_candidate_revision, None);

    database.cleanup();
    let _ = fs::remove_file(config);
}

struct TestDatabase {
    runtime: tokio::runtime::Runtime,
    admin_pool: PgPool,
    pool: PgPool,
    schema: String,
    database_url: String,
}

impl TestDatabase {
    fn new() -> Self {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build");

        let (admin_pool, pool, schema, database_url) = runtime.block_on(async {
            let admin_database_url = std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| default_test_database_url().to_owned());
            let admin_pool = PgPoolOptions::new()
                .max_connections(8)
                .connect(&admin_database_url)
                .await
                .expect("test database should connect");
            let schema = format!(
                "app_live_targets_write_{}_{}",
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

    fn database_url(&self) -> &str {
        &self.database_url
    }

    fn legacy_explicit_config_path(&self) -> PathBuf {
        temp_config(LEGACY_EXPLICIT_LIVE_CONFIG)
    }

    fn latest_history(&self) -> Option<OperatorStrategyAdoptionHistoryRow> {
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

    fn history_count(&self) -> i64 {
        self.runtime.block_on(async {
            sqlx::query_scalar::<_, i64>(
                r#"
                SELECT COUNT(*)
                FROM (
                    SELECT adoption_id FROM operator_strategy_adoption_history
                    UNION ALL
                    SELECT adoption_id FROM operator_target_adoption_history
                ) AS combined_history
                "#,
            )
                .fetch_one(&self.pool)
                .await
                .expect("history count should succeed")
        })
    }

    fn provenance_for(
        &self,
        operator_target_revision: &str,
    ) -> persistence::Result<Option<StrategyAdoptionProvenanceRow>> {
        self.runtime.block_on(async {
            StrategyAdoptionRepo
                .get_by_operator_strategy_revision(&self.pool, operator_target_revision)
                .await
        })
    }

    fn seed_adoptable_revision_with_active_runtime(
        &self,
        adoptable_revision: &str,
        candidate_revision: &str,
        operator_target_revision: &str,
    ) {
        self.runtime.block_on(async {
            self.seed_adoptable_artifacts(
                adoptable_revision,
                candidate_revision,
                operator_target_revision,
            )
            .await;
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
                .expect("adoption provenance should seed");
            RuntimeProgressRepo
                .record_progress(
                    &self.pool,
                    41,
                    7,
                    Some("snapshot-7"),
                    Some("targets-rev-7"),
                    None,
                )
                .await
                .expect("runtime progress should seed");
        });
    }

    fn seed_fresh_adoptable_revision_with_active_runtime(
        &self,
        adoptable_revision: &str,
        candidate_revision: &str,
        operator_target_revision: &str,
    ) {
        self.runtime.block_on(async {
            self.seed_adoptable_artifacts(
                adoptable_revision,
                candidate_revision,
                operator_target_revision,
            )
            .await;
            RuntimeProgressRepo
                .record_progress(
                    &self.pool,
                    41,
                    7,
                    Some("snapshot-7"),
                    Some("targets-rev-7"),
                    None,
                )
                .await
                .expect("runtime progress should seed");
        });
    }

    fn seed_rollback_history(&self) {
        self.seed_adoptable_revision_with_active_runtime(
            "adoptable-8",
            "candidate-8",
            "targets-rev-8",
        );
        self.seed_adoptable_revision_with_active_runtime(
            "adoptable-9",
            "candidate-9",
            "targets-rev-9",
        );

        self.runtime.block_on(async {
            OperatorTargetAdoptionHistoryRepo
                .append(
                    &self.pool,
                    &persistence::models::OperatorTargetAdoptionHistoryRow {
                        adoption_id: "adoption-8".to_owned(),
                        action_kind: "adopt".to_owned(),
                        operator_target_revision: "targets-rev-8".to_owned(),
                        previous_operator_target_revision: Some("targets-rev-7".to_owned()),
                        adoptable_revision: Some("adoptable-8".to_owned()),
                        candidate_revision: Some("candidate-8".to_owned()),
                        adopted_at: chrono::Utc::now(),
                    },
                )
                .await
                .expect("adoption history should seed");
            OperatorTargetAdoptionHistoryRepo
                .append(
                    &self.pool,
                    &persistence::models::OperatorTargetAdoptionHistoryRow {
                        adoption_id: "adoption-9".to_owned(),
                        action_kind: "adopt".to_owned(),
                        operator_target_revision: "targets-rev-9".to_owned(),
                        previous_operator_target_revision: Some("targets-rev-8".to_owned()),
                        adoptable_revision: Some("adoptable-9".to_owned()),
                        candidate_revision: Some("candidate-9".to_owned()),
                        adopted_at: chrono::Utc::now(),
                    },
                )
                .await
                .expect("adoption history should seed");
            RuntimeProgressRepo
                .record_progress(
                    &self.pool,
                    51,
                    9,
                    Some("snapshot-9"),
                    Some("targets-rev-9"),
                    None,
                )
                .await
                .expect("runtime progress should seed");
        });
    }

    fn seed_legacy_explicit_runtime_progress(&self) {
        self.runtime.block_on(async {
            RuntimeProgressRepo
                .record_progress(
                    &self.pool,
                    61,
                    11,
                    Some("snapshot-compatibility"),
                    Some(&legacy_explicit_operator_target_revision()),
                    None,
                )
                .await
                .expect("runtime progress should seed");
        });
    }

    fn append_rollback_history_row(
        &self,
        operator_target_revision: &str,
        previous_operator_target_revision: Option<&str>,
    ) {
        self.runtime.block_on(async {
            OperatorTargetAdoptionHistoryRepo
                .append(
                    &self.pool,
                    &persistence::models::OperatorTargetAdoptionHistoryRow {
                        adoption_id: format!("rollback-{operator_target_revision}"),
                        action_kind: "rollback".to_owned(),
                        operator_target_revision: operator_target_revision.to_owned(),
                        previous_operator_target_revision: previous_operator_target_revision
                            .map(str::to_owned),
                        adoptable_revision: None,
                        candidate_revision: None,
                        adopted_at: chrono::Utc::now(),
                    },
                )
                .await
                .expect("rollback history should seed");
        });
    }

    fn seed_history_only_revision(
        &self,
        adoptable_revision: &str,
        candidate_revision: &str,
        operator_target_revision: &str,
    ) {
        self.runtime.block_on(async {
            let artifacts = CandidateArtifactRepo;
            artifacts
                .upsert_candidate_target_set(
                    &self.pool,
                    &CandidateTargetSetRow {
                        candidate_revision: candidate_revision.to_owned(),
                        snapshot_id: "snapshot-7".to_owned(),
                        source_revision: "discovery-7".to_owned(),
                        payload: json!({
                            "candidate_revision": candidate_revision,
                        }),
                    },
                )
                .await
                .expect("candidate should seed");
            artifacts
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
                            "rendered_live_targets": sample_rendered_live_targets_json(),
                        }),
                    },
                )
                .await
                .expect("adoptable row should seed");
            OperatorTargetAdoptionHistoryRepo
                .append(
                    &self.pool,
                    &persistence::models::OperatorTargetAdoptionHistoryRow {
                        adoption_id: "history-only-7".to_owned(),
                        action_kind: "adopt".to_owned(),
                        operator_target_revision: operator_target_revision.to_owned(),
                        previous_operator_target_revision: Some("targets-rev-6".to_owned()),
                        adoptable_revision: Some(adoptable_revision.to_owned()),
                        candidate_revision: Some(candidate_revision.to_owned()),
                        adopted_at: chrono::Utc::now(),
                    },
                )
                .await
                .expect("history row should seed");
            RuntimeProgressRepo
                .record_progress(
                    &self.pool,
                    41,
                    7,
                    Some("snapshot-7"),
                    Some(operator_target_revision),
                    None,
                )
                .await
                .expect("runtime progress should seed");
        });
    }

    fn seed_adoptable_revision_without_rendered_live_targets(
        &self,
        adoptable_revision: &str,
        candidate_revision: &str,
        operator_target_revision: &str,
    ) {
        self.runtime.block_on(async {
            let artifacts = CandidateArtifactRepo;
            artifacts
                .upsert_candidate_target_set(
                    &self.pool,
                    &CandidateTargetSetRow {
                        candidate_revision: candidate_revision.to_owned(),
                        snapshot_id: "snapshot-bad".to_owned(),
                        source_revision: "discovery-bad".to_owned(),
                        payload: json!({
                            "candidate_revision": candidate_revision,
                        }),
                    },
                )
                .await
                .expect("candidate should seed");
            artifacts
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
                            "rendered_live_targets": {},
                        }),
                    },
                )
                .await
                .expect("adoptable row should seed");
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
                .expect("provenance should seed");
        });
    }

    fn seed_neutral_adoptable_revision_with_invalid_route_artifacts(
        &self,
        adoptable_revision: &str,
        strategy_candidate_revision: &str,
        operator_strategy_revision: &str,
    ) {
        self.runtime.block_on(async {
            let artifacts = StrategyControlArtifactRepo;
            artifacts
                .upsert_strategy_candidate_set(
                    &self.pool,
                    &StrategyCandidateSetRow {
                        strategy_candidate_revision: strategy_candidate_revision.to_owned(),
                        snapshot_id: "snapshot-neutral-bad".to_owned(),
                        source_revision: "discovery-neutral-bad".to_owned(),
                        payload: json!({
                            "strategy_candidate_revision": strategy_candidate_revision,
                        }),
                    },
                )
                .await
                .expect("strategy candidate should seed");
            artifacts
                .upsert_adoptable_strategy_revision(
                    &self.pool,
                    &AdoptableStrategyRevisionRow {
                        adoptable_strategy_revision: adoptable_revision.to_owned(),
                        strategy_candidate_revision: strategy_candidate_revision.to_owned(),
                        rendered_operator_strategy_revision: operator_strategy_revision.to_owned(),
                        payload: json!({
                            "route_artifacts": [
                                {
                                    "key": {
                                        "route": "neg-risk"
                                    }
                                }
                            ]
                        }),
                    },
                )
                .await
                .expect("neutral adoptable should seed");
        });
    }

    fn cleanup(self) {
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

    async fn seed_adoptable_artifacts(
        &self,
        adoptable_revision: &str,
        candidate_revision: &str,
        operator_target_revision: &str,
    ) {
        let artifacts = CandidateArtifactRepo;
        artifacts
            .upsert_candidate_target_set(
                &self.pool,
                &CandidateTargetSetRow {
                    candidate_revision: candidate_revision.to_owned(),
                    snapshot_id: "snapshot-9".to_owned(),
                    source_revision: "discovery-9".to_owned(),
                    payload: json!({
                        "candidate_revision": candidate_revision,
                    }),
                },
            )
            .await
            .expect("candidate should seed");
        artifacts
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
                        "rendered_live_targets": sample_rendered_live_targets_json(),
                    }),
                },
            )
            .await
            .expect("adoptable row should seed");
    }
}

fn temp_config(contents: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "app-live-targets-write-{}-{}.toml",
        std::process::id(),
        NEXT_SCHEMA_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, contents).expect("temporary config should write");
    path
}

fn assert_rewritten_strategy_control(text: &str, operator_strategy_revision: &str) {
    assert!(text.contains("[strategy_control]"), "{text}");
    assert!(
        text.contains(&format!(
            "operator_strategy_revision = \"{operator_strategy_revision}\""
        )),
        "{text}"
    );
    assert!(!text.contains("operator_target_revision ="), "{text}");
}

fn schema_scoped_database_url(base: &str, schema: &str) -> String {
    let options = format!("options=-csearch_path%3D{schema}");
    if base.contains('?') {
        format!("{base}&{options}")
    } else {
        format!("{base}?{options}")
    }
}

fn app_live_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("CARGO_BIN_EXE_app-live") {
        return PathBuf::from(path);
    }

    let mut path = std::env::current_exe().expect("current test executable path");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("app-live");
    if cfg!(windows) {
        path.set_extension("exe");
    }

    path
}

fn legacy_explicit_operator_target_revision() -> String {
    let canonical = CanonicalNegRiskLiveTargetSet {
        families: vec![CanonicalNegRiskFamily {
            family_id: "family-a",
            members: vec![CanonicalNegRiskMember {
                condition_id: "condition-1",
                token_id: "token-1",
                price: "0.43",
                quantity: "5",
            }],
        }],
    };
    let digest =
        Sha256::digest(serde_json::to_vec(&canonical).expect("canonical payload should serialize"));
    format!("sha256:{digest:x}")
}

fn legacy_explicit_strategy_revision() -> String {
    format!(
        "strategy-rev-{}",
        legacy_explicit_operator_target_revision()
            .trim_start_matches("sha256:")
    )
}

#[derive(Serialize)]
struct CanonicalNegRiskLiveTargetSet {
    families: Vec<CanonicalNegRiskFamily>,
}

#[derive(Serialize)]
struct CanonicalNegRiskFamily {
    family_id: &'static str,
    members: Vec<CanonicalNegRiskMember>,
}

#[derive(Serialize)]
struct CanonicalNegRiskMember {
    condition_id: &'static str,
    token_id: &'static str,
    price: &'static str,
    quantity: &'static str,
}

fn default_test_database_url() -> &'static str {
    "postgres://axiom:axiom@localhost:5432/axiom_arb"
}

fn combined(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn sample_rendered_live_targets_json() -> serde_json::Value {
    json!({
        "family-a": {
            "family_id": "family-a",
            "members": [
                {
                    "condition_id": "condition-1",
                    "token_id": "token-1",
                    "price": "0.43",
                    "quantity": "5"
                }
            ]
        }
    })
}
