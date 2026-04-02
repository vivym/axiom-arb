use std::{
    fs,
    io::{Read, Write},
    net::TcpListener as StdTcpListener,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use config_schema::{load_raw_config_from_str, ValidatedConfig};
use persistence::{
    models::{AdoptableTargetRevisionRow, CandidateAdoptionProvenanceRow, CandidateTargetSetRow},
    run_migrations, CandidateAdoptionRepo, CandidateArtifactRepo,
};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool};
use tokio_tungstenite::tungstenite::{accept as accept_websocket, Message as WsMessage};

static NEXT_SCHEMA_ID: AtomicU64 = AtomicU64::new(1);

#[test]
fn bootstrap_help_lists_command() {
    let output = Command::new(app_live_binary())
        .arg("--help")
        .output()
        .unwrap();

    assert!(output.status.success(), "{}", combined(&output));
    assert!(
        combined(&output).contains("bootstrap"),
        "expected `bootstrap` in help output, got:\n{}",
        combined(&output)
    );
}

#[test]
fn bootstrap_defaults_to_local_config_for_paper() {
    let temp = tempfile::tempdir().expect("temp dir");
    let config_path = temp.path().join("config").join("axiom-arb.local.toml");

    let output = Command::new(app_live_binary())
        .arg("bootstrap")
        .current_dir(temp.path())
        .env("DATABASE_URL", default_test_database_url())
        .output()
        .expect("app-live bootstrap should execute");

    assert!(output.status.success(), "{}", combined(&output));
    assert!(
        config_path.exists(),
        "expected default config path to exist: {}",
        config_path.display()
    );
    assert!(
        fs::read_to_string(&config_path).expect("generated config should exist")
            == "[runtime]\nmode = \"paper\"\n",
        "expected paper-only config at {}",
        config_path.display(),
    );
    let combined = combined(&output);
    assert!(combined.contains("Paper bootstrap ready"), "{}", combined);
    let expected_summary =
        "Runtime not started. Re-run with --start or use: app-live run --config 'config/axiom-arb.local.toml'";
    assert!(combined.contains(expected_summary), "{}", combined);
    assert!(
        combined.contains("Next: app-live run --config 'config/axiom-arb.local.toml'"),
        "{}",
        combined
    );
    assert!(
        !combined.contains("app-live -- run --config"),
        "{}",
        combined
    );
    assert!(
        !combined.contains("Choose an init mode:"),
        "bootstrap should stay paper-only, got:\n{}",
        combined
    );
}

