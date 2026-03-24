use chrono::{Duration, Utc};
use domain::{
    ApprovalKey, ApprovalState, ApprovalStatus, ConditionId, DisputeState, ExecutionMode,
    ResolutionState, ResolutionStatus, RuntimeMode, SignatureType, TokenId, WalletRoute,
};
use risk::fullset::{
    evaluate_fullset_trade, evaluate_fullset_trade_in_mode, evaluate_redeem, Freshness,
    FreshnessPolicy, FullSetRiskContext, FullSetRiskThresholds, RedeemRiskContext, RejectReason,
};
use rust_decimal::Decimal;

#[test]
fn mode_not_healthy_is_rejected() {
    let decision = evaluate_fullset_trade(&FullSetRiskContext {
        runtime_mode: RuntimeMode::Reconciling,
        required_approval_keys: vec![sample_approval_key()],
        approvals: vec![sample_approval(
            ApprovalStatus::Approved,
            Decimal::new(100, 0),
        )],
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
        required_approval_keys: vec![sample_approval_key()],
        approvals: vec![sample_approval(
            ApprovalStatus::Approved,
            Decimal::new(49, 0),
        )],
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
        required_approval_keys: vec![sample_approval_key()],
        approvals: vec![sample_approval(
            ApprovalStatus::Missing,
            Decimal::new(100, 0),
        )],
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
        required_approval_keys: vec![sample_approval_key()],
        approvals: vec![sample_approval(
            ApprovalStatus::Approved,
            Decimal::new(100, 0),
        )],
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
        required_approval_keys: vec![sample_approval_key()],
        approvals: vec![sample_approval(
            ApprovalStatus::Approved,
            Decimal::new(100, 0),
        )],
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
fn net_edge_equal_to_threshold_is_rejected() {
    let decision = evaluate_fullset_trade(&FullSetRiskContext {
        runtime_mode: RuntimeMode::Healthy,
        required_approval_keys: vec![sample_approval_key()],
        approvals: vec![sample_approval(
            ApprovalStatus::Approved,
            Decimal::new(100, 0),
        )],
        net_edge_usdc: Decimal::new(5, 2),
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
fn any_insufficient_approval_rejects_trade() {
    let decision = evaluate_fullset_trade(&FullSetRiskContext {
        runtime_mode: RuntimeMode::Healthy,
        required_approval_keys: vec![sample_approval_key(), sample_no_approval_key()],
        approvals: vec![
            sample_approval(ApprovalStatus::Approved, Decimal::new(100, 0)),
            sample_approval(ApprovalStatus::Pending, Decimal::new(100, 0)),
        ],
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
fn missing_required_approval_is_rejected() {
    let decision = evaluate_fullset_trade(&FullSetRiskContext {
        runtime_mode: RuntimeMode::Healthy,
        required_approval_keys: vec![sample_approval_key(), sample_no_approval_key()],
        approvals: vec![sample_approval(
            ApprovalStatus::Approved,
            Decimal::new(100, 0),
        )],
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
fn required_approvals_covering_all_keys_do_not_trigger_approval_rejection() {
    let decision = evaluate_fullset_trade(&FullSetRiskContext {
        runtime_mode: RuntimeMode::Healthy,
        required_approval_keys: vec![sample_approval_key(), sample_no_approval_key()],
        approvals: vec![
            sample_approval(ApprovalStatus::Approved, Decimal::new(100, 0)),
            sample_no_approval(ApprovalStatus::Approved, Decimal::new(100, 0)),
        ],
        net_edge_usdc: Decimal::new(10, 2),
        thresholds: sample_thresholds(),
        freshness: Freshness::fresh(Utc::now()),
        freshness_policy: sample_freshness_policy(),
    });

    assert_ne!(
        decision.reject_reason(),
        Some(RejectReason::ApprovalInsufficient)
    );
}

#[test]
fn redeem_is_rejected_while_condition_is_disputed() {
    let decision = evaluate_redeem(&sample_redeem_context(sample_resolution(
        DisputeState::Disputed,
    )));

    assert_eq!(
        decision.reject_reason(),
        Some(RejectReason::RedeemBlockedByDispute)
    );
}

#[test]
fn redeem_is_rejected_when_freshness_is_stale() {
    let decision = evaluate_redeem(&RedeemRiskContext {
        runtime_mode: RuntimeMode::Healthy,
        resolution: sample_resolution(DisputeState::None),
        freshness: Freshness::fresh(Utc::now() - Duration::seconds(31)),
        freshness_policy: sample_freshness_policy(),
    });

    assert_eq!(decision.reject_reason(), Some(RejectReason::FreshnessStale));
}

#[test]
fn redeem_requires_resolved_state_and_runnable_metadata() {
    for resolution in [
        sample_resolution_with(
            ResolutionStatus::Unresolved,
            Some(Utc::now()),
            Some(Utc::now()),
            vec![Decimal::new(1, 0), Decimal::ZERO],
            DisputeState::None,
        ),
        sample_resolution_with(
            ResolutionStatus::Resolved,
            None,
            Some(Utc::now()),
            vec![Decimal::new(1, 0), Decimal::ZERO],
            DisputeState::None,
        ),
        sample_resolution_with(
            ResolutionStatus::Resolved,
            Some(Utc::now()),
            None,
            vec![Decimal::new(1, 0), Decimal::ZERO],
            DisputeState::None,
        ),
        sample_resolution_with(
            ResolutionStatus::Resolved,
            Some(Utc::now()),
            Some(Utc::now()),
            vec![],
            DisputeState::None,
        ),
    ] {
        let decision = evaluate_redeem(&sample_redeem_context(resolution));
        assert_eq!(
            decision.reject_reason(),
            Some(RejectReason::RedeemNotRunnable)
        );
    }
}

#[test]
fn redeem_rejects_mode_outside_healthy_and_no_new_risk() {
    let decision = evaluate_redeem(&RedeemRiskContext {
        runtime_mode: RuntimeMode::Reconciling,
        resolution: sample_resolution(DisputeState::None),
        freshness: Freshness::fresh(Utc::now()),
        freshness_policy: sample_freshness_policy(),
    });

    assert_eq!(decision.reject_reason(), Some(RejectReason::ModeNotHealthy));
}

#[test]
fn redeem_allows_no_new_risk_when_resolution_is_runnable() {
    let decision = evaluate_redeem(&RedeemRiskContext {
        runtime_mode: RuntimeMode::NoNewRisk,
        resolution: sample_resolution(DisputeState::None),
        freshness: Freshness::fresh(Utc::now()),
        freshness_policy: sample_freshness_policy(),
    });

    assert_eq!(decision.reject_reason(), None);
}

#[test]
fn non_live_modes_are_rejected_by_mode_aware_fullset_entrypoint() {
    let decision = evaluate_fullset_trade_in_mode(
        ExecutionMode::RecoveryOnly,
        &FullSetRiskContext {
            runtime_mode: RuntimeMode::Healthy,
            required_approval_keys: vec![sample_approval_key()],
            approvals: vec![sample_approval(
                ApprovalStatus::Approved,
                Decimal::new(100, 0),
            )],
            net_edge_usdc: Decimal::new(10, 2),
            thresholds: sample_thresholds(),
            freshness: Freshness::fresh(Utc::now()),
            freshness_policy: sample_freshness_policy(),
        },
    );

    assert_eq!(decision.reject_reason(), Some(RejectReason::ModeNotHealthy));
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

fn sample_no_approval(status: ApprovalStatus, allowance: Decimal) -> ApprovalState {
    ApprovalState {
        token_id: TokenId::from("token-no"),
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

fn sample_approval_key() -> ApprovalKey {
    ApprovalKey {
        token_id: TokenId::from("token-yes"),
        spender: "0xspender".to_owned(),
        owner_address: "0xowner".to_owned(),
    }
}

fn sample_no_approval_key() -> ApprovalKey {
    ApprovalKey {
        token_id: TokenId::from("token-no"),
        spender: "0xspender".to_owned(),
        owner_address: "0xowner".to_owned(),
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

fn sample_redeem_context(resolution: ResolutionState) -> RedeemRiskContext {
    RedeemRiskContext {
        runtime_mode: RuntimeMode::Healthy,
        resolution,
        freshness: Freshness::fresh(Utc::now()),
        freshness_policy: sample_freshness_policy(),
    }
}

fn sample_resolution(dispute_state: DisputeState) -> ResolutionState {
    sample_resolution_with(
        ResolutionStatus::Resolved,
        Some(Utc::now()),
        Some(Utc::now()),
        vec![Decimal::new(1, 0), Decimal::new(0, 0)],
        dispute_state,
    )
}

fn sample_resolution_with(
    resolution_status: ResolutionStatus,
    resolved_at: Option<chrono::DateTime<Utc>>,
    redeemable_at: Option<chrono::DateTime<Utc>>,
    payout_vector: Vec<Decimal>,
    dispute_state: DisputeState,
) -> ResolutionState {
    ResolutionState {
        condition_id: ConditionId::from("condition-1"),
        resolution_status,
        payout_vector,
        resolved_at,
        dispute_state,
        redeemable_at,
    }
}
