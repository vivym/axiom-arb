mod support;

use observability::{bootstrap_observability, field_keys, span_names};
use support::{
    capture_spans_async, sample_client_with_injected_metadata_api_and_instrumentation,
    sample_client_with_instrumentation, sample_failing_client_with_instrumentation,
    sample_refresh_then_fail_client_with_instrumentation,
    scripted_metadata_api_valid_then_all_malformed, scripted_metadata_api_with_valid_and_malformed,
};

#[tokio::test]
async fn successful_metadata_refresh_records_revision_snapshot_and_discovered_count() {
    let observability = bootstrap_observability("venue-metadata-test");
    let (client, _server) = sample_client_with_instrumentation(observability.recorder());

    let (captured_spans, rows) =
        capture_spans_async(|| async { client.try_fetch_neg_risk_metadata_rows().await.unwrap() })
            .await;

    assert!(!rows.is_empty());
    let refresh_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::VENUE_METADATA_REFRESH)
        .expect("metadata refresh span missing");
    assert_eq!(
        refresh_span
            .field(field_keys::DISCOVERY_REVISION)
            .map(String::as_str),
        Some("1")
    );
    assert!(refresh_span
        .field(field_keys::METADATA_SNAPSHOT_HASH)
        .is_some());
    assert_eq!(
        refresh_span
            .field(field_keys::REFRESH_RESULT)
            .map(String::as_str),
        Some("\"success\"")
    );
    assert!(refresh_span
        .field(field_keys::REFRESH_DURATION_MS)
        .is_some());
    assert!(refresh_span
        .field(field_keys::DISCOVERED_FAMILY_COUNT)
        .is_some());
    assert_eq!(
        observability.registry().snapshot().gauge(
            observability
                .metrics()
                .neg_risk_family_discovered_count
                .key()
        ),
        Some(1.0)
    );
    assert_eq!(
        observability.registry().snapshot().counter(
            observability
                .metrics()
                .neg_risk_metadata_refresh_count
                .key()
        ),
        Some(1)
    );
}

#[tokio::test]
async fn failed_metadata_refresh_does_not_publish_new_discovered_family_gauge() {
    let observability = bootstrap_observability("venue-metadata-test");
    let (client, _server) = sample_failing_client_with_instrumentation(observability.recorder());

    let (captured_spans, result) =
        capture_spans_async(|| async { client.try_fetch_neg_risk_metadata_rows().await }).await;

    assert!(result.is_err());
    let refresh_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::VENUE_METADATA_REFRESH)
        .expect("metadata refresh span missing");
    assert_eq!(
        refresh_span
            .field(field_keys::REFRESH_RESULT)
            .map(String::as_str),
        Some("\"failure\"")
    );
    assert!(refresh_span
        .field(field_keys::REFRESH_DURATION_MS)
        .is_some());

    assert_eq!(
        observability.registry().snapshot().gauge(
            observability
                .metrics()
                .neg_risk_family_discovered_count
                .key()
        ),
        None
    );
    assert_eq!(
        observability.registry().snapshot().counter(
            observability
                .metrics()
                .neg_risk_metadata_refresh_count
                .key()
        ),
        Some(1)
    );
}

#[tokio::test]
async fn fallback_cache_read_does_not_publish_new_discovered_family_gauge() {
    let observability = bootstrap_observability("venue-metadata-test");
    let (client, _server) =
        sample_refresh_then_fail_client_with_instrumentation(observability.recorder());

    let primed_rows = client.try_fetch_neg_risk_metadata_rows().await.unwrap();
    assert!(!primed_rows.is_empty());

    observability
        .recorder()
        .record_neg_risk_family_discovered_count(99.0);

    let (captured_spans, rows) =
        capture_spans_async(|| async { client.fetch_neg_risk_metadata_rows().await.unwrap() })
            .await;

    assert_eq!(rows, primed_rows);
    let refresh_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::VENUE_METADATA_REFRESH)
        .expect("metadata refresh span missing");
    assert_eq!(
        refresh_span
            .field(field_keys::REFRESH_RESULT)
            .map(String::as_str),
        Some("\"failure\"")
    );
    assert_eq!(
        observability.registry().snapshot().gauge(
            observability
                .metrics()
                .neg_risk_family_discovered_count
                .key()
        ),
        Some(99.0)
    );
    assert_eq!(
        observability.registry().snapshot().counter(
            observability
                .metrics()
                .neg_risk_metadata_refresh_count
                .key()
        ),
        Some(2)
    );
}

#[tokio::test]
async fn injected_metadata_refresh_records_success_span_and_metrics() {
    let observability = bootstrap_observability("venue-metadata-test");
    let client = sample_client_with_injected_metadata_api_and_instrumentation(
        scripted_metadata_api_with_valid_and_malformed(),
        observability.recorder(),
    );

    let (captured_spans, rows) =
        capture_spans_async(|| async { client.try_fetch_neg_risk_metadata_rows().await.unwrap() })
            .await;

    assert!(!rows.is_empty());
    let refresh_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::VENUE_METADATA_REFRESH)
        .expect("metadata refresh span missing");
    assert_eq!(
        refresh_span
            .field(field_keys::DISCOVERY_REVISION)
            .map(String::as_str),
        Some("1")
    );
    assert_eq!(
        refresh_span
            .field(field_keys::REFRESH_RESULT)
            .map(String::as_str),
        Some("\"success\"")
    );
    assert_eq!(
        observability.registry().snapshot().counter(
            observability
                .metrics()
                .neg_risk_metadata_refresh_count
                .key()
        ),
        Some(1)
    );
}

#[tokio::test]
async fn injected_metadata_fallback_records_failure_span_without_republishing_gauge() {
    let observability = bootstrap_observability("venue-metadata-test");
    let client = sample_client_with_injected_metadata_api_and_instrumentation(
        scripted_metadata_api_valid_then_all_malformed(),
        observability.recorder(),
    );

    let primed_rows = client.try_fetch_neg_risk_metadata_rows().await.unwrap();
    assert!(!primed_rows.is_empty());

    observability
        .recorder()
        .record_neg_risk_family_discovered_count(99.0);

    let (captured_spans, rows) =
        capture_spans_async(|| async { client.fetch_neg_risk_metadata_rows().await.unwrap() })
            .await;

    assert_eq!(rows, primed_rows);
    let refresh_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::VENUE_METADATA_REFRESH)
        .expect("metadata refresh span missing");
    assert_eq!(
        refresh_span
            .field(field_keys::REFRESH_RESULT)
            .map(String::as_str),
        Some("\"failure\"")
    );
    assert_eq!(
        observability.registry().snapshot().gauge(
            observability
                .metrics()
                .neg_risk_family_discovered_count
                .key()
        ),
        Some(99.0)
    );
    assert_eq!(
        observability.registry().snapshot().counter(
            observability
                .metrics()
                .neg_risk_metadata_refresh_count
                .key()
        ),
        Some(2)
    );
}
