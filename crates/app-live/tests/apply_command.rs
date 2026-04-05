mod support;

use std::{
    fs,
    io::{Read, Write},
    net::TcpListener as StdTcpListener,
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

use chrono::{Duration as ChronoDuration, Utc};
use persistence::models::{RunSessionRow, RunSessionState};
use support::cli;
use support::{apply_db, status_db::TestDatabase};
use tokio_tungstenite::tungstenite::{accept as accept_websocket, Message as WsMessage};
use toml_edit::{value, DocumentMut};

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
    assert!(
        live_text.contains("migrate to adopted target source or use lower-level commands"),
        "{live_text}"
    );
}

#[test]
fn apply_live_config_no_longer_returns_generic_unsupported_error() {
    let output = Command::new(cli::app_live_binary())
        .arg("apply")
        .arg("--config")
        .arg(cli::config_fixture("app-live-ux-live.toml"))
        .env_remove("DATABASE_URL")
        .output()
        .expect("app-live apply should execute for adopted live config");

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("DATABASE_URL"), "{text}");
    assert!(!text.contains("status -> doctor -> run"), "{text}");
}

#[test]
fn apply_live_discovery_required_stops_with_discover_guidance() {
    let database = TestDatabase::new();
    let config_path = temp_config_fixture_path("app-live-ux-live.toml", |config| {
        config.replace("operator_target_revision = \"targets-rev-9\"\n", "")
    });

    let output = Command::new(cli::app_live_binary())
        .arg("apply")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live apply should execute for live discovery required");

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("Execution"), "{text}");
    assert!(text.contains("Stopping before doctor preflight."), "{text}");
    assert!(text.contains("discovery-required"), "{text}");
    assert!(text.contains("app-live discover --config"), "{text}");
    assert!(!text.contains("status -> doctor -> run"), "{text}");

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn apply_live_rollout_required_stops_before_doctor_or_run() {
    let database = TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", None);
    let config = cli::config_fixture("app-live-ux-live.toml");

    let output = Command::new(cli::app_live_binary())
        .arg("apply")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live apply should execute for live rollout required");

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("live-rollout-required"), "{text}");
    assert!(
        text.contains(
            "[negrisk.rollout].approved_families and ready_families for adopted families"
        ),
        "{text}"
    );
    assert!(!text.contains("Running doctor preflight"), "{text}");
    assert!(!text.contains("app-live bootstrap complete"), "{text}");

    database.cleanup();
}

#[test]
fn apply_live_restart_required_with_rollout_missing_stops_before_doctor_or_run() {
    let database = TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-8"));
    let config = cli::config_fixture("app-live-ux-live.toml");

    let output = app_live_command()
        .arg("apply")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live apply should execute for live restart required with rollout missing");

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("Rollout state: required"), "{text}");
    assert!(
        text.contains(
            "[negrisk.rollout].approved_families and ready_families for adopted families"
        ),
        "{text}"
    );
    assert!(!text.contains("Running doctor preflight"), "{text}");
    assert!(!text.contains("Choose one:"), "{text}");
    assert!(!text.contains("app-live bootstrap complete"), "{text}");

    database.cleanup();
}

#[test]
fn apply_live_generic_blocked_stops_with_existing_blocking_guidance() {
    let database = TestDatabase::new();
    database.seed_adopted_target_without_provenance("targets-rev-9");
    let config = cli::config_fixture("app-live-ux-live.toml");

    let output = Command::new(cli::app_live_binary())
        .arg("apply")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live apply should execute for live blocked readiness");

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("Readiness: blocked"), "{text}");
    assert!(
        text.contains("fix the blocking issue, then rerun app-live status --config"),
        "{text}"
    );
    assert!(!text.contains("status -> doctor -> run"), "{text}");

    database.cleanup();
}

