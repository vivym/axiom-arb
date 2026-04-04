use state::RemoteSnapshot;
use venue_polymarket::HeartbeatFetchResult;

use crate::{
    config::PolymarketSourceConfig,
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
    bootstrap_snapshot: RemoteSnapshot,
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
    Ok(RealUserShadowSmokeSources {
        source_config: source,
        signer_config: signer,
        market: MarketDataTaskGroup,
        user: UserStateTaskGroup,
        heartbeat: HeartbeatTaskGroup::for_runtime(SmokeHeartbeatSource, run_session_id),
        relayer: RelayerTaskGroup,
        metadata: MetadataTaskGroup,
        bootstrap_snapshot: RemoteSnapshot::empty(),
    })
}

#[cfg(test)]
mod tests {
    use config_schema::{load_raw_config_from_str, ValidatedConfig};

    use super::build_real_user_shadow_smoke_sources;
    use crate::LocalSignerConfig;

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

    fn live_config_view() -> config_schema::AppLiveConfigView<'static> {
        let raw = load_raw_config_from_str(
            r#"
[runtime]
mode = "live"
real_user_shadow_smoke = true

[polymarket.source]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://data-api.polymarket.com"
relayer_host = "https://relayer-v2.polymarket.com"
market_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
user_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5

[signer]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "EOA"
wallet_route = "direct"

[signer.l2_auth]
api_key = "key"
passphrase = "pass"
secret = "secret"

[signer.relayer_auth]
api_key = "relayer-key"
secret = "relayer-secret"
passphrase = "relayer-pass"
"#,
        )
        .expect("raw config should parse");
        let validated = ValidatedConfig::new(raw).expect("validated config should parse");
        let leaked = Box::leak(Box::new(validated));
        leaked.for_app_live().expect("app-live config should load")
    }
}
