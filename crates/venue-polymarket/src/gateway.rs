use std::sync::Arc;

use serde_json::Value;

use crate::sdk_backend::PolymarketClobApi;
use crate::PolymarketGatewayError;

#[derive(Clone)]
pub struct PolymarketGateway {
    clob_api: Arc<dyn PolymarketClobApi>,
}

impl std::fmt::Debug for PolymarketGateway {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolymarketGateway").finish_non_exhaustive()
    }
}

impl PolymarketGateway {
    #[must_use]
    pub fn from_clob_api(clob_api: Arc<dyn PolymarketClobApi>) -> Self {
        Self { clob_api }
    }

    pub async fn open_orders(
        &self,
        query: PolymarketOrderQuery,
    ) -> Result<Vec<PolymarketOpenOrderSummary>, PolymarketGatewayError> {
        self.clob_api.open_orders(&query).await
    }

    pub async fn submit_order(
        &self,
        order: PolymarketSignedOrder,
    ) -> Result<PolymarketSubmitResponse, PolymarketGatewayError> {
        self.clob_api.submit_order(&order).await
    }

    pub async fn post_heartbeat(
        &self,
        previous_heartbeat_id: Option<&str>,
    ) -> Result<PolymarketHeartbeatStatus, PolymarketGatewayError> {
        self.clob_api.post_heartbeat(previous_heartbeat_id).await
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
