use std::{path::PathBuf, process::Command};

#[test]
fn bootstrap_help_lists_command() {
    let output = Command::new(app_live_binary()).arg("--help").output().unwrap();

    assert!(output.status.success(), "{}", combined(&output));
    assert!(
        combined(&output).contains("bootstrap"),
        "expected `bootstrap` in help output, got:\n{}",
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
