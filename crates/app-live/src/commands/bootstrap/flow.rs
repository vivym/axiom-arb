use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use config_schema::{load_raw_config_from_path, RuntimeModeToml, ValidatedConfig};
use persistence::connect_pool_from_env;

use crate::cli::{BootstrapArgs, DoctorArgs};
use crate::commands::{
    discover, doctor, init, run,
    targets::{
        adopt,
        state::{load_target_candidates_catalog, summarize_target_candidates},
    },
};
use crate::startup;

use super::{
    error::{BootstrapError, SmokeFollowUp},
    output, prompt,
};

const DEFAULT_CONFIG_PATH: &str = "config/axiom-arb.local.toml";

enum ExistingBootstrapMode {
    Paper,
    Smoke(SmokeFollowUp),
}

enum MissingConfigOutcome {
    PaperWritten,
    SmokeWritten,
}

enum SmokeAdoptionOutcome {
    AwaitingConfirmation,
    Adopted,
}

#[derive(Clone, Copy)]
pub(super) enum DiscoveryArtifactsSource {
    FreshDiscover,
    Persisted,
}

enum SmokeBootstrapState {
    PreflightOnly { family_ids: Vec<String> },
    ShadowWorkReady { family_ids: Vec<String> },
}

pub fn execute(args: BootstrapArgs) -> Result<(), BootstrapError> {
    let config_path = args
        .config
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH));
    let mut smoke_mode = false;

    if !config_path.exists() {
        match complete_missing_config(&config_path, args.start)? {
            MissingConfigOutcome::PaperWritten => {}
            MissingConfigOutcome::SmokeWritten => {}
        }
    }

    match detect_existing_bootstrap_mode(&config_path)? {
        ExistingBootstrapMode::Paper => {}
        ExistingBootstrapMode::Smoke(follow_up) => match follow_up {
            SmokeFollowUp::NeedsAdoption => match inline_smoke_adoption(&config_path)? {
                SmokeAdoptionOutcome::AwaitingConfirmation => return Ok(()),
                SmokeAdoptionOutcome::Adopted => {
                    smoke_mode = true;
                }
            },
            SmokeFollowUp::AlreadyAdopted => {
                smoke_mode = true;
            }
            SmokeFollowUp::LegacyExplicitTargets => {
                return Err(BootstrapError::SmokeConfigCompletionOnly {
                    config_path,
                    follow_up,
                });
            }
        },
    };

    let doctor_args = DoctorArgs {
        config: config_path.clone(),
    };
    let doctor_execution = doctor::run_report(&doctor_args);
    doctor_execution.render();
    doctor_execution
        .into_result()
        .map_err(BootstrapError::Doctor)?;

    let smoke_state = if smoke_mode {
        Some(ensure_smoke_rollout_state(&config_path, args.start)?)
    } else {
        None
    };

    if args.start {
        if smoke_state.is_some() {
            output::print_starting_smoke_runtime(&config_path);
        } else {
            output::print_starting_runtime(&config_path);
        }
        run::run_from_config_path_with_invoked_by(&config_path, "bootstrap")
            .map_err(BootstrapError::Run)?;
    } else {
        match smoke_state {
            Some(SmokeBootstrapState::PreflightOnly { family_ids }) => {
                output::print_smoke_preflight_only_summary(&config_path, &family_ids);
            }
            Some(SmokeBootstrapState::ShadowWorkReady { family_ids }) => {
                output::print_smoke_rollout_ready_summary(&config_path, &family_ids);
            }
            None => output::print_ready_summary(&config_path),
        }
    }

    Ok(())
}

