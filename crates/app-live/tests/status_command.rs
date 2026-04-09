mod support;

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use chrono::{Duration, Utc};
use persistence::{
    models::{
        AdoptableStrategyRevisionRow, StrategyAdoptionProvenanceRow, StrategyCandidateSetRow,
    },
    RunSessionRow, RunSessionState, RuntimeProgressRepo, StrategyAdoptionRepo,
    StrategyControlArtifactRepo,
};
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use support::{cli, status_db::TestDatabase};
use toml_edit::{table, value, DocumentMut};

#[test]
fn status_subcommand_is_exposed() {
    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(cli::config_fixture("app-live-paper.toml"))
        .env(
            "DATABASE_URL",
            "postgres://axiom:axiom@localhost:5432/axiom_arb",
        )
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    let summary_idx = combined
        .find("Summary")
        .unwrap_or_else(|| panic!("{combined}"));
    let key_details_idx = combined
        .find("Key Details")
        .unwrap_or_else(|| panic!("{combined}"));
    let next_actions_idx = combined
        .find("Next Actions")
        .unwrap_or_else(|| panic!("{combined}"));
    assert!(summary_idx < key_details_idx, "{combined}");
    assert!(key_details_idx < next_actions_idx, "{combined}");
    assert!(combined.contains("Mode: paper"), "{combined}");
    assert!(combined.contains("Readiness: paper-ready"), "{combined}");
    assert!(combined.contains("No additional details"), "{combined}");
    assert!(!combined.contains("Restart needed:"), "{combined}");
    assert!(
        combined.contains("Next: app-live run --config"),
        "{combined}"
    );
}

#[test]
fn status_invalid_paper_config_is_blocked() {
    let config_path = temp_invalid_config_path();

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config_path)
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(!combined.contains("Mode: paper"), "{combined}");
    assert!(!combined.contains("Mode:"), "{combined}");
    assert!(combined.contains("Readiness: blocked"), "{combined}");
    assert!(combined.contains("Next Actions"), "{combined}");
    assert!(
        combined.contains("Next: fix the blocking issue, then rerun app-live status --config"),
        "{combined}"
    );
}

#[test]
fn status_paper_mode_without_database_url_is_blocked() {
    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(cli::config_fixture("app-live-paper.toml"))
        .env_remove("DATABASE_URL")
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(combined.contains("Mode: paper"), "{combined}");
    assert!(combined.contains("Readiness: blocked"), "{combined}");
    assert!(
        combined.contains("Reason: DATABASE_URL is required before paper run can start"),
        "{combined}"
    );
    assert!(
        combined.contains("Next: fix the blocking issue, then rerun app-live status --config"),
        "{combined}"
    );
    assert!(!combined.contains("Readiness: paper-ready"), "{combined}");
}

#[test]
fn status_adopted_source_without_operator_strategy_revision_is_blocked() {
    let database = TestDatabase::new();
    let config_path = temp_config_fixture_path("app-live-ux-live.toml", |config| {
        config.replace("operator_strategy_revision = \"targets-rev-9\"\n", "")
    });

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(combined.contains("Readiness: blocked"), "{combined}");
    assert!(
        combined.contains("missing strategy_control.operator_strategy_revision"),
        "{combined}"
    );
    assert!(
        combined.contains("Next: fix the blocking issue, then rerun app-live status --config"),
        "{combined}"
    );

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn status_smoke_without_discovery_artifacts_is_discovery_required() {
    let database = TestDatabase::new();
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        with_legacy_smoke_target_source_without_revision(config)
    });

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(
        combined.contains("Mode: real-user shadow smoke"),
        "{combined}"
    );
    assert!(
        combined.contains("Readiness: discovery-required"),
        "{combined}"
    );
    assert!(
        combined.contains("Next: app-live discover --config"),
        "{combined}"
    );
    assert!(
        !combined.contains("Next: app-live apply --config"),
        "{combined}"
    );

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn status_smoke_with_advisory_only_candidate_is_discovery_ready_not_adoptable() {
    let database = TestDatabase::new();
    database.seed_advisory_candidate(
        "candidate-8",
        "candidate generation deferred until discovery backfill completes",
    );
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        with_legacy_smoke_target_source_without_revision(config)
    });

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(
        combined.contains("Readiness: discovery-ready-not-adoptable"),
        "{combined}"
    );
    assert!(
        combined.contains("candidate generation deferred until discovery backfill completes"),
        "{combined}"
    );
    assert!(
        combined.contains("Next: app-live targets candidates --config"),
        "{combined}"
    );
    assert!(
        !combined.contains("Next: app-live apply --config"),
        "{combined}"
    );

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn status_smoke_with_adoptable_artifacts_and_no_anchor_is_adoptable_ready() {
    let database = TestDatabase::new();
    database.seed_adopted_target_without_provenance("targets-rev-9");
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        with_legacy_smoke_target_source_without_revision(config)
    });

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(
        combined.contains("Readiness: adoptable-ready"),
        "{combined}"
    );
    assert!(
        combined.contains("Next: app-live apply --config"),
        "{combined}"
    );
    assert!(
        !combined.contains("Next: app-live doctor --config"),
        "{combined}"
    );
    assert!(
        !combined.contains("Next: app-live targets adopt --config"),
        "{combined}"
    );

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn status_adopted_source_with_mismatched_active_revision_is_restart_required() {
    let database = TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-10"));
    let config = cli::config_fixture("app-live-ux-live.toml");

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(combined.contains("Mode: live"), "{combined}");
    assert!(
        combined.contains("Readiness: restart-required"),
        "{combined}"
    );
    assert!(combined.contains("Rollout state: required"), "{combined}");
    assert!(
        combined.contains("Reason: configured and active operator_strategy_revision differ; rollout must cover adopted families: family-a"),
        "{combined}"
    );
    assert!(combined.contains("Next: edit "), "{combined}");
    assert!(
        combined.contains("Next: perform controlled restart"),
        "{combined}"
    );
    assert!(
        !combined.contains("Next: app-live apply --config"),
        "{combined}"
    );

    database.cleanup();
}

