use std::{fs, path::PathBuf, process::Command};

#[test]
fn init_defaults_write_minimal_paper_config() {
    let temp = tempfile::NamedTempFile::new().expect("temp file");

    let output = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(temp.path())
        .arg("--defaults")
        .arg("--mode")
        .arg("paper")
        .output()
        .expect("app-live init should execute");

    assert!(output.status.success(), "{}", combined(&output));
    let text = fs::read_to_string(temp.path()).expect("generated config should exist");
    assert!(text.contains("[runtime]"));
    assert!(text.contains("mode = \"paper\""));
    assert!(!text.contains("timestamp ="));
    assert!(!text.contains("[[negrisk.targets]]"));
}

#[test]
fn init_live_smoke_defaults_write_target_source_not_raw_targets() {
    let temp = tempfile::NamedTempFile::new().expect("temp file");

    let output = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(temp.path())
        .arg("--defaults")
        .arg("--mode")
        .arg("live")
        .arg("--real-user-shadow-smoke")
        .output()
        .expect("app-live init should execute");

    assert!(output.status.success(), "{}", combined(&output));
    let text = fs::read_to_string(temp.path()).expect("generated config should exist");
    assert!(text.contains("[negrisk.target_source]"));
    assert!(text.contains("source = \"adopted\""));
    assert!(!text.contains("[[negrisk.targets]]"));
    assert!(!text.contains("timestamp ="));
    assert!(!text.contains("signature ="));
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

fn combined(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
