use std::{collections::BTreeMap, error::Error, path::Path};

use config_schema::{load_raw_config_from_path, ValidatedConfig};
use persistence::{
    models::{
        AdoptableTargetRevisionRow, CandidateAdoptionProvenanceRow, CandidateTargetSetRow,
        OperatorTargetAdoptionHistoryRow,
    },
    CandidateAdoptionRepo, CandidateArtifactRepo, OperatorTargetAdoptionHistoryRepo,
    RuntimeProgressRepo,
};
use sqlx::{PgPool, Row};

use crate::config::NegRiskFamilyLiveTarget;

#[derive(Debug, Clone)]
pub struct TargetControlPlaneState {
    pub configured_operator_target_revision: Option<String>,
    pub active_operator_target_revision: Option<String>,
    pub restart_needed: Option<bool>,
    pub provenance: Option<CandidateAdoptionProvenanceRow>,
    pub latest_action: Option<OperatorTargetAdoptionHistoryRow>,
}

#[derive(Debug, Clone, Default)]
pub struct TargetCandidatesCatalog {
    pub advisory_candidates: Vec<CandidateTargetSetRow>,
    pub adoptable_revisions: Vec<AdoptableTargetRevisionRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetCandidatesSummary {
    pub advisory_candidate_count: usize,
    pub adoptable_revision_count: usize,
    pub deferred_target_count: usize,
    pub excluded_target_count: usize,
    pub recommended_adoptable_revision: Option<String>,
    pub non_adoptable_reasons: Vec<String>,
}

impl TargetCandidatesSummary {
    pub fn non_adoptable_summary(&self) -> String {
        format!(
            "deferred:{} excluded:{}",
            self.deferred_target_count, self.excluded_target_count
        )
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedAdoptionSelection {
    pub operator_target_revision: String,
    pub adoptable_revision: Option<String>,
    pub candidate_revision: Option<String>,
}

#[derive(Debug)]
struct TargetStateError {
    message: String,
}

impl TargetStateError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for TargetStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for TargetStateError {}

pub async fn resolve_adoption_selection(
    pool: &PgPool,
    operator_target_revision: Option<&str>,
    adoptable_revision: Option<&str>,
) -> Result<ResolvedAdoptionSelection, Box<dyn Error>> {
    match (operator_target_revision, adoptable_revision) {
        (Some(_), Some(_)) | (None, None) => Err(TargetStateError::new(
            "exactly one of --operator-target-revision or --adoptable-revision must be provided",
        )
        .into()),
        (Some(operator_target_revision), None) => {
            if let Some(provenance) = CandidateAdoptionRepo
                .get_by_operator_target_revision(pool, operator_target_revision)
                .await?
            {
                return selection_from_lineage(
                    pool,
                    &provenance.operator_target_revision,
                    &provenance.adoptable_revision,
                    &provenance.candidate_revision,
                )
                .await;
            }

            if let Some((adoptable_revision, candidate_revision)) =
                latest_history_lineage(pool, operator_target_revision).await?
            {
                return selection_from_lineage(
                    pool,
                    operator_target_revision,
                    &adoptable_revision,
                    &candidate_revision,
                )
                .await;
            }

            Err(TargetStateError::new(format!(
                "operator_target_revision {operator_target_revision} has no durable adoption provenance or history lineage"
            ))
            .into())
        }
        (None, Some(adoptable_revision)) => {
            let adoptable = CandidateArtifactRepo
                .get_adoptable_target_revision(pool, adoptable_revision)
                .await?
                .ok_or_else(|| {
                    TargetStateError::new(format!(
                        "adoptable_revision {adoptable_revision} is unavailable"
                    ))
                })?;
            validate_rendered_live_targets(
                &adoptable.payload,
                &adoptable.rendered_operator_target_revision,
            )?;
            CandidateArtifactRepo
                .get_candidate_target_set(pool, &adoptable.candidate_revision)
                .await?
                .ok_or_else(|| {
                    TargetStateError::new(format!(
                        "candidate_revision {} is unavailable",
                        adoptable.candidate_revision
                    ))
                })?;

            Ok(ResolvedAdoptionSelection {
                operator_target_revision: adoptable.rendered_operator_target_revision,
                adoptable_revision: Some(adoptable.adoptable_revision),
                candidate_revision: Some(adoptable.candidate_revision),
            })
        }
    }
}

pub async fn resolve_rollback_selection(
    pool: &PgPool,
    configured_operator_target_revision: Option<&str>,
    to_operator_target_revision: Option<&str>,
) -> Result<ResolvedAdoptionSelection, Box<dyn Error>> {
    let configured_operator_target_revision =
        configured_operator_target_revision.ok_or_else(|| {
            TargetStateError::new("configured operator_target_revision is unavailable")
        })?;
    let destination = match to_operator_target_revision {
        Some(operator_target_revision) => operator_target_revision.to_owned(),
        None => OperatorTargetAdoptionHistoryRepo
            .previous_distinct_revision(pool, configured_operator_target_revision)
            .await?
            .ok_or_else(|| {
                TargetStateError::new(format!(
                    "operator_target_revision {configured_operator_target_revision} has no previous distinct revision"
                ))
            })?,
    };

    resolve_adoption_selection(pool, Some(destination.as_str()), None).await
}

pub async fn load_active_operator_target_revision(
    pool: &PgPool,
) -> Result<Option<String>, Box<dyn Error>> {
    match RuntimeProgressRepo.current(pool).await? {
        Some(row) => Ok(Some(row.operator_target_revision.ok_or_else(|| {
            TargetStateError::new(
                "runtime progress row exists without operator_target_revision anchor",
            )
        })?)),
        None => Ok(None),
    }
}

pub async fn load_target_control_plane_state(
    pool: &PgPool,
    config_path: &Path,
) -> Result<TargetControlPlaneState, Box<dyn Error>> {
    let raw = load_raw_config_from_path(config_path)?;
    let _ = ValidatedConfig::new(raw)?;
    let configured_operator_target_revision = configured_operator_target_revision(config_path)?;
    let active_operator_target_revision = load_active_operator_target_revision(pool).await?;
    let restart_needed = match (
        configured_operator_target_revision.as_deref(),
        active_operator_target_revision.as_deref(),
    ) {
        (Some(configured), Some(active)) => Some(configured != active),
        _ => None,
    };
    let provenance = match configured_operator_target_revision.as_deref() {
        Some(operator_target_revision) => Some(
            CandidateAdoptionRepo
                .get_by_operator_target_revision(pool, operator_target_revision)
                .await?
                .ok_or_else(|| {
                    TargetStateError::new(format!(
                        "configured operator_target_revision {operator_target_revision} has no durable adoption provenance"
                    ))
                })?,
        ),
        None => None,
    };
    let latest_action = OperatorTargetAdoptionHistoryRepo.latest(pool).await?;

    Ok(TargetControlPlaneState {
        configured_operator_target_revision,
        active_operator_target_revision,
        restart_needed,
        provenance,
        latest_action,
    })
}

pub async fn load_target_candidates_catalog(
    pool: &PgPool,
) -> Result<TargetCandidatesCatalog, Box<dyn Error>> {
    let advisory_candidates = sqlx::query(
        r#"
        SELECT candidate_revision, snapshot_id, source_revision, payload
        FROM candidate_target_sets AS candidate
        WHERE NOT EXISTS (
            SELECT 1
            FROM adoptable_target_revisions AS adoptable
            WHERE adoptable.candidate_revision = candidate.candidate_revision
        )
        ORDER BY candidate_revision DESC
        "#,
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        Ok(CandidateTargetSetRow {
            candidate_revision: row.try_get("candidate_revision")?,
            snapshot_id: row.try_get("snapshot_id")?,
            source_revision: row.try_get("source_revision")?,
            payload: row.try_get("payload")?,
        })
    })
    .collect::<Result<Vec<_>, sqlx::Error>>()?;

    let adoptable_revisions = sqlx::query(
        r#"
        SELECT
            adoptable_revision,
            candidate_revision,
            rendered_operator_target_revision,
            payload
        FROM adoptable_target_revisions
        ORDER BY adoptable_revision DESC
        "#,
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        Ok(AdoptableTargetRevisionRow {
            adoptable_revision: row.try_get("adoptable_revision")?,
            candidate_revision: row.try_get("candidate_revision")?,
            rendered_operator_target_revision: row.try_get("rendered_operator_target_revision")?,
            payload: row.try_get("payload")?,
        })
    })
    .collect::<Result<Vec<_>, sqlx::Error>>()?;

