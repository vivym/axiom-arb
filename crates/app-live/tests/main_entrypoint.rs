use config_schema::{load_raw_config_from_path, ValidatedConfig};
use observability::span_names;
use persistence::{
    models::{
        AdoptableStrategyRevisionRow, ExecutionAttemptRow, LiveSubmissionRecordRow,
        RuntimeProgressRow, StrategyAdoptionProvenanceRow, StrategyCandidateSetRow,
    },
    run_migrations, ExecutionAttemptRepo, LiveSubmissionRepo, RuntimeProgressRepo,
    StrategyAdoptionRepo, StrategyControlArtifactRepo,
};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_SCHEMA_ID: AtomicU64 = AtomicU64::new(1);

#[test]
fn binary_entrypoint_reads_paper_mode_from_config_file() {
    let output = app_live_output_with_config("fixtures/app-live-paper.toml");

    assert!(output.status.success());
    assert!(combined(&output).contains("app_mode=paper"));
}

#[test]
fn example_config_mentions_strategy_control_anchor() {
    let text = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("config")
            .join("axiom-arb.example.toml"),
    )
    .expect("example config should load");

    assert!(text.contains("[strategy_control]"), "{text}");
    assert!(text.contains("operator_strategy_revision"), "{text}");
}

#[test]
fn binary_entrypoint_requires_a_subcommand() {
    let output = Command::new(app_live_binary()).output().unwrap();

    assert!(!output.status.success());
    assert!(combined(&output).contains("Usage: app-live <COMMAND>"));
}

#[test]
fn legacy_business_env_vars_alone_do_not_start_app_live() {
    let output = Command::new(app_live_binary())
        .env("AXIOM_MODE", "live")
        .env("DATABASE_URL", default_test_database_url())
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(combined(&output).contains("Usage: app-live <COMMAND>"));
}

#[test]
fn workspace_metadata_no_longer_includes_legacy_config_crate() {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .expect("cargo metadata should run");

    assert!(output.status.success(), "cargo metadata should succeed");

    let stdout = String::from_utf8(output.stdout).expect("metadata should be utf8");
    assert!(
        !stdout.contains("\"name\":\"config\",\"version\""),
        "legacy config crate should not remain in the workspace metadata"
    );
}

#[test]
fn paper_mode_still_requires_database_url_after_config_load() {
    let output = app_live_output_with_config_and_database("fixtures/app-live-paper.toml", None);

    assert!(!output.status.success());
    assert!(combined(&output).contains("DATABASE_URL"));
}

#[test]
fn binary_entrypoint_emits_structured_bootstrap_log_from_config_file() {
    let output = app_live_output_with_config("fixtures/app-live-paper.toml");

    assert!(output.status.success());

    let combined = combined(&output);
    assert!(
        combined.contains(span_names::APP_BOOTSTRAP_COMPLETE),
        "{combined}"
    );
    assert!(
        combined.contains("app-live bootstrap complete"),
        "{combined}"
    );
    assert!(combined.contains("app_mode=paper"), "{combined}");
    assert!(combined.contains("bootstrap_status=Ready"), "{combined}");
}

#[test]
fn live_config_requires_database_url_after_config_parse() {
    let output = app_live_output_with_config_and_database("fixtures/app-live-live.toml", None);

    assert!(!output.status.success());
    assert!(combined(&output).contains("DATABASE_URL"));
}

#[test]
fn live_config_rejects_mismatched_signature_fields_through_binary() {
    let config_path = temp_config_fixture_path("fixtures/app-live-live.toml", |config| {
        config.replace("wallet_route = \"eoa\"", "wallet_route = \"safe\"")
    });
    let output = app_live_output_with_config_path(&config_path, None);

    assert!(!output.status.success());
    assert!(combined(&output).contains("wallet_route"));
}

#[test]
fn smoke_config_surfaces_validated_config_error_before_database_bootstrap() {
    let output = app_live_output_with_config_and_database("fixtures/app-live-smoke.toml", None);

    assert!(!output.status.success());
    assert!(combined(&output).contains("polymarket.account"));
}

