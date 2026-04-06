use std::error::Error;

use chrono::Utc;
use persistence::{
    connect_pool_from_env, models::OperatorStrategyAdoptionHistoryRow,
    OperatorStrategyAdoptionHistoryRepo,
};

use crate::{
    cli::TargetRollbackArgs,
    commands::targets::{
        config_file::rewrite_operator_strategy_revision,
        state::{
            compatibility_mode, configured_operator_strategy_revision,
            load_active_operator_strategy_revision, resolve_rollback_selection,
            synthetic_strategy_revision_for_legacy_explicit_config,
            ResolvedStrategyAdoptionSelection,
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
        let compatibility_mode = compatibility_mode(&args.config)?;
        let previous_operator_strategy_revision = configured_operator_strategy_revision(&args.config)?
            .or_else(|| {
                compatibility_mode
                    .as_deref()
                    .map(|_| synthetic_strategy_revision_for_legacy_explicit_config(&args.config))
                    .transpose()
                    .ok()
                    .flatten()
            });
        let selection = resolve_rollback_selection(
            &pool,
            compatibility_mode.as_deref(),
            previous_operator_strategy_revision.as_deref(),
            args.to_operator_strategy_revision.as_deref(),
        )
        .await?;
        let active_operator_strategy_revision = load_active_operator_strategy_revision(
            &pool,
            Some(selection.operator_strategy_revision.as_str()),
        )
        .await?;
        let changed = previous_operator_strategy_revision.as_deref()
            != Some(selection.operator_strategy_revision.as_str());
        let restart_required = active_operator_strategy_revision
            .as_deref()
            .map(|active| active != selection.operator_strategy_revision);

        if changed {
            let history_row =
                build_rollback_history_row(previous_operator_strategy_revision.clone(), &selection);
            OperatorStrategyAdoptionHistoryRepo
                .append(&pool, &history_row)
                .await?;
            rewrite_operator_strategy_revision(&args.config, &selection.operator_strategy_revision)?;
        }

        Ok::<_, Box<dyn Error>>(RollbackSummary {
            selection,
            previous_operator_strategy_revision: if changed {
                previous_operator_strategy_revision
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
    previous_operator_strategy_revision: Option<String>,
    selection: &ResolvedStrategyAdoptionSelection,
) -> OperatorStrategyAdoptionHistoryRow {
    OperatorStrategyAdoptionHistoryRow {
        adoption_id: format!(
            "rollback-{}-{}",
            Utc::now()
                .timestamp_nanos_opt()
                .unwrap_or_else(|| Utc::now().timestamp_micros() * 1_000),
            selection.operator_strategy_revision
        ),
        action_kind: "rollback".to_owned(),
        operator_strategy_revision: selection.operator_strategy_revision.clone(),
        previous_operator_strategy_revision,
        adoptable_strategy_revision: None,
        strategy_candidate_revision: None,
        adopted_at: Utc::now(),
    }
}

struct RollbackSummary {
    selection: ResolvedStrategyAdoptionSelection,
    previous_operator_strategy_revision: Option<String>,
    restart_required: Option<bool>,
}

fn print_summary(summary: &RollbackSummary) {
    println!(
        "operator_strategy_revision = {}",
        summary.selection.operator_strategy_revision
    );
    println!(
        "previous_operator_strategy_revision = {}",
        summary
            .previous_operator_strategy_revision
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
        "migration_source = {}",
        summary
            .selection
            .migration_source
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
            Some("strategy-rev-9".to_owned()),
            &ResolvedStrategyAdoptionSelection {
                operator_strategy_revision: "strategy-rev-8".to_owned(),
                adoptable_revision: Some("adoptable-8".to_owned()),
                migration_source: None,
            },
        );

        assert_eq!(row.action_kind, "rollback");
        assert_eq!(
            row.previous_operator_strategy_revision.as_deref(),
            Some("strategy-rev-9")
        );
        assert_eq!(row.operator_strategy_revision, "strategy-rev-8");
        assert_eq!(row.adoptable_strategy_revision, None);
        assert_eq!(row.strategy_candidate_revision, None);
    }
}
