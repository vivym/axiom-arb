use chrono::Utc;

use app_live::{AppSupervisor, InputTaskEvent};
use domain::ExternalFactEvent;

#[test]
fn restart_resumes_from_durable_journal_state_snapshot_anchors() {
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.seed_runtime_progress(41, 7, None);
    supervisor.seed_committed_state_version(7);

    let resumed = supervisor.resume_once().unwrap();

    assert_eq!(resumed.last_journal_seq, 41);
    assert_eq!(resumed.last_state_version, 7);
    assert_eq!(resumed.published_snapshot_id.as_deref(), Some("snapshot-7"));
    assert_eq!(resumed.published_snapshot_committed_journal_seq, Some(41));
}

#[test]
fn restart_republishes_stale_snapshot_anchor_before_dispatch_resumes() {
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.seed_runtime_progress(41, 7, Some("snapshot-6"));
    supervisor.seed_committed_state_version(7);

    let resumed = supervisor.resume_once().unwrap();

    assert_eq!(resumed.last_journal_seq, 41);
    assert_eq!(resumed.last_state_version, 7);
    assert_eq!(resumed.published_snapshot_id.as_deref(), Some("snapshot-7"));
    assert_eq!(resumed.published_snapshot_committed_journal_seq, Some(41));
}

#[test]
fn restart_replays_unapplied_journal_entries_before_dispatch_resumes() {
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.seed_runtime_progress(41, 6, Some("snapshot-6"));
    supervisor.seed_unapplied_journal_entry(42, sample_input_task_event());

    let resumed = supervisor.resume_once().unwrap();

    assert_eq!(resumed.last_journal_seq, 42);
    assert_eq!(resumed.last_state_version, 7);
    assert_eq!(resumed.published_snapshot_id.as_deref(), Some("snapshot-7"));
    assert_eq!(resumed.published_snapshot_committed_journal_seq, Some(42));
}

fn sample_input_task_event() -> InputTaskEvent {
    InputTaskEvent {
        journal_seq: 42,
        event: ExternalFactEvent::new("market_ws", "session-1", "evt-42", "v1", Utc::now()),
    }
}
