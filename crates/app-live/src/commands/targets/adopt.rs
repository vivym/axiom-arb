use std::{error::Error, path::Path};

use chrono::Utc;
use persistence::{
    connect_pool_from_env,
    models::{CandidateAdoptionProvenanceRow, OperatorTargetAdoptionHistoryRow},
    CandidateAdoptionRepo, OperatorTargetAdoptionHistoryRepo,
};
use sqlx::PgPool;

use crate::{
    cli::TargetAdoptArgs,
    commands::targets::{
        config_file::rewrite_operator_target_revision,
        state::{
            configured_operator_target_revision, load_active_operator_target_revision,
            resolve_adoption_selection, ResolvedAdoptionSelection,
        },
    },
};

pub fn execute(args: TargetAdoptArgs) -> Result<(), Box<dyn Error>> {
    if let Err(error) = execute_inner(args) {
        eprintln!("{error}");
        return Err(error);
    }

    Ok(())
}

fn execute_inner(args: TargetAdoptArgs) -> Result<(), Box<dyn Error>> {
    validate_selector_flags(
        args.operator_target_revision.as_deref(),
        args.adoptable_revision.as_deref(),
    )?;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let summary = runtime.block_on(async {
        let pool = connect_pool_from_env().await?;
        adopt_selected_revision(
            &pool,
            &args.config,
            args.operator_target_revision.as_deref(),
            args.adoptable_revision.as_deref(),
        )
        .await
    })?;

    print_summary(&summary);
    Ok(())
}

fn validate_selector_flags(
    operator_target_revision: Option<&str>,
    adoptable_revision: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    match (operator_target_revision, adoptable_revision) {
        (Some(_), None) | (None, Some(_)) => Ok(()),
        _ => Err(
            "exactly one of --operator-target-revision or --adoptable-revision must be provided"
                .into(),
        ),
    }
}

pub(crate) struct AdoptSummary {
    pub(crate) selection: ResolvedAdoptionSelection,
    pub(crate) previous_operator_target_revision: Option<String>,
    pub(crate) restart_required: Option<bool>,
}

fn print_summary(summary: &AdoptSummary) {
    println!(
        "operator_target_revision = {}",
        summary.selection.operator_target_revision
    );
    println!(
        "previous_operator_target_revision = {}",
        summary
            .previous_operator_target_revision
            .as_deref()
            .unwrap_or("unavailable")
    );
    println!(
        "adoptable_revision = {}",
        summary
            .selection
            .adoptable_revision
            .as_deref()
            .unwrap_or("unavailable")
    );
    println!(
        "candidate_revision = {}",
        summary
            .selection
            .candidate_revision
            .as_deref()
            .unwrap_or("unavailable")
    );
    println!(
        "restart_required = {}",
        match summary.restart_required {
            Some(true) => "true",
            Some(false) => "false",
            None => "unknown",
        }
    );
}

pub(crate) async fn adopt_selected_revision(
    pool: &PgPool,
    config_path: &Path,
    operator_target_revision: Option<&str>,
    adoptable_revision: Option<&str>,
) -> Result<AdoptSummary, Box<dyn Error>> {
    validate_selector_flags(operator_target_revision, adoptable_revision)?;

    let selection =
        resolve_adoption_selection(pool, operator_target_revision, adoptable_revision).await?;
    let active_operator_target_revision = load_active_operator_target_revision(pool).await?;
    let previous_operator_target_revision = configured_operator_target_revision(config_path)?;
    let changed = previous_operator_target_revision.as_deref()
        != Some(selection.operator_target_revision.as_str());
    let restart_required = active_operator_target_revision
        .as_deref()
        .map(|active| active != selection.operator_target_revision);

    ensure_canonical_provenance(pool, &selection).await?;

    let history_row = OperatorTargetAdoptionHistoryRow {
        adoption_id: format!(
            "adopt-{}-{}",
            Utc::now()
                .timestamp_nanos_opt()
                .unwrap_or_else(|| Utc::now().timestamp_micros() * 1_000),
            selection.operator_target_revision
        ),
        action_kind: "adopt".to_owned(),
        operator_target_revision: selection.operator_target_revision.clone(),
        previous_operator_target_revision: previous_operator_target_revision.clone(),
        adoptable_revision: selection.adoptable_revision.clone(),
        candidate_revision: selection.candidate_revision.clone(),
        adopted_at: Utc::now(),
    };
    OperatorTargetAdoptionHistoryRepo
        .append(pool, &history_row)
        .await?;

    if changed {
        rewrite_operator_target_revision(config_path, &selection.operator_target_revision)?;
    }

    Ok(AdoptSummary {
        selection,
        previous_operator_target_revision: if changed {
            previous_operator_target_revision
        } else {
            None
        },
        restart_required,
    })
}

async fn ensure_canonical_provenance(
    pool: &PgPool,
    selection: &ResolvedAdoptionSelection,
) -> Result<(), Box<dyn Error>> {
    let (Some(adoptable_revision), Some(candidate_revision)) = (
        selection.adoptable_revision.as_deref(),
        selection.candidate_revision.as_deref(),
    ) else {
        return Ok(());
    };

    let canonical = CandidateAdoptionProvenanceRow {
        operator_target_revision: selection.operator_target_revision.clone(),
        adoptable_revision: adoptable_revision.to_owned(),
        candidate_revision: candidate_revision.to_owned(),
    };

    if CandidateAdoptionRepo
        .get_by_operator_target_revision(pool, &canonical.operator_target_revision)
        .await?
        .is_none()
    {
        CandidateAdoptionRepo
            .upsert_provenance(pool, &canonical)
            .await?;
    }

    Ok(())
}
