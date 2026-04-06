use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub struct PolymarketGateway;

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
