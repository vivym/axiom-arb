use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

use config_schema::{load_raw_config_from_str, ValidatedConfig};
use persistence::{
    models::{AdoptableTargetRevisionRow, CandidateAdoptionProvenanceRow, CandidateTargetSetRow},
    run_migrations, CandidateAdoptionRepo, CandidateArtifactRepo,
};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool};

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
fn bootstrap_existing_smoke_config_fails_with_clear_in_scope_message() {
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
operator_target_revision = "targets-rev-9"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = []
"#,
    )
    .expect("seed smoke config");

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
        combined.contains("bootstrap smoke follow-through is not implemented yet"),
        "{combined}"
    );
    assert!(
        combined.contains("app-live targets status --config"),
        "{combined}"
    );
    assert!(
        combined.contains("app-live targets show-current --config"),
        "{combined}"
    );
    assert!(
        combined.contains(&config_path.display().to_string()),
        "{combined}"
    );
    assert!(
        !combined.contains("Smoke bootstrap config written"),
        "{combined}"
    );
    assert!(
        !combined.contains("app-live targets adopt --config"),
        "{combined}"
    );
}

#[test]
fn bootstrap_existing_smoke_config_without_target_anchor_points_to_candidates_and_adopt() {
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
        .env("DATABASE_URL", default_test_database_url())
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
}

#[test]
fn bootstrap_smoke_inlines_adopt_when_target_anchor_missing() {
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
        .write_all(b"adoptable-9\n")
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
