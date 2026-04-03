mod support;

use std::{fs, process::Command};

use chrono::{Duration, Utc};
use domain::ExecutionMode;
use serde_json::json;
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
fn verify_paper_passes_with_warnings_when_basic_run_evidence_is_incomplete_but_no_live_attempts_exist(
) {
    let verify_db = verify_db::TestDatabase::new();

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(cli::config_fixture("app-live-paper.toml"))
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Scenario: paper"), "{text}");
    assert!(text.contains("Verdict: PASS WITH WARNINGS"), "{text}");
    assert!(text.contains("Expectation: paper-no-live"), "{text}");

    verify_db.cleanup();
}

#[test]
fn verify_paper_blocked_without_database_url_fails_instead_of_warning() {
    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(cli::config_fixture("app-live-paper.toml"))
        .env_remove("DATABASE_URL")
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("Scenario: paper"), "{text}");
    assert!(text.contains("Verdict: FAIL"), "{text}");
    assert!(text.contains("DATABASE_URL"), "{text}");
}

#[test]
fn verify_paper_fails_when_live_attempts_are_observed() {
    let verify_db = verify_db::TestDatabase::new();
    verify_db.seed_live_attempt("attempt-live-1");

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(cli::config_fixture("app-live-paper.toml"))
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Verdict: FAIL"), "{text}");
    assert!(text.contains("forbidden live side effects"), "{text}");
    assert!(text.contains("Side Effects: 1"), "{text}");

    verify_db.cleanup();
}

