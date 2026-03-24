use chrono::{Duration, Utc};
use domain::{
    ApprovalState, ApprovalStatus, ConditionId, DisputeState, ResolutionState, ResolutionStatus,
    RuntimeMode, SignatureType, TokenId, WalletRoute,
};
use risk::fullset::{
    evaluate_fullset_trade, evaluate_redeem, Freshness, FreshnessPolicy, FullSetRiskContext,
    FullSetRiskThresholds, RejectReason,
};
use rust_decimal::Decimal;

#[test]
fn mode_not_healthy_is_rejected() {
    let decision = evaluate_fullset_trade(&FullSetRiskContext {
        runtime_mode: RuntimeMode::Reconciling,
        approval: sample_approval(ApprovalStatus::Approved, Decimal::new(100, 0)),
        net_edge_usdc: Decimal::new(10, 2),
        thresholds: sample_thresholds(),
        freshness: Freshness::fresh(Utc::now()),
        freshness_policy: sample_freshness_policy(),
    });

    assert_eq!(decision.reject_reason(), Some(RejectReason::ModeNotHealthy));
}

#[test]
fn approval_insufficient_is_rejected() {
    let decision = evaluate_fullset_trade(&FullSetRiskContext {
        runtime_mode: RuntimeMode::Healthy,
        approval: sample_approval(ApprovalStatus::Approved, Decimal::new(49, 0)),
        net_edge_usdc: Decimal::new(10, 2),
        thresholds: sample_thresholds(),
        freshness: Freshness::fresh(Utc::now()),
        freshness_policy: sample_freshness_policy(),
    });

    assert_eq!(
        decision.reject_reason(),
        Some(RejectReason::ApprovalInsufficient)
    );
}

#[test]
fn approval_status_not_approved_is_rejected() {
    let decision = evaluate_fullset_trade(&FullSetRiskContext {
        runtime_mode: RuntimeMode::Healthy,
        approval: sample_approval(ApprovalStatus::Missing, Decimal::new(100, 0)),
        net_edge_usdc: Decimal::new(10, 2),
        thresholds: sample_thresholds(),
        freshness: Freshness::fresh(Utc::now()),
        freshness_policy: sample_freshness_policy(),
    });

    assert_eq!(
        decision.reject_reason(),
        Some(RejectReason::ApprovalInsufficient)
    );
}

#[test]
fn freshness_stale_is_rejected() {
    let decision = evaluate_fullset_trade(&FullSetRiskContext {
        runtime_mode: RuntimeMode::Healthy,
        approval: sample_approval(ApprovalStatus::Approved, Decimal::new(100, 0)),
        net_edge_usdc: Decimal::new(10, 2),
        thresholds: sample_thresholds(),
        freshness: Freshness::fresh(Utc::now() - Duration::seconds(31)),
        freshness_policy: sample_freshness_policy(),
    });

    assert_eq!(decision.reject_reason(), Some(RejectReason::FreshnessStale));
}

#[test]
fn net_edge_below_threshold_is_rejected() {
    let decision = evaluate_fullset_trade(&FullSetRiskContext {
        runtime_mode: RuntimeMode::Healthy,
        approval: sample_approval(ApprovalStatus::Approved, Decimal::new(100, 0)),
        net_edge_usdc: Decimal::new(4, 2),
        thresholds: sample_thresholds(),
        freshness: Freshness::fresh(Utc::now()),
        freshness_policy: sample_freshness_policy(),
    });

    assert_eq!(
        decision.reject_reason(),
        Some(RejectReason::NetEdgeBelowThreshold)
    );
}

#[test]
fn redeem_is_rejected_while_condition_is_disputed() {
    let decision = evaluate_redeem(
        &sample_resolution(DisputeState::Disputed),
        &Freshness::fresh(Utc::now()),
        &sample_freshness_policy(),
    );

    assert_eq!(
        decision.reject_reason(),
        Some(RejectReason::RedeemBlockedByDispute)
    );
}

#[test]
fn redeem_is_rejected_when_freshness_is_stale() {
    let decision = evaluate_redeem(
        &sample_resolution(DisputeState::None),
        &Freshness::fresh(Utc::now() - Duration::seconds(31)),
        &sample_freshness_policy(),
    );

    assert_eq!(decision.reject_reason(), Some(RejectReason::FreshnessStale));
}

fn sample_approval(status: ApprovalStatus, allowance: Decimal) -> ApprovalState {
    ApprovalState {
        token_id: TokenId::from("token-yes"),
        spender: "0xspender".to_owned(),
        owner_address: "0xowner".to_owned(),
        funder_address: "0xfunder".to_owned(),
        wallet_route: WalletRoute::Eoa,
        signature_type: SignatureType::Eoa,
        allowance,
        required_min_allowance: Decimal::new(50, 0),
        last_checked_at: Utc::now(),
        approval_status: status,
    }
}

fn sample_freshness_policy() -> FreshnessPolicy {
    FreshnessPolicy {
        max_age: Duration::seconds(30),
    }
}

fn sample_thresholds() -> FullSetRiskThresholds {
    FullSetRiskThresholds {
        min_net_edge_usdc: Decimal::new(5, 2),
    }
}

fn sample_resolution(dispute_state: DisputeState) -> ResolutionState {
    ResolutionState {
        condition_id: ConditionId::from("condition-1"),
        resolution_status: ResolutionStatus::Resolved,
        payout_vector: vec![Decimal::new(1, 0), Decimal::new(0, 0)],
        resolved_at: Some(Utc::now()),
        dispute_state,
        redeemable_at: Some(Utc::now()),
    }
}
