use observability::span_names;
use std::{path::PathBuf, process::Command};

#[test]
fn binary_entrypoint_emits_structured_bootstrap_log() {
    let output = app_live_output("paper");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(
        !combined
            .lines()
            .any(|line| line.starts_with("app-live starting ")),
        "legacy success line should no longer be printed: {combined}"
    );
    assert!(
        combined.contains(span_names::APP_BOOTSTRAP_COMPLETE),
        "{combined}"
    );
    assert!(combined.contains("app-live bootstrap complete"), "{combined}");
    assert!(combined.contains("app_mode=paper"), "{combined}");
    assert!(combined.contains("bootstrap_status=Ready"), "{combined}");
    assert!(combined.contains("promoted_from_bootstrap=true"), "{combined}");
    assert!(combined.contains("runtime_mode=Healthy"), "{combined}");
    assert!(combined.contains("fullset_mode=Live"), "{combined}");
    assert!(combined.contains("negrisk_mode=Shadow"), "{combined}");
    assert!(combined.contains("pending_reconcile_count=0"), "{combined}");
    assert!(combined.contains("published_snapshot_id=snapshot-0"), "{combined}");
}

#[test]
fn binary_entrypoint_emits_structured_error_log_for_invalid_mode() {
    let output = app_live_output("invalid-mode");

    assert!(
        !output.status.success(),
        "binary should fail for invalid AXIOM_MODE"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(combined.contains("ERROR app-live bootstrap failed"), "{combined}");
    assert!(
        combined.contains("unsupported AXIOM_MODE 'invalid-mode'"),
        "{combined}"
    );
}

fn app_live_output(app_mode: &str) -> std::process::Output {
    Command::new(app_live_binary())
        .env("AXIOM_MODE", app_mode)
        .output()
        .expect("app-live should run")
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
