mod error;
mod raw;
mod validate;

pub use error::ConfigSchemaError;
pub use raw::{
    NegRiskRolloutToml, NegRiskTargetSourceKindToml, NegRiskTargetSourceToml, NegRiskTargetsToml,
    NegRiskToml, PolymarketAccountToml, PolymarketRelayerAuthToml, PolymarketSourceToml,
    PolymarketToml, RawAxiomConfig, RelayerAuthKindToml, RuntimeModeToml, RuntimeToml,
    SignatureTypeToml, StrategiesToml, StrategyControlSourceToml, StrategyControlToml,
    StrategyRouteRolloutToml, StrategyRouteToml, WalletRouteToml,
};
pub use validate::{
    AppLiveConfigView, AppLiveNegRiskRolloutView, AppLiveNegRiskTargetMemberView,
    AppLiveNegRiskTargetMembersView, AppLiveNegRiskTargetSourceView, AppLiveNegRiskTargetView,
    AppLiveNegRiskTargetsView, AppLivePolymarketAccountView, AppLivePolymarketRelayerAuthKind,
    AppLivePolymarketRelayerAuthView, AppLivePolymarketSignerView, AppLivePolymarketSourceView,
    AppReplayConfigView, ValidatedConfig,
};

pub fn load_raw_config_from_str(text: &str) -> Result<RawAxiomConfig, ConfigSchemaError> {
    let value: toml::Value = toml::from_str(text)?;
    reject_removed_polymarket_http_config(&value)?;
    Ok(value.try_into()?)
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

fn reject_removed_polymarket_http_config(value: &toml::Value) -> Result<(), ConfigSchemaError> {
    let has_removed_http_block = value
        .get("polymarket")
        .and_then(toml::Value::as_table)
        .is_some_and(|polymarket| polymarket.contains_key("http"));
    if has_removed_http_block {
        return Err(ConfigSchemaError::Validation(
            "[polymarket.http] is no longer supported and proxy_url has been removed; use standard proxy environment variables such as HTTPS_PROXY or ALL_PROXY instead".to_owned(),
        ));
    }
    Ok(())
}
