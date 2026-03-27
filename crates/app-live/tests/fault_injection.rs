use chrono::Utc;

use app_live::{AppSupervisor, InputTaskEvent, NegRiskRolloutEvidence};
use domain::{ExternalFactEvent, RuntimeMode};

#[test]
fn restart_with_empty_durable_history_rebuilds_baseline_snapshot_anchor() {
    let mut supervisor = AppSupervisor::for_tests();

    let resumed = supervisor.resume_once().unwrap();

    assert_eq!(resumed.last_journal_seq, 0);
    assert_eq!(resumed.last_state_version, 0);
    assert_eq!(resumed.runtime_mode, RuntimeMode::Healthy);
    assert_eq!(resumed.pending_reconcile_count, 0);
    assert_eq!(resumed.published_snapshot_id.as_deref(), Some("snapshot-0"));
    assert_eq!(resumed.published_snapshot_committed_journal_seq, Some(0));
    assert!(supervisor.can_resume_ingest_loops());
}

#[test]
fn restart_resumes_from_durable_journal_state_snapshot_anchors() {
    let mut supervisor = AppSupervisor::for_tests();
    for journal_seq in 35..=41 {
        supervisor.seed_committed_input(sample_input_task_event(journal_seq));
    }
    supervisor.seed_runtime_progress(41, 7, None);
    supervisor.seed_committed_state_version(7);
    supervisor.seed_neg_risk_rollout_evidence(sample_rollout_evidence("snapshot-7"));

    let resumed = supervisor.resume_once().unwrap();

    assert_eq!(resumed.last_journal_seq, 41);
    assert_eq!(resumed.last_state_version, 7);
    assert_eq!(resumed.runtime_mode, RuntimeMode::Healthy);
    assert_eq!(resumed.pending_reconcile_count, 0);
    assert_eq!(resumed.published_snapshot_id.as_deref(), Some("snapshot-7"));
    assert_eq!(resumed.published_snapshot_committed_journal_seq, Some(41));
}

#[test]
fn restart_republishes_stale_snapshot_anchor_before_dispatch_resumes() {
    let mut supervisor = AppSupervisor::for_tests();
    for journal_seq in 35..=41 {
        supervisor.seed_committed_input(sample_input_task_event(journal_seq));
    }
    supervisor.seed_runtime_progress(41, 7, Some("snapshot-6"));
    supervisor.seed_committed_state_version(7);
    supervisor.seed_neg_risk_rollout_evidence(sample_rollout_evidence("snapshot-7"));

    let resumed = supervisor.resume_once().unwrap();

    assert_eq!(resumed.last_journal_seq, 41);
    assert_eq!(resumed.last_state_version, 7);
    assert_eq!(resumed.runtime_mode, RuntimeMode::Healthy);
    assert_eq!(resumed.pending_reconcile_count, 0);
    assert_eq!(resumed.published_snapshot_id.as_deref(), Some("snapshot-7"));
    assert_eq!(resumed.published_snapshot_committed_journal_seq, Some(41));
}

#[test]
fn restart_replays_unapplied_journal_entries_before_dispatch_resumes() {
    let mut supervisor = AppSupervisor::for_tests();
    for journal_seq in 36..=41 {
        supervisor.seed_committed_input(sample_input_task_event(journal_seq));
    }
    supervisor.seed_runtime_progress(41, 6, Some("snapshot-6"));
    supervisor.seed_unapplied_journal_entry(42, sample_input_task_event(42));
    supervisor.seed_neg_risk_rollout_evidence(sample_rollout_evidence("snapshot-6"));

    let resumed = supervisor.resume_once().unwrap();

    assert_eq!(resumed.last_journal_seq, 42);
    assert_eq!(resumed.last_state_version, 7);
    assert_eq!(resumed.runtime_mode, RuntimeMode::Healthy);
    assert_eq!(resumed.pending_reconcile_count, 0);
    assert_eq!(resumed.published_snapshot_id.as_deref(), Some("snapshot-7"));
    assert_eq!(resumed.published_snapshot_committed_journal_seq, Some(42));
    assert_eq!(
        resumed
            .neg_risk_rollout_evidence
            .as_ref()
            .map(|evidence| evidence.snapshot_id.as_str()),
        Some("snapshot-7")
    );
}

