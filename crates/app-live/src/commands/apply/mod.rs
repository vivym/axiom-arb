use std::{error::Error, fmt};

use config_schema::{load_raw_config_from_path, ValidatedConfig};

use crate::cli::ApplyArgs;
use crate::commands::status::model::StatusReadiness;
use crate::commands::status::{self, evaluate::StatusOutcome};

pub mod model;

use self::model::{ApplyFailureKind, ApplyScenario, ApplyUnsupportedScenario};

#[derive(Debug)]
struct ApplyError {
    kind: ApplyFailureKind,
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind.guidance())
    }
}

impl Error for ApplyError {}

pub fn execute(args: ApplyArgs) -> Result<(), Box<dyn Error>> {
    let status_outcome = status::evaluate::evaluate(&args.config);

    let raw = match load_raw_config_from_path(&args.config) {
        Ok(raw) => raw,
        Err(error) => return handle_config_error(status_outcome, error),
    };
    let validated = match ValidatedConfig::new(raw) {
        Ok(validated) => validated,
        Err(error) => return handle_config_error(status_outcome, error),
    };
    let config = match validated.for_app_live() {
        Ok(config) => config,
        Err(error) => return handle_config_error(status_outcome, error),
    };
    let scenario = ApplyScenario::from_config(&config);

    match scenario {
        ApplyScenario::Paper => Err(apply_failure(ApplyFailureKind::UnsupportedScenario(
            ApplyUnsupportedScenario::Paper,
        ))),
        ApplyScenario::Live => Err(apply_failure(ApplyFailureKind::UnsupportedScenario(
            ApplyUnsupportedScenario::Live,
        ))),
        ApplyScenario::Smoke => match status_outcome {
            StatusOutcome::Summary(summary) => Err(apply_failure(
                ApplyFailureKind::from_status_readiness(summary.readiness),
            )),
            StatusOutcome::Deferred(_) => Err(apply_failure(ApplyFailureKind::ReadinessError)),
        },
    }
}

fn handle_config_error(
    status_outcome: StatusOutcome,
    error: impl Error + 'static,
) -> Result<(), Box<dyn Error>> {
    match status_outcome {
        StatusOutcome::Summary(summary) if summary.readiness == StatusReadiness::Blocked => {
            Err(apply_failure(ApplyFailureKind::ReadinessError))
        }
        _ => Err(Box::new(error)),
    }
}

fn apply_failure(kind: ApplyFailureKind) -> Box<dyn Error> {
    let error = ApplyError { kind };
    eprintln!("{error}");
    Box::new(error)
}