#[test]
fn apply_live_legacy_explicit_targets_keep_migration_specific_guidance() {
    let output = Command::new(cli::app_live_binary())
        .arg("apply")
        .arg("--config")
        .arg(cli::config_fixture("app-live-live.toml"))
        .output()
        .expect("app-live apply should execute for legacy explicit live config");

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(
        text.contains("legacy explicit targets are not supported in the high-level status flow"),
        "{text}"
    );
    assert!(
        text.contains("migrate to adopted target source or use lower-level commands"),
        "{text}"
    );
    assert!(
        !text.contains("fix the blocking issue, then rerun app-live status --config"),
        "{text}"
    );
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
fn apply_maps_discovery_required_to_truthful_next_action() {
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
    assert!(text.contains("app-live bootstrap --config"), "{text}");
    assert!(text.contains("app-live discover --config"), "{text}");
    assert!(!text.contains("ensure-target-anchor"), "{text}");

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
    assert!(text.contains("choose-adoptable-revision"), "{text}");
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
fn apply_does_not_inline_discovery_when_only_advisory_candidates_exist() {
    let database = apply_db::TestDatabase::new();
    database.seed_advisory_candidate(
        "candidate-8",
        "candidate generation deferred until discovery backfill completes",
    );
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        config.replace("operator_target_revision = \"targets-rev-9\"\n", "")
    });

    let output = run_apply_with_stdin(&config_path, database.database_url(), "cancel\n", true);

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(
        text.contains("candidate generation deferred until discovery backfill completes"),
        "{text}"
    );
    assert!(
        text.contains("app-live targets candidates --config"),
        "{text}"
    );
    assert!(!text.contains("Choose an adoptable revision"), "{text}");

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
    let venue = MockDoctorVenue::success();
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        with_mock_doctor_venue(config, &venue)
    });

    let output = run_apply_with_stdin(&config_path, database.database_url(), "confirm\n", true);

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("Current State"), "{text}");
    assert!(text.contains("Execution"), "{text}");
    assert!(text.contains("Overall: PASS"), "{text}");
    assert!(text.contains("Runtime not started"), "{text}");

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
fn apply_smoke_config_ready_without_start_stops_at_ready_summary() {
    let database = TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", None);
    let venue = MockDoctorVenue::success();
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        format!(
            "{}\n[negrisk.rollout]\napproved_families = [\"family-a\"]\nready_families = [\"family-a\"]\n",
            with_mock_doctor_venue(config, &venue)
        )
    });

    let output = app_live_command()
        .arg("apply")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live apply should execute for smoke config ready without start");

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("Current State"), "{text}");
    assert!(text.contains("Planned Actions"), "{text}");
    assert!(text.contains("Execution"), "{text}");
    assert!(text.contains("Outcome"), "{text}");
    assert!(text.contains("Next Actions"), "{text}");
    assert!(text.contains("Readiness: smoke-config-ready"), "{text}");
    assert!(text.contains("Overall: PASS"), "{text}");
    assert!(text.contains("Runtime not started"), "{text}");
    assert!(text.contains("app-live apply --config"), "{text}");
    let planned = section_text(&text, "Planned Actions");
    assert!(planned.contains("Run doctor preflight checks."), "{text}");
    assert!(
        planned.contains("Stop at ready without starting the runtime."),
        "{text}"
    );
    assert!(!planned.contains("to continue in the foreground"), "{text}");
    assert!(!text.contains("run-preflight"), "{text}");

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn apply_live_config_ready_without_start_stops_at_ready_to_start_outcome() {
    let database = TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", None);
    let venue = MockDoctorVenue::success();
    let config_path = temp_config_fixture_path("app-live-ux-live.toml", |config| {
        format!(
            "{}\n[negrisk.rollout]\napproved_families = [\"family-a\"]\nready_families = [\"family-a\"]\n",
            with_mock_doctor_venue(config, &venue)
        )
    });

    let output = app_live_command()
        .arg("apply")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live apply should execute for live config ready without start");

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("Planned Actions"), "{text}");
    let planned = section_text(&text, "Planned Actions");
    assert!(planned.contains("Run doctor preflight checks."), "{text}");
    assert!(
        planned.contains("Stop at Ready to start without starting the runtime."),
        "{text}"
    );
    assert!(
        !planned.contains("Start the runtime in the foreground."),
        "{text}"
    );
    assert!(text.contains("Outcome"), "{text}");
    assert!(text.contains("Ready to start"), "{text}");
    assert!(!text.contains("apply reached ready state"), "{text}");
    assert!(text.contains("Overall: PASS"), "{text}");
    assert!(text.contains("app-live apply --config"), "{text}");
    assert!(
        !section_text(&text, "Next Actions").contains("app-live run --config"),
        "{text}"
    );

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn apply_live_config_ready_with_start_enters_run_successfully() {
    let database = TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", None);
    let venue = MockDoctorVenue::success();
    let config_path = temp_config_fixture_path("app-live-ux-live.toml", |config| {
        format!(
            "{}\n[negrisk.rollout]\napproved_families = [\"family-a\"]\nready_families = [\"family-a\"]\n",
            with_mock_doctor_venue(config, &venue)
        )
    });

    let output = app_live_command()
        .arg("apply")
        .arg("--config")
        .arg(&config_path)
        .arg("--start")
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live apply should execute for live config ready with start");

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("Planned Actions"), "{text}");
    let planned = section_text(&text, "Planned Actions");
    assert!(planned.contains("Run doctor preflight checks."), "{text}");
    assert!(
        planned.contains("Start the runtime in the foreground."),
        "{text}"
    );
    assert!(text.contains("Execution"), "{text}");
    assert!(
        section_text(&text, "Outcome").contains("Outcome\nStarted"),
        "{text}"
    );
    assert!(
        text.contains("Starting runtime in the foreground."),
        "{text}"
    );
    assert!(text.contains("app-live bootstrap complete"), "{text}");

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn apply_live_config_ready_with_start_blocks_when_matching_active_run_session_is_running() {
    let database = TestDatabase::new();
    let venue = MockDoctorVenue::success();
    let config_path = temp_config_fixture_path("app-live-ux-live.toml", |config| {
        format!(
            "{}\n[negrisk.rollout]\napproved_families = [\"family-a\"]\nready_families = [\"family-a\"]\n",
            with_mock_doctor_venue(config, &venue)
        )
    });
    database.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-9"));
    database.seed_run_session(live_run_session(
        "rs-active",
        &config_path,
        "targets-rev-9",
        RunSessionState::Running,
        Utc::now() - ChronoDuration::minutes(2),
    ));
    database.seed_runtime_progress(Some("targets-rev-9"), Some("rs-active"));

    let output = run_apply_with_options(
        &config_path,
        database.database_url(),
        "confirm\n",
        true,
        true,
    );

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("Current State"), "{text}");
    assert!(text.contains("Relevant run session: rs-active"), "{text}");
    assert!(
        !text.contains("Conflicting active run session: rs-active"),
        "{text}"
    );
    assert!(text.contains("Blocked"), "{text}");
    assert!(
        text.contains("resolve the existing runtime outside apply"),
        "{text}"
    );
    assert!(
        !text.contains("Starting runtime in the foreground."),
        "{text}"
    );
    assert!(!text.contains("app-live bootstrap complete"), "{text}");

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn apply_restart_required_without_start_stops_at_ready_with_restart_messaging() {
    let database = TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-10"));
    let venue = MockDoctorVenue::success();
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        format!(
            "{}\n[negrisk.rollout]\napproved_families = [\"family-a\"]\nready_families = [\"family-a\"]\n",
            with_mock_doctor_venue(config, &venue)
        )
    });

    let output = app_live_command()
        .arg("apply")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live apply should execute for restart required without start");

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("Current State"), "{text}");
    assert!(text.contains("Planned Actions"), "{text}");
    let planned = section_text(&text, "Planned Actions");
    assert!(planned.contains("Run doctor preflight checks."), "{text}");
    assert!(
        planned.contains("Stop at the manual restart boundary without starting the runtime."),
        "{text}"
    );
    assert!(
        !planned.contains("Start the runtime in the foreground only if confirmed."),
        "{text}"
    );
    assert!(text.contains("Outcome"), "{text}");
    assert!(text.contains("Next Actions"), "{text}");
    assert!(text.contains("Readiness: restart-required"), "{text}");
    assert!(text.contains("manual restart boundary"), "{text}");
    assert!(text.contains("Runtime not started"), "{text}");
    assert!(text.contains("app-live apply --config"), "{text}");
    assert!(!text.contains("Choose one:"), "{text}");

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn apply_live_restart_required_without_start_stops_at_ready_to_start() {
    let database = TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-8"));
    let venue = MockDoctorVenue::success();
    let config_path = temp_config_fixture_path("app-live-ux-live.toml", |config| {
        format!(
            "{}\n[negrisk.rollout]\napproved_families = [\"family-a\"]\nready_families = [\"family-a\"]\n",
            with_mock_doctor_venue(config, &venue)
        )
    });

    let output = app_live_command()
        .arg("apply")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live apply should execute for live restart required without start");

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("Execution"), "{text}");
    assert!(text.contains("Ready to start"), "{text}");
    assert!(text.contains("manual restart boundary"), "{text}");
    assert!(
        !section_text(&text, "Next Actions").contains("app-live run --config"),
        "{text}"
    );
    assert!(text.contains("app-live apply --config"), "{text}");

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn apply_restart_required_with_start_requires_explicit_confirmation() {
    let database = apply_db::TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-10"));
    let venue = MockDoctorVenue::success();
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        format!(
            "{}\n[negrisk.rollout]\napproved_families = [\"family-a\"]\nready_families = [\"family-a\"]\n",
            with_mock_doctor_venue(config, &venue)
        )
    });

    let output = run_apply_with_options(
        &config_path,
        database.database_url(),
        "decline\n",
        true,
        true,
    );

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("Planned Actions"), "{text}");
    let planned = section_text(&text, "Planned Actions");
    assert!(planned.contains("Run doctor preflight checks."), "{text}");
    assert!(
        planned.contains("Require explicit confirmation at the manual restart boundary."),
        "{text}"
    );
    assert!(
        planned.contains("Start the runtime in the foreground only if confirmed."),
        "{text}"
    );
    assert!(text.contains("Choose one:"), "{text}");
    assert!(text.contains("confirm"), "{text}");
    assert!(text.contains("decline"), "{text}");
    assert!(text.contains("manual restart boundary"), "{text}");
    assert!(text.contains("foreground"), "{text}");
    assert!(
        text.contains("will not stop or replace an existing daemon"),
        "{text}"
    );

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn apply_restart_required_with_start_fails_closed_when_not_interactive() {
    let database = apply_db::TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-10"));
    let venue = MockDoctorVenue::success();
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        format!(
            "{}\n[negrisk.rollout]\napproved_families = [\"family-a\"]\nready_families = [\"family-a\"]\n",
            with_mock_doctor_venue(config, &venue)
        )
    });

    let output = app_live_command()
        .arg("apply")
        .arg("--config")
        .arg(&config_path)
        .arg("--start")
        .env("DATABASE_URL", database.database_url())
        .stdin(Stdio::null())
        .output()
        .expect("app-live apply should execute for non-interactive restart boundary");

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("confirm-manual-restart-boundary"), "{text}");
    assert!(!text.contains("Choose one:"), "{text}");
    assert!(
        text.contains("manual restart boundary requires interactive confirmation"),
        "{text}"
    );

    let progress = database
        .runtime_progress()
        .expect("runtime progress should remain seeded");
    assert_eq!(
        progress.operator_target_revision.as_deref(),
        Some("targets-rev-10")
    );

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn apply_declining_restart_confirmation_stops_without_invoking_run() {
    let database = apply_db::TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-10"));
    let venue = MockDoctorVenue::success();
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        format!(
            "{}\n[negrisk.rollout]\napproved_families = [\"family-a\"]\nready_families = [\"family-a\"]\n",
            with_mock_doctor_venue(config, &venue)
        )
    });

    let output = run_apply_with_options(
        &config_path,
        database.database_url(),
        "decline\n",
        true,
        true,
    );

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("Runtime not started"), "{text}");
    assert!(text.contains("restart confirmation declined"), "{text}");

    let progress = database
        .runtime_progress()
        .expect("runtime progress should remain seeded");
    assert_eq!(
        progress.operator_target_revision.as_deref(),
        Some("targets-rev-10")
    );

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn apply_accepting_restart_confirmation_enters_run() {
    let database = apply_db::TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-10"));
    let venue = MockDoctorVenue::success();
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        format!(
            "{}\n[negrisk.rollout]\napproved_families = [\"family-a\"]\nready_families = [\"family-a\"]\n",
            with_mock_doctor_venue(config, &venue)
        )
    });

    let output = run_apply_with_options(
        &config_path,
        database.database_url(),
        "confirm\n",
        true,
        true,
    );

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("Execution"), "{text}");
    assert!(
        text.contains("Starting runtime in the foreground"),
        "{text}"
    );
    assert!(text.contains("app-live bootstrap complete"), "{text}");

    let progress = database
        .runtime_progress()
        .expect("run should persist updated runtime progress");
    assert_eq!(
        progress.operator_target_revision.as_deref(),
        Some("targets-rev-9")
    );

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn apply_restart_required_with_rollout_missing_enters_inline_smoke_rollout_enablement_first() {
    let database = apply_db::TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-10"));
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| config);

    let output = run_apply_with_stdin(&config_path, database.database_url(), "decline\n", true);

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(
        text.contains("Smoke rollout readiness is not enabled"),
        "{text}"
    );
    assert!(
        text.contains("inline smoke rollout enablement declined"),
        "{text}"
    );
    assert!(!text.contains("confirm-manual-restart-boundary"), "{text}");

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn apply_live_restart_required_with_start_fails_closed_when_not_interactive() {
    let database = TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-8"));
    let venue = MockDoctorVenue::success();
    let config_path = temp_config_fixture_path("app-live-ux-live.toml", |config| {
        format!(
            "{}\n[negrisk.rollout]\napproved_families = [\"family-a\"]\nready_families = [\"family-a\"]\n",
            with_mock_doctor_venue(config, &venue)
        )
    });

    let output = app_live_command()
        .arg("apply")
        .arg("--config")
        .arg(&config_path)
        .arg("--start")
        .env("DATABASE_URL", database.database_url())
        .stdin(Stdio::null())
        .output()
        .expect("app-live apply should execute for live non-interactive restart boundary");

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("Overall: PASS"), "{text}");
    assert!(
        section_text(&text, "Outcome").contains("Outcome\nBlocked"),
        "{text}"
    );
    assert!(text.contains("confirm-manual-restart-boundary"), "{text}");
    assert!(
        text.contains("manual restart boundary requires interactive confirmation"),
        "{text}"
    );
    assert!(!text.contains("Choose one:"), "{text}");
    assert!(!text.contains("app-live bootstrap complete"), "{text}");

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn apply_live_declining_restart_confirmation_stops_cleanly() {
    let database = TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-8"));
    let venue = MockDoctorVenue::success();
    let config_path = temp_config_fixture_path("app-live-ux-live.toml", |config| {
        format!(
            "{}\n[negrisk.rollout]\napproved_families = [\"family-a\"]\nready_families = [\"family-a\"]\n",
            with_mock_doctor_venue(config, &venue)
        )
    });

    let output = run_apply_with_options(
        &config_path,
        database.database_url(),
        "decline\n",
        true,
        true,
    );

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("Overall: PASS"), "{text}");
    assert!(text.contains("Ready to start"), "{text}");
    assert!(text.contains("restart confirmation declined"), "{text}");
    assert!(!text.contains("apply reached ready state"), "{text}");
    assert!(
        !section_text(&text, "Next Actions").contains("app-live run --config"),
        "{text}"
    );
    assert!(text.contains("app-live apply --config"), "{text}");
    assert!(!text.contains("app-live bootstrap complete"), "{text}");

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn apply_live_restart_required_with_start_and_confirm_enters_run_successfully() {
    let database = TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-8"));
    let venue = MockDoctorVenue::success();
    let config_path = temp_config_fixture_path("app-live-ux-live.toml", |config| {
        format!(
            "{}\n[negrisk.rollout]\napproved_families = [\"family-a\"]\nready_families = [\"family-a\"]\n",
            with_mock_doctor_venue(config, &venue)
        )
    });

    let output = run_apply_with_options(
        &config_path,
        database.database_url(),
        "confirm\n",
        true,
        true,
    );

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("Choose one:"), "{text}");
    assert!(
        section_text(&text, "Outcome").contains("Outcome\nStarted"),
        "{text}"
    );
    assert!(
        text.contains("Manual restart boundary confirmed. Starting runtime in the foreground."),
        "{text}"
    );
    assert!(text.contains("app-live bootstrap complete"), "{text}");

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn apply_live_conflicting_active_running_session_with_start_stops_at_boundary() {
    let database = TestDatabase::new();
    let venue = MockDoctorVenue::success();
    let config_path = temp_config_fixture_path("app-live-ux-live.toml", |config| {
        format!(
            "{}\n[negrisk.rollout]\napproved_families = [\"family-a\"]\nready_families = [\"family-a\"]\n",
            with_mock_doctor_venue(config, &venue)
        )
    });
    database.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-8"));
    database.seed_run_session(live_run_session(
        "rs-old",
        &config_path,
        "targets-rev-8",
        RunSessionState::Running,
        Utc::now() - ChronoDuration::minutes(2),
    ));
    database.seed_runtime_progress(Some("targets-rev-8"), Some("rs-old"));

    let output = run_apply_with_options(
        &config_path,
        database.database_url(),
        "confirm\n",
        true,
        true,
    );

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("Overall: PASS"), "{text}");
    assert!(
        section_text(&text, "Outcome").contains("Outcome\nBlocked"),
        "{text}"
    );
    assert!(text.contains("Planned Actions"), "{text}");
    assert!(
        text.contains(
            "Do not start the runtime while a conflicting active run session is still running."
        ),
        "{text}"
    );
    assert!(
        !text.contains("Start the runtime in the foreground only if confirmed."),
        "{text}"
    );
    assert!(
        text.contains("Conflicting active run session: rs-old"),
        "{text}"
    );
    assert!(
        text.contains("resolve the existing runtime outside apply"),
        "{text}"
    );
    assert!(!text.contains("Choose one:"), "{text}");
    assert!(!text.contains("app-live bootstrap complete"), "{text}");

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn apply_live_stale_active_run_session_id_does_not_block_restart_path() {
    let database = TestDatabase::new();
    let venue = MockDoctorVenue::success();
    let config_path = temp_config_fixture_path("app-live-ux-live.toml", |config| {
        format!(
            "{}\n[negrisk.rollout]\napproved_families = [\"family-a\"]\nready_families = [\"family-a\"]\n",
            with_mock_doctor_venue(config, &venue)
        )
    });
    database.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-8"));
    database.seed_run_session(live_run_session(
        "rs-stale",
        &config_path,
        "targets-rev-8",
        RunSessionState::Running,
        Utc::now() - ChronoDuration::minutes(20),
    ));
    database.seed_runtime_progress(Some("targets-rev-8"), Some("rs-stale"));

    let output = run_apply_with_options(
        &config_path,
        database.database_url(),
        "confirm\n",
        true,
        true,
    );

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("Conflicting active state: stale"), "{text}");
    assert!(text.contains("Choose one:"), "{text}");
    assert!(text.contains("app-live bootstrap complete"), "{text}");

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn apply_live_exited_active_run_session_id_does_not_block_restart_path() {
    let database = TestDatabase::new();
    let venue = MockDoctorVenue::success();
    let config_path = temp_config_fixture_path("app-live-ux-live.toml", |config| {
        format!(
            "{}\n[negrisk.rollout]\napproved_families = [\"family-a\"]\nready_families = [\"family-a\"]\n",
            with_mock_doctor_venue(config, &venue)
        )
    });
    database.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-8"));
    database.seed_run_session(live_run_session(
        "rs-exited",
        &config_path,
        "targets-rev-8",
        RunSessionState::Exited,
        Utc::now() - ChronoDuration::minutes(2),
    ));
    database.seed_runtime_progress(Some("targets-rev-8"), Some("rs-exited"));

    let output = run_apply_with_options(
        &config_path,
        database.database_url(),
        "confirm\n",
        true,
        true,
    );

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("Conflicting active state: exited"), "{text}");
    assert!(text.contains("Choose one:"), "{text}");
    assert!(text.contains("app-live bootstrap complete"), "{text}");

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

