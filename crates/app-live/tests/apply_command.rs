mod support;

use std::{fs, path::PathBuf, process::Command};

use support::cli;
use support::status_db::TestDatabase;

#[test]
fn apply_subcommand_is_exposed() {
    let output = Command::new(cli::app_live_binary())
        .arg("apply")
        .arg("--help")
        .output()
        .expect("app-live apply --help should execute");

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("--config"), "{text}");
    assert!(text.contains("--start"), "{text}");
}

#[test]
fn apply_rejects_non_smoke_config_with_specific_guidance() {
    let paper_output = Command::new(cli::app_live_binary())
        .arg("apply")
        .arg("--config")
        .arg(cli::config_fixture("app-live-paper.toml"))
        .output()
        .expect("app-live apply should execute for paper config");
    let paper_text = cli::combined(&paper_output);
    assert!(!paper_output.status.success(), "{paper_text}");
    assert!(paper_text.contains("bootstrap"), "{paper_text}");
    assert!(paper_text.contains("run"), "{paper_text}");

    let live_output = Command::new(cli::app_live_binary())
        .arg("apply")
        .arg("--config")
        .arg(cli::config_fixture("app-live-live.toml"))
        .output()
        .expect("app-live apply should execute for live config");
    let live_text = cli::combined(&live_output);
    assert!(!live_output.status.success(), "{live_text}");
    assert!(live_text.contains("status -> doctor -> run"), "{live_text}");
}

#[test]
fn apply_maps_invalid_config_to_parse_error() {
    let config_path = temp_invalid_config_path();

    let output = Command::new(cli::app_live_binary())
        .arg("apply")
        .arg("--config")
        .arg(&config_path)
        .output()
        .expect("app-live apply should execute for blocked config");

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("toml parse error"), "{text}");

    let _ = fs::remove_file(config_path);
}

#[test]
fn apply_maps_smoke_blocked_readiness_to_concrete_reason() {
    let config = cli::config_fixture("app-live-ux-smoke.toml");

    let output = Command::new(cli::app_live_binary())
        .arg("apply")
        .arg("--config")
        .arg(&config)
        .env_remove("DATABASE_URL")
        .output()
        .expect("app-live apply should execute for smoke blocked readiness");

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("DATABASE_URL is not set"), "{text}");
    assert!(!text.contains("readiness error"), "{text}");
}

#[test]
fn apply_maps_target_adoption_required_to_ensure_target_anchor() {
    let database = TestDatabase::new();
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        config.replace("operator_target_revision = \"targets-rev-9\"\n", "")
    });

    let output = Command::new(cli::app_live_binary())
        .arg("apply")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live apply should execute for target adoption required");

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("ensure-target-anchor"), "{text}");

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn apply_maps_smoke_rollout_required_to_ensure_smoke_rollout() {
    let database = TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", None);
    let config = cli::config_fixture("app-live-ux-smoke.toml");

    let output = Command::new(cli::app_live_binary())
        .arg("apply")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live apply should execute for smoke rollout required");

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("ensure-smoke-rollout"), "{text}");

    database.cleanup();
}

#[test]
fn apply_maps_smoke_config_ready_to_run_preflight() {
    let database = TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", None);
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        format!(
            "{config}\n[negrisk.rollout]\napproved_families = [\"family-a\"]\nready_families = [\"family-a\"]\n"
        )
    });

    let output = Command::new(cli::app_live_binary())
        .arg("apply")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live apply should execute for smoke config ready");

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("run-preflight"), "{text}");

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn apply_maps_restart_required_to_restart_boundary_gate() {
    let database = TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-10"));
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        format!(
            "{config}\n[negrisk.rollout]\napproved_families = [\"family-a\"]\nready_families = [\"family-a\"]\n"
        )
    });

    let output = Command::new(cli::app_live_binary())
        .arg("apply")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live apply should execute for restart required");

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("confirm-manual-restart-boundary"), "{text}");

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

fn temp_config_fixture_path(relative: &str, edit: impl FnOnce(String) -> String) -> PathBuf {
    let source = cli::config_fixture(relative);
    let text = fs::read_to_string(&source).expect("fixture should be readable");
    let edited = edit(text);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "app-live-apply-{}-{}.toml",
        std::process::id(),
        NEXT_TEMP_CONFIG_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::write(&path, edited).expect("temp fixture should be writable");
    path
}

fn temp_invalid_config_path() -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "app-live-apply-{}-{}.toml",
        std::process::id(),
        NEXT_TEMP_CONFIG_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::write(&path, "runtime = [").expect("temp fixture should be writable");
    path
}

static NEXT_TEMP_CONFIG_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
