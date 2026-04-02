mod support;

use std::process::Command;

use support::cli;

#[test]
fn verify_subcommand_is_exposed() {
    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--help")
        .output()
        .expect("app-live verify --help should execute");

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("--config"), "{text}");
    assert!(text.contains("--expect"), "{text}");
    assert!(text.contains("--from-seq"), "{text}");
    assert!(text.contains("--to-seq"), "{text}");
    assert!(text.contains("--attempt-id"), "{text}");
    assert!(text.contains("--since"), "{text}");
}

#[test]
fn verify_placeholder_fails_for_missing_config() {
    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg("/definitely/missing.toml")
        .output()
        .expect("app-live verify --config /definitely/missing.toml should execute");

    let text = cli::combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("not implemented"), "{text}");
}
