use std::collections::{BTreeSet, HashSet};

use crate::error::ConfigSchemaError;
use crate::raw::{
    NegRiskRolloutToml, NegRiskTargetMemberToml, NegRiskTargetSourceKindToml,
    NegRiskTargetSourceToml, NegRiskTargetToml, PolymarketAccountToml,
    PolymarketRelayerAuthToml, PolymarketSignerToml, PolymarketSourceToml, RawAxiomConfig,
    RelayerAuthKindToml, RuntimeModeToml, SignatureTypeToml, WalletRouteToml,
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
pub struct AppLivePolymarketAccountView<'a> {
    raw: &'a PolymarketAccountToml,
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
pub struct AppLiveNegRiskRolloutView<'a> {
    raw: &'a NegRiskRolloutToml,
}

#[derive(Debug, Clone, Copy)]
pub struct AppLiveNegRiskTargetSourceView<'a> {
    raw: &'a NegRiskTargetSourceToml,
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

    pub fn has_polymarket_account(&self) -> bool {
        self.raw
            .polymarket
            .as_ref()
            .and_then(|polymarket| polymarket.account.as_ref())
            .is_some()
    }

    pub fn has_polymarket_source(&self) -> bool {
        self.raw
            .polymarket
            .as_ref()
            .and_then(|polymarket| {
                polymarket
                    .source_overrides
                    .as_ref()
                    .or(polymarket.source.as_ref())
            })
            .is_some()
    }

    pub fn has_polymarket_signer(&self) -> bool {
        self.raw
            .polymarket
            .as_ref()
            .and_then(|polymarket| polymarket.signer.as_ref())
            .is_some()
    }

    pub fn has_target_source(&self) -> bool {
        self.raw
            .negrisk
            .as_ref()
            .and_then(|negrisk| negrisk.target_source.as_ref())
            .is_some()
    }

    pub fn account(&self) -> Option<AppLivePolymarketAccountView<'a>> {
        self.raw
            .polymarket
            .as_ref()
            .and_then(|polymarket| polymarket.account.as_ref())
            .map(|raw| AppLivePolymarketAccountView { raw })
    }

    pub fn polymarket_source(&self) -> Option<AppLivePolymarketSourceView<'a>> {
        self.raw
            .polymarket
            .as_ref()
            .and_then(|polymarket| {
                polymarket
                    .source_overrides
                    .as_ref()
                    .or(polymarket.source.as_ref())
            })
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

    pub fn target_source(&self) -> Option<AppLiveNegRiskTargetSourceView<'a>> {
        self.raw
            .negrisk
            .as_ref()
            .and_then(|negrisk| negrisk.target_source.as_ref())
            .map(|raw| AppLiveNegRiskTargetSourceView { raw })
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

    pub fn negrisk_rollout(&self) -> Option<AppLiveNegRiskRolloutView<'a>> {
        self.raw
            .negrisk
            .as_ref()
            .and_then(|negrisk| negrisk.rollout.as_ref())
            .map(|raw| AppLiveNegRiskRolloutView { raw })
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

impl<'a> AppLivePolymarketAccountView<'a> {
    pub fn address(&self) -> &'a str {
        &self.raw.address
    }

    pub fn funder_address(&self) -> Option<&'a str> {
        self.raw.funder_address.as_deref()
    }

    pub fn api_key(&self) -> &'a str {
        &self.raw.api_key
    }

    pub fn secret(&self) -> &'a str {
        &self.raw.secret
    }

    pub fn passphrase(&self) -> &'a str {
        &self.raw.passphrase
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

impl<'a> AppLiveNegRiskRolloutView<'a> {
    pub fn approved_families(&self) -> &'a [String] {
        &self.raw.approved_families
    }

    pub fn ready_families(&self) -> &'a [String] {
        &self.raw.ready_families
    }
}

impl<'a> AppLiveNegRiskTargetSourceView<'a> {
    pub fn source(&self) -> NegRiskTargetSourceKindToml {
        self.raw.source
    }