#[test]
fn status_restart_required_preserves_ready_rollout_state_when_rollout_is_already_configured() {
    let database = TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-10"));
    let config = temp_config_fixture_path("app-live-ux-live.toml", |config| {
        format!(
            "{config}\n[negrisk.rollout]\napproved_families = [\"family-a\"]\nready_families = [\"family-a\"]\n"
        )
    });

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(combined.contains("Mode: live"), "{combined}");
    assert!(
        combined.contains("Readiness: restart-required"),
        "{combined}"
    );
    assert!(combined.contains("Rollout state: ready"), "{combined}");
    assert!(
        combined.contains("Reason: configured and active operator_strategy_revision differ; adopted families are covered by rollout: family-a"),
        "{combined}"
    );
    assert!(
        combined.contains("Next: app-live apply --config"),
        "{combined}"
    );
    assert!(
        !combined.contains("Next: perform controlled restart"),
        "{combined}"
    );

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn status_smoke_restart_required_with_ready_rollout_keeps_restart_guidance() {
    let database = TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-10"));
    let config = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        config
            .replace("approved_scopes = []", "approved_scopes = [\"family-a\"]")
            .replace("ready_scopes = []", "ready_scopes = [\"family-a\"]")
    });

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(
        combined.contains("Mode: real-user shadow smoke"),
        "{combined}"
    );
    assert!(
        combined.contains("Readiness: restart-required"),
        "{combined}"
    );
    assert!(combined.contains("Rollout state: ready"), "{combined}");
    assert!(
        combined.contains("Next: perform controlled restart"),
        "{combined}"
    );
    assert!(
        !combined.contains("Next: app-live apply --config"),
        "{combined}"
    );

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn status_restart_required_shows_relevant_and_conflicting_active_sessions() {
    let database = TestDatabase::new();
    let config = cli::config_fixture("app-live-ux-smoke.toml");
    database.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-8"));
    database.seed_run_session(strategy_only_smoke_run_session(
        "rs-new",
        &config,
        "targets-rev-9",
        RunSessionState::Exited,
        Utc::now() - Duration::minutes(1),
    ));
    database.seed_run_session(strategy_only_smoke_run_session(
        "rs-old",
        &config,
        "targets-rev-8",
        RunSessionState::Running,
        Utc::now() - Duration::minutes(2),
    ));
    database.seed_runtime_progress(Some("targets-rev-8"), Some("rs-old"));

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(
        combined.contains("Relevant run session: rs-new"),
        "{combined}"
    );
    assert!(
        combined.contains("Conflicting active run session: rs-old"),
        "{combined}"
    );
    assert!(
        combined.contains("Conflicting active state: running"),
        "{combined}"
    );
    let relevant_idx = combined
        .find("Relevant run session: rs-new")
        .unwrap_or_else(|| panic!("{combined}"));
    let conflicting_idx = combined
        .find("Conflicting active run session: rs-old")
        .unwrap_or_else(|| panic!("{combined}"));
    let next_actions_idx = combined
        .find("Next Actions")
        .unwrap_or_else(|| panic!("{combined}"));
    assert!(relevant_idx < next_actions_idx, "{combined}");
    assert!(conflicting_idx < next_actions_idx, "{combined}");

    database.cleanup();
}

