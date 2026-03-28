use crate::{
    config::{ConfigError, PolymarketSourceConfig},
    runtime::AppRuntimeMode,
};
use config_schema::{AppLiveConfigView, RuntimeModeToml};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealUserShadowSmokeConfig {
    pub enabled: bool,
    pub source_config: PolymarketSourceConfig,
}

pub fn load_real_user_shadow_smoke_config(
    config: &AppLiveConfigView<'_>,
) -> Result<Option<RealUserShadowSmokeConfig>, ConfigError> {
    if !config.real_user_shadow_smoke() {
        return Ok(None);
    }

    let source_config = PolymarketSourceConfig::try_from(config)?;

    Ok(Some(RealUserShadowSmokeConfig {
        enabled: true,
        source_config,
    }))
}

pub fn app_runtime_mode_from_config(mode: RuntimeModeToml) -> AppRuntimeMode {
    match mode {
        RuntimeModeToml::Paper => AppRuntimeMode::Paper,
        RuntimeModeToml::Live => AppRuntimeMode::Live,
    }
}
