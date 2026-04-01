use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

#[test]
fn bootstrap_help_lists_command() {
    let output = Command::new(app_live_binary())
        .arg("--help")
        .output()
        .unwrap();

    assert!(output.status.success(), "{}", combined(&output));
    assert!(
        combined(&output).contains("bootstrap"),
        "expected `bootstrap` in help output, got:\n{}",
        combined(&output)
    );
}

#[test]
fn bootstrap_defaults_to_local_config_for_paper() {
    let temp = tempfile::tempdir().expect("temp dir");
    let config_path = temp.path().join("config").join("axiom-arb.local.toml");

    let mut child = Command::new(app_live_binary())
        .arg("bootstrap")
        .current_dir(temp.path())
        .env("DATABASE_URL", default_test_database_url())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live bootstrap should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"paper\n")
        .expect("wizard answers should write");

    let output = child.wait_with_output().expect("output");
    assert!(output.status.success(), "{}", combined(&output));
    assert!(
        config_path.exists(),
        "expected default config path to exist: {}",
        config_path.display()
    );
    assert!(
        fs::read_to_string(&config_path)
            .expect("generated config should exist")
            .contains("mode = \"paper\""),
        "expected paper config at {}",
        config_path.display()
    );
    assert!(
        combined(&output).contains("Paper bootstrap ready"),
        "expected ready summary, got:\n{}",
        combined(&output)
    );
    assert!(
        combined(&output).contains("Runtime not started"),
        "expected preflight-only summary, got:\n{}",
        combined(&output)
    );
}

#[test]
fn bootstrap_paper_start_runs_runtime_after_preflight() {
    let temp = tempfile::tempdir().expect("temp dir");

    let mut child = Command::new(app_live_binary())
        .arg("bootstrap")
        .arg("--start")
        .current_dir(temp.path())
        .env("DATABASE_URL", default_test_database_url())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live bootstrap should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"paper\n")
        .expect("wizard answers should write");

    let output = child.wait_with_output().expect("output");
    assert!(output.status.success(), "{}", combined(&output));
    assert!(
        combined(&output).contains("app_mode=paper"),
        "expected paper runtime output, got:\n{}",
        combined(&output)
    );
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

fn default_test_database_url() -> &'static str {
    "postgres://axiom:axiom@localhost:5432/axiom_arb"
}
