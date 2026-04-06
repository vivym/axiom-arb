use std::sync::Arc;

use serde_json::Value;

use crate::sdk_backend::{PolymarketClobApi, PolymarketStreamApi};
use crate::{MarketWsEvent, PolymarketGatewayError, UserWsEvent};

#[derive(Clone)]
pub struct PolymarketGateway {
    clob_api: Option<Arc<dyn PolymarketClobApi>>,
    stream_api: Option<Arc<dyn PolymarketStreamApi>>,
}

impl std::fmt::Debug for PolymarketGateway {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolymarketGateway").finish_non_exhaustive()
    }
}

impl PolymarketGateway {
    #[must_use]
    pub fn from_clob_api(clob_api: Arc<dyn PolymarketClobApi>) -> Self {
        Self {
            clob_api: Some(clob_api),
            stream_api: None,
        }
    }

    #[must_use]
    pub fn from_stream_api(stream_api: Arc<dyn PolymarketStreamApi>) -> Self {
        Self {
            clob_api: None,
            stream_api: Some(stream_api),
        }
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
