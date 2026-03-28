use crate::{
    config::{load_polymarket_source_config, ConfigError, PolymarketSourceConfig},
    runtime::AppRuntimeMode,
};
use std::ffi::OsStr;

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

pub fn load_real_user_shadow_smoke_config_from_env(
    app_mode: AppRuntimeMode,
    guard: Option<&OsStr>,
    source_json: Option<&OsStr>,
) -> Result<Option<RealUserShadowSmokeConfig>, Box<dyn std::error::Error>> {
    let Some(guard) = guard else {
        return Ok(None);
    };
    let guard = guard.to_str().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid value for AXIOM_REAL_USER_SHADOW_SMOKE: value is not valid UTF-8",
        )
    })?;
    if guard != "1" {
        return Ok(None);
    }
    if app_mode == AppRuntimeMode::Paper {
        return Err("real-user shadow smoke is not supported in paper mode".into());
    }

    let source_json = match source_json {
        Some(value) => Some(value.to_str().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid value for AXIOM_POLYMARKET_SOURCE_CONFIG: value is not valid UTF-8",
            )
        })?),
        None => None,
    };

    Ok(load_real_user_shadow_smoke_config(
        Some(guard),
        source_json,
    )?)
}
