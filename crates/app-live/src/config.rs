use std::{collections::BTreeMap, fmt};

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use venue_polymarket::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegRiskLiveTargetSet {
    revision: String,
    targets: BTreeMap<String, NegRiskFamilyLiveTarget>,
}

impl NegRiskLiveTargetSet {
    pub fn empty() -> Self {
        Self::new(BTreeMap::new())
    }

    pub fn revision(&self) -> &str {
        &self.revision
    }

    pub fn targets(&self) -> &BTreeMap<String, NegRiskFamilyLiveTarget> {
        &self.targets
    }

    pub fn into_targets(self) -> BTreeMap<String, NegRiskFamilyLiveTarget> {
        self.targets
    }

    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
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
pub struct PolymarketSourceConfig {
    pub clob_host: Url,
    pub data_api_host: Url,
    pub relayer_host: Url,
    pub market_ws_url: Url,
    pub user_ws_url: Url,
    pub heartbeat_interval_seconds: u64,
    pub relayer_poll_interval_seconds: u64,
    pub metadata_refresh_interval_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    InvalidJson { value: String, message: String },
    InvalidLocalSignerConfig { value: String, message: String },
    InvalidPolymarketSourceConfig { value: String, message: String },
    DuplicateFamilyId { family_id: String },
    MissingLocalSignerConfig,
    MissingPolymarketSourceConfig,
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
            Self::InvalidPolymarketSourceConfig { message, .. } => {
                write!(f, "invalid polymarket source config: {message}")
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
            Self::MissingPolymarketSourceConfig => {
                write!(f, "missing polymarket source config for live daemon inputs")
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
    let canonical = CanonicalNegRiskLiveTargetSet {
        families: targets
            .iter()
            .map(|(family_id, family)| CanonicalNegRiskFamily {
                family_id,
                members: canonical_members(&family.members),
            })
            .collect(),
    };
    let canonical_bytes =
        serde_json::to_vec(&canonical).expect("neg-risk live target config should serialize");
    let digest = Sha256::digest(canonical_bytes);

    format!("sha256:{digest:x}")
}

#[derive(Serialize)]
struct CanonicalNegRiskLiveTargetSet<'a> {
    families: Vec<CanonicalNegRiskFamily<'a>>,
}

#[derive(Serialize)]
struct CanonicalNegRiskFamily<'a> {
    family_id: &'a str,
    members: Vec<CanonicalNegRiskMember<'a>>,
}

#[derive(Serialize)]
struct CanonicalNegRiskMember<'a> {
    condition_id: &'a str,
    token_id: &'a str,
    price: String,
    quantity: String,
}

fn canonical_members<'a>(
    members: &'a [NegRiskMemberLiveTarget],
) -> Vec<CanonicalNegRiskMember<'a>> {
    let mut canonical_members: Vec<_> = members.iter().collect();
    canonical_members.sort_by(|left, right| {
        left.condition_id
            .as_str()
            .cmp(right.condition_id.as_str())
            .then_with(|| left.token_id.as_str().cmp(right.token_id.as_str()))
            .then_with(|| normalize_decimal(&left.price).cmp(&normalize_decimal(&right.price)))
            .then_with(|| {
                normalize_decimal(&left.quantity).cmp(&normalize_decimal(&right.quantity))
            })
    });

    canonical_members
        .into_iter()
        .map(|member| CanonicalNegRiskMember {
            condition_id: member.condition_id.as_str(),
            token_id: member.token_id.as_str(),
            price: normalize_decimal(&member.price),
            quantity: normalize_decimal(&member.quantity),
        })
        .collect()
}

fn normalize_decimal(value: &Decimal) -> String {
    value.normalize().to_string()
}

pub fn load_polymarket_source_config(
    json: Option<&str>,
) -> Result<PolymarketSourceConfig, ConfigError> {
    let Some(json) = json else {
        return Err(ConfigError::MissingPolymarketSourceConfig);
    };

    let raw: RawPolymarketSourceConfig =
        serde_json::from_str(json).map_err(|error| ConfigError::InvalidPolymarketSourceConfig {
            value: json.to_owned(),
            message: error.to_string(),
        })?;

    Ok(PolymarketSourceConfig {
        clob_host: parse_source_url("clob_host", &raw.clob_host, json)?,
        data_api_host: parse_source_url("data_api_host", &raw.data_api_host, json)?,
        relayer_host: parse_source_url("relayer_host", &raw.relayer_host, json)?,
        market_ws_url: parse_source_url("market_ws_url", &raw.market_ws_url, json)?,
        user_ws_url: parse_source_url("user_ws_url", &raw.user_ws_url, json)?,
        heartbeat_interval_seconds: raw.heartbeat_interval_seconds,
        relayer_poll_interval_seconds: raw.relayer_poll_interval_seconds,
        metadata_refresh_interval_seconds: raw.metadata_refresh_interval_seconds,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct RawPolymarketSourceConfig {
    clob_host: String,
    data_api_host: String,
    relayer_host: String,
    market_ws_url: String,
    user_ws_url: String,
    heartbeat_interval_seconds: u64,
    relayer_poll_interval_seconds: u64,
    metadata_refresh_interval_seconds: u64,
}

fn parse_source_url(field: &'static str, value: &str, raw_json: &str) -> Result<Url, ConfigError> {
    Url::parse(value).map_err(|error| ConfigError::InvalidPolymarketSourceConfig {
        value: raw_json.to_owned(),
        message: format!("{field}: {error}"),
    })
}
