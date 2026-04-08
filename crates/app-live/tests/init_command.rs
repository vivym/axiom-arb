use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use config_schema::{load_raw_config_from_str, ValidatedConfig};

#[test]
fn init_preserve_updates_credentials_but_keeps_config_carried_operator_target_revision_and_rollout()
{
    let temp = tempfile::NamedTempFile::new().expect("temp file");
    fs::write(
        temp.path(),
        r#"
[runtime]
mode = "live"

[polymarket.account]
address = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
funder_address = "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
signature_type = "safe"
wallet_route = "safe"
api_key = "existing-account-api-key"
secret = "existing-account-secret"
passphrase = "existing-account-passphrase"

[polymarket.relayer_auth]
kind = "builder_api_key"
api_key = "existing-relay-key"
secret = "existing-relay-secret"
timestamp = "1700000001"
passphrase = "existing-relay-passphrase"
signature = "existing-relay-signature"
address = "0xcccccccccccccccccccccccccccccccccccccccc"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-b"]
"#,
    )
    .expect("seed existing config");

    let mut child = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live init should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            b"live\npreserve\nsafe\n0x1111111111111111111111111111111111111111\n\npoly-api-key-1\npoly-secret-1\npoly-passphrase-1\nbuilder_api_key\nrelay-key-1\nrelay-secret-1\nrelay-passphrase-1\n",
        )
        .expect("wizard answers should write");

    let output = child.wait_with_output().expect("output");
    assert!(output.status.success(), "{}", combined(&output));

    let text = fs::read_to_string(temp.path()).expect("generated config should exist");
    let raw = load_raw_config_from_str(&text).expect("generated config should parse");
    let validated = ValidatedConfig::new(raw).expect("generated config should validate");
    let live = validated
        .for_app_live()
        .expect("generated live config should validate");

    let account = live.account().expect("account should exist");
    assert_eq!(
        account.address(),
        "0x1111111111111111111111111111111111111111"
    );
    assert_eq!(
        account.funder_address(),
        Some("0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
    );
    assert_eq!(account.api_key(), "poly-api-key-1");
    assert_eq!(account.secret(), "poly-secret-1");
    assert_eq!(account.passphrase(), "poly-passphrase-1");
    assert_eq!(account.signature_type_label(), "Safe");
    assert_eq!(account.wallet_route_label(), "Safe");

    let relayer = live
        .polymarket_relayer_auth()
        .expect("relayer auth should exist");
    assert!(relayer.is_builder_api_key());
    assert_eq!(relayer.api_key(), "relay-key-1");
    assert_eq!(relayer.secret(), Some("relay-secret-1"));
    assert_eq!(relayer.passphrase(), Some("relay-passphrase-1"));
    assert_eq!(relayer.timestamp(), Some("1700000001"));
    assert_eq!(relayer.signature(), Some("existing-relay-signature"));
    assert_eq!(
        relayer.address(),
        Some("0xcccccccccccccccccccccccccccccccccccccccc")
    );

    let target_source = live.target_source().expect("target source should exist");
    assert_eq!(
        target_source.operator_target_revision(),
        Some("targets-rev-9")
    );

    let rollout = live.negrisk_rollout().expect("rollout should exist");
    assert_eq!(rollout.approved_families(), ["family-a"]);
    assert_eq!(rollout.ready_families(), ["family-b"]);

    assert!(!text.contains("existing-account-api-key"));
    assert!(!text.contains("existing-relay-key"));
}

