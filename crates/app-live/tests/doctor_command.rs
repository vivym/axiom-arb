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
    models::{
        AdoptableStrategyRevisionRow, AdoptableTargetRevisionRow, CandidateAdoptionProvenanceRow,
        CandidateTargetSetRow, StrategyAdoptionProvenanceRow, StrategyCandidateSetRow,
    },
    run_migrations, CandidateAdoptionRepo, CandidateArtifactRepo, RuntimeProgressRepo,
    StrategyAdoptionRepo, StrategyControlArtifactRepo,
};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use tokio_tungstenite::tungstenite::{
    accept_hdr as accept_websocket,
    handshake::server::{Request as WsRequest, Response as WsResponse},
    Message as WsMessage,
};
use toml_edit::{value, DocumentMut, TableLike};

static SCHEMA_COUNTER: AtomicU64 = AtomicU64::new(0);
const TEST_PRIVATE_KEY: &str = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
const TEST_SIGNER_ADDRESS: &str = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266";
const TEST_PRIMARY_SECRET: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
const TEST_SECONDARY_SECRET: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
const TEST_PRIMARY_PASSPHRASE: &str =
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const TEST_SECONDARY_PASSPHRASE: &str =
    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

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

[strategy_control]
source = "adopted"

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
api_key = "00000000-0000-0000-0000-000000000001"
secret = "poly-secret-1"
passphrase = "poly-passphrase-1"

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

#[test]
fn doctor_live_mode_reports_missing_operator_strategy_revision_from_neutral_adopted_shape() {
    let temp = tempfile::NamedTempFile::new().expect("temp file");
    fs::write(
        temp.path(),
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[strategy_control]
source = "adopted"

[strategies.neg_risk]
enabled = true

[strategies.neg_risk.rollout]
approved_scopes = []
ready_scopes = []

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
api_key = "00000000-0000-0000-0000-000000000001"
secret = "poly-secret-1"
passphrase = "poly-passphrase-1"
"#,
    )
    .expect("seed neutral adopted config");

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
        combined.contains("missing strategy_control.operator_strategy_revision"),
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

[strategy_control]
source = "adopted"

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
api_key = "00000000-0000-0000-0000-000000000002"
secret = "poly-secret"
passphrase = "poly-passphrase"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-a"]

[[negrisk.targets]]
family_id = "family-a"

[[negrisk.targets.members]]
condition_id = "0x0000000000000000000000000000000000000000000000000000000000000001"
token_id = "29"
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
    assert!(combined.contains("[SKIP] compatibility mode"), "{combined}");
    assert!(combined.contains("--adopt-compatibility"), "{combined}");
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

[strategy_control]
source = "adopted"

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
api_key = "00000000-0000-0000-0000-000000000002"
secret = "poly-secret"
passphrase = "poly-passphrase"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-a"]

[[negrisk.targets]]
family_id = "family-a"

[[negrisk.targets.members]]
condition_id = "0x0000000000000000000000000000000000000000000000000000000000000001"
token_id = "29"
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
    assert_section_summary(&combined, "Connectivity", "PASS");
    assert_section_summary(&combined, "Overall", "PASS WITH SKIPS");
    assert!(
        !combined.contains("relayer reachability probe"),
        "{combined}"
    );
    assert!(
        combined.contains("Next: app-live run --config"),
        "{combined}"
    );
    assert!(combined.contains("[SKIP] compatibility mode"), "{combined}");
    assert!(combined.contains("--adopt-compatibility"), "{combined}");
}

