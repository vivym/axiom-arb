use std::{error::Error, fmt};

use config_schema::{load_raw_config_from_path, ValidatedConfig};

use crate::cli::ApplyArgs;

pub mod model;

use model::{ApplyFailureKind, ApplyScenario};

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
        ApplyScenario::Smoke => Ok(()),
        ApplyScenario::Paper | ApplyScenario::Live => {
            let error = ApplyError {
                kind: ApplyFailureKind::UnsupportedScenario(scenario),
            };
            eprintln!("{error}");
            Err(Box::new(error))
        }
    }
}