#[test]
fn init_preserve_migrates_existing_source_block_to_source_overrides_when_present() {
    let temp = tempfile::NamedTempFile::new().expect("temp file");
    fs::write(
        temp.path(),
        r#"
[runtime]
mode = "live"

[polymarket.source]
clob_host = "https://custom-clob.example"
data_api_host = "https://custom-data-api.example"
relayer_host = "https://custom-relayer.example"
market_ws_url = "wss://custom-market.example"
user_ws_url = "wss://custom-user.example"
heartbeat_interval_seconds = 42
relayer_poll_interval_seconds = 7
metadata_refresh_interval_seconds = 99

[polymarket.account]
address = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
funder_address = "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "existing-account-api-key"
secret = "existing-account-secret"
passphrase = "existing-account-passphrase"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "existing-relay-key"
address = "0xcccccccccccccccccccccccccccccccccccccccc"
"#,
    )
    .expect("seed existing config");

    let mut child = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live init should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            b"live\npreserve\neoa\n0x1111111111111111111111111111111111111111\n\npoly-api-key-1\npoly-secret-1\npoly-passphrase-1\n",
        )
        .expect("wizard answers should write");

    let output = child.wait_with_output().expect("output");
    assert!(output.status.success(), "{}", combined(&output));

    let text = fs::read_to_string(temp.path()).expect("generated config should exist");
    let raw = load_raw_config_from_str(&text).expect("generated config should parse");
    let polymarket = raw
        .polymarket
        .as_ref()
        .expect("polymarket section should exist");
    assert!(!text.contains("[polymarket.source]"));
    assert!(text.contains("[polymarket.source_overrides]"));
    assert!(text.contains("clob_host = \"https://custom-clob.example\""));
    assert!(text.contains("metadata_refresh_interval_seconds = 99"));
    assert!(
        combined(&output)
            .contains("migrated existing [polymarket.source] into [polymarket.source_overrides]."),
        "{}",
        combined(&output)
    );
    assert!(polymarket.source.is_none());
    assert!(polymarket.source_overrides.as_ref().is_some());
}

#[test]
fn init_preserve_keeps_existing_source_overrides_block_when_present() {
    let temp = tempfile::NamedTempFile::new().expect("temp file");
    fs::write(
        temp.path(),
        r#"
[runtime]
mode = "live"

[polymarket.source_overrides]
clob_host = "https://override-clob.example"
data_api_host = "https://override-data-api.example"
relayer_host = "https://override-relayer.example"
market_ws_url = "wss://override-market.example/ws"
user_ws_url = "wss://override-user.example/ws"
heartbeat_interval_seconds = 41
relayer_poll_interval_seconds = 17
metadata_refresh_interval_seconds = 23

[polymarket.account]
address = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
funder_address = "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "existing-account-api-key"
secret = "existing-account-secret"
passphrase = "existing-account-passphrase"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "existing-relay-key"
address = "0xcccccccccccccccccccccccccccccccccccccccc"
"#,
    )
    .expect("seed existing config");

    let mut child = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live init should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            b"live\npreserve\neoa\n0x1111111111111111111111111111111111111111\n\npoly-api-key-1\npoly-secret-1\npoly-passphrase-1\n",
        )
        .expect("wizard answers should write");

    let output = child.wait_with_output().expect("output");
    assert!(output.status.success(), "{}", combined(&output));

    let text = fs::read_to_string(temp.path()).expect("generated config should exist");
    let raw = load_raw_config_from_str(&text).expect("generated config should parse");
    let polymarket = raw
        .polymarket
        .as_ref()
        .expect("polymarket section should exist");
    assert!(text.contains("[polymarket.source_overrides]"));
    assert!(text.contains("clob_host = \"https://override-clob.example\""));
    assert!(text.contains("metadata_refresh_interval_seconds = 23"));
    assert!(
        combined(&output).contains("preserved existing [polymarket.source_overrides]."),
        "{}",
        combined(&output)
    );
    assert!(polymarket.source.is_none());
    assert!(polymarket.source_overrides.as_ref().is_some());
}