fn complete_missing_config(
    config_path: &std::path::Path,
    _start_requested: bool,
) -> Result<MissingConfigOutcome, BootstrapError> {
    let mut prompt = prompt::BootstrapPrompt::new(None);
    let selection = if prompt::stdin_is_terminal() {
        prompt::choose_bootstrap_mode(&mut prompt, prompt::BootstrapModeInput::Terminal)
            .map_err(|error| BootstrapError::Init(Box::new(error)))?
    } else {
        let first_line = prompt::read_piped_first_line()
            .map_err(|error| BootstrapError::Init(Box::new(error)))?;
        prompt::choose_bootstrap_mode(&mut prompt, prompt::BootstrapModeInput::Piped(first_line))
            .map_err(|error| BootstrapError::Init(Box::new(error)))?
    };

    let wizard = match selection {
        prompt::BootstrapModeSelection::Paper => init::paper_wizard_result(config_path)
            .map_err(|error| BootstrapError::Init(Box::new(error)))?,
        prompt::BootstrapModeSelection::Smoke => {
            let mut prompt = prompt::BootstrapPrompt::new(None);
            init::smoke_wizard_with_prompt(&mut prompt, config_path)
                .map_err(|error| BootstrapError::Init(Box::new(error)))?
        }
    };
    init::validate_and_write_rendered_config(config_path, &wizard.rendered_config)
        .map_err(|error| BootstrapError::Init(Box::new(error)))?;
    Ok(match selection {
        prompt::BootstrapModeSelection::Paper => MissingConfigOutcome::PaperWritten,
        prompt::BootstrapModeSelection::Smoke => MissingConfigOutcome::SmokeWritten,
    })
}

fn detect_existing_bootstrap_mode(
    config_path: &Path,
) -> Result<ExistingBootstrapMode, BootstrapError> {
    let raw = load_raw_config_from_path(config_path)?;

    if raw.runtime.mode == RuntimeModeToml::Live
        && raw.runtime.real_user_shadow_smoke
        && raw
            .negrisk
            .as_ref()
            .and_then(|negrisk| negrisk.target_source.as_ref())
            .is_none()
        && raw
            .negrisk
            .as_ref()
            .is_some_and(|negrisk| !negrisk.targets.is_empty())
    {
        return Ok(ExistingBootstrapMode::Smoke(
            SmokeFollowUp::LegacyExplicitTargets,
        ));
    }

    let validated = ValidatedConfig::new(raw.clone())?;
    let config = validated.for_app_live()?;

    match config.mode() {
        RuntimeModeToml::Paper => Ok(ExistingBootstrapMode::Paper),
        RuntimeModeToml::Live if config.real_user_shadow_smoke() => {
            let follow_up = match raw
                .negrisk
                .as_ref()
                .and_then(|negrisk| negrisk.target_source.as_ref())
            {
                Some(target_source) if target_source.operator_target_revision.is_some() => {
                    SmokeFollowUp::AlreadyAdopted
                }
                Some(_) => SmokeFollowUp::NeedsAdoption,
                None => SmokeFollowUp::LegacyExplicitTargets,
            };
            Ok(ExistingBootstrapMode::Smoke(follow_up))
        }
        RuntimeModeToml::Live => Err(BootstrapError::UnsupportedMode {
            config_path: config_path.to_path_buf(),
            mode: "live",
        }),
    }
}

