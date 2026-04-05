#[path = "support/cli.rs"]
mod cli;
#[path = "support/run_session_db.rs"]
mod run_session_db;

use std::process::Command;

use persistence::RunSessionState;

#[test]
fn run_subcommand_starts_paper_mode_from_operator_config() {
    let db = run_session_db::TestDatabase::new();

    let output = Command::new(cli::app_live_binary())
        .arg("run")
        .arg("--config")
        .arg(cli::config_fixture("app-live-paper.toml"))
        .env("DATABASE_URL", db.database_url())
        .output()
        .expect("app-live run should execute");

    assert!(output.status.success(), "{}", cli::combined(&output));
    assert!(cli::combined(&output).contains("app_mode=paper"));

    db.cleanup();
}

#[test]
fn run_paper_creates_running_then_exited_session() {
    let db = run_session_db::TestDatabase::new();

    let output = Command::new(cli::app_live_binary())
        .arg("run")
        .arg("--config")
        .arg(cli::config_fixture("app-live-paper.toml"))
        .env("DATABASE_URL", db.database_url())
        .output()
        .expect("paper run should execute");

    assert!(output.status.success(), "{}", cli::combined(&output));

    let session = db.latest_session().expect("paper run session should exist");
    assert_eq!(session.mode, "paper");
    assert_eq!(session.invoked_by, "run");
    assert_eq!(session.state, RunSessionState::Exited);

    db.cleanup();
}

#[test]
fn run_startup_failure_records_failed_session() {
    let db = run_session_db::TestDatabase::new();

    let output = Command::new(cli::app_live_binary())
        .arg("run")
        .arg("--config")
        .arg(cli::config_fixture("app-live-ux-live.toml"))
        .env("DATABASE_URL", db.database_url())
        .output()
        .expect("broken live run should execute");

    assert!(!output.status.success(), "{}", cli::combined(&output));

    let session = db
        .latest_session()
        .expect("failed run session should exist");
    assert_eq!(session.mode, "live");
    assert_eq!(session.invoked_by, "run");
    assert_eq!(session.state, RunSessionState::Failed);

    db.cleanup();
}
