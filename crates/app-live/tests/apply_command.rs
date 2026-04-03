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

#[test]
fn apply_rejects_smoke_config_as_scaffold_only_for_now() {
    let smoke_output = Command::new(cli::app_live_binary())
        .arg("apply")
        .arg("--config")
        .arg(cli::config_fixture("app-live-ux-smoke.toml"))
        .output()
        .expect("app-live apply should execute for smoke config");
    let smoke_text = cli::combined(&smoke_output);
    assert!(!smoke_output.status.success(), "{smoke_text}");
    assert!(smoke_text.contains("scaffold"), "{smoke_text}");
    assert!(smoke_text.contains("not implemented"), "{smoke_text}");

    let smoke_start_output = Command::new(cli::app_live_binary())
        .arg("apply")
        .arg("--config")
        .arg(cli::config_fixture("app-live-ux-smoke.toml"))
        .arg("--start")
        .output()
        .expect("app-live apply --start should execute for smoke config");
    let smoke_start_text = cli::combined(&smoke_start_output);
    assert!(!smoke_start_output.status.success(), "{smoke_start_text}");
    assert!(smoke_start_text.contains("scaffold"), "{smoke_start_text}");
    assert!(
        smoke_start_text.contains("not implemented"),
        "{smoke_start_text}"
    );
}
