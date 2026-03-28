use app_live::{AppDaemon, AppSupervisor, NegRiskRolloutEvidence};
use state::PendingReconcileAnchor;

#[test]
fn daemon_startup_restores_truth_before_resuming_ingest_loops() {
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.seed_runtime_progress(41, 7, Some("snapshot-7"));
    supervisor.seed_committed_state_version(7);
    supervisor.seed_pending_reconcile_count(0);
    supervisor.seed_neg_risk_rollout_evidence(NegRiskRolloutEvidence {
        snapshot_id: "snapshot-7".to_owned(),
        live_ready_family_count: 0,
        blocked_family_count: 0,
        parity_mismatch_count: 0,
    });

    let mut daemon = AppDaemon::for_tests(supervisor);
    let report = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("test runtime")
        .block_on(async { daemon.run_until_idle_for_tests(3).await })
        .expect("daemon should run");

    assert_eq!(
        report.startup_order,
        vec!["restore", "state", "decision", "ingest"]
    );
    assert_eq!(report.ticks_run, 1);
    assert!(report.idle_reached);
}

#[test]
fn daemon_startup_reports_seeded_candidate_restore_status() {
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.seed_runtime_progress(41, 7, Some("snapshot-7"));
    supervisor.seed_committed_state_version(7);
    supervisor.seed_pending_reconcile_count(0);
    supervisor.seed_candidate_restore_status(
        Some("candidate-7"),
        Some("adoptable-7"),
        Some("targets-rev-7"),
        true,
    );

    let mut daemon = AppDaemon::for_tests(supervisor);
    let report = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("test runtime")
        .block_on(async { daemon.run_until_idle_for_tests(1).await })
        .expect("daemon should run");

    assert_eq!(
        report.summary.latest_candidate_revision.as_deref(),
        Some("candidate-7")
    );
    assert_eq!(
        report.summary.latest_adoptable_revision.as_deref(),
        Some("adoptable-7")
    );
    assert_eq!(
        report
            .summary
            .latest_candidate_operator_target_revision
            .as_deref(),
        Some("targets-rev-7")
    );
    assert!(report.summary.adoption_provenance_resolved);
    assert_eq!(report.ticks_run, 1);
    assert!(report.idle_reached);
}

#[test]
fn daemon_run_until_shutdown_loops_until_no_more_progress_is_possible() {
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.seed_runtime_progress(41, 7, Some("snapshot-7"));
    supervisor.seed_committed_state_version(7);
    supervisor.seed_pending_reconcile_count(1);
    supervisor.seed_pending_reconcile_anchor(PendingReconcileAnchor::new(
        "pending-7",
        "submission-7",
        "family-7",
        "neg-risk",
        "awaiting_reconcile",
    ));

    let report = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("test runtime")
        .block_on(async { AppDaemon::for_tests(supervisor).run_until_shutdown().await })
        .expect("daemon should stop once no more progress is possible");

    assert_eq!(report.startup_order, vec!["restore", "state", "decision"]);
    assert_eq!(report.summary.pending_reconcile_count, 1);
    assert_eq!(report.ticks_run, 2);
    assert!(!report.idle_reached);
}