#[tokio::test]
async fn doctor_live_explicit_targets_without_private_key_runs_l2_probe() {
    let database = TestDatabase::new().await;
    let venue = MockDoctorVenue::success();
    let config = temp_live_config(&format!(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[strategy_control]
source = "adopted"

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
api_key = "00000000-0000-0000-0000-000000000002"
secret = "poly-secret"
passphrase = "poly-passphrase"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-a"]

[[negrisk.targets]]
family_id = "family-a"

[[negrisk.targets.members]]
condition_id = "0x0000000000000000000000000000000000000000000000000000000000000001"
token_id = "29"
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
        .env_remove("POLYMARKET_PRIVATE_KEY")
        .output()
        .expect("app-live doctor should execute");

    assert!(output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert_section_summary(&combined, "Connectivity", "PASS");
    assert_section_summary(&combined, "Overall", "PASS WITH SKIPS");
    assert!(
        !combined.contains("relayer reachability probe"),
        "{combined}"
    );
    assert!(
        !combined.contains("missing required environment variable POLYMARKET_PRIVATE_KEY"),
        "{combined}"
    );
}

#[tokio::test]
async fn doctor_live_non_eoa_explicit_targets_still_runs_relayer_probe() {
    let database = TestDatabase::new().await;
    let venue = MockDoctorVenue::success();
    let config = temp_live_config(&format!(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[strategy_control]
source = "adopted"

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
signature_type = "proxy"
wallet_route = "proxy"
api_key = "00000000-0000-0000-0000-000000000002"
secret = "poly-secret"
passphrase = "poly-passphrase"

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
condition_id = "0x0000000000000000000000000000000000000000000000000000000000000001"
token_id = "29"
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
    assert_section_summary(&combined, "Connectivity", "PASS");
    assert!(
        combined.contains("relayer reachability probe succeeded"),
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

[strategy_control]
source = "adopted"

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
api_key = "00000000-0000-0000-0000-000000000002"
secret = "poly-secret"
passphrase = "poly-passphrase"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-a"]

[[negrisk.targets]]
family_id = "family-a"

[[negrisk.targets.members]]
condition_id = "0x0000000000000000000000000000000000000000000000000000000000000001"
token_id = "29"
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

[strategy_control]
source = "adopted"

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
api_key = "00000000-0000-0000-0000-000000000002"
secret = "poly-secret"
passphrase = "poly-passphrase"

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
        combined.contains("configured operator strategy revision"),
        "{combined}"
    );
    assert!(
        combined.contains("active operator strategy revision"),
        "{combined}"
    );
    assert!(combined.contains("restart not needed"), "{combined}");
    assert!(
        !combined.contains("[SKIP] control-plane checks not required for explicit targets"),
        "{combined}"
    );
}

#[tokio::test]
async fn doctor_pure_neutral_adopted_config_does_not_take_explicit_targets_shortcut() {
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

[strategy_control]
source = "adopted"
operator_strategy_revision = "targets-rev-9"

[strategies.full_set]
enabled = true

[strategies.neg_risk]
enabled = true

[strategies.neg_risk.rollout]
approved_scopes = ["family-a"]
ready_scopes = ["family-a"]

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
api_key = "00000000-0000-0000-0000-000000000002"
secret = "poly-secret"
passphrase = "poly-passphrase"
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
    assert!(
        !combined.contains("[SKIP] control-plane checks not required for explicit targets"),
        "{combined}"
    );
}

#[tokio::test]
async fn doctor_pure_neutral_adopted_config_uses_strategy_anchor_when_distinct_from_target() {
    let database = TestDatabase::new().await;
    let venue = MockDoctorVenue::success();
    database
        .seed_distinct_strategy_adoption_with_runtime_progress(
            "strategy-candidate-12",
            "adoptable-strategy-12",
            "strategy-rev-12",
            "targets-rev-9",
            "targets-rev-active",
            "strategy-rev-12",
        )
        .await;
    let config = temp_live_config(&format!(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-12"

[strategies.full_set]
enabled = true

[strategies.neg_risk]
enabled = true

[strategies.neg_risk.rollout]
approved_scopes = ["family-a"]
ready_scopes = ["family-a"]

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
api_key = "00000000-0000-0000-0000-000000000002"
secret = "poly-secret"
passphrase = "poly-passphrase"
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
    assert!(
        combined.contains("configured operator strategy revision: strategy-rev-12"),
        "{combined}"
    );
    assert!(
        combined.contains("active operator strategy revision: strategy-rev-12"),
        "{combined}"
    );
    assert!(!combined.contains("restart needed"), "{combined}");
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

[strategy_control]
source = "adopted"

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
api_key = "00000000-0000-0000-0000-000000000001"
secret = "poly-secret-1"
passphrase = "poly-passphrase-1"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-a"]

[[negrisk.targets]]
family_id = "family-a"

[[negrisk.targets.members]]
condition_id = "0x0000000000000000000000000000000000000000000000000000000000000001"
token_id = "29"
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

[strategy_control]
source = "adopted"

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
api_key = "00000000-0000-0000-0000-000000000002"
secret = "poly-secret"
passphrase = "poly-passphrase"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-a"]

[[negrisk.targets]]
family_id = "family-a"

[[negrisk.targets.members]]
condition_id = "0x0000000000000000000000000000000000000000000000000000000000000001"
token_id = "29"
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

[strategy_control]
source = "adopted"

[polymarket.source]
clob_host = "ftp://clob.polymarket.com"
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
api_key = "00000000-0000-0000-0000-000000000001"
secret = "poly-secret-1"
passphrase = "poly-passphrase-1"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-a"]

[[negrisk.targets]]
family_id = "family-a"

[[negrisk.targets.members]]
condition_id = "0x0000000000000000000000000000000000000000000000000000000000000001"
token_id = "29"
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
    command.env("POLYMARKET_PRIVATE_KEY", TEST_PRIVATE_KEY);
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
    fs::write(temp.path(), normalize_sdk_fixture(contents.to_owned())).expect("write temp config");
    temp
}

#[test]
fn normalize_sdk_fixture_only_rewrites_polymarket_account_fields() {
    let normalized = normalize_sdk_fixture(
        r#"
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
api_key = "poly-api-key-1"
address = "0x1111111111111111111111111111111111111111"

[probe_fixture]
secret = "poly-secret-1"
passphrase = "poly-passphrase-1"
"#
        .to_owned(),
    );

    assert!(normalized.contains(r#"address = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266""#));
    assert!(normalized.contains(r#"funder_address = "0x2222222222222222222222222222222222222222""#));
    assert!(normalized.contains(r#"api_key = "00000000-0000-0000-0000-000000000002""#));
    assert!(normalized.contains(r#"secret = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=""#));
    assert!(normalized.contains(
        r#"passphrase = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb""#
    ));
    assert!(normalized.contains(r#"api_key = "poly-api-key-1""#));
    assert!(normalized.contains(r#"secret = "poly-secret-1""#));
    assert!(normalized.contains(r#"passphrase = "poly-passphrase-1""#));
    assert!(normalized.contains(r#"address = "0x1111111111111111111111111111111111111111""#));
}

fn normalize_sdk_fixture(config: String) -> String {
    let mut document = config.parse::<DocumentMut>().expect("parse sdk fixture");
    let Some(account) = document
        .get_mut("polymarket")
        .and_then(|polymarket| polymarket.get_mut("account"))
        .and_then(|account| account.as_table_like_mut())
    else {
        return config;
    };

    rewrite_fixture_string(
        account,
        "address",
        "0x1111111111111111111111111111111111111111",
        TEST_SIGNER_ADDRESS,
    );
    rewrite_fixture_string(
        account,
        "api_key",
        "poly-api-key-1",
        "00000000-0000-0000-0000-000000000001",
    );
    rewrite_fixture_string(
        account,
        "api_key",
        "poly-api-key",
        "00000000-0000-0000-0000-000000000002",
    );
    rewrite_fixture_string(account, "secret", "poly-secret-1", TEST_PRIMARY_SECRET);
    rewrite_fixture_string(account, "secret", "poly-secret", TEST_SECONDARY_SECRET);
    rewrite_fixture_string(
        account,
        "passphrase",
        "poly-passphrase-1",
        TEST_PRIMARY_PASSPHRASE,
    );
    rewrite_fixture_string(
        account,
        "passphrase",
        "poly-passphrase",
        TEST_SECONDARY_PASSPHRASE,
    );

    document.to_string()
}

fn rewrite_fixture_string(table: &mut dyn TableLike, key: &str, from: &str, to: &str) {
    let Some(item) = table.get_mut(key) else {
        return;
    };
    let Some(current) = item.as_str() else {
        return;
    };
    if current == from {
        *item = value(to);
    }
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
                                        "condition_id": "0x0000000000000000000000000000000000000000000000000000000000000001",
                                        "token_id": "29",
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

    async fn seed_distinct_strategy_adoption_with_runtime_progress(
        &self,
        strategy_candidate_revision: &str,
        adoptable_strategy_revision: &str,
        operator_strategy_revision: &str,
        rendered_operator_target_revision: &str,
        active_operator_target_revision: &str,
        active_operator_strategy_revision: &str,
    ) {
        StrategyControlArtifactRepo
            .upsert_strategy_candidate_set(
                &self.pool,
                &StrategyCandidateSetRow {
                    strategy_candidate_revision: strategy_candidate_revision.to_owned(),
                    snapshot_id: "snapshot-strategy-12".to_owned(),
                    source_revision: "discovery-strategy-12".to_owned(),
                    payload: json!({
                        "strategy_candidate_revision": strategy_candidate_revision,
                        "snapshot_id": "snapshot-strategy-12",
                    }),
                },
            )
            .await
            .expect("strategy candidate row should persist");

        StrategyControlArtifactRepo
            .upsert_adoptable_strategy_revision(
                &self.pool,
                &AdoptableStrategyRevisionRow {
                    adoptable_strategy_revision: adoptable_strategy_revision.to_owned(),
                    strategy_candidate_revision: strategy_candidate_revision.to_owned(),
                    rendered_operator_strategy_revision: operator_strategy_revision.to_owned(),
                    payload: json!({
                        "adoptable_strategy_revision": adoptable_strategy_revision,
                        "strategy_candidate_revision": strategy_candidate_revision,
                        "rendered_operator_strategy_revision": operator_strategy_revision,
                        "rendered_operator_target_revision": rendered_operator_target_revision,
                        "rendered_live_targets": {
                            "family-a": {
                                "family_id": "family-a",
                                "members": [
                                    {
                                        "condition_id": "0x0000000000000000000000000000000000000000000000000000000000000001",
                                        "token_id": "29",
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
            .expect("adoptable strategy row should persist");

        StrategyAdoptionRepo
            .upsert_provenance(
                &self.pool,
                &StrategyAdoptionProvenanceRow {
                    operator_strategy_revision: operator_strategy_revision.to_owned(),
                    adoptable_strategy_revision: adoptable_strategy_revision.to_owned(),
                    strategy_candidate_revision: strategy_candidate_revision.to_owned(),
                },
            )
            .await
            .expect("strategy provenance should persist");

        RuntimeProgressRepo
            .record_progress_with_strategy_revision(
                &self.pool,
                41,
                7,
                Some("snapshot-7"),
                Some(active_operator_target_revision),
                Some(active_operator_strategy_revision),
                None,
            )
            .await
            .expect("runtime progress should seed with strategy and target anchors");
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
    ws: ProbeWsServer,
}

impl MockDoctorVenue {
    fn success() -> Self {
        Self {
            http: ProbeHttpServer::spawn(ProbeHttpBehavior::success()),
            ws: ProbeWsServer::spawn(),
        }
    }

    fn orders_fail(status_line: &str, body: &str) -> Self {
        Self {
            http: ProbeHttpServer::spawn(ProbeHttpBehavior::orders_fail(status_line, body)),
            ws: ProbeWsServer::spawn(),
        }
    }

    fn http_base_url(&self) -> &str {
        self.http.base_url()
    }

    fn market_ws_url(&self) -> &str {
        self.ws.market_url()
    }

    fn user_ws_url(&self) -> &str {
        self.ws.user_url()
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
            orders_body: r#"{"data":[],"next_cursor":"LTE=","limit":0,"count":0}"#.to_owned(),
            heartbeat_status_line: "200 OK".to_owned(),
            heartbeat_body:
                r#"{"success":true,"heartbeat_id":"00000000-0000-0000-0000-000000000001"}"#
                    .to_owned(),
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
) -> (&'a str, String) {
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let path = target.split('?').next().unwrap_or_default();

    match (method, path) {
        ("GET", "/data/orders") => (&behavior.orders_status_line, behavior.orders_body.clone()),
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
    base_url: String,
    market_url: String,
    user_url: String,
    shutdown_tx: Option<mpsc::Sender<()>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl ProbeWsServer {
    fn spawn() -> Self {
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
                    let mut requested_path = None::<String>;
                    let mut websocket =
                        accept_websocket(stream, |request: &WsRequest, response: WsResponse| {
                            requested_path = Some(request.uri().path().to_owned());
                            Ok(response)
                        })
                        .expect("accept ws probe websocket");
                    loop {
                        match websocket.read() {
                            Ok(WsMessage::Text(text)) => {
                                websocket
                                    .send(WsMessage::Text(
                                        response_payload_for_request(
                                            requested_path
                                                .as_deref()
                                                .expect("ws request path should be captured"),
                                            &text,
                                        )
                                        .into(),
                                    ))
                                    .expect("send ws probe response");
                                break;
                            }
                            Ok(_) => {}
                            Err(_) => break,
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("ws probe server accept failed: {error}"),
            }
        });

        Self {
            base_url: format!("ws://{address}"),
            market_url: format!("ws://{address}/ws/market"),
            user_url: format!("ws://{address}/ws/user"),
            shutdown_tx: Some(shutdown_tx),
            handle: Some(handle),
        }
    }

    fn market_url(&self) -> &str {
        &self.market_url
    }

    fn user_url(&self) -> &str {
        &self.user_url
    }

    #[allow(dead_code)]
    fn base_url(&self) -> &str {
        &self.base_url
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

fn response_payload_for_request(path: &str, _request: &str) -> &'static str {
    if path == "/ws/user" {
        r#"{"event_type":"trade","id":"trade-1","market":"0x0000000000000000000000000000000000000000000000000000000000000001","asset_id":"29","side":"BUY","size":"100","price":"0.41","status":"MATCHED","timestamp":"1750428146322"}"#
    } else if path == "/ws/market" {
        r#"{"event_type":"last_trade_price","asset_id":"29","market":"0x0000000000000000000000000000000000000000000000000000000000000001","price":"0.40","side":"BUY","size":"100","fee_rate_bps":"15","timestamp":"1750428146322"}"#
    } else {
        panic!("unexpected websocket path: {path}");
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
