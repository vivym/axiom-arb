use observability::{bootstrap_tracing, metrics::MetricKey, Observability};

#[test]
fn observability_exposes_required_runtime_metric_keys() {
    let observability = Observability::new("app-live");
    let metrics = observability.metrics();

    assert_eq!(
        metrics.heartbeat_freshness.key(),
        MetricKey::new("axiom_heartbeat_freshness_seconds")
    );
    assert_eq!(
        metrics.runtime_mode.key(),
        MetricKey::new("axiom_runtime_mode")
    );
    assert_eq!(
        metrics.relayer_pending_age.key(),
        MetricKey::new("axiom_relayer_pending_age_seconds")
    );
    assert_eq!(
        metrics.divergence_count.key(),
        MetricKey::new("axiom_runtime_divergence_total")
    );
}

#[test]
fn tracing_bootstrap_is_explicit_and_reports_service_name() {
    let tracing = bootstrap_tracing("app-live");

    assert_eq!(tracing.service_name(), "app-live");
}

#[test]
fn runtime_metrics_recorder_updates_registry() {
    let observability = Observability::new("app-live");
    let recorder = observability.recorder();
    let metrics = observability.metrics();

    recorder.record_heartbeat_freshness(12.5);
    recorder.record_runtime_mode("paper");
    recorder.record_relayer_pending_age(4.0);
    recorder.increment_divergence_count(3);

    let snapshot = observability.registry().snapshot();
    assert_eq!(
        snapshot.gauge(metrics.heartbeat_freshness.key()),
        Some(12.5)
    );
    assert_eq!(snapshot.mode(metrics.runtime_mode.key()), Some("paper"));
    assert_eq!(snapshot.gauge(metrics.relayer_pending_age.key()), Some(4.0));
    assert_eq!(snapshot.counter(metrics.divergence_count.key()), Some(3));
}
