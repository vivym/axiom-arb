use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

use crate::TokenId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InventoryBucket {
    Free,
    ReservedForOrder,
    MatchedUnsettled,
    PendingCtfIn,
    PendingCtfOut,
    Redeemable,
    Quarantined,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WalletRoute {
    Eoa,
    Proxy,
    Safe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignatureType {
    Eoa,
    Proxy,
    Safe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalStatus {
    Unknown,
    Missing,
    Pending,
    Approved,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReservationState {
    Pending,
    Reserved,
    Released,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ApprovalKey {
    pub token_id: TokenId,
    pub spender: String,
    pub owner_address: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalState {
    pub token_id: TokenId,
    pub spender: String,
    pub owner_address: String,
    pub funder_address: String,
    pub wallet_route: WalletRoute,
    pub signature_type: SignatureType,
    pub allowance: Decimal,
    pub required_min_allowance: Decimal,
    pub last_checked_at: DateTime<Utc>,
    pub approval_status: ApprovalStatus,
}
