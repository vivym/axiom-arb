use std::collections::{BTreeSet, HashSet};

use url::Url;

use crate::error::ConfigSchemaError;
use crate::raw::{
    NegRiskRolloutToml, NegRiskTargetMemberToml, NegRiskTargetToml, PolymarketRelayerAuthToml,
    PolymarketSignerToml, PolymarketSourceToml, RawAxiomConfig, RelayerAuthKindToml,
    RuntimeModeToml, SignatureTypeToml, WalletRouteToml,
};

#[derive(Debug, Clone)]
pub struct ValidatedConfig {
    raw: RawAxiomConfig,
}

impl ValidatedConfig {
    pub fn new(raw: RawAxiomConfig) -> Result<Self, ConfigSchemaError> {
        validate_global_invariants(&raw)?;
        Ok(Self { raw })
    }

    pub fn for_app_live(&self) -> Result<AppLiveConfigView<'_>, ConfigSchemaError> {
        validate_app_live_requiredness(&self.raw)
    }

    pub fn for_app_replay(&self) -> Result<AppReplayConfigView<'_>, ConfigSchemaError> {
        validate_app_replay_requiredness(&self.raw)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AppLiveConfigView<'a> {
    raw: &'a RawAxiomConfig,
}

#[derive(Debug, Clone, Copy)]
pub struct AppReplayConfigView<'a> {
    raw: &'a RawAxiomConfig,
}

#[derive(Debug, Clone, Copy)]
pub struct AppLivePolymarketSourceView<'a> {
    raw: &'a PolymarketSourceToml,
}

#[derive(Debug, Clone, Copy)]
pub struct AppLivePolymarketSignerView<'a> {
    raw: &'a PolymarketSignerToml,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppLivePolymarketRelayerAuthKind {
    BuilderApiKey,
    RelayerApiKey,
}

#[derive(Debug, Clone, Copy)]
pub struct AppLivePolymarketRelayerAuthView<'a> {
    raw: &'a PolymarketRelayerAuthToml,
}

#[derive(Debug, Clone, Copy)]
pub struct AppLiveNegRiskTargetsView<'a> {
    raw: &'a [NegRiskTargetToml],
}

#[derive(Debug, Clone, Copy)]
pub struct AppLiveNegRiskTargetView<'a> {
    raw: &'a NegRiskTargetToml,
}

#[derive(Debug, Clone, Copy)]
pub struct AppLiveNegRiskTargetMembersView<'a> {
    raw: &'a [NegRiskTargetMemberToml],
}

#[derive(Debug, Clone, Copy)]
pub struct AppLiveNegRiskTargetMemberView<'a> {
    raw: &'a NegRiskTargetMemberToml,
}

impl<'a> AppLiveConfigView<'a> {
    pub fn mode(&self) -> RuntimeModeToml {
        self.raw.runtime.mode
    }

    pub fn is_live(&self) -> bool {
        self.raw.runtime.mode == RuntimeModeToml::Live
    }

    pub fn is_paper(&self) -> bool {
        self.raw.runtime.mode == RuntimeModeToml::Paper
    }

    pub fn real_user_shadow_smoke(&self) -> bool {
        self.raw.runtime.real_user_shadow_smoke
    }

    pub fn has_polymarket_source(&self) -> bool {
        self.raw
            .polymarket
            .as_ref()
            .and_then(|polymarket| polymarket.source.as_ref())
            .is_some()
    }

    pub fn has_polymarket_signer(&self) -> bool {
        self.raw
            .polymarket
            .as_ref()
            .and_then(|polymarket| polymarket.signer.as_ref())
            .is_some()
    }