#[test]
fn bootstrap_quotes_config_path_with_spaces_in_follow_up_commands() {
    let temp = tempfile::tempdir().expect("temp dir");
    let config_dir = temp.path().join("config with spaces");
    let config_path = config_dir.join("paper config.toml");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(&config_path, "[runtime]\nmode = \"paper\"\n").expect("seed paper config");

    let output = Command::new(app_live_binary())
        .arg("bootstrap")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", default_test_database_url())
        .output()
        .expect("app-live bootstrap should execute");

    assert!(output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    let quoted_path = format!("'{}'", config_path.display());
    assert!(
        combined.contains(&format!("Next: app-live run --config {quoted_path}")),
        "{}",
        combined
    );
    assert!(
        combined.contains(&format!(
            "Runtime not started. Re-run with --start or use: app-live run --config {quoted_path}"
        )),
        "{}",
        combined
    );
}

#[test]
fn bootstrap_paper_start_runs_runtime_after_preflight() {
    let temp = tempfile::tempdir().expect("temp dir");

    let output = Command::new(app_live_binary())
        .arg("bootstrap")
        .arg("--start")
        .current_dir(temp.path())
        .env("DATABASE_URL", default_test_database_url())
        .output()
        .expect("app-live bootstrap should execute");

    assert!(output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    let doctor_index = combined
        .find("Overall: PASS WITH SKIPS")
        .expect("doctor preflight summary should be present");
    let start_index = combined
        .find("Paper bootstrap ready. Starting runtime with")
        .expect("bootstrap should announce runtime startup");
    let runtime_index = combined
        .find("app_mode=paper")
        .expect("paper runtime output should be present");
    assert!(
        doctor_index < start_index && start_index < runtime_index,
        "expected doctor preflight before runtime startup, got:\n{}",
        combined
    );
}

#[test]
fn bootstrap_rejects_live_config_with_visible_paper_only_error() {
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
    .expect("seed live config");

    let output = Command::new(app_live_binary())
        .arg("bootstrap")
        .arg("--config")
        .arg(temp.path())
        .env("DATABASE_URL", default_test_database_url())
        .output()
        .expect("app-live bootstrap should execute");

    assert!(!output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert!(
        combined.contains("bootstrap currently supports only paper configs for this flow"),
        "{}",
        combined
    );
    assert!(
        combined.contains(&temp.path().display().to_string()),
        "{}",
        combined
    );
}

#[test]
fn bootstrap_reuses_init_doctor_run_semantics_for_paper() {
    let bootstrap_temp = tempfile::tempdir().expect("bootstrap temp dir");
    let bootstrap_config_path = bootstrap_temp
        .path()
        .join("config")
        .join("axiom-arb.local.toml");

    let bootstrap_output = Command::new(app_live_binary())
        .arg("bootstrap")
        .current_dir(bootstrap_temp.path())
        .env("DATABASE_URL", default_test_database_url())
        .output()
        .expect("app-live bootstrap should execute");

    assert!(
        bootstrap_output.status.success(),
        "{}",
        combined(&bootstrap_output)
    );

    let init_temp = tempfile::tempdir().expect("init temp dir");
    let init_config_path = init_temp.path().join("config").join("axiom-arb.local.toml");

    let mut init_child = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(&init_config_path)
        .current_dir(init_temp.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("app-live init should spawn");

    use std::io::Write;
    init_child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"paper\n")
        .expect("paper answer should write");

    let init_output = init_child.wait_with_output().expect("init output");
    assert!(init_output.status.success(), "{}", combined(&init_output));

    assert_eq!(
        fs::read_to_string(&bootstrap_config_path).expect("bootstrap config"),
        fs::read_to_string(&init_config_path).expect("init config"),
    );

    let bootstrap_text = combined(&bootstrap_output);
    assert!(
        bootstrap_text.contains("Overall: PASS WITH SKIPS"),
        "{bootstrap_text}"
    );
    assert!(
        bootstrap_text.contains("Next: app-live run --config 'config/axiom-arb.local.toml'"),
        "{bootstrap_text}"
    );
    assert!(
        !bootstrap_text.contains("Choose an init mode:"),
        "{bootstrap_text}"
    );

    let init_text = combined(&init_output);
    assert!(init_text.contains("What Was Written"), "{init_text}");
    assert!(init_text.contains("[runtime]"), "{init_text}");
    assert!(init_text.contains("mode = \"paper\""), "{init_text}");
    assert!(
        init_text.contains(&format!(
            "app-live doctor --config '{}'",
            init_config_path.display()
        )),
        "{init_text}"
    );
    assert!(
        init_text.contains(&format!(
            "app-live run --config '{}'",
            init_config_path.display()
        )),
        "{init_text}"
    );
}

#[test]
fn bootstrap_smoke_completes_local_config() {
    let temp = tempfile::tempdir().expect("temp dir");
    let config_path = temp.path().join("config").join("axiom-arb.local.toml");

    let mut child = Command::new(app_live_binary())
        .arg("bootstrap")
        .current_dir(temp.path())
        .env("DATABASE_URL", default_test_database_url())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live bootstrap should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            b"smoke\n0x1111111111111111111111111111111111111111\n\npoly-api-key-1\npoly-secret-1\npoly-passphrase-1\nrelayer_api_key\nrelay-key-1\n0x2222222222222222222222222222222222222222\n",
        )
        .expect("wizard answers should write");

    let output = child.wait_with_output().expect("output");
    assert!(output.status.success(), "{}", combined(&output));

    let text = fs::read_to_string(&config_path).expect("generated config should exist");
    let raw = load_raw_config_from_str(&text).expect("generated config should parse");
    let validated = ValidatedConfig::new(raw).expect("generated config should validate");
    let live = validated
        .for_app_live()
        .expect("generated live config should validate for app-live");

    assert_eq!(live.mode(), config_schema::RuntimeModeToml::Live);
    assert!(live.real_user_shadow_smoke());
    let target_source = live.target_source().expect("target source should exist");
    assert!(target_source.is_adopted());
    assert!(target_source.operator_target_revision().is_none());

    let rollout = live.negrisk_rollout().expect("rollout should exist");
    assert!(rollout.approved_families().is_empty());
    assert!(rollout.ready_families().is_empty());

    let combined = combined(&output);
    assert!(!combined.contains("Choose a bootstrap mode:"), "{combined}");
    assert!(!combined.contains("Choose an init mode:"), "{combined}");
    assert!(
        combined.contains("app-live targets candidates --config"),
        "{combined}"
    );
    assert!(
        combined.contains("app-live targets adopt --config"),
        "{combined}"
    );
    assert!(!combined.contains("Paper bootstrap ready"), "{combined}");
    assert!(
        !combined.contains("Runtime not started. Re-run with --start"),
        "{combined}"
    );
    assert!(!combined.contains("Overall: PASS WITH SKIPS"), "{combined}");
}

#[test]
fn bootstrap_rejects_live_selection_without_persisting_live_config() {
    let temp = tempfile::tempdir().expect("temp dir");
    let config_path = temp.path().join("config").join("axiom-arb.local.toml");

    let mut child = Command::new(app_live_binary())
        .arg("bootstrap")
        .current_dir(temp.path())
        .env("DATABASE_URL", default_test_database_url())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live bootstrap should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"live\n")
        .expect("wizard answers should write");

    let output = child.wait_with_output().expect("output");
    assert!(!output.status.success(), "{}", combined(&output));
    assert!(
        !config_path.exists(),
        "unsupported live selection should not write {}",
        config_path.display()
    );

    let combined = combined(&output);
    assert!(!combined.contains("Choose a bootstrap mode:"), "{combined}");
    assert!(
        !combined.contains("Please choose one of the listed options."),
        "{combined}"
    );
    assert!(
        combined.contains("bootstrap only supports paper or smoke"),
        "{combined}"
    );
}

#[test]
fn bootstrap_smoke_start_fails_closed_without_writing_config() {
    let temp = tempfile::tempdir().expect("temp dir");
    let config_path = temp.path().join("config").join("axiom-arb.local.toml");

    let mut child = Command::new(app_live_binary())
        .arg("bootstrap")
        .arg("--start")
        .current_dir(temp.path())
        .env("DATABASE_URL", default_test_database_url())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live bootstrap should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"smoke\n")
        .expect("wizard answers should write");

    let output = child.wait_with_output().expect("output");
    assert!(!output.status.success(), "{}", combined(&output));
    assert!(
        !config_path.exists(),
        "smoke --start should fail before writing {}",
        config_path.display()
    );

    let combined = combined(&output);
    assert!(!combined.contains("Choose a bootstrap mode:"), "{combined}");
    assert!(
        combined.contains("bootstrap smoke does not support --start yet"),
        "{combined}"
    );
    assert!(
        !combined.contains("Smoke bootstrap config written"),
        "{combined}"
    );
}