#[test]
fn init_preserve_drops_legacy_source_block_when_source_overrides_already_present() {
    let temp = tempfile::NamedTempFile::new().expect("temp file");
    fs::write(
        temp.path(),
        r#"
[runtime]
mode = "live"

[polymarket.source]
clob_host = "https://source-clob.example"
data_api_host = "https://source-data-api.example"
relayer_host = "https://source-relayer.example"
market_ws_url = "wss://source-market.example/ws"
user_ws_url = "wss://source-user.example/ws"
heartbeat_interval_seconds = 42
relayer_poll_interval_seconds = 24
metadata_refresh_interval_seconds = 60

[polymarket.source_overrides]
clob_host = "https://override-clob.example"
data_api_host = "https://override-data-api.example"
relayer_host = "https://override-relayer.example"
market_ws_url = "wss://override-market.example/ws"
user_ws_url = "wss://override-user.example/ws"
heartbeat_interval_seconds = 41
relayer_poll_interval_seconds = 17
metadata_refresh_interval_seconds = 23

[polymarket.account]
address = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
funder_address = "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "existing-account-api-key"
secret = "existing-account-secret"
passphrase = "existing-account-passphrase"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "existing-relay-key"
address = "0xcccccccccccccccccccccccccccccccccccccccc"
"#,
    )
    .expect("seed existing config");

    let mut child = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live init should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            b"live\npreserve\neoa\n0x1111111111111111111111111111111111111111\n\npoly-api-key-1\npoly-secret-1\npoly-passphrase-1\n",
        )
        .expect("wizard answers should write");

    let output = child.wait_with_output().expect("output");
    assert!(output.status.success(), "{}", combined(&output));

    let text = fs::read_to_string(temp.path()).expect("generated config should exist");
    let raw = load_raw_config_from_str(&text).expect("generated config should parse");
    let polymarket = raw
        .polymarket
        .as_ref()
        .expect("polymarket section should exist");
    assert!(!text.contains("[polymarket.source]"));
    assert!(!text.contains("clob_host = \"https://source-clob.example\""));
    assert!(text.contains("[polymarket.source_overrides]"));
    assert!(text.contains("clob_host = \"https://override-clob.example\""));
    assert!(
        combined(&output).contains(
            "kept existing [polymarket.source_overrides] and dropped legacy [polymarket.source].",
        ),
        "{}",
        combined(&output)
    );
    assert!(polymarket.source.is_none());
    assert!(polymarket.source_overrides.as_ref().is_some());
}

#[test]
fn init_preserve_drops_stale_relayer_fields_when_auth_kind_changes() {
    let temp = tempfile::NamedTempFile::new().expect("temp file");
    fs::write(
        temp.path(),
        r#"
[runtime]
mode = "live"

[polymarket.account]
address = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
funder_address = "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
signature_type = "safe"
wallet_route = "safe"
api_key = "existing-account-api-key"
secret = "existing-account-secret"
passphrase = "existing-account-passphrase"

[polymarket.relayer_auth]
kind = "builder_api_key"
api_key = "existing-relay-key"
secret = "existing-relay-secret"
timestamp = "1700000001"
passphrase = "existing-relay-passphrase"
signature = "existing-relay-signature"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-b"]
"#,
    )
    .expect("seed existing config");

    let mut child = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live init should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            b"live\npreserve\nsafe\n0x1111111111111111111111111111111111111111\n\npoly-api-key-1\npoly-secret-1\npoly-passphrase-1\nrelayer_api_key\nrelay-key-1\n0x2222222222222222222222222222222222222222\n",
        )
        .expect("wizard answers should write");

    let output = child.wait_with_output().expect("output");
    assert!(output.status.success(), "{}", combined(&output));

    let text = fs::read_to_string(temp.path()).expect("generated config should exist");
    let raw = load_raw_config_from_str(&text).expect("generated config should parse");
    let validated = ValidatedConfig::new(raw).expect("generated config should validate");
    let live = validated
        .for_app_live()
        .expect("generated live config should validate");

    let account = live.account().expect("account should exist");
    assert_eq!(
        account.address(),
        "0x1111111111111111111111111111111111111111"
    );
    assert_eq!(
        account.funder_address(),
        Some("0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
    );

    let relayer = live
        .polymarket_relayer_auth()
        .expect("relayer auth should exist");
    assert_eq!(
        relayer.kind(),
        config_schema::AppLivePolymarketRelayerAuthKind::RelayerApiKey
    );
    assert_eq!(relayer.api_key(), "relay-key-1");
    assert_eq!(relayer.secret(), None);
    assert_eq!(relayer.timestamp(), None);
    assert_eq!(relayer.signature(), None);
    assert_eq!(
        relayer.address(),
        Some("0x2222222222222222222222222222222222222222")
    );

    assert!(text.contains("operator_target_revision = \"targets-rev-9\""));
    assert!(text.contains("approved_families = [\"family-a\"]"));
    assert!(text.contains("ready_families = [\"family-b\"]"));
    assert!(!text.contains("existing-relay-secret"));
    assert!(!text.contains("existing-relay-passphrase"));
    assert!(!text.contains("existing-relay-signature"));
}

