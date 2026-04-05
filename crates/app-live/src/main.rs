use std::process;

use app_live::cli::{AppLiveCli, AppLiveCommand};
use app_live::commands::apply::execute as apply_execute;
use app_live::commands::bootstrap::execute as bootstrap_execute;
use app_live::commands::discover::execute as discover_execute;
use app_live::commands::doctor::execute as doctor_execute;
use app_live::commands::init::execute as init_execute;
use app_live::commands::run::execute as run_execute;
use app_live::commands::status::execute as status_execute;
use app_live::commands::targets::execute as targets_execute;
use app_live::commands::verify::execute as verify_execute;
use clap::Parser;
use tracing_subscriber::EnvFilter;

fn main() {
    init_tracing();
    if let Err(error) = run() {
        tracing::error!(error = %error, "app-live bootstrap failed");
        process::exit(1);
    }
}

fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_target(false)
        .without_time()
        .with_env_filter(env_filter)
        .try_init();
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = AppLiveCli::parse();
    match cli.command {
        AppLiveCommand::Apply(args) => apply_execute(args),
        AppLiveCommand::Bootstrap(args) => bootstrap_execute(args),
        AppLiveCommand::Discover(args) => discover_execute(args),
        AppLiveCommand::Doctor(args) => doctor_execute(args),
        AppLiveCommand::Init(args) => init_execute(args),
        AppLiveCommand::Status(args) => status_execute(args),
        AppLiveCommand::Run(args) => run_execute(args),
        AppLiveCommand::Targets(args) => targets_execute(args),
        AppLiveCommand::Verify(args) => verify_execute(args),
    }
}
