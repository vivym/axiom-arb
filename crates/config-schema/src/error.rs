use std::{fmt, io};

#[derive(Debug)]
pub enum ConfigSchemaError {
    Io(io::Error),
    Toml(toml::de::Error),
}

impl fmt::Display for ConfigSchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::Toml(err) => write!(f, "toml parse error: {err}"),
        }
    }
}

impl std::error::Error for ConfigSchemaError {}

impl From<io::Error> for ConfigSchemaError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<toml::de::Error> for ConfigSchemaError {
    fn from(value: toml::de::Error) -> Self {
        Self::Toml(value)
    }
}