#[test]
fn init_preserve_new_wallet_kind_overrides_stale_invalid_existing_account_kind() {
    let temp = tempfile::NamedTempFile::new().expect("temp file");
    let original = r#"
[runtime]
mode = "live"

[polymarket.account]
address = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
signature_type = "safe"
wallet_route = "eoa"
api_key = "existing-account-api-key"
secret = "existing-account-secret"
passphrase = "existing-account-passphrase"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-b"]
"#;
    fs::write(temp.path(), original).expect("seed invalid config");

    let mut child = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live init should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            b"live\npreserve\neoa\n0x1111111111111111111111111111111111111111\n\npoly-api-key-1\npoly-secret-1\npoly-passphrase-1\n",
        )
        .expect("wizard answers should write");

    let output = child.wait_with_output().expect("output");
    assert!(output.status.success(), "{}", combined(&output));
    let text = fs::read_to_string(temp.path()).expect("config should still exist");
    assert_ne!(text, original);
    let raw = load_raw_config_from_str(&text).expect("generated config should parse");
    let validated = ValidatedConfig::new(raw).expect("generated config should validate");
    let live = validated
        .for_app_live()
        .expect("generated live config should validate");
    let account = live.account().expect("account should exist");
    assert_eq!(account.signature_type_label(), "Eoa");
    assert_eq!(account.wallet_route_label(), "Eoa");
    assert!(live.polymarket_relayer_auth().is_none());
}

#[test]
fn init_replace_discards_existing_target_anchor_and_resets_rollout_to_safe_empty_lists() {
    let temp = tempfile::NamedTempFile::new().expect("temp file");
    fs::write(
        temp.path(),
        r#"
[runtime]
mode = "live"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-b"]
"#,
    )
    .expect("seed existing config");

    let mut child = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live init should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            b"live\nreplace\neoa\n0x1111111111111111111111111111111111111111\n\npoly-api-key-1\npoly-secret-1\npoly-passphrase-1\n",
        )
        .expect("wizard answers should write");

    let output = child.wait_with_output().expect("output");
    assert!(output.status.success(), "{}", combined(&output));

    let text = fs::read_to_string(temp.path()).expect("generated config should exist");
    assert!(!text.contains("operator_target_revision = \"targets-rev-9\""));
    assert!(text.contains("[negrisk.rollout]"));
    assert!(text.contains("approved_families = []"));
    assert!(text.contains("ready_families = []"));
    assert!(combined(&output).contains("Config already exists"));
}

#[test]
fn init_interactive_paper_writes_minimal_config_and_next_steps() {
    let temp = tempfile::NamedTempFile::new().expect("temp file");
    let mut child = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live init should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"paper\nreplace\n")
        .expect("wizard answers should write");

    let output = child.wait_with_output().expect("output");
    assert!(output.status.success(), "{}", combined(&output));
    let text = fs::read_to_string(temp.path()).expect("generated config should exist");
    assert!(text.contains("[runtime]"));
    assert!(text.contains("mode = \"paper\""));
    assert!(combined(&output).contains("What Was Written"));
    assert!(combined(&output).contains("What To Run Next"));
    assert!(combined(&output).contains("app-live doctor --config"));
    assert!(combined(&output).contains("app-live run --config"));
}

