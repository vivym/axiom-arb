use rust_decimal::Decimal;

use crate::{ConditionId, MarketId, TokenId};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OrderId(String);

impl OrderId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for OrderId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for OrderId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedOrderIdentity {
    pub signed_order_hash: String,
    pub salt: String,
    pub nonce: String,
    pub signature: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmissionState {
    Draft,
    Planned,
    RiskApproved,
    Signed,
    Submitted,
    Acked,
    Rejected,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VenueOrderState {
    Live,
    Matched,
    Delayed,
    Unmatched,
    CancelPending,
    Cancelled,
    Expired,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettlementState {
    Matched,
    Mined,
    Confirmed,
    Retrying,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    pub order_id: OrderId,
    pub market_id: MarketId,
    pub condition_id: ConditionId,
    pub token_id: TokenId,
    pub quantity: Decimal,
    pub price: Decimal,
    pub submission_state: SubmissionState,
    pub venue_state: VenueOrderState,
    pub settlement_state: SettlementState,
    pub signed_order: Option<SignedOrderIdentity>,
}
