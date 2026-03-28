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
) -> Result<RealUserShadowSmokeSources, String> {
    Ok(RealUserShadowSmokeSources {
        source_config: source,
        signer_config: signer,
        market: MarketDataTaskGroup,
        user: UserStateTaskGroup,
        heartbeat: HeartbeatTaskGroup::for_tests(SmokeHeartbeatSource),
        relayer: RelayerTaskGroup,
        metadata: MetadataTaskGroup,
        bootstrap_snapshot: RemoteSnapshot::empty(),
    })
}