#[test]
fn restart_replays_out_of_order_user_trade_from_durable_log() {
    let mut supervisor = AppSupervisor::for_tests();
    for journal_seq in 36..=41 {
        supervisor.seed_committed_input(sample_input_task_event(journal_seq));
    }
    supervisor.seed_runtime_progress(41, 6, Some("snapshot-6"));
    supervisor.seed_unapplied_journal_entry(42, sample_out_of_order_user_trade(42));
    supervisor.seed_neg_risk_rollout_evidence(sample_rollout_evidence("snapshot-6"));

    let resumed = supervisor.resume_once().unwrap();

    assert_eq!(resumed.last_journal_seq, 42);
    assert_eq!(resumed.last_state_version, 6);
    assert_eq!(resumed.runtime_mode, RuntimeMode::Reconciling);
    assert_eq!(resumed.pending_reconcile_count, 1);
    assert_eq!(resumed.published_snapshot_id.as_deref(), Some("snapshot-6"));
    assert_eq!(resumed.published_snapshot_committed_journal_seq, Some(41));
    assert!(!supervisor.can_resume_ingest_loops());

    supervisor.seed_runtime_progress(
        resumed.last_journal_seq,
        resumed.last_state_version,
        resumed.published_snapshot_id.as_deref(),
    );
    supervisor.seed_committed_state_version(resumed.last_state_version);
    supervisor.seed_pending_reconcile_count(resumed.pending_reconcile_count);
    supervisor.seed_neg_risk_rollout_evidence(
        resumed
            .neg_risk_rollout_evidence
            .clone()
            .expect("resumed supervisor should rebuild rollout evidence"),
    );

    let replayed = supervisor.resume_once().unwrap();

    assert_eq!(replayed.last_journal_seq, 42);
    assert_eq!(replayed.last_state_version, 6);
    assert_eq!(replayed.runtime_mode, RuntimeMode::Reconciling);
    assert_eq!(replayed.pending_reconcile_count, 1);
    assert_eq!(
        replayed.published_snapshot_id.as_deref(),
        Some("snapshot-6")
    );
    assert_eq!(replayed.published_snapshot_committed_journal_seq, Some(41));
}

#[test]
fn restart_retains_failed_backlog_entries_for_retry() {
    let mut supervisor = AppSupervisor::for_tests();
    for journal_seq in 36..=41 {
        supervisor.seed_committed_input(sample_input_task_event(journal_seq));
    }
    supervisor.seed_runtime_progress(41, 6, Some("snapshot-6"));
    supervisor.seed_unapplied_journal_entry(42, sample_input_task_event(42));
    supervisor.seed_unapplied_journal_entry(42, conflicting_input_task_event(42));
    supervisor.seed_neg_risk_rollout_evidence(sample_rollout_evidence("snapshot-6"));

    let first_err = supervisor.resume_once().unwrap_err();
    assert!(first_err
        .to_string()
        .contains("journal sequence 42 is already bound to a different fact"));
    assert_eq!(supervisor.pending_input_count(), 1);
}

#[test]
fn restart_preserves_cleared_pending_reconcile_count_after_successful_reconcile() {
    let mut supervisor = AppSupervisor::for_tests();
    for journal_seq in 36..=41 {
        supervisor.seed_committed_input(sample_input_task_event(journal_seq));
    }
    supervisor.seed_committed_input(sample_out_of_order_user_trade(42));
    supervisor.seed_runtime_progress(42, 6, Some("snapshot-6"));
    supervisor.seed_committed_state_version(6);
    supervisor.seed_pending_reconcile_count(0);
    supervisor.seed_neg_risk_rollout_evidence(sample_rollout_evidence("snapshot-6"));

    let resumed = supervisor.resume_once().unwrap();

    assert_eq!(resumed.last_journal_seq, 42);
    assert_eq!(resumed.last_state_version, 6);
    assert_eq!(resumed.runtime_mode, RuntimeMode::Healthy);
    assert_eq!(resumed.pending_reconcile_count, 0);
    assert_eq!(resumed.published_snapshot_id.as_deref(), Some("snapshot-6"));
    assert_eq!(resumed.published_snapshot_committed_journal_seq, Some(41));
}

#[test]
fn restart_requires_durable_pending_reconcile_count_when_history_contains_follow_up_work() {
    let mut supervisor = AppSupervisor::for_tests();
    for journal_seq in 36..=41 {
        supervisor.seed_committed_input(sample_input_task_event(journal_seq));
    }
    supervisor.seed_committed_input(sample_out_of_order_user_trade(42));
    supervisor.seed_runtime_progress(42, 6, Some("snapshot-6"));
    supervisor.seed_committed_state_version(6);

    let err = supervisor.resume_once().unwrap_err();

    assert!(err
        .to_string()
        .contains("durable pending reconcile count is required"));
}

fn sample_input_task_event(journal_seq: i64) -> InputTaskEvent {
    InputTaskEvent::new(
        journal_seq,
        ExternalFactEvent::new(
            "market_ws",
            "session-1",
            format!("evt-{journal_seq}"),
            "v1",
            Utc::now(),
        ),
    )
}

fn sample_out_of_order_user_trade(journal_seq: i64) -> InputTaskEvent {
    InputTaskEvent::out_of_order_user_trade(
        journal_seq,
        ExternalFactEvent::new(
            "market_ws",
            "session-2",
            format!("trade-{journal_seq}"),
            "v1",
            Utc::now(),
        ),
    )
}

fn conflicting_input_task_event(journal_seq: i64) -> InputTaskEvent {
    InputTaskEvent::new(
        journal_seq,
        ExternalFactEvent::new(
            "market_ws",
            "session-conflict",
            format!("evt-conflict-{journal_seq}"),
            "v1",
            Utc::now(),
        ),
    )
}

fn sample_rollout_evidence(snapshot_id: &str) -> NegRiskRolloutEvidence {
    NegRiskRolloutEvidence {
        snapshot_id: snapshot_id.to_owned(),
        live_ready_family_count: 0,
        blocked_family_count: 0,
        parity_mismatch_count: 0,
    }
}
