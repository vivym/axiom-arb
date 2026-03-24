use observability::bootstrap_observability;
use std::{env, process::Command};

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
    if env::var_os("OBSERVABILITY_BOOTSTRAP_ONCE_HELPER").is_some() {
        let first = bootstrap_observability("app-live");
        let second = bootstrap_observability("app-live");
        assert!(first.tracing().initialized_global_subscriber());
        assert!(!second.tracing().initialized_global_subscriber());
        return;
    }

    let output = Command::new(env::current_exe().expect("current test binary"))
        .arg("--exact")
        .arg("bootstrap_surface_initializes_tracing_only_once")
        .arg("--nocapture")
        .env("OBSERVABILITY_BOOTSTRAP_ONCE_HELPER", "1")
        .output()
        .expect("spawn bootstrap helper");

    assert!(
        output.status.success(),
        "bootstrap helper failed: stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