#[test]
fn bootstrap_existing_smoke_config_without_target_anchor_points_to_candidates_and_adopt_when_no_adoptables_exist(
) {
    let database = BootstrapTestDatabase::new();
    let temp = tempfile::tempdir().expect("temp dir");
    let config_path = temp.path().join("axiom-arb.local.toml");
    fs::write(
        &config_path,
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

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key-1"
secret = "poly-secret-1"
passphrase = "poly-passphrase-1"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key-1"
address = "0x2222222222222222222222222222222222222222"

[negrisk.target_source]
source = "adopted"

[negrisk.rollout]
approved_families = []
ready_families = []
"#,
    )
    .expect("seed smoke config without target anchor");

    let output = Command::new(app_live_binary())
        .arg("bootstrap")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live bootstrap should execute");

    assert!(!output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert!(
        combined.contains("app-live targets candidates --config"),
        "{combined}"
    );
    assert!(
        combined.contains("app-live targets adopt --config"),
        "{combined}"
    );
    assert!(
        !combined.contains("app-live targets status --config"),
        "{combined}"
    );
    assert!(
        !combined.contains("app-live targets show-current --config"),
        "{combined}"
    );

    database.cleanup();
}

#[test]
fn bootstrap_smoke_inlines_adopt_when_target_anchor_missing() {
    let database = BootstrapTestDatabase::new();
    database.seed_adoptable_revision("adoptable-9", "candidate-9", "targets-rev-9");
    let venue = MockDoctorVenue::success();

    let temp = tempfile::tempdir().expect("temp dir");
    let config_path = temp.path().join("axiom-arb.local.toml");
    let smoke_config = format!(
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

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key-1"
secret = "poly-secret-1"
passphrase = "poly-passphrase-1"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key-1"
address = "0x2222222222222222222222222222222222222222"

[negrisk.target_source]
source = "adopted"

[negrisk.rollout]
approved_families = []
ready_families = []
"#,
        clob_host = venue.http_base_url(),
        data_api_host = venue.http_base_url(),
        relayer_host = venue.http_base_url(),
        market_ws_url = venue.market_ws_url(),
        user_ws_url = venue.user_ws_url(),
    );
    fs::write(&config_path, smoke_config).expect("seed smoke config without target anchor");

    let mut child = Command::new(app_live_binary())
        .arg("bootstrap")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live bootstrap should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"adoptable-9\npreflight-only\n")
        .expect("adoptable selection should write");

    let output = child.wait_with_output().expect("output");
    assert!(output.status.success(), "{}", combined(&output));

    let rewritten = fs::read_to_string(&config_path).expect("rewritten config should load");
    assert!(
        rewritten.contains("operator_target_revision = \"targets-rev-9\""),
        "{rewritten}"
    );

    database.cleanup();
}

