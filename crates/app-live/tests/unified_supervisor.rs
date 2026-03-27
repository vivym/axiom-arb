use app_live::{AppSupervisor, SupervisorPosture};
use domain::ExecutionMode;

#[test]
fn supervisor_bootstraps_fullset_live_and_negrisk_shadow_modes() {
    let result = AppSupervisor::for_tests().run_once().unwrap();

    assert_eq!(result.fullset_mode, ExecutionMode::Live);
    assert_eq!(result.negrisk_mode, ExecutionMode::Shadow);
}

#[test]
fn dispatcher_coalesces_dirty_snapshots_without_dropping_latest_version() {
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.push_dirty_snapshot(7, true, false);
    supervisor.push_dirty_snapshot(8, false, true);
    supervisor.push_dirty_snapshot(9, false, false);

    let dispatched = supervisor.flush_dispatch();

    assert_eq!(dispatched.coalesced_versions, vec![9]);
    assert_eq!(dispatched.last_stable_state_version, Some(7));
    assert_eq!(dispatched.fullset_last_ready_state_version, Some(7));
    assert_eq!(dispatched.negrisk_last_ready_state_version, Some(8));
    assert_eq!(
        dispatched.fullset_last_ready_snapshot_id.as_deref(),
        Some("snapshot-7")
    );
    assert_eq!(
        dispatched.negrisk_last_ready_snapshot_id.as_deref(),
        Some("snapshot-8")
    );
    assert_eq!(
        dispatched.last_stable_snapshot_id.as_deref(),
        Some("snapshot-7")
    );
}

#[test]
fn dispatcher_retains_only_latest_negrisk_dirty_work_when_fullset_keeps_publishing() {
    let mut supervisor = AppSupervisor::for_tests();

    supervisor.push_dirty_snapshot(7, true, false);
    let first = supervisor.flush_dispatch();
    assert_eq!(first.coalesced_versions, vec![7]);
    assert_eq!(first.last_stable_state_version, None);

    supervisor.push_dirty_snapshot(8, true, false);
    let second = supervisor.flush_dispatch();
    assert_eq!(second.coalesced_versions, vec![8]);
    assert_eq!(second.fullset_last_ready_state_version, Some(8));
    assert_eq!(second.negrisk_last_ready_state_version, None);
    assert_eq!(second.last_stable_state_version, None);

    let repeated = supervisor.flush_dispatch();
    assert_eq!(repeated.coalesced_versions, vec![8]);
    assert_eq!(repeated.last_stable_state_version, None);

    supervisor.push_dirty_snapshot(9, true, false);
    let third = supervisor.flush_dispatch();
    assert_eq!(third.coalesced_versions, vec![9]);
    assert_eq!(third.fullset_last_ready_state_version, Some(9));
    assert_eq!(third.negrisk_last_ready_state_version, None);
    assert_eq!(third.last_stable_state_version, None);
}

#[test]
fn dispatcher_stability_uses_cross_projection_ready_watermark() {
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.push_dirty_snapshot(7, true, true);
    supervisor.push_dirty_snapshot(8, true, false);
    supervisor.push_dirty_snapshot(9, false, true);

    let dispatched = supervisor.flush_dispatch();

    assert_eq!(dispatched.fullset_last_ready_state_version, Some(8));
    assert_eq!(dispatched.negrisk_last_ready_state_version, Some(9));
    assert_eq!(dispatched.last_stable_state_version, Some(8));
    assert_eq!(
        dispatched.last_stable_snapshot_id.as_deref(),
        Some("snapshot-8")
    );
}

#[test]
fn supervisor_starts_with_healthy_global_posture() {
    let supervisor = AppSupervisor::for_tests();

    assert_eq!(supervisor.posture(), SupervisorPosture::Healthy);
}

#[test]
fn supervisor_summary_surfaces_candidate_restore_status() {
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.seed_candidate_restore_status(
        Some("candidate-11"),
        Some("adoptable-11"),
        Some("targets-rev-11"),
        false,
    );

    let summary = supervisor.run_once().unwrap();

    assert_eq!(
        summary.latest_candidate_revision.as_deref(),
        Some("candidate-11")
    );
    assert_eq!(
        summary.latest_adoptable_revision.as_deref(),
        Some("adoptable-11")
    );
    assert_eq!(
        summary.latest_candidate_operator_target_revision.as_deref(),
        Some("targets-rev-11")
    );
    assert!(!summary.adoption_provenance_resolved);
}

#[test]
fn seeded_restore_requires_durable_journal_anchor() {
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.seed_committed_state_version(7);
    supervisor.seed_pending_reconcile_count(0);

    let err = supervisor
        .run_once()
        .expect_err("partial restore seed should fail closed without last_journal_seq");

    assert!(
        err.to_string()
            .contains("durable last journal sequence is required"),
        "{err}"
    );
}
