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
    SmokeStartUnsupported {
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
                        "bootstrap smoke follow-through is not implemented yet; {} already exists, so continue with: app-live targets candidates --config {} ; app-live targets adopt --config {} --adoptable-revision ADOPTABLE_REVISION",
                        config_path.display(),
                        quoted_config_path,
                        quoted_config_path,
                    ),
                    SmokeFollowUp::AlreadyAdopted => write!(
                        f,
                        "bootstrap smoke follow-through is not implemented yet; {} already has an adopted target anchor, so continue with: app-live targets status --config {} ; app-live targets show-current --config {}",
                        config_path.display(),
                        quoted_config_path,
                        quoted_config_path,
                    ),
                    SmokeFollowUp::LegacyExplicitTargets => write!(
                        f,
                        "bootstrap smoke follow-through is not implemented yet; {} still uses legacy explicit targets, so move it aside or rewrite it to an adopted target source, then rerun app-live bootstrap --config {} before continuing with targets candidates/adopt",
                        config_path.display(),
                        quoted_config_path,
                    ),
                }
            }
            Self::SmokeStartUnsupported { config_path } => write!(
                f,
                "bootstrap smoke does not support --start yet; complete config first, then continue with targets workflow using {}",
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
