#[path = "support/apply_db.rs"]
mod apply_db;
#[path = "support/cli.rs"]
mod cli;
#[path = "support/run_session_db.rs"]
mod run_session_db;

use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

use persistence::RunSessionState;

static NEXT_TEMP_CONFIG_ID: AtomicU64 = AtomicU64::new(1);
const TEST_PRIVATE_KEY: &str = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

#[test]
fn run_subcommand_starts_paper_mode_from_operator_config() {
    let db = run_session_db::TestDatabase::new();

    let output = Command::new(cli::app_live_binary())
        .arg("run")
        .arg("--config")
        .arg(cli::config_fixture("app-live-paper.toml"))
        .env("DATABASE_URL", db.database_url())
        .output()
        .expect("app-live run should execute");

    assert!(output.status.success(), "{}", cli::combined(&output));
    assert!(cli::combined(&output).contains("app_mode=paper"));

    db.cleanup();
}

#[test]
fn run_paper_creates_running_then_exited_session() {
    let db = run_session_db::TestDatabase::new();

    let output = Command::new(cli::app_live_binary())
        .arg("run")
        .arg("--config")
        .arg(cli::config_fixture("app-live-paper.toml"))
        .env("DATABASE_URL", db.database_url())
        .output()
        .expect("paper run should execute");

    assert!(output.status.success(), "{}", cli::combined(&output));

    let session = db.latest_session().expect("paper run session should exist");
    assert_eq!(session.mode, "paper");
    assert_eq!(session.invoked_by, "run");
    assert_eq!(session.state, RunSessionState::Exited);

    db.cleanup();
}

#[test]
fn run_startup_failure_records_failed_session() {
    let db = run_session_db::TestDatabase::new();

    let output = Command::new(cli::app_live_binary())
        .arg("run")
        .arg("--config")
        .arg(cli::config_fixture("app-live-ux-live.toml"))
        .env("DATABASE_URL", db.database_url())
        .output()
        .expect("broken live run should execute");

    assert!(!output.status.success(), "{}", cli::combined(&output));

    let session = db
        .latest_session()
        .expect("failed run session should exist");
    assert_eq!(session.mode, "live");
    assert_eq!(session.invoked_by, "run");
    assert_eq!(session.state, RunSessionState::Failed);

    db.cleanup();
}

#[test]
fn run_live_input_reports_provenance_chain_failure_and_not_compatibility() {
    let db = run_session_db::TestDatabase::new();
    let config_path = temp_config_fixture_path("app-live-ux-live.toml", |config| config);

    let output = Command::new(cli::app_live_binary())
        .arg("run")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", db.database_url())
        .output()
        .expect("legacy run should execute");

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(
        text.contains("could not be linked back to a candidate adoption provenance chain"),
        "{text}"
    );
    assert!(!text.contains("compatibility"), "{text}");

    db.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn run_smoke_mode_builds_shared_source_bundle_via_app_facing_run_path() {
    let db = apply_db::TestDatabase::new();
    db.seed_adopted_target_with_active_revision("targets-rev-9", None);
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", normalize_sdk_fixture);

    let output = app_live_command()
        .arg("run")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", db.database_url())
        .output()
        .expect("smoke run should execute");

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("app_mode=live"), "{text}");
    assert!(text.contains("negrisk_mode=Shadow"), "{text}");

    db.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn run_eoa_non_shadow_live_fails_closed_before_relayer_backed_runtime_work() {
    let db = apply_db::TestDatabase::new();
    db.seed_adopted_target_with_active_revision("targets-rev-9", None);
    let config_path =
        temp_config_fixture_path("app-live-ux-smoke.toml", normalize_non_shadow_live_fixture);

    let output = app_live_command()
        .arg("run")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", db.database_url())
        .env("POLYMARKET_PRIVATE_KEY", TEST_PRIVATE_KEY)
        .output()
        .expect("live run should execute");

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(
        text.contains("EOA non-shadow live runtime is not supported"),
        "{text}"
    );

    db.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn run_non_eoa_non_shadow_live_does_not_trip_eoa_fail_closed_gate() {
    let db = apply_db::TestDatabase::new();
    db.seed_adopted_target_with_active_revision("targets-rev-9", None);
    let config_path = temp_config_fixture_path(
        "app-live-ux-smoke.toml",
        normalize_non_eoa_non_shadow_live_fixture,
    );

    let output = app_live_command()
        .arg("run")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", db.database_url())
        .env_remove("POLYMARKET_PRIVATE_KEY")
        .output()
        .expect("live run should execute");

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(
        !text.contains("EOA non-shadow live runtime is not supported"),
        "{text}"
    );
    assert!(text.contains("POLYMARKET_PRIVATE_KEY"), "{text}");

    db.cleanup();
    let _ = fs::remove_file(config_path);
}

fn app_live_command() -> Command {
    let mut command = Command::new(cli::app_live_binary());
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

fn temp_config_fixture_path(relative: &str, edit: impl FnOnce(String) -> String) -> PathBuf {
    let source = cli::config_fixture(relative);
    let text = fs::read_to_string(&source).expect("fixture should be readable");
    let edited = edit(text);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "app-live-run-{}-{}.toml",
        std::process::id(),
        NEXT_TEMP_CONFIG_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, edited).expect("temp fixture should be writable");
    path
}

fn normalize_sdk_fixture(config: String) -> String {
    config.replace("poly-api-key", "00000000-0000-0000-0000-000000000002")
}

fn normalize_non_shadow_live_fixture(config: String) -> String {
    normalize_sdk_fixture(config)
        .replace(
            "real_user_shadow_smoke = true",
            "real_user_shadow_smoke = false",
        )
        .replace("approved_scopes = []", "approved_scopes = [\"family-a\"]")
        .replace("ready_scopes = []", "ready_scopes = [\"family-a\"]")
}

fn normalize_non_eoa_non_shadow_live_fixture(config: String) -> String {
    format!(
        "{}\n\n[polymarket.relayer_auth]\nkind = \"relayer_api_key\"\napi_key = \"relay-key\"\naddress = \"0x1111111111111111111111111111111111111111\"\n",
        normalize_non_shadow_live_fixture(config)
            .replace("signature_type = \"eoa\"", "signature_type = \"proxy\"")
            .replace("wallet_route = \"eoa\"", "wallet_route = \"proxy\"")
    )
}
