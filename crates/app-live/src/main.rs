use std::{env, process, str::FromStr};

use app_live::{run_live, run_paper, AppRuntimeMode, StaticSnapshotSource};
use domain::RuntimeMode;
use observability::{bootstrap_observability, span_names};

fn main() {
    if let Err(error) = run() {
        tracing::error!(error = %error, "app-live bootstrap failed");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let observability = bootstrap_observability("app-live");
    let bootstrap_span = tracing::info_span!(span_names::APP_BOOTSTRAP);
    let _bootstrap_guard = bootstrap_span.enter();
    let app_mode = env::var("AXIOM_MODE").unwrap_or_else(|_| "paper".to_owned());
    let app_mode = AppRuntimeMode::from_str(&app_mode)?;
    let source = StaticSnapshotSource::empty();
    let result = match app_mode {
        AppRuntimeMode::Paper => run_paper(&source),
        AppRuntimeMode::Live => run_live(&source),
    };
    observability
        .recorder()
        .record_runtime_mode(runtime_mode_label(result.runtime.runtime_mode()));

    let completion_span = tracing::info_span!(
        span_names::APP_BOOTSTRAP_COMPLETE,
        app_mode = %result.runtime.app_mode().as_str(),
        bootstrap_status = ?result.runtime.bootstrap_status(),
        promoted_from_bootstrap = result.report.promoted_from_bootstrap,
        runtime_mode = ?result.runtime.runtime_mode(),
        fullset_mode = ?result.summary.fullset_mode,
        negrisk_mode = ?result.summary.negrisk_mode,
        published_snapshot_id = %result
            .summary
            .published_snapshot_id
            .as_deref()
            .unwrap_or("none")
    );
    let _completion_guard = completion_span.enter();
    tracing::info!("app-live bootstrap complete");

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
