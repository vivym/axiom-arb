mod support;

use chrono::{Duration, TimeZone, Utc};
use observability::{
    bootstrap_observability, field_keys, metric_dimensions::Channel, span_names, MetricDimension,
    MetricDimensions,
};
use support::capture_spans;
use venue_polymarket::{
    HeartbeatReconcileReason, OrderHeartbeatMonitor, OrderHeartbeatState,
    VenueProducerInstrumentation, WsChannelKind, WsSessionMonitor, WsSessionState,
};

#[test]
fn reconnect_event_emits_repo_owned_span_and_channel_counter() {
    let observability = bootstrap_observability("venue-polymarket-test");
    let instrumentation = VenueProducerInstrumentation::enabled(observability.recorder());
    let monitor = WsSessionMonitor::new(WsChannelKind::Market);
    let mut state = WsSessionState::new(WsChannelKind::Market);

    monitor.record_connected(&mut state, "conn-1", ts(10, 0, 0));
    let reconnect = monitor
        .record_disconnected(&mut state, "network_gap", ts(10, 0, 5))
        .unwrap();
    instrumentation.record_ws_session_event(&reconnect);
    let reconnect = monitor.record_connected(&mut state, "conn-2", ts(10, 0, 8));

    let (captured_spans, ()) =
        capture_spans(|| instrumentation.record_ws_session_event(&reconnect));

    let dims = MetricDimensions::new([MetricDimension::Channel(Channel::Market)]);
    assert_eq!(
        observability.registry().snapshot().counter_with_dimensions(
            observability.metrics().websocket_reconnect_total.key(),
            &dims
        ),
        Some(1)
    );

    let span = captured_spans
        .iter()
        .find(|span| span.name == span_names::VENUE_WS_SESSION)
        .expect("venue websocket span missing");
    assert_eq!(
        span.field(field_keys::CHANNEL).map(String::as_str),
        Some("\"market\"")
    );
    assert_eq!(
        span.field(field_keys::SESSION_STATUS).map(String::as_str),
        Some("\"reconnected\"")
    );
}

#[test]
fn disabled_instrumentation_emits_no_ws_session_span_or_reconnect_counter() {
    let observability = bootstrap_observability("venue-polymarket-test");
    let instrumentation = VenueProducerInstrumentation::disabled();
    let monitor = WsSessionMonitor::new(WsChannelKind::Market);
    let mut state = WsSessionState::new(WsChannelKind::Market);

    monitor.record_connected(&mut state, "conn-1", ts(10, 0, 0));
    monitor
        .record_disconnected(&mut state, "network_gap", ts(10, 0, 5))
        .unwrap();
    let reconnect = monitor.record_connected(&mut state, "conn-2", ts(10, 0, 8));

    let (captured_spans, ()) =
        capture_spans(|| instrumentation.record_ws_session_event(&reconnect));

    let dims = MetricDimensions::new([MetricDimension::Channel(Channel::Market)]);
    assert_eq!(
        observability.registry().snapshot().counter_with_dimensions(
            observability.metrics().websocket_reconnect_total.key(),
            &dims
        ),
        None
    );
    assert!(
        captured_spans
            .iter()
            .all(|span| span.name != span_names::VENUE_WS_SESSION),
        "disabled instrumentation should not emit venue websocket spans"
    );
}

#[test]
fn heartbeat_success_records_freshness_and_structured_status() {
    let observability = bootstrap_observability("venue-polymarket-test");
    let instrumentation = VenueProducerInstrumentation::enabled(observability.recorder());
    let monitor = OrderHeartbeatMonitor::new(Duration::seconds(30));
    let mut state = OrderHeartbeatState {
        heartbeat_id: Some("hb-1".to_owned()),
        last_success_at: ts(10, 0, 0),
        reconcile_attention_since: None,
        reconcile_reason: None,
        requires_reconcile_attention: false,
    };
    let freshness = monitor.record_success(&mut state, "hb-2", ts(10, 0, 12));

    let (captured_spans, ()) =
        capture_spans(|| instrumentation.record_heartbeat_success(&state, freshness));

    assert_eq!(
        observability.registry().snapshot().gauge(
            observability.metrics().heartbeat_freshness.key()
        ),
        Some(freshness)
    );

    let span = captured_spans
        .iter()
        .find(|span| span.name == span_names::VENUE_HEARTBEAT)
        .expect("heartbeat span missing");
    assert_eq!(
        span.field(field_keys::HEARTBEAT_STATUS).map(String::as_str),
        Some("\"success\"")
    );
    assert_eq!(
        span.field(field_keys::HEARTBEAT_ID).map(String::as_str),
        Some("\"hb-2\"")
    );
}

#[test]
fn disabled_instrumentation_emits_no_heartbeat_span_or_freshness_metric() {
    let observability = bootstrap_observability("venue-polymarket-test");
    let monitor = OrderHeartbeatMonitor::new(Duration::seconds(30));
    let instrumentation = VenueProducerInstrumentation::disabled();
    let mut state = OrderHeartbeatState {
        heartbeat_id: Some("hb-1".to_owned()),
        last_success_at: ts(10, 0, 0),
        reconcile_attention_since: None,
        reconcile_reason: None,
        requires_reconcile_attention: false,
    };
    let freshness = monitor.record_success(&mut state, "hb-2", ts(10, 0, 12));

    let (captured_spans, ()) = capture_spans(|| {
        instrumentation.record_heartbeat_success(&state, freshness);
        instrumentation.record_heartbeat_attention(
            &state,
            HeartbeatReconcileReason::MissedHeartbeat,
            ts(10, 0, 31),
        );
    });

    assert_eq!(
        observability.registry().snapshot().gauge(
            observability.metrics().heartbeat_freshness.key()
        ),
        None
    );
    assert!(
        captured_spans
            .iter()
            .all(|span| span.name != span_names::VENUE_HEARTBEAT),
        "disabled heartbeat instrumentation should not emit venue heartbeat spans"
    );
}

#[test]
fn missed_heartbeat_records_freshness_and_structured_status() {
    let observability = bootstrap_observability("venue-polymarket-test");
    let instrumentation = VenueProducerInstrumentation::enabled(observability.recorder());
    let monitor = OrderHeartbeatMonitor::new(Duration::seconds(30));
    let mut state = OrderHeartbeatState {
        heartbeat_id: Some("hb-1".to_owned()),
        last_success_at: ts(10, 0, 0),
        reconcile_attention_since: None,
        reconcile_reason: None,
        requires_reconcile_attention: false,
    };

    let (captured_spans, reason) = capture_spans(|| {
        let reason = monitor.reconcile_trigger(&mut state, ts(10, 0, 31)).unwrap();
        instrumentation.record_heartbeat_attention(&state, reason, ts(10, 0, 31));
        reason
    });

    assert_eq!(reason, HeartbeatReconcileReason::MissedHeartbeat);
    assert_eq!(
        observability.registry().snapshot().gauge(
            observability.metrics().heartbeat_freshness.key()
        ),
        Some(31.0)
    );

    let span = captured_spans
        .iter()
        .find(|span| span.name == span_names::VENUE_HEARTBEAT)
        .expect("heartbeat span missing");
    assert_eq!(
        span.field(field_keys::HEARTBEAT_STATUS).map(String::as_str),
        Some("\"missed\"")
    );
    assert_eq!(
        span.field(field_keys::HEARTBEAT_ID).map(String::as_str),
        Some("\"hb-1\"")
    );
}

fn ts(hour: u32, minute: u32, second: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, 25, hour, minute, second)
        .single()
        .unwrap()
}
