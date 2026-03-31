use std::error::Error;

use chrono::Utc;
use persistence::{
    connect_pool_from_env, models::OperatorTargetAdoptionHistoryRow,
    OperatorTargetAdoptionHistoryRepo,
};

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
        let selection = resolve_adoption_selection(
            &pool,
            args.operator_target_revision.as_deref(),
            args.adoptable_revision.as_deref(),
        )
        .await?;
        let active_operator_target_revision = load_active_operator_target_revision(&pool).await?;
        let previous_operator_target_revision = configured_operator_target_revision(&args.config)?;
        let changed = previous_operator_target_revision.as_deref()
            != Some(selection.operator_target_revision.as_str());
        let restart_required = active_operator_target_revision
            .as_deref()
            .map(|active| active != selection.operator_target_revision);

        if changed {
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
                .append(&pool, &history_row)
                .await?;
            rewrite_operator_target_revision(&args.config, &selection.operator_target_revision)?;
        }

        Ok::<_, Box<dyn Error>>(AdoptSummary {
            selection,
            previous_operator_target_revision: if changed {
                previous_operator_target_revision
            } else {
                None
            },
            restart_required,
        })
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

struct AdoptSummary {
    selection: ResolvedAdoptionSelection,
    previous_operator_target_revision: Option<String>,
    restart_required: Option<bool>,
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
