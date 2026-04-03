use std::{error::Error, fmt};

use config_schema::{load_raw_config_from_path, ValidatedConfig};

use crate::cli::ApplyArgs;

pub mod model;

use model::{ApplyFailureKind, ApplyScenario, ApplyUnsupportedScenario};

#[derive(Debug)]
struct ApplyError {
    kind: ApplyFailureKind,
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind.unsupported_guidance())
    }
}

impl Error for ApplyError {}

pub fn execute(args: ApplyArgs) -> Result<(), Box<dyn Error>> {
    let raw = load_raw_config_from_path(&args.config)?;
    let validated = ValidatedConfig::new(raw)?;
    let config = validated.for_app_live()?;
    let scenario = ApplyScenario::from_config(&config);

    match scenario {
        ApplyScenario::Paper => Err(apply_failure(ApplyFailureKind::UnsupportedScenario(
            ApplyUnsupportedScenario::Paper,
        ))),
        ApplyScenario::Live => Err(apply_failure(ApplyFailureKind::UnsupportedScenario(
            ApplyUnsupportedScenario::Live,
        ))),
        ApplyScenario::Smoke => Err(apply_failure(ApplyFailureKind::SmokeScaffoldOnly)),
    }
}

fn apply_failure(kind: ApplyFailureKind) -> Box<dyn Error> {
    let error = ApplyError { kind };
    eprintln!("{error}");
    Box::new(error)
}
