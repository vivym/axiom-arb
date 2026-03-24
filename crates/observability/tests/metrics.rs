use observability::{bootstrap_tracing, metrics::MetricKey, Observability, RuntimeMetrics};

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
fn runtime_metrics_expose_neg_risk_family_counts() {
    let observability = Observability::new("app-live");
    let metrics = observability.metrics();

    assert_eq!(
        metrics.neg_risk_family_discovered_count.key(),
        MetricKey::new("axiom_neg_risk_family_discovered_count")
    );
    assert_eq!(
        metrics.neg_risk_family_included_count.key(),
        MetricKey::new("axiom_neg_risk_family_included_count")
    );
    assert_eq!(
        metrics.neg_risk_family_excluded_count.key(),
        MetricKey::new("axiom_neg_risk_family_excluded_count")
    );
    assert_eq!(
        metrics.neg_risk_family_halt_count.key(),
        MetricKey::new("axiom_neg_risk_family_halt_count")
    );
    assert_eq!(
        metrics.neg_risk_metadata_refresh_count.key(),
        MetricKey::new("axiom_neg_risk_metadata_refresh_total")
    );
}

#[test]
fn runtime_metrics_expose_dispatch_and_recovery_backlog_signals() {
    let metrics = RuntimeMetrics::default();

    assert_eq!(
        metrics.dispatcher_backlog_count.key(),
        MetricKey::new("axiom_dispatcher_backlog_count")
    );
    assert_eq!(
        metrics.projection_publish_lag_count.key(),
        MetricKey::new("axiom_projection_publish_lag_count")
    );
    assert_eq!(
        metrics.recovery_backlog_count.key(),
        MetricKey::new("axiom_recovery_backlog_count")
    );
    assert_eq!(
        metrics.shadow_attempt_count.key(),
        MetricKey::new("axiom_shadow_attempt_total")
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
    recorder.record_dispatcher_backlog_count(11.0);
    recorder.record_projection_publish_lag_count(2.0);
    recorder.record_recovery_backlog_count(5.0);
    recorder.increment_shadow_attempt_count(13);
    recorder.record_neg_risk_family_discovered_count(9.0);
    recorder.record_neg_risk_family_included_count(6.0);
    recorder.record_neg_risk_family_excluded_count(3.0);
    recorder.record_neg_risk_family_halt_count(2.0);
    recorder.increment_neg_risk_metadata_refresh_count(7);

    let snapshot = observability.registry().snapshot();
    assert_eq!(
        snapshot.gauge(metrics.heartbeat_freshness.key()),
        Some(12.5)
    );
    assert_eq!(snapshot.mode(metrics.runtime_mode.key()), Some("paper"));
    assert_eq!(snapshot.gauge(metrics.relayer_pending_age.key()), Some(4.0));
    assert_eq!(snapshot.counter(metrics.divergence_count.key()), Some(3));
    assert_eq!(
        snapshot.gauge(metrics.dispatcher_backlog_count.key()),
        Some(11.0)
    );
    assert_eq!(
        snapshot.gauge(metrics.projection_publish_lag_count.key()),
        Some(2.0)
    );
    assert_eq!(
        snapshot.gauge(metrics.recovery_backlog_count.key()),
        Some(5.0)
    );
    assert_eq!(
        snapshot.counter(metrics.shadow_attempt_count.key()),
        Some(13)
    );
    assert_eq!(
        snapshot.gauge(metrics.neg_risk_family_discovered_count.key()),
        Some(9.0)
    );
    assert_eq!(
        snapshot.gauge(metrics.neg_risk_family_included_count.key()),
        Some(6.0)
    );
    assert_eq!(
        snapshot.gauge(metrics.neg_risk_family_excluded_count.key()),
        Some(3.0)
    );
    assert_eq!(
        snapshot.gauge(metrics.neg_risk_family_halt_count.key()),
        Some(2.0)
    );
    assert_eq!(
        snapshot.counter(metrics.neg_risk_metadata_refresh_count.key()),
        Some(7)
    );
}
