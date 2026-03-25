mod support;

use chrono::{TimeZone, Utc};
use observability::{
    bootstrap_observability, field_keys, metric_dimensions::Channel, span_names, MetricDimension,
    MetricDimensions,
};
use support::capture_spans;
use venue_polymarket::{
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

fn ts(hour: u32, minute: u32, second: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, 25, hour, minute, second)
        .single()
        .unwrap()
}
