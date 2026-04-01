mod credentials;
mod report;
mod runtime_safety;
mod target_source;

use std::{error::Error, fmt};

use config_schema::{load_raw_config_from_path, RuntimeModeToml, ValidatedConfig};

use crate::cli::DoctorArgs;

use self::report::{DoctorCheckStatus, DoctorReport};

pub(crate) struct DoctorLiveContext {
    pub(crate) runtime: tokio::runtime::Runtime,
}

impl DoctorLiveContext {
    fn new(report: &mut DoctorReport) -> Result<Self, DoctorFailure> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| {
                report.push_check(
                    "Target Source",
                    DoctorCheckStatus::Fail,
                    "TargetSourceError",
                    error.to_string(),
                );
                DoctorFailure::new("TargetSourceError", error.to_string())
            })?;

        Ok(Self { runtime })
    }
}

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
    let live_context = match config.mode() {
        RuntimeModeToml::Live => Some(DoctorLiveContext::new(report)?),
        RuntimeModeToml::Paper => None,
    };
    target_source::evaluate(&args.config, &config, live_context.as_ref(), report)?;
    runtime_safety::evaluate(&config, report)?;
    evaluate_connectivity(report)?;

    Ok(())
}

fn evaluate_connectivity(report: &mut DoctorReport) -> Result<(), DoctorFailure> {
    match std::env::var("DATABASE_URL") {
        Ok(_) => {
            report.push_check(
                "Connectivity",
                DoctorCheckStatus::Pass,
                "DATABASE_URL is set",
                "",
            );
            report.push_check(
                "Connectivity",
                DoctorCheckStatus::Skip,
                "real REST/ws/heartbeat/relayer probes not implemented yet",
                "",
            );
            Ok(())
        }
        Err(_) => {
            let message = "DATABASE_URL is not set";
            report.push_check(
                "Connectivity",
                DoctorCheckStatus::Fail,
                "ConnectivityError",
                message,
            );
            Err(DoctorFailure::new("ConnectivityError", message))
        }
    }
}
