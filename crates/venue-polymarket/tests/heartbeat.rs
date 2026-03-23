use chrono::{Duration, TimeZone, Utc};
use venue_polymarket::{
    HeartbeatReconcileReason, OrderHeartbeatMonitor, OrderHeartbeatState,
};

#[test]
fn heartbeat_missing_success_triggers_reconcile() {
    let monitor = OrderHeartbeatMonitor::new(Duration::seconds(30));
    let state = OrderHeartbeatState {
        heartbeat_id: Some("hb-1".to_owned()),
        last_success_at: ts(10, 0, 0),
    };

    assert_eq!(
        monitor.reconcile_trigger(&state, ts(10, 0, 31)),
        Some(HeartbeatReconcileReason::MissedHeartbeat)
    );
}

#[test]
fn heartbeat_invalid_response_triggers_reconcile_immediately() {
    let monitor = OrderHeartbeatMonitor::new(Duration::seconds(30));
    let mut state = OrderHeartbeatState {
        heartbeat_id: Some("hb-1".to_owned()),
        last_success_at: ts(10, 0, 0),
    };

    assert_eq!(
        monitor.record_invalid(&mut state, ts(10, 0, 10)),
        Some(HeartbeatReconcileReason::InvalidHeartbeat)
    );
}

#[test]
fn heartbeat_success_updates_latest_id_and_freshness() {
    let monitor = OrderHeartbeatMonitor::new(Duration::seconds(30));
    let mut state = OrderHeartbeatState {
        heartbeat_id: None,
        last_success_at: ts(10, 0, 0),
    };

    monitor.record_success(&mut state, "hb-2", ts(10, 0, 5));

    assert_eq!(state.heartbeat_id.as_deref(), Some("hb-2"));
    assert_eq!(state.last_success_at, ts(10, 0, 5));
    assert_eq!(monitor.reconcile_trigger(&state, ts(10, 0, 30)), None);
}

fn ts(hour: u32, minute: u32, second: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, 24, hour, minute, second)
        .single()
        .unwrap()
}
