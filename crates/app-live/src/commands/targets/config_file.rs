use std::{
    error::Error,
    io::Write,
    io::{Error as IoError, ErrorKind},
    path::Path,
};

use config_schema::{load_raw_config_from_path, load_raw_config_from_str, ValidatedConfig};
use tempfile::NamedTempFile;
use toml_edit::{value, DocumentMut};

pub fn rewrite_operator_target_revision(
    path: &Path,
    operator_target_revision: &str,
) -> Result<(), Box<dyn Error>> {
    let raw = load_raw_config_from_path(path)?;
    let text = std::fs::read_to_string(path)?;

    ValidatedConfig::new(raw.clone())?;
    if raw
        .negrisk
        .as_ref()
        .and_then(|negrisk| negrisk.target_source.as_ref())
        .is_none()
    {
        return Err(ConfigTargetSourceMissing.into());
    }

    let mut document = text.parse::<DocumentMut>()?;
    document["negrisk"]["target_source"]["operator_target_revision"] =
        value(operator_target_revision);
    let rewritten_text = document.to_string();

    let rewritten = ValidatedConfig::new(load_raw_config_from_str(&rewritten_text)?)?;
    rewritten.target_source()?;

    atomic_write(path, rewritten_text.as_bytes())?;
    Ok(())
}

#[derive(Debug)]
struct ConfigTargetSourceMissing;

impl std::fmt::Display for ConfigTargetSourceMissing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "config validation error: missing required section: negrisk.target_source"
        )
    }
}

impl Error for ConfigTargetSourceMissing {}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), Box<dyn Error>> {
    let parent = path.parent().ok_or_else(|| {
        IoError::new(
            ErrorKind::InvalidInput,
            "config path must have a parent directory",
        )
    })?;

    let mut temp = NamedTempFile::new_in(parent)?;
    temp.write_all(contents)?;
    temp.as_file_mut().sync_all()?;
    temp.persist(path)?;
    Ok(())
}
