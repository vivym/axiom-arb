use observability::span_names;
use std::{ffi::OsString, path::PathBuf, process::Command};

#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;

#[test]
fn binary_entrypoint_emits_structured_bootstrap_log() {
    let output = app_live_output("paper", None);

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
    assert!(
        combined.contains("app-live bootstrap complete"),
        "{combined}"
    );
    assert!(combined.contains("app_mode=paper"), "{combined}");
    assert!(combined.contains("bootstrap_status=Ready"), "{combined}");
    assert!(
        combined.contains("promoted_from_bootstrap=true"),
        "{combined}"
    );
    assert!(combined.contains("runtime_mode=Healthy"), "{combined}");
    assert!(combined.contains("fullset_mode=Live"), "{combined}");
    assert!(combined.contains("negrisk_mode=Shadow"), "{combined}");
    assert!(combined.contains("pending_reconcile_count=0"), "{combined}");
    assert!(
        combined.contains("published_snapshot_id=snapshot-0"),
        "{combined}"
    );
}

#[test]
fn binary_entrypoint_emits_structured_error_log_for_invalid_mode() {
    let output = app_live_output("invalid-mode", None);

    assert!(
        !output.status.success(),
        "binary should fail for invalid AXIOM_MODE"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(
        combined.contains("ERROR app-live bootstrap failed"),
        "{combined}"
    );
    assert!(
        combined.contains("unsupported AXIOM_MODE 'invalid-mode'"),
        "{combined}"
    );
}

#[test]
fn paper_entrypoint_ignores_invalid_neg_risk_target_config() {
    let output = app_live_output(
        "paper",
        Some(
            r#"
            [
              {
                "family_id": "family-a",
                "members": [
                  { "condition_id": "condition-1", "token_id": "token-1", "price": "0.43", "quantity": "5" }
                ]
              },
            ]
            "#,
        ),
    );

    assert!(
        output.status.success(),
        "paper mode should ignore live config"
    );
}

#[test]
fn live_entrypoint_rejects_invalid_neg_risk_target_config() {
    let output = app_live_output(
        "live",
        Some(
            r#"
            [
              {
                "family_id": "family-a",
                "members": [
                  { "condition_id": "condition-1", "token_id": "token-1", "price": "0.43", "quantity": "5" }
                ]
              },
            ]
            "#,
        ),
    );

    assert!(
        !output.status.success(),
        "binary should fail for invalid neg-risk live target config"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(
        combined.contains("ERROR app-live bootstrap failed"),
        "{combined}"
    );
    assert!(
        combined.contains("invalid neg-risk live target config"),
        "{combined}"
    );
}

#[cfg(unix)]
#[test]
fn live_entrypoint_rejects_non_utf8_neg_risk_target_config() {
    let output = app_live_output_raw_env("live", Some(OsString::from_vec(vec![0xff, 0xfe, 0xfd])));

    assert!(
        !output.status.success(),
        "binary should fail for non-UTF-8 neg-risk live target config"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(
        combined.contains("invalid value for AXIOM_NEG_RISK_LIVE_TARGETS"),
        "{combined}"
    );
}

fn app_live_output(app_mode: &str, neg_risk_live_targets: Option<&str>) -> std::process::Output {
    app_live_output_raw_env(app_mode, neg_risk_live_targets.map(OsString::from))
}

fn app_live_output_raw_env(
    app_mode: &str,
    neg_risk_live_targets: Option<impl Into<OsString>>,
) -> std::process::Output {
    let mut command = Command::new(app_live_binary());
    command.env("AXIOM_MODE", app_mode);
    if let Some(value) = neg_risk_live_targets {
        command.env("AXIOM_NEG_RISK_LIVE_TARGETS", value.into());
    }
    command.output().expect("app-live should run")
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
