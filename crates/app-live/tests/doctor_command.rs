use std::{path::PathBuf, process::Command};

#[test]
fn doctor_paper_mode_marks_live_checks_as_skip() {
    let output = Command::new(app_live_binary())
        .arg("doctor")
        .arg("--config")
        .arg(config_fixture("fixtures/app-live-paper.toml"))
        .output()
        .expect("app-live doctor should execute");

    assert!(output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert!(combined.contains("[OK] config parsed"), "{combined}");
    assert!(
        combined.contains("[SKIP] REST authentication not required in paper mode"),
        "{combined}"
    );
}

#[test]
fn doctor_live_mode_reports_missing_adopted_target_as_target_source_error() {
    let output = Command::new(app_live_binary())
        .arg("doctor")
        .arg("--config")
        .arg(config_fixture("fixtures/app-live-ux-live.toml"))
        .env("DATABASE_URL", default_test_database_url())
        .output()
        .expect("app-live doctor should execute");

    assert!(!output.status.success(), "{}", combined(&output));
    assert!(combined(&output).contains("TargetSourceError"));
}

fn app_live_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("CARGO_BIN_EXE_app-live") {
        return PathBuf::from(path);
    }

    let mut path = std::env::current_exe().expect("current test executable path");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("app-live");
    if cfg!(windows) {
        path.set_extension("exe");
    }

    path
}

fn config_fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("config-schema")
        .join("tests")
        .join(relative)
}

fn default_test_database_url() -> &'static str {
    "postgres://axiom:axiom@localhost:5432/axiom_arb"
}

fn combined(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
