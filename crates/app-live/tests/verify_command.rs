mod support;

use std::{fs, process::Command};

use domain::ExecutionMode;
use support::{cli, verify_db};

#[test]
fn verify_subcommand_is_exposed() {
    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--help")
        .output()
        .expect("app-live verify --help should execute");

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("--config"), "{text}");
    assert!(text.contains("--expect"), "{text}");
    assert!(text.contains("--from-seq"), "{text}");
    assert!(text.contains("--to-seq"), "{text}");
    assert!(text.contains("--attempt-id"), "{text}");
    assert!(text.contains("--since"), "{text}");
}

#[test]
fn verify_placeholder_fails_for_missing_config() {
    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg("/definitely/missing.toml")
        .output()
        .expect("app-live verify --config /definitely/missing.toml should execute");

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("Verdict: FAIL"), "{text}");
}

#[test]
fn verify_explicit_target_config_is_reported_as_legacy_unsupported() {
    let verify_db = verify_db::TestDatabase::new();

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(cli::config_fixture("app-live-live.toml"))
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Verdict: FAIL"), "{text}");
    assert!(text.contains("legacy explicit targets"), "{text}");

    verify_db.cleanup();
}

#[test]
fn verify_historical_attempt_window_degrades_when_it_cannot_be_tied_to_current_config() {
    let verify_db = verify_db::TestDatabase::new();
    verify_db.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-9"));
    verify_db.seed_runtime_progress(41, 7, Some("snapshot-verify-7"), Some("targets-rev-9"));
    verify_db.seed_attempt(verify_db::sample_attempt(
        "attempt-old",
        ExecutionMode::Live,
    ));
    let config_path = verify_db::temp_config_path(
        "app-live-verify",
        &verify_db::live_ready_config_for("targets-rev-9"),
    );

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(&config_path)
        .arg("--attempt-id")
        .arg("attempt-old")
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(
        text.contains("Verdict: PASS WITH WARNINGS") || text.contains("Verdict: FAIL"),
        "{text}"
    );
    assert!(
        text.contains("historical window is not provably tied to the current config anchor"),
        "{text}"
    );

    verify_db.cleanup();
    let _ = fs::remove_file(config_path);
}