#[test]
fn status_restart_required_does_not_duplicate_the_same_run_session_as_conflicting_active() {
    let database = TestDatabase::new();
    let config = cli::config_fixture("app-live-ux-live.toml");
    database.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-8"));
    database.seed_run_session(RunSessionRow {
        run_session_id: "rs-same".to_owned(),
        invoked_by: "run".to_owned(),
        mode: "live".to_owned(),
        state: RunSessionState::Running,
        started_at: Utc::now() - Duration::minutes(1),
        last_seen_at: Utc::now() - Duration::minutes(1),
        ended_at: None,
        exit_status: None,
        exit_reason: None,
        config_path: config.display().to_string(),
        config_fingerprint: config_fingerprint(&config),
        target_source_kind: "adopted".to_owned(),
        startup_target_revision_at_start: "targets-rev-9".to_owned(),
        configured_operator_target_revision: Some("targets-rev-9".to_owned()),
        active_operator_target_revision_at_start: Some("targets-rev-8".to_owned()),
        configured_operator_strategy_revision: Some("targets-rev-9".to_owned()),
        active_operator_strategy_revision_at_start: Some("targets-rev-8".to_owned()),
        rollout_state_at_start: Some("required".to_owned()),
        real_user_shadow_smoke: false,
    });
    database.seed_runtime_progress(Some("targets-rev-8"), Some("rs-same"));

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(
        !combined.contains("Relevant run session: rs-same"),
        "{combined}"
    );
    assert!(
        combined.contains("Conflicting active run session: rs-same"),
        "{combined}"
    );

    database.cleanup();
}

#[test]
fn status_pure_neutral_adopted_config_matches_strategy_only_relevant_run_session() {
    let database = TestDatabase::new();
    let config = cli::config_fixture("app-live-ux-smoke.toml");
    database.seed_adopted_target_with_active_revision("targets-rev-9", None);
    database.seed_run_session(strategy_only_smoke_run_session(
        "rs-strategy-only",
        &config,
        "targets-rev-9",
        RunSessionState::Running,
        Utc::now() - Duration::minutes(1),
    ));

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(
        combined.contains("Relevant run session: rs-strategy-only"),
        "{combined}"
    );

    database.cleanup();
}

#[test]
fn status_pure_neutral_adopted_config_with_distinct_strategy_revision_matches_relevant_session() {
    let database = TestDatabase::new();
    let config = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        config.replace(
            "operator_strategy_revision = \"targets-rev-9\"",
            "operator_strategy_revision = \"strategy-rev-12\"",
        )
    });
    seed_strategy_adoption_lineage(
        database.database_url(),
        "strategy-candidate-12",
        "adoptable-strategy-12",
        "strategy-rev-12",
        "targets-rev-9",
    );
    database.seed_run_session(strategy_only_smoke_run_session(
        "rs-strategy-distinct",
        &config,
        "strategy-rev-12",
        RunSessionState::Running,
        Utc::now() - Duration::minutes(1),
    ));

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(
        combined.contains("Relevant run session: rs-strategy-distinct"),
        "{combined}"
    );
    assert!(
        !combined.contains("resolved adopted target set did not contain any families"),
        "{combined}"
    );

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn status_pure_neutral_adopted_config_with_missing_strategy_provenance_reports_specific_reason() {
    let database = TestDatabase::new();
    let config = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        config.replace(
            "operator_strategy_revision = \"targets-rev-9\"",
            "operator_strategy_revision = \"strategy-rev-missing\"",
        )
    });

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(combined.contains("Readiness: blocked"), "{combined}");
    assert!(combined.contains("strategy-rev-missing"), "{combined}");
    assert!(combined.contains("provenance"), "{combined}");
    assert!(
        !combined.contains("resolved adopted target set did not contain any families"),
        "{combined}"
    );

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn status_pure_neutral_adopted_config_without_operator_strategy_revision_is_blocked() {
    let database = TestDatabase::new();
    let config = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        config.replace("operator_strategy_revision = \"targets-rev-9\"\n", "")
    });

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(combined.contains("Readiness: blocked"), "{combined}");
    assert!(
        combined.contains("missing strategy_control.operator_strategy_revision"),
        "{combined}"
    );
    assert!(
        !combined.contains("Readiness: adoptable-ready"),
        "{combined}"
    );
    assert!(
        !combined.contains("Readiness: discovery-required"),
        "{combined}"
    );

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn status_pure_neutral_adopted_config_prefers_active_strategy_anchor_for_restart_detection() {
    let database = TestDatabase::new();
    let config = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        config
            .replace(
                "operator_strategy_revision = \"targets-rev-9\"",
                "operator_strategy_revision = \"strategy-rev-12\"",
            )
            .replace("approved_scopes = []", "approved_scopes = [\"family-a\"]")
            .replace("ready_scopes = []", "ready_scopes = [\"family-a\"]")
    });
    seed_strategy_adoption_lineage(
        database.database_url(),
        "strategy-candidate-12",
        "adoptable-strategy-12",
        "strategy-rev-12",
        "targets-rev-9",
    );
    seed_runtime_progress_with_strategy_anchor(
        database.database_url(),
        Some("targets-rev-active"),
        Some("strategy-rev-12"),
    );

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(
        !combined.contains("Readiness: restart-required"),
        "{combined}"
    );
    assert!(
        combined.contains("Readiness: smoke-config-ready"),
        "{combined}"
    );
    assert!(
        !combined.contains("configured and active operator_strategy_revision differ"),
        "{combined}"
    );

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn status_projects_overdue_running_session_as_stale() {
    let database = TestDatabase::new();
    let config = cli::config_fixture("app-live-ux-smoke.toml");
    database.seed_adopted_target_with_active_revision("targets-rev-9", None);
    database.seed_run_session(strategy_only_smoke_run_session(
        "rs-stale",
        &config,
        "targets-rev-9",
        RunSessionState::Running,
        Utc::now() - Duration::minutes(20),
    ));

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(combined.contains("Relevant run state: stale"), "{combined}");

    database.cleanup();
}

