use chrono::{TimeZone, Utc};
use venue_polymarket::{WsChannelKind, WsSessionMonitor, WsSessionState, WsSessionStatus};

#[test]
fn reconnect_after_disconnect_increments_counter_and_updates_connection_id() {
    let monitor = WsSessionMonitor::new(WsChannelKind::Market);
    let mut state = WsSessionState::new(WsChannelKind::Market);

    let first = monitor.record_connected(&mut state, "conn-1", ts(10, 0, 0));
    assert_eq!(first.channel, WsChannelKind::Market);
    assert_eq!(first.connection_id, "conn-1");
    assert_eq!(first.status, WsSessionStatus::Connected);
    assert_eq!(first.reconnect_total, 0);
    assert_eq!(first.observed_at, ts(10, 0, 0));
    assert_eq!(first.disconnect_reason, None);
    assert_eq!(state.connection_id.as_deref(), Some("conn-1"));
    assert!(state.connected);
    assert_eq!(state.reconnect_total, 0);
    assert_eq!(state.last_connected_at, Some(ts(10, 0, 0)));
    assert_eq!(state.last_disconnected_at, None);
    assert_eq!(state.last_disconnect_reason, None);

    let disconnected = monitor.record_disconnected(&mut state, "network_gap", ts(10, 0, 5));
    let disconnected = disconnected.unwrap();
    assert_eq!(disconnected.channel, WsChannelKind::Market);
    assert_eq!(disconnected.connection_id, "conn-1");
    assert_eq!(disconnected.status, WsSessionStatus::Disconnected);
    assert_eq!(disconnected.reconnect_total, 0);
    assert_eq!(disconnected.observed_at, ts(10, 0, 5));
    assert_eq!(disconnected.disconnect_reason.as_deref(), Some("network_gap"));
    assert!(!state.connected);
    assert_eq!(state.connection_id.as_deref(), Some("conn-1"));
    assert_eq!(state.reconnect_total, 0);
    assert_eq!(state.last_connected_at, Some(ts(10, 0, 0)));
    assert_eq!(state.last_disconnected_at, Some(ts(10, 0, 5)));
    assert_eq!(state.last_disconnect_reason.as_deref(), Some("network_gap"));

    let second = monitor.record_connected(&mut state, "conn-2", ts(10, 0, 8));
    assert_eq!(second.channel, WsChannelKind::Market);
    assert_eq!(second.connection_id, "conn-2");
    assert_eq!(second.status, WsSessionStatus::Reconnected);
    assert_eq!(second.reconnect_total, 1);
    assert_eq!(second.observed_at, ts(10, 0, 8));
    assert_eq!(second.disconnect_reason, None);
    assert_eq!(state.connection_id.as_deref(), Some("conn-2"));
    assert!(state.connected);
    assert_eq!(state.reconnect_total, 1);
    assert_eq!(state.last_connected_at, Some(ts(10, 0, 8)));
    assert_eq!(state.last_disconnected_at, None);
    assert_eq!(state.last_disconnect_reason, None);
}

#[test]
fn duplicate_disconnect_without_active_connection_is_ignored() {
    let monitor = WsSessionMonitor::new(WsChannelKind::User);
    let mut state = WsSessionState::new(WsChannelKind::User);

    let connected = monitor.record_connected(&mut state, "conn-1", ts(10, 1, 0));
    assert_eq!(connected.status, WsSessionStatus::Connected);
    assert_eq!(state.last_connected_at, Some(ts(10, 1, 0)));
    assert!(state.connected);

    let first_disconnect = monitor.record_disconnected(&mut state, "network_gap", ts(10, 1, 5));
    let first_disconnect = first_disconnect.unwrap();
    assert_eq!(first_disconnect.status, WsSessionStatus::Disconnected);
    assert_eq!(first_disconnect.connection_id, "conn-1");
    assert_eq!(
        first_disconnect.disconnect_reason.as_deref(),
        Some("network_gap")
    );
    assert_eq!(state.last_disconnected_at, Some(ts(10, 1, 5)));
    assert_eq!(state.last_disconnect_reason.as_deref(), Some("network_gap"));
    assert!(!state.connected);

    assert!(monitor
        .record_disconnected(&mut state, "duplicate_disconnect", ts(10, 1, 6))
        .is_none());
    assert_eq!(state.last_disconnected_at, Some(ts(10, 1, 5)));
    assert_eq!(state.last_disconnect_reason.as_deref(), Some("network_gap"));
    assert_eq!(state.connection_id.as_deref(), Some("conn-1"));
    assert!(!state.connected);
}

fn ts(hour: u32, minute: u32, second: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, 25, hour, minute, second)
        .single()
        .unwrap()
}
