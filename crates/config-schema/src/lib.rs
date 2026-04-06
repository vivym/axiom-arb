mod error;
mod raw;
mod validate;

pub use error::ConfigSchemaError;
pub use raw::{
    NegRiskRolloutToml, NegRiskTargetSourceKindToml, NegRiskTargetSourceToml, NegRiskTargetsToml,
    NegRiskToml, PolymarketAccountToml, PolymarketHttpToml, PolymarketRelayerAuthToml,
    PolymarketSourceToml, PolymarketToml, RawAxiomConfig, RelayerAuthKindToml, RuntimeModeToml,
    RuntimeToml, SignatureTypeToml, StrategiesToml, StrategyControlSourceToml, StrategyControlToml,
    StrategyRouteRolloutToml, StrategyRouteToml, WalletRouteToml,
};
pub use validate::{
    AppLiveConfigView, AppLiveNegRiskRolloutView, AppLiveNegRiskTargetMemberView,
    AppLiveNegRiskTargetMembersView, AppLiveNegRiskTargetSourceView, AppLiveNegRiskTargetView,
    AppLiveNegRiskTargetsView, AppLivePolymarketAccountView, AppLivePolymarketHttpView,
    AppLivePolymarketRelayerAuthKind, AppLivePolymarketRelayerAuthView,
    AppLivePolymarketSignerView, AppLivePolymarketSourceView, AppReplayConfigView, ValidatedConfig,
};

pub fn load_raw_config_from_str(text: &str) -> Result<RawAxiomConfig, ConfigSchemaError> {
    Ok(toml::from_str(text)?)
}

pub fn load_raw_config_from_path(
    path: &std::path::Path,
) -> Result<RawAxiomConfig, ConfigSchemaError> {
    let text = std::fs::read_to_string(path)?;
    load_raw_config_from_str(&text)
}

pub fn render_raw_config_to_string(raw: &RawAxiomConfig) -> Result<String, ConfigSchemaError> {
    toml::to_string_pretty(raw)
        .map_err(|error| ConfigSchemaError::Validation(format!("toml serialize error: {error}")))
}
