use observability::span_names;
use std::{path::PathBuf, process::Command};

#[test]
fn binary_entrypoint_emits_structured_replay_summary() {
    let Some(database_url) = std::env::var_os("DATABASE_URL") else {
        return;
    };

    let output = Command::new(app_replay_binary())
        .env("DATABASE_URL", database_url)
        .args(["--from-seq", "0", "--limit", "1"])
        .output()
        .expect("app-replay should run");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(
        !combined
            .lines()
            .any(|line| line.starts_with("app-replay processed_count=")),
        "legacy success line should no longer be printed: {combined}"
    );
    assert!(combined.contains(span_names::REPLAY_RUN), "{combined}");
    assert!(combined.contains("after_seq=0"), "{combined}");
    assert!(combined.contains(span_names::REPLAY_SUMMARY), "{combined}");
    assert!(combined.contains("processed_count="), "{combined}");
    assert!(combined.contains("last_journal_seq="), "{combined}");
    assert!(combined.contains("app-replay summary"), "{combined}");
    assert!(
        !combined.contains("replay summary emitted"),
        "legacy success message should no longer be printed: {combined}"
    );
}

#[test]
fn binary_entrypoint_emits_structured_error_log_for_invalid_database_url() {
    let output = Command::new(app_replay_binary())
        .env("DATABASE_URL", "not-a-valid-postgres-url")
        .args(["--from-seq", "0", "--limit", "1"])
        .output()
        .expect("app-replay should run");

    assert!(
        !output.status.success(),
        "binary should fail for invalid DATABASE_URL"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(combined.contains("app-replay replay failed"), "{combined}");
    assert!(combined.contains(span_names::REPLAY_RUN), "{combined}");
    assert!(combined.contains("after_seq=0"), "{combined}");
    assert!(combined.contains("error with configuration"), "{combined}");
}

#[test]
fn binary_entrypoint_emits_structured_error_log_for_invalid_cli_args() {
    let output = Command::new(app_replay_binary())
        .args(["--limit", "1"])
        .output()
        .expect("app-replay should run");

    assert!(
        !output.status.success(),
        "binary should fail for missing replay arguments"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(combined.contains("app-replay replay failed"), "{combined}");
    assert!(combined.contains(span_names::REPLAY_RUN), "{combined}");
    assert!(
        combined.contains("missing required argument --from-seq"),
        "{combined}"
    );
}

fn app_replay_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("CARGO_BIN_EXE_app-replay") {
        return PathBuf::from(path);
    }

    let mut path = std::env::current_exe().expect("current test executable path");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("app-replay");
    if cfg!(windows) {
        path.set_extension("exe");
    }

    path
}
