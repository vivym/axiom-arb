use std::collections::BTreeMap;

use app_live::{
    AppSupervisor, NegRiskFamilyLiveTarget, NegRiskLiveArtifact, NegRiskLiveExecutionRecord,
    NegRiskLiveStateSource, NegRiskMemberLiveTarget, NegRiskRolloutEvidence,
};
use chrono::Utc;
use domain::{ExecutionMode, ExternalFactEvent};
use rust_decimal::Decimal;
use serde_json::json;

#[test]
fn live_ready_family_with_config_and_live_approval_records_real_attempt_artifacts_and_requests() {
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
        summary.neg_risk_live_state_source,
        NegRiskLiveStateSource::SyntheticBootstrap
    );
    assert_eq!(
        summary
            .neg_risk_rollout_evidence
            .as_ref()
            .map(|evidence| evidence.live_ready_family_count),
        Some(1)
    );
    let records = supervisor.neg_risk_live_execution_records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].execution_mode, ExecutionMode::Live);
    assert_eq!(records[0].route, "neg-risk");
    assert_eq!(records[0].scope, "family-a");
    assert_eq!(records[0].matched_rule_id.as_deref(), Some("family-a-live"));
    assert_eq!(records[0].artifacts.len(), 1);
    assert_eq!(records[0].artifacts[0].stream, "neg-risk-live-orders");
    assert_eq!(records[0].order_requests.len(), 1);
    assert_eq!(records[0].order_requests[0]["order"]["tokenId"], "token-1");
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
    assert_eq!(
        summary.neg_risk_live_state_source,
        NegRiskLiveStateSource::None
    );
    assert_eq!(
        summary
            .neg_risk_rollout_evidence
            .as_ref()
            .map(|evidence| evidence.live_ready_family_count),
        Some(0)
    );
    assert_eq!(
        summary
            .neg_risk_rollout_evidence
            .as_ref()
            .map(|evidence| evidence.blocked_family_count),
        Some(1)
    );
}

#[test]
fn resume_does_not_require_live_attempt_anchors_for_ready_but_unapproved_families() {
    let mut boot = AppSupervisor::for_tests();
    boot.seed_neg_risk_live_targets(BTreeMap::from([(
        "family-a".to_owned(),
        sample_live_target("family-a"),
    )]));
    boot.seed_neg_risk_live_ready_family("family-a");

    let boot_summary = boot.run_once().unwrap();

    let mut resumed = AppSupervisor::for_tests();
    resumed.seed_neg_risk_live_targets(BTreeMap::from([(
        "family-a".to_owned(),
        sample_live_target("family-a"),
    )]));
    resumed.seed_neg_risk_live_ready_family("family-a");
    resumed.seed_runtime_progress(
        boot_summary.last_journal_seq,
        boot_summary.last_state_version,
        boot_summary.published_snapshot_id.as_deref(),
    );
    resumed.seed_committed_state_version(boot_summary.last_state_version);
    resumed.seed_pending_reconcile_count(boot_summary.pending_reconcile_count);
    resumed.seed_neg_risk_rollout_evidence(
        boot_summary
            .neg_risk_rollout_evidence
            .clone()
            .expect("boot summary should include rollout evidence"),
    );

    let resumed_summary = resumed.resume_once().unwrap();

    assert_eq!(resumed_summary.negrisk_mode, ExecutionMode::Shadow);
    assert_eq!(resumed_summary.neg_risk_live_attempt_count, 0);
    assert_eq!(
        resumed_summary.neg_risk_live_state_source,
        NegRiskLiveStateSource::None
    );
}

#[test]
fn resume_does_not_fabricate_rollout_evidence_from_operator_sets() {
    let mut supervisor = AppSupervisor::for_tests();
    for journal_seq in 36..=41 {
        supervisor.seed_committed_input(sample_input_task_event(journal_seq));
    }
    supervisor.seed_neg_risk_live_targets(BTreeMap::from([(
        "family-a".to_owned(),
        sample_live_target("family-a"),
    )]));
    supervisor.seed_neg_risk_live_approval("family-a");
    supervisor.seed_neg_risk_live_ready_family("family-a");
    supervisor.seed_runtime_progress(41, 6, Some("snapshot-6"));
    supervisor.seed_committed_state_version(6);
    supervisor.seed_pending_reconcile_count(0);

    let err = supervisor.resume_once().unwrap_err();

    assert!(err.to_string().contains("rollout gate evidence"), "{err}");
}

