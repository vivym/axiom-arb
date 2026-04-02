mod connectivity;
mod credentials;
mod report;
mod runtime_safety;
mod target_source;

use std::{error::Error, fmt, path::Path};

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

pub(crate) struct DoctorExecution {
    report: DoctorReport,
    next_actions: Vec<String>,
    failure: Option<DoctorFailure>,
}

impl DoctorExecution {
    pub(crate) fn render(&self) {
        self.report.render(&self.next_actions);
    }

    pub(crate) fn into_result(self) -> Result<(), Box<dyn Error>> {
        match self.failure {
            Some(error) => Err(Box::new(error)),
            None => Ok(()),
        }
    }
}

pub fn execute(args: DoctorArgs) -> Result<(), Box<dyn Error>> {
    let execution = run_report(&args);
    execution.render();
    execution.into_result()
}

pub(crate) fn run_report(args: &DoctorArgs) -> DoctorExecution {
    let mut report = DoctorReport::new();
    let failure = execute_inner(args, &mut report).err();
    let next_actions = next_actions(&report, &args.config);
    DoctorExecution {
        report,
        next_actions,
        failure,
    }
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
    let resolved_targets =
        target_source::evaluate(&args.config, &config, live_context.as_ref(), report)?;
    runtime_safety::evaluate(&config, report)?;
    connectivity::evaluate(
        &config,
        live_context.as_ref(),
        resolved_targets.as_ref(),
        report,
    )?;

    Ok(())
}

fn next_actions(report: &DoctorReport, config_path: &Path) -> Vec<String> {
    let quoted_config_path = shell_quote(config_path.display().to_string());

    if report.section_failed("Target Source") {
        return vec![
            format!(
                "app-live targets candidates --config {quoted_config_path}"
            ),
            format!(
                "app-live targets adopt --config {quoted_config_path} --adoptable-revision <revision>"
            ),
        ];
    }

    if report.section_failed("Config")
        || report.section_failed("Credentials")
        || report.section_failed("Connectivity")
        || report.section_failed("Runtime Safety")
    {
        return vec!["fix the reported issue and rerun doctor".to_owned()];
    }

    vec![format!("app-live run --config {quoted_config_path}")]
}

fn shell_quote(value: String) -> String {
    let escaped = value.replace('\'', r"'\''");
    format!("'{escaped}'")
}
