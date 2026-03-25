use std::{collections::BTreeMap, fmt};

use rust_decimal::Decimal;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NegRiskMemberLiveTarget {
    pub condition_id: String,
    pub token_id: String,
    pub price: Decimal,
    pub quantity: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NegRiskFamilyLiveTarget {
    pub family_id: String,
    pub members: Vec<NegRiskMemberLiveTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    InvalidJson { value: String, message: String },
    DuplicateFamilyId { family_id: String },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson { message, .. } => {
                write!(f, "invalid neg-risk live target config: {message}")
            }
            Self::DuplicateFamilyId { family_id } => {
                write!(
                    f,
                    "duplicate neg-risk family_id in live target config: {family_id}"
                )
            }
        }
    }
}

impl std::error::Error for ConfigError {}

pub fn load_neg_risk_live_targets(
    json: Option<&str>,
) -> Result<BTreeMap<String, NegRiskFamilyLiveTarget>, ConfigError> {
    let Some(json) = json else {
        return Ok(BTreeMap::new());
    };

    let families: Vec<NegRiskFamilyLiveTarget> =
        serde_json::from_str(json).map_err(|error| ConfigError::InvalidJson {
            value: json.to_owned(),
            message: error.to_string(),
        })?;

    let mut targets = BTreeMap::new();
    for family in families {
        let family_id = family.family_id.clone();
        if targets.insert(family_id.clone(), family).is_some() {
            return Err(ConfigError::DuplicateFamilyId { family_id });
        }
    }

    Ok(targets)
}