    pub fn polymarket_source(&self) -> Option<AppLivePolymarketSourceView<'a>> {
        self.raw
            .polymarket
            .as_ref()
            .and_then(|polymarket| polymarket.source.as_ref())
            .map(|raw| AppLivePolymarketSourceView { raw })
    }

    pub fn polymarket_signer(&self) -> Option<AppLivePolymarketSignerView<'a>> {
        self.raw
            .polymarket
            .as_ref()
            .and_then(|polymarket| polymarket.signer.as_ref())
            .map(|raw| AppLivePolymarketSignerView { raw })
    }

    pub fn polymarket_relayer_auth(&self) -> Option<AppLivePolymarketRelayerAuthView<'a>> {
        self.raw
            .polymarket
            .as_ref()
            .and_then(|polymarket| polymarket.relayer_auth.as_ref())
            .map(|raw| AppLivePolymarketRelayerAuthView { raw })
    }

    pub fn negrisk_targets(&self) -> AppLiveNegRiskTargetsView<'a> {
        AppLiveNegRiskTargetsView {
            raw: self
                .raw
                .negrisk
                .as_ref()
                .map(|negrisk| negrisk.targets.as_slice())
                .unwrap_or(&[]),
        }
    }

    pub fn negrisk_rollout(&self) -> Option<&'a NegRiskRolloutToml> {
        self.raw
            .negrisk
            .as_ref()
            .and_then(|negrisk| negrisk.rollout.as_ref())
    }
}

impl<'a> AppReplayConfigView<'a> {
    pub fn mode(&self) -> RuntimeModeToml {
        self.raw.runtime.mode
    }

    pub fn real_user_shadow_smoke(&self) -> bool {
        self.raw.runtime.real_user_shadow_smoke
    }
}

impl<'a> AppLivePolymarketSourceView<'a> {
    pub fn clob_host(&self) -> &'a str {
        &self.raw.clob_host
    }

    pub fn data_api_host(&self) -> &'a str {
        &self.raw.data_api_host
    }

    pub fn relayer_host(&self) -> &'a str {
        &self.raw.relayer_host
    }

    pub fn market_ws_url(&self) -> &'a str {
        &self.raw.market_ws_url
    }

    pub fn user_ws_url(&self) -> &'a str {
        &self.raw.user_ws_url
    }

    pub fn heartbeat_interval_seconds(&self) -> u64 {
        self.raw.heartbeat_interval_seconds
    }

    pub fn relayer_poll_interval_seconds(&self) -> u64 {
        self.raw.relayer_poll_interval_seconds
    }

    pub fn metadata_refresh_interval_seconds(&self) -> u64 {
        self.raw.metadata_refresh_interval_seconds
    }
}

impl<'a> AppLivePolymarketSignerView<'a> {
    pub fn address(&self) -> &'a str {
        &self.raw.address
    }

    pub fn funder_address(&self) -> &'a str {
        &self.raw.funder_address
    }

    pub fn api_key(&self) -> &'a str {
        &self.raw.api_key
    }

    pub fn passphrase(&self) -> &'a str {
        &self.raw.passphrase
    }

    pub fn timestamp(&self) -> &'a str {
        &self.raw.timestamp
    }

    pub fn signature(&self) -> &'a str {
        &self.raw.signature
    }

    pub fn signature_type_label(&self) -> &'static str {
        match self.raw.signature_type {
            SignatureTypeToml::Eoa => "Eoa",
            SignatureTypeToml::Proxy => "Proxy",
            SignatureTypeToml::Safe => "Safe",
        }
    }

    pub fn wallet_route_label(&self) -> &'static str {
        match self.raw.wallet_route {
            WalletRouteToml::Eoa => "Eoa",
            WalletRouteToml::Proxy => "Proxy",
            WalletRouteToml::Safe => "Safe",
        }
    }
}

impl<'a> AppLivePolymarketRelayerAuthView<'a> {
    pub fn kind(&self) -> AppLivePolymarketRelayerAuthKind {
        match self.raw.kind {
            RelayerAuthKindToml::BuilderApiKey => AppLivePolymarketRelayerAuthKind::BuilderApiKey,
            RelayerAuthKindToml::RelayerApiKey => AppLivePolymarketRelayerAuthKind::RelayerApiKey,
        }
    }

    pub fn is_builder_api_key(&self) -> bool {
        matches!(self.kind(), AppLivePolymarketRelayerAuthKind::BuilderApiKey)
    }

