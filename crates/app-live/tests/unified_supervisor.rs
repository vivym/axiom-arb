use app_live::AppSupervisor;
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
    assert_eq!(dispatched.last_stable_state_version, Some(8));
    assert_eq!(dispatched.fullset_last_ready_state_version, Some(7));
    assert_eq!(dispatched.negrisk_last_ready_state_version, Some(8));
    assert_eq!(
        dispatched.last_stable_snapshot_id.as_deref(),
        Some("snapshot-8")
    );
}
