mod support;

use std::process::Command;

use support::cli;

#[test]
fn apply_subcommand_is_exposed() {
    let output = Command::new(cli::app_live_binary())
        .arg("apply")
        .arg("--help")
        .output()
        .expect("app-live apply --help should execute");

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("--config"), "{text}");
    assert!(text.contains("--start"), "{text}");
}

#[test]
fn apply_rejects_non_smoke_config_with_specific_guidance() {
    let paper_output = Command::new(cli::app_live_binary())
        .arg("apply")
        .arg("--config")
        .arg(cli::config_fixture("app-live-paper.toml"))
        .output()
        .expect("app-live apply should execute for paper config");
    let paper_text = cli::combined(&paper_output);
    assert!(!paper_output.status.success(), "{paper_text}");
    assert!(paper_text.contains("bootstrap"), "{paper_text}");
    assert!(paper_text.contains("run"), "{paper_text}");

    let live_output = Command::new(cli::app_live_binary())
        .arg("apply")
        .arg("--config")
        .arg(cli::config_fixture("app-live-live.toml"))
        .output()
        .expect("app-live apply should execute for live config");
    let live_text = cli::combined(&live_output);
    assert!(!live_output.status.success(), "{live_text}");
    assert!(live_text.contains("status -> doctor -> run"), "{live_text}");
}