    pub fn api_key(&self) -> &'a str {
        &self.raw.api_key
    }

    pub fn timestamp(&self) -> Option<&'a str> {
        self.raw.timestamp.as_deref()
    }

    pub fn passphrase(&self) -> Option<&'a str> {
        self.raw.passphrase.as_deref()
    }

    pub fn signature(&self) -> Option<&'a str> {
        self.raw.signature.as_deref()
    }

    pub fn address(&self) -> Option<&'a str> {
        self.raw.address.as_deref()
    }
}

impl<'a> AppLiveNegRiskTargetsView<'a> {
    pub fn iter(&self) -> impl Iterator<Item = AppLiveNegRiskTargetView<'a>> + 'a {
        self.raw.iter().map(|raw| AppLiveNegRiskTargetView { raw })
    }
}

impl<'a> AppLiveNegRiskTargetView<'a> {
    pub fn family_id(&self) -> &'a str {
        &self.raw.family_id
    }

    pub fn members(&self) -> AppLiveNegRiskTargetMembersView<'a> {
        AppLiveNegRiskTargetMembersView {
            raw: self.raw.members.as_slice(),
        }
    }
}

impl<'a> AppLiveNegRiskTargetMembersView<'a> {
    pub fn iter(&self) -> impl Iterator<Item = AppLiveNegRiskTargetMemberView<'a>> + 'a {
        self.raw
            .iter()
            .map(|raw| AppLiveNegRiskTargetMemberView { raw })
    }
}

impl<'a> AppLiveNegRiskTargetMemberView<'a> {
    pub fn condition_id(&self) -> &'a str {
        &self.raw.condition_id
    }

    pub fn token_id(&self) -> &'a str {
        &self.raw.token_id
    }

    pub fn price(&self) -> &'a str {
        &self.raw.price
    }

    pub fn quantity(&self) -> &'a str {
        &self.raw.quantity
    }
}

pub(crate) fn validate_global_invariants(raw: &RawAxiomConfig) -> Result<(), ConfigSchemaError> {
    if raw.runtime.mode == RuntimeModeToml::Paper && raw.runtime.real_user_shadow_smoke {
        return Err(validation_error(
            "runtime.real_user_shadow_smoke is not supported when runtime.mode = \"paper\"",
        ));
    }

    if let Some(source) = raw
        .polymarket
        .as_ref()
        .and_then(|polymarket| polymarket.source.as_ref())
    {
        validate_source(source)?;
    }

    if let Some(signer) = raw
        .polymarket
        .as_ref()
        .and_then(|polymarket| polymarket.signer.as_ref())
    {
        validate_signer(signer)?;
    }

    if let Some(relayer_auth) = raw
        .polymarket
        .as_ref()
        .and_then(|polymarket| polymarket.relayer_auth.as_ref())
    {
        validate_relayer_auth(relayer_auth)?;
    }

    validate_negrisk(raw)?;

    Ok(())
}

fn validate_app_live_requiredness(
    raw: &RawAxiomConfig,
) -> Result<AppLiveConfigView<'_>, ConfigSchemaError> {
    match raw.runtime.mode {
        RuntimeModeToml::Paper => Ok(AppLiveConfigView { raw }),
        RuntimeModeToml::Live => {
            require_source(raw)?;
            require_signer(raw)?;
            require_relayer_auth(raw)?;
            require_rollout(raw)?;

            Ok(AppLiveConfigView { raw })
        }
    }
}

fn validate_app_replay_requiredness(
    raw: &RawAxiomConfig,
) -> Result<AppReplayConfigView<'_>, ConfigSchemaError> {
    Ok(AppReplayConfigView { raw })
}