    pub fn is_adopted(&self) -> bool {
        matches!(self.raw.source, NegRiskTargetSourceKindToml::Adopted)
    }

    pub fn operator_target_revision(&self) -> Option<&'a str> {
        self.raw.operator_target_revision.as_deref()
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

    validate_negrisk(raw)?;

    Ok(())
}

fn validate_app_live_requiredness(
    raw: &RawAxiomConfig,
) -> Result<AppLiveConfigView<'_>, ConfigSchemaError> {
    match raw.runtime.mode {
        RuntimeModeToml::Paper => Ok(AppLiveConfigView { raw }),
        RuntimeModeToml::Live => {
            if has_operator_facing_live_inputs(raw) {
                let account = require_account(raw)?;
                validate_account_view(account)?;

                let relayer_auth = require_relayer_auth_view(raw)?;
                validate_relayer_auth_view(relayer_auth)?;

                let target_source = require_target_source(raw)?;
                validate_target_source_view(target_source)?;

                if raw.runtime.real_user_shadow_smoke {
                    require_source_defaults_or_overrides(raw)?;
                }
            } else {
                require_source(raw)?;
                require_signer(raw)?;

                let relayer_auth = require_relayer_auth_view(raw)?;
                validate_relayer_auth_view(relayer_auth)?;

                require_rollout(raw)?;
                validate_negrisk_rollout_referential_integrity(raw)?;
            }

            Ok(AppLiveConfigView { raw })
        }
    }
}

fn validate_app_replay_requiredness(
    raw: &RawAxiomConfig,
) -> Result<AppReplayConfigView<'_>, ConfigSchemaError> {
    Ok(AppReplayConfigView { raw })
}

fn require_source_defaults_or_overrides(raw: &RawAxiomConfig) -> Result<(), ConfigSchemaError> {
    if raw
        .polymarket
        .as_ref()
        .and_then(|polymarket| {
            polymarket
                .source_overrides
                .as_ref()
                .or(polymarket.source.as_ref())
        })
        .is_none()
    {
        return Err(validation_error(
            "missing required section: polymarket.source or polymarket.source_overrides",
        ));
    }
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

fn has_operator_facing_live_inputs(raw: &RawAxiomConfig) -> bool {
    raw.polymarket
        .as_ref()
        .and_then(|polymarket| polymarket.account.as_ref())
        .is_some()
        || raw
            .negrisk
            .as_ref()
            .and_then(|negrisk| negrisk.target_source.as_ref())
            .is_some()
}

fn require_account(
    raw: &RawAxiomConfig,
) -> Result<AppLivePolymarketAccountView<'_>, ConfigSchemaError> {
    raw.polymarket
        .as_ref()
        .and_then(|polymarket| polymarket.account.as_ref())
        .map(|raw| AppLivePolymarketAccountView { raw })
        .ok_or_else(|| validation_error("missing required section: polymarket.account"))
}

fn require_target_source(
    raw: &RawAxiomConfig,
) -> Result<AppLiveNegRiskTargetSourceView<'_>, ConfigSchemaError> {
    raw.negrisk
        .as_ref()
        .and_then(|negrisk| negrisk.target_source.as_ref())
        .map(|raw| AppLiveNegRiskTargetSourceView { raw })
        .ok_or_else(|| validation_error("missing required section: negrisk.target_source"))
}

fn require_relayer_auth_view(
    raw: &RawAxiomConfig,
) -> Result<AppLivePolymarketRelayerAuthView<'_>, ConfigSchemaError> {
    raw.polymarket
        .as_ref()
        .and_then(|polymarket| polymarket.relayer_auth.as_ref())
        .map(|raw| AppLivePolymarketRelayerAuthView { raw })
        .ok_or_else(|| validation_error("missing required section: polymarket.relayer_auth"))
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

    Ok(())
}

