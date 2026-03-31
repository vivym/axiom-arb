use std::error::Error;

use persistence::connect_pool_from_env;

use crate::{
    cli::TargetCandidatesArgs,
    commands::targets::state::{
        load_target_candidates_catalog, load_target_control_plane_state, TargetCandidatesCatalog,
        TargetControlPlaneState,
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
    if catalog.advisory_candidates.is_empty() {
        println!("advisory = none");
    } else {
        for candidate in &catalog.advisory_candidates {
            println!(
                "advisory candidate_revision = {} snapshot_id = {}",
                candidate.candidate_revision, candidate.snapshot_id
            );
        }
    }

    if catalog.adoptable_revisions.is_empty() {
        println!("adoptable = none");
    } else {
        for adoptable in &catalog.adoptable_revisions {
            println!(
                "adoptable adoptable_revision = {} candidate_revision = {} operator_target_revision = {}",
                adoptable.adoptable_revision,
                adoptable.candidate_revision,
                adoptable.rendered_operator_target_revision
            );
        }
    }

    if let Some(operator_target_revision) = state.configured_operator_target_revision.as_deref() {
        let adoptable_revision = state
            .provenance
            .as_ref()
            .map(|row| row.adoptable_revision.as_str())
            .unwrap_or("unavailable");
        let candidate_revision = state
            .provenance
            .as_ref()
            .map(|row| row.candidate_revision.as_str())
            .unwrap_or("unavailable");
        println!(
            "adopted operator_target_revision = {} adoptable_revision = {} candidate_revision = {}",
            operator_target_revision, adoptable_revision, candidate_revision
        );
    } else {
        println!("adopted = none");
    }
}
