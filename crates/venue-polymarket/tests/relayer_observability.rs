mod support;

use chrono::{TimeZone, Utc};
use observability::{bootstrap_observability, field_keys, span_names};
use support::{capture_spans, sample_builder_relayer_auth, sample_client_for, MockServer};
use venue_polymarket::VenueProducerInstrumentation;

#[test]
fn instrumented_recent_transactions_record_oldest_pending_age() {
    let observability = bootstrap_observability("venue-polymarket-test");
    let instrumentation = VenueProducerInstrumentation::enabled(observability.recorder());
    let server = MockServer::spawn(
        "200 OK",
        r#"[{"transactionID":"tx-pending","state":"STATE_PENDING","createdAt":"2026-03-25T10:00:00Z"},{"transactionID":"tx-confirmed","state":"STATE_CONFIRMED","createdAt":"2026-03-25T10:00:10Z"}]"#,
    );
    let client = sample_client_for(server.base_url());

    let now = Utc
        .with_ymd_and_hms(2026, 3, 25, 10, 0, 30)
        .single()
        .unwrap();
    let (captured_spans, transactions) = capture_spans(|| {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            client
                .fetch_recent_transactions_instrumented(
                    &sample_builder_relayer_auth(),
                    &instrumentation,
                    now,
                )
                .await
                .unwrap()
        })
    });

    assert_eq!(transactions.len(), 2);
    assert_eq!(
        observability
            .registry()
            .snapshot()
            .gauge(observability.metrics().relayer_pending_age.key()),
        Some(30.0)
    );

    let relayer_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::VENUE_RELAYER_POLL)
        .expect("relayer poll span missing");
    assert_eq!(
        relayer_span
            .field(field_keys::RELAYER_TX_COUNT)
            .map(String::as_str),
        Some("2")
    );
    assert_eq!(
        relayer_span
            .field(field_keys::PENDING_TX_COUNT)
            .map(String::as_str),
        Some("1")
    );
    assert_eq!(
        relayer_span
            .field(field_keys::PENDING_AGE_SECONDS)
            .map(String::as_str),
        Some("30.0")
    );
}
