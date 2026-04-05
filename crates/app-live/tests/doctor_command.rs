use std::{
    env, fs,
    io::{Read, Write},
    net::TcpListener as StdTcpListener,
    path::PathBuf,
    process::Command,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use persistence::{
    models::{AdoptableTargetRevisionRow, CandidateAdoptionProvenanceRow, CandidateTargetSetRow},
    run_migrations, CandidateAdoptionRepo, CandidateArtifactRepo, RuntimeProgressRepo,
};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use tokio_tungstenite::tungstenite::{accept as accept_websocket, Message as WsMessage};

static SCHEMA_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn doctor_paper_mode_marks_live_checks_as_skip() {
    let output = app_live_command()
        .arg("doctor")
        .arg("--config")
        .arg(config_fixture("fixtures/app-live-paper.toml"))
        .env("DATABASE_URL", default_test_database_url())
        .output()
        .expect("app-live doctor should execute");

    assert!(output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert!(combined.contains("[OK] config parsed"), "{combined}");
    assert!(
        combined.contains(
            "[SKIP] long-lived account and relayer credentials not required in paper mode"
        ),
        "{combined}"
    );
}

#[test]
fn doctor_paper_mode_includes_sectioned_summary() {
    let output = app_live_command()
        .arg("doctor")
        .arg("--config")
        .arg(config_fixture("fixtures/app-live-paper.toml"))
        .env("DATABASE_URL", default_test_database_url())
        .output()
        .expect("app-live doctor should execute");

    assert!(output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert!(combined.contains("Config: PASS"), "{combined}");
    assert!(
        combined.contains("Credentials: PASS WITH SKIPS"),
        "{combined}"
    );
    assert!(
        combined.contains("Connectivity: PASS WITH SKIPS"),
        "{combined}"
    );
    assert!(combined.contains("Overall: PASS WITH SKIPS"), "{combined}");
    assert!(
        combined.contains("Next: app-live run --config"),
        "{combined}"
    );
}

#[test]
fn doctor_paper_mode_quotes_follow_up_config_paths_with_spaces() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let config_dir = temp_dir.path().join("config with spaces");
    fs::create_dir_all(&config_dir).expect("config dir with spaces");
    let config_path = config_dir.join("app live paper.toml");
    fs::copy(config_fixture("fixtures/app-live-paper.toml"), &config_path)
        .expect("copy paper fixture into spaced path");

    let output = app_live_command()
        .arg("doctor")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", default_test_database_url())
        .output()
        .expect("app-live doctor should execute");

    assert!(output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    let expected = format!(
        "Next: app-live run --config '{}'",
        config_path.display().to_string().replace('\'', r"'\''")
    );
    assert!(combined.contains(&expected), "{combined}");
}

#[test]
fn doctor_paper_mode_fails_without_database_url() {
    let output = app_live_command()
        .arg("doctor")
        .arg("--config")
        .arg(config_fixture("fixtures/app-live-paper.toml"))
        .env_remove("DATABASE_URL")
        .output()
        .expect("app-live doctor should execute");

    let combined = combined(&output);
    assert!(!output.status.success(), "{combined}");
    assert_section_summary(&combined, "Credentials", "PASS WITH SKIPS");
    assert_section_summary(&combined, "Target Source", "PASS WITH SKIPS");
    assert_section_summary(&combined, "Runtime Safety", "PASS WITH SKIPS");
    assert_section_summary(&combined, "Connectivity", "FAIL");
    assert_section_summary(&combined, "Overall", "FAIL");
    assert!(combined.contains("ConnectivityError"), "{combined}");
}

#[test]
fn doctor_live_mode_reports_missing_adopted_target_as_target_source_error() {
    let output = app_live_command()
        .arg("doctor")
        .arg("--config")
        .arg(config_fixture("fixtures/app-live-ux-live.toml"))
        .env("DATABASE_URL", default_test_database_url())
        .output()
        .expect("app-live doctor should execute");

    assert!(!output.status.success(), "{}", combined(&output));
    assert!(combined(&output).contains("TargetSourceError"));
    assert!(combined(&output).contains("Target Source: FAIL"));
}

#[test]
fn doctor_live_mode_reports_missing_operator_target_revision_from_wizard_shape() {
    let temp = tempfile::NamedTempFile::new().expect("temp file");
    fs::write(
        temp.path(),
        r#"
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

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key-1"
secret = "poly-secret-1"
passphrase = "poly-passphrase-1"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key-1"
address = "0x3333333333333333333333333333333333333333"

[negrisk.target_source]
source = "adopted"

[negrisk.rollout]
approved_families = []
ready_families = []
"#,
    )
    .expect("seed wizard-shaped config");

    let output = app_live_command()
        .arg("doctor")
        .arg("--config")
        .arg(temp.path())
        .env("DATABASE_URL", default_test_database_url())
        .output()
        .expect("app-live doctor should execute");

    assert!(!output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert!(combined.contains("TargetSourceError"), "{combined}");
    assert!(
        combined.contains("missing negrisk.target_source.operator_target_revision"),
        "{combined}"
    );
    assert!(
        combined.contains("Next: app-live targets candidates --config"),
        "{combined}"
    );
    assert!(
        combined.contains("Next: app-live targets adopt --config"),
        "{combined}"
    );
}

#[tokio::test]
async fn doctor_live_mode_fails_without_database_url_for_explicit_targets() {
    let config = temp_live_config(
        r#"
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
api_key = "poly-api-key"
passphrase = "poly-passphrase"
timestamp = "1700000000"
signature = "poly-signature"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key"
address = "0x1111111111111111111111111111111111111111"

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
"#,
    );
    let output = app_live_command()
        .arg("doctor")
        .arg("--config")
        .arg(config.path())
        .env_remove("DATABASE_URL")
        .output()
        .expect("app-live doctor should execute");

    let combined = combined(&output);
    assert!(!output.status.success(), "{combined}");
    assert_section_summary(&combined, "Credentials", "PASS");
    assert_section_summary(&combined, "Target Source", "PASS WITH SKIPS");
    assert_section_summary(&combined, "Runtime Safety", "PASS WITH SKIPS");
    assert_section_summary(&combined, "Connectivity", "FAIL");
    assert_section_summary(&combined, "Overall", "FAIL");
    assert!(
        combined.contains("[OK] startup target resolution succeeded"),
        "{combined}"
    );
    assert!(
        combined.contains("[SKIP] control-plane checks not required for explicit targets"),
        "{combined}"
    );
}

#[tokio::test]
async fn doctor_live_explicit_targets_with_database_url_pass() {
    let database = TestDatabase::new().await;
    let venue = MockDoctorVenue::success();
    let config = temp_live_config(&format!(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[polymarket.source]
clob_host = "{clob_host}"
data_api_host = "{data_api_host}"
relayer_host = "{relayer_host}"
market_ws_url = "{market_ws_url}"
user_ws_url = "{user_ws_url}"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60

[polymarket.signer]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key"
passphrase = "poly-passphrase"
timestamp = "1700000000"
signature = "poly-signature"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key"
address = "0x1111111111111111111111111111111111111111"

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
"#,
        clob_host = venue.http_base_url(),
        data_api_host = venue.http_base_url(),
        relayer_host = venue.http_base_url(),
        market_ws_url = venue.market_ws_url(),
        user_ws_url = venue.user_ws_url(),
    ));
    let output = app_live_command()
        .arg("doctor")
        .arg("--config")
        .arg(config.path())
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live doctor should execute");

    assert!(output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert_section_summary(&combined, "Target Source", "PASS WITH SKIPS");
    assert_section_summary(&combined, "Runtime Safety", "PASS WITH SKIPS");
    assert_section_summary(&combined, "Connectivity", "PASS WITH SKIPS");
    assert_section_summary(&combined, "Overall", "PASS WITH SKIPS");
    assert!(
        combined.contains("Next: app-live run --config"),
        "{combined}"
    );
}

#[tokio::test]
async fn doctor_live_explicit_targets_fail_when_database_is_unreachable() {
    let venue = MockDoctorVenue::success();
    let config = temp_live_config(&format!(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[polymarket.source]
clob_host = "{clob_host}"
data_api_host = "{data_api_host}"
relayer_host = "{relayer_host}"
market_ws_url = "{market_ws_url}"
user_ws_url = "{user_ws_url}"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60

[polymarket.signer]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key"
passphrase = "poly-passphrase"
timestamp = "1700000000"
signature = "poly-signature"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key"
address = "0x1111111111111111111111111111111111111111"

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
"#,
        clob_host = venue.http_base_url(),
        data_api_host = venue.http_base_url(),
        relayer_host = venue.http_base_url(),
        market_ws_url = venue.market_ws_url(),
        user_ws_url = venue.user_ws_url(),
    ));
    let output = app_live_command()
        .arg("doctor")
        .arg("--config")
        .arg(config.path())
        .env(
            "DATABASE_URL",
            "postgres://axiom:axiom@127.0.0.1:9/axiom_arb",
        )
        .output()
        .expect("app-live doctor should execute");

    let combined = combined(&output);
    assert!(!output.status.success(), "{combined}");
    assert_section_summary(&combined, "Target Source", "PASS WITH SKIPS");
    assert_section_summary(&combined, "Connectivity", "FAIL");
    assert_section_summary(&combined, "Overall", "FAIL");
    assert!(combined.contains("ConnectivityError"), "{combined}");
    assert!(
        combined.contains("Next: fix the reported issue and rerun doctor"),
        "{combined}"
    );
}

#[tokio::test]
async fn doctor_adopted_source_reports_control_plane_details_without_explicit_target_skip_text() {
    let database = TestDatabase::new().await;
    let venue = MockDoctorVenue::success();
    database
        .seed_adopted_target_with_runtime_progress("targets-rev-9")
        .await;
    let config = temp_live_config(&format!(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[polymarket.source]
clob_host = "{clob_host}"
data_api_host = "{data_api_host}"
relayer_host = "{relayer_host}"
market_ws_url = "{market_ws_url}"
user_ws_url = "{user_ws_url}"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60

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
"#,
        clob_host = venue.http_base_url(),
        data_api_host = venue.http_base_url(),
        relayer_host = venue.http_base_url(),
        market_ws_url = venue.market_ws_url(),
        user_ws_url = venue.user_ws_url(),
    ));
    let output = app_live_command()
        .arg("doctor")
        .arg("--config")
        .arg(config.path())
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live doctor should execute");

    assert!(output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert_section_summary(&combined, "Target Source", "PASS");
    assert_section_summary(&combined, "Runtime Safety", "PASS WITH SKIPS");
    assert!(
        combined.contains("configured operator target revision"),
        "{combined}"
    );
    assert!(
        combined.contains("active operator target revision"),
        "{combined}"
    );
    assert!(combined.contains("restart not needed"), "{combined}");
    assert!(
        !combined.contains("[SKIP] control-plane checks not required for explicit targets"),
        "{combined}"
    );
}

#[tokio::test]
async fn doctor_smoke_mode_reports_runtime_safety_as_pass() {
    let database = TestDatabase::new().await;
    let venue = MockDoctorVenue::success();
    let config = temp_live_config(&format!(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = true

[polymarket.source]
clob_host = "{clob_host}"
data_api_host = "{data_api_host}"
relayer_host = "{relayer_host}"
market_ws_url = "{market_ws_url}"
user_ws_url = "{user_ws_url}"
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
kind = "relayer_api_key"
api_key = "relay-key-1"
address = "0x1111111111111111111111111111111111111111"

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
"#,
        clob_host = venue.http_base_url(),
        data_api_host = venue.http_base_url(),
        relayer_host = venue.http_base_url(),
        market_ws_url = venue.market_ws_url(),
        user_ws_url = venue.user_ws_url(),
    ));
    let output = app_live_command()
        .arg("doctor")
        .arg("--config")
        .arg(config.path())
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live doctor should execute");

    assert!(output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert_section_summary(&combined, "Runtime Safety", "PASS");
}

#[tokio::test]
async fn doctor_live_mode_reports_connectivity_failure_when_authenticated_rest_is_rejected() {
    let database = TestDatabase::new().await;
    let venue = MockDoctorVenue::orders_fail("401 Unauthorized", r#"{"error":"unauthorized"}"#);
    let config = temp_live_config(&format!(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[polymarket.source]
clob_host = "{clob_host}"
data_api_host = "{data_api_host}"
relayer_host = "{relayer_host}"
market_ws_url = "{market_ws_url}"
user_ws_url = "{user_ws_url}"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60

[polymarket.signer]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key"
passphrase = "poly-passphrase"
timestamp = "1700000000"
signature = "poly-signature"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key"
address = "0x1111111111111111111111111111111111111111"

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
"#,
        clob_host = venue.http_base_url(),
        data_api_host = venue.http_base_url(),
        relayer_host = venue.http_base_url(),
        market_ws_url = venue.market_ws_url(),
        user_ws_url = venue.user_ws_url(),
    ));
    let output = app_live_command()
        .arg("doctor")
        .arg("--config")
        .arg(config.path())
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live doctor should execute");

    let combined = combined(&output);
    assert!(!output.status.success(), "{combined}");
    assert_section_summary(&combined, "Credentials", "PASS");
    assert_section_summary(&combined, "Target Source", "PASS WITH SKIPS");
    assert_section_summary(&combined, "Connectivity", "FAIL");
    assert_section_summary(&combined, "Overall", "FAIL");
    assert!(combined.contains("ConnectivityError"), "{combined}");
    assert!(
        combined.contains("Next: fix the reported issue and rerun doctor"),
        "{combined}"
    );
}

#[tokio::test]
async fn doctor_live_mode_reports_fail_summary_when_real_user_shadow_smoke_config_is_invalid() {
    let database = TestDatabase::new().await;
    let config = temp_live_config(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = true

[polymarket.source]
clob_host = "ftp://clob.polymarket.com"
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
secret = "poly-secret-1"
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
"#,
    );
    let output = app_live_command()
        .arg("doctor")
        .arg("--config")
        .arg(config.path())
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live doctor should execute");

    let combined = combined(&output);
    assert!(!output.status.success(), "{combined}");
    assert!(combined.contains("RuntimeSafetyError"), "{combined}");
    assert!(combined.contains("Runtime Safety: FAIL"), "{combined}");
    assert!(combined.contains("Credentials: PASS"), "{combined}");
    assert!(combined.contains("Overall: FAIL"), "{combined}");
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

fn app_live_command() -> Command {
    let mut command = Command::new(app_live_binary());
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
        .env("NO_PROXY", "127.0.0.1,localhost");
    command
}

fn config_fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("config-schema")
        .join("tests")
        .join(relative)
}

fn temp_live_config(contents: &str) -> tempfile::NamedTempFile {
    let temp = tempfile::NamedTempFile::new().expect("temp file");
    fs::write(temp.path(), contents).expect("write temp config");
    temp
}

fn default_test_database_url() -> &'static str {
    "postgres://axiom:axiom@localhost:5432/axiom_arb"
}

struct TestDatabase {
    database_url: String,
    pool: sqlx::PgPool,
}

impl TestDatabase {
    async fn new() -> Self {
        let database_url = env::var("TEST_DATABASE_URL")
            .or_else(|_| env::var("DATABASE_URL"))
            .unwrap_or_else(|_| default_test_database_url().to_owned());
        let admin_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
            .expect("admin pool should connect");
        let schema = format!(
            "app_live_doctor_{}_{}",
            std::process::id(),
            SCHEMA_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        sqlx::query(&format!(r#"DROP SCHEMA IF EXISTS "{schema}" CASCADE"#))
            .execute(&admin_pool)
            .await
            .expect("drop schema");
        sqlx::query(&format!(r#"CREATE SCHEMA "{schema}""#))
            .execute(&admin_pool)
            .await
            .expect("create schema");

        let scoped_url = schema_scoped_database_url(&database_url, &schema);
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&scoped_url)
            .await
            .expect("pool should connect");
        run_migrations(&pool).await.expect("migrations should run");

        Self {
            database_url: scoped_url,
            pool,
        }
    }

    fn database_url(&self) -> &str {
        &self.database_url
    }

    async fn seed_adopted_target_with_runtime_progress(&self, operator_target_revision: &str) {
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
            .expect("runtime progress should seed with anchor");
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

    fn orders_fail(status_line: &str, body: &str) -> Self {
        Self {
            http: ProbeHttpServer::spawn(ProbeHttpBehavior::orders_fail(status_line, body)),
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

    fn orders_fail(status_line: &str, body: &str) -> Self {
        Self {
            orders_status_line: status_line.to_owned(),
            orders_body: body.to_owned(),
            ..Self::success()
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
) -> (&'a str, &'a str) {
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let path = target.split('?').next().unwrap_or_default();

    match (method, path) {
        ("GET", "/orders") => (&behavior.orders_status_line, &behavior.orders_body),
        ("POST", "/heartbeat") => (&behavior.heartbeat_status_line, &behavior.heartbeat_body),
        ("GET", "/transactions") => (
            &behavior.transactions_status_line,
            &behavior.transactions_body,
        ),
        _ => ("404 Not Found", r#"{"error":"not found"}"#),
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
    fn response_payload(self) -> &'static str {
        match self {
            Self::Market => {
                r#"{"event":"book","asset_id":"token-1","best_bid":"0.40","best_ask":"0.41"}"#
            }
            Self::User => {
                r#"{"event":"trade","trade_id":"trade-1","order_id":"order-1","status":"MATCHED","condition_id":"condition-1","price":"0.41","size":"100","fee_rate_bps":"15","transaction_hash":"0xtrade"}"#
            }
        }
    }
}

fn combined(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assert_section_summary(output: &str, section: &str, result: &str) {
    let expected = format!("{section}: {result}");
    assert!(output.lines().any(|line| line == expected), "{output}");
}
