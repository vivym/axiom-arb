mod error;
mod raw;

pub use error::ConfigSchemaError;
pub use raw::{RawAxiomConfig, RuntimeModeToml, RuntimeToml};

pub fn load_raw_config_from_path(
    path: &std::path::Path,
) -> Result<RawAxiomConfig, ConfigSchemaError> {
    let text = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&text)?)
}
