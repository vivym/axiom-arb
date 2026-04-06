use std::{collections::BTreeSet, path::Path};

use config_schema::{load_raw_config_from_path, RuntimeModeToml, ValidatedConfig};
use persistence::{
    connect_pool_from_env, LatestRelevantRunSessionQuery, RunSessionProjectedRow, RunSessionRepo,
    RuntimeProgressRepo,
};
use sha2::{Digest, Sha256};
use tokio::runtime::Builder;

use super::model::{
    StatusAction, StatusDetails, StatusMode, StatusReadiness, StatusRolloutState, StatusSummary,
    StatusTargetSource,
};
use crate::commands::targets::state::{
    load_target_candidates_catalog, load_target_control_plane_state, summarize_target_candidates,
    TargetCandidatesSummary, TargetControlPlaneState,
};
use crate::startup::resolve_startup_targets;

const RUN_SESSION_STALE_AFTER: chrono::Duration = chrono::Duration::minutes(5);

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum StatusOutcome {
    Summary(Box<StatusSummary>),
    Deferred(StatusDeferred),
}

#[derive(Debug, Clone)]
pub struct StatusDeferred {
    pub mode: StatusMode,
    pub reason: String,
}

pub fn evaluate(config_path: &Path) -> StatusOutcome {
    match load_raw_config_from_path(config_path) {
        Ok(raw) => {
            if let Some(legacy_summary) = legacy_explicit_targets_summary_from_raw(&raw) {
                return StatusOutcome::Summary(Box::new(legacy_summary));
            }

            match ValidatedConfig::new(raw) {
                Ok(validated) => match validated.for_app_live() {
                    Ok(config) => match config.mode() {
                        RuntimeModeToml::Paper => StatusOutcome::Summary(Box::new(paper_summary())),
                        RuntimeModeToml::Live => {
                            let smoke_mode = config.real_user_shadow_smoke();
                            if config.has_adopted_strategy_source() {
                                match adopted_summary(config_path, &config, smoke_mode) {
                                    Ok(outcome) => outcome,
                                    Err(reason) => {
                                        StatusOutcome::Summary(Box::new(blocked_summary(reason)))
                                    }
                                }
                            } else if config.is_legacy_explicit_strategy_config() {
                                StatusOutcome::Summary(Box::new(legacy_explicit_targets_summary(
                                    smoke_mode,
                                )))
                            } else {
                                StatusOutcome::Summary(Box::new(blocked_summary(
                                    "high-level status requires an adopted target source"
                                        .to_owned(),
                                )))
                            }
                        }
                    },
                    Err(error) => {
                        StatusOutcome::Summary(Box::new(blocked_summary(error.to_string())))
                    }
                },
                Err(error) => StatusOutcome::Summary(Box::new(blocked_summary(error.to_string()))),
            }
        }
        Err(error) => StatusOutcome::Summary(Box::new(blocked_summary(error.to_string()))),
    }
}

fn paper_summary() -> StatusSummary {
    if std::env::var("DATABASE_URL").is_err() {
        return StatusSummary {
            mode: Some(StatusMode::Paper),
            readiness: StatusReadiness::Blocked,
            details: StatusDetails {
                configured_target: None,
                active_target: None,
                target_source: None,
                rollout_state: None,
                restart_needed: None,
                relevant_run_session_id: None,
                relevant_run_state: None,
                relevant_run_started_at: None,
                relevant_startup_target_revision: None,
                conflicting_active_run_session_id: None,
                conflicting_active_run_state: None,
                conflicting_active_started_at: None,
                conflicting_active_startup_target_revision: None,
                reason: Some("DATABASE_URL is required before paper run can start".to_owned()),
            },
            actions: vec![StatusAction::FixBlockingIssueAndRerunStatus],
        };
    }

    StatusSummary {
        mode: Some(StatusMode::Paper),
        readiness: StatusReadiness::PaperReady,
        details: StatusDetails {
            configured_target: None,
            active_target: None,
            target_source: None,
            rollout_state: None,
            restart_needed: None,
            relevant_run_session_id: None,
            relevant_run_state: None,
            relevant_run_started_at: None,
            relevant_startup_target_revision: None,
            conflicting_active_run_session_id: None,
            conflicting_active_run_state: None,
            conflicting_active_started_at: None,
            conflicting_active_startup_target_revision: None,
            reason: None,
        },
        actions: vec![StatusAction::RunAppLiveRun],
    }
}

