#[path = "support/cli.rs"]
mod cli;

use std::process::Command;

#[test]
fn run_subcommand_starts_paper_mode_from_operator_config() {
    let output = Command::new(cli::app_live_binary())
        .arg("run")
        .arg("--config")
        .arg(cli::config_fixture("app-live-paper.toml"))
        .env("DATABASE_URL", cli::default_test_database_url())
        .output()
        .expect("app-live run should execute");

    assert!(output.status.success(), "{}", cli::combined(&output));
    assert!(cli::combined(&output).contains("app_mode=paper"));
}
