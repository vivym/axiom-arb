use std::path::PathBuf;

use config_schema::{load_raw_config_from_path, RuntimeModeToml, ValidatedConfig};

use crate::cli::{BootstrapArgs, DoctorArgs, InitArgs, RunArgs};
use crate::commands::{doctor, init, run};

use super::{error::BootstrapError, output};

const DEFAULT_CONFIG_PATH: &str = "config/axiom-arb.local.toml";

pub fn execute(args: BootstrapArgs) -> Result<(), BootstrapError> {
    let config_path = args
        .config
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH));

    if !config_path.exists() {
        init::execute(InitArgs {
            config: config_path.clone(),
        })
        .map_err(BootstrapError::Init)?;
    }

    if !config_path.exists() {
        return Err(BootstrapError::MissingConfigAfterInit { config_path });
    }

    ensure_paper_mode(&config_path)?;

    doctor::execute(DoctorArgs {
        config: config_path.clone(),
    })
    .map_err(BootstrapError::Doctor)?;

    if args.start {
        output::print_starting_runtime(&config_path);
        run::execute(RunArgs {
            config: config_path,
        })
        .map_err(BootstrapError::Run)?;
    } else {
        output::print_ready_summary(&config_path);
    }

    Ok(())
}

fn ensure_paper_mode(config_path: &PathBuf) -> Result<(), BootstrapError> {
    let raw = load_raw_config_from_path(config_path)?;
    let validated = ValidatedConfig::new(raw)?;
    let config = validated.for_app_live()?;

    match config.mode() {
        RuntimeModeToml::Paper => Ok(()),
        RuntimeModeToml::Live => Err(BootstrapError::UnsupportedMode {
            config_path: config_path.clone(),
            mode: "live",
        }),
    }
}
