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

    print_current(&state, &args.config);
    Ok(())
}

fn print_current(state: &TargetControlPlaneState, config_path: &std::path::Path) {
    println!(
        "configured_operator_strategy_revision = {}",
        state
            .configured_operator_strategy_revision
            .as_deref()
            .unwrap_or("unavailable")
    );
    println!(
        "active_operator_strategy_revision = {}",
        state
            .active_operator_strategy_revision
            .as_deref()
            .unwrap_or("unavailable")
    );
    println!(
        "migration_required = {}",
        migration_required_value(state.compatibility_mode.as_deref(), config_path)
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
            provenance.adoptable_strategy_revision.as_str()
        );
        println!(
            "strategy_candidate_revision = {}",
            provenance.strategy_candidate_revision.as_str()
        );
    } else {
        println!("adoptable_revision = unavailable");
        println!("strategy_candidate_revision = unavailable");
    }
    if let Some(latest_action) = state.latest_action.as_ref() {
        println!("latest_action_kind = {}", latest_action.action_kind);
        println!(
            "latest_action_operator_strategy_revision = {}",
            latest_action.operator_strategy_revision
        );
    } else {
        println!("latest_action_kind = unavailable");
        println!("latest_action_operator_strategy_revision = unavailable");
    }
}

fn migration_required_value(
    compatibility_mode: Option<&str>,
    config_path: &std::path::Path,
) -> String {
    if compatibility_mode.is_some() {
        format!(
            "app-live targets adopt --config {}",
            shell_quote(config_path.display().to_string())
        )
    } else {
        "none".to_owned()
    }
}

fn shell_quote(value: String) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '-' | '_'))
    {
        value
    } else {
        format!("'{}'", value.replace('\'', r"'\''"))
    }
}