#[test]
fn bootstrap_smoke_enables_rollout_for_adopted_families() {
    let database = BootstrapTestDatabase::new();
    database.seed_adoptable_revision("adoptable-9", "candidate-9", "targets-rev-9");
    let venue = MockDoctorVenue::success();

    let temp = tempfile::tempdir().expect("temp dir");
    let config_path = temp.path().join("axiom-arb.local.toml");
    let smoke_config = format!(
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

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key-1"
secret = "poly-secret-1"
passphrase = "poly-passphrase-1"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key-1"
address = "0x2222222222222222222222222222222222222222"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"

[negrisk.rollout]
approved_families = []
ready_families = []
"#,
        clob_host = venue.http_base_url(),
        data_api_host = venue.http_base_url(),
        relayer_host = venue.http_base_url(),
        market_ws_url = venue.market_ws_url(),
        user_ws_url = venue.user_ws_url(),
    );
    fs::write(&config_path, smoke_config).expect("seed adopted smoke config with empty rollout");

    let mut child = Command::new(app_live_binary())
        .arg("bootstrap")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live bootstrap should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"enable\n")
        .expect("rollout choice should write");

    let output = child.wait_with_output().expect("output");
    assert!(output.status.success(), "{}", combined(&output));

    let rewritten = fs::read_to_string(&config_path).expect("rewritten config should load");
    assert!(
        rewritten.contains("approved_families = [\"family-a\"]"),
        "{rewritten}"
    );
    assert!(
        rewritten.contains("ready_families = [\"family-a\"]"),
        "{rewritten}"
    );

    database.cleanup();
}

