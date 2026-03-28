use std::collections::BTreeMap;

use app_live::{AppSupervisor, NegRiskFamilyLiveTarget, NegRiskMemberLiveTarget};
use domain::{ExecutionMode, RuntimeMode};
use persistence::models::{ExecutionAttemptRow, ShadowExecutionArtifactRow};
use rust_decimal::Decimal;
use serde_json::json;

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
    assert!(summary.real_user_shadow_smoke);
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

#[test]
fn ordinary_live_startup_does_not_report_real_user_shadow_smoke() {
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.seed_neg_risk_live_targets(BTreeMap::from([(
        "family-a".to_owned(),
        sample_live_target("family-a"),
    )]));
    supervisor.seed_neg_risk_live_approval("family-a");
    supervisor.seed_neg_risk_live_ready_family("family-a");

    let summary = supervisor.run_once().unwrap();

    assert_eq!(summary.negrisk_mode, ExecutionMode::Live);
    assert!(!summary.real_user_shadow_smoke);
}

#[test]
fn smoke_restore_fails_when_snapshot_filter_retains_no_shadow_execution_records() {
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.enable_real_user_shadow_smoke();
    supervisor.seed_runtime_progress(0, 0, Some("snapshot-0"));
    supervisor.seed_committed_state_version(0);
    supervisor.seed_pending_reconcile_count(0);
    supervisor.seed_neg_risk_rollout_evidence(app_live::NegRiskRolloutEvidence {
        snapshot_id: "snapshot-0".to_owned(),
        live_ready_family_count: 0,
        blocked_family_count: 0,
        parity_mismatch_count: 0,
    });
    supervisor.seed_neg_risk_shadow_execution_attempt(ExecutionAttemptRow {
        attempt_id: "attempt-7".to_owned(),
        plan_id: "plan-7".to_owned(),
        snapshot_id: "snapshot-1".to_owned(),
        route: "neg-risk".to_owned(),
        scope: "family-a".to_owned(),
        matched_rule_id: Some("family-a-live".to_owned()),
        execution_mode: ExecutionMode::Shadow,
        attempt_no: 1,
        idempotency_key: "idem-attempt-7".to_owned(),
    });
    supervisor.seed_neg_risk_shadow_execution_artifact(ShadowExecutionArtifactRow {
        attempt_id: "attempt-7".to_owned(),
        stream: "neg-risk-shadow-plan".to_owned(),
        payload: json!({
            "attempt_id": "attempt-7",
            "plan_id": "plan-7",
            "snapshot_id": "snapshot-1",
            "route": "neg-risk",
            "scope": "family-a",
            "matched_rule_id": "family-a-live"
        }),
    });

    let err = supervisor.resume_once().expect_err(
        "smoke restore should fail closed when snapshot filtering removes all shadow records",
    );

    assert!(
        err.to_string()
            .contains("retained shadow execution records"),
        "{err}"
    );
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
