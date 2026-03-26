use std::collections::BTreeMap;

use app_live::{
    AppSupervisor, NegRiskFamilyLiveTarget, NegRiskLiveArtifact, NegRiskLiveExecutionRecord,
    NegRiskLiveStateSource, NegRiskMemberLiveTarget,
};
use domain::{ExecutionMode, RuntimeMode};
use rust_decimal::Decimal;
use serde_json::json;
use state::PendingReconcileAnchor;

#[test]
fn startup_restores_seeded_live_execution_records_before_fresh_live_submit() {
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.seed_neg_risk_live_targets(BTreeMap::from([(
        "family-a".to_owned(),
        sample_live_target("family-a"),
    )]));
    supervisor.seed_neg_risk_live_approval("family-a");
    supervisor.seed_neg_risk_live_ready_family("family-a");
    supervisor.seed_runtime_progress(0, 0, Some("snapshot-0"));
    supervisor.seed_committed_state_version(0);
    supervisor.seed_pending_reconcile_count(1);
    supervisor.seed_pending_reconcile_anchor(PendingReconcileAnchor::new(
        "pending-family-a-1",
        "submission-family-a-1",
        "family-a",
        "neg-risk",
        "accepted but unconfirmed on restore",
    ));
    supervisor.seed_neg_risk_live_execution_record(sample_live_execution_record("snapshot-0"));

    let summary = supervisor.run_once().unwrap();

    assert_eq!(summary.runtime_mode, RuntimeMode::Reconciling);
    assert_eq!(summary.pending_reconcile_count, 1);
    assert_eq!(summary.neg_risk_live_attempt_count, 1);
    assert_eq!(summary.negrisk_mode, ExecutionMode::Live);
    assert_eq!(
        summary.neg_risk_live_state_source,
        NegRiskLiveStateSource::DurableRestore
    );
}

#[test]
fn startup_restores_pending_reconcile_anchor_before_fresh_live_submit() {
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.seed_neg_risk_live_targets(BTreeMap::from([(
        "family-a".to_owned(),
        sample_live_target("family-a"),
    )]));
    supervisor.seed_neg_risk_live_approval("family-a");
    supervisor.seed_neg_risk_live_ready_family("family-a");
    supervisor.seed_runtime_progress(0, 0, Some("snapshot-0"));
    supervisor.seed_committed_state_version(0);
    supervisor.seed_pending_reconcile_count(1);
    supervisor.seed_pending_reconcile_anchor(PendingReconcileAnchor::new(
        "pending-family-a-1",
        "submission-family-a-1",
        "family-a",
        "neg-risk",
        "accepted but unconfirmed on restore",
    ));

    let summary = supervisor.run_once().unwrap();

    assert_eq!(summary.runtime_mode, RuntimeMode::Reconciling);
    assert_eq!(summary.pending_reconcile_count, 1);
    assert_eq!(summary.neg_risk_live_attempt_count, 0);
    assert_eq!(summary.negrisk_mode, ExecutionMode::Shadow);
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

fn sample_live_execution_record(snapshot_id: &str) -> NegRiskLiveExecutionRecord {
    NegRiskLiveExecutionRecord {
        attempt_id: "attempt-family-a-1".to_owned(),
        plan_id: "negrisk-submit-family:family-a:condition-1:token-1:0.45:10".to_owned(),
        snapshot_id: snapshot_id.to_owned(),
        execution_mode: ExecutionMode::Live,
        attempt_no: 1,
        idempotency_key: "idem-attempt-family-a-1".to_owned(),
        route: "neg-risk".to_owned(),
        scope: "family-a".to_owned(),
        matched_rule_id: Some("family-a-live".to_owned()),
        submission_ref: Some("submission-family-a-1".to_owned()),
        pending_ref: Some("pending-family-a-1".to_owned()),
        artifacts: vec![NegRiskLiveArtifact {
            stream: "neg-risk-live-orders".to_owned(),
            payload: json!({
                "submission_ref": "submission-family-a-1",
            }),
        }],
        order_requests: vec![json!({
            "submission_ref": "submission-family-a-1",
        })],
    }
}
