use std::{
    fs,
    path::{Path, PathBuf},
};

use config_schema::{load_raw_config_from_path, RuntimeModeToml, ValidatedConfig};

use crate::cli::{BootstrapArgs, DoctorArgs, RunArgs};
use crate::commands::{doctor, run};

use super::{error::BootstrapError, output};

const DEFAULT_CONFIG_PATH: &str = "config/axiom-arb.local.toml";
const PAPER_CONFIG: &str = "[runtime]\nmode = \"paper\"\n";

pub fn execute(args: BootstrapArgs) -> Result<(), BootstrapError> {
    let config_path = args
        .config
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH));

    if !config_path.exists() {
        write_paper_config(&config_path)?;
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

fn write_paper_config(config_path: &Path) -> Result<(), BootstrapError> {
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(config_path, PAPER_CONFIG)?;
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
