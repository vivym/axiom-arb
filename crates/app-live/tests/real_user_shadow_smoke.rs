use std::collections::BTreeMap;

use app_live::{AppSupervisor, NegRiskFamilyLiveTarget, NegRiskMemberLiveTarget};
use domain::{ExecutionMode, RuntimeMode};
use rust_decimal::Decimal;

#[test]
fn smoke_guard_turns_live_eligible_family_into_shadow_attempt_and_never_live_attempt() {
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.enable_real_user_shadow_smoke();
    supervisor.seed_neg_risk_live_targets(BTreeMap::from([(
        "family-a".to_owned(),
        sample_live_target("family-a"),
    )]));
    supervisor.seed_neg_risk_live_approval("family-a");
    supervisor.seed_neg_risk_live_ready_family("family-a");

    let summary = supervisor.run_once().unwrap();

    assert_eq!(summary.negrisk_mode, ExecutionMode::Shadow);
    assert_eq!(summary.neg_risk_live_attempt_count, 0);
    assert_eq!(summary.runtime_mode, RuntimeMode::Healthy);
    assert_eq!(summary.pending_reconcile_count, 0);
    assert!(supervisor.neg_risk_live_execution_records().is_empty());

    let attempts = supervisor.neg_risk_shadow_execution_attempts();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].execution_mode, ExecutionMode::Shadow);
    assert_eq!(attempts[0].route, "neg-risk");
    assert_eq!(attempts[0].scope, "family-a");
    assert_eq!(
        attempts[0].matched_rule_id.as_deref(),
        Some("family-a-live")
    );

    let artifacts = supervisor.neg_risk_shadow_execution_artifacts();
    assert!(!artifacts.is_empty());
    assert!(artifacts
        .iter()
        .all(|artifact| artifact.attempt_id == attempts[0].attempt_id));
}

fn sample_live_target(family_id: &str) -> NegRiskFamilyLiveTarget {
    NegRiskFamilyLiveTarget {
        family_id: family_id.to_owned(),
        members: vec![NegRiskMemberLiveTarget {
            condition_id: "condition-1".to_owned(),
            token_id: "token-1".to_owned(),
            price: Decimal::new(45, 2),
            quantity: Decimal::new(10, 0),
        }],
    }
}