    Ok(TargetCandidatesCatalog {
        advisory_candidates,
        adoptable_revisions,
    })
}

pub fn summarize_target_candidates(catalog: &TargetCandidatesCatalog) -> TargetCandidatesSummary {
    let mut deferred_target_count = 0usize;
    let mut excluded_target_count = 0usize;
    let mut reasons = std::collections::BTreeSet::new();

    for candidate in &catalog.advisory_candidates {
        let Some(targets) = candidate
            .payload
            .get("targets")
            .and_then(|value| value.as_array())
        else {
            continue;
        };

        for target in targets {
            let Some(validation) = target.get("validation") else {
                continue;
            };

            match validation.get("status").and_then(|value| value.as_str()) {
                Some("deferred") => deferred_target_count += 1,
                Some("excluded") => excluded_target_count += 1,
                _ => {}
            }

            if let Some(reason) = validation.get("reason").and_then(|value| value.as_str()) {
                reasons.insert(reason.to_owned());
            }
        }
    }

    TargetCandidatesSummary {
        advisory_candidate_count: catalog.advisory_candidates.len(),
        adoptable_revision_count: catalog.adoptable_revisions.len(),
        deferred_target_count,
        excluded_target_count,
        recommended_adoptable_revision: catalog
            .adoptable_revisions
            .first()
            .map(|adoptable| adoptable.adoptable_revision.clone()),
        non_adoptable_reasons: reasons.into_iter().collect(),
    }
}

pub fn configured_operator_target_revision(
    config_path: &Path,
) -> Result<Option<String>, Box<dyn Error>> {
    let raw = load_raw_config_from_path(config_path)?;
    let validated = ValidatedConfig::new(raw)?;
    let target_source = validated.target_source()?;
    Ok(target_source
        .operator_target_revision()
        .map(ToOwned::to_owned))
}

async fn selection_from_lineage(
    pool: &PgPool,
    operator_target_revision: &str,
    adoptable_revision: &str,
    candidate_revision: &str,
) -> Result<ResolvedAdoptionSelection, Box<dyn Error>> {
    let adoptable = CandidateArtifactRepo
        .get_adoptable_target_revision(pool, adoptable_revision)
        .await?
        .ok_or_else(|| {
            TargetStateError::new(format!(
                "adoptable_revision {adoptable_revision} is unavailable"
            ))
        })?;

    if adoptable.candidate_revision != candidate_revision
        || adoptable.rendered_operator_target_revision != operator_target_revision
    {
        return Err(TargetStateError::new(format!(
            "adoptable_revision {adoptable_revision} does not match operator_target_revision {operator_target_revision}"
        ))
        .into());
    }

    CandidateArtifactRepo
        .get_candidate_target_set(pool, candidate_revision)
        .await?
        .ok_or_else(|| {
            TargetStateError::new(format!(
                "candidate_revision {candidate_revision} is unavailable"
            ))
        })?;

    validate_rendered_live_targets(&adoptable.payload, operator_target_revision)?;

    Ok(ResolvedAdoptionSelection {
        operator_target_revision: operator_target_revision.to_owned(),
        adoptable_revision: Some(adoptable_revision.to_owned()),
        candidate_revision: Some(candidate_revision.to_owned()),
    })
}

async fn latest_history_lineage(
    pool: &PgPool,
    operator_target_revision: &str,
) -> Result<Option<(String, String)>, Box<dyn Error>> {
    let rows = sqlx::query(
        r#"
        SELECT action_kind, adoptable_revision, candidate_revision
        FROM operator_target_adoption_history
        WHERE operator_target_revision = $1
        ORDER BY history_seq DESC
        "#,
    )
    .bind(operator_target_revision)
    .fetch_all(pool)
    .await?;

    let rows = rows
        .into_iter()
        .map(|row| {
            Ok(OperatorTargetAdoptionHistoryRow {
                adoption_id: String::new(),
                action_kind: row.try_get("action_kind")?,
                operator_target_revision: operator_target_revision.to_owned(),
                previous_operator_target_revision: None,
                adoptable_revision: row.try_get("adoptable_revision")?,
                candidate_revision: row.try_get("candidate_revision")?,
                adopted_at: chrono::Utc::now(),
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;

    latest_history_lineage_from_rows(operator_target_revision, &rows).map_err(Into::into)
}

fn latest_history_lineage_from_rows(
    operator_target_revision: &str,
    rows: &[OperatorTargetAdoptionHistoryRow],
) -> Result<Option<(String, String)>, TargetStateError> {
    if rows.is_empty() {
        return Ok(None);
    }

    for row in rows {
        match (
            row.action_kind.as_str(),
            row.adoptable_revision.as_deref(),
            row.candidate_revision.as_deref(),
        ) {
            ("rollback", None, None) => continue,
            ("adopt", Some(adoptable_revision), Some(candidate_revision)) => {
                return Ok(Some((
                    adoptable_revision.to_owned(),
                    candidate_revision.to_owned(),
                )))
            }
            _ => {
                return Err(TargetStateError::new(format!(
                    "operator_target_revision {operator_target_revision} has history but no durable lineage"
                )))
            }
        }
    }

    Err(TargetStateError::new(format!(
        "operator_target_revision {operator_target_revision} has history but no durable lineage"
    )))
}

fn validate_rendered_live_targets(
    payload: &serde_json::Value,
    operator_target_revision: &str,
) -> Result<(), Box<dyn Error>> {
    let rendered_live_targets = payload
        .get("rendered_live_targets")
        .ok_or_else(|| {
            TargetStateError::new(format!(
                "missing rendered_live_targets for operator_target_revision {operator_target_revision}"
            ))
        })?
        .clone();
    let targets = serde_json::from_value::<BTreeMap<String, NegRiskFamilyLiveTarget>>(
        rendered_live_targets,
    )
    .map_err(|error| {
        TargetStateError::new(format!(
            "invalid rendered_live_targets for operator_target_revision {operator_target_revision}: {error}"
        ))
    })?;

    if targets.is_empty() {
        return Err(TargetStateError::new(format!(
            "empty rendered_live_targets for operator_target_revision {operator_target_revision}"
        ))
        .into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use persistence::models::OperatorTargetAdoptionHistoryRow;
    use serde_json::json;

    use super::{
        latest_history_lineage_from_rows, summarize_target_candidates, TargetCandidatesCatalog,
    };

    fn history_row(
        action_kind: &str,
        operator_target_revision: &str,
        adoptable_revision: Option<&str>,
        candidate_revision: Option<&str>,
    ) -> OperatorTargetAdoptionHistoryRow {
        OperatorTargetAdoptionHistoryRow {
            adoption_id: format!("{action_kind}-{operator_target_revision}"),
            action_kind: action_kind.to_owned(),
            operator_target_revision: operator_target_revision.to_owned(),
            previous_operator_target_revision: None,
            adoptable_revision: adoptable_revision.map(str::to_owned),
            candidate_revision: candidate_revision.map(str::to_owned),
            adopted_at: Utc::now(),
        }
    }

    #[test]
    fn latest_history_lineage_skips_newer_rollback_rows() {
        let rows = vec![
            history_row("rollback", "targets-rev-7", None, None),
            history_row(
                "adopt",
                "targets-rev-7",
                Some("adoptable-7"),
                Some("candidate-7"),
            ),
        ];

        let lineage = latest_history_lineage_from_rows("targets-rev-7", &rows).unwrap();
        assert_eq!(
            lineage,
            Some(("adoptable-7".to_owned(), "candidate-7".to_owned()))
        );
    }

    #[test]
    fn latest_history_lineage_rejects_malformed_adopt_rows() {
        let rows = vec![history_row(
            "adopt",
            "targets-rev-7",
            Some("adoptable-7"),
            None,
        )];

        let error = latest_history_lineage_from_rows("targets-rev-7", &rows).unwrap_err();
        assert_eq!(
            error.to_string(),
            "operator_target_revision targets-rev-7 has history but no durable lineage"
        );
    }

    #[test]
    fn target_candidates_summary_prefers_first_adoptable_and_collects_non_adoptable_reasons() {
        let catalog = TargetCandidatesCatalog {
            advisory_candidates: vec![persistence::models::CandidateTargetSetRow {
                candidate_revision: "candidate-8".to_owned(),
                snapshot_id: "snapshot-8".to_owned(),
                source_revision: "discovery-8".to_owned(),
                payload: json!({
                    "targets": [
                        {
                            "validation": {
                                "status": "deferred",
                                "reason": "candidate generation deferred until discovery backfill completes",
                            }
                        },
                        {
                            "validation": {
                                "status": "excluded",
                                "reason": "candidate excluded by conservative discovery policy",
                            }
                        }
                    ]
                }),
            }],
            adoptable_revisions: vec![persistence::models::AdoptableTargetRevisionRow {
                adoptable_revision: "adoptable-9".to_owned(),
                candidate_revision: "candidate-9".to_owned(),
                rendered_operator_target_revision: "targets-rev-9".to_owned(),
                payload: json!({}),
            }],
        };

        let summary = summarize_target_candidates(&catalog);
        assert_eq!(summary.advisory_candidate_count, 1);
        assert_eq!(summary.adoptable_revision_count, 1);
        assert_eq!(summary.deferred_target_count, 1);
        assert_eq!(summary.excluded_target_count, 1);
        assert_eq!(
            summary.recommended_adoptable_revision.as_deref(),
            Some("adoptable-9")
        );
        assert_eq!(
            summary.non_adoptable_reasons,
            vec![
                "candidate excluded by conservative discovery policy".to_owned(),
                "candidate generation deferred until discovery backfill completes".to_owned(),
            ]
        );
        assert_eq!(summary.non_adoptable_summary(), "deferred:1 excluded:1");
    }
}