#[test]
fn init_interactive_paper_exits_on_eof_stdin() {
    let temp = tempfile::NamedTempFile::new().expect("temp file");
    let mut child = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(temp.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live init should spawn");

    let deadline = Instant::now() + Duration::from_secs(2);
    let output = loop {
        if let Some(status) = child.try_wait().expect("child status") {
            let output = child.wait_with_output().expect("output");
            assert!(!status.success(), "{}", combined(&output));
            break output;
        }

        if Instant::now() >= deadline {
            child.kill().expect("kill stalled child");
            panic!("init should exit when stdin reaches EOF");
        }

        thread::sleep(Duration::from_millis(20));
    };

    let text = combined(&output);
    assert!(text.contains("end of input"), "{text}");
}

#[test]
fn init_interactive_paper_quotes_next_step_config_path() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let config_path = temp_dir.path().join("config with spaces.toml");
    let mut child = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(&config_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live init should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"paper\nreplace\n")
        .expect("wizard answers should write");

    let output = child.wait_with_output().expect("output");
    assert!(output.status.success(), "{}", combined(&output));
    let text = combined(&output);
    assert!(text.contains("What To Run Next"));
    assert!(
        text.contains(&format!(
            "app-live doctor --config '{}'",
            config_path.display()
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "app-live run --config '{}'",
            config_path.display()
        )),
        "{text}"
    );
}

#[test]
fn init_without_operator_target_revision_points_operator_to_candidates_then_adopt() {
    let temp = tempfile::NamedTempFile::new().expect("temp file");
    let mut child = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live init should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            b"live\nreplace\neoa\n0x1111111111111111111111111111111111111111\n\npoly-api-key-1\npoly-secret-1\npoly-passphrase-1\n",
        )
        .expect("wizard answers should write");

    let output = child.wait_with_output().expect("output");
    assert!(output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert!(combined.contains("targets candidates"), "{combined}");
    assert!(combined.contains("targets adopt"), "{combined}");
    assert!(combined.contains("doctor"), "{combined}");
    assert!(combined.contains("run"), "{combined}");
}

#[test]
fn init_with_empty_rollout_warns_that_negrisk_work_remains_inactive() {
    let temp = tempfile::NamedTempFile::new().expect("temp file");
    let mut child = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live init should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            b"live\nreplace\neoa\n0x1111111111111111111111111111111111111111\n\npoly-api-key-1\npoly-secret-1\npoly-passphrase-1\n",
        )
        .expect("wizard answers should write");

    let output = child.wait_with_output().expect("output");
    assert!(output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert!(combined.contains("rollout is still empty"), "{combined}");
    assert!(
        combined.contains("negrisk work remains inactive"),
        "{combined}"
    );
}

#[test]
fn init_paper_summary_only_points_to_doctor_then_run() {
    let temp = tempfile::NamedTempFile::new().expect("temp file");
    let mut child = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live init should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"paper\nreplace\n")
        .expect("wizard answers should write");

    let output = child.wait_with_output().expect("output");
    assert!(output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert!(combined.contains("doctor"), "{combined}");
    assert!(combined.contains("run"), "{combined}");
    assert!(!combined.contains("targets candidates"), "{combined}");
    assert!(!combined.contains("targets adopt"), "{combined}");
}