#[test]
fn status_adopted_source_with_unavailable_active_revision_is_live_rollout_required() {
    let database = TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", None);
    let config = cli::config_fixture("app-live-ux-live.toml");

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(combined.contains("Mode: live"), "{combined}");
    assert!(
        combined.contains("Readiness: live-rollout-required"),
        "{combined}"
    );
    assert!(
        combined.contains("Reason: rollout must cover adopted families: family-a"),
        "{combined}"
    );
    assert!(combined.contains("Next: edit "), "{combined}");
    assert!(
        combined.contains(
            "[strategies.neg_risk.rollout].approved_scopes and ready_scopes for adopted scopes"
        ),
        "{combined}"
    );

    database.cleanup();
}

#[test]
fn status_adopted_source_with_live_rollout_enabled_is_live_config_ready() {
    let database = TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", None);
    let config = temp_config_fixture_path("app-live-ux-live.toml", |config| {
        format!(
            "{config}\n[negrisk.rollout]\napproved_families = [\"family-a\"]\nready_families = [\"family-a\"]\n"
        )
    });

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(combined.contains("Mode: live"), "{combined}");
    assert!(
        combined.contains("Readiness: live-config-ready"),
        "{combined}"
    );
    assert!(
        combined.contains("Reason: adopted families are covered by rollout: family-a"),
        "{combined}"
    );
    assert!(
        combined.contains("Next: app-live apply --config"),
        "{combined}"
    );
    assert!(
        !combined.contains("Next: app-live doctor --config"),
        "{combined}"
    );

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn status_route_owned_rollout_overrides_legacy_rollout_when_both_are_present() {
    let database = TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", None);
    let config = temp_config_fixture_path("app-live-ux-live.toml", |config| {
        format!(
            "{config}\n[negrisk.rollout]\napproved_families = []\nready_families = []\n\n[strategies.neg_risk]\nenabled = true\n\n[strategies.neg_risk.rollout]\napproved_scopes = [\"family-a\"]\nready_scopes = [\"family-a\"]\n"
        )
    });

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(
        combined.contains("Readiness: live-config-ready"),
        "{combined}"
    );
    assert!(
        combined.contains("Reason: adopted families are covered by rollout: family-a"),
        "{combined}"
    );

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn status_adopted_smoke_source_with_unavailable_active_revision_is_smoke_rollout_required() {
    let database = TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", None);
    let config = cli::config_fixture("app-live-ux-smoke.toml");

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(
        combined.contains("Mode: real-user shadow smoke"),
        "{combined}"
    );
    assert!(
        combined.contains("Readiness: smoke-rollout-required"),
        "{combined}"
    );
    assert!(
        combined.contains("Reason: rollout must cover adopted families: family-a"),
        "{combined}"
    );
    assert!(
        combined.contains("Next: app-live apply --config"),
        "{combined}"
    );
    assert!(
        !combined.contains("Next: app-live bootstrap --config"),
        "{combined}"
    );

    database.cleanup();
}

#[test]
fn status_smoke_rollout_required_points_to_apply() {
    let database = TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", None);
    let config = cli::config_fixture("app-live-ux-smoke.toml");

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(
        combined.contains("Readiness: smoke-rollout-required"),
        "{combined}"
    );
    assert!(
        combined.contains("Next: app-live apply --config"),
        "{combined}"
    );
    assert!(
        !combined.contains("Next: app-live bootstrap --config"),
        "{combined}"
    );

    database.cleanup();
}

#[test]
fn status_adopted_smoke_source_with_rollout_enabled_is_smoke_config_ready() {
    let database = TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", None);
    let config = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        config
            .replace("approved_scopes = []", "approved_scopes = [\"family-a\"]")
            .replace("ready_scopes = []", "ready_scopes = [\"family-a\"]")
    });

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(
        combined.contains("Mode: real-user shadow smoke"),
        "{combined}"
    );
    assert!(
        combined.contains("Readiness: smoke-config-ready"),
        "{combined}"
    );
    assert!(
        combined.contains("Reason: adopted families are covered by rollout: family-a"),
        "{combined}"
    );
    assert!(
        combined.contains("Next: app-live doctor --config"),
        "{combined}"
    );

    database.cleanup();
    let _ = fs::remove_file(config);
}

