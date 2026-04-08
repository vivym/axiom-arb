use state::RemoteSnapshot;
use venue_polymarket::HeartbeatFetchResult;

use crate::{
    config::PolymarketSourceConfig,
    polymarket_runtime_adapter::PolymarketMetadataGatewayBackend,
    task_groups::{RelayerTaskGroup, UserStateTaskGroup},
    BootstrapSource, HeartbeatSource, HeartbeatTaskGroup, LocalSignerConfig, MarketDataTaskGroup,
    MetadataTaskGroup, StaticSnapshotSource,
};

#[derive(Debug)]
pub struct RealUserShadowSmokeSources {
    pub source_config: PolymarketSourceConfig,
    pub signer_config: LocalSignerConfig,
    pub market: MarketDataTaskGroup,
    pub user: UserStateTaskGroup,
    pub heartbeat: HeartbeatTaskGroup<SmokeHeartbeatSource>,
    pub relayer: RelayerTaskGroup,
    pub metadata: MetadataTaskGroup,
    #[cfg_attr(not(test), allow(dead_code))]
    metadata_backend: SmokeMetadataBackend,
    bootstrap_snapshot: RemoteSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SmokeMetadataBackend {
    Sdk,
    LegacyRest,
}

impl SmokeMetadataBackend {
    #[cfg_attr(not(test), allow(dead_code))]
    const fn as_str(self) -> &'static str {
        match self {
            Self::Sdk => "sdk",
            Self::LegacyRest => "legacy-rest",
        }
    }
}

impl From<PolymarketMetadataGatewayBackend> for SmokeMetadataBackend {
    fn from(value: PolymarketMetadataGatewayBackend) -> Self {
        match value {
            PolymarketMetadataGatewayBackend::Sdk => Self::Sdk,
            PolymarketMetadataGatewayBackend::LegacyRest => Self::LegacyRest,
        }
    }
}

#[derive(Debug, Default)]
pub struct SmokeHeartbeatSource;

impl HeartbeatSource for SmokeHeartbeatSource {
    fn poll<'a>(
        &'a mut self,
        previous_heartbeat_id: Option<&'a str>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<HeartbeatFetchResult, String>> + Send + 'a>,
    > {
        let heartbeat_id = previous_heartbeat_id
            .unwrap_or("smoke-heartbeat")
            .to_owned();
        Box::pin(async move {
            Ok(HeartbeatFetchResult {
                heartbeat_id,
                valid: true,
            })
        })
    }
}

#[derive(Debug)]
pub enum SmokeSafeStartupSource {
    Static(StaticSnapshotSource),
    RealUserShadowSmoke(Box<RealUserShadowSmokeSources>),
}

impl BootstrapSource for RealUserShadowSmokeSources {
    fn snapshot(&self) -> RemoteSnapshot {
        self.bootstrap_snapshot.clone()
    }
}

impl BootstrapSource for SmokeSafeStartupSource {
    fn snapshot(&self) -> RemoteSnapshot {
        match self {
            Self::Static(source) => source.snapshot(),
            Self::RealUserShadowSmoke(source) => source.snapshot(),
        }
    }
}

pub fn build_real_user_shadow_smoke_sources(
    source: PolymarketSourceConfig,
    signer: LocalSignerConfig,
    run_session_id: &str,
) -> Result<RealUserShadowSmokeSources, String> {
    let metadata_backend =
        crate::polymarket_runtime_adapter::polymarket_metadata_gateway_backend(&source).into();
    Ok(RealUserShadowSmokeSources {
        source_config: source,
        signer_config: signer,
        market: MarketDataTaskGroup,
        user: UserStateTaskGroup,
        heartbeat: HeartbeatTaskGroup::for_runtime(SmokeHeartbeatSource, run_session_id),
        relayer: RelayerTaskGroup,
        metadata: MetadataTaskGroup,
        metadata_backend,
        bootstrap_snapshot: RemoteSnapshot::empty(),
    })
}

impl RealUserShadowSmokeSources {
    #[cfg(test)]
    pub(crate) fn metadata_backend_for_tests(&self) -> &'static str {
        self.metadata_backend.as_str()
    }
}

#[cfg(test)]
mod tests {
    use config_schema::{load_raw_config_from_str, ValidatedConfig};

    use super::{build_real_user_shadow_smoke_sources, SmokeMetadataBackend};
    use crate::polymarket_runtime_adapter::PolymarketMetadataGatewayBackend;
    use crate::{config::PolymarketSourceConfig, LocalSignerConfig};

    #[test]
    fn smoke_source_builder_threads_run_session_id_into_heartbeat_group() {
        let config = live_config_view();
        let smoke = crate::load_real_user_shadow_smoke_config(&config)
            .expect("smoke config should parse")
            .expect("smoke should be enabled");
        let signer = LocalSignerConfig::try_from(&config).expect("signer config should parse");

        let sources = build_real_user_shadow_smoke_sources(
            smoke.source_config.clone(),
            signer,
            "run-session-77",
        )
        .expect("source bundle should build");

        assert_eq!(sources.heartbeat.test_source_session_id(), "run-session-77");
    }

    #[test]
    fn smoke_source_builder_uses_adapter_owned_metadata_backend_selection() {
        let sources = build_real_user_shadow_smoke_sources(
            smoke_source_config(),
            sample_signer_config(),
            "run-session-1",
        )
        .expect("source bundle should build");

        assert_eq!(sources.metadata_backend_for_tests(), "sdk");
    }

    #[test]
    fn smoke_source_builder_preserves_legacy_rest_backend_mapping() {
        assert_eq!(
            SmokeMetadataBackend::from(PolymarketMetadataGatewayBackend::LegacyRest).as_str(),
            "legacy-rest"
        );
    }

    fn live_config_view() -> config_schema::AppLiveConfigView<'static> {
        let raw = load_raw_config_from_str(
            r#"
[runtime]
mode = "live"
real_user_shadow_smoke = true

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key"
secret = "poly-secret"
passphrase = "poly-passphrase"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key"
address = "0x1111111111111111111111111111111111111111"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"

[polymarket.source_overrides]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://gamma-api.polymarket.com"
relayer_host = "https://relayer-v2.polymarket.com"
market_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
user_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60
"#,
        )
        .expect("raw config should parse");
        let validated = ValidatedConfig::new(raw).expect("validated config should parse");
        let leaked = Box::leak(Box::new(validated));
        leaked.for_app_live().expect("app-live config should load")
    }

    fn sample_signer_config() -> LocalSignerConfig {
        let config = live_config_view();
        LocalSignerConfig::try_from(&config).expect("signer config should parse")
    }

    fn smoke_source_config() -> PolymarketSourceConfig {
        PolymarketSourceConfig {
            clob_host: "https://clob.polymarket.com"
                .parse()
                .expect("clob host should parse"),
            data_api_host: "https://gamma-api.polymarket.com"
                .parse()
                .expect("data host should parse"),
            relayer_host: "https://relayer-v2.polymarket.com"
                .parse()
                .expect("relayer host should parse"),
            market_ws_url: "wss://ws-subscriptions-clob.polymarket.com/ws/market"
                .parse()
                .expect("market ws should parse"),
            user_ws_url: "wss://ws-subscriptions-clob.polymarket.com/ws/user"
                .parse()
                .expect("user ws should parse"),
            heartbeat_interval_seconds: 15,
            relayer_poll_interval_seconds: 5,
            metadata_refresh_interval_seconds: 60,
        }
    }
}
