use std::{error::Error, fmt, path::PathBuf};

use crate::ConfigError;
use config_schema::ConfigSchemaError;

#[derive(Debug)]
pub enum BootstrapError {
    UnsupportedMode {
        config_path: PathBuf,
        mode: &'static str,
    },
    SmokeConfigCompletionOnly {
        config_path: PathBuf,
    },
    SmokeStartUnsupported {
        config_path: PathBuf,
    },
    Init(Box<dyn Error>),
    Doctor(Box<dyn Error>),
    Run(Box<dyn Error>),
    Io(std::io::Error),
    Schema(ConfigSchemaError),
    Config(ConfigError),
}

impl fmt::Display for BootstrapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedMode { config_path, mode } => write!(
                f,
                "bootstrap currently supports only paper configs for this flow; {} is configured for {mode}",
                config_path.display()
            ),
            Self::SmokeConfigCompletionOnly { config_path } => write!(
                f,
                "bootstrap smoke follow-through is not implemented yet; {} already exists, so use the targets workflow directly",
                config_path.display()
            ),
            Self::SmokeStartUnsupported { config_path } => write!(
                f,
                "bootstrap smoke does not support --start yet; complete config first, then continue with targets workflow using {}",
                config_path.display()
            ),
            Self::Init(error) | Self::Doctor(error) | Self::Run(error) => error.fmt(f),
            Self::Io(error) => error.fmt(f),
            Self::Schema(error) => error.fmt(f),
            Self::Config(error) => error.fmt(f),
        }
    }
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
