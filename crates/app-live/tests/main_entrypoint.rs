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
    io::{Read, Write},
    net::TcpListener as StdTcpListener,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};
use tokio_tungstenite::tungstenite::{accept as accept_websocket, Message as WsMessage};
use toml_edit::{table, value, DocumentMut};

static NEXT_SCHEMA_ID: AtomicU64 = AtomicU64::new(1);
const TEST_PRIVATE_KEY: &str = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
const TEST_ACCOUNT_ADDRESS: &str = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266";
const TEST_CONDITION_ID: &str =
    "0x0000000000000000000000000000000000000000000000000000000000000001";
const TEST_TOKEN_ID: &str = "29";

#[test]
fn binary_entrypoint_reads_paper_mode_from_config_file() {
    let output = app_live_output_with_config("fixtures/app-live-paper.toml");

    assert!(output.status.success());
    assert!(combined(&output).contains("app_mode=paper"));
}

#[test]
fn example_config_uses_canonical_strategy_control_only() {
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
    assert!(!text.contains("[negrisk.target_source]"), "{text}");
    assert!(!text.contains("operator_target_revision"), "{text}");
    assert!(!text.contains("[[negrisk.targets]]"), "{text}");
}

#[test]
fn operator_docs_stop_teaching_compatibility_mode_and_target_source_aliases() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let readme = fs::read_to_string(repo_root.join("README.md")).expect("README should load");
    let smoke = fs::read_to_string(repo_root.join("docs/runbooks/real-user-shadow-smoke.md"))
        .expect("smoke runbook should load");
    let adoption = fs::read_to_string(repo_root.join("docs/runbooks/operator-target-adoption.md"))
        .expect("adoption runbook should load");

    assert!(!readme.contains("--adopt-compatibility"), "{readme}");
    assert!(!readme.contains("compatibility mode"), "{readme}");
    assert!(!smoke.contains("compatibility mode"), "{smoke}");
    assert!(!smoke.contains("[[negrisk.targets]]"), "{smoke}");
    assert!(!adoption.contains("[negrisk.target_source]"), "{adoption}");
    assert!(!adoption.contains("operator_target_revision"), "{adoption}");
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
    let venue = MockDoctorVenue::success();
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
    let config_path = temp_config_fixture_path("fixtures/app-live-live.toml", |config| {
        with_mock_live_venue(normalize_non_eoa_live_fixture(config), &venue)
    });

    let output = app_live_output_with_config_path_and_private_key(
        &config_path,
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

    let _ = fs::remove_file(config_path);
    database.cleanup();
}