fn with_legacy_smoke_target_source_without_revision(config: String) -> String {
    let mut document = config
        .parse::<DocumentMut>()
        .expect("config fixture should parse as TOML");
    let root = document.as_table_mut();
    root.remove("strategy_control");
    if root.get("negrisk").is_none() {
        root.insert("negrisk", table());
    }
    let negrisk = root
        .get_mut("negrisk")
        .expect("config fixture should contain [negrisk]")
        .as_table_like_mut()
        .expect("config fixture should contain [negrisk]");
    if negrisk.get("target_source").is_none() {
        negrisk.insert("target_source", table());
    }
    let target_source = negrisk
        .get_mut("target_source")
        .expect("config fixture should contain [negrisk.target_source]")
        .as_table_like_mut()
        .expect("[negrisk.target_source] should be a table");
    target_source.insert("source", value("adopted"));
    target_source.remove("operator_target_revision");

    document.to_string()
}

#[test]
fn status_adopted_source_with_broken_durable_provenance_is_blocked() {
    let database = TestDatabase::new();
    database.seed_adopted_target_without_provenance("targets-rev-9");
    let config = cli::config_fixture("app-live-ux-live.toml");

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(!combined.contains("Mode:"), "{combined}");
    assert!(combined.contains("Readiness: blocked"), "{combined}");
    assert!(combined.contains("Next Actions"), "{combined}");
    assert!(
        combined.contains("Next: fix the blocking issue, then rerun app-live status --config"),
        "{combined}"
    );

    database.cleanup();
}

#[test]
fn status_reports_compatibility_mode_explicitly_for_legacy_explicit_targets() {
    let config = compatibility_mode_live_config_path();

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config)
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(combined.contains("Mode: live"), "{combined}");
    assert!(combined.contains("Readiness: blocked"), "{combined}");
    assert!(
        combined.contains("Target source: legacy explicit targets"),
        "{combined}"
    );
    assert!(
        combined.contains("Reason: migration required: legacy explicit targets require migration"),
        "{combined}"
    );
    assert!(
        combined.contains("Next: app-live targets adopt --config"),
        "{combined}"
    );
    assert!(!combined.contains("compatibility"), "{combined}");

    let _ = fs::remove_file(config);
}

#[test]
fn status_reports_migration_required_for_legacy_explicit_targets() {
    let config = compatibility_mode_live_config_path();

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config)
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(combined.contains("migration required"), "{combined}");
    assert!(
        combined.contains("Next: app-live targets adopt --config"),
        "{combined}"
    );
    assert!(!combined.contains("compatibility"), "{combined}");
    assert!(!combined.contains("--adopt-compatibility"), "{combined}");

    let _ = fs::remove_file(config);
}

