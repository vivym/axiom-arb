use observability::bootstrap_observability;
use std::{
    env,
    process::{Command, Output},
};

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
    if helper_mode().as_deref() == Some("fresh") {
        let _first = bootstrap_observability("app-live");
        let _second = bootstrap_observability("app-live");
        return;
    }

    let output = spawn_bootstrap_helper("bootstrap_surface_initializes_tracing_only_once", "fresh");

    assert_bootstrap_helper_succeeded(&output);
    assert_eq!(bootstrap_log_count(&output), 1);
}

#[test]
fn bootstrap_surface_only_reports_bootstrap_when_it_installs_global_tracing() {
    if helper_mode().as_deref() == Some("preinstalled") {
        let subscriber = tracing_subscriber::fmt()
            .with_target(false)
            .without_time()
            .finish();
        let _ = ::tracing::subscriber::set_global_default(subscriber);

        let _first = bootstrap_observability("app-live");
        let _second = bootstrap_observability("app-live");
        return;
    }

    let output = spawn_bootstrap_helper(
        "bootstrap_surface_only_reports_bootstrap_when_it_installs_global_tracing",
        "preinstalled",
    );

    assert_bootstrap_helper_succeeded(&output);
    assert_eq!(bootstrap_log_count(&output), 0);
}

fn helper_mode() -> Option<String> {
    env::var("OBSERVABILITY_BOOTSTRAP_HELPER_MODE").ok()
}

fn spawn_bootstrap_helper(test_name: &str, helper_mode: &str) -> Output {
    Command::new(env::current_exe().expect("current test binary"))
        .arg("--exact")
        .arg(test_name)
        .arg("--nocapture")
        .env("OBSERVABILITY_BOOTSTRAP_HELPER_MODE", helper_mode)
        .output()
        .expect("spawn bootstrap helper")
}

fn assert_bootstrap_helper_succeeded(output: &Output) {
    assert!(
        output.status.success(),
        "bootstrap helper failed: stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn bootstrap_log_count(output: &Output) -> usize {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .matches("tracing bootstrapped")
    .count()
}
