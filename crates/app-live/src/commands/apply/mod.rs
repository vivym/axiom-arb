use std::{error::Error, fmt};

use config_schema::{load_raw_config_from_path, ValidatedConfig};

use crate::cli::ApplyArgs;
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
    let raw = match load_raw_config_from_path(&args.config) {
        Ok(raw) => raw,
        Err(error) => {
            return Err(apply_failure(ApplyFailureKind::ReadinessError(
                error.to_string(),
            )))
        }
    };
    let validated = match ValidatedConfig::new(raw) {
        Ok(validated) => validated,
        Err(error) => {
            return Err(apply_failure(ApplyFailureKind::ReadinessError(
                error.to_string(),
            )))
        }
    };
    let config = match validated.for_app_live() {
        Ok(config) => config,
        Err(error) => {
            return Err(apply_failure(ApplyFailureKind::ReadinessError(
                error.to_string(),
            )))
        }
    };
    let scenario = ApplyScenario::from_config(&config);

    match scenario {
        ApplyScenario::Paper => Err(apply_failure(ApplyFailureKind::UnsupportedScenario(
            ApplyUnsupportedScenario::Paper,
        ))),
        ApplyScenario::Live => Err(apply_failure(ApplyFailureKind::UnsupportedScenario(
            ApplyUnsupportedScenario::Live,
        ))),
        ApplyScenario::Smoke => match status::evaluate::evaluate(&args.config) {
            StatusOutcome::Summary(summary) => {
                Err(apply_failure(ApplyFailureKind::from_status_readiness(
                    summary.readiness,
                    summary
                        .details
                        .reason
                        .unwrap_or_else(|| "blocked".to_owned()),
                )))
            }
            StatusOutcome::Deferred(deferred) => Err(apply_failure(
                ApplyFailureKind::ReadinessError(deferred.reason),
            )),
        },
    }
}

fn apply_failure(kind: ApplyFailureKind) -> Box<dyn Error> {
    let error = ApplyError { kind };
    eprintln!("{error}");
    Box::new(error)
}
