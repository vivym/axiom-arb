use observability::{metrics::MetricKey, Observability};

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
