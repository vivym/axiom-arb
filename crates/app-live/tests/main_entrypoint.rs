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
fn paper_entrypoint_ignores_invalid_local_signer_config() {
    let output =
        app_live_output_raw_env_with_signer("paper", None, None, None, Some(OsString::from("{")));

    assert!(
        output.status.success(),
        "paper mode should ignore live signer config"
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

#[test]
fn live_entrypoint_rejects_duplicate_neg_risk_target_config() {
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
              {
                "family_id": "family-a",
                "members": [
                  { "condition_id": "condition-2", "token_id": "token-2", "price": "0.41", "quantity": "5" }
                ]
              }
            ]
            "#,
        ),
    );

    assert!(
        !output.status.success(),
        "binary should fail for duplicate neg-risk family ids"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(
        combined.contains("duplicate neg-risk family_id in live target config"),
        "{combined}"
    );
}

#[test]
fn live_entrypoint_rejects_blank_neg_risk_target_config() {
    let output = app_live_output("live", Some(""));

    assert!(
        !output.status.success(),
        "binary should fail for blank neg-risk live target config"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(
        combined.contains("invalid neg-risk live target config"),
        "{combined}"
    );
}

#[cfg(unix)]
#[test]
fn live_entrypoint_rejects_non_utf8_neg_risk_target_config() {
    let output = app_live_output_raw_env(
        "live",
        Some(OsString::from_vec(vec![0xff, 0xfe, 0xfd])),
        Option::<OsString>::None,
        Option::<OsString>::None,
    );

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

#[cfg(unix)]
#[test]
fn paper_entrypoint_ignores_non_utf8_live_only_env_vars() {
    let cases = [
        (
            Some(OsString::from_vec(vec![0xff, 0xfe, 0xfd])),
            Option::<OsString>::None,
            Option::<OsString>::None,
        ),
        (
            Option::<OsString>::None,
            Some(OsString::from_vec(vec![0xff, 0xfe, 0xfd])),
            Option::<OsString>::None,
        ),
        (
            Option::<OsString>::None,
            Option::<OsString>::None,
            Some(OsString::from_vec(vec![0xff, 0xfe, 0xfd])),
        ),
    ];

    for (targets, approved, ready) in cases {
        let output = app_live_output_raw_env("paper", targets, approved, ready);
        assert!(
            output.status.success(),
            "paper mode should ignore live-only env vars"
        );
    }
}

#[test]
fn live_entrypoint_boots_without_neg_risk_target_config() {
    let output = app_live_output("live", None);

    assert!(
        output.status.success(),
        "live mode should boot without config"
    );
}

#[test]
fn live_entrypoint_surfaces_live_negrisk_mode_when_explicit_operator_inputs_agree() {
    let output = app_live_output_with_operator_inputs_and_signer(
        "live",
        Some(
            r#"
            [
              {
                "family_id": "family-a",
                "members": [
                  { "condition_id": "condition-1", "token_id": "token-1", "price": "0.43", "quantity": "5" }
                ]
              }
            ]
            "#,
        ),
        Some("family-a"),
        Some("family-a"),
        Some(valid_local_signer_config_json()),
    );

    assert!(
        output.status.success(),
        "live mode should boot with explicit operator inputs"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(combined.contains("negrisk_mode=Live"), "{combined}");
    assert!(
        combined.contains("neg_risk_live_attempt_count=1"),
        "{combined}"
    );
    assert!(
        combined.contains("neg_risk_live_state_source=\"synthetic_bootstrap\""),
        "{combined}"
    );
}

#[test]
fn live_entrypoint_rejects_missing_local_signer_config_when_live_work_is_requested() {
    let output = app_live_output_with_operator_inputs(
        "live",
        Some(
            r#"
            [
              {
                "family_id": "family-a",
                "members": [
                  { "condition_id": "condition-1", "token_id": "token-1", "price": "0.43", "quantity": "5" }
                ]
              }
            ]
            "#,
        ),
        Some("family-a"),
        Some("family-a"),
    );

    assert!(
        !output.status.success(),
        "binary should fail when live neg-risk work is requested without signer config"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(
        combined.contains("missing local signer config"),
        "{combined}"
    );
}

#[test]
fn live_entrypoint_rejects_invalid_local_signer_config_when_live_work_is_requested() {
    let output = app_live_output_raw_env_with_signer(
        "live",
        Some(
            r#"
            [
              {
                "family_id": "family-a",
                "members": [
                  { "condition_id": "condition-1", "token_id": "token-1", "price": "0.43", "quantity": "5" }
                ]
              }
            ]
            "#,
        ),
        Some("family-a"),
        Some("family-a"),
        Some(OsString::from("{")),
    );

    assert!(
        !output.status.success(),
        "binary should fail when live neg-risk work is requested with invalid signer config"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(
        combined.contains("invalid local signer config"),
        "{combined}"
    );
}

fn app_live_output(app_mode: &str, neg_risk_live_targets: Option<&str>) -> std::process::Output {
    app_live_output_with_operator_inputs(app_mode, neg_risk_live_targets, None, None)
}

fn app_live_output_with_operator_inputs(
    app_mode: &str,
    neg_risk_live_targets: Option<&str>,
    approved_families: Option<&str>,
    ready_families: Option<&str>,
) -> std::process::Output {
    app_live_output_raw_env_with_signer(
        app_mode,
        neg_risk_live_targets.map(OsString::from),
        approved_families.map(OsString::from),
        ready_families.map(OsString::from),
        None,
    )
}

fn app_live_output_raw_env(
    app_mode: &str,
    neg_risk_live_targets: Option<OsString>,
    approved_families: Option<OsString>,
    ready_families: Option<OsString>,
) -> std::process::Output {
    app_live_output_raw_env_with_signer(
        app_mode,
        neg_risk_live_targets,
        approved_families,
        ready_families,
        None,
    )
}

fn app_live_output_with_operator_inputs_and_signer(
    app_mode: &str,
    neg_risk_live_targets: Option<&str>,
    approved_families: Option<&str>,
    ready_families: Option<&str>,
    local_signer_config: Option<&str>,
) -> std::process::Output {
    app_live_output_raw_env_with_signer(
        app_mode,
        neg_risk_live_targets.map(OsString::from),
        approved_families.map(OsString::from),
        ready_families.map(OsString::from),
        local_signer_config.map(OsString::from),
    )
}

fn app_live_output_raw_env_with_signer(
    app_mode: &str,
    neg_risk_live_targets: Option<OsString>,
    approved_families: Option<OsString>,
    ready_families: Option<OsString>,
    local_signer_config: Option<OsString>,
) -> std::process::Output {
    let mut command = Command::new(app_live_binary());
    command.env("AXIOM_MODE", app_mode);
    command.env_remove("AXIOM_NEG_RISK_LIVE_TARGETS");
    command.env_remove("AXIOM_NEG_RISK_LIVE_APPROVED_FAMILIES");
    command.env_remove("AXIOM_NEG_RISK_LIVE_READY_FAMILIES");
    command.env_remove("AXIOM_LOCAL_SIGNER_CONFIG");
    if let Some(value) = neg_risk_live_targets {
        command.env("AXIOM_NEG_RISK_LIVE_TARGETS", value);
    }
    if let Some(value) = approved_families {
        command.env("AXIOM_NEG_RISK_LIVE_APPROVED_FAMILIES", value);
    }
    if let Some(value) = ready_families {
        command.env("AXIOM_NEG_RISK_LIVE_READY_FAMILIES", value);
    }
    if let Some(value) = local_signer_config {
        command.env("AXIOM_LOCAL_SIGNER_CONFIG", value);
    }
    command.output().expect("app-live should run")
}

fn valid_local_signer_config_json() -> &'static str {
    r#"
    {
      "signer": {
        "address": "0x1111111111111111111111111111111111111111",
        "funder_address": "0x2222222222222222222222222222222222222222",
        "signature_type": "Eoa",
        "wallet_route": "Eoa"
      },
      "l2_auth": {
        "api_key": "poly-api-key-1",
        "passphrase": "poly-passphrase-1",
        "timestamp": "1700000000",
        "signature": "poly-signature-1"
      },
      "relayer_auth": {
        "kind": "builder_api_key",
        "api_key": "builder-api-key-1",
        "timestamp": "1700000001",
        "passphrase": "builder-passphrase-1",
        "signature": "builder-signature-1"
      }
    }
    "#
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