#[test]
fn resume_requires_durable_live_execution_records_before_live_promotion() {
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.seed_neg_risk_live_targets(BTreeMap::from([(
        "family-a".to_owned(),
        sample_live_target("family-a"),
    )]));
    supervisor.seed_neg_risk_live_approval("family-a");
    supervisor.seed_neg_risk_live_ready_family("family-a");
    supervisor.seed_runtime_progress(0, 0, Some("snapshot-0"));
    supervisor.seed_committed_state_version(0);
    supervisor.seed_pending_reconcile_count(0);
    supervisor.seed_neg_risk_rollout_evidence(NegRiskRolloutEvidence {
        snapshot_id: "snapshot-0".to_owned(),
        live_ready_family_count: 1,
        blocked_family_count: 0,
        parity_mismatch_count: 0,
    });

    let err = supervisor.resume_once().unwrap_err();

    assert!(err.to_string().contains("live attempt"), "{err}");
}

#[test]
fn resume_restores_seeded_live_execution_records_without_reexecuting() {
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.seed_neg_risk_live_targets(BTreeMap::from([(
        "family-a".to_owned(),
        sample_live_target("family-a"),
    )]));
    supervisor.seed_neg_risk_live_approval("family-a");
    supervisor.seed_neg_risk_live_ready_family("family-a");
    supervisor.seed_runtime_progress(0, 0, Some("snapshot-0"));
    supervisor.seed_committed_state_version(0);
    supervisor.seed_pending_reconcile_count(0);
    supervisor.seed_neg_risk_rollout_evidence(NegRiskRolloutEvidence {
        snapshot_id: "snapshot-0".to_owned(),
        live_ready_family_count: 1,
        blocked_family_count: 0,
        parity_mismatch_count: 0,
    });
    supervisor.seed_neg_risk_live_execution_record(sample_live_execution_record("snapshot-0"));

    let summary = supervisor.resume_once().unwrap();

    assert_eq!(summary.negrisk_mode, ExecutionMode::Live);
    assert_eq!(summary.neg_risk_live_attempt_count, 1);
    assert_eq!(
        summary.neg_risk_live_state_source,
        NegRiskLiveStateSource::DurableRestore
    );
    let records = supervisor.neg_risk_live_execution_records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].attempt_id, "attempt-family-a-1");
    assert_eq!(records[0].order_requests.len(), 1);
}

#[test]
fn resume_discards_stale_live_execution_records_after_snapshot_advances() {
    let mut supervisor = AppSupervisor::for_tests();
    for journal_seq in 36..=41 {
        supervisor.seed_committed_input(sample_input_task_event(journal_seq));
    }
    supervisor.seed_runtime_progress(41, 6, Some("snapshot-6"));
    supervisor.seed_committed_state_version(6);
    supervisor.seed_pending_reconcile_count(0);
    supervisor.seed_neg_risk_rollout_evidence(NegRiskRolloutEvidence {
        snapshot_id: "snapshot-6".to_owned(),
        live_ready_family_count: 1,
        blocked_family_count: 0,
        parity_mismatch_count: 0,
    });
    supervisor.seed_neg_risk_live_execution_record(sample_live_execution_record("snapshot-6"));
    supervisor.seed_unapplied_journal_entry(42, sample_input_task_event(42));

    let summary = supervisor.resume_once().unwrap();

    assert_eq!(summary.published_snapshot_id.as_deref(), Some("snapshot-7"));
    assert_eq!(summary.negrisk_mode, ExecutionMode::Shadow);
    assert_eq!(summary.neg_risk_live_attempt_count, 0);
    assert_eq!(
        summary.neg_risk_live_state_source,
        NegRiskLiveStateSource::None
    );
    assert!(supervisor.neg_risk_live_execution_records().is_empty());
}

fn sample_input_task_event(journal_seq: i64) -> app_live::InputTaskEvent {
    app_live::InputTaskEvent::new(
        journal_seq,
        ExternalFactEvent::new(
            "market_ws",
            "session-negrisk-live",
            format!("evt-{journal_seq}"),
            "v1",
            Utc::now(),
        ),
    )
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
        artifacts: vec![NegRiskLiveArtifact {
            stream: "neg-risk-live-orders".to_owned(),
            payload: json!({
                "requests": [
                    {
                        "order": {
                            "tokenId": "token-1"
                        }
                    }
                ]
            }),
        }],
        order_requests: vec![json!({
            "order": {
                "tokenId": "token-1"
            }
        })],
    }
}