#[test]
fn bootstrap_start_runs_after_smoke_ready() {
    let database = BootstrapTestDatabase::new();
    database.seed_adoptable_revision("adoptable-9", "candidate-9", "targets-rev-9");
    let venue = MockDoctorVenue::success();

    let temp = tempfile::tempdir().expect("temp dir");
    let config_path = temp.path().join("axiom-arb.local.toml");
    let smoke_ready_config = format!(
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

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key-1"
secret = "poly-secret-1"
passphrase = "poly-passphrase-1"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key-1"
address = "0x2222222222222222222222222222222222222222"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-a"]
        "#,
        clob_host = venue.http_base_url(),
        data_api_host = venue.http_base_url(),
        relayer_host = venue.http_base_url(),
        market_ws_url = venue.market_ws_url(),
        user_ws_url = venue.user_ws_url(),
    );
    fs::write(&config_path, smoke_ready_config).expect("seed smoke-ready config");

    let output = Command::new(app_live_binary())
        .arg("bootstrap")
        .arg("--config")
        .arg(&config_path)
        .arg("--start")
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live bootstrap should execute");

    assert!(output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert!(
        combined.contains("Smoke bootstrap reached shadow-work-ready smoke startup"),
        "{combined}"
    );
    assert!(combined.contains("app_mode=live"), "{combined}");

    database.cleanup();
}

#[test]
fn bootstrap_smoke_summary_distinguishes_preflight_and_shadow_ready() {
    let database = BootstrapTestDatabase::new();
    database.seed_adoptable_revision("adoptable-9", "candidate-9", "targets-rev-9");
    let venue = MockDoctorVenue::success();

    let preflight_temp = tempfile::tempdir().expect("preflight temp dir");
    let preflight_config_path = preflight_temp.path().join("axiom-arb.local.toml");
    let smoke_config = format!(
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

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key-1"
secret = "poly-secret-1"
passphrase = "poly-passphrase-1"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key-1"
address = "0x2222222222222222222222222222222222222222"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"

[negrisk.rollout]
approved_families = []
ready_families = []
"#,
        clob_host = venue.http_base_url(),
        data_api_host = venue.http_base_url(),
        relayer_host = venue.http_base_url(),
        market_ws_url = venue.market_ws_url(),
        user_ws_url = venue.user_ws_url(),
    );
    fs::write(&preflight_config_path, &smoke_config).expect("seed preflight-only smoke config");

    let mut preflight_child = Command::new(app_live_binary())
        .arg("bootstrap")
        .arg("--config")
        .arg(&preflight_config_path)
        .env("DATABASE_URL", database.database_url())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("preflight-only bootstrap should spawn");

    preflight_child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"preflight-only\n")
        .expect("preflight-only selection should write");

    let preflight_output = preflight_child.wait_with_output().expect("output");
    assert!(
        preflight_output.status.success(),
        "{}",
        combined(&preflight_output)
    );
    let preflight_combined = combined(&preflight_output);
    assert!(
        preflight_combined.contains("preflight-ready smoke"),
        "{preflight_combined}"
    );

    let ready_temp = tempfile::tempdir().expect("ready temp dir");
    let ready_config_path = ready_temp.path().join("axiom-arb.local.toml");
    fs::write(&ready_config_path, smoke_config).expect("seed rollout-ready smoke config");

    let mut ready_child = Command::new(app_live_binary())
        .arg("bootstrap")
        .arg("--config")
        .arg(&ready_config_path)
        .env("DATABASE_URL", database.database_url())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("shadow-work-ready bootstrap should spawn");

    ready_child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"enable\n")
        .expect("rollout selection should write");

    let ready_output = ready_child.wait_with_output().expect("output");
    assert!(ready_output.status.success(), "{}", combined(&ready_output));
    let ready_combined = combined(&ready_output);
    assert!(
        ready_combined.contains("shadow-work-ready smoke"),
        "{ready_combined}"
    );

    database.cleanup();
}

#[test]
fn bootstrap_smoke_start_requires_rollout_readiness() {
    let database = BootstrapTestDatabase::new();
    database.seed_adoptable_revision("adoptable-9", "candidate-9", "targets-rev-9");
    let venue = MockDoctorVenue::success();

    let temp = tempfile::tempdir().expect("temp dir");
    let config_path = temp.path().join("axiom-arb.local.toml");
    let smoke_config = format!(
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

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key-1"
secret = "poly-secret-1"
passphrase = "poly-passphrase-1"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key-1"
address = "0x2222222222222222222222222222222222222222"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"

[negrisk.rollout]
approved_families = []
ready_families = []
"#,
        clob_host = venue.http_base_url(),
        data_api_host = venue.http_base_url(),
        relayer_host = venue.http_base_url(),
        market_ws_url = venue.market_ws_url(),
        user_ws_url = venue.user_ws_url(),
    );
    fs::write(&config_path, smoke_config).expect("seed adopted smoke config with empty rollout");

    let mut child = Command::new(app_live_binary())
        .arg("bootstrap")
        .arg("--config")
        .arg(&config_path)
        .arg("--start")
        .env("DATABASE_URL", database.database_url())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live bootstrap should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"preflight-only\n")
        .expect("preflight-only choice should write");

    let output = child.wait_with_output().expect("output");
    assert!(!output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert!(combined.contains("rollout readiness"), "{combined}");
    assert!(combined.contains("preflight-only"), "{combined}");

    database.cleanup();
}