#[test]
fn live_config_persists_operator_strategy_revision_anchor_during_startup() {
    let database = TestDatabase::new();
    let revision = ValidatedConfig::new(
        load_raw_config_from_path(&config_fixture_path("fixtures/app-live-live.toml"))
            .expect("config should parse"),
    )
    .expect("config should validate")
    .for_app_live()
    .expect("live view should validate")
    .operator_strategy_revision()
    .expect("live config should define operator_strategy_revision")
    .to_owned();
    database.seed_adopted_strategy_revision_with_routes(&revision);

    let output = app_live_output_with_config_and_database(
        "fixtures/app-live-live.toml",
        Some(database.database_url()),
    );

    assert!(output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert!(combined.contains("negrisk_mode=Live"), "{combined}");
    assert!(
        combined.contains("neg_risk_live_attempt_count=1"),
        "{combined}"
    );

    let progress = database
        .runtime_progress()
        .expect("startup should persist runtime progress");
    assert_eq!(
        progress.operator_strategy_revision.as_deref(),
        Some(revision.as_str())
    );
    assert_eq!(
        progress.operator_target_revision.as_deref(),
        Some(revision.as_str())
    );
    assert_eq!(progress.last_snapshot_id.as_deref(), Some("snapshot-0"));

    database.cleanup();
}

#[test]
fn live_config_fails_when_restored_operator_strategy_revision_anchor_is_stale() {
    let database = TestDatabase::new();
    database.seed_adopted_strategy_revision_with_routes("strategy-rev-12");
    database.seed_live_execution_state("strategy-rev-stale");

    let output = app_live_output_with_config_and_database(
        "fixtures/app-live-live.toml",
        Some(database.database_url()),
    );

    assert!(!output.status.success(), "{}", combined(&output));
    assert!(combined(&output).contains("operator strategy revision anchor mismatch"));

    database.cleanup();
}

fn app_live_output_with_config(config_fixture: &str) -> std::process::Output {
    let database = TestDatabase::new();
    let output =
        app_live_output_with_config_and_database(config_fixture, Some(database.database_url()));
    database.cleanup();
    output
}

fn app_live_output_with_config_and_database(
    config_fixture: &str,
    database_url: Option<&str>,
) -> std::process::Output {
    app_live_output_with_config_path(&config_fixture_path(config_fixture), database_url)
}

fn app_live_output_with_config_path(
    config_path: &Path,
    database_url: Option<&str>,
) -> std::process::Output {
    let mut command = Command::new(app_live_binary());
    command.arg("run").arg("--config").arg(config_path);
    command.env_remove("AXIOM_MODE");
    command.env_remove("AXIOM_NEG_RISK_LIVE_TARGETS");
    command.env_remove("AXIOM_NEG_RISK_LIVE_APPROVED_FAMILIES");
    command.env_remove("AXIOM_NEG_RISK_LIVE_READY_FAMILIES");
    command.env_remove("AXIOM_LOCAL_SIGNER_CONFIG");
    command.env_remove("AXIOM_REAL_USER_SHADOW_SMOKE");
    command.env_remove("AXIOM_POLYMARKET_SOURCE_CONFIG");
    command.env_remove("DATABASE_URL");
    if let Some(database_url) = database_url {
        command.env("DATABASE_URL", database_url);
    }
    command.output().expect("app-live should run")
}

struct TestDatabase {
    admin_pool: PgPool,
    pool: PgPool,
    schema: String,
    database_url: String,
}

impl TestDatabase {
    fn new() -> Self {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(async {
                let admin_database_url = std::env::var("DATABASE_URL")
                    .unwrap_or_else(|_| default_test_database_url().to_owned());
                let admin_pool = PgPoolOptions::new()
                    .max_connections(8)
                    .connect(&admin_database_url)
                    .await
                    .expect("test database should connect");
                let schema = format!(
                    "app_live_main_entrypoint_{}_{}",
                    std::process::id(),
                    NEXT_SCHEMA_ID.fetch_add(1, Ordering::Relaxed)
                );
                let create_schema = format!(r#"CREATE SCHEMA "{schema}""#);
                sqlx::query(&create_schema)
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

                Self {
                    admin_pool,
                    pool,
                    schema,
                    database_url,
                }
            })
    }

    fn database_url(&self) -> &str {
        &self.database_url
    }

    fn runtime_progress(&self) -> Option<RuntimeProgressRow> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(async {
                let pool = PgPoolOptions::new()
                    .max_connections(1)
                    .connect(&self.database_url)
                    .await
                    .expect("schema-scoped test pool should connect");
                let progress = RuntimeProgressRepo
                    .current(&pool)
                    .await
                    .expect("runtime progress lookup should succeed");
                pool.close().await;
                progress
            })
    }

    fn seed_live_execution_state(&self, operator_strategy_revision: &str) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(async {
                let pool = PgPoolOptions::new()
                    .max_connections(1)
                    .connect(&self.database_url)
                    .await
                    .expect("schema-scoped test pool should connect");
                RuntimeProgressRepo
                    .record_progress_with_strategy_revision(
                        &pool,
                        41,
                        7,
                        Some("snapshot-7"),
                        None,
                        Some(operator_strategy_revision),
                        None,
                    )
                    .await
                    .expect("runtime progress should seed");

                let attempt = ExecutionAttemptRow {
                    attempt_id: "attempt-live-entrypoint-1".to_owned(),
                    plan_id: "negrisk-submit-family:family-a".to_owned(),
                    snapshot_id: "snapshot-7".to_owned(),
                    route: "neg-risk".to_owned(),
                    scope: "family-a".to_owned(),
                    matched_rule_id: Some("family-a-live".to_owned()),
                    execution_mode: domain::ExecutionMode::Live,
                    attempt_no: 1,
                    idempotency_key: "idem-attempt-live-entrypoint-1".to_owned(),
                    run_session_id: None,
                };
                ExecutionAttemptRepo
                    .append(&pool, &attempt)
                    .await
                    .expect("live attempt should seed");

                let submission = LiveSubmissionRecordRow {
                    submission_ref: "submission-ref-entrypoint-1".to_owned(),
                    attempt_id: attempt.attempt_id.clone(),
                    route: "neg-risk".to_owned(),
                    scope: "family-a".to_owned(),
                    provider: "venue-polymarket".to_owned(),
                    state: "pending_reconcile".to_owned(),
                    payload: serde_json::json!({
                        "submission_ref": "submission-ref-entrypoint-1",
                        "family_id": "family-a",
                        "route": "neg-risk",
                        "reason": "ambiguous_attempt",
                    }),
                };
                LiveSubmissionRepo
                    .append(&pool, submission)
                    .await
                    .expect("live submission should seed");
                pool.close().await;
            });
    }

    fn seed_adopted_strategy_revision_with_routes(&self, operator_strategy_revision: &str) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(async {
                let pool = PgPoolOptions::new()
                    .max_connections(1)
                    .connect(&self.database_url)
                    .await
                    .expect("schema-scoped test pool should connect");
                let strategy_candidate_revision =
                    format!("strategy-candidate-{operator_strategy_revision}");
                let adoptable_strategy_revision = format!("adoptable-{operator_strategy_revision}");

                StrategyControlArtifactRepo
                    .upsert_strategy_candidate_set(
                        &pool,
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
                    .expect("strategy candidate row should seed");

                StrategyControlArtifactRepo
                    .upsert_adoptable_strategy_revision(
                        &pool,
                        &AdoptableStrategyRevisionRow {
                            adoptable_strategy_revision: adoptable_strategy_revision.clone(),
                            strategy_candidate_revision: strategy_candidate_revision.clone(),
                            rendered_operator_strategy_revision: operator_strategy_revision
                                .to_owned(),
                            payload: json!({
                                "adoptable_strategy_revision": adoptable_strategy_revision,
                                "strategy_candidate_revision": strategy_candidate_revision,
                                "rendered_operator_strategy_revision": operator_strategy_revision,
                                "route_artifacts": [
                                    {
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
                                    },
                                    {
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
                                    }
                                ],
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
                    .expect("adoptable strategy row should seed");

                StrategyAdoptionRepo
                    .upsert_provenance(
                        &pool,
                        &StrategyAdoptionProvenanceRow {
                            operator_strategy_revision: operator_strategy_revision.to_owned(),
                            adoptable_strategy_revision,
                            strategy_candidate_revision,
                        },
                    )
                    .await
                    .expect("strategy provenance should seed");
                pool.close().await;
            });
    }

    fn cleanup(self) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(async {
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

fn schema_scoped_database_url(base: &str, schema: &str) -> String {
    let options = format!("options=-csearch_path%3D{schema}");
    if base.contains('?') {
        format!("{base}&{options}")
    } else {
        format!("{base}?{options}")
    }
}

fn combined(output: &std::process::Output) -> String {
    let stdout = String::from_utf8(output.stdout.clone()).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr.clone()).expect("stderr should be utf8");
    format!("{stdout}{stderr}")
}

fn config_fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("config-schema")
        .join("tests")
        .join(name)
}

fn temp_config_fixture_path(
    fixture_name: &str,
    transform: impl FnOnce(String) -> String,
) -> PathBuf {
    let original =
        fs::read_to_string(config_fixture_path(fixture_name)).expect("fixture should exist");
    let transformed = transform(original);
    let path = std::env::temp_dir().join(format!(
        "app-live-entrypoint-{}-{}.toml",
        std::process::id(),
        NEXT_SCHEMA_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, transformed).expect("temporary config fixture should write");
    path
}

fn default_test_database_url() -> &'static str {
    "postgres://axiom:axiom@localhost:5432/axiom_arb"
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
