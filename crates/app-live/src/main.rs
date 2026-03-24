use std::{env, process, str::FromStr};

use app_live::{run_live, run_paper, AppRuntimeMode, StaticSnapshotSource};
use domain::RuntimeMode;
use observability::{bootstrap_tracing, Observability};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let app_mode = env::var("AXIOM_MODE").unwrap_or_else(|_| "paper".to_owned());
    let app_mode = AppRuntimeMode::from_str(&app_mode)?;
    let _tracing = bootstrap_tracing("app-live");
    let observability = Observability::new("app-live");
    let source = StaticSnapshotSource::empty();
    let result = match app_mode {
        AppRuntimeMode::Paper => run_paper(&source),
        AppRuntimeMode::Live => run_live(&source),
    };
    observability
        .recorder()
        .record_runtime_mode(runtime_mode_label(result.runtime.runtime_mode()));

    println!(
        "app-live starting app_mode={} bootstrap_status={:?} promoted_from_bootstrap={} runtime_mode={:?} fullset_mode={:?} negrisk_mode={:?} pending_reconcile_count={} published_snapshot_id={}",
        result.runtime.app_mode().as_str(),
        result.runtime.bootstrap_status(),
        result.report.promoted_from_bootstrap,
        result.runtime.runtime_mode(),
        result.summary.fullset_mode,
        result.summary.negrisk_mode,
        result.summary.pending_reconcile_count,
        result
            .summary
            .published_snapshot_id
            .as_deref()
            .unwrap_or("none")
    );

    Ok(())
}

fn runtime_mode_label(mode: RuntimeMode) -> &'static str {
    match mode {
        RuntimeMode::Bootstrapping => "bootstrapping",
        RuntimeMode::Healthy => "healthy",
        RuntimeMode::Reconciling => "reconciling",
        RuntimeMode::Degraded => "degraded",
        RuntimeMode::NoNewRisk => "no_new_risk",
        RuntimeMode::GlobalHalt => "global_halt",
    }
}