fn validate_account_view(
    raw: AppLivePolymarketAccountView<'_>,
) -> Result<(), ConfigSchemaError> {
    require_non_empty_local_signer_field(raw.address(), "polymarket.account.address")?;

    if let Some(funder_address) = raw.funder_address() {
        require_non_empty_local_signer_field(funder_address, "polymarket.account.funder_address")?;
    }

    require_non_empty_local_signer_field(raw.api_key(), "polymarket.account.api_key")?;
    require_non_empty_local_signer_field(raw.secret(), "polymarket.account.secret")?;
    require_non_empty_local_signer_field(raw.passphrase(), "polymarket.account.passphrase")?;

    if raw.signature_type_label() != raw.wallet_route_label() {
        return Err(validation_error(
            "polymarket.account.wallet_route must match polymarket.account.signature_type",
        ));
    }

    Ok(())
}

fn validate_relayer_auth_view(
    raw: AppLivePolymarketRelayerAuthView<'_>,
) -> Result<(), ConfigSchemaError> {
    require_non_empty_local_signer_field(raw.api_key(), "polymarket.relayer_auth.api_key")?;

    match raw.kind() {
        AppLivePolymarketRelayerAuthKind::BuilderApiKey => {
            require_non_empty_optional_local_signer_field(
                raw.timestamp(),
                "polymarket.relayer_auth.timestamp",
            )?;
            require_non_empty_optional_local_signer_field(
                raw.passphrase(),
                "polymarket.relayer_auth.passphrase",
            )?;
            require_non_empty_optional_local_signer_field(
                raw.signature(),
                "polymarket.relayer_auth.signature",
            )?;
        }
        AppLivePolymarketRelayerAuthKind::RelayerApiKey => {
            require_non_empty_optional_local_signer_field(
                raw.address(),
                "polymarket.relayer_auth.address",
            )?;
        }
    }

    Ok(())
}

fn validate_target_source_view(
    raw: AppLiveNegRiskTargetSourceView<'_>,
) -> Result<(), ConfigSchemaError> {
    if let Some(operator_target_revision) = raw.operator_target_revision() {
        require_non_empty_local_signer_field(
            operator_target_revision,
            "negrisk.target_source.operator_target_revision",
        )?;
    }

    Ok(())
}

fn require_non_empty_local_signer_field(
    value: &str,
    field: &'static str,
) -> Result<(), ConfigSchemaError> {
    if value.trim().is_empty() {
        Err(validation_error(format!("{field} must not be empty")))
    } else {
        Ok(())
    }
}

fn require_non_empty_optional_local_signer_field(
    value: Option<&str>,
    field: &'static str,
) -> Result<String, ConfigSchemaError> {
    let value = value.ok_or_else(|| validation_error(format!("{field} is required")))?;

    if value.trim().is_empty() {
        Err(validation_error(format!("{field} must not be empty")))
    } else {
        Ok(value.to_owned())
    }
}

fn validate_negrisk_rollout_referential_integrity(
    raw: &RawAxiomConfig,
) -> Result<(), ConfigSchemaError> {
    let Some(negrisk) = raw.negrisk.as_ref() else {
        return Ok(());
    };
    let Some(rollout) = negrisk.rollout.as_ref() else {
        return Ok(());
    };

    let family_ids = negrisk
        .targets
        .iter()
        .map(|target| target.family_id.as_str())
        .collect::<HashSet<_>>();

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

    Ok(())
}

fn validate_target_member(member: &NegRiskTargetMemberToml) -> Result<(), ConfigSchemaError> {
    require_non_empty("negrisk.targets.members.condition_id", &member.condition_id)?;
    require_non_empty("negrisk.targets.members.token_id", &member.token_id)?;
    require_non_empty("negrisk.targets.members.price", &member.price)?;
    require_non_empty("negrisk.targets.members.quantity", &member.quantity)?;
    Ok(())
}

fn require_non_empty(field: &str, value: &str) -> Result<(), ConfigSchemaError> {
    if value.trim().is_empty() {
        return Err(validation_error(format!("{field} must not be empty")));
    }
    Ok(())
}

fn validation_error(message: impl Into<String>) -> ConfigSchemaError {
    ConfigSchemaError::Validation(message.into())
}
