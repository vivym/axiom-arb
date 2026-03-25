use std::{path::PathBuf, process::Command};

#[test]
fn binary_entrypoint_emits_structured_bootstrap_log() {
    let output = Command::new(app_live_binary())
        .env("AXIOM_MODE", "paper")
        .output()
        .expect("app-live should run");

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
    assert!(combined.contains("INFO app-live starting"), "{combined}");
    assert!(combined.contains("app_mode=paper"), "{combined}");
    assert!(combined.contains("bootstrap_status=Ready"), "{combined}");
    assert!(combined.contains("promoted_from_bootstrap=true"), "{combined}");
    assert!(combined.contains("runtime_mode=Healthy"), "{combined}");
    assert!(combined.contains("fullset_mode=Live"), "{combined}");
    assert!(combined.contains("negrisk_mode=Shadow"), "{combined}");
    assert!(combined.contains("published_snapshot_id=snapshot-0"), "{combined}");
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