#[test]
fn status_output_uses_summary_key_details_next_actions_order() {
    let database = TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", None);
    let config = temp_config_fixture_path("app-live-ux-live.toml", |config| {
        format!(
            "{config}\n[negrisk.rollout]\napproved_families = [\"family-a\"]\nready_families = [\"family-a\"]\n"
        )
    });

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");

    let summary_idx = combined
        .find("Summary")
        .unwrap_or_else(|| panic!("{combined}"));
    let key_details_idx = combined
        .find("Key Details")
        .unwrap_or_else(|| panic!("{combined}"));
    let next_actions_idx = combined
        .find("Next Actions")
        .unwrap_or_else(|| panic!("{combined}"));
    assert!(summary_idx < key_details_idx, "{combined}");
    assert!(key_details_idx < next_actions_idx, "{combined}");

    assert!(combined.contains("Mode: live"), "{combined}");
    assert!(
        combined.contains("Readiness: live-config-ready"),
        "{combined}"
    );
    assert!(
        combined.contains("Configured target: targets-rev-9"),
        "{combined}"
    );
    assert!(combined.contains("Rollout state: ready"), "{combined}");
    assert!(
        combined.contains("Next: app-live apply --config"),
        "{combined}"
    );

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn status_operator_shaped_explicit_targets_still_report_compatibility_mode() {
    let config = compatibility_mode_live_config_path();

    let output = Command::new(cli::app_live_binary())
        .arg("status")
        .arg("--config")
        .arg(&config)
        .output()
        .expect("app-live status should execute");

    let combined = cli::combined(&output);
    assert!(output.status.success(), "{combined}");
    assert!(combined.contains("Mode: live"), "{combined}");
    assert!(combined.contains("Readiness: blocked"), "{combined}");
    assert!(
        combined.contains("Target source: legacy explicit targets"),
        "{combined}"
    );
    assert!(
        combined.contains("Reason: migration required: legacy explicit targets require migration"),
        "{combined}"
    );
    assert!(
        combined.contains("Next: app-live targets adopt --config"),
        "{combined}"
    );
    assert!(!combined.contains("compatibility"), "{combined}");

    let _ = fs::remove_file(config);
}

#[test]
fn status_mode_labels_use_operator_vocabulary() {
    use app_live::commands::status::model::StatusMode;

    let cases = [
        (StatusMode::Paper, "paper"),
        (StatusMode::RealUserShadowSmoke, "real-user shadow smoke"),
        (StatusMode::Live, "live"),
    ];

    for (mode, label) in cases {
        assert_eq!(mode.label(), label);
    }
}

#[test]
fn status_readiness_labels_use_operator_vocabulary() {
    use app_live::commands::status::model::StatusReadiness;

    let cases = [
        (StatusReadiness::PaperReady, "paper-ready"),
        (StatusReadiness::DiscoveryRequired, "discovery-required"),
        (
            StatusReadiness::DiscoveryReadyNotAdoptable,
            "discovery-ready-not-adoptable",
        ),
        (StatusReadiness::AdoptableReady, "adoptable-ready"),
        (StatusReadiness::RestartRequired, "restart-required"),
        (
            StatusReadiness::SmokeRolloutRequired,
            "smoke-rollout-required",
        ),
        (StatusReadiness::SmokeConfigReady, "smoke-config-ready"),
        (
            StatusReadiness::LiveRolloutRequired,
            "live-rollout-required",
        ),
        (StatusReadiness::LiveConfigReady, "live-config-ready"),
        (StatusReadiness::Blocked, "blocked"),
    ];

    for (readiness, label) in cases {
        assert_eq!(readiness.label(), label);
    }
}

#[test]
fn status_actions_are_concrete_operator_actions() {
    use app_live::commands::status::model::StatusAction;

    let cases = [
        (StatusAction::RunDiscover, "run discover"),
        (
            StatusAction::InspectDiscoveryReasons,
            "inspect discovery reasons",
        ),
        (
            StatusAction::ChooseAndAdoptRevision,
            "choose and adopt an adoptable revision",
        ),
        (StatusAction::RunDoctor, "run doctor"),
        (StatusAction::RunAppLiveApply, "run app-live apply"),
        (
            StatusAction::PerformControlledRestart,
            "perform controlled restart",
        ),
        (StatusAction::RunAppLiveRun, "run app-live run"),
        (
            StatusAction::FixBlockingIssueAndRerunStatus,
            "fix blocking issue and rerun status",
        ),
        (StatusAction::EnableSmokeRollout, "enable smoke rollout"),
        (StatusAction::EnableLiveRollout, "enable live rollout"),
        (
            StatusAction::MigrateLegacyExplicitTargets,
            "migrate legacy explicit targets",
        ),
    ];

    for (action, label) in cases {
        assert_eq!(action.label(), label);
    }
}

#[test]
fn status_details_use_structured_key_fields() {
    use app_live::commands::status::model::{
        StatusAction, StatusDetails, StatusReadiness, StatusRolloutState, StatusSummary,
        StatusTargetSource,
    };

    let details = StatusDetails {
        configured_target: Some("target-a".to_owned()),
        active_target: Some("target-b".to_owned()),
        target_source: Some(StatusTargetSource::LegacyExplicitTargets),
        rollout_state: Some(StatusRolloutState::Ready),
        restart_needed: Some(true),
        relevant_run_session_id: Some("rs-relevant".to_owned()),
        relevant_run_state: Some("running".to_owned()),
        relevant_run_started_at: None,
        relevant_startup_target_revision: Some("startup-target-a".to_owned()),
        conflicting_active_run_session_id: Some("rs-conflict".to_owned()),
        conflicting_active_run_state: Some("stale".to_owned()),
        conflicting_active_started_at: None,
        conflicting_active_startup_target_revision: Some("startup-target-b".to_owned()),
        reason: Some("blocked until rollout is enabled".to_owned()),
    };

    assert_eq!(details.configured_target.as_deref(), Some("target-a"));
    assert_eq!(details.active_target.as_deref(), Some("target-b"));
    assert_eq!(
        details.target_source,
        Some(StatusTargetSource::LegacyExplicitTargets)
    );
    assert_eq!(
        details.target_source.unwrap().label(),
        "legacy explicit targets"
    );
    assert_eq!(details.rollout_state, Some(StatusRolloutState::Ready));
    assert_eq!(details.rollout_state.unwrap().label(), "ready");
    assert_eq!(details.restart_needed, Some(true));
    assert_eq!(
        details.relevant_run_session_id.as_deref(),
        Some("rs-relevant")
    );
    assert_eq!(details.relevant_run_state.as_deref(), Some("running"));
    assert_eq!(
        details.relevant_startup_target_revision.as_deref(),
        Some("startup-target-a")
    );
    assert_eq!(
        details.conflicting_active_run_session_id.as_deref(),
        Some("rs-conflict")
    );
    assert_eq!(
        details.conflicting_active_run_state.as_deref(),
        Some("stale")
    );
    assert_eq!(
        details
            .conflicting_active_startup_target_revision
            .as_deref(),
        Some("startup-target-b")
    );
    assert_eq!(
        details.reason.as_deref(),
        Some("blocked until rollout is enabled")
    );

    let summary = StatusSummary {
        mode: Some(app_live::commands::status::model::StatusMode::Live),
        readiness: StatusReadiness::Blocked,
        details,
        actions: vec![StatusAction::RunDoctor],
    };

    assert_eq!(summary.mode.map(|mode| mode.label()), Some("live"));
    assert_eq!(summary.readiness.label(), "blocked");
    assert_eq!(summary.actions[0].label(), "run doctor");
}

#[test]
fn status_target_source_labels_are_structured() {
    use app_live::commands::status::model::StatusTargetSource;

    let cases = [
        (
            StatusTargetSource::LegacyExplicitTargets,
            "legacy explicit targets",
        ),
        (StatusTargetSource::AdoptedTargets, "adopted targets"),
    ];

    for (source, label) in cases {
        assert_eq!(source.label(), label);
    }
}

#[test]
fn status_rollout_state_labels_are_structured() {
    use app_live::commands::status::model::StatusRolloutState;

    let cases = [
        (StatusRolloutState::Required, "required"),
        (StatusRolloutState::Ready, "ready"),
    ];

    for (state, label) in cases {
        assert_eq!(state.label(), label);
    }
}
static NEXT_TEMP_CONFIG_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn compatibility_mode_live_config_path() -> PathBuf {
    temp_config_fixture_path("app-live-ux-live.toml", |config| {
        let without_target_source = config.replace(
            "[strategy_control]\nsource = \"adopted\"\noperator_strategy_revision = \"targets-rev-9\"\n",
            "",
        );
        format!(
            "{without_target_source}\n[[negrisk.targets]]\nfamily_id = \"family-a\"\n\n[[negrisk.targets.members]]\ncondition_id = \"condition-1\"\ntoken_id = \"token-1\"\nprice = \"0.43\"\nquantity = \"5\"\n"
        )
    })
}

fn temp_config_fixture_path(relative: &str, edit: impl FnOnce(String) -> String) -> PathBuf {
    let source = cli::config_fixture(relative);
    let text = fs::read_to_string(&source).expect("fixture should be readable");
    let edited = edit(text);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "app-live-status-{}-{}.toml",
        std::process::id(),
        NEXT_TEMP_CONFIG_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::write(&path, edited).expect("temp fixture should be writable");
    path
}

fn strategy_only_smoke_run_session(
    run_session_id: &str,
    config_path: &Path,
    startup_strategy_revision_at_start: &str,
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
            .then(|| started_at + Duration::seconds(30)),
        exit_status: (state == RunSessionState::Exited).then(|| "success".to_owned()),
        exit_reason: (state == RunSessionState::Failed).then(|| "seeded failure".to_owned()),
        config_path: config_path.display().to_string(),
        config_fingerprint: config_fingerprint(config_path),
        target_source_kind: "adopted".to_owned(),
        startup_target_revision_at_start: startup_strategy_revision_at_start.to_owned(),
        configured_operator_target_revision: None,
        active_operator_target_revision_at_start: Some(
            startup_strategy_revision_at_start.to_owned(),
        ),
        configured_operator_strategy_revision: Some(startup_strategy_revision_at_start.to_owned()),
        active_operator_strategy_revision_at_start: Some(
            startup_strategy_revision_at_start.to_owned(),
        ),
        rollout_state_at_start: Some("required".to_owned()),
        real_user_shadow_smoke: true,
    }
}

