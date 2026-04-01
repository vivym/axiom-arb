use std::{
    env, fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

use persistence::{
    models::{AdoptableTargetRevisionRow, CandidateAdoptionProvenanceRow, CandidateTargetSetRow},
    run_migrations, CandidateAdoptionRepo, CandidateArtifactRepo, RuntimeProgressRepo,
};
use serde_json::json;
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
        combined.contains(
            "[SKIP] long-lived account and relayer credentials not required in paper mode"
        ),
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
        combined.contains("Credentials: PASS WITH SKIPS"),
        "{combined}"
    );
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
async fn doctor_live_mode_accepts_explicit_targets_without_runtime_progress_anchor() {
    let database = TestDatabase::new().await;
    database.seed_runtime_progress_without_anchor().await;
    let config = temp_live_config(
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
    let output = Command::new(app_live_binary())
        .arg("doctor")
        .arg("--config")
        .arg(config.path())
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live doctor should execute");

    assert!(output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert!(combined.contains("[OK] config parsed"), "{combined}");
    assert!(combined.contains("Target Source: PASS"), "{combined}");
    assert!(
        combined.contains("[OK] startup target resolution succeeded"),
        "{combined}"
    );
    assert!(
        combined.contains("[SKIP] control-plane checks not required for explicit targets"),
        "{combined}"
    );
    assert!(
        !combined.contains("runtime progress row exists without operator_target_revision anchor"),
        "{combined}"
    );
}

#[tokio::test]
async fn doctor_adopted_source_reports_control_plane_details_without_explicit_target_skip_text() {
    let database = TestDatabase::new().await;
    database
        .seed_adopted_target_with_runtime_progress("targets-rev-9")
        .await;
    let config = temp_live_config(
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
    );
    let output = Command::new(app_live_binary())
        .arg("doctor")
        .arg("--config")
        .arg(config.path())
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live doctor should execute");

    assert!(output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert!(combined.contains("Target Source: PASS"), "{combined}");
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
    let config = temp_live_config(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = true

[polymarket.source]
clob_host = "https://clob.polymarket.com"
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

    assert!(output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert!(combined.contains("Runtime Safety: PASS"), "{combined}");
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
    assert!(combined.contains("CredentialError"), "{combined}");
    assert!(combined.contains("Credentials: FAIL"), "{combined}");
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

fn combined(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
