use std::error::Error;

use persistence::connect_pool_from_env;

use crate::{
    cli::TargetCandidatesArgs,
    commands::targets::state::{
        load_target_candidates_catalog, load_target_control_plane_state,
        summarize_target_candidates, TargetCandidatesCatalog, TargetControlPlaneState,
    },
};

pub fn execute(args: TargetCandidatesArgs) -> Result<(), Box<dyn Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let (state, catalog) = runtime.block_on(async {
        let pool = connect_pool_from_env().await?;
        let state = load_target_control_plane_state(&pool, &args.config).await?;
        let catalog = load_target_candidates_catalog(&pool).await?;
        Ok::<_, Box<dyn Error>>((state, catalog))
    })?;

    print_candidates(&state, &catalog);
    Ok(())
}

fn print_candidates(state: &TargetControlPlaneState, catalog: &TargetCandidatesCatalog) {
    let summary = summarize_target_candidates(catalog);
    println!(
        "recommended_adoptable_revision = {}",
        summary
            .recommended_adoptable_revision
            .as_deref()
            .unwrap_or("none")
    );
    println!(
        "non_adoptable_summary = {}",
        summary.non_adoptable_summary()
    );
    println!(
        "compatibility_mode = {}",
        state.compatibility_mode.as_deref().unwrap_or("none")
    );

    if catalog.advisory_candidates.is_empty() {
        println!("advisory = none");
    } else {
        for candidate in &catalog.advisory_candidates {
            println!(
                "advisory strategy_candidate_revision = {} snapshot_id = {}",
                candidate.strategy_candidate_revision, candidate.snapshot_id
            );
        }
    }

    if catalog.adoptable_revisions.is_empty() {
        println!("adoptable = none");
    } else {
        for line in adoptable_revision_lines(catalog) {
            println!("{line}");
        }
    }

    if let Some(operator_strategy_revision) = state.configured_operator_strategy_revision.as_deref()
    {
        let adoptable_revision = state
            .provenance
            .as_ref()
            .map(|row| row.adoptable_strategy_revision.as_str())
            .unwrap_or("unavailable");
        let strategy_candidate_revision = state
            .provenance
            .as_ref()
            .map(|row| row.strategy_candidate_revision.as_str())
            .unwrap_or("unavailable");
        println!(
            "adopted operator_strategy_revision = {} adoptable_revision = {} strategy_candidate_revision = {}",
            operator_strategy_revision, adoptable_revision, strategy_candidate_revision
        );
    } else if let Some(mode) = state.compatibility_mode.as_deref() {
        println!("adopted = compatibility:{mode}");
    } else {
        println!("adopted = none");
    }
}

pub(crate) fn adoptable_revision_lines(catalog: &TargetCandidatesCatalog) -> Vec<String> {
    catalog
        .adoptable_revisions
        .iter()
        .map(|adoptable| {
            format!(
                "adoptable adoptable_revision = {} strategy_candidate_revision = {} operator_strategy_revision = {}",
                adoptable.adoptable_strategy_revision,
                adoptable.strategy_candidate_revision,
                adoptable.rendered_operator_strategy_revision
            )
        })
        .collect()
}
