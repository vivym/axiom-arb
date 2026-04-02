use std::path::PathBuf;

use config_schema::{load_raw_config_from_path, RuntimeModeToml, ValidatedConfig};

use crate::cli::{BootstrapArgs, DoctorArgs};
use crate::commands::{doctor, init, run};

use super::{error::BootstrapError, output, prompt};

const DEFAULT_CONFIG_PATH: &str = "config/axiom-arb.local.toml";

enum ExistingBootstrapMode {
    Paper,
    Smoke,
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
        ExistingBootstrapMode::Smoke => {
            return Err(BootstrapError::SmokeConfigCompletionOnly { config_path });
        }
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
    let selection = if prompt::stdin_is_terminal() {
        let mut prompt = prompt::BootstrapPrompt::new(None);
        prompt::select_bootstrap_mode(&mut prompt)
            .map_err(|error| BootstrapError::Init(Box::new(error)))?
    } else {
        match prompt::read_piped_first_line()
            .map_err(|error| BootstrapError::Init(Box::new(error)))?
        {
            Some(first_line) => {
                let mut prompt = prompt::BootstrapPrompt::new(Some(first_line));
                prompt::select_bootstrap_mode(&mut prompt)
                    .map_err(|error| BootstrapError::Init(Box::new(error)))?
            }
            None => prompt::BootstrapModeSelection::Paper,
        }
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
    config_path: &PathBuf,
) -> Result<ExistingBootstrapMode, BootstrapError> {
    let raw = load_raw_config_from_path(config_path)?;
    let validated = ValidatedConfig::new(raw)?;
    let config = validated.for_app_live()?;

    match config.mode() {
        RuntimeModeToml::Paper => Ok(ExistingBootstrapMode::Paper),
        RuntimeModeToml::Live if config.real_user_shadow_smoke() => {
            Ok(ExistingBootstrapMode::Smoke)
        }
        RuntimeModeToml::Live => Err(BootstrapError::UnsupportedMode {
            config_path: config_path.clone(),
            mode: "live",
        }),
    }
}
