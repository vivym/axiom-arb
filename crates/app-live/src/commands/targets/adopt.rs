use std::{error::Error, path::Path};

use chrono::Utc;
use config_schema::load_raw_config_from_path;
use persistence::{
    connect_pool_from_env,
    models::{
        AdoptableStrategyRevisionRow, OperatorStrategyAdoptionHistoryRow,
        StrategyAdoptionProvenanceRow, StrategyCandidateSetRow,
    },
    OperatorStrategyAdoptionHistoryRepo, StrategyAdoptionRepo, StrategyControlArtifactRepo,
};
use sqlx::{PgPool, Row};

use crate::{
    cli::TargetAdoptArgs,
    commands::targets::{
        config_file::rewrite_operator_strategy_revision,
        state::{
            configured_operator_strategy_revision, load_active_operator_strategy_revision,
            resolve_adoption_selection, ResolvedStrategyAdoptionSelection,
        },
    },
    strategy_control::{migrate_legacy_strategy_control, MigrationSource},
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
        &args.config,
        args.operator_strategy_revision.as_deref(),
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
            args.operator_strategy_revision.as_deref(),
            args.adoptable_revision.as_deref(),
            false,
        )
        .await
    })?;

    print_summary(&summary);
    Ok(())
}

fn validate_selector_flags(
    config_path: &Path,
    operator_strategy_revision: Option<&str>,
    adoptable_revision: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    match (operator_strategy_revision, adoptable_revision) {
        (Some(_), None) | (None, Some(_)) => Ok(()),
        (None, None) if has_migratable_legacy_strategy_control(config_path)? => Ok(()),
        _ => Err(
            "exactly one of --operator-strategy-revision or --adoptable-revision must be provided"
                .into(),
        ),
    }
}

pub(crate) struct AdoptSummary {
    pub(crate) selection: ResolvedStrategyAdoptionSelection,
    pub(crate) previous_operator_strategy_revision: Option<String>,
    pub(crate) restart_required: Option<bool>,
}

