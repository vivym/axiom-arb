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
    supervisor.push_dirty_snapshot(7);
    supervisor.push_dirty_snapshot(8);
    supervisor.push_dirty_snapshot(9);

    let dispatched = supervisor.flush_dispatch();

    assert_eq!(dispatched.last_dispatched_state_version, Some(9));
    assert_eq!(dispatched.coalesced_versions, vec![7, 8, 9]);
}
