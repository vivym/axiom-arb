use std::{
    collections::BTreeSet,
    error::Error,
    io::Write,
    io::{Error as IoError, ErrorKind},
    path::Path,
};

use config_schema::{load_raw_config_from_path, load_raw_config_from_str, ValidatedConfig};
use tempfile::NamedTempFile;
use toml_edit::{value, Array, DocumentMut};

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

pub fn rewrite_smoke_rollout_families(
    path: &Path,
    family_ids: &[String],
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

    let normalized_family_ids = normalize_family_ids(family_ids)?;

    let mut document = text.parse::<DocumentMut>()?;
    let approved = family_array(&normalized_family_ids);
    let ready = family_array(&normalized_family_ids);
    document["negrisk"]["rollout"]["approved_families"] = value(approved);
    document["negrisk"]["rollout"]["ready_families"] = value(ready);
    let rewritten_text = document.to_string();

    let rewritten_raw = load_raw_config_from_str(&rewritten_text)?;
    let _ = ValidatedConfig::new(rewritten_raw.clone())?;
    let rollout = rewritten_raw
        .negrisk
        .as_ref()
        .and_then(|negrisk| negrisk.rollout.as_ref())
        .ok_or_else(|| {
            IoError::new(
                ErrorKind::InvalidData,
                "config validation error: missing required section: negrisk.rollout",
            )
        })?;
    if rollout.approved_families != normalized_family_ids
        || rollout.ready_families != normalized_family_ids
    {
        return Err(IoError::new(
            ErrorKind::InvalidData,
            "config validation error: rewritten smoke rollout families did not persist",
        )
        .into());
    }

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

fn normalize_family_ids(family_ids: &[String]) -> Result<Vec<String>, Box<dyn Error>> {
    let mut normalized = BTreeSet::new();
    for family_id in family_ids {
        let trimmed = family_id.trim();
        if trimmed.is_empty() {
            return Err(IoError::new(
                ErrorKind::InvalidInput,
                "smoke rollout family ids must be non-empty",
            )
            .into());
        }
        normalized.insert(trimmed.to_owned());
    }

    if normalized.is_empty() {
        return Err(IoError::new(
            ErrorKind::InvalidInput,
            "smoke rollout requires at least one family id",
        )
        .into());
    }

    Ok(normalized.into_iter().collect())
}

fn family_array(family_ids: &[String]) -> Array {
    let mut array = Array::new();
    for family_id in family_ids {
        array.push(family_id.as_str());
    }
    array
}

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
