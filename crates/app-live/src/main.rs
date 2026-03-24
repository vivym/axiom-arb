use std::{env, process, str::FromStr};

use app_live::{AppRuntime, AppRuntimeMode};
use observability::Observability;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let app_mode = env::var("AXIOM_MODE").unwrap_or_else(|_| "paper".to_owned());
    let app_mode = AppRuntimeMode::from_str(&app_mode)?;
    let observability = Observability::new("app-live");
    let runtime = AppRuntime::new(app_mode);
    let mode_metric = observability
        .metrics()
        .runtime_mode
        .sample(runtime.app_mode().as_str());

    println!(
        "app-live starting in {} mode with bootstrap {:?} ({})",
        runtime.app_mode().as_str(),
        runtime.bootstrap_status(),
        mode_metric.mode()
    );

    Ok(())
}