fn inline_smoke_adoption(config_path: &Path) -> Result<SmokeAdoptionOutcome, BootstrapError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| BootstrapError::Init(Box::new(error)))?;

    let (pool, catalog, artifacts_source) = runtime
        .block_on(async {
            let pool = connect_pool_from_env().await?;
            let mut catalog = load_target_candidates_catalog(&pool).await?;
            let mut artifacts_source = DiscoveryArtifactsSource::Persisted;
            if catalog.advisory_candidates.is_empty() && catalog.adoptable_revisions.is_empty() {
                let _ = discover::run_discover_from_config(config_path).await?;
                catalog = load_target_candidates_catalog(&pool).await?;
                artifacts_source = DiscoveryArtifactsSource::FreshDiscover;
            }
            Ok::<_, Box<dyn std::error::Error>>((pool, catalog, artifacts_source))
        })
        .map_err(BootstrapError::Init)?;

    let summary = summarize_target_candidates(&catalog);

    if catalog.adoptable_revisions.is_empty() {
        output::print_smoke_discovery_ready_not_adoptable(
            artifacts_source,
            config_path,
            &summary.non_adoptable_reasons,
        );
        return Ok(SmokeAdoptionOutcome::AwaitingConfirmation);
    }

    let adoptable_revisions = catalog
        .adoptable_revisions
        .iter()
        .map(|adoptable| adoptable.adoptable_revision.clone())
        .collect::<Vec<_>>();
    output::print_smoke_discovery_completed(
        artifacts_source,
        &adoptable_revisions,
        summary.recommended_adoptable_revision.as_deref(),
    );

    let mut prompt = prompt::BootstrapPrompt::new(None);
    let selected_adoptable_revision = if prompt::stdin_is_terminal() {
        prompt::maybe_choose_adoptable_revision(
            &mut prompt,
            prompt::AdoptableRevisionInput::Terminal,
            &adoptable_revisions,
        )
    } else {
        let first_line = prompt::read_piped_first_line()
            .map_err(|error| BootstrapError::Init(Box::new(error)))?;
        prompt::maybe_choose_adoptable_revision(
            &mut prompt,
            prompt::AdoptableRevisionInput::Piped(first_line),
            &adoptable_revisions,
        )
    }
    .map_err(|error| BootstrapError::Init(Box::new(error)))?;

    let Some(selected_adoptable_revision) = selected_adoptable_revision else {
        output::print_waiting_for_explicit_adoption_confirmation(config_path);
        return Ok(SmokeAdoptionOutcome::AwaitingConfirmation);
    };

    runtime
        .block_on(adopt::adopt_selected_revision(
            &pool,
            config_path,
            None,
            Some(selected_adoptable_revision.as_str()),
        ))
        .map_err(BootstrapError::Init)?;

    Ok(SmokeAdoptionOutcome::Adopted)
}

fn ensure_smoke_rollout_state(
    config_path: &Path,
    start_requested: bool,
) -> Result<SmokeBootstrapState, BootstrapError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| BootstrapError::SmokeRollout(Box::new(error)))?;

    let family_ids = runtime
        .block_on(adopted_smoke_family_ids(config_path))
        .map_err(BootstrapError::SmokeRollout)?;

    if smoke_rollout_is_ready(config_path, &family_ids)? {
        return Ok(SmokeBootstrapState::ShadowWorkReady { family_ids });
    }

    let mut prompt = prompt::BootstrapPrompt::new(None);
    let selection = prompt::choose_smoke_rollout_selection(&mut prompt, &family_ids)
        .map_err(|error| BootstrapError::SmokeRollout(Box::new(error)))?;

    match selection {
        prompt::SmokeRolloutSelection::PreflightOnly => {
            if start_requested {
                Err(BootstrapError::SmokeStartRequiresRolloutReadiness {
                    config_path: config_path.to_path_buf(),
                })
            } else {
                Ok(SmokeBootstrapState::PreflightOnly { family_ids })
            }
        }
        prompt::SmokeRolloutSelection::Enable => {
            crate::commands::targets::config_file::rewrite_smoke_rollout_families(
                config_path,
                &family_ids,
            )
            .map_err(BootstrapError::SmokeRollout)?;
            Ok(SmokeBootstrapState::ShadowWorkReady { family_ids })
        }
    }
}

async fn adopted_smoke_family_ids(
    config_path: &Path,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let raw = load_raw_config_from_path(config_path)?;
    let validated = ValidatedConfig::new(raw)?;
    let config = validated.for_app_live()?;
    let pool = connect_pool_from_env().await?;
    let resolved = startup::resolve_startup_targets(&pool, &config).await?;
    let family_ids = resolved
        .targets
        .targets()
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if family_ids.is_empty() {
        return Err("bootstrap could not derive any adopted smoke families".into());
    }
    Ok(family_ids)
}

fn smoke_rollout_is_ready(
    config_path: &Path,
    family_ids: &[String],
) -> Result<bool, BootstrapError> {
    let raw = load_raw_config_from_path(config_path)?;
    let validated = ValidatedConfig::new(raw)?;
    let config = validated.for_app_live()?;
    let approved = config
        .negrisk_rollout()
        .map(|rollout| {
            rollout
                .approved_families()
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let ready = config
        .negrisk_rollout()
        .map(|rollout| {
            rollout
                .ready_families()
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();

    Ok(family_ids
        .iter()
        .all(|family_id| approved.contains(family_id) && ready.contains(family_id)))
}