fn seed_strategy_adoption_lineage(
    database_url: &str,
    strategy_candidate_revision: &str,
    adoptable_strategy_revision: &str,
    operator_strategy_revision: &str,
    rendered_operator_target_revision: &str,
) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime should build");
    runtime.block_on(async {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(database_url)
            .await
            .expect("test pool should connect");

        StrategyControlArtifactRepo
            .upsert_strategy_candidate_set(
                &pool,
                &StrategyCandidateSetRow {
                    strategy_candidate_revision: strategy_candidate_revision.to_owned(),
                    snapshot_id: "snapshot-strategy-12".to_owned(),
                    source_revision: "discovery-strategy-12".to_owned(),
                    payload: serde_json::json!({
                        "strategy_candidate_revision": strategy_candidate_revision,
                        "snapshot_id": "snapshot-strategy-12",
                    }),
                },
            )
            .await
            .expect("strategy candidate row should persist");

        StrategyControlArtifactRepo
            .upsert_adoptable_strategy_revision(
                &pool,
                &AdoptableStrategyRevisionRow {
                    adoptable_strategy_revision: adoptable_strategy_revision.to_owned(),
                    strategy_candidate_revision: strategy_candidate_revision.to_owned(),
                    rendered_operator_strategy_revision: operator_strategy_revision.to_owned(),
                    payload: serde_json::json!({
                        "adoptable_strategy_revision": adoptable_strategy_revision,
                        "strategy_candidate_revision": strategy_candidate_revision,
                        "rendered_operator_strategy_revision": operator_strategy_revision,
                        "rendered_operator_target_revision": rendered_operator_target_revision,
                        "rendered_live_targets": {
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
                        }
                    }),
                },
            )
            .await
            .expect("adoptable strategy row should persist");

        StrategyAdoptionRepo
            .upsert_provenance(
                &pool,
                &StrategyAdoptionProvenanceRow {
                    operator_strategy_revision: operator_strategy_revision.to_owned(),
                    adoptable_strategy_revision: adoptable_strategy_revision.to_owned(),
                    strategy_candidate_revision: strategy_candidate_revision.to_owned(),
                },
            )
            .await
            .expect("strategy provenance should persist");

        pool.close().await;
    });
}

fn seed_runtime_progress_with_strategy_anchor(
    database_url: &str,
    operator_target_revision: Option<&str>,
    operator_strategy_revision: Option<&str>,
) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime should build");
    runtime.block_on(async {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(database_url)
            .await
            .expect("test pool should connect");
        RuntimeProgressRepo
            .record_progress_with_strategy_revision(
                &pool,
                41,
                7,
                Some("snapshot-7"),
                operator_target_revision,
                operator_strategy_revision,
                None,
            )
            .await
            .expect("runtime progress should persist with strategy anchor");
        pool.close().await;
    });
}

fn config_fingerprint(config_path: &Path) -> String {
    let raw = std::fs::read(config_path).expect("config fixture should read");
    format!("{:x}", Sha256::digest(raw))
}

#[allow(dead_code)]
fn combined(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn temp_invalid_config_path() -> std::path::PathBuf {
    use std::fs;

    let mut path = std::env::temp_dir();
    path.push(format!("app-live-status-{}.toml", std::process::id()));
    fs::write(&path, "runtime = [").expect("temp fixture should be writable");
    path
}
