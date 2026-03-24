use observability::bootstrap_observability;

#[test]
fn bootstrap_surface_returns_service_identity_metrics_and_registry() {
    let bootstrapped = bootstrap_observability("app-live");

    assert_eq!(bootstrapped.service_name(), "app-live");
    bootstrapped.recorder().record_runtime_mode("paper");
    assert_eq!(
        bootstrapped
            .registry()
            .snapshot()
            .mode(bootstrapped.metrics().runtime_mode.key()),
        Some("paper")
    );
}

#[test]
fn bootstrap_surface_initializes_tracing_only_once() {
    let first = bootstrap_observability("app-live");
    let second = bootstrap_observability("app-live");

    assert_eq!(first.service_name(), second.service_name());
}
