use std::{collections::BTreeMap, fmt, str::FromStr};

use config_schema::{
    AppLiveConfigView, PolymarketRelayerAuthToml, PolymarketSourceToml, RelayerAuthKindToml,
    SignatureTypeToml, WalletRouteToml,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use venue_polymarket::PolymarketUrl as Url;

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
            revision: neg_risk_live_target_revision_from_targets(&targets),
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

impl TryFrom<&AppLiveConfigView<'_>> for NegRiskLiveTargetSet {
    type Error = ConfigError;

    fn try_from(config: &AppLiveConfigView<'_>) -> Result<Self, Self::Error> {
        let mut targets = BTreeMap::new();
        for family in config.negrisk_targets() {
            let members = family
                .members
                .iter()
                .map(|member| {
                    Ok(NegRiskMemberLiveTarget {
                        condition_id: member.condition_id.clone(),
                        token_id: member.token_id.clone(),
                        price: parse_decimal(
                            "negrisk.targets.members.price",
                            &member.price,
                            "app_live",
                        )?,
                        quantity: parse_decimal(
                            "negrisk.targets.members.quantity",
                            &member.quantity,
                            "app_live",
                        )?,
                    })
                })
                .collect::<Result<Vec<_>, ConfigError>>()?;
            let family_id = family.family_id.clone();
            let family = NegRiskFamilyLiveTarget {
                family_id: family_id.clone(),
                members,
            };
            if targets.insert(family_id.clone(), family).is_some() {
                return Err(ConfigError::DuplicateFamilyId { family_id });
            }
        }

        Ok(NegRiskLiveTargetSet::new(targets))
    }
}

impl TryFrom<&AppLiveConfigView<'_>> for LocalSignerConfig {
    type Error = ConfigError;

    fn try_from(config: &AppLiveConfigView<'_>) -> Result<Self, Self::Error> {
        let signer = config
            .polymarket_signer()
            .ok_or(ConfigError::MissingLocalSignerConfig)?;
        let relayer_auth = config
            .polymarket_relayer_auth()
            .ok_or(ConfigError::MissingLocalSignerConfig)?;

        Ok(LocalSignerConfig {
            signer: LocalSignerIdentity {
                address: signer.address.clone(),
                funder_address: signer.funder_address.clone(),
                signature_type: signature_type_label(signer.signature_type).to_owned(),
                wallet_route: wallet_route_label(signer.wallet_route).to_owned(),
            },
            l2_auth: LocalL2AuthHeaders {
                api_key: signer.api_key.clone(),
                passphrase: signer.passphrase.clone(),
                timestamp: signer.timestamp.clone(),
                signature: signer.signature.clone(),
            },
            relayer_auth: map_relayer_auth(relayer_auth)?,
        })
    }
}

pub(crate) fn neg_risk_live_target_revision_from_targets(
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
        clob_host: parse_host_url("clob_host", &raw.clob_host, &["http", "https"], json)?,
        data_api_host: parse_host_url(
            "data_api_host",
            &raw.data_api_host,
            &["http", "https"],
            json,
        )?,
        relayer_host: parse_host_url("relayer_host", &raw.relayer_host, &["http", "https"], json)?,
        market_ws_url: parse_source_url("market_ws_url", &raw.market_ws_url, &["ws", "wss"], json)?,
        user_ws_url: parse_source_url("user_ws_url", &raw.user_ws_url, &["ws", "wss"], json)?,
        heartbeat_interval_seconds: parse_positive_interval(
            "heartbeat_interval_seconds",
            raw.heartbeat_interval_seconds,
            json,
        )?,
        relayer_poll_interval_seconds: parse_positive_interval(
            "relayer_poll_interval_seconds",
            raw.relayer_poll_interval_seconds,
            json,
        )?,
        metadata_refresh_interval_seconds: parse_positive_interval(
            "metadata_refresh_interval_seconds",
            raw.metadata_refresh_interval_seconds,
            json,
        )?,
    })
}

impl TryFrom<&AppLiveConfigView<'_>> for PolymarketSourceConfig {
    type Error = ConfigError;

    fn try_from(config: &AppLiveConfigView<'_>) -> Result<Self, Self::Error> {
        let source = config
            .polymarket_source()
            .ok_or(ConfigError::MissingPolymarketSourceConfig)?;

        source_config_from_toml(source)
    }
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