#[test]
fn init_interactive_live_writes_eoa_account_target_source_and_safe_empty_rollout() {
    let temp = tempfile::NamedTempFile::new().expect("temp file");
    let mut child = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live init should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            b"live\nreplace\neoa\n0x1111111111111111111111111111111111111111\n\npoly-api-key-1\npoly-secret-1\npoly-passphrase-1\n",
        )
        .expect("wizard answers should write");

    let output = child.wait_with_output().expect("output");
    assert!(output.status.success(), "{}", combined(&output));

    let text = fs::read_to_string(temp.path()).expect("generated config should exist");
    assert!(text.contains("[runtime]"));
    assert!(text.contains("mode = \"live\""));
    assert!(text.contains("real_user_shadow_smoke = false"));
    assert!(text.contains("[polymarket.account]"));
    assert!(text.contains("address = \"0x1111111111111111111111111111111111111111\""));
    assert!(!text.contains("timestamp ="));
    assert!(!text.contains("signature ="));
    assert!(!text.contains("[polymarket.relayer_auth]"));
    assert!(text.contains("[negrisk.target_source]"));
    assert!(text.contains("source = \"adopted\""));
    assert!(!text.contains("operator_target_revision ="));
    assert!(text.contains("[negrisk.rollout]"));
    assert!(text.contains("approved_families = []"));
    assert!(text.contains("ready_families = []"));
    assert!(!text.contains("[polymarket.source]"));
    let combined = combined(&output);
    assert!(combined.contains("app-live targets candidates --config"));
    assert!(combined.contains("app-live targets adopt --config"));
    assert!(combined.contains("--adoptable-revision ADOPTABLE_REVISION"));
    assert!(combined.contains("app-live doctor --config"));
    assert!(combined.contains("app-live run --config"));
    assert!(!combined.contains("[polymarket.source]"));
    assert!(combined.contains("built-in defaults"));
    assert!(combined.contains("source_overrides"));
    assert!(!combined.contains("[polymarket.relayer_auth]"));
    assert_generated_live_config_is_schema_valid(&text);
}

#[test]
fn init_interactive_live_eoa_omits_relayer_auth() {
    let temp = tempfile::NamedTempFile::new().expect("temp file");
    let mut child = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live init should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            b"live\nreplace\neoa\n0x1111111111111111111111111111111111111111\n\npoly-api-key-1\npoly-secret-1\npoly-passphrase-1\n",
        )
        .expect("wizard answers should write");

    let output = child.wait_with_output().expect("output");
    assert!(output.status.success(), "{}", combined(&output));

    let text = fs::read_to_string(temp.path()).expect("generated config should exist");
    assert!(text.contains("[polymarket.account]"));
    assert!(!text.contains("[polymarket.relayer_auth]"));
    let combined = combined(&output);
    assert!(combined.contains("[polymarket.account]"), "{combined}");
    assert!(
        !combined.contains("[polymarket.relayer_auth]"),
        "{combined}"
    );
    assert_generated_live_config_is_schema_valid(&text);
}

#[test]
fn init_interactive_smoke_sets_live_mode_plus_shadow_guard() {
    let temp = tempfile::NamedTempFile::new().expect("temp file");
    let mut child = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live init should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            b"smoke\nreplace\neoa\n0x1111111111111111111111111111111111111111\n\npoly-api-key-1\npoly-secret-1\npoly-passphrase-1\n",
        )
        .expect("wizard answers should write");

    let output = child.wait_with_output().expect("output");
    assert!(output.status.success(), "{}", combined(&output));

    let text = fs::read_to_string(temp.path()).expect("generated config should exist");
    assert!(text.contains("mode = \"live\""));
    assert!(text.contains("real_user_shadow_smoke = true"));
    assert!(text.contains("[polymarket.account]"));
    assert!(!text.contains("[polymarket.relayer_auth]"));
    assert!(text.contains("[negrisk.target_source]"));
    assert!(text.contains("source = \"adopted\""));
    assert!(text.contains("approved_families = []"));
    assert!(text.contains("ready_families = []"));
    assert!(!text.contains("[polymarket.source]"));
    let combined = combined(&output);
    assert!(combined.contains("app-live targets candidates --config"));
    assert!(combined.contains("app-live targets adopt --config"));
    assert!(combined.contains("--adoptable-revision ADOPTABLE_REVISION"));
    assert!(combined.contains("app-live doctor --config"));
    assert!(combined.contains("app-live run --config"));
    assert!(!combined.contains("[polymarket.source]"));
    assert!(combined.contains("built-in defaults"));
    assert!(combined.contains("source_overrides"));
    assert!(!combined.contains("[polymarket.relayer_auth]"));
    assert_generated_live_config_is_schema_valid(&text);
}

