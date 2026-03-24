use chrono::{DateTime, Duration, Utc};
use domain::{ApprovalState, ApprovalStatus, DisputeState, ResolutionState, RuntimeMode};
use rust_decimal::Decimal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    ModeNotHealthy,
    NetEdgeBelowThreshold,
    ApprovalInsufficient,
    FreshnessStale,
    RedeemBlockedByDispute,
    RedeemNotRunnable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskDecision {
    Accept,
    Reject(RejectReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FullSetRiskThresholds {
    pub min_net_edge_usdc: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreshnessPolicy {
    pub max_age: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Freshness {
    pub observed_at: DateTime<Utc>,
    pub checked_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FullSetRiskContext {
    pub runtime_mode: RuntimeMode,
    pub approvals: Vec<ApprovalState>,
    pub net_edge_usdc: Decimal,
    pub thresholds: FullSetRiskThresholds,
    pub freshness: Freshness,
    pub freshness_policy: FreshnessPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedeemRiskContext {
    pub runtime_mode: RuntimeMode,
    pub resolution: ResolutionState,
    pub freshness: Freshness,
    pub freshness_policy: FreshnessPolicy,
}

impl Freshness {
    pub fn new(observed_at: DateTime<Utc>, checked_at: DateTime<Utc>) -> Self {
        Self {
            observed_at,
            checked_at,
        }
    }

    pub fn fresh(observed_at: DateTime<Utc>) -> Self {
        Self::new(observed_at, Utc::now())
    }

    fn is_stale(&self, policy: &FreshnessPolicy) -> bool {
        self.checked_at - self.observed_at > policy.max_age
    }
}

impl RiskDecision {
    pub fn reject_reason(self) -> Option<RejectReason> {
        match self {
            Self::Accept => None,
            Self::Reject(reason) => Some(reason),
        }
    }
}

pub fn evaluate_fullset_trade(context: &FullSetRiskContext) -> RiskDecision {
    if context.runtime_mode != RuntimeMode::Healthy {
        return RiskDecision::Reject(RejectReason::ModeNotHealthy);
    }

    if context.approvals.is_empty()
        || context.approvals.iter().any(|approval| {
            approval.approval_status != ApprovalStatus::Approved
                || approval.allowance < approval.required_min_allowance
        })
    {
        return RiskDecision::Reject(RejectReason::ApprovalInsufficient);
    }

    if context.freshness.is_stale(&context.freshness_policy) {
        return RiskDecision::Reject(RejectReason::FreshnessStale);
    }

    if context.net_edge_usdc <= context.thresholds.min_net_edge_usdc {
        return RiskDecision::Reject(RejectReason::NetEdgeBelowThreshold);
    }

    RiskDecision::Accept
}

pub fn evaluate_redeem(context: &RedeemRiskContext) -> RiskDecision {
    if !matches!(
        context.runtime_mode,
        RuntimeMode::Healthy | RuntimeMode::NoNewRisk
    ) {
        return RiskDecision::Reject(RejectReason::ModeNotHealthy);
    }

    if matches!(
        context.resolution.dispute_state,
        DisputeState::Disputed | DisputeState::Challenged | DisputeState::UnderReview
    ) {
        return RiskDecision::Reject(RejectReason::RedeemBlockedByDispute);
    }

    if context.resolution.resolution_status != domain::ResolutionStatus::Resolved
        || context.resolution.resolved_at.is_none()
        || context.resolution.redeemable_at.is_none()
        || context.resolution.payout_vector.is_empty()
    {
        return RiskDecision::Reject(RejectReason::RedeemNotRunnable);
    }

    if context.freshness.is_stale(&context.freshness_policy) {
        return RiskDecision::Reject(RejectReason::FreshnessStale);
    }

    RiskDecision::Accept
}
