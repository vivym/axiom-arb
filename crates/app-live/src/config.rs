use std::{collections::BTreeMap, fmt};

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegRiskLiveTargetSet {
    pub revision: String,
    pub targets: BTreeMap<String, NegRiskFamilyLiveTarget>,
}

impl NegRiskLiveTargetSet {
    pub fn empty() -> Self {
        Self::new(BTreeMap::new())
    }

    fn new(targets: BTreeMap<String, NegRiskFamilyLiveTarget>) -> Self {
        Self {
            revision: stable_neg_risk_live_target_revision(&targets),
            targets,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct NegRiskMemberLiveTarget {
    pub condition_id: String,
    pub token_id: String,
    pub price: Decimal,
    pub quantity: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct NegRiskFamilyLiveTarget {
    pub family_id: String,
    pub members: Vec<NegRiskMemberLiveTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LocalSignerConfig {
    pub signer: LocalSignerIdentity,
    pub l2_auth: LocalL2AuthHeaders,
    pub relayer_auth: LocalRelayerAuth,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LocalSignerIdentity {
    pub address: String,
    pub funder_address: String,
    pub signature_type: String,
    pub wallet_route: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LocalL2AuthHeaders {
    pub api_key: String,
    pub passphrase: String,
    pub timestamp: String,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LocalRelayerAuth {
    BuilderApiKey {
        api_key: String,
        timestamp: String,
        passphrase: String,
        signature: String,
    },
    RelayerApiKey {
        api_key: String,
        address: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    InvalidJson { value: String, message: String },
    InvalidLocalSignerConfig { value: String, message: String },
    DuplicateFamilyId { family_id: String },
    MissingLocalSignerConfig,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson { message, .. } => {
                write!(f, "invalid neg-risk live target config: {message}")
            }
            Self::InvalidLocalSignerConfig { message, .. } => {
                write!(f, "invalid local signer config: {message}")
            }
            Self::DuplicateFamilyId { family_id } => {
                write!(
                    f,
                    "duplicate neg-risk family_id in live target config: {family_id}"
                )
            }
            Self::MissingLocalSignerConfig => {
                write!(
                    f,
                    "missing local signer config for live neg-risk operator inputs"
                )
            }
        }
    }
}

impl std::error::Error for ConfigError {}

pub fn load_neg_risk_live_targets(json: Option<&str>) -> Result<NegRiskLiveTargetSet, ConfigError> {
    let Some(json) = json else {
        return Ok(NegRiskLiveTargetSet::empty());
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

    Ok(NegRiskLiveTargetSet::new(targets))
}

pub fn load_local_signer_config(json: Option<&str>) -> Result<LocalSignerConfig, ConfigError> {
    let Some(json) = json else {
        return Err(ConfigError::MissingLocalSignerConfig);
    };

    serde_json::from_str(json).map_err(|error| ConfigError::InvalidLocalSignerConfig {
        value: json.to_owned(),
        message: error.to_string(),
    })
}

fn stable_neg_risk_live_target_revision(
    targets: &BTreeMap<String, NegRiskFamilyLiveTarget>,
) -> String {
    serde_json::to_string(targets)
        .expect("neg-risk live target config should serialize into a stable revision")
}
