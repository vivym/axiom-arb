use std::{
    env, fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

use persistence::{run_migrations, RuntimeProgressRepo};
use sqlx::postgres::PgPoolOptions;

static SCHEMA_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn doctor_paper_mode_marks_live_checks_as_skip() {
    let output = Command::new(app_live_binary())
        .arg("doctor")
        .arg("--config")
        .arg(config_fixture("fixtures/app-live-paper.toml"))
        .output()
        .expect("app-live doctor should execute");

    assert!(output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert!(combined.contains("[OK] config parsed"), "{combined}");
    assert!(
        combined.contains("[SKIP] REST authentication not required in paper mode"),
        "{combined}"
    );
}

#[test]
fn doctor_paper_mode_includes_sectioned_summary() {
    let output = Command::new(app_live_binary())
        .arg("doctor")
        .arg("--config")
        .arg(config_fixture("fixtures/app-live-paper.toml"))
        .output()
        .expect("app-live doctor should execute");

    assert!(output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert!(combined.contains("Config: PASS"), "{combined}");
    assert!(
        combined.contains("Connectivity: PASS WITH SKIPS"),
        "{combined}"
    );
    assert!(combined.contains("Overall: PASS WITH SKIPS"), "{combined}");
}

#[test]
fn doctor_live_mode_reports_missing_adopted_target_as_target_source_error() {
    let output = Command::new(app_live_binary())
        .arg("doctor")
        .arg("--config")
        .arg(config_fixture("fixtures/app-live-ux-live.toml"))
        .env("DATABASE_URL", default_test_database_url())
        .output()
        .expect("app-live doctor should execute");

    assert!(!output.status.success(), "{}", combined(&output));
    assert!(combined(&output).contains("TargetSourceError"));
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
data_api_host = "https://data-api.polymarket.com"
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

    let output = Command::new(app_live_binary())
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
}

#[tokio::test]
async fn doctor_live_mode_accepts_explicit_targets_without_target_source() {
    let database = TestDatabase::new().await;
    let output = Command::new(app_live_binary())
        .arg("doctor")
        .arg("--config")
        .arg(config_fixture("fixtures/app-live-live.toml"))
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live doctor should execute");

    assert!(output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert!(combined.contains("[OK] config parsed"), "{combined}");
    assert!(
        combined.contains("[OK] target source resolved"),
        "{combined}"
    );
    assert!(
        combined.contains("[SKIP] control-plane checks not required for explicit targets"),
        "{combined}"
    );
}

#[tokio::test]
async fn doctor_live_mode_reports_fail_summary_when_runtime_progress_anchor_is_missing() {
    let database = TestDatabase::new().await;
    database.seed_runtime_progress_without_anchor().await;
    let output = Command::new(app_live_binary())
        .arg("doctor")
        .arg("--config")
        .arg(config_fixture("fixtures/app-live-live.toml"))
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live doctor should execute");

    let combined = combined(&output);
    assert!(!output.status.success(), "{combined}");
    assert!(combined.contains("TargetStateError"), "{combined}");
    assert!(combined.contains("Connectivity: FAIL"), "{combined}");
    assert!(combined.contains("Overall: FAIL"), "{combined}");
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
data_api_host = "https://data-api.polymarket.com"
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
    let output = Command::new(app_live_binary())
        .arg("doctor")
        .arg("--config")
        .arg(config.path())
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live doctor should execute");

    let combined = combined(&output);
    assert!(!output.status.success(), "{combined}");
    assert!(combined.contains("ConfigError"), "{combined}");
    assert!(combined.contains("Connectivity: FAIL"), "{combined}");
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

    async fn seed_runtime_progress_without_anchor(&self) {
        RuntimeProgressRepo
            .record_progress(&self.pool, 41, 7, Some("snapshot-7"), None)
            .await
            .expect("runtime progress should seed without anchor");
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

fn combined(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
