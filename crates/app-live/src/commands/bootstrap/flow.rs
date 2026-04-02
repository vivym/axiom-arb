use std::path::{Path, PathBuf};

use config_schema::{load_raw_config_from_path, RuntimeModeToml, ValidatedConfig};
use persistence::connect_pool_from_env;

use crate::cli::{BootstrapArgs, DoctorArgs};
use crate::commands::{
    doctor, init, run,
    targets::{adopt, state::load_target_candidates_catalog},
};

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

pub fn execute(args: BootstrapArgs) -> Result<(), BootstrapError> {
    let config_path = args
        .config
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH));

    if !config_path.exists() {
        match complete_missing_config(&config_path, args.start)? {
            MissingConfigOutcome::PaperWritten => {}
            MissingConfigOutcome::SmokeWritten => {
                output::print_smoke_ready_summary(&config_path);
                return Ok(());
            }
        }
    }

    match detect_existing_bootstrap_mode(&config_path)? {
        ExistingBootstrapMode::Paper => {}
        ExistingBootstrapMode::Smoke(follow_up) => match follow_up {
            SmokeFollowUp::NeedsAdoption => {
                inline_smoke_adoption(&config_path)?;
                output::print_smoke_target_anchor_summary(&config_path);
                return Ok(());
            }
            SmokeFollowUp::AlreadyAdopted | SmokeFollowUp::LegacyExplicitTargets => {
                return Err(BootstrapError::SmokeConfigCompletionOnly {
                    config_path,
                    follow_up,
                });
            }
        },
    }

    let doctor_args = DoctorArgs {
        config: config_path.clone(),
    };
    let doctor_execution = doctor::run_report(&doctor_args);
    doctor_execution.render();
    doctor_execution
        .into_result()
        .map_err(BootstrapError::Doctor)?;

    if args.start {
        output::print_starting_runtime(&config_path);
        run::run_from_config_path(&config_path).map_err(BootstrapError::Run)?;
    } else {
        output::print_ready_summary(&config_path);
    }

    Ok(())
}

fn complete_missing_config(
    config_path: &std::path::Path,
    start_requested: bool,
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
            if start_requested {
                return Err(BootstrapError::SmokeStartUnsupported {
                    config_path: config_path.to_path_buf(),
                });
            }
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

fn inline_smoke_adoption(config_path: &Path) -> Result<(), BootstrapError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| BootstrapError::Init(Box::new(error)))?;

    let (pool, catalog) = runtime
        .block_on(async {
            let pool = connect_pool_from_env().await?;
            let catalog = load_target_candidates_catalog(&pool).await?;
            Ok::<_, Box<dyn std::error::Error>>((pool, catalog))
        })
        .map_err(BootstrapError::Init)?;

    if catalog.adoptable_revisions.is_empty() {
        return Err(BootstrapError::SmokeConfigCompletionOnly {
            config_path: config_path.to_path_buf(),
            follow_up: SmokeFollowUp::NeedsAdoption,
        });
    }

    let adoptable_revisions = catalog
        .adoptable_revisions
        .iter()
        .map(|adoptable| adoptable.adoptable_revision.clone())
        .collect::<Vec<_>>();
    let mut prompt = prompt::BootstrapPrompt::new(None);
    let selected_adoptable_revision =
        prompt::choose_adoptable_revision(&mut prompt, &adoptable_revisions)
            .map_err(|error| BootstrapError::Init(Box::new(error)))?;

    runtime
        .block_on(adopt::adopt_selected_revision(
            &pool,
            config_path,
            None,
            Some(selected_adoptable_revision.as_str()),
        ))
        .map(|_| ())
        .map_err(BootstrapError::Init)
}
