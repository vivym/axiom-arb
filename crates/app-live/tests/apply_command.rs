mod support;

use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

use support::cli;
use support::{apply_db, status_db::TestDatabase};

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
fn apply_enters_inline_target_adoption_when_target_anchor_missing() {
    let database = apply_db::TestDatabase::new();
    database.seed_adoptable_revision("adoptable-9", "candidate-9", "targets-rev-9");
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        config.replace("operator_target_revision = \"targets-rev-9\"\n", "")
    });

    let output = run_apply_with_stdin(&config_path, database.database_url(), "cancel\n", true);

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("Choose an adoptable revision"), "{text}");
    assert!(text.contains("adoptable-9"), "{text}");
    assert!(text.contains("cancel"), "{text}");

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn apply_cancelled_inline_target_adoption_stops_without_writes() {
    let database = apply_db::TestDatabase::new();
    database.seed_adoptable_revision("adoptable-9", "candidate-9", "targets-rev-9");
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        config.replace("operator_target_revision = \"targets-rev-9\"\n", "")
    });

    let output = run_apply_with_stdin(&config_path, database.database_url(), "cancel\n", true);

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("cancelled"), "{text}");

    let rewritten = fs::read_to_string(&config_path).expect("config should still load");
    assert!(
        !rewritten.contains("operator_target_revision = \"targets-rev-9\""),
        "{rewritten}"
    );
    assert_eq!(database.history_count(), 0);

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn apply_can_inline_smoke_target_adoption() {
    let database = apply_db::TestDatabase::new();
    database.seed_adoptable_revision("adoptable-9", "candidate-9", "targets-rev-9");
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        config.replace("operator_target_revision = \"targets-rev-9\"\n", "")
    });

    let output = run_apply_with_stdin(
        &config_path,
        database.database_url(),
        "adoptable-9\ndecline\n",
        true,
    );

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(
        text.contains("adopted adoptable revision adoptable-9"),
        "{text}"
    );
    assert!(
        text.contains("Smoke rollout readiness is not enabled"),
        "{text}"
    );
    assert!(
        text.contains("inline smoke rollout enablement declined"),
        "{text}"
    );

    let rewritten = fs::read_to_string(&config_path).expect("rewritten config should load");
    assert!(
        rewritten.contains("operator_target_revision = \"targets-rev-9\""),
        "{rewritten}"
    );

    let latest = database.latest_history().expect("history row should exist");
    assert_eq!(latest.action_kind, "adopt");
    assert_eq!(latest.operator_target_revision, "targets-rev-9");
    assert_eq!(latest.adoptable_revision.as_deref(), Some("adoptable-9"));
    assert_eq!(latest.candidate_revision.as_deref(), Some("candidate-9"));

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn apply_non_interactive_target_adoption_required_stays_fail_closed_at_stage() {
    let database = apply_db::TestDatabase::new();
    database.seed_adoptable_revision("adoptable-9", "candidate-9", "targets-rev-9");
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        config.replace("operator_target_revision = \"targets-rev-9\"\n", "")
    });

    let output = Command::new(cli::app_live_binary())
        .arg("apply")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .stdin(Stdio::null())
        .output()
        .expect("app-live apply should execute for non-interactive target adoption required");

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("ensure-target-anchor"), "{text}");
    assert!(!text.contains("Choose an adoptable revision"), "{text}");

    let rewritten = fs::read_to_string(&config_path).expect("config should still load");
    assert!(
        !rewritten.contains("operator_target_revision = \"targets-rev-9\""),
        "{rewritten}"
    );
    assert_eq!(database.history_count(), 0);

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
fn apply_rollout_missing_smoke_config_enters_explicit_confirmation() {
    let database = apply_db::TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", None);
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| config);

    let output = run_apply_with_stdin(&config_path, database.database_url(), "decline\n", true);

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(
        text.contains("Smoke rollout readiness is not enabled"),
        "{text}"
    );
    assert!(text.contains("Adopted families:"), "{text}");
    assert!(text.contains("family-a"), "{text}");
    assert!(text.contains("confirm"), "{text}");
    assert!(text.contains("decline"), "{text}");

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn apply_can_inline_smoke_rollout_enablement() {
    let database = apply_db::TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", None);
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| config);

    let output = run_apply_with_stdin(&config_path, database.database_url(), "confirm\n", true);

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("run-preflight"), "{text}");

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
    let _ = fs::remove_file(config_path);
}

#[test]
fn apply_declining_inline_smoke_rollout_keeps_rollout_unchanged_and_stops_cleanly() {
    let database = apply_db::TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", None);
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| config);

    let output = run_apply_with_stdin(&config_path, database.database_url(), "decline\n", true);

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(
        text.contains("inline smoke rollout enablement declined"),
        "{text}"
    );

    let rewritten = fs::read_to_string(&config_path).expect("config should still load");
    assert!(!rewritten.contains("approved_families"), "{rewritten}");
    assert!(!rewritten.contains("ready_families"), "{rewritten}");

    database.cleanup();
    let _ = fs::remove_file(config_path);
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

fn run_apply_with_stdin(
    config_path: &std::path::Path,
    database_url: &str,
    stdin_input: &str,
    force_interactive: bool,
) -> std::process::Output {
    let mut command = Command::new(cli::app_live_binary());
    command
        .arg("apply")
        .arg("--config")
        .arg(config_path)
        .env("DATABASE_URL", database_url)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if force_interactive {
        command.env("APP_LIVE_APPLY_FORCE_INTERACTIVE", "1");
    }

    let mut child = command.spawn().expect("app-live apply should spawn");

    child
        .stdin
        .as_mut()
        .expect("stdin should be piped")
        .write_all(stdin_input.as_bytes())
        .expect("stdin should write");

    child
        .wait_with_output()
        .expect("app-live apply should finish")
}
