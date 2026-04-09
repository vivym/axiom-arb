use std::{fmt, path::Path};

use config_schema::{load_raw_config_from_path, NegRiskTargetSourceKindToml, RawAxiomConfig};
use persistence::{
    models::{
        AdoptableStrategyRevisionRow, StrategyAdoptionProvenanceRow, StrategyCandidateSetRow,
    },
    CandidateAdoptionRepo, CandidateArtifactRepo, StrategyAdoptionRepo,
};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};

use crate::commands::targets::{
    config_file::rewrite_operator_strategy_revision,
    state::{legacy_explicit_targets, synthetic_strategy_revision_for_legacy_explicit_config},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationOutcome {
    pub operator_strategy_revision: String,
    pub adoptable_strategy_revision: String,
    pub strategy_candidate_revision: String,
    pub source: MigrationSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationSource {
    LegacyTargetSource,
    LegacyExplicitTargets,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrategyControlMigrationError {
    InvalidConfig(String),
    MissingLineage(String),
    Persistence(String),
    Rewrite(String),
}

pub type LegacyStrategyControlMigrationOutcome = MigrationOutcome;
pub type LegacyStrategyControlMigrationError = StrategyControlMigrationError;

impl fmt::Display for StrategyControlMigrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(message)
            | Self::MissingLineage(message)
            | Self::Persistence(message)
            | Self::Rewrite(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for StrategyControlMigrationError {}

pub async fn migrate_legacy_strategy_control(
    pool: &PgPool,
    config_path: &Path,
) -> Result<MigrationOutcome, StrategyControlMigrationError> {
    let raw = load_raw_config_from_path(config_path)
        .map_err(|error| StrategyControlMigrationError::InvalidConfig(error.to_string()))?;

    let has_canonical = raw.strategy_control.is_some();
    let has_target_source = raw
        .negrisk
        .as_ref()
        .and_then(|negrisk| negrisk.target_source.as_ref())
        .is_some();
    let has_explicit_targets = raw
        .negrisk
        .as_ref()
        .is_some_and(|negrisk| negrisk.targets.is_present());

    if has_canonical && (has_target_source || has_explicit_targets) {
        return Err(StrategyControlMigrationError::InvalidConfig(
            "canonical [strategy_control] cannot be combined with legacy negrisk control-plane input"
                .to_owned(),
        ));
    }

    if has_target_source && has_explicit_targets {
        return Err(StrategyControlMigrationError::InvalidConfig(
            "legacy negrisk.target_source cannot be combined with explicit negrisk.targets"
                .to_owned(),
        ));
    }

    if has_target_source {
        return migrate_legacy_target_source(pool, config_path, &raw).await;
    }

    if has_explicit_targets {
        return migrate_legacy_explicit_targets(pool, config_path, &raw).await;
    }

    Err(StrategyControlMigrationError::InvalidConfig(
        "config does not contain migratable legacy strategy-control input".to_owned(),
    ))
}

async fn migrate_legacy_target_source(
    pool: &PgPool,
    config_path: &Path,
    raw: &RawAxiomConfig,
) -> Result<MigrationOutcome, StrategyControlMigrationError> {
    let target_source = raw
        .negrisk
        .as_ref()
        .and_then(|negrisk| negrisk.target_source.as_ref())
        .ok_or_else(|| {
            StrategyControlMigrationError::InvalidConfig(
                "missing required section: negrisk.target_source".to_owned(),
            )
        })?;

    if !matches!(target_source.source, NegRiskTargetSourceKindToml::Adopted) {
        return Err(StrategyControlMigrationError::InvalidConfig(
            "legacy negrisk.target_source must use source = \"adopted\"".to_owned(),
        ));
    }

    let operator_target_revision = target_source
        .operator_target_revision
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            StrategyControlMigrationError::InvalidConfig(
                "missing negrisk.target_source.operator_target_revision".to_owned(),
            )
        })?;

    let existing_marker_lineage =
        load_existing_canonical_target_source_migration(pool, operator_target_revision).await?;

    let legacy_provenance = CandidateAdoptionRepo
        .get_by_operator_target_revision(pool, operator_target_revision)
        .await
        .map_err(|error| StrategyControlMigrationError::Persistence(error.to_string()))?;

    let derived_operator_strategy_revision =
        canonical_strategy_revision_from_legacy_target_revision(operator_target_revision)?;

    let existing_derived_lineage = if legacy_provenance.is_some() {
        StrategyAdoptionRepo
            .get_by_operator_strategy_revision(pool, &derived_operator_strategy_revision)
            .await
            .map_err(|error| StrategyControlMigrationError::Persistence(error.to_string()))?
    } else {
        None
    };
    let missing_all_lineage = legacy_provenance.is_none() && existing_marker_lineage.is_none();
    if missing_all_lineage {
        return Err(StrategyControlMigrationError::MissingLineage(format!(
            "legacy operator_target_revision {operator_target_revision} has no target-shaped provenance or canonical strategy lineage"
        )));
    }

    let (operator_strategy_revision, adoptable_strategy_revision, strategy_candidate_revision) =
        match (existing_marker_lineage, existing_derived_lineage) {
            (Some(marker), Some(derived))
                if marker.operator_strategy_revision != derived.operator_strategy_revision =>
            {
                return Err(StrategyControlMigrationError::MissingLineage(format!(
                    "legacy operator_target_revision {operator_target_revision} maps to conflicting canonical strategy revisions"
                )));
            }
            (Some(existing), _) => (
                existing.operator_strategy_revision,
                existing.adoptable_strategy_revision,
                existing.strategy_candidate_revision,
            ),
            (None, Some(existing)) => (
                existing.operator_strategy_revision,
                existing.adoptable_strategy_revision,
                existing.strategy_candidate_revision,
            ),
            (None, None) => {
                let operator_strategy_revision = derived_operator_strategy_revision;
                let strategy_candidate_revision =
                    synthetic_strategy_candidate_revision(&operator_strategy_revision);
                let adoptable_strategy_revision =
                    synthetic_adoptable_revision(&operator_strategy_revision);
                (
                    operator_strategy_revision,
                    adoptable_strategy_revision,
                    strategy_candidate_revision,
                )
            }
        };

    if let Some(provenance) = legacy_provenance {
        let candidate = CandidateArtifactRepo
            .get_candidate_target_set(pool, &provenance.candidate_revision)
            .await
            .map_err(|error| StrategyControlMigrationError::Persistence(error.to_string()))?
            .ok_or_else(|| {
                StrategyControlMigrationError::MissingLineage(format!(
                    "candidate_revision {} is unavailable",
                    provenance.candidate_revision
                ))
            })?;

        let adoptable = CandidateArtifactRepo
            .get_adoptable_target_revision(pool, &provenance.adoptable_revision)
            .await
            .map_err(|error| StrategyControlMigrationError::Persistence(error.to_string()))?
            .ok_or_else(|| {
                StrategyControlMigrationError::MissingLineage(format!(
                    "adoptable_revision {} is unavailable",
                    provenance.adoptable_revision
                ))
            })?;

        let strategy_candidate = StrategyCandidateSetRow {
            strategy_candidate_revision: strategy_candidate_revision.clone(),
            snapshot_id: candidate.snapshot_id,
            source_revision: candidate.source_revision,
            payload: payload_with_inserted_string(
                payload_with_inserted_string(
                    candidate.payload,
                    "legacy_operator_target_revision",
                    operator_target_revision,
                ),
                "strategy_candidate_revision",
                &strategy_candidate_revision,
            ),
        };
        materialize_strategy_candidate_set(pool, &strategy_candidate).await?;

        let strategy_adoptable = AdoptableStrategyRevisionRow {
            adoptable_strategy_revision: adoptable_strategy_revision.clone(),
            strategy_candidate_revision: strategy_candidate_revision.clone(),
            rendered_operator_strategy_revision: operator_strategy_revision.clone(),
            payload: payload_with_inserted_string(
                payload_with_inserted_string(
                    payload_with_rendered_strategy_revision(
                        adoptable.payload,
                        &operator_strategy_revision,
                        "legacy-target-source",
                    ),
                    "legacy_operator_target_revision",
                    operator_target_revision,
                ),
                "adoptable_strategy_revision",
                &adoptable_strategy_revision,
            ),
        };
        materialize_adoptable_strategy_revision(pool, &strategy_adoptable).await?;
    } else {
        ensure_existing_canonical_artifacts_present(
            pool,
            &operator_strategy_revision,
            &adoptable_strategy_revision,
            &strategy_candidate_revision,
        )
        .await?;
    }

    StrategyAdoptionRepo
        .upsert_provenance(
            pool,
            &StrategyAdoptionProvenanceRow {
                operator_strategy_revision: operator_strategy_revision.clone(),
                adoptable_strategy_revision: adoptable_strategy_revision.clone(),
                strategy_candidate_revision: strategy_candidate_revision.clone(),
            },
        )
        .await
        .map_err(|error| StrategyControlMigrationError::Persistence(error.to_string()))?;

    rewrite_operator_strategy_revision(config_path, &operator_strategy_revision)
        .map_err(|error| StrategyControlMigrationError::Rewrite(error.to_string()))?;

    Ok(MigrationOutcome {
        operator_strategy_revision,
        adoptable_strategy_revision,
        strategy_candidate_revision,
        source: MigrationSource::LegacyTargetSource,
    })
}

async fn migrate_legacy_explicit_targets(
    pool: &PgPool,
    config_path: &Path,
    raw: &RawAxiomConfig,
) -> Result<MigrationOutcome, StrategyControlMigrationError> {
    if raw
        .negrisk
        .as_ref()
        .is_some_and(|negrisk| negrisk.targets.is_present() && negrisk.targets.is_empty())
    {
        return Err(StrategyControlMigrationError::InvalidConfig(
            "empty negrisk.targets array is invalid legacy input".to_owned(),
        ));
    }

    let operator_strategy_revision =
        synthetic_strategy_revision_for_legacy_explicit_config(config_path)
            .map_err(|error| StrategyControlMigrationError::InvalidConfig(error.to_string()))?;
    let targets = legacy_explicit_targets(config_path)
        .map_err(|error| StrategyControlMigrationError::InvalidConfig(error.to_string()))?;
    let strategy_candidate_revision =
        synthetic_strategy_candidate_revision(&operator_strategy_revision);
    let adoptable_strategy_revision = synthetic_adoptable_revision(&operator_strategy_revision);

    let strategy_candidate = StrategyCandidateSetRow {
        strategy_candidate_revision: strategy_candidate_revision.clone(),
        snapshot_id: format!("compatibility-migration-{operator_strategy_revision}"),
        source_revision: "legacy-explicit".to_owned(),
        payload: json!({
            "migration_source": "legacy-explicit",
            "strategy_candidate_revision": strategy_candidate_revision,
        }),
    };
    materialize_strategy_candidate_set(pool, &strategy_candidate).await?;

    let strategy_adoptable = AdoptableStrategyRevisionRow {
        adoptable_strategy_revision: adoptable_strategy_revision.clone(),
        strategy_candidate_revision: strategy_candidate_revision.clone(),
        rendered_operator_strategy_revision: operator_strategy_revision.clone(),
        payload: json!({
            "migration_source": "legacy-explicit",
            "adoptable_strategy_revision": adoptable_strategy_revision,
            "strategy_candidate_revision": strategy_candidate_revision,
            "rendered_operator_strategy_revision": operator_strategy_revision,
            "rendered_live_targets": targets.targets(),
        }),
    };
    materialize_adoptable_strategy_revision(pool, &strategy_adoptable).await?;

    StrategyAdoptionRepo
        .upsert_provenance(
            pool,
            &StrategyAdoptionProvenanceRow {
                operator_strategy_revision: operator_strategy_revision.clone(),
                adoptable_strategy_revision: adoptable_strategy_revision.clone(),
                strategy_candidate_revision: strategy_candidate_revision.clone(),
            },
        )
        .await
        .map_err(|error| StrategyControlMigrationError::Persistence(error.to_string()))?;

    rewrite_operator_strategy_revision(config_path, &operator_strategy_revision)
        .map_err(|error| StrategyControlMigrationError::Rewrite(error.to_string()))?;

    Ok(MigrationOutcome {
        operator_strategy_revision,
        adoptable_strategy_revision,
        strategy_candidate_revision,
        source: MigrationSource::LegacyExplicitTargets,
    })
}

fn canonical_strategy_revision_from_legacy_target_revision(
    operator_target_revision: &str,
) -> Result<String, StrategyControlMigrationError> {
    if let Some(suffix) = operator_target_revision.strip_prefix("targets-rev-") {
        return Ok(format!("strategy-rev-{suffix}"));
    }

    if let Some(suffix) = operator_target_revision.strip_prefix("sha256:") {
        return Ok(format!("strategy-rev-{suffix}"));
    }

    Err(StrategyControlMigrationError::InvalidConfig(format!(
        "legacy operator_target_revision {operator_target_revision} cannot be mapped to a canonical operator_strategy_revision"
    )))
}

async fn ensure_existing_canonical_artifacts_present(
    pool: &PgPool,
    operator_strategy_revision: &str,
    adoptable_strategy_revision: &str,
    strategy_candidate_revision: &str,
) -> Result<(), StrategyControlMigrationError> {
    let candidate =
        load_materialized_strategy_candidate_set(pool, strategy_candidate_revision).await?;
    if candidate.is_none() {
        return Err(StrategyControlMigrationError::MissingLineage(format!(
            "canonical strategy_candidate_revision {strategy_candidate_revision} is unavailable and no legacy artifacts exist to repair it"
        )));
    }

    let adoptable =
        load_materialized_adoptable_strategy_revision(pool, adoptable_strategy_revision).await?;
    let Some(adoptable) = adoptable else {
        return Err(StrategyControlMigrationError::MissingLineage(format!(
            "canonical adoptable_strategy_revision {adoptable_strategy_revision} is unavailable and no legacy artifacts exist to repair it"
        )));
    };

    if adoptable.strategy_candidate_revision != strategy_candidate_revision
        || adoptable.rendered_operator_strategy_revision != operator_strategy_revision
    {
        return Err(StrategyControlMigrationError::MissingLineage(format!(
            "canonical adoptable_strategy_revision {adoptable_strategy_revision} does not match expected strategy lineage"
        )));
    }

    Ok(())
}

async fn load_existing_canonical_target_source_migration(
    pool: &PgPool,
    operator_target_revision: &str,
) -> Result<Option<StrategyAdoptionProvenanceRow>, StrategyControlMigrationError> {
    let rows = sqlx::query(
        r#"
        SELECT
            provenance.operator_strategy_revision,
            provenance.adoptable_strategy_revision,
            provenance.strategy_candidate_revision
        FROM strategy_adoption_provenance AS provenance
        JOIN adoptable_strategy_revisions AS adoptable
          ON adoptable.adoptable_strategy_revision = provenance.adoptable_strategy_revision
         AND adoptable.strategy_candidate_revision = provenance.strategy_candidate_revision
         AND adoptable.rendered_operator_strategy_revision = provenance.operator_strategy_revision
        WHERE adoptable.payload ->> 'legacy_operator_target_revision' = $1
        ORDER BY provenance.operator_strategy_revision ASC
        "#,
    )
    .bind(operator_target_revision)
    .fetch_all(pool)
    .await
    .map_err(|error| StrategyControlMigrationError::Persistence(error.to_string()))?;

    let mut mapped = rows
        .into_iter()
        .map(|row| {
            Ok(StrategyAdoptionProvenanceRow {
                operator_strategy_revision: row.try_get("operator_strategy_revision")?,
                adoptable_strategy_revision: row.try_get("adoptable_strategy_revision")?,
                strategy_candidate_revision: row.try_get("strategy_candidate_revision")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(|error| StrategyControlMigrationError::Persistence(error.to_string()))?;

    if mapped.len() > 1 {
        return Err(StrategyControlMigrationError::MissingLineage(format!(
            "legacy operator_target_revision {operator_target_revision} maps to multiple canonical strategy revisions"
        )));
    }

    Ok(mapped.pop())
}

async fn materialize_strategy_candidate_set(
    pool: &PgPool,
    row: &StrategyCandidateSetRow,
) -> Result<(), StrategyControlMigrationError> {
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
    .await
    .map_err(|error| StrategyControlMigrationError::Persistence(error.to_string()))?;

    let stored = load_materialized_strategy_candidate_set(pool, &row.strategy_candidate_revision)
        .await?
        .ok_or_else(|| {
            StrategyControlMigrationError::Persistence(format!(
                "strategy_candidate_revision {} was not materialized",
                row.strategy_candidate_revision
            ))
        })?;
    if stored != *row {
        return Err(StrategyControlMigrationError::Persistence(format!(
            "strategy_candidate_revision {} conflicts with existing canonical lineage",
            row.strategy_candidate_revision
        )));
    }

    Ok(())
}

async fn materialize_adoptable_strategy_revision(
    pool: &PgPool,
    row: &AdoptableStrategyRevisionRow,
) -> Result<(), StrategyControlMigrationError> {
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
    .await
    .map_err(|error| StrategyControlMigrationError::Persistence(error.to_string()))?;

    let stored =
        load_materialized_adoptable_strategy_revision(pool, &row.adoptable_strategy_revision)
            .await?
            .ok_or_else(|| {
                StrategyControlMigrationError::Persistence(format!(
                    "adoptable_strategy_revision {} was not materialized",
                    row.adoptable_strategy_revision
                ))
            })?;
    if stored != *row {
        return Err(StrategyControlMigrationError::Persistence(format!(
            "adoptable_strategy_revision {} conflicts with existing canonical lineage",
            row.adoptable_strategy_revision
        )));
    }

    Ok(())
}

async fn load_materialized_strategy_candidate_set(
    pool: &PgPool,
    strategy_candidate_revision: &str,
) -> Result<Option<StrategyCandidateSetRow>, StrategyControlMigrationError> {
    let row = sqlx::query(
        r#"
        SELECT strategy_candidate_revision, snapshot_id, source_revision, payload
        FROM strategy_candidate_sets
        WHERE strategy_candidate_revision = $1
        "#,
    )
    .bind(strategy_candidate_revision)
    .fetch_optional(pool)
    .await
    .map_err(|error| StrategyControlMigrationError::Persistence(error.to_string()))?;

    row.map(|row| {
        Ok(StrategyCandidateSetRow {
            strategy_candidate_revision: row.try_get("strategy_candidate_revision")?,
            snapshot_id: row.try_get("snapshot_id")?,
            source_revision: row.try_get("source_revision")?,
            payload: row.try_get("payload")?,
        })
    })
    .transpose()
    .map_err(|error: sqlx::Error| StrategyControlMigrationError::Persistence(error.to_string()))
}

async fn load_materialized_adoptable_strategy_revision(
    pool: &PgPool,
    adoptable_strategy_revision: &str,
) -> Result<Option<AdoptableStrategyRevisionRow>, StrategyControlMigrationError> {
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
    .await
    .map_err(|error| StrategyControlMigrationError::Persistence(error.to_string()))?;

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
    .map_err(|error: sqlx::Error| StrategyControlMigrationError::Persistence(error.to_string()))
}

fn payload_with_rendered_strategy_revision(
    mut payload: Value,
    operator_strategy_revision: &str,
    migration_source: &str,
) -> Value {
    if let Some(object) = payload.as_object_mut() {
        object.insert(
            "rendered_operator_strategy_revision".to_owned(),
            Value::String(operator_strategy_revision.to_owned()),
        );
        object.insert(
            "migration_source".to_owned(),
            Value::String(migration_source.to_owned()),
        );
    }
    payload
}

fn payload_with_inserted_string(mut payload: Value, key: &str, value: &str) -> Value {
    if let Some(object) = payload.as_object_mut() {
        object.insert(key.to_owned(), Value::String(value.to_owned()));
    }
    payload
}

fn synthetic_adoptable_revision(operator_strategy_revision: &str) -> String {
    format!(
        "adoptable-strategy-{}",
        operator_strategy_revision.trim_start_matches("strategy-rev-")
    )
}

fn synthetic_strategy_candidate_revision(operator_strategy_revision: &str) -> String {
    format!(
        "strategy-candidate-{}",
        operator_strategy_revision.trim_start_matches("strategy-rev-")
    )
}
