use std::{collections::BTreeMap, fmt, str::FromStr};

use chrono::Utc;
use config_schema::{
    AppLiveConfigView, AppLivePolymarketAccountView, AppLivePolymarketRelayerAuthKind,
    AppLivePolymarketRelayerAuthView, AppLivePolymarketSourceView,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use venue_polymarket::{
    derive_builder_relayer_auth_material, derive_l2_auth_material, PolymarketUrl as Url,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegRiskLiveTargetSet {
    revision: String,
    targets: BTreeMap<String, NegRiskFamilyLiveTarget>,
}

impl NegRiskLiveTargetSet {
    pub fn empty() -> Self {
        Self::new(BTreeMap::new())
    }

    pub fn from_targets_with_revision(
        revision: impl Into<String>,
        targets: BTreeMap<String, NegRiskFamilyLiveTarget>,
    ) -> Self {
        Self {
            revision: revision.into(),
            targets,
        }
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RouteRuntimeArtifact {
    pub scope: String,
    pub route_policy_version: String,
    pub semantic_digest: String,
    pub content: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LocalSignerConfig {
    pub signer: LocalSignerIdentity,
    pub l2_auth: LocalL2AuthHeaders,
    pub relayer_auth: LocalRelayerAuth,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LocalAccountRuntimeConfig {
    pub signer: LocalSignerIdentity,
    pub l2_auth: LocalL2AuthHeaders,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LocalSignerIdentity {
    pub address: String,
    pub funder_address: String,
    pub signature_type: String,
    pub wallet_route: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolymarketGatewayCredentials {
    pub address: String,
    pub funder_address: String,
    pub signature_type: String,
    pub wallet_route: String,
    pub api_key: String,
    pub secret: String,
    pub passphrase: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LocalL2AuthHeaders {
    pub api_key: String,
    pub passphrase: String,
    pub timestamp: String,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LocalRelayerRuntimeConfig {
    pub auth: LocalRelayerAuth,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeWalletKind {
    Eoa,
    NonEoa,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    InvalidJson { value: String, message: String },
    InvalidLocalAccountRuntimeConfig { value: String, message: String },
    InvalidLocalRelayerRuntimeConfig { value: String, message: String },
    InvalidLocalSignerConfig { value: String, message: String },
    MissingPolymarketGatewayCredentials,
    InvalidPolymarketSourceConfig { value: String, message: String },
    DuplicateFamilyId { family_id: String },
    MissingLocalAccountRuntimeConfig,
    MissingLocalRelayerRuntimeConfig,
    MissingLocalSignerConfig,
    MissingPolymarketSourceConfig,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson { message, .. } => {
                write!(f, "invalid neg-risk live target config: {message}")
            }
            Self::InvalidLocalAccountRuntimeConfig { message, .. } => {
                write!(f, "invalid local account runtime config: {message}")
            }
            Self::InvalidLocalRelayerRuntimeConfig { message, .. } => {
                write!(f, "invalid local relayer runtime config: {message}")
            }
            Self::InvalidLocalSignerConfig { message, .. } => {
                write!(f, "invalid local signer config: {message}")
            }
            Self::MissingPolymarketGatewayCredentials => {
                write!(
                    f,
                    "missing polymarket gateway credentials for live daemon inputs"
                )
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
            Self::MissingLocalAccountRuntimeConfig => {
                write!(
                    f,
                    "missing local account runtime config for live neg-risk operator inputs"
                )
            }
            Self::MissingLocalRelayerRuntimeConfig => {
                write!(
                    f,
                    "missing local relayer runtime config for live neg-risk operator inputs"
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
        for family in config.negrisk_targets().iter() {
            let members = family
                .members()
                .iter()
                .map(|member| {
                    Ok(NegRiskMemberLiveTarget {
                        condition_id: member.condition_id().to_owned(),
                        token_id: member.token_id().to_owned(),
                        price: parse_decimal(
                            "negrisk.targets.members.price",
                            member.price(),
                            "app_live",
                        )?,
                        quantity: parse_decimal(
                            "negrisk.targets.members.quantity",
                            member.quantity(),
                            "app_live",
                        )?,
                    })
                })
                .collect::<Result<Vec<_>, ConfigError>>()?;
            let family_id = family.family_id().to_owned();
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
        let account_runtime = LocalAccountRuntimeConfig::try_from(config)
            .map_err(map_account_error_to_signer_error)?;
        let relayer_runtime = LocalRelayerRuntimeConfig::required_from(config)
            .map_err(map_relayer_error_to_signer_error)?;

        Ok(LocalSignerConfig {
            signer: account_runtime.signer,
            l2_auth: account_runtime.l2_auth,
            relayer_auth: relayer_runtime.auth,
        })
    }
}

impl TryFrom<&AppLiveConfigView<'_>> for LocalAccountRuntimeConfig {
    type Error = ConfigError;

    fn try_from(config: &AppLiveConfigView<'_>) -> Result<Self, Self::Error> {
        if config.polymarket_signer().is_some() {
            return Err(ConfigError::InvalidLocalAccountRuntimeConfig {
                value: "app_live".to_owned(),
                message: "polymarket.signer is no longer supported; use polymarket.account"
                    .to_owned(),
            });
        }
        let credentials = PolymarketGatewayCredentials::try_from(config).map_err(|error| {
            if matches!(error, ConfigError::MissingPolymarketGatewayCredentials) {
                ConfigError::MissingLocalAccountRuntimeConfig
            } else {
                error
            }
        })?;
        let derived = derive_l2_auth_material(
            &credentials.api_key,
            &credentials.secret,
            &credentials.passphrase,
            Utc::now(),
        )
        .map_err(|error| ConfigError::InvalidLocalAccountRuntimeConfig {
            value: "app_live".to_owned(),
            message: error.to_string(),
        })?;

        Ok(LocalAccountRuntimeConfig {
            signer: signer_identity_from_credentials(&credentials),
            l2_auth: LocalL2AuthHeaders {
                api_key: derived.api_key,
                passphrase: derived.passphrase,
                timestamp: derived.timestamp,
                signature: derived.signature,
            },
        })
    }
}

impl TryFrom<&AppLiveConfigView<'_>> for PolymarketGatewayCredentials {
    type Error = ConfigError;

    fn try_from(config: &AppLiveConfigView<'_>) -> Result<Self, Self::Error> {
        if config.polymarket_signer().is_some() {
            return Err(ConfigError::InvalidLocalAccountRuntimeConfig {
                value: "app_live".to_owned(),
                message: "polymarket.signer is no longer supported; use polymarket.account"
                    .to_owned(),
            });
        }

        let account = config
            .account()
            .ok_or(ConfigError::MissingPolymarketGatewayCredentials)?;
        validate_account_view(account)?;

        Ok(gateway_credentials_from_account(account))
    }
}

impl LocalRelayerRuntimeConfig {
    pub fn optional_from(config: &AppLiveConfigView<'_>) -> Result<Option<Self>, ConfigError> {
        config
            .polymarket_relayer_auth()
            .map(|raw| map_relayer_auth(raw).map(|auth| Self { auth }))
            .transpose()
    }

    pub fn required_from(config: &AppLiveConfigView<'_>) -> Result<Self, ConfigError> {
        Self::optional_from(config)?.ok_or(ConfigError::MissingLocalRelayerRuntimeConfig)
    }
}

pub(crate) fn runtime_wallet_kind(config: &AppLiveConfigView<'_>) -> Option<RuntimeWalletKind> {
    config.account().map(|_| {
        if config.polymarket_relayer_auth().is_some() {
            RuntimeWalletKind::NonEoa
        } else {
            RuntimeWalletKind::Eoa
        }
    })
}

pub(crate) fn runtime_wallet_kind_requires_relayer(config: &AppLiveConfigView<'_>) -> bool {
    matches!(runtime_wallet_kind(config), Some(RuntimeWalletKind::NonEoa))
}

pub fn neg_risk_live_targets_from_route_artifacts(
    route_artifacts: &BTreeMap<String, Vec<RouteRuntimeArtifact>>,
    operator_strategy_revision: Option<&str>,
) -> Result<NegRiskLiveTargetSet, ConfigError> {
    let mut targets = BTreeMap::new();

    for artifact in route_artifacts.get("neg-risk").into_iter().flatten() {
        let Some(rendered_live_target) = artifact.content.get("rendered_live_target") else {
            continue;
        };
        if rendered_live_target.is_null() {
            continue;
        }
        let target =
            serde_json::from_value::<NegRiskFamilyLiveTarget>(rendered_live_target.clone())
                .map_err(|error| ConfigError::InvalidJson {
                    value: artifact.scope.clone(),
                    message: error.to_string(),
                })?;
        let family_id = target.family_id.clone();
        if targets.insert(family_id.clone(), target).is_some() {
            return Err(ConfigError::DuplicateFamilyId { family_id });
        }
    }

    Ok(match operator_strategy_revision {
        Some(revision) => NegRiskLiveTargetSet::from_targets_with_revision(revision, targets),
        None => NegRiskLiveTargetSet::new(targets),
    })
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

impl TryFrom<&AppLiveConfigView<'_>> for PolymarketSourceConfig {
    type Error = ConfigError;

    fn try_from(config: &AppLiveConfigView<'_>) -> Result<Self, Self::Error> {
        let source = config
            .effective_polymarket_source()
            .ok_or(ConfigError::MissingPolymarketSourceConfig)?;

        source_config_from_view(source)
    }
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

fn source_config_from_view(
    source: AppLivePolymarketSourceView<'_>,
) -> Result<PolymarketSourceConfig, ConfigError> {
    Ok(PolymarketSourceConfig {
        clob_host: parse_host_url(
            "polymarket.source.clob_host",
            source.clob_host(),
            &["http", "https"],
            "app_live",
        )?,
        data_api_host: parse_host_url(
            "polymarket.source.data_api_host",
            source.data_api_host(),
            &["http", "https"],
            "app_live",
        )?,
        relayer_host: parse_host_url(
            "polymarket.source.relayer_host",
            source.relayer_host(),
            &["http", "https"],
            "app_live",
        )?,
        market_ws_url: parse_source_url(
            "polymarket.source.market_ws_url",
            source.market_ws_url(),
            &["ws", "wss"],
            "app_live",
        )?,
        user_ws_url: parse_source_url(
            "polymarket.source.user_ws_url",
            source.user_ws_url(),
            &["ws", "wss"],
            "app_live",
        )?,
        heartbeat_interval_seconds: parse_positive_interval(
            "polymarket.source.heartbeat_interval_seconds",
            source.heartbeat_interval_seconds(),
            "app_live",
        )?,
        relayer_poll_interval_seconds: parse_positive_interval(
            "polymarket.source.relayer_poll_interval_seconds",
            source.relayer_poll_interval_seconds(),
            "app_live",
        )?,
        metadata_refresh_interval_seconds: parse_positive_interval(
            "polymarket.source.metadata_refresh_interval_seconds",
            source.metadata_refresh_interval_seconds(),
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

fn map_relayer_auth(
    raw: AppLivePolymarketRelayerAuthView<'_>,
) -> Result<LocalRelayerAuth, ConfigError> {
    validate_relayer_auth_view(raw)?;

    match raw.kind() {
        AppLivePolymarketRelayerAuthKind::BuilderApiKey => {
            let api_key = raw.api_key().to_owned();
            let passphrase = require_non_empty_optional_relayer_field(
                raw.passphrase(),
                "polymarket.relayer_auth.passphrase",
            )?;

            if let Some(secret) = raw.secret() {
                let derived =
                    derive_builder_relayer_auth_material(&api_key, secret, &passphrase, Utc::now())
                        .map_err(|error| ConfigError::InvalidLocalRelayerRuntimeConfig {
                            value: "app_live".to_owned(),
                            message: error.to_string(),
                        })?;

                Ok(LocalRelayerAuth::BuilderApiKey {
                    api_key: derived.api_key,
                    timestamp: derived.timestamp,
                    passphrase: derived.passphrase,
                    signature: derived.signature,
                })
            } else {
                Ok(LocalRelayerAuth::BuilderApiKey {
                    api_key,
                    timestamp: require_non_empty_optional_relayer_field(
                        raw.timestamp(),
                        "polymarket.relayer_auth.timestamp",
                    )?,
                    passphrase,
                    signature: require_non_empty_optional_relayer_field(
                        raw.signature(),
                        "polymarket.relayer_auth.signature",
                    )?,
                })
            }
        }
        AppLivePolymarketRelayerAuthKind::RelayerApiKey => Ok(LocalRelayerAuth::RelayerApiKey {
            api_key: raw.api_key().to_owned(),
            address: require_non_empty_optional_relayer_field(
                raw.address(),
                "polymarket.relayer_auth.address",
            )?,
        }),
    }
}

fn gateway_credentials_from_account(
    account: AppLivePolymarketAccountView<'_>,
) -> PolymarketGatewayCredentials {
    PolymarketGatewayCredentials {
        address: account.address().to_owned(),
        funder_address: account
            .funder_address()
            .unwrap_or(account.address())
            .to_owned(),
        signature_type: account.signature_type_label().to_owned(),
        wallet_route: account.wallet_route_label().to_owned(),
        api_key: account.api_key().to_owned(),
        secret: account.secret().to_owned(),
        passphrase: account.passphrase().to_owned(),
    }
}

fn signer_identity_from_credentials(
    credentials: &PolymarketGatewayCredentials,
) -> LocalSignerIdentity {
    LocalSignerIdentity {
        address: credentials.address.clone(),
        funder_address: credentials.funder_address.clone(),
        signature_type: credentials.signature_type.clone(),
        wallet_route: credentials.wallet_route.clone(),
    }
}

impl From<LocalSignerConfig> for LocalAccountRuntimeConfig {
    fn from(value: LocalSignerConfig) -> Self {
        Self {
            signer: value.signer,
            l2_auth: value.l2_auth,
        }
    }
}

fn validate_account_view(account: AppLivePolymarketAccountView<'_>) -> Result<(), ConfigError> {
    require_non_empty_account_field(account.address(), "polymarket.account.address")?;
    if let Some(funder_address) = account.funder_address() {
        require_non_empty_account_field(funder_address, "polymarket.account.funder_address")?;
    }
    require_non_empty_account_field(account.api_key(), "polymarket.account.api_key")?;
    require_non_empty_account_field(account.secret(), "polymarket.account.secret")?;
    require_non_empty_account_field(account.passphrase(), "polymarket.account.passphrase")?;

    if account.signature_type_label() != account.wallet_route_label() {
        return Err(ConfigError::InvalidLocalAccountRuntimeConfig {
            value: "app_live".to_owned(),
            message: "polymarket.account.wallet_route must match polymarket.account.signature_type"
                .to_owned(),
        });
    }

    Ok(())
}

fn validate_relayer_auth_view(
    raw: AppLivePolymarketRelayerAuthView<'_>,
) -> Result<(), ConfigError> {
    require_non_empty_relayer_field(raw.api_key(), "polymarket.relayer_auth.api_key")?;

    match raw.kind() {
        AppLivePolymarketRelayerAuthKind::BuilderApiKey => {
            require_non_empty_optional_relayer_field(
                raw.passphrase(),
                "polymarket.relayer_auth.passphrase",
            )?;
            if raw.secret().is_some() {
                require_non_empty_optional_relayer_field(
                    raw.secret(),
                    "polymarket.relayer_auth.secret",
                )?;
            } else {
                require_non_empty_optional_relayer_field(
                    raw.timestamp(),
                    "polymarket.relayer_auth.timestamp",
                )?;
                require_non_empty_optional_relayer_field(
                    raw.signature(),
                    "polymarket.relayer_auth.signature",
                )?;
            }
        }
        AppLivePolymarketRelayerAuthKind::RelayerApiKey => {
            require_non_empty_optional_relayer_field(
                raw.address(),
                "polymarket.relayer_auth.address",
            )?;
        }
    }

    Ok(())
}

fn require_non_empty_account_field(value: &str, field: &'static str) -> Result<(), ConfigError> {
    if value.trim().is_empty() {
        Err(ConfigError::InvalidLocalAccountRuntimeConfig {
            value: "app_live".to_owned(),
            message: format!("{field} must not be empty"),
        })
    } else {
        Ok(())
    }
}

fn require_non_empty_optional_relayer_field(
    value: Option<&str>,
    field: &'static str,
) -> Result<String, ConfigError> {
    let value = value.ok_or_else(|| ConfigError::InvalidLocalRelayerRuntimeConfig {
        value: "app_live".to_owned(),
        message: format!("{field} is required"),
    })?;

    if value.trim().is_empty() {
        Err(ConfigError::InvalidLocalRelayerRuntimeConfig {
            value: "app_live".to_owned(),
            message: format!("{field} must not be empty"),
        })
    } else {
        Ok(value.to_owned())
    }
}

fn require_non_empty_relayer_field(value: &str, field: &'static str) -> Result<(), ConfigError> {
    if value.trim().is_empty() {
        Err(ConfigError::InvalidLocalRelayerRuntimeConfig {
            value: "app_live".to_owned(),
            message: format!("{field} must not be empty"),
        })
    } else {
        Ok(())
    }
}

fn map_account_error_to_signer_error(error: ConfigError) -> ConfigError {
    match error {
        ConfigError::MissingLocalAccountRuntimeConfig => ConfigError::MissingLocalSignerConfig,
        ConfigError::InvalidLocalAccountRuntimeConfig { value, message } => {
            ConfigError::InvalidLocalSignerConfig { value, message }
        }
        other => other,
    }
}

fn map_relayer_error_to_signer_error(error: ConfigError) -> ConfigError {
    match error {
        ConfigError::MissingLocalRelayerRuntimeConfig => ConfigError::MissingLocalSignerConfig,
        ConfigError::InvalidLocalRelayerRuntimeConfig { value, message } => {
            ConfigError::InvalidLocalSignerConfig { value, message }
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use config_schema::{load_raw_config_from_str, ValidatedConfig};

    use super::{runtime_wallet_kind, runtime_wallet_kind_requires_relayer, RuntimeWalletKind};

    #[test]
    fn runtime_wallet_kind_marks_validated_eoa_live_config_as_not_requiring_relayer() {
        let config = live_view(
            r#"
[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key"
secret = "poly-secret"
passphrase = "poly-passphrase"
"#,
        );

        assert_eq!(runtime_wallet_kind(&config), Some(RuntimeWalletKind::Eoa));
        assert!(!runtime_wallet_kind_requires_relayer(&config));
    }

    #[test]
    fn runtime_wallet_kind_marks_validated_non_eoa_live_config_as_requiring_relayer() {
        let config = live_view(
            r#"
[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "proxy"
wallet_route = "proxy"
api_key = "poly-api-key"
secret = "poly-secret"
passphrase = "poly-passphrase"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key"
address = "0x1111111111111111111111111111111111111111"
"#,
        );

        assert_eq!(
            runtime_wallet_kind(&config),
            Some(RuntimeWalletKind::NonEoa)
        );
        assert!(runtime_wallet_kind_requires_relayer(&config));
    }

    fn live_view(extra: &str) -> config_schema::AppLiveConfigView<'static> {
        let raw = Box::leak(Box::new(
            load_raw_config_from_str(&format!(
                r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[polymarket.source]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://gamma-api.polymarket.com"
relayer_host = "https://relayer-v2.polymarket.com"
market_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
user_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"

{extra}
"#
            ))
            .expect("config should parse"),
        ));
        let validated = Box::leak(Box::new(
            ValidatedConfig::new(raw.clone()).expect("config should validate"),
        ));

        validated.for_app_live().expect("live view should validate")
    }
}
