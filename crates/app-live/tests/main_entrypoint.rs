use std::{path::PathBuf, process::Command};

#[test]
fn binary_entrypoint_runs_runtime_bootstrap_path() {
    let output = Command::new(app_live_binary())
        .env("AXIOM_MODE", "paper")
        .output()
        .expect("app-live should run");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    assert!(stdout.contains("app_mode=paper"));
    assert!(stdout.contains("bootstrap_status=Ready"));
    assert!(stdout.contains("promoted_from_bootstrap=true"));
    assert!(stdout.contains("runtime_mode=Healthy"));
    assert!(stdout.contains("fullset_mode=Live"));
    assert!(stdout.contains("negrisk_mode=Shadow"));
    assert!(stdout.contains("published_snapshot_id=none"));
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
