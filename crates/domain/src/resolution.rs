use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

use crate::ConditionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionStatus {
    Unresolved,
    Resolved,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisputeState {
    None,
    Disputed,
    Challenged,
    UnderReview,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionState {
    pub condition_id: ConditionId,
    pub resolution_status: ResolutionStatus,
    pub payout_vector: Vec<Decimal>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub dispute_state: DisputeState,
    pub redeemable_at: Option<DateTime<Utc>>,
}
