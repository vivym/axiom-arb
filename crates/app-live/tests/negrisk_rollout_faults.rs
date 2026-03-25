use chrono::Utc;

use app_live::{AppSupervisor, InputTaskEvent};
use domain::ExternalFactEvent;

#[test]
fn restart_requires_durable_rollout_gate_evidence_before_live_promotion() {
    let mut supervisor = AppSupervisor::for_tests();
    for journal_seq in 36..=41 {
        supervisor.seed_committed_input(sample_input_task_event(journal_seq));
    }
    supervisor.seed_runtime_progress(41, 6, Some("snapshot-6"));
    supervisor.seed_committed_state_version(6);
    supervisor.seed_pending_reconcile_count(0);

    let err = supervisor.resume_once().unwrap_err();

    assert!(err.to_string().contains("rollout gate evidence"));
}

fn sample_input_task_event(journal_seq: i64) -> InputTaskEvent {
    InputTaskEvent::new(
        journal_seq,
        ExternalFactEvent::new(
            "market_ws",
            "session-negrisk-rollout",
            format!("evt-{journal_seq}"),
            "v1",
            Utc::now(),
        ),
    )
}
