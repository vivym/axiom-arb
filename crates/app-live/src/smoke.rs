use crate::config::{load_polymarket_source_config, ConfigError, PolymarketSourceConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealUserShadowSmokeConfig {
    pub enabled: bool,
    pub source_config: PolymarketSourceConfig,
}

pub fn load_real_user_shadow_smoke_config(
    guard: Option<&str>,
    source_json: Option<&str>,
) -> Result<Option<RealUserShadowSmokeConfig>, ConfigError> {
    if guard != Some("1") {
        return Ok(None);
    }

    let source_config = load_polymarket_source_config(source_json)?;

    Ok(Some(RealUserShadowSmokeConfig {
        enabled: true,
        source_config,
    }))
}
