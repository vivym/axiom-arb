use std::{
    collections::BTreeSet,
    error::Error,
    io::Write,
    io::{Error as IoError, ErrorKind},
    path::Path,
};

use config_schema::{load_raw_config_from_path, load_raw_config_from_str, ValidatedConfig};
use tempfile::NamedTempFile;
use toml_edit::{table, value, Array, DocumentMut, Item};

pub fn rewrite_operator_strategy_revision(
    path: &Path,
    operator_strategy_revision: &str,
) -> Result<(), Box<dyn Error>> {
    let raw = load_raw_config_from_path(path)?;
    let text = std::fs::read_to_string(path)?;

    let _ = ValidatedConfig::new(raw.clone())?;

    let mut document = text.parse::<DocumentMut>()?;
    {
        let root = document.as_table_mut();
        if root.get("strategy_control").is_none() {
            root.insert("strategy_control", table());
        }
        let strategy_control = root
            .get_mut("strategy_control")
            .expect("document should contain [strategy_control]")
            .as_table_like_mut()
            .expect("[strategy_control] should be a table");
        strategy_control.insert("source", value("adopted"));
        strategy_control.insert(
            "operator_strategy_revision",
            value(operator_strategy_revision),
        );
    }

    if let Some(negrisk) = document["negrisk"].as_table_like_mut() {
        let rollout = negrisk.remove("rollout");
        negrisk.remove("target_source");
        negrisk.remove("targets");

        if let Some(rollout) = rollout {
            let root = document.as_table_mut();
            if root.get("strategies").is_none() {
                root.insert("strategies", table());
            }
            let strategies = root
                .get_mut("strategies")
                .expect("document should contain [strategies]")
                .as_table_like_mut()
                .expect("[strategies] should be a table");
            if strategies.get("neg_risk").is_none() {
                strategies.insert("neg_risk", table());
            }
            let neg_risk = strategies
                .get_mut("neg_risk")
                .expect("[strategies.neg_risk] should exist")
                .as_table_like_mut()
                .expect("[strategies.neg_risk] should be a table");
            neg_risk.insert("rollout", normalize_route_owned_rollout_item(rollout));
        }
    }

    let rewritten_text = document.to_string();
    let rewritten_raw = load_raw_config_from_str(&rewritten_text)?;
    let _ = ValidatedConfig::new(rewritten_raw.clone())?;
    let persisted_revision = rewritten_raw
        .strategy_control
        .as_ref()
        .and_then(|strategy_control| strategy_control.operator_strategy_revision.as_deref())
        .or_else(|| {
            rewritten_raw
                .negrisk
                .as_ref()
                .and_then(|negrisk| negrisk.target_source.as_ref())
                .and_then(|target_source| target_source.operator_target_revision.as_deref())
        });
    if persisted_revision != Some(operator_strategy_revision) {
        return Err(IoError::new(
            ErrorKind::InvalidData,
            "config validation error: rewritten operator_strategy_revision did not persist",
        )
        .into());
    }

    atomic_write(path, rewritten_text.as_bytes())?;
    Ok(())
}

fn normalize_route_owned_rollout_item(mut rollout: Item) -> Item {
    if let Some(rollout_table) = rollout.as_table_like_mut() {
        if let Some(approved_families) = rollout_table.remove("approved_families") {
            rollout_table.insert("approved_scopes", approved_families);
        }
        if let Some(ready_families) = rollout_table.remove("ready_families") {
            rollout_table.insert("ready_scopes", ready_families);
        }
    }

    rollout
}

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
    let normalized_family_ids = normalize_family_ids(family_ids)?;
    let use_route_owned_rollout = raw.strategy_control.is_some()
        || raw
            .strategies
            .as_ref()
            .and_then(|strategies| strategies.neg_risk.as_ref())
            .and_then(|neg_risk| neg_risk.rollout.as_ref())
            .is_some();

    let mut document = text.parse::<DocumentMut>()?;
    let approved = family_array(&normalized_family_ids);
    let ready = family_array(&normalized_family_ids);
    if use_route_owned_rollout {
        let root = document.as_table_mut();
        if root.get("strategies").is_none() {
            root.insert("strategies", table());
        }
        let strategies = root
            .get_mut("strategies")
            .expect("document should contain [strategies]")
            .as_table_like_mut()
            .expect("[strategies] should be a table");
        if strategies.get("neg_risk").is_none() {
            strategies.insert("neg_risk", table());
        }
        let neg_risk = strategies
            .get_mut("neg_risk")
            .expect("[strategies.neg_risk] should exist")
            .as_table_like_mut()
            .expect("[strategies.neg_risk] should be a table");
        if neg_risk.get("rollout").is_none() {
            neg_risk.insert("rollout", table());
        }
        let rollout = neg_risk
            .get_mut("rollout")
            .expect("[strategies.neg_risk.rollout] should exist")
            .as_table_like_mut()
            .expect("[strategies.neg_risk.rollout] should be a table");
        rollout.insert("approved_scopes", value(approved));
        rollout.insert("ready_scopes", value(ready));
    } else {
        if raw
            .negrisk
            .as_ref()
            .and_then(|negrisk| negrisk.target_source.as_ref())
            .is_none()
        {
            return Err(ConfigTargetSourceMissing.into());
        }
        document["negrisk"]["rollout"]["approved_families"] = value(approved);
        document["negrisk"]["rollout"]["ready_families"] = value(ready);
    }
    let rewritten_text = document.to_string();

    let rewritten_raw = load_raw_config_from_str(&rewritten_text)?;
    let _ = ValidatedConfig::new(rewritten_raw.clone())?;
    let persisted = if use_route_owned_rollout {
        rewritten_raw
            .strategies
            .as_ref()
            .and_then(|strategies| strategies.neg_risk.as_ref())
            .and_then(|neg_risk| neg_risk.rollout.as_ref())
            .map(|rollout| {
                (
                    rollout.approved_scopes.as_slice(),
                    rollout.ready_scopes.as_slice(),
                )
            })
            .ok_or_else(|| {
                IoError::new(
                    ErrorKind::InvalidData,
                    "config validation error: missing required section: strategies.neg_risk.rollout",
                )
            })?
    } else {
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
        (
            rollout.approved_families.as_slice(),
            rollout.ready_families.as_slice(),
        )
    };
    if persisted.0 != normalized_family_ids.as_slice() || persisted.1 != normalized_family_ids.as_slice() {
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
