use std::path::PathBuf;

use config_schema::{load_raw_config_from_path, RuntimeModeToml, ValidatedConfig};

use crate::cli::{BootstrapArgs, DoctorArgs};
use crate::commands::{doctor, init, run};

use super::{error::BootstrapError, output, prompt};

const DEFAULT_CONFIG_PATH: &str = "config/axiom-arb.local.toml";

enum BootstrapMode {
    Paper,
    Smoke,
}

pub fn execute(args: BootstrapArgs) -> Result<(), BootstrapError> {
    let config_path = args
        .config
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH));

    if !config_path.exists() {
        write_paper_config(&config_path)?;
    }

    match detect_bootstrap_mode(&config_path)? {
        BootstrapMode::Paper => {}
        BootstrapMode::Smoke => {
            output::print_smoke_ready_summary(&config_path);
            return Ok(());
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

fn write_paper_config(config_path: &std::path::Path) -> Result<(), BootstrapError> {
    let wizard = match prompt::read_first_stdin_line()
        .map_err(|error| BootstrapError::Init(Box::new(error)))?
    {
        Some(first_line) => {
            let mut prompt = prompt::BootstrapPrompt::from_buffered_line(first_line);
            init::run_wizard_with_prompt(&mut prompt, config_path)
                .map_err(|error| BootstrapError::Init(Box::new(error)))?
        }
        None => init::paper_wizard_result(config_path)
            .map_err(|error| BootstrapError::Init(Box::new(error)))?,
    };
    init::validate_and_write_rendered_config(config_path, &wizard.rendered_config)
        .map_err(|error| BootstrapError::Init(Box::new(error)))?;
    Ok(())
}

fn detect_bootstrap_mode(config_path: &PathBuf) -> Result<BootstrapMode, BootstrapError> {
    let raw = load_raw_config_from_path(config_path)?;
    let validated = ValidatedConfig::new(raw)?;
    let config = validated.for_app_live()?;

    match config.mode() {
        RuntimeModeToml::Paper => Ok(BootstrapMode::Paper),
        RuntimeModeToml::Live if config.real_user_shadow_smoke() => Ok(BootstrapMode::Smoke),
        RuntimeModeToml::Live => Err(BootstrapError::UnsupportedMode {
            config_path: config_path.clone(),
            mode: "live",
        }),
    }
}
