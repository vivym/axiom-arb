use std::error::Error;

use chrono::Utc;
use persistence::{
    connect_pool_from_env, models::OperatorTargetAdoptionHistoryRow,
    OperatorTargetAdoptionHistoryRepo,
};

use crate::{
    cli::TargetRollbackArgs,
    commands::targets::{
        config_file::rewrite_operator_target_revision,
        state::{
            configured_operator_target_revision, load_active_operator_target_revision,
            resolve_rollback_selection, ResolvedAdoptionSelection,
        },
    },
};

pub fn execute(args: TargetRollbackArgs) -> Result<(), Box<dyn Error>> {
    if let Err(error) = execute_inner(args) {
        eprintln!("{error}");
        return Err(error);
    }

    Ok(())
}

fn execute_inner(args: TargetRollbackArgs) -> Result<(), Box<dyn Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let summary = runtime.block_on(async {
        let pool = connect_pool_from_env().await?;
        let previous_operator_target_revision = configured_operator_target_revision(&args.config)?;
        let active_operator_target_revision = load_active_operator_target_revision(&pool).await?;
        let selection = resolve_rollback_selection(
            &pool,
            previous_operator_target_revision.as_deref(),
            args.to_operator_target_revision.as_deref(),
        )
        .await?;
        let changed = previous_operator_target_revision.as_deref()
            != Some(selection.operator_target_revision.as_str());
        let restart_required = active_operator_target_revision
            .as_deref()
            .map(|active| active != selection.operator_target_revision);

        if changed {
            let history_row =
                build_rollback_history_row(previous_operator_target_revision.clone(), &selection);
            OperatorTargetAdoptionHistoryRepo
                .append(&pool, &history_row)
                .await?;
            rewrite_operator_target_revision(&args.config, &selection.operator_target_revision)?;
        }

        Ok::<_, Box<dyn Error>>(RollbackSummary {
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

fn build_rollback_history_row(
    previous_operator_target_revision: Option<String>,
    selection: &ResolvedAdoptionSelection,
) -> OperatorTargetAdoptionHistoryRow {
    OperatorTargetAdoptionHistoryRow {
        adoption_id: format!(
            "rollback-{}-{}",
            Utc::now()
                .timestamp_nanos_opt()
                .unwrap_or_else(|| Utc::now().timestamp_micros() * 1_000),
            selection.operator_target_revision
        ),
        action_kind: "rollback".to_owned(),
        operator_target_revision: selection.operator_target_revision.clone(),
        previous_operator_target_revision,
        // Rollback rows deliberately carry no lineage; adopt rows remain the durable source.
        adoptable_revision: None,
        candidate_revision: None,
        adopted_at: Utc::now(),
    }
}

struct RollbackSummary {
    selection: ResolvedAdoptionSelection,
    previous_operator_target_revision: Option<String>,
    restart_required: Option<bool>,
}

fn print_summary(summary: &RollbackSummary) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollback_history_rows_do_not_persist_candidate_lineage() {
        let row = build_rollback_history_row(
            Some("targets-rev-9".to_owned()),
            &ResolvedAdoptionSelection {
                operator_target_revision: "targets-rev-8".to_owned(),
                adoptable_revision: Some("adoptable-8".to_owned()),
                candidate_revision: Some("candidate-8".to_owned()),
            },
        );

        assert_eq!(row.action_kind, "rollback");
        assert_eq!(
            row.previous_operator_target_revision.as_deref(),
            Some("targets-rev-9")
        );
        assert_eq!(row.operator_target_revision, "targets-rev-8");
        assert_eq!(row.adoptable_revision, None);
        assert_eq!(row.candidate_revision, None);
    }
}