#[test]
fn verify_paper_explicit_live_attempt_window_still_fails_forbidden_live_side_effects() {
    let verify_db = verify_db::TestDatabase::new();
    verify_db.seed_live_attempt("attempt-live-explicit-1");

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(cli::config_fixture("app-live-paper.toml"))
        .arg("--attempt-id")
        .arg("attempt-live-explicit-1")
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Verdict: FAIL"), "{text}");
    assert!(text.contains("forbidden live side effects"), "{text}");

    verify_db.cleanup();
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
fn verify_live_passes_when_local_results_match_current_config_and_control_plane() {
    let verify_db = verify_db::TestDatabase::new();
    verify_db.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-9"));
    verify_db.seed_live_attempt_with_artifacts("attempt-live-1");
    let config_path = verify_db::temp_config_path(
        "app-live-verify-live-ready",
        &verify_db::live_ready_config(),
    );

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Scenario: live"), "{text}");
    assert!(text.contains("Verdict: PASS"), "{text}");
    assert!(
        text.contains("Expectation: live-config-consistent"),
        "{text}"
    );

    verify_db.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn verify_live_latest_window_ignores_non_neg_risk_live_attempts() {
    let verify_db = verify_db::TestDatabase::new();
    verify_db.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-9"));
    verify_db.seed_attempt(verify_db::sample_attempt_for_route(
        "attempt-live-other-route",
        ExecutionMode::Live,
        "other-route",
    ));
    let config_path = verify_db::temp_config_path(
        "app-live-verify-live-neg-risk-only",
        &verify_db::live_ready_config(),
    );

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Scenario: live"), "{text}");
    assert!(text.contains("Verdict: FAIL"), "{text}");
    assert!(text.contains("no live results were observed"), "{text}");

    verify_db.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn verify_live_fails_when_results_conflict_with_current_mode_and_readiness() {
    let verify_db = verify_db::TestDatabase::new();
    verify_db.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-9"));
    verify_db.seed_shadow_attempt_with_artifacts("attempt-shadow-1");
    let config_path = verify_db::temp_config_path(
        "app-live-verify-live-conflict",
        &verify_db::live_ready_config(),
    );

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Verdict: FAIL"), "{text}");
    assert!(text.contains("contradictory local outcomes"), "{text}");

    verify_db.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn verify_expect_override_selects_a_fixed_profile_set() {
    let verify_db = verify_db::TestDatabase::new();
    verify_db.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-9"));
    verify_db.seed_live_attempt_with_artifacts("attempt-live-override-1");
    let config_path = verify_db::temp_config_path(
        "app-live-verify-live-override",
        &verify_db::live_ready_config(),
    );

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(&config_path)
        .arg("--expect")
        .arg("paper-no-live")
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Expectation: paper-no-live"), "{text}");
    assert!(text.contains("Verdict: FAIL"), "{text}");

    verify_db.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn verify_smoke_expect_override_rejects_incompatible_profile() {
    let verify_db = verify_db::TestDatabase::new();
    verify_db.seed_smoke_runtime_progress("targets-rev-9");
    verify_db.seed_shadow_attempt_with_artifacts("attempt-shadow-smoke-override-1");
    let config_path = verify_db::temp_config_path(
        "app-live-verify-smoke-override",
        &verify_db::config_shapes::smoke_ready_config(),
    );

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(&config_path)
        .arg("--expect")
        .arg("live-config-consistent")
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(
        text.contains("Expectation: live-config-consistent"),
        "{text}"
    );
    assert!(text.contains("Verdict: FAIL"), "{text}");
    assert!(text.contains("not compatible"), "{text}");

    verify_db.cleanup();
    let _ = fs::remove_file(config_path);
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
    assert!(text.contains("Verdict: PASS WITH WARNINGS"), "{text}");
    assert!(
        text.contains("historical window is not provably tied to the current config anchor"),
        "{text}"
    );

    verify_db.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn verify_since_includes_recent_journal_rows_even_when_runtime_progress_is_ahead() {
    let verify_db = verify_db::TestDatabase::new();
    verify_db.seed_runtime_progress(500, 7, Some("snapshot-verify-7"), None);
    verify_db.seed_journal(verify_db::sample_journal(
        "recent-journal-1",
        1,
        Utc::now() - Duration::minutes(5),
        json!({ "kind": "recent-journal" }),
    ));

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(cli::config_fixture("app-live-paper.toml"))
        .arg("--since")
        .arg("10m")
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Replay: 1"), "{text}");

    verify_db.cleanup();
}

#[test]
fn verify_paper_latest_window_ignores_unrelated_live_attempts() {
    let verify_db = verify_db::TestDatabase::new();
    verify_db.seed_attempt(verify_db::sample_attempt(
        "attempt-live-unrelated",
        ExecutionMode::Live,
    ));

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(cli::config_fixture("app-live-paper.toml"))
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Scenario: paper"), "{text}");
    assert!(text.contains("Attempts: 0"), "{text}");

    verify_db.cleanup();
}

#[test]
fn verify_report_counts_grouped_live_records_by_total_record_count() {
    let verify_db = verify_db::TestDatabase::new();
    verify_db.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-9"));
    verify_db.seed_runtime_progress(41, 7, Some("snapshot-verify-7"), Some("targets-rev-9"));
    verify_db.seed_attempt(verify_db::sample_attempt(
        "attempt-counts",
        ExecutionMode::Live,
    ));

    let artifact_a = verify_db::sample_live_artifact("attempt-counts");
    let mut artifact_b = verify_db::sample_live_artifact("attempt-counts");
    artifact_b.stream = "negrisk.live.secondary".to_owned();
    verify_db.seed_live_artifact(artifact_a);
    verify_db.seed_live_artifact(artifact_b);

    verify_db.seed_live_submission(verify_db::sample_live_submission(
        "attempt-counts",
        "submission-1",
    ));
    verify_db.seed_live_submission(verify_db::sample_live_submission(
        "attempt-counts",
        "submission-2",
    ));

    let config_path = verify_db::temp_config_path(
        "app-live-verify-counts",
        &verify_db::live_ready_config_for("targets-rev-9"),
    );
    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(&config_path)
        .arg("--attempt-id")
        .arg("attempt-counts")
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Verdict: PASS WITH WARNINGS"), "{text}");
    assert!(text.contains("Artifacts: 2"), "{text}");
    assert!(text.contains("Side Effects: 4"), "{text}");

    verify_db.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn verify_smoke_passes_when_shadow_only_evidence_is_complete() {
    let verify_db = verify_db::TestDatabase::new();
    verify_db.seed_shadow_attempt_with_artifacts("attempt-shadow-1");
    verify_db.seed_smoke_runtime_progress("targets-rev-9");
    let config_path = verify_db::temp_config_path(
        "app-live-verify-smoke-ready",
        &verify_db::config_shapes::smoke_ready_config(),
    );

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Scenario: real-user shadow smoke"), "{text}");
    assert!(text.contains("Verdict: PASS"), "{text}");
    assert!(text.contains("shadow attempts: 1"), "{text}");

    verify_db.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn verify_smoke_fails_when_a_durable_live_attempt_is_present_outside_the_shadow_attempt_window() {
    let verify_db = verify_db::TestDatabase::new();
    verify_db.seed_shadow_attempt_with_artifacts_in_snapshot("attempt-shadow-1", "snapshot-smoke");
    verify_db.seed_live_attempt_in_snapshot("attempt-live-1", "snapshot-live-outside");
    verify_db.seed_smoke_runtime_progress("targets-rev-9");
    let config_path = verify_db::temp_config_path(
        "app-live-verify-smoke-live-side-effects",
        &verify_db::config_shapes::smoke_ready_config(),
    );

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Verdict: FAIL"), "{text}");
    assert!(text.contains("forbidden live side effects"), "{text}");

    verify_db.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn verify_smoke_default_window_reflects_the_latest_smoke_rerun_only() {
    let verify_db = verify_db::TestDatabase::new();
    verify_db.seed_non_working_smoke_run_window_in_snapshot("snapshot-smoke-old");
    verify_db.seed_shadow_attempt_with_artifacts_in_snapshot(
        "attempt-shadow-2",
        "snapshot-smoke-latest",
    );
    verify_db.seed_smoke_runtime_progress("targets-rev-9");
    let config_path = verify_db::temp_config_path(
        "app-live-verify-smoke-latest",
        &verify_db::config_shapes::smoke_ready_config(),
    );

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Verdict: PASS"), "{text}");
    assert!(text.contains("shadow attempts: 1"), "{text}");

    verify_db.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn verify_smoke_latest_window_ignores_newer_non_neg_risk_shadow_snapshots() {
    let verify_db = verify_db::TestDatabase::new();
    verify_db.seed_shadow_attempt_with_artifacts_in_snapshot(
        "attempt-shadow-neg-risk",
        "snapshot-smoke-neg-risk",
    );
    verify_db.seed_attempt(verify_db::sample_attempt_in_snapshot_for_route(
        "attempt-shadow-other-route",
        ExecutionMode::Shadow,
        "snapshot-smoke-other-route",
        "other-route",
    ));
    verify_db.seed_smoke_runtime_progress("targets-rev-9");
    let config_path = verify_db::temp_config_path(
        "app-live-verify-smoke-neg-risk-anchor",
        &verify_db::config_shapes::smoke_ready_config(),
    );

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Scenario: real-user shadow smoke"), "{text}");
    assert!(text.contains("Verdict: PASS"), "{text}");
    assert!(text.contains("shadow attempts: 1"), "{text}");

    verify_db.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn verify_smoke_latest_window_prefers_latest_shadow_attempt_per_scope_within_snapshot() {
    let verify_db = verify_db::TestDatabase::new();
    verify_db.seed_non_working_smoke_run_window_in_snapshot("snapshot-smoke-shared");
    verify_db.seed_shadow_attempt_with_artifacts_in_snapshot(
        "attempt-shadow-latest",
        "snapshot-smoke-shared",
    );
    verify_db.seed_smoke_runtime_progress("targets-rev-9");
    let config_path = verify_db::temp_config_path(
        "app-live-verify-smoke-shared-snapshot",
        &verify_db::config_shapes::smoke_ready_config(),
    );

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Verdict: PASS"), "{text}");
    assert!(text.contains("shadow attempts: 1"), "{text}");

    verify_db.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn verify_smoke_requires_real_run_evidence_before_it_can_be_any_pass_variant() {
    let verify_db = verify_db::TestDatabase::new();
    verify_db.seed_smoke_runtime_progress("targets-rev-9");
    let config_path = verify_db::temp_config_path(
        "app-live-verify-smoke-empty",
        &verify_db::config_shapes::smoke_ready_config(),
    );

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Verdict: FAIL"), "{text}");
    assert!(text.contains("no credible run evidence exists"), "{text}");

    verify_db.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn verify_smoke_preflight_only_run_can_warn_without_failing() {
    let verify_db = verify_db::TestDatabase::new();
    verify_db.seed_smoke_runtime_progress("targets-rev-9");
    verify_db.seed_non_working_smoke_run_window();
    let config_path = verify_db::temp_config_path(
        "app-live-verify-smoke-rollout-required",
        &verify_db::config_shapes::smoke_rollout_required_config(),
    );

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Verdict: PASS WITH WARNINGS"), "{text}");
    assert!(text.contains("rollout not ready"), "{text}");
    assert!(
        text.contains(&format!(
            "Next: app-live bootstrap --config {}",
            cli::shell_quote_path(&config_path)
        )),
        "{text}"
    );

    verify_db.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn verify_live_rollout_required_uses_concrete_rollout_enablement_guidance() {
    let verify_db = verify_db::TestDatabase::new();
    verify_db.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-9"));
    verify_db.seed_live_attempt_with_artifacts("attempt-live-rollout-required-1");
    let config_path = verify_db::temp_config_path(
        "app-live-verify-live-rollout-required",
        &verify_db::live_rollout_required_config_for("targets-rev-9"),
    );

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Verdict: PASS WITH WARNINGS"), "{text}");
    assert!(
        text.contains(&format!(
            "Next: edit {} and set [negrisk.rollout].approved_families and ready_families for adopted families",
            cli::shell_quote_path(&config_path)
        )),
        "{text}"
    );

    verify_db.cleanup();
    let _ = fs::remove_file(config_path);
}
