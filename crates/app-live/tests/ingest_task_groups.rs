use app_live::{
    FollowUpQueue, FollowUpWork, IngressQueue, InputTaskEvent, ScopeRestriction,
    ScopeRestrictionKind, SnapshotDispatchQueue, SnapshotNotice, SupervisorPosture,
};
use chrono::Utc;
use domain::ExternalFactEvent;
use state::DirtyDomain;

#[test]
fn global_posture_and_scope_restrictions_are_not_the_same_authority() {
    let posture = SupervisorPosture::DegradedIngress;
    let restriction = ScopeRestriction::reconciling_only("family-a");

    assert!(posture.is_global());
    assert_eq!(restriction.scope_id(), "family-a");
    assert_eq!(restriction.kind(), ScopeRestrictionKind::ReconcilingOnly);
}

#[test]
fn snapshot_dispatch_queue_keeps_latest_stable_snapshot_for_dirty_domain() {
    let mut queue = SnapshotDispatchQueue::default();
    queue.push(SnapshotNotice::new("snapshot-7", 7, [DirtyDomain::Runtime]));
    queue.push(SnapshotNotice::new(
        "snapshot-8",
        8,
        [DirtyDomain::Runtime, DirtyDomain::NegRiskFamilies],
    ));

    let drained = queue.coalesced();

    assert_eq!(
        drained
            .iter()
            .map(|notice| notice.state_version)
            .collect::<Vec<_>>(),
        vec![8]
    );
    assert_eq!(drained.last().unwrap().state_version, 8);
}

#[test]
fn ingress_queue_orders_inputs_by_journal_seq() {
    let mut queue = IngressQueue::default();
    queue.push(sample_input_task_event(9));
    queue.push(sample_input_task_event(7));

    let first = queue.next_after(None).expect("first input");
    let second = queue
        .next_after(Some(first.journal_seq))
        .expect("second input");

    assert_eq!(first.journal_seq, 7);
    assert_eq!(second.journal_seq, 9);
}

#[test]
fn follow_up_queue_preserves_fifo_work_items() {
    let mut queue = FollowUpQueue::default();
    queue.push(FollowUpWork::pending_reconcile(
        "family-a",
        "pending-1",
        "heartbeat freshness exceeded threshold",
    ));
    queue.push(FollowUpWork::recovery(
        "family-b",
        "relayer ambiguity requires recovery",
    ));

    assert_eq!(queue.len(), 2);
    assert_eq!(
        queue.pop_front(),
        Some(FollowUpWork::pending_reconcile(
            "family-a",
            "pending-1",
            "heartbeat freshness exceeded threshold",
        ))
    );
    assert_eq!(
        queue.pop_front(),
        Some(FollowUpWork::recovery(
            "family-b",
            "relayer ambiguity requires recovery",
        ))
    );
    assert!(queue.is_empty());
}

fn sample_input_task_event(journal_seq: i64) -> InputTaskEvent {
    InputTaskEvent::new(
        journal_seq,
        ExternalFactEvent::new(
            "market_ws",
            "session-test",
            format!("evt-{journal_seq}"),
            "v1-test",
            Utc::now(),
        ),
    )
}
