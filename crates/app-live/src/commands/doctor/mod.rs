mod credentials;
mod report;
mod runtime_safety;
mod target_source;

use std::{error::Error, fmt};

use config_schema::{load_raw_config_from_path, RuntimeModeToml, ValidatedConfig};

use crate::cli::DoctorArgs;

use self::report::{DoctorCheckStatus, DoctorReport};

#[derive(Debug)]
struct DoctorFailure {
    category: &'static str,
    message: String,
}

impl DoctorFailure {
    fn new(category: &'static str, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
        }
    }
}

impl fmt::Display for DoctorFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.category, self.message)
    }
}

impl Error for DoctorFailure {}

pub fn execute(args: DoctorArgs) -> Result<(), Box<dyn Error>> {
    let mut report = DoctorReport::new();
    let result = execute_inner(&args, &mut report);
    report.render();
    result.map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn execute_inner(args: &DoctorArgs, report: &mut DoctorReport) -> Result<(), DoctorFailure> {
    let raw = load_raw_config_from_path(&args.config).map_err(|error| {
        report.push_check(
            "Config",
            DoctorCheckStatus::Fail,
            "ConfigError",
            error.to_string(),
        );
        DoctorFailure::new("ConfigError", error.to_string())
    })?;
    let validated = ValidatedConfig::new(raw).map_err(|error| {
        report.push_check(
            "Config",
            DoctorCheckStatus::Fail,
            "ConfigError",
            error.to_string(),
        );
        DoctorFailure::new("ConfigError", error.to_string())
    })?;
    let config = validated.for_app_live().map_err(|error| {
        report.push_check(
            "Config",
            DoctorCheckStatus::Fail,
            "ConfigError",
            error.to_string(),
        );
        DoctorFailure::new("ConfigError", error.to_string())
    })?;

    report.push_check("Config", DoctorCheckStatus::Pass, "config parsed", "");

    credentials::evaluate(&config, report)?;
    target_source::evaluate(&args.config, &config, report)?;
    runtime_safety::evaluate(&config, report)?;
    let connectivity_label = match config.mode() {
        RuntimeModeToml::Paper => "REST authentication not required in paper mode",
        RuntimeModeToml::Live => "control-plane checks not required for explicit targets",
    };
    report.push_check(
        "Connectivity",
        DoctorCheckStatus::Skip,
        connectivity_label,
        "",
    );

    Ok(())
}