#[test]
fn bootstrap_surfaces_doctor_report_and_next_actions() {
    let database = BootstrapTestDatabase::new();
    database.seed_adoptable_revision("adoptable-9", "candidate-9", "targets-rev-9");

    let temp = tempfile::tempdir().expect("temp dir");
    let config_path = temp.path().join("axiom-arb.local.toml");
    fs::write(
        &config_path,
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = true

[polymarket.source]
clob_host = "http://127.0.0.1:9"
data_api_host = "http://127.0.0.1:9"
relayer_host = "http://127.0.0.1:9"
market_ws_url = "ws://127.0.0.1:9/market"
user_ws_url = "ws://127.0.0.1:9/user"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key-1"
secret = "poly-secret-1"
passphrase = "poly-passphrase-1"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key-1"
address = "0x2222222222222222222222222222222222222222"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-a"]
"#,
    )
    .expect("seed shadow-work-ready smoke config");

    let output = Command::new(app_live_binary())
        .arg("bootstrap")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live bootstrap should execute");

    assert!(!output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert!(combined.contains("Connectivity: FAIL"), "{combined}");
    assert!(combined.contains("Overall: FAIL"), "{combined}");
    assert!(
        combined.contains("Next: fix the reported issue and rerun doctor"),
        "{combined}"
    );

    database.cleanup();
}

#[test]
fn bootstrap_existing_smoke_config_with_legacy_explicit_targets_surfaces_migration_steps() {
    let temp = tempfile::tempdir().expect("temp dir");
    let config_path = temp.path().join("axiom-arb.local.toml");
    fs::write(
        &config_path,
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

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key-1"
secret = "poly-secret-1"
passphrase = "poly-passphrase-1"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key-1"
address = "0x2222222222222222222222222222222222222222"

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
    )
    .expect("seed legacy explicit-target smoke config");

    let output = Command::new(app_live_binary())
        .arg("bootstrap")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", default_test_database_url())
        .output()
        .expect("app-live bootstrap should execute");

    assert!(!output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert!(
        combined.contains("still uses legacy explicit targets"),
        "{combined}"
    );
    assert!(
        combined.contains("rerun app-live bootstrap --config"),
        "{combined}"
    );
    assert!(
        combined.contains(&format!("'{}'", config_path.display())),
        "{combined}"
    );
}

#[test]
fn bootstrap_existing_smoke_config_without_target_source_or_explicit_targets_still_surfaces_validation_error(
) {
    let temp = tempfile::tempdir().expect("temp dir");
    let config_path = temp.path().join("axiom-arb.local.toml");
    fs::write(
        &config_path,
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

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key-1"
secret = "poly-secret-1"
passphrase = "poly-passphrase-1"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key-1"
address = "0x2222222222222222222222222222222222222222"

[negrisk.rollout]
approved_families = []
ready_families = []
"#,
    )
    .expect("seed malformed smoke config without target source or explicit targets");

    let output = Command::new(app_live_binary())
        .arg("bootstrap")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", default_test_database_url())
        .output()
        .expect("app-live bootstrap should execute");

    assert!(!output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert!(
        combined.contains("missing required section: negrisk.target_source"),
        "{combined}"
    );
    assert!(
        !combined.contains("still uses legacy explicit targets"),
        "{combined}"
    );
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

fn combined(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn default_test_database_url() -> &'static str {
    "postgres://axiom:axiom@localhost:5432/axiom_arb"
}

struct BootstrapTestDatabase {
    runtime: tokio::runtime::Runtime,
    admin_pool: PgPool,
    pool: PgPool,
    schema: String,
    database_url: String,
}

impl BootstrapTestDatabase {
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
                "app_live_bootstrap_{}_{}",
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

    fn seed_adoptable_revision(
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
}

fn schema_scoped_database_url(base: &str, schema: &str) -> String {
    let options = format!("options=-csearch_path%3D{schema}");
    if base.contains('?') {
        format!("{base}&{options}")
    } else {
        format!("{base}?{options}")
    }
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