fn validate_source(source: &PolymarketSourceToml) -> Result<(), ConfigSchemaError> {
    validate_host_url("polymarket.source.clob_host", &source.clob_host)?;
    validate_host_url("polymarket.source.data_api_host", &source.data_api_host)?;
    validate_host_url("polymarket.source.relayer_host", &source.relayer_host)?;
    validate_ws_url("polymarket.source.market_ws_url", &source.market_ws_url)?;
    validate_ws_url("polymarket.source.user_ws_url", &source.user_ws_url)?;

    validate_positive_u64(
        "polymarket.source.heartbeat_interval_seconds",
        source.heartbeat_interval_seconds,
    )?;
    validate_positive_u64(
        "polymarket.source.relayer_poll_interval_seconds",
        source.relayer_poll_interval_seconds,
    )?;
    validate_positive_u64(
        "polymarket.source.metadata_refresh_interval_seconds",
        source.metadata_refresh_interval_seconds,
    )?;

    Ok(())
}

fn validate_signer(signer: &PolymarketSignerToml) -> Result<(), ConfigSchemaError> {
    require_non_empty("polymarket.signer.address", &signer.address)?;
    require_non_empty("polymarket.signer.funder_address", &signer.funder_address)?;
    require_non_empty("polymarket.signer.api_key", &signer.api_key)?;
    require_non_empty("polymarket.signer.passphrase", &signer.passphrase)?;
    require_non_empty("polymarket.signer.timestamp", &signer.timestamp)?;
    require_non_empty("polymarket.signer.signature", &signer.signature)?;

    let expected_wallet_route = match signer.signature_type {
        SignatureTypeToml::Eoa => WalletRouteToml::Eoa,
        SignatureTypeToml::Proxy => WalletRouteToml::Proxy,
        SignatureTypeToml::Safe => WalletRouteToml::Safe,
    };

    if signer.wallet_route != expected_wallet_route {
        return Err(validation_error(
            "polymarket.signer.wallet_route must match polymarket.signer.signature_type",
        ));
    }

    Ok(())
}

fn validate_relayer_auth(
    relayer_auth: &PolymarketRelayerAuthToml,
) -> Result<(), ConfigSchemaError> {
    require_non_empty("polymarket.relayer_auth.api_key", &relayer_auth.api_key)?;

    match relayer_auth.kind {
        RelayerAuthKindToml::BuilderApiKey => {
            require_optional_non_empty(
                "polymarket.relayer_auth.timestamp",
                relayer_auth.timestamp.as_deref(),
            )?;
            require_optional_non_empty(
                "polymarket.relayer_auth.passphrase",
                relayer_auth.passphrase.as_deref(),
            )?;
            require_optional_non_empty(
                "polymarket.relayer_auth.signature",
                relayer_auth.signature.as_deref(),
            )?;
        }
        RelayerAuthKindToml::RelayerApiKey => {
            require_optional_non_empty(
                "polymarket.relayer_auth.address",
                relayer_auth.address.as_deref(),
            )?;
        }
    }

    Ok(())
}

fn validate_negrisk(raw: &RawAxiomConfig) -> Result<(), ConfigSchemaError> {
    let Some(negrisk) = raw.negrisk.as_ref() else {
        return Ok(());
    };

    let mut family_ids = BTreeSet::new();
    for target in &negrisk.targets {
        require_non_empty("negrisk.targets.family_id", &target.family_id)?;
        if !family_ids.insert(target.family_id.as_str()) {
            return Err(validation_error(format!(
                "duplicate negrisk.targets.family_id: {}",
                target.family_id
            )));
        }

        let mut members = HashSet::new();
        for member in &target.members {
            validate_target_member(member)?;
            if !members.insert((member.condition_id.as_str(), member.token_id.as_str())) {
                return Err(validation_error(format!(
                    "duplicate negrisk.targets.members token_id for family {} condition {} token {}",
                    target.family_id, member.condition_id, member.token_id
                )));
            }
        }
    }

    if let Some(rollout) = negrisk.rollout.as_ref() {
        for family_id in &rollout.approved_families {
            require_non_empty("negrisk.rollout.approved_families", family_id)?;
            if !family_ids.contains(family_id.as_str()) {
                return Err(validation_error(format!(
                    "negrisk.rollout.approved_families references missing family_id: {family_id}"
                )));
            }
        }

        for family_id in &rollout.ready_families {
            require_non_empty("negrisk.rollout.ready_families", family_id)?;
            if !family_ids.contains(family_id.as_str()) {
                return Err(validation_error(format!(
                    "negrisk.rollout.ready_families references missing family_id: {family_id}"
                )));
            }
        }
    }

    Ok(())
}

