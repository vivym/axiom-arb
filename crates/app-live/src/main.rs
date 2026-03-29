use std::process;

use app_live::cli::{AppLiveCli, AppLiveCommand};
use app_live::commands::run::execute as run_execute;
use clap::Parser;

fn main() {
    if let Err(error) = run() {
        tracing::error!(error = %error, "app-live bootstrap failed");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = AppLiveCli::parse();
    match cli.command {
        AppLiveCommand::Run(args) => run_execute(args),
    }
}