fn adopted_summary(
    config_path: &Path,
    config: &config_schema::AppLiveConfigView<'_>,
    smoke_mode: bool,
) -> Result<StatusOutcome, String> {
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    let (pool, state) = runtime.block_on(async {
        let pool = connect_pool_from_env()
            .await
            .map_err(|error| error.to_string())?;
        let state = if config.target_source().is_some() {
            load_target_control_plane_state(&pool, config_path)
                .await
                .map_err(|error| error.to_string())?
        } else {
            let active_operator_target_revision = RuntimeProgressRepo
                .current(&pool)
                .await
                .map_err(|error| error.to_string())?
                .and_then(|progress| progress.operator_target_revision);
            TargetControlPlaneState {
                configured_operator_target_revision: None,
                active_operator_target_revision,
                restart_needed: None,
                provenance: None,
                latest_action: None,
            }
        };
        Ok::<_, String>((pool, state))
    })?;

    let mode = if smoke_mode {
        StatusMode::RealUserShadowSmoke
    } else {
        StatusMode::Live
    };
    let configured_revision = match state.configured_operator_target_revision.clone() {
        Some(revision) => revision,
        None if config.target_source().is_none() => match config.operator_strategy_revision() {
            Some(revision) => revision.to_owned(),
            None => {
                let catalog = runtime.block_on(async {
                    load_target_candidates_catalog(&pool)
                        .await
                        .map_err(|error| error.to_string())
                })?;
                let summary = summarize_target_candidates(&catalog);
                return Ok(StatusOutcome::Summary(Box::new(pre_adoption_summary(
                    mode, &state, &summary,
                ))));
            }
        },
        None => {
            let catalog = runtime.block_on(async {
                load_target_candidates_catalog(&pool)
                    .await
                    .map_err(|error| error.to_string())
            })?;
            let summary = summarize_target_candidates(&catalog);
            return Ok(StatusOutcome::Summary(Box::new(pre_adoption_summary(
                mode, &state, &summary,
            ))));
        }
    };
    let (configured_target_query, configured_strategy_query) = if config.target_source().is_some() {
        (Some(configured_revision.as_str()), None)
    } else {
        (None, Some(configured_revision.as_str()))
    };
    let config_path_string = config_path.display().to_string();
    let config_fingerprint = config_fingerprint(config_path).map_err(|error| error.to_string())?;
    let (resolved_targets, relevant_run_session, conflicting_active_run_session) = runtime
        .block_on(async {
            let resolved_targets = resolve_startup_targets(&pool, config)
                .await
                .map_err(|error| error.to_string())?;
            let family_ids = resolved_targets
                .targets
                .targets()
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>();
            let rollout_state = if rollout_covers_families(config, &family_ids) {
                Some("ready")
            } else {
                Some("required")
            };
            let startup_target_revision = resolved_targets
                .operator_target_revision
                .clone()
                .unwrap_or_else(|| resolved_targets.targets.revision().to_owned());
            let relevant = RunSessionRepo
                .latest_relevant(
                    &pool,
                    LatestRelevantRunSessionQuery {
                        mode: "live",
                        config_path: &config_path_string,
                        config_fingerprint: &config_fingerprint,
                        configured_target: configured_target_query,
                        configured_strategy: configured_strategy_query,
                        startup_target_revision_at_start: &startup_target_revision,
                        rollout_state,
                        stale_after: RUN_SESSION_STALE_AFTER,
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
            let relevant = match relevant {
                Some(row) => RunSessionRepo
                    .load_with_projected_state(&pool, &row.run_session_id, RUN_SESSION_STALE_AFTER)
                    .await
                    .map_err(|error| error.to_string())?,
                None => None,
            };
            let conflicting = match RuntimeProgressRepo
                .current(&pool)
                .await
                .map_err(|error| error.to_string())?
                .and_then(|progress| progress.active_run_session_id)
            {
                Some(run_session_id) => RunSessionRepo
                    .load_with_projected_state(&pool, &run_session_id, RUN_SESSION_STALE_AFTER)
                    .await
                    .map_err(|error| error.to_string())?,
                None => None,
            };

            Ok::<_, String>((resolved_targets, relevant, conflicting))
        })?;
    let family_ids = resolved_targets
        .targets
        .targets()
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();

    if family_ids.is_empty() {
        return Ok(StatusOutcome::Summary(Box::new(blocked_summary(
            "resolved adopted target set did not contain any families".to_owned(),
        ))));
    }

    let rollout_ready = rollout_covers_families(config, &family_ids);
    let family_ids_label = family_ids.iter().cloned().collect::<Vec<_>>().join(", ");
    let rollout_state = if rollout_ready {
        StatusRolloutState::Ready
    } else {
        StatusRolloutState::Required
    };
    let rollout_reason = if rollout_ready {
        format!("adopted families are covered by rollout: {family_ids_label}")
    } else {
        format!("rollout must cover adopted families: {family_ids_label}")
    };
    let active_target = state.active_operator_target_revision.clone();
    let restart_needed = active_target
        .as_deref()
        .map(|active| active != configured_revision);

    if restart_needed == Some(true) {
        let mut actions = Vec::new();
        if rollout_ready && !smoke_mode {
            actions.push(StatusAction::RunAppLiveApply);
        } else {
            if !rollout_ready {
                actions.push(if smoke_mode {
                    StatusAction::EnableSmokeRollout
                } else {
                    StatusAction::EnableLiveRollout
                });
            }
            actions.push(StatusAction::PerformControlledRestart);
        }

        let mut details = session_details(&relevant_run_session, &conflicting_active_run_session);
        details.configured_target = Some(configured_revision.clone());
        details.active_target = active_target;
        details.target_source = Some(StatusTargetSource::AdoptedTargets);
        details.rollout_state = Some(rollout_state);
        details.restart_needed = Some(true);
        details.reason = Some(format!(
            "configured and active operator_target_revision differ; {rollout_reason}"
        ));

        return Ok(StatusOutcome::Summary(Box::new(StatusSummary {
            mode: Some(mode),
            readiness: StatusReadiness::RestartRequired,
            details,
            actions,
        })));
    }

    let (readiness, actions, reason) = if rollout_ready {
        let readiness = if smoke_mode {
            StatusReadiness::SmokeConfigReady
        } else {
            StatusReadiness::LiveConfigReady
        };
        let action = if smoke_mode {
            StatusAction::RunDoctor
        } else {
            StatusAction::RunAppLiveApply
        };
        (readiness, vec![action], Some(rollout_reason))
    } else {
        let readiness = if smoke_mode {
            StatusReadiness::SmokeRolloutRequired
        } else {
            StatusReadiness::LiveRolloutRequired
        };
        let action = if smoke_mode {
            StatusAction::EnableSmokeRollout
        } else {
            StatusAction::EnableLiveRollout
        };
        (readiness, vec![action], Some(rollout_reason))
    };

    let mut details = session_details(&relevant_run_session, &conflicting_active_run_session);
    details.configured_target = Some(configured_revision);
    details.active_target = active_target;
    details.target_source = Some(StatusTargetSource::AdoptedTargets);
    details.rollout_state = Some(rollout_state);
    details.restart_needed = restart_needed;
    details.reason = reason;

    Ok(StatusOutcome::Summary(Box::new(StatusSummary {
        mode: Some(mode),
        readiness,
        details,
        actions,
    })))
}

fn blocked_summary(reason: String) -> StatusSummary {
    StatusSummary {
        mode: None,
        readiness: StatusReadiness::Blocked,
        details: StatusDetails {
            configured_target: None,
            active_target: None,
            target_source: None,
            rollout_state: None,
            restart_needed: None,
            relevant_run_session_id: None,
            relevant_run_state: None,
            relevant_run_started_at: None,
            relevant_startup_target_revision: None,
            conflicting_active_run_session_id: None,
            conflicting_active_run_state: None,
            conflicting_active_started_at: None,
            conflicting_active_startup_target_revision: None,
            reason: Some(reason),
        },
        actions: vec![StatusAction::FixBlockingIssueAndRerunStatus],
    }
}

fn legacy_explicit_targets_summary(smoke_mode: bool) -> StatusSummary {
    StatusSummary {
        mode: Some(if smoke_mode {
            StatusMode::RealUserShadowSmoke
        } else {
            StatusMode::Live
        }),
        readiness: StatusReadiness::Blocked,
        details: StatusDetails {
            configured_target: None,
            active_target: None,
            target_source: Some(StatusTargetSource::LegacyExplicitTargets),
            rollout_state: None,
            restart_needed: None,
            relevant_run_session_id: None,
            relevant_run_state: None,
            relevant_run_started_at: None,
            relevant_startup_target_revision: None,
            conflicting_active_run_session_id: None,
            conflicting_active_run_state: None,
            conflicting_active_started_at: None,
            conflicting_active_startup_target_revision: None,
            reason: Some(
                "legacy explicit targets are not supported in the high-level status flow"
                    .to_owned(),
            ),
        },
        actions: vec![StatusAction::MigrateLegacyExplicitTargets],
    }
}

fn legacy_explicit_targets_summary_from_raw(
    raw: &config_schema::RawAxiomConfig,
) -> Option<StatusSummary> {
    if raw.runtime.mode != RuntimeModeToml::Live {
        return None;
    }

    let Some(negrisk) = raw.negrisk.as_ref() else {
        return None;
    };

    if !negrisk.targets.is_present() {
        return None;
    }

    Some(legacy_explicit_targets_summary(
        raw.runtime.real_user_shadow_smoke,
    ))
}

fn pre_adoption_summary(
    mode: StatusMode,
    state: &TargetControlPlaneState,
    summary: &TargetCandidatesSummary,
) -> StatusSummary {
    let (readiness, actions, reason) = if summary.adoptable_revision_count > 0 {
        let recommendation = summary
            .recommended_adoptable_revision
            .as_deref()
            .unwrap_or("unavailable");
        (
            StatusReadiness::AdoptableReady,
            vec![StatusAction::ChooseAndAdoptRevision],
            Some(format!(
                "adoptable revisions are available; recommended_adoptable_revision = {recommendation}"
            )),
        )
    } else if summary.advisory_candidate_count > 0 {
        let reason = if summary.non_adoptable_reasons.is_empty() {
            "discovery artifacts exist but no adoptable revisions were produced".to_owned()
        } else {
            format!(
                "discovery artifacts exist but no adoptable revisions were produced; reasons: {}",
                summary.non_adoptable_reasons.join("; ")
            )
        };
        (
            StatusReadiness::DiscoveryReadyNotAdoptable,
            vec![StatusAction::InspectDiscoveryReasons],
            Some(reason),
        )
    } else {
        (
            StatusReadiness::DiscoveryRequired,
            vec![StatusAction::RunDiscover],
            Some("no persisted discovery artifacts exist yet".to_owned()),
        )
    };

    StatusSummary {
        mode: Some(mode),
        readiness,
        details: StatusDetails {
            configured_target: None,
            active_target: state.active_operator_target_revision.clone(),
            target_source: Some(StatusTargetSource::AdoptedTargets),
            rollout_state: None,
            restart_needed: state.restart_needed,
            relevant_run_session_id: None,
            relevant_run_state: None,
            relevant_run_started_at: None,
            relevant_startup_target_revision: None,
            conflicting_active_run_session_id: None,
            conflicting_active_run_state: None,
            conflicting_active_started_at: None,
            conflicting_active_startup_target_revision: None,
            reason,
        },
        actions,
    }
}

fn rollout_covers_families(
    config: &config_schema::AppLiveConfigView<'_>,
    family_ids: &BTreeSet<String>,
) -> bool {
    let Some(rollout) = config.negrisk_rollout() else {
        return false;
    };

    let approved = rollout
        .approved_families()
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let ready = rollout
        .ready_families()
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();

    family_ids
        .iter()
        .all(|family_id| approved.contains(family_id) && ready.contains(family_id))
}

fn session_details(
    relevant_run_session: &Option<RunSessionProjectedRow>,
    conflicting_active_run_session: &Option<RunSessionProjectedRow>,
) -> StatusDetails {
    let conflicting_active_run_session = match (
        relevant_run_session.as_ref(),
        conflicting_active_run_session.as_ref(),
    ) {
        (Some(relevant_session), Some(conflicting_session))
            if relevant_session.row.run_session_id == conflicting_session.row.run_session_id =>
        {
            None
        }
        _ => conflicting_active_run_session.as_ref(),
    };

    StatusDetails {
        configured_target: None,
        active_target: None,
        target_source: None,
        rollout_state: None,
        restart_needed: None,
        relevant_run_session_id: relevant_run_session
            .as_ref()
            .map(|session| session.row.run_session_id.clone()),
        relevant_run_state: relevant_run_session
            .as_ref()
            .map(|session| session.state_label.clone()),
        relevant_run_started_at: relevant_run_session
            .as_ref()
            .map(|session| session.row.started_at),
        relevant_startup_target_revision: relevant_run_session
            .as_ref()
            .map(|session| session.row.startup_target_revision_at_start.clone()),
        conflicting_active_run_session_id: conflicting_active_run_session
            .as_ref()
            .map(|session| session.row.run_session_id.clone()),
        conflicting_active_run_state: conflicting_active_run_session
            .as_ref()
            .map(|session| session.state_label.clone()),
        conflicting_active_started_at: conflicting_active_run_session
            .as_ref()
            .map(|session| session.row.started_at),
        conflicting_active_startup_target_revision: conflicting_active_run_session
            .as_ref()
            .map(|session| session.row.startup_target_revision_at_start.clone()),
        reason: None,
    }
}

fn config_fingerprint(config_path: &Path) -> Result<String, std::io::Error> {
    let raw = std::fs::read(config_path)?;
    Ok(format!("{:x}", Sha256::digest(raw)))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{legacy_explicit_targets_summary_from_raw, rollout_covers_families};
    use config_schema::{
        load_raw_config_from_path, load_raw_config_from_str, RuntimeModeToml, ValidatedConfig,
    };

    static NEXT_TEMP_CONFIG_ID: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn rollout_requires_both_approved_and_ready_lists() {
        let config_path = temp_config_fixture_path("app-live-ux-live.toml", |config| {
            format!(
                "{config}\n[negrisk.rollout]\napproved_families = [\"family-a\"]\nready_families = []\n"
            )
        });
        let result = with_validated_app_live_config(&config_path, |config| {
            rollout_covers_families(&config, &["family-a".to_owned()].into_iter().collect())
        });

        assert!(!result);

        let _ = fs::remove_file(config_path);
    }

    #[test]
    fn rollout_requires_ready_families_to_cover_the_same_adopted_set() {
        let config_path = temp_config_fixture_path("app-live-ux-live.toml", |config| {
            format!(
                "{config}\n[negrisk.rollout]\napproved_families = []\nready_families = [\"family-a\"]\n"
            )
        });
        let result = with_validated_app_live_config(&config_path, |config| {
            rollout_covers_families(&config, &["family-a".to_owned()].into_iter().collect())
        });

        assert!(!result);

        let _ = fs::remove_file(config_path);
    }

    #[test]
    fn rollout_requires_every_adopted_family_to_be_covered() {
        let config_path = temp_config_fixture_path("app-live-ux-live.toml", |config| {
            format!(
                "{config}\n[negrisk.rollout]\napproved_families = [\"family-a\"]\nready_families = [\"family-a\"]\n"
            )
        });
        let result = with_validated_app_live_config(&config_path, |config| {
            rollout_covers_families(
                &config,
                &["family-a".to_owned(), "family-b".to_owned()]
                    .into_iter()
                    .collect(),
            )
        });

        assert!(!result);

        let _ = fs::remove_file(config_path);
    }

    #[test]
    fn rollout_is_ready_only_when_all_adopted_families_are_approved_and_ready() {
        let config_path = temp_config_fixture_path("app-live-ux-live.toml", |config| {
            format!(
                "{config}\n[negrisk.rollout]\napproved_families = [\"family-a\", \"family-b\"]\nready_families = [\"family-a\", \"family-b\"]\n"
            )
        });
        let result = with_validated_app_live_config(&config_path, |config| {
            rollout_covers_families(
                &config,
                &["family-a".to_owned(), "family-b".to_owned()]
                    .into_iter()
                    .collect(),
            )
        });

        assert!(result);

        let _ = fs::remove_file(config_path);
    }

    #[test]
    fn rollout_reads_route_owned_scopes_when_legacy_rollout_is_missing() {
        let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
            config
                .replace("approved_scopes = []", "approved_scopes = [\"family-a\"]")
                .replace("ready_scopes = []", "ready_scopes = [\"family-a\"]")
        });
        let result = with_validated_app_live_config(&config_path, |config| {
            rollout_covers_families(&config, &["family-a".to_owned()].into_iter().collect())
        });

        assert!(result);

        let _ = fs::remove_file(config_path);
    }

    #[test]
    fn legacy_explicit_summary_detects_explicit_empty_targets_table() {
        let raw = load_raw_config_from_str(
            r#"
[runtime]
mode = "live"
real_user_shadow_smoke = true

[negrisk]
targets = []
"#,
        )
        .expect("config should parse");

        let summary = legacy_explicit_targets_summary_from_raw(&raw)
            .expect("explicit targets table should be treated as compatibility mode");
        assert_eq!(raw.runtime.mode, RuntimeModeToml::Live);
        assert_eq!(
            summary.details.target_source,
            Some(super::StatusTargetSource::LegacyExplicitTargets)
        );
    }

    fn with_validated_app_live_config<T>(
        config_path: &std::path::Path,
        f: impl FnOnce(config_schema::AppLiveConfigView<'_>) -> T,
    ) -> T {
        let raw = load_raw_config_from_path(config_path).expect("fixture should load");
        let validated = ValidatedConfig::new(raw).expect("fixture should validate");
        let config = validated
            .for_app_live()
            .expect("fixture should support app-live");
        f(config)
    }

    fn temp_config_fixture_path(
        relative: &str,
        edit: impl FnOnce(String) -> String,
    ) -> std::path::PathBuf {
        let source = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("config-schema")
            .join("tests")
            .join("fixtures")
            .join(relative);
        let text = fs::read_to_string(&source).expect("fixture should be readable");
        let edited = edit(text);
        let mut path = std::env::temp_dir();
        path.push(format!(
            "app-live-status-rollout-{}-{}.toml",
            std::process::id(),
            NEXT_TEMP_CONFIG_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, edited).expect("temp fixture should be writable");
        path
    }
}
