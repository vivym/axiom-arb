use std::error::Error;

use persistence::connect_pool_from_env;

use crate::{
    cli::TargetStatusArgs,
    commands::targets::state::{load_target_control_plane_state, TargetControlPlaneState},
};

pub fn execute(args: TargetStatusArgs) -> Result<(), Box<dyn Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let state = runtime.block_on(async {
        let pool = connect_pool_from_env().await?;
        load_target_control_plane_state(&pool, &args.config).await
    })?;

    print_status(&state);
    Ok(())
}

fn print_status(state: &TargetControlPlaneState) {
    println!(
        "configured_operator_strategy_revision = {}",
        optional_revision(state.configured_operator_strategy_revision.as_deref())
    );
    println!(
        "active_operator_strategy_revision = {}",
        optional_revision(state.active_operator_strategy_revision.as_deref())
    );
    println!(
        "compatibility_mode = {}",
        state.compatibility_mode.as_deref().unwrap_or("none")
    );
    println!(
        "restart_needed = {}",
        optional_restart_needed(state.restart_needed)
    );
    println!(
        "provenance = {}",
        if state.provenance.is_some() {
            "complete"
        } else {
            "unavailable"
        }
    );
    println!(
        "latest_action = {}",
        state
            .latest_action
            .as_ref()
            .map(|row| format!("{}:{}", row.action_kind, row.operator_strategy_revision))
            .unwrap_or_else(|| "unavailable".to_owned())
    );
}

fn optional_revision(value: Option<&str>) -> &str {
    value.unwrap_or("unavailable")
}

fn optional_restart_needed(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "unknown",
    }
}