#[test]
fn init_preserve_eoa_rewrites_stale_relayer_section_out_of_config() {
    let temp = tempfile::NamedTempFile::new().expect("temp file");
    fs::write(
        temp.path(),
        r#"
[runtime]
mode = "live"

[polymarket.account]
address = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
funder_address = "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "existing-account-api-key"
secret = "existing-account-secret"
passphrase = "existing-account-passphrase"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "existing-relay-key"
address = "0xcccccccccccccccccccccccccccccccccccccccc"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-b"]
"#,
    )
    .expect("seed existing config");

    let mut child = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live init should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            b"live\npreserve\neoa\n0x1111111111111111111111111111111111111111\n\npoly-api-key-1\npoly-secret-1\npoly-passphrase-1\n",
        )
        .expect("wizard answers should write");

    let output = child.wait_with_output().expect("output");
    assert!(output.status.success(), "{}", combined(&output));

    let text = fs::read_to_string(temp.path()).expect("generated config should exist");
    let raw = load_raw_config_from_str(&text).expect("generated config should parse");
    let validated = ValidatedConfig::new(raw).expect("generated config should validate");
    let live = validated
        .for_app_live()
        .expect("generated live config should validate");

    assert!(live.polymarket_relayer_auth().is_none());
    assert!(!text.contains("[polymarket.relayer_auth]"));
    assert!(text.contains("operator_target_revision = \"targets-rev-9\""));
    assert!(text.contains("approved_families = [\"family-a\"]"));
    assert!(text.contains("ready_families = [\"family-b\"]"));
}

#[test]
fn init_interactive_live_supports_builder_relayer_auth_without_transient_fields() {
    let temp = tempfile::NamedTempFile::new().expect("temp file");
    let mut child = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live init should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            b"live\nreplace\nsafe\n0x1111111111111111111111111111111111111111\n\npoly-api-key-1\npoly-secret-1\npoly-passphrase-1\nbuilder_api_key\nbuilder-relayer-key-1\nbuilder-relayer-secret-1\nbuilder-relayer-passphrase-1\n",
        )
        .expect("wizard answers should write");

    let output = child.wait_with_output().expect("output");
    assert!(output.status.success(), "{}", combined(&output));

    let text = fs::read_to_string(temp.path()).expect("generated config should exist");
    assert!(text.contains("kind = \"builder_api_key\""));
    assert!(text.contains("api_key = \"builder-relayer-key-1\""));
    assert!(text.contains("secret = \"builder-relayer-secret-1\""));
    assert!(text.contains("passphrase = \"builder-relayer-passphrase-1\""));
    assert!(!text.contains("timestamp ="));
    assert!(!text.contains("signature ="));
    assert_generated_live_config_is_schema_valid(&text);
}

#[test]
fn init_interactive_non_eoa_still_collects_and_renders_relayer_auth() {
    let temp = tempfile::NamedTempFile::new().expect("temp file");
    let mut child = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live init should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            b"live\nreplace\nsafe\n0x1111111111111111111111111111111111111111\n0x2222222222222222222222222222222222222222\npoly-api-key-1\npoly-secret-1\npoly-passphrase-1\nbuilder_api_key\nbuilder-relayer-key-1\nbuilder-relayer-secret-1\nbuilder-relayer-passphrase-1\n",
        )
        .expect("wizard answers should write");

    let output = child.wait_with_output().expect("output");
    assert!(output.status.success(), "{}", combined(&output));

    let text = fs::read_to_string(temp.path()).expect("generated config should exist");
    let raw = load_raw_config_from_str(&text).expect("generated config should parse");
    let validated = ValidatedConfig::new(raw).expect("generated config should validate");
    let live = validated
        .for_app_live()
        .expect("generated live config should validate");

    let account = live.account().expect("account should exist");
    assert_eq!(account.signature_type_label(), "Safe");
    assert_eq!(account.wallet_route_label(), "Safe");
    let relayer_auth = live
        .polymarket_relayer_auth()
        .expect("non-EOA path should still render relayer auth");
    assert!(relayer_auth.is_builder_api_key());
    assert!(text.contains("[polymarket.relayer_auth]"));
    assert!(text.contains("kind = \"builder_api_key\""));
}

