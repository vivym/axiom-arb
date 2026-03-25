use std::collections::BTreeMap;

use app_live::{AppSupervisor, NegRiskFamilyLiveTarget, NegRiskMemberLiveTarget};
use domain::ExecutionMode;
use rust_decimal::Decimal;

#[test]
fn live_ready_family_with_config_and_live_approval_promotes_negrisk_summary_to_live() {
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.seed_neg_risk_live_targets(BTreeMap::from([(
        "family-a".to_owned(),
        sample_live_target("family-a"),
    )]));
    supervisor.seed_neg_risk_live_approval("family-a");
    supervisor.seed_neg_risk_live_ready_family("family-a");

    let summary = supervisor.run_once().unwrap();

    assert_eq!(summary.negrisk_mode, ExecutionMode::Live);
    assert_eq!(summary.neg_risk_live_attempt_count, 1);
    assert_eq!(
        summary
            .neg_risk_rollout_evidence
            .as_ref()
            .map(|evidence| evidence.live_ready_family_count),
        Some(1)
    );
}

#[test]
fn config_backed_family_without_live_approval_stays_shadow_and_emits_no_live_attempts() {
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.seed_neg_risk_live_targets(BTreeMap::from([(
        "family-a".to_owned(),
        sample_live_target("family-a"),
    )]));
    supervisor.seed_neg_risk_live_ready_family("family-a");

    let summary = supervisor.run_once().unwrap();

    assert_eq!(summary.negrisk_mode, ExecutionMode::Shadow);
    assert_eq!(summary.neg_risk_live_attempt_count, 0);
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