fn print_summary(summary: &AdoptSummary) {
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

pub(crate) async fn adopt_selected_revision(
    pool: &PgPool,
    config_path: &Path,
    operator_strategy_revision: Option<&str>,
    adoptable_revision: Option<&str>,
    _adopt_compatibility: bool,
) -> Result<AdoptSummary, Box<dyn Error>> {
    validate_selector_flags(config_path, operator_strategy_revision, adoptable_revision)?;

    let previous_operator_strategy_revision = configured_operator_strategy_revision(config_path)?;

    let selection = if operator_strategy_revision.is_none() && adoptable_revision.is_none() {
        let outcome = migrate_legacy_strategy_control(pool, config_path)
            .await
            .map_err(|error| -> Box<dyn Error> { Box::new(error) })?;
        ResolvedStrategyAdoptionSelection {
            operator_strategy_revision: outcome.operator_strategy_revision,
            adoptable_revision: Some(outcome.adoptable_strategy_revision),
            migration_source: Some(match outcome.source {
                MigrationSource::LegacyTargetSource => "legacy-target-source".to_owned(),
                MigrationSource::LegacyExplicitTargets => "legacy-explicit".to_owned(),
            }),
        }
    } else {
        resolve_adoption_selection(pool, operator_strategy_revision, adoptable_revision).await?
    };
    let active_operator_strategy_revision = load_active_operator_strategy_revision(
        pool,
        Some(selection.operator_strategy_revision.as_str()),
    )
    .await?;
    let rewrite_required = config_requires_strategy_control_rewrite(config_path)?
        || previous_operator_strategy_revision.as_deref()
            != Some(selection.operator_strategy_revision.as_str());
    let restart_required = active_operator_strategy_revision
        .as_deref()
        .map(|active| active != selection.operator_strategy_revision);

    ensure_canonical_provenance(pool, &selection).await?;
    let strategy_candidate_revision =
        strategy_candidate_revision_for_selection(pool, &selection).await?;

    let history_row = OperatorStrategyAdoptionHistoryRow {
        adoption_id: format!(
            "adopt-{}-{}",
            Utc::now()
                .timestamp_nanos_opt()
                .unwrap_or_else(|| Utc::now().timestamp_micros() * 1_000),
            selection.operator_strategy_revision
        ),
        action_kind: "adopt".to_owned(),
        operator_strategy_revision: selection.operator_strategy_revision.clone(),
        previous_operator_strategy_revision: previous_operator_strategy_revision.clone(),
        adoptable_strategy_revision: selection.adoptable_revision.clone(),
        strategy_candidate_revision,
        adopted_at: Utc::now(),
    };
    OperatorStrategyAdoptionHistoryRepo
        .append(pool, &history_row)
        .await?;

    if rewrite_required {
        rewrite_operator_strategy_revision(config_path, &selection.operator_strategy_revision)?;
    }

    Ok(AdoptSummary {
        selection,
        previous_operator_strategy_revision: if rewrite_required {
            previous_operator_strategy_revision
        } else {
            None
        },
        restart_required,
    })
}

async fn ensure_canonical_provenance(
    pool: &PgPool,
    selection: &ResolvedStrategyAdoptionSelection,
) -> Result<(), Box<dyn Error>> {
    let Some(adoptable_revision) = selection.adoptable_revision.as_deref() else {
        return Ok(());
    };

    ensure_strategy_artifact_lineage(pool, adoptable_revision).await?;

    let adoptable = StrategyControlArtifactRepo
        .get_adoptable_strategy_revision(pool, adoptable_revision)
        .await?
        .ok_or_else(|| format!("adoptable_revision {adoptable_revision} is unavailable"))?;
    let canonical = StrategyAdoptionProvenanceRow {
        operator_strategy_revision: selection.operator_strategy_revision.clone(),
        adoptable_strategy_revision: adoptable.adoptable_strategy_revision,
        strategy_candidate_revision: adoptable.strategy_candidate_revision,
    };

    if StrategyAdoptionRepo
        .get_by_operator_strategy_revision(pool, &canonical.operator_strategy_revision)
        .await?
        .is_none()
    {
        StrategyAdoptionRepo
            .upsert_provenance(pool, &canonical)
            .await?;
    }

    Ok(())
}

async fn ensure_strategy_artifact_lineage(
    pool: &PgPool,
    adoptable_revision: &str,
) -> Result<(), Box<dyn Error>> {
    let adoptable = StrategyControlArtifactRepo
        .get_adoptable_strategy_revision(pool, adoptable_revision)
        .await?
        .ok_or_else(|| format!("adoptable_revision {adoptable_revision} is unavailable"))?;
    let candidate = StrategyControlArtifactRepo
        .get_strategy_candidate_set(pool, &adoptable.strategy_candidate_revision)
        .await?
        .ok_or_else(|| {
            format!(
                "strategy_candidate_revision {} is unavailable",
                adoptable.strategy_candidate_revision
            )
        })?;

    materialize_strategy_candidate_set(pool, &candidate).await?;
    materialize_adoptable_strategy_revision(pool, &adoptable).await?;
    Ok(())
}

async fn materialize_strategy_candidate_set(
    pool: &PgPool,
    row: &StrategyCandidateSetRow,
) -> Result<(), Box<dyn Error>> {
    sqlx::query(
        r#"
        INSERT INTO strategy_candidate_sets (
            strategy_candidate_revision,
            snapshot_id,
            source_revision,
            payload
        )
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (strategy_candidate_revision) DO NOTHING
        "#,
    )
    .bind(&row.strategy_candidate_revision)
    .bind(&row.snapshot_id)
    .bind(&row.source_revision)
    .bind(&row.payload)
    .execute(pool)
    .await?;

    let stored = load_materialized_strategy_candidate_set(pool, &row.strategy_candidate_revision)
        .await?
        .ok_or_else(|| {
            format!(
                "strategy_candidate_revision {} was not materialized",
                row.strategy_candidate_revision
            )
        })?;
    if stored != *row {
        return Err(format!(
            "strategy_candidate_revision {} conflicts with existing neutral lineage",
            row.strategy_candidate_revision
        )
        .into());
    }

    Ok(())
}

async fn materialize_adoptable_strategy_revision(
    pool: &PgPool,
    row: &AdoptableStrategyRevisionRow,
) -> Result<(), Box<dyn Error>> {
    sqlx::query(
        r#"
        INSERT INTO adoptable_strategy_revisions (
            adoptable_strategy_revision,
            strategy_candidate_revision,
            rendered_operator_strategy_revision,
            payload
        )
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (adoptable_strategy_revision) DO NOTHING
        "#,
    )
    .bind(&row.adoptable_strategy_revision)
    .bind(&row.strategy_candidate_revision)
    .bind(&row.rendered_operator_strategy_revision)
    .bind(&row.payload)
    .execute(pool)
    .await?;

    let stored =
        load_materialized_adoptable_strategy_revision(pool, &row.adoptable_strategy_revision)
            .await?
            .ok_or_else(|| {
                format!(
                    "adoptable_strategy_revision {} was not materialized",
                    row.adoptable_strategy_revision
                )
            })?;
    if stored != *row {
        return Err(format!(
            "adoptable_strategy_revision {} conflicts with existing neutral lineage",
            row.adoptable_strategy_revision
        )
        .into());
    }

    Ok(())
}

async fn load_materialized_strategy_candidate_set(
    pool: &PgPool,
    strategy_candidate_revision: &str,
) -> Result<Option<StrategyCandidateSetRow>, Box<dyn Error>> {
    let row = sqlx::query(
        r#"
        SELECT strategy_candidate_revision, snapshot_id, source_revision, payload
        FROM strategy_candidate_sets
        WHERE strategy_candidate_revision = $1
        "#,
    )
    .bind(strategy_candidate_revision)
    .fetch_optional(pool)
    .await?;

    row.map(|row| {
        Ok(StrategyCandidateSetRow {
            strategy_candidate_revision: row.try_get("strategy_candidate_revision")?,
            snapshot_id: row.try_get("snapshot_id")?,
            source_revision: row.try_get("source_revision")?,
            payload: row.try_get("payload")?,
        })
    })
    .transpose()
}

async fn load_materialized_adoptable_strategy_revision(
    pool: &PgPool,
    adoptable_strategy_revision: &str,
) -> Result<Option<AdoptableStrategyRevisionRow>, Box<dyn Error>> {
    let row = sqlx::query(
        r#"
        SELECT
            adoptable_strategy_revision,
            strategy_candidate_revision,
            rendered_operator_strategy_revision,
            payload
        FROM adoptable_strategy_revisions
        WHERE adoptable_strategy_revision = $1
        "#,
    )
    .bind(adoptable_strategy_revision)
    .fetch_optional(pool)
    .await?;

    row.map(|row| {
        Ok(AdoptableStrategyRevisionRow {
            adoptable_strategy_revision: row.try_get("adoptable_strategy_revision")?,
            strategy_candidate_revision: row.try_get("strategy_candidate_revision")?,
            rendered_operator_strategy_revision: row
                .try_get("rendered_operator_strategy_revision")?,
            payload: row.try_get("payload")?,
        })
    })
    .transpose()
}

async fn strategy_candidate_revision_for_selection(
    pool: &PgPool,
    selection: &ResolvedStrategyAdoptionSelection,
) -> Result<Option<String>, Box<dyn Error>> {
    let Some(adoptable_revision) = selection.adoptable_revision.as_deref() else {
        return Ok(None);
    };

    Ok(StrategyControlArtifactRepo
        .get_adoptable_strategy_revision(pool, adoptable_revision)
        .await?
        .map(|row| row.strategy_candidate_revision))
}

fn config_requires_strategy_control_rewrite(config_path: &Path) -> Result<bool, Box<dyn Error>> {
    let raw = load_raw_config_from_path(config_path)?;
    Ok(raw.strategy_control.is_none()
        || raw
            .negrisk
            .as_ref()
            .and_then(|negrisk| negrisk.target_source.as_ref())
            .is_some()
        || raw
            .negrisk
            .as_ref()
            .map(|negrisk| negrisk.targets.is_present())
            .unwrap_or(false))
}

fn has_migratable_legacy_strategy_control(config_path: &Path) -> Result<bool, Box<dyn Error>> {
    let raw = load_raw_config_from_path(config_path)?;
    Ok(raw.strategy_control.is_none()
        && raw
            .negrisk
            .as_ref()
            .is_some_and(|negrisk| negrisk.target_source.is_some() || negrisk.targets.is_present()))
}