#[test]
fn init_interactive_non_eoa_relayer_api_key_still_collects_and_renders_relayer_auth() {
    let temp = tempfile::NamedTempFile::new().expect("temp file");
    let mut child = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live init should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            b"live\nreplace\nproxy\n0x1111111111111111111111111111111111111111\n0x2222222222222222222222222222222222222222\npoly-api-key-1\npoly-secret-1\npoly-passphrase-1\nrelayer_api_key\nrelay-key-1\n0x3333333333333333333333333333333333333333\n",
        )
        .expect("wizard answers should write");

    let output = child.wait_with_output().expect("output");
    assert!(output.status.success(), "{}", combined(&output));

    let text = fs::read_to_string(temp.path()).expect("generated config should exist");
    let raw = load_raw_config_from_str(&text).expect("generated config should parse");
    let validated = ValidatedConfig::new(raw).expect("generated config should validate");
    let live = validated
        .for_app_live()
        .expect("generated live config should validate");

    let account = live.account().expect("account should exist");
    assert_eq!(account.signature_type_label(), "Proxy");
    assert_eq!(account.wallet_route_label(), "Proxy");
    let relayer_auth = live
        .polymarket_relayer_auth()
        .expect("non-EOA relayer_api_key path should still render relayer auth");
    assert_eq!(
        relayer_auth.kind(),
        config_schema::AppLivePolymarketRelayerAuthKind::RelayerApiKey
    );
    assert_eq!(relayer_auth.api_key(), "relay-key-1");
    assert_eq!(
        relayer_auth.address(),
        Some("0x3333333333333333333333333333333333333333")
    );
    assert!(text.contains("[polymarket.relayer_auth]"));
    assert!(text.contains("kind = \"relayer_api_key\""));
}

#[test]
fn example_config_omits_default_source_block_and_separates_eoa_from_non_eoa_relayer_examples() {
    let text = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config/axiom-arb.example.toml"),
    )
    .expect("example config should be readable");
    assert!(!text.contains("[polymarket.source]"));
    assert!(text.contains("built-in defaults"));
    assert!(text.contains("source_overrides"));
    assert!(!text.contains("\n[polymarket.relayer_auth]\n"));
    assert!(text.contains("EOA"));
    assert!(text.contains("# [polymarket.relayer_auth]"));
}

#[test]
fn readme_scopes_eoa_truth_to_smoke_and_l2_only_flows() {
    let text =
        fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../README.md"))
            .expect("README should be readable");
    assert!(text.contains("EOA"));
    assert!(text.contains("smoke"));
    assert!(text.contains("account-L2-only"));
    assert!(text.contains("non-shadow live"));
    assert!(text.contains("fail-closed"));
}

#[test]
fn smoke_runbook_describes_wallet_kind_aware_doctor_probe_set() {
    let text = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/runbooks/real-user-shadow-smoke.md"),
    )
    .expect("smoke runbook should be readable");
    assert!(text.contains("wallet kind"));
    assert!(text.contains("EOA smoke"));
    assert!(text.contains("relayer"));
    assert!(text.contains("omits relayer probe"));
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

fn assert_generated_live_config_is_schema_valid(text: &str) {
    let raw = load_raw_config_from_str(text).expect("generated config should parse as raw config");
    let validated = ValidatedConfig::new(raw).expect("generated config should validate");
    let live = validated
        .for_app_live()
        .expect("generated live config should validate for app-live");

    let target_source = live.target_source().expect("target source should exist");
    assert!(target_source.is_adopted());
    assert!(target_source.operator_target_revision().is_none());

    let rollout = live.negrisk_rollout().expect("rollout should exist");
    assert!(rollout.approved_families().is_empty());
    assert!(rollout.ready_families().is_empty());
}