fn validate_target_member(member: &NegRiskTargetMemberToml) -> Result<(), ConfigSchemaError> {
    require_non_empty("negrisk.targets.members.condition_id", &member.condition_id)?;
    require_non_empty("negrisk.targets.members.token_id", &member.token_id)?;
    require_non_empty("negrisk.targets.members.price", &member.price)?;
    require_non_empty("negrisk.targets.members.quantity", &member.quantity)?;
    Ok(())
}

fn require_source(raw: &RawAxiomConfig) -> Result<(), ConfigSchemaError> {
    if raw
        .polymarket
        .as_ref()
        .and_then(|polymarket| polymarket.source.as_ref())
        .is_none()
    {
        return Err(validation_error(
            "missing required section: polymarket.source",
        ));
    }
    Ok(())
}

fn require_signer(raw: &RawAxiomConfig) -> Result<(), ConfigSchemaError> {
    if raw
        .polymarket
        .as_ref()
        .and_then(|polymarket| polymarket.signer.as_ref())
        .is_none()
    {
        return Err(validation_error(
            "missing required section: polymarket.signer",
        ));
    }
    Ok(())
}

fn require_relayer_auth(raw: &RawAxiomConfig) -> Result<(), ConfigSchemaError> {
    if raw
        .polymarket
        .as_ref()
        .and_then(|polymarket| polymarket.relayer_auth.as_ref())
        .is_none()
    {
        return Err(validation_error(
            "missing required section: polymarket.relayer_auth",
        ));
    }
    Ok(())
}

fn require_rollout(raw: &RawAxiomConfig) -> Result<(), ConfigSchemaError> {
    if raw
        .negrisk
        .as_ref()
        .and_then(|negrisk| negrisk.rollout.as_ref())
        .is_none()
    {
        return Err(validation_error(
            "missing required section: negrisk.rollout",
        ));
    }
    Ok(())
}

fn validate_positive_u64(field: &str, value: u64) -> Result<(), ConfigSchemaError> {
    if value == 0 {
        return Err(validation_error(format!(
            "{field} must be greater than zero"
        )));
    }
    Ok(())
}

fn require_non_empty(field: &str, value: &str) -> Result<(), ConfigSchemaError> {
    if value.trim().is_empty() {
        return Err(validation_error(format!("{field} must not be empty")));
    }
    Ok(())
}

fn require_optional_non_empty(field: &str, value: Option<&str>) -> Result<(), ConfigSchemaError> {
    let Some(value) = value else {
        return Err(validation_error(format!("missing required field: {field}")));
    };
    require_non_empty(field, value)
}

fn validate_host_url(field: &str, value: &str) -> Result<(), ConfigSchemaError> {
    let url = parse_url(field, value)?;
    match url.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(validation_error(format!(
                "{field} has unsupported scheme {scheme}"
            )));
        }
    }
    if url.host_str().is_none() {
        return Err(validation_error(format!("{field} must include a host")));
    }
    if url.path() != "/" && !url.path().is_empty() {
        return Err(validation_error(format!(
            "{field} must use a root host URL"
        )));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(validation_error(format!(
            "{field} must not include query or fragment"
        )));
    }
    Ok(())
}

fn validate_ws_url(field: &str, value: &str) -> Result<(), ConfigSchemaError> {
    let url = parse_url(field, value)?;
    match url.scheme() {
        "ws" | "wss" => Ok(()),
        scheme => Err(validation_error(format!(
            "{field} has unsupported scheme {scheme}"
        ))),
    }
}

fn parse_url(field: &str, value: &str) -> Result<Url, ConfigSchemaError> {
    Url::parse(value).map_err(|error| validation_error(format!("{field}: {error}")))
}

fn validation_error(message: impl Into<String>) -> ConfigSchemaError {
    ConfigSchemaError::Validation(message.into())
}