#[test]
fn live_config_fails_when_restored_operator_strategy_revision_anchor_is_stale() {
    let database = TestDatabase::new();
    let venue = MockDoctorVenue::success();
    database.seed_adopted_strategy_revision_with_routes("strategy-rev-12");
    database.seed_live_execution_state("strategy-rev-stale");
    let config_path = temp_config_fixture_path("fixtures/app-live-live.toml", |config| {
        with_mock_live_venue(normalize_non_eoa_live_fixture(config), &venue)
    });

    let output = app_live_output_with_config_path_and_private_key(
        &config_path,
        Some(database.database_url()),
    );

    assert!(!output.status.success(), "{}", combined(&output));
    assert!(combined(&output).contains("operator strategy revision anchor mismatch"));

    let _ = fs::remove_file(config_path);
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
    for key in [
        "all_proxy",
        "ALL_PROXY",
        "http_proxy",
        "HTTP_PROXY",
        "https_proxy",
        "HTTPS_PROXY",
        "POLYMARKET_PRIVATE_KEY",
    ] {
        command.env_remove(key);
    }
    command
        .env("no_proxy", "127.0.0.1,localhost")
        .env("NO_PROXY", "127.0.0.1,localhost");
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

fn app_live_output_with_config_path_and_private_key(
    config_path: &Path,
    database_url: Option<&str>,
) -> std::process::Output {
    let mut command = Command::new(app_live_binary());
    command.arg("run").arg("--config").arg(config_path);
    for key in [
        "all_proxy",
        "ALL_PROXY",
        "http_proxy",
        "HTTP_PROXY",
        "https_proxy",
        "HTTPS_PROXY",
    ] {
        command.env_remove(key);
    }
    command
        .env("no_proxy", "127.0.0.1,localhost")
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("POLYMARKET_PRIVATE_KEY", TEST_PRIVATE_KEY);
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
                                                    "condition_id": TEST_CONDITION_ID,
                                                    "token_id": TEST_TOKEN_ID,
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
                                            "condition_id": TEST_CONDITION_ID,
                                            "token_id": TEST_TOKEN_ID,
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

fn normalize_non_eoa_live_fixture(config: String) -> String {
    format!(
        "{}\n\n[polymarket.relayer_auth]\nkind = \"relayer_api_key\"\napi_key = \"relay-key\"\naddress = \"{TEST_ACCOUNT_ADDRESS}\"\n",
        config
            .replace(
                "0x1111111111111111111111111111111111111111",
                TEST_ACCOUNT_ADDRESS,
            )
            .replace(
                "0x2222222222222222222222222222222222222222",
                TEST_ACCOUNT_ADDRESS,
            )
            .replace("signature_type = \"eoa\"", "signature_type = \"proxy\"")
            .replace("wallet_route = \"eoa\"", "wallet_route = \"proxy\"")
            .replace("poly-api-key", "00000000-0000-0000-0000-000000000002")
            .replace(
                "poly-secret",
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            )
            .replace(
                "poly-passphrase",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            )
    )
}

fn with_mock_live_venue(config: String, venue: &MockDoctorVenue) -> String {
    let mut document = config
        .parse::<DocumentMut>()
        .expect("config fixture should parse as TOML");

    let polymarket = document["polymarket"]
        .as_table_like_mut()
        .expect("config fixture should contain [polymarket]");
    if polymarket.get("source_overrides").is_none() {
        polymarket.insert("source_overrides", table());
    }
    let source = polymarket
        .get_mut("source_overrides")
        .expect("config fixture should contain [polymarket.source_overrides]")
        .as_table_like_mut()
        .expect("config fixture should contain [polymarket.source_overrides]");

    for (key, rewritten) in [
        ("clob_host", venue.http_base_url()),
        ("data_api_host", venue.http_base_url()),
        ("relayer_host", venue.http_base_url()),
        ("market_ws_url", venue.market_ws_url()),
        ("user_ws_url", venue.user_ws_url()),
    ] {
        source.insert(key, value(rewritten));
    }
    for (key, rewritten) in [
        ("heartbeat_interval_seconds", toml_edit::Value::from(15)),
        ("relayer_poll_interval_seconds", toml_edit::Value::from(5)),
        (
            "metadata_refresh_interval_seconds",
            toml_edit::Value::from(60),
        ),
    ] {
        source.insert(key, toml_edit::Item::Value(rewritten));
    }

    document.to_string()
}

struct MockDoctorVenue {
    http: ProbeHttpServer,
    market_ws: ProbeWsServer,
    user_ws: ProbeWsServer,
}

impl MockDoctorVenue {
    fn success() -> Self {
        Self {
            http: ProbeHttpServer::spawn(ProbeHttpBehavior::success()),
            market_ws: ProbeWsServer::spawn(WsProbeKind::Market),
            user_ws: ProbeWsServer::spawn(WsProbeKind::User),
        }
    }

    fn http_base_url(&self) -> &str {
        self.http.base_url()
    }

    fn market_ws_url(&self) -> &str {
        self.market_ws.url()
    }

    fn user_ws_url(&self) -> &str {
        self.user_ws.url()
    }
}

struct ProbeHttpServer {
    base_url: String,
    shutdown_tx: Option<mpsc::Sender<()>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl ProbeHttpServer {
    fn spawn(behavior: ProbeHttpBehavior) -> Self {
        let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind http probe server");
        let address = listener.local_addr().expect("http probe server address");
        listener
            .set_nonblocking(true)
            .expect("http probe server should be nonblocking");
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let handle = thread::spawn(move || loop {
            if shutdown_rx.try_recv().is_ok() {
                break;
            }

            match listener.accept() {
                Ok((stream, _)) => {
                    stream
                        .set_nonblocking(false)
                        .expect("accepted http stream should be blocking");
                    handle_http_probe_connection(stream, &behavior)
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("http probe server accept failed: {error}"),
            }
        });

        Self {
            base_url: format!("http://{address}"),
            shutdown_tx: Some(shutdown_tx),
            handle: Some(handle),
        }
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }
}

impl Drop for ProbeHttpServer {
    fn drop(&mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
        if let Some(handle) = self.handle.take() {
            handle.join().expect("http probe server should join");
        }
    }
}

#[derive(Clone)]
struct ProbeHttpBehavior {
    orders_status_line: String,
    orders_body: String,
    heartbeat_status_line: String,
    heartbeat_body: String,
    transactions_status_line: String,
    transactions_body: String,
}

impl ProbeHttpBehavior {
    fn success() -> Self {
        Self {
            orders_status_line: "200 OK".to_owned(),
            orders_body: "[]".to_owned(),
            heartbeat_status_line: "200 OK".to_owned(),
            heartbeat_body: r#"{"success":true,"heartbeat_id":"hb-1"}"#.to_owned(),
            transactions_status_line: "200 OK".to_owned(),
            transactions_body: "[]".to_owned(),
        }
    }
}

fn handle_http_probe_connection(mut stream: std::net::TcpStream, behavior: &ProbeHttpBehavior) {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    let mut header_end = None;
    let mut content_length = 0_usize;

    loop {
        let read = stream.read(&mut chunk).expect("read probe request");
        if read == 0 {
            break;
        }

        buffer.extend_from_slice(&chunk[..read]);
        if header_end.is_none() {
            header_end = find_header_end(&buffer);
            if let Some(index) = header_end {
                let headers = String::from_utf8_lossy(&buffer[..index]);
                content_length = content_length_from_headers(&headers);
            }
        }

        if let Some(index) = header_end {
            let body_bytes = buffer.len().saturating_sub(index + 4);
            if body_bytes >= content_length {
                break;
            }
        }
    }

    let request = String::from_utf8_lossy(&buffer);
    let request_line = request.lines().next().unwrap_or_default();
    let (status_line, body) = http_probe_response(request_line, behavior);
    let response = format!(
        "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .expect("write probe response");
    stream.flush().expect("flush probe response");
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn content_length_from_headers(headers: &str) -> usize {
    headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.trim().eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0)
}

fn http_probe_response<'a>(
    request_line: &str,
    behavior: &'a ProbeHttpBehavior,
) -> (&'a str, String) {
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let path = target.split('?').next().unwrap_or_default();

    match (method, path) {
        ("GET", "/data/orders") => (&behavior.orders_status_line, behavior.orders_body.clone()),
        ("GET", "/fee-rate") => ("200 OK", r#"{"base_fee":17}"#.to_owned()),
        ("GET", "/tick-size") => (
            "200 OK",
            r#"{"minimum_tick_size":"0.01"}"#.to_owned(),
        ),
        ("GET", "/neg-risk") => ("200 OK", r#"{"neg_risk":false}"#.to_owned()),
        ("POST", "/order") => (
            "200 OK",
            r#"{"makingAmount":"0","orderID":"order-1","status":"live","success":true,"takingAmount":"0"}"#
                .to_owned(),
        ),
        ("POST", "/v1/heartbeats") => (
            &behavior.heartbeat_status_line,
            behavior.heartbeat_body.clone(),
        ),
        ("GET", "/transactions") => (
            &behavior.transactions_status_line,
            behavior.transactions_body.clone(),
        ),
        _ => (
            "404 Not Found",
            format!(r#"{{"error":"not found","request_line":"{request_line}"}}"#),
        ),
    }
}

struct ProbeWsServer {
    url: String,
    shutdown_tx: Option<mpsc::Sender<()>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl ProbeWsServer {
    fn spawn(kind: WsProbeKind) -> Self {
        let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind ws probe server");
        let address = listener.local_addr().expect("ws probe server address");
        listener
            .set_nonblocking(true)
            .expect("ws probe server should be nonblocking");
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let handle = thread::spawn(move || loop {
            if shutdown_rx.try_recv().is_ok() {
                break;
            }

            match listener.accept() {
                Ok((stream, _)) => {
                    stream
                        .set_nonblocking(false)
                        .expect("accepted ws stream should be blocking");
                    let mut websocket =
                        accept_websocket(stream).expect("accept ws probe websocket");
                    let mut responded = false;
                    loop {
                        match websocket.read() {
                            Ok(WsMessage::Text(_)) if !responded => {
                                websocket
                                    .send(WsMessage::Text(kind.response_payload().into()))
                                    .expect("send ws probe response");
                                responded = true;
                            }
                            Ok(_) => {}
                            Err(_) => break,
                        }
                    }
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("ws probe server accept failed: {error}"),
            }
        });

        Self {
            url: format!("ws://{address}"),
            shutdown_tx: Some(shutdown_tx),
            handle: Some(handle),
        }
    }

    fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for ProbeWsServer {
    fn drop(&mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
        if let Some(handle) = self.handle.take() {
            handle.join().expect("ws probe server should join");
        }
    }
}

#[derive(Clone, Copy)]
enum WsProbeKind {
    Market,
    User,
}

impl WsProbeKind {
    fn response_payload(self) -> String {
        match self {
            Self::Market => format!(
                r#"{{"event":"book","asset_id":"{TEST_TOKEN_ID}","best_bid":"0.40","best_ask":"0.41"}}"#
            ),
            Self::User => format!(
                r#"{{"event":"trade","trade_id":"trade-1","order_id":"order-1","status":"MATCHED","condition_id":"{TEST_CONDITION_ID}","price":"0.41","size":"100","fee_rate_bps":"15","transaction_hash":"0xtrade"}}"#
            ),
        }
    }
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
