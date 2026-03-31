use std::error::Error;

use persistence::connect_pool_from_env;

use crate::{
    cli::TargetShowCurrentArgs,
    commands::targets::state::{load_target_control_plane_state, TargetControlPlaneState},
};

pub fn execute(args: TargetShowCurrentArgs) -> Result<(), Box<dyn Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let state = runtime.block_on(async {
        let pool = connect_pool_from_env().await?;
        load_target_control_plane_state(&pool, &args.config).await
    })?;

    print_current(&state);
    Ok(())
}

fn print_current(state: &TargetControlPlaneState) {
    println!(
        "configured_operator_target_revision = {}",
        state
            .configured_operator_target_revision
            .as_deref()
            .unwrap_or("unavailable")
    );
    println!(
        "active_operator_target_revision = {}",
        state
            .active_operator_target_revision
            .as_deref()
            .unwrap_or("unavailable")
    );
    println!(
        "restart_needed = {}",
        match state.restart_needed {
            Some(true) => "true",
            Some(false) => "false",
            None => "unknown",
        }
    );
    if let Some(provenance) = state.provenance.as_ref() {
        println!(
            "adoptable_revision = {}",
            provenance.adoptable_revision.as_str()
        );
        println!(
            "candidate_revision = {}",
            provenance.candidate_revision.as_str()
        );
    } else {
        println!("adoptable_revision = unavailable");
        println!("candidate_revision = unavailable");
    }
    if let Some(latest_action) = state.latest_action.as_ref() {
        println!("latest_action_kind = {}", latest_action.action_kind);
        println!(
            "latest_action_operator_target_revision = {}",
            latest_action.operator_target_revision
        );
    } else {
        println!("latest_action_kind = unavailable");
        println!("latest_action_operator_target_revision = unavailable");
    }
}