fn section_text<'a>(text: &'a str, title: &str) -> &'a str {
    let start = text.find(title).unwrap_or_else(|| panic!("{text}"));
    let section_titles = [
        "Current State",
        "Planned Actions",
        "Execution",
        "Outcome",
        "Next Actions",
    ];
    let next_start = section_titles
        .iter()
        .filter(|candidate| **candidate != title)
        .filter_map(|candidate| {
            text[start + title.len()..]
                .find(candidate)
                .map(|idx| start + title.len() + idx)
        })
        .min()
        .unwrap_or(text.len());
    &text[start..next_start]
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
    run_apply_with_options(
        config_path,
        database_url,
        stdin_input,
        force_interactive,
        false,
    )
}

fn run_apply_with_options(
    config_path: &std::path::Path,
    database_url: &str,
    stdin_input: &str,
    force_interactive: bool,
    start: bool,
) -> std::process::Output {
    let mut command = app_live_command();
    command
        .arg("apply")
        .arg("--config")
        .arg(config_path)
        .env("DATABASE_URL", database_url)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if start {
        command.arg("--start");
    }
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

fn with_mock_doctor_venue(config: String, venue: &MockDoctorVenue) -> String {
    let mut document = config
        .parse::<DocumentMut>()
        .expect("config fixture should parse as TOML");

    let polymarket = document["polymarket"]
        .as_table_like_mut()
        .expect("config fixture should contain [polymarket]");
    if polymarket.get("source_overrides").is_none() {
        polymarket.insert("source_overrides", toml_edit::table());
    }
    let source = polymarket
        .get_mut("source_overrides")
        .expect("config fixture should contain [polymarket.source_overrides]")
        .as_table_like_mut()
        .expect("config fixture should contain [polymarket.source_overrides]");

    for (key, rewritten) in [
        ("clob_host", venue.http_base_url()),
        ("data_api_host", venue.http_base_url()),
        ("relayer_host", venue.http_base_url()),
        ("market_ws_url", venue.market_ws_url()),
        ("user_ws_url", venue.user_ws_url()),
    ] {
        source.insert(key, value(rewritten));
    }
    for (key, rewritten) in [
        ("heartbeat_interval_seconds", toml_edit::Value::from(15)),
        ("relayer_poll_interval_seconds", toml_edit::Value::from(5)),
        (
            "metadata_refresh_interval_seconds",
            toml_edit::Value::from(60),
        ),
    ] {
        source.insert(key, toml_edit::Item::Value(rewritten));
    }

    document.to_string()
}

fn live_run_session(
    run_session_id: &str,
    config_path: &std::path::Path,
    startup_target_revision_at_start: &str,
    state: RunSessionState,
    started_at: chrono::DateTime<Utc>,
) -> RunSessionRow {
    RunSessionRow {
        run_session_id: run_session_id.to_owned(),
        invoked_by: "run".to_owned(),
        mode: "live".to_owned(),
        state,
        started_at,
        last_seen_at: started_at,
        ended_at: matches!(state, RunSessionState::Exited | RunSessionState::Failed)
            .then(|| started_at + ChronoDuration::seconds(30)),
        exit_status: (state == RunSessionState::Exited).then(|| "success".to_owned()),
        exit_reason: (state == RunSessionState::Failed).then(|| "seeded failure".to_owned()),
        config_path: config_path.display().to_string(),
        config_fingerprint: config_fingerprint(config_path),
        target_source_kind: "adopted".to_owned(),
        startup_target_revision_at_start: startup_target_revision_at_start.to_owned(),
        configured_operator_target_revision: Some(startup_target_revision_at_start.to_owned()),
        active_operator_target_revision_at_start: Some(startup_target_revision_at_start.to_owned()),
        configured_operator_strategy_revision: None,
        active_operator_strategy_revision_at_start: None,
        rollout_state_at_start: Some("ready".to_owned()),
        real_user_shadow_smoke: false,
    }
}

fn config_fingerprint(config_path: &std::path::Path) -> String {
    use sha2::{Digest, Sha256};

    let raw = std::fs::read(config_path).expect("config fixture should read");
    format!("{:x}", Sha256::digest(raw))
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
                    break;
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
