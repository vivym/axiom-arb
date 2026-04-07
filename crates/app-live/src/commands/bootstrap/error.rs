use std::{error::Error, fmt, path::PathBuf};

use config_schema::ConfigSchemaError;

use crate::ConfigError;

#[derive(Debug)]
pub enum BootstrapError {
    UnsupportedMode {
        config_path: PathBuf,
        mode: &'static str,
    },
    SmokeConfigCompletionOnly {
        config_path: PathBuf,
        follow_up: SmokeFollowUp,
    },
    MissingOperatorStrategyRevision {
        config_path: PathBuf,
    },
    SmokeStartRequiresRolloutReadiness {
        config_path: PathBuf,
    },
    Init(Box<dyn Error>),
    SmokeRollout(Box<dyn Error>),
    Doctor(Box<dyn Error>),
    Run(Box<dyn Error>),
    Io(std::io::Error),
    Schema(ConfigSchemaError),
    Config(ConfigError),
}

#[derive(Debug, Clone, Copy)]
pub enum SmokeFollowUp {
    NeedsAdoption,
    AlreadyAdopted,
    LegacyExplicitTargets,
}

impl fmt::Display for BootstrapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedMode { config_path, mode } => write!(
                f,
                "bootstrap currently supports only paper and real-user shadow smoke configs; {} is configured for {mode}",
                config_path.display()
            ),
            Self::SmokeConfigCompletionOnly {
                config_path,
                follow_up,
            } => {
                let quoted_config_path = shell_quote(config_path.display().to_string());
                match follow_up {
                    SmokeFollowUp::NeedsAdoption => write!(
                        f,
                        "bootstrap is waiting for explicit adoption confirmation for {}; rerun app-live bootstrap --config {} and enter one of the listed adoptable revisions",
                        config_path.display(),
                        quoted_config_path,
                    ),
                    SmokeFollowUp::AlreadyAdopted => write!(
                        f,
                        "bootstrap detected an already adopted smoke target for {}; continue with app-live bootstrap --config {}",
                        config_path.display(),
                        quoted_config_path,
                    ),
                    SmokeFollowUp::LegacyExplicitTargets => write!(
                        f,
                        "{} still uses legacy explicit targets, so move it aside or rewrite it to an adopted target source, then rerun app-live bootstrap --config {}",
                        config_path.display(),
                        quoted_config_path,
                    ),
                }
            }
            Self::MissingOperatorStrategyRevision { config_path } => write!(
                f,
                "{} is configured for adopted strategy control but missing strategy_control.operator_strategy_revision",
                config_path.display()
            ),
            Self::SmokeStartRequiresRolloutReadiness { config_path } => write!(
                f,
                "bootstrap smoke --start requires rollout readiness; {} is still preflight-only, so rerun without --start or enable rollout readiness first",
                config_path.display()
            ),
            Self::Init(error)
            | Self::SmokeRollout(error)
            | Self::Doctor(error)
            | Self::Run(error) => error.fmt(f),
            Self::Io(error) => error.fmt(f),
            Self::Schema(error) => error.fmt(f),
            Self::Config(error) => error.fmt(f),
        }
    }
}

fn shell_quote(value: String) -> String {
    let escaped = value.replace('\'', r"'\''");
    format!("'{escaped}'")
}

impl Error for BootstrapError {}

impl From<std::io::Error> for BootstrapError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ConfigSchemaError> for BootstrapError {
    fn from(value: ConfigSchemaError) -> Self {
        Self::Schema(value)
    }
}

impl From<ConfigError> for BootstrapError {
    fn from(value: ConfigError) -> Self {
        Self::Config(value)
    }
}
