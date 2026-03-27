use observability::{
    bootstrap_tracing,
    metric_dimensions::{Channel, HaltScope, ReconcileReason},
    metrics::{MetricDimension, MetricDimensions, MetricKey},
    CounterHandle, Observability, RuntimeMetrics,
};

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
fn runtime_metrics_expose_neg_risk_rollout_gate_counts() {
    let metrics = RuntimeMetrics::default();

    assert_eq!(
        metrics.neg_risk_live_ready_family_count.key(),
        MetricKey::new("axiom_neg_risk_live_ready_family_count")
    );
    assert_eq!(
        metrics.neg_risk_live_attempt_count.key(),
        MetricKey::new("axiom_neg_risk_live_attempt_count")
    );
    assert_eq!(
        metrics.neg_risk_live_gate_block_count.key(),
        MetricKey::new("axiom_neg_risk_live_gate_block_count")
    );
    assert_eq!(
        metrics.neg_risk_rollout_parity_mismatch_count.key(),
        MetricKey::new("axiom_neg_risk_rollout_parity_mismatch_total")
    );
}

#[test]
fn runtime_metrics_expose_negrisk_live_submit_closure_signals() {
    let metrics = RuntimeMetrics::default();

    assert_eq!(
        metrics.neg_risk_live_submit_accepted_total.key(),
        MetricKey::new("axiom_neg_risk_live_submit_accepted_total")
    );
    assert_eq!(
        metrics.neg_risk_live_submit_ambiguous_total.key(),
        MetricKey::new("axiom_neg_risk_live_submit_ambiguous_total")
    );
}

#[test]
fn runtime_metrics_keep_wave1c_control_plane_contracts() {
    let metrics = RuntimeMetrics::default();
    assert_eq!(
        metrics.neg_risk_family_discovered_count.key().as_str(),
        "axiom_neg_risk_family_discovered_count"
    );
    assert_eq!(
        metrics.neg_risk_rollout_parity_mismatch_count.key().as_str(),
        "axiom_neg_risk_rollout_parity_mismatch_total"
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
    assert_eq!(
        metrics.websocket_reconnect_total.key(),
        MetricKey::new("axiom_websocket_reconnect_total")
    );
    assert_eq!(
        metrics.halt_activation_total.key(),
        MetricKey::new("axiom_halt_activation_total")
    );
    assert_eq!(
        metrics.reconcile_attention_total.key(),
        MetricKey::new("axiom_reconcile_attention_total")
    );
    assert_eq!(
        metrics.ingress_backlog.key(),
        MetricKey::new("axiom_ingress_backlog")
    );
    assert_eq!(
        metrics.follow_up_backlog.key(),
        MetricKey::new("axiom_follow_up_backlog")
    );
    assert_eq!(
        metrics.daemon_posture.key(),
        MetricKey::new("axiom_daemon_posture")
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
    recorder.record_neg_risk_live_ready_family_count(4.0);
    recorder.record_neg_risk_live_attempt_count(1.0);
    recorder.record_neg_risk_live_gate_block_count(5.0);
    recorder.increment_neg_risk_live_submit_accepted_total(2);
    recorder.increment_neg_risk_live_submit_ambiguous_total(1);
    recorder.increment_neg_risk_rollout_parity_mismatch_count(2);
    recorder.record_ingress_backlog(8.0);
    recorder.record_follow_up_backlog(3.0);
    recorder.record_daemon_posture("healthy");

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
    assert_eq!(
        snapshot.gauge(metrics.neg_risk_live_ready_family_count.key()),
        Some(4.0)
    );
    assert_eq!(
        snapshot.gauge(metrics.neg_risk_live_attempt_count.key()),
        Some(1.0)
    );
    assert_eq!(
        snapshot.gauge(metrics.neg_risk_live_gate_block_count.key()),
        Some(5.0)
    );
    assert_eq!(
        snapshot.counter(metrics.neg_risk_live_submit_accepted_total.key()),
        Some(2)
    );
    assert_eq!(
        snapshot.counter(metrics.neg_risk_live_submit_ambiguous_total.key()),
        Some(1)
    );
    assert_eq!(
        snapshot.counter(metrics.neg_risk_rollout_parity_mismatch_count.key()),
        Some(2)
    );
    assert_eq!(snapshot.gauge(metrics.ingress_backlog.key()), Some(8.0));
    assert_eq!(snapshot.gauge(metrics.follow_up_backlog.key()), Some(3.0));
    assert_eq!(snapshot.mode(metrics.daemon_posture.key()), Some("healthy"));
}

#[test]
fn registry_round_trips_dimensioned_counter_samples() {
    let observability = Observability::new("app-live");
    let metrics = observability.metrics();
    let dims = MetricDimensions::new([MetricDimension::Channel(Channel::User)]);

    observability
        .recorder()
        .increment_websocket_reconnect_total(2, dims.clone());

    let snapshot = observability.registry().snapshot();
    assert_eq!(
        snapshot.counter(metrics.websocket_reconnect_total.key()),
        None
    );
    assert_eq!(
        snapshot.counter_with_dimensions(metrics.websocket_reconnect_total.key(), &dims),
        Some(2)
    );
}

#[test]
#[should_panic(expected = "dimensioned counters must not be recorded through the scalar path")]
fn registry_rejects_scalar_samples_for_dimensioned_counter_keys() {
    let observability = Observability::new("app-live");

    observability
        .registry()
        .record_counter(CounterHandle::new("axiom_websocket_reconnect_total").increment(1));
}

#[test]
fn counter_dimension_order_is_canonicalized() {
    let observability = Observability::new("app-live");
    let metrics = observability.metrics();
    let recorded_dims = MetricDimensions::new([
        MetricDimension::HaltScope(HaltScope::Market),
        MetricDimension::Channel(Channel::User),
    ]);
    let lookup_dims = MetricDimensions::new([
        MetricDimension::Channel(Channel::User),
        MetricDimension::HaltScope(HaltScope::Market),
    ]);

    observability
        .recorder()
        .increment_halt_activation_total(1, recorded_dims);

    let snapshot = observability.registry().snapshot();
    assert_eq!(
        snapshot.counter_with_dimensions(metrics.halt_activation_total.key(), &lookup_dims),
        Some(1)
    );
}

#[test]
fn mode_samples_remain_queryable_without_forcing_numeric_encoding() {
    let observability = Observability::new("app-live");
    observability.recorder().record_runtime_mode("healthy");

    let snapshot = observability.registry().snapshot();
    assert_eq!(
        snapshot.mode(observability.metrics().runtime_mode.key()),
        Some("healthy")
    );
}

#[test]
#[should_panic(expected = "conflicting metric dimension values for key channel")]
fn metric_dimensions_reject_conflicting_values_for_the_same_key() {
    let _ = MetricDimensions::new([
        MetricDimension::Channel(Channel::User),
        MetricDimension::Channel(Channel::Market),
    ]);
}

#[test]
fn reconcile_reason_dimensions_round_trip_in_metric_registry() {
    let observability = Observability::new("app-live");
    let metrics = observability.metrics();
    let dims = MetricDimensions::new([MetricDimension::ReconcileReason(
        ReconcileReason::InventoryMismatch,
    )]);

    observability
        .recorder()
        .increment_reconcile_attention_total(4, dims.clone());

    let snapshot = observability.registry().snapshot();
    assert_eq!(
        snapshot.counter(metrics.reconcile_attention_total.key()),
        None
    );
    assert_eq!(
        snapshot.counter_with_dimensions(metrics.reconcile_attention_total.key(), &dims),
        Some(4)
    );
}
