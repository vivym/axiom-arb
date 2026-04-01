use std::{
    fs,
    path::PathBuf,
    process::Command,
};

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
        fs::read_to_string(&config_path)
            .expect("generated config should exist")
            == "[runtime]\nmode = \"paper\"\n",
        "expected paper-only config at {}",
        config_path.display(),
    );
    let combined = combined(&output);
    assert!(combined.contains("Paper bootstrap ready"), "{}", combined);
    let expected_summary =
        "Runtime not started. Re-run with --start or run app-live run --config config/axiom-arb.local.toml";
    assert!(combined.contains(expected_summary), "{}", combined);
    assert!(
        combined.contains("app-live run --config config/axiom-arb.local.toml"),
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
    assert!(combined.contains(&temp.path().display().to_string()), "{}", combined);
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
