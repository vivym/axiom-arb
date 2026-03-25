use chrono::{TimeZone, Utc};
use venue_polymarket::{WsChannelKind, WsSessionMonitor, WsSessionState, WsSessionStatus};

#[test]
fn reconnect_after_disconnect_increments_counter_and_updates_connection_id() {
    let monitor = WsSessionMonitor::new(WsChannelKind::Market);
    let mut state = WsSessionState::new(WsChannelKind::Market);

    let first = monitor.record_connected(&mut state, "conn-1", ts(10, 0, 0));
    assert_eq!(first.status, WsSessionStatus::Connected);
    assert_eq!(first.reconnect_total, 0);

    let disconnected = monitor.record_disconnected(&mut state, "network_gap", ts(10, 0, 5));
    assert_eq!(disconnected.unwrap().status, WsSessionStatus::Disconnected);

    let second = monitor.record_connected(&mut state, "conn-2", ts(10, 0, 8));
    assert_eq!(second.status, WsSessionStatus::Reconnected);
    assert_eq!(second.reconnect_total, 1);
    assert_eq!(state.connection_id.as_deref(), Some("conn-2"));
}

#[test]
fn duplicate_disconnect_without_active_connection_is_ignored() {
    let monitor = WsSessionMonitor::new(WsChannelKind::User);
    let mut state = WsSessionState::new(WsChannelKind::User);

    let connected = monitor.record_connected(&mut state, "conn-1", ts(10, 1, 0));
    assert_eq!(connected.status, WsSessionStatus::Connected);

    let first_disconnect = monitor.record_disconnected(&mut state, "network_gap", ts(10, 1, 5));
    assert_eq!(
        first_disconnect.unwrap().status,
        WsSessionStatus::Disconnected
    );

    assert!(monitor
        .record_disconnected(&mut state, "duplicate_disconnect", ts(10, 1, 6))
        .is_none());
}

fn ts(hour: u32, minute: u32, second: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, 25, hour, minute, second)
        .single()
        .unwrap()
}
