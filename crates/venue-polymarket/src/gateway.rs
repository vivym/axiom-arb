use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio::sync::Mutex as AsyncMutex;

use crate::auth::RelayerAuth;
use crate::metadata::{
    refresh_neg_risk_metadata_from_api, NegRiskMarketMetadata, NegRiskMetadataCache,
};
use crate::relayer::{RelayerTransaction, RelayerTransactionType};
use crate::sdk_backend::{
    PolymarketClobApi, PolymarketMetadataApi, PolymarketRelayerApi, PolymarketStreamApi,
};
use crate::{MarketWsEvent, PolymarketGatewayError, UserWsEvent};

#[derive(Clone)]
pub struct PolymarketGateway {
    clob_api: Option<Arc<dyn PolymarketClobApi>>,
    stream_api: Option<Arc<dyn PolymarketStreamApi>>,
    metadata_api: Option<Arc<dyn PolymarketMetadataApi>>,
    relayer_api: Option<Arc<dyn PolymarketRelayerApi>>,
    metadata_state: Arc<Mutex<NegRiskMetadataCache>>,
    metadata_refresh_lock: Arc<AsyncMutex<()>>,
}

impl std::fmt::Debug for PolymarketGateway {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolymarketGateway").finish_non_exhaustive()
    }
}

impl PolymarketGateway {
    fn empty() -> Self {
        Self {
            clob_api: None,
            stream_api: None,
            metadata_api: None,
            relayer_api: None,
            metadata_state: Arc::new(Mutex::new(NegRiskMetadataCache::default())),
            metadata_refresh_lock: Arc::new(AsyncMutex::new(())),
        }
    }

    #[must_use]
    pub fn from_clob_api(clob_api: Arc<dyn PolymarketClobApi>) -> Self {
        let mut gateway = Self::empty();
        gateway.clob_api = Some(clob_api);
        gateway
    }

    #[must_use]
    pub fn from_stream_api(stream_api: Arc<dyn PolymarketStreamApi>) -> Self {
        let mut gateway = Self::empty();
        gateway.stream_api = Some(stream_api);
        gateway
    }

    #[must_use]
    pub fn from_metadata_api(metadata_api: Arc<dyn PolymarketMetadataApi>) -> Self {
        let mut gateway = Self::empty();
        gateway.metadata_api = Some(metadata_api);
        gateway
    }

    #[must_use]
    pub fn from_relayer_api(relayer_api: Arc<dyn PolymarketRelayerApi>) -> Self {
        let mut gateway = Self::empty();
        gateway.relayer_api = Some(relayer_api);
        gateway
    }

    pub async fn open_orders(
        &self,
        query: PolymarketOrderQuery,
    ) -> Result<Vec<PolymarketOpenOrderSummary>, PolymarketGatewayError> {
        self.clob_api()?.open_orders(&query).await
    }

    pub async fn submit_order(
        &self,
        order: PolymarketSignedOrder,
    ) -> Result<PolymarketSubmitResponse, PolymarketGatewayError> {
        self.clob_api()?.submit_order(&order).await
    }

    pub async fn post_heartbeat(
        &self,
        previous_heartbeat_id: Option<&str>,
    ) -> Result<PolymarketHeartbeatStatus, PolymarketGatewayError> {
        self.clob_api()?.post_heartbeat(previous_heartbeat_id).await
    }

    pub async fn collect_market_events(
        &self,
        token_ids: Vec<String>,
    ) -> Result<Vec<MarketWsEvent>, PolymarketGatewayError> {
        self.stream_api()?.market_events(&token_ids).await
    }

    pub async fn collect_user_events(
        &self,
        auth: PolymarketUserStreamAuth,
        condition_ids: Vec<String>,
    ) -> Result<Vec<UserWsEvent>, PolymarketGatewayError> {
        self.stream_api()?.user_events(&auth, &condition_ids).await
    }

    pub async fn refresh_neg_risk_metadata(
        &self,
    ) -> Result<Vec<NegRiskMarketMetadata>, PolymarketGatewayError> {
        refresh_neg_risk_metadata_from_api(
            self.metadata_api()?.as_ref(),
            &self.metadata_state,
            &self.metadata_refresh_lock,
        )
        .await
    }

    pub async fn recent_transactions(
        &self,
        auth: &RelayerAuth<'_>,
    ) -> Result<Vec<RelayerTransaction>, PolymarketGatewayError> {
        self.relayer_api()?.recent_transactions(auth).await
    }

    pub async fn current_nonce(
        &self,
        auth: &RelayerAuth<'_>,
        address: &str,
        wallet_type: RelayerTransactionType,
    ) -> Result<String, PolymarketGatewayError> {
        self.relayer_api()?
            .current_nonce(auth, address, wallet_type)
            .await
    }

    fn clob_api(&self) -> Result<&Arc<dyn PolymarketClobApi>, PolymarketGatewayError> {
        self.clob_api.as_ref().ok_or_else(|| {
            PolymarketGatewayError::protocol("clob api is not configured on this gateway")
        })
    }

    fn stream_api(&self) -> Result<&Arc<dyn PolymarketStreamApi>, PolymarketGatewayError> {
        self.stream_api.as_ref().ok_or_else(|| {
            PolymarketGatewayError::protocol("stream api is not configured on this gateway")
        })
    }

    fn metadata_api(&self) -> Result<&Arc<dyn PolymarketMetadataApi>, PolymarketGatewayError> {
        self.metadata_api.as_ref().ok_or_else(|| {
            PolymarketGatewayError::protocol("metadata api is not configured on this gateway")
        })
    }

    fn relayer_api(&self) -> Result<&Arc<dyn PolymarketRelayerApi>, PolymarketGatewayError> {
        self.relayer_api.as_ref().ok_or_else(|| {
            PolymarketGatewayError::protocol("relayer api is not configured on this gateway")
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PolymarketSignedOrder {
    pub order: Value,
    pub owner: String,
    pub order_type: String,
    pub defer_exec: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolymarketOrderQuery {
    OpenOrders,
}

impl PolymarketOrderQuery {
    #[must_use]
    pub fn open_orders() -> Self {
        Self::OpenOrders
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolymarketOpenOrderSummary {
    pub order_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolymarketHeartbeatStatus {
    pub heartbeat_id: String,
    pub valid: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolymarketSubmitResponse {
    pub order_id: String,
    pub status: String,
    pub success: bool,
    pub error_message: Option<String>,
    pub transaction_hashes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolymarketUserStreamAuth {
    pub address: String,
    pub api_key: String,
    pub secret: String,
    pub passphrase: String,
}
