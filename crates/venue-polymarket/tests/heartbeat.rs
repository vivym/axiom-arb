use chrono::{Duration, TimeZone, Utc};
mod support;

use support::MockServer;
use url::Url;
use venue_polymarket::{
    HeartbeatFetchResult, HeartbeatReconcileReason, OrderHeartbeatMonitor, OrderHeartbeatState,
    PolymarketRestClient,
};

#[test]
fn heartbeat_missing_success_triggers_reconcile_once_and_persists_attention() {
    let monitor = OrderHeartbeatMonitor::new(Duration::seconds(30));
    let mut state = OrderHeartbeatState {
        heartbeat_id: Some("hb-1".to_owned()),
        last_success_at: ts(10, 0, 0),
        reconcile_attention_since: None,
        reconcile_reason: None,
        requires_reconcile_attention: false,
    };

    assert_eq!(
        monitor.reconcile_trigger(&mut state, ts(10, 0, 31)),
        Some(HeartbeatReconcileReason::MissedHeartbeat)
    );
    assert_eq!(monitor.reconcile_trigger(&mut state, ts(10, 0, 45)), None);
    assert!(state.requires_reconcile_attention);
    assert_eq!(
        state.reconcile_reason,
        Some(HeartbeatReconcileReason::MissedHeartbeat)
    );
    assert_eq!(state.reconcile_attention_since, Some(ts(10, 0, 30)));
}

#[test]
fn heartbeat_invalid_response_triggers_reconcile_immediately_and_dedupes() {
    let monitor = OrderHeartbeatMonitor::new(Duration::seconds(30));
    let mut state = OrderHeartbeatState {
        heartbeat_id: Some("hb-1".to_owned()),
        last_success_at: ts(10, 0, 0),
        reconcile_attention_since: None,
        reconcile_reason: None,
        requires_reconcile_attention: false,
    };

    assert_eq!(
        monitor.record_invalid(&mut state, ts(10, 0, 10)),
        Some(HeartbeatReconcileReason::InvalidHeartbeat)
    );
    assert_eq!(monitor.record_invalid(&mut state, ts(10, 0, 11)), None);
    assert!(state.requires_reconcile_attention);
    assert_eq!(
        state.reconcile_reason,
        Some(HeartbeatReconcileReason::InvalidHeartbeat)
    );
    assert_eq!(state.reconcile_attention_since, Some(ts(10, 0, 10)));
}

#[test]
fn heartbeat_success_updates_latest_id_and_freshness() {
    let monitor = OrderHeartbeatMonitor::new(Duration::seconds(30));
    let mut state = OrderHeartbeatState {
        heartbeat_id: None,
        last_success_at: ts(10, 0, 0),
        reconcile_attention_since: Some(ts(9, 59, 59)),
        reconcile_reason: Some(HeartbeatReconcileReason::InvalidHeartbeat),
        requires_reconcile_attention: true,
    };

    let freshness = monitor.record_success(&mut state, "hb-2", ts(10, 0, 5));

    assert_eq!(freshness, 5.0);
    assert_eq!(state.heartbeat_id.as_deref(), Some("hb-2"));
    assert_eq!(state.last_success_at, ts(10, 0, 5));
    assert!(!state.requires_reconcile_attention);
    assert_eq!(state.reconcile_reason, None);
    assert_eq!(state.reconcile_attention_since, None);
    assert_eq!(monitor.reconcile_trigger(&mut state, ts(10, 0, 30)), None);
}

#[test]
fn heartbeat_helpers_expose_status_labels_and_freshness_age() {
    let state = OrderHeartbeatState {
        heartbeat_id: Some("hb-1".to_owned()),
        last_success_at: ts(10, 0, 0),
        reconcile_attention_since: None,
        reconcile_reason: None,
        requires_reconcile_attention: false,
    };

    assert_eq!(state.freshness_seconds(ts(10, 0, 31)), 31.0);
    assert_eq!(
        HeartbeatReconcileReason::MissedHeartbeat.as_status(),
        "missed"
    );
    assert_eq!(
        HeartbeatReconcileReason::InvalidHeartbeat.as_status(),
        "invalid"
    );
}

#[tokio::test]
async fn heartbeat_fetch_maps_success_payload_into_monitor_input() {
    let server = MockServer::spawn("200 OK", r#"{"success":true,"heartbeat_id":"hb-42"}"#);
    let client = sample_client(server.base_url());

    let heartbeat = client
        .fetch_order_heartbeat()
        .await
        .expect("heartbeat fetch should succeed");

    assert_eq!(
        heartbeat,
        HeartbeatFetchResult {
            heartbeat_id: "hb-42".to_owned(),
            valid: true,
        }
    );
    assert!(server.finish().starts_with("GET /heartbeat HTTP/1.1"));
}

fn sample_client(base_url: Url) -> PolymarketRestClient {
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test client");

    PolymarketRestClient::with_http_client(client, base_url.clone(), base_url.clone(), base_url)
}

fn ts(hour: u32, minute: u32, second: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, 24, hour, minute, second)
        .single()
        .unwrap()
}