fn parse_source_url(
    field: &'static str,
    value: &str,
    allowed_schemes: &[&'static str],
    raw_json: &str,
) -> Result<Url, ConfigError> {
    let url = Url::parse(value).map_err(|error| ConfigError::InvalidPolymarketSourceConfig {
        value: raw_json.to_owned(),
        message: format!("{field}: {error}"),
    })?;

    if !allowed_schemes.contains(&url.scheme()) {
        return Err(ConfigError::InvalidPolymarketSourceConfig {
            value: raw_json.to_owned(),
            message: format!(
                "{field}: unsupported scheme '{}', expected one of: {}",
                url.scheme(),
                allowed_schemes.join(", ")
            ),
        });
    }

    Ok(url)
}

fn parse_host_url(
    field: &'static str,
    value: &str,
    allowed_schemes: &[&'static str],
    raw_json: &str,
) -> Result<Url, ConfigError> {
    let url = parse_source_url(field, value, allowed_schemes, raw_json)?;

    if url.host_str().is_none() {
        return Err(ConfigError::InvalidPolymarketSourceConfig {
            value: raw_json.to_owned(),
            message: format!("{field}: must include a host"),
        });
    }
    if url.path() != "/" {
        return Err(ConfigError::InvalidPolymarketSourceConfig {
            value: raw_json.to_owned(),
            message: format!("{field}: host URL must not include a path"),
        });
    }
    if url.query().is_some() {
        return Err(ConfigError::InvalidPolymarketSourceConfig {
            value: raw_json.to_owned(),
            message: format!("{field}: host URL must not include a query string"),
        });
    }
    if url.fragment().is_some() {
        return Err(ConfigError::InvalidPolymarketSourceConfig {
            value: raw_json.to_owned(),
            message: format!("{field}: host URL must not include a fragment"),
        });
    }

    Ok(url)
}

fn parse_positive_interval(
    field: &'static str,
    value: u64,
    raw_json: &str,
) -> Result<u64, ConfigError> {
    if value > 0 {
        Ok(value)
    } else {
        Err(ConfigError::InvalidPolymarketSourceConfig {
            value: raw_json.to_owned(),
            message: format!("{field}: must be > 0"),
        })
    }
}

fn source_config_from_toml(
    source: &PolymarketSourceToml,
) -> Result<PolymarketSourceConfig, ConfigError> {
    Ok(PolymarketSourceConfig {
        clob_host: parse_host_url(
            "polymarket.source.clob_host",
            &source.clob_host,
            &["http", "https"],
            "app_live",
        )?,
        data_api_host: parse_host_url(
            "polymarket.source.data_api_host",
            &source.data_api_host,
            &["http", "https"],
            "app_live",
        )?,
        relayer_host: parse_host_url(
            "polymarket.source.relayer_host",
            &source.relayer_host,
            &["http", "https"],
            "app_live",
        )?,
        market_ws_url: parse_source_url(
            "polymarket.source.market_ws_url",
            &source.market_ws_url,
            &["ws", "wss"],
            "app_live",
        )?,
        user_ws_url: parse_source_url(
            "polymarket.source.user_ws_url",
            &source.user_ws_url,
            &["ws", "wss"],
            "app_live",
        )?,
        heartbeat_interval_seconds: parse_positive_interval(
            "polymarket.source.heartbeat_interval_seconds",
            source.heartbeat_interval_seconds,
            "app_live",
        )?,
        relayer_poll_interval_seconds: parse_positive_interval(
            "polymarket.source.relayer_poll_interval_seconds",
            source.relayer_poll_interval_seconds,
            "app_live",
        )?,
        metadata_refresh_interval_seconds: parse_positive_interval(
            "polymarket.source.metadata_refresh_interval_seconds",
            source.metadata_refresh_interval_seconds,
            "app_live",
        )?,
    })
}

fn parse_decimal(
    field: &'static str,
    value: &str,
    raw_value: &str,
) -> Result<Decimal, ConfigError> {
    Decimal::from_str(value).map_err(|error| ConfigError::InvalidJson {
        value: raw_value.to_owned(),
        message: format!("{field}: {error}"),
    })
}

fn signature_type_label(value: SignatureTypeToml) -> &'static str {
    match value {
        SignatureTypeToml::Eoa => "Eoa",
        SignatureTypeToml::Proxy => "Proxy",
        SignatureTypeToml::Safe => "Safe",
    }
}

fn wallet_route_label(value: WalletRouteToml) -> &'static str {
    match value {
        WalletRouteToml::Eoa => "Eoa",
        WalletRouteToml::Proxy => "Proxy",
        WalletRouteToml::Safe => "Safe",
    }
}

fn map_relayer_auth(raw: &PolymarketRelayerAuthToml) -> Result<LocalRelayerAuth, ConfigError> {
    match raw.kind {
        RelayerAuthKindToml::BuilderApiKey => Ok(LocalRelayerAuth::BuilderApiKey {
            api_key: raw.api_key.clone(),
            timestamp: raw.timestamp.clone().ok_or_else(|| {
                ConfigError::InvalidLocalSignerConfig {
                    value: "app_live".to_owned(),
                    message: "polymarket.relayer_auth.timestamp is required".to_owned(),
                }
            })?,
            passphrase: raw.passphrase.clone().ok_or_else(|| {
                ConfigError::InvalidLocalSignerConfig {
                    value: "app_live".to_owned(),
                    message: "polymarket.relayer_auth.passphrase is required".to_owned(),
                }
            })?,
            signature: raw.signature.clone().ok_or_else(|| {
                ConfigError::InvalidLocalSignerConfig {
                    value: "app_live".to_owned(),
                    message: "polymarket.relayer_auth.signature is required".to_owned(),
                }
            })?,
        }),
        RelayerAuthKindToml::RelayerApiKey => Ok(LocalRelayerAuth::RelayerApiKey {
            api_key: raw.api_key.clone(),
            address: raw
                .address
                .clone()
                .ok_or_else(|| ConfigError::InvalidLocalSignerConfig {
                    value: "app_live".to_owned(),
                    message: "polymarket.relayer_auth.address is required".to_owned(),
                })?,
        }),
    }
}
