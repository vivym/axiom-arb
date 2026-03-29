mod error;
mod raw;
mod validate;

pub use error::ConfigSchemaError;
pub use raw::{
    NegRiskTargetSourceKindToml, NegRiskTargetSourceToml, PolymarketAccountToml, RawAxiomConfig,
    RuntimeModeToml,
};
pub use validate::{
    AppLiveConfigView, AppLiveNegRiskRolloutView, AppLiveNegRiskTargetMemberView,
    AppLiveNegRiskTargetMembersView, AppLiveNegRiskTargetSourceView, AppLiveNegRiskTargetView,
    AppLiveNegRiskTargetsView, AppLivePolymarketAccountView, AppLivePolymarketRelayerAuthKind,
    AppLivePolymarketRelayerAuthView, AppLivePolymarketSignerView, AppLivePolymarketSourceView,
    AppReplayConfigView, ValidatedConfig,
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
