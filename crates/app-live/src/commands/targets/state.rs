use std::{collections::BTreeMap, error::Error, path::Path, str::FromStr};

use config_schema::{load_raw_config_from_path, RawAxiomConfig, ValidatedConfig};
use persistence::{
    models::{
        AdoptableStrategyRevisionRow, OperatorStrategyAdoptionHistoryRow,
        StrategyAdoptionProvenanceRow, StrategyCandidateSetRow,
    },
    OperatorStrategyAdoptionHistoryRepo, RuntimeProgressRepo, StrategyAdoptionRepo,
    StrategyControlArtifactRepo,
};
use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use toml_edit::DocumentMut;

use crate::config::{
    neg_risk_live_target_revision_from_targets, NegRiskFamilyLiveTarget, NegRiskLiveTargetSet,
    NegRiskMemberLiveTarget,
};
use crate::strategy_control::validate_live_route_scope;

const LEGACY_EXPLICIT_COMPATIBILITY_MODE: &str = "legacy-explicit";

#[derive(Debug, Clone)]
pub struct TargetControlPlaneState {
    pub configured_operator_strategy_revision: Option<String>,
    pub active_operator_strategy_revision: Option<String>,
    pub restart_needed: Option<bool>,
    pub compatibility_mode: Option<String>,
    pub provenance: Option<StrategyAdoptionProvenanceRow>,
    pub latest_action: Option<OperatorStrategyAdoptionHistoryRow>,
}

#[derive(Debug, Clone, Default)]
pub struct TargetCandidatesCatalog {
    pub advisory_candidates: Vec<StrategyCandidateSetRow>,
    pub adoptable_revisions: Vec<AdoptableStrategyRevisionRow>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedStrategyAdoptionSelection {
    pub operator_strategy_revision: String,
    pub adoptable_revision: Option<String>,
    pub migration_source: Option<String>,
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
    operator_strategy_revision: Option<&str>,
    adoptable_revision: Option<&str>,
) -> Result<ResolvedStrategyAdoptionSelection, Box<dyn Error>> {
    match (operator_strategy_revision, adoptable_revision) {
        (Some(_), Some(_)) | (None, None) => Err(TargetStateError::new(
            "exactly one of --operator-strategy-revision or --adoptable-revision must be provided",
        )
        .into()),
        (Some(operator_strategy_revision), None) => {
            if let Some(provenance) = StrategyAdoptionRepo
                .get_by_operator_strategy_revision(pool, operator_strategy_revision)
                .await?
            {
                return selection_from_lineage(
                    pool,
                    &provenance.operator_strategy_revision,
                    &provenance.adoptable_strategy_revision,
                    &provenance.strategy_candidate_revision,
                )
                .await;
            }

            if let Some((adoptable_revision, strategy_candidate_revision)) =
                latest_history_lineage(pool, operator_strategy_revision).await?
            {
                return selection_from_lineage(
                    pool,
                    operator_strategy_revision,
                    &adoptable_revision,
                    &strategy_candidate_revision,
                )
                .await;
            }

            Err(TargetStateError::new(format!(
                "operator_strategy_revision {operator_strategy_revision} has no durable adoption provenance or history lineage"
            ))
            .into())
        }
        (None, Some(adoptable_revision)) => {
            let adoptable = StrategyControlArtifactRepo
                .get_adoptable_strategy_revision(pool, adoptable_revision)
                .await?
                .ok_or_else(|| {
                    TargetStateError::new(format!(
                        "adoptable_revision {adoptable_revision} is unavailable"
                    ))
                })?;
            validate_strategy_payload(
                &adoptable.payload,
                &adoptable.rendered_operator_strategy_revision,
            )?;
            StrategyControlArtifactRepo
                .get_strategy_candidate_set(pool, &adoptable.strategy_candidate_revision)
                .await?
                .ok_or_else(|| {
                    TargetStateError::new(format!(
                        "strategy_candidate_revision {} is unavailable",
                        adoptable.strategy_candidate_revision
                    ))
                })?;

            Ok(ResolvedStrategyAdoptionSelection {
                operator_strategy_revision: adoptable.rendered_operator_strategy_revision,
                adoptable_revision: Some(adoptable.adoptable_strategy_revision),
                migration_source: None,
            })
        }
    }
}

pub async fn resolve_rollback_selection(
    pool: &PgPool,
    compatibility_mode: Option<&str>,
    configured_operator_strategy_revision: Option<&str>,
    to_operator_strategy_revision: Option<&str>,
) -> Result<ResolvedStrategyAdoptionSelection, Box<dyn Error>> {
    let configured_operator_strategy_revision =
        configured_operator_strategy_revision.ok_or_else(|| {
            TargetStateError::new("configured operator_strategy_revision is unavailable")
        })?;
    if compatibility_mode == Some(LEGACY_EXPLICIT_COMPATIBILITY_MODE) {
        let has_neutral_history = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM operator_strategy_adoption_history
                WHERE operator_strategy_revision = $1
            )
            "#,
        )
        .bind(configured_operator_strategy_revision)
        .fetch_one(pool)
        .await?;
        if !has_neutral_history {
            return Err(TargetStateError::new(
                "neutral adoption history is required before compatibility mode can roll back",
            )
            .into());
        }
    }

    let destination = match to_operator_strategy_revision {
        Some(operator_strategy_revision) => operator_strategy_revision.to_owned(),
        None => OperatorStrategyAdoptionHistoryRepo
            .previous_distinct_revision(pool, configured_operator_strategy_revision)
            .await?
            .ok_or_else(|| {
                TargetStateError::new(format!(
                    "operator_strategy_revision {configured_operator_strategy_revision} has no previous distinct revision"
                ))
            })?,
    };

    resolve_adoption_selection(pool, Some(destination.as_str()), None).await
}

pub async fn load_active_operator_strategy_revision(
    pool: &PgPool,
    configured_operator_strategy_revision: Option<&str>,
) -> Result<Option<String>, Box<dyn Error>> {
    match RuntimeProgressRepo.current(pool).await? {
        Some(row) => normalize_active_operator_strategy_revision(
            configured_operator_strategy_revision,
            row.operator_target_revision.as_deref(),
            row.operator_strategy_revision.as_deref(),
        )
        .map(Some)
        .ok_or_else(|| {
            TargetStateError::new(
                "runtime progress row exists without operator_strategy_revision anchor",
            )
            .into()
        }),
        None => Ok(None),
    }
}

pub fn normalize_active_operator_strategy_revision(
    configured_operator_strategy_revision: Option<&str>,
    active_operator_target_revision: Option<&str>,
    active_operator_strategy_revision: Option<&str>,
) -> Option<String> {
    if let Some(active_operator_strategy_revision) = active_operator_strategy_revision {
        return Some(active_operator_strategy_revision.to_owned());
    }

    let active_operator_target_revision = active_operator_target_revision?;
    if let Some(configured_operator_strategy_revision) = configured_operator_strategy_revision {
        if configured_operator_strategy_revision == active_operator_target_revision
            || legacy_target_revision_matches_strategy_revision(
                configured_operator_strategy_revision,
                active_operator_target_revision,
            )
        {
            return Some(configured_operator_strategy_revision.to_owned());
        }
    }

    Some(active_operator_target_revision.to_owned())
}

pub async fn load_target_control_plane_state(
    pool: &PgPool,
    config_path: &Path,
) -> Result<TargetControlPlaneState, Box<dyn Error>> {
    let compatibility_mode = compatibility_mode(config_path)?;
    let configured_operator_strategy_revision = configured_operator_strategy_revision(config_path)?;
    let active_operator_strategy_revision = load_active_operator_strategy_revision(
        pool,
        configured_operator_strategy_revision.as_deref(),
    )
    .await?;
    let restart_needed = match (
        configured_operator_strategy_revision.as_deref(),
        active_operator_strategy_revision.as_deref(),
    ) {
        (Some(configured), Some(active)) => Some(configured != active),
        _ => None,
    };
    let provenance = match configured_operator_strategy_revision.as_deref() {
        Some(operator_strategy_revision) => Some(
            StrategyAdoptionRepo
                .get_by_operator_strategy_revision(pool, operator_strategy_revision)
                .await?
                .ok_or_else(|| {
                    TargetStateError::new(format!(
                        "configured operator_strategy_revision {operator_strategy_revision} has no durable adoption provenance"
                    ))
                })?,
        ),
        None => None,
    };
    let latest_action = OperatorStrategyAdoptionHistoryRepo.latest(pool).await?;

    Ok(TargetControlPlaneState {
        configured_operator_strategy_revision,
        active_operator_strategy_revision,
        restart_needed,
        compatibility_mode,
        provenance,
        latest_action,
    })
}

pub async fn load_target_candidates_catalog(
    pool: &PgPool,
) -> Result<TargetCandidatesCatalog, Box<dyn Error>> {
    let advisory_candidates = sqlx::query(
        r#"
        SELECT strategy_candidate_revision, snapshot_id, source_revision, payload
        FROM (
            SELECT
                strategy_candidate_revision,
                snapshot_id,
                source_revision,
                payload,
                ROW_NUMBER() OVER (
                    PARTITION BY strategy_candidate_revision
                    ORDER BY source_priority ASC
                ) AS row_rank
            FROM (
                SELECT
                    candidate.strategy_candidate_revision,
                    candidate.snapshot_id,
                    candidate.source_revision,
                    candidate.payload,
                    0 AS source_priority
                FROM strategy_candidate_sets AS candidate
                WHERE NOT EXISTS (
                    SELECT 1
                    FROM adoptable_strategy_revisions AS adoptable
                    WHERE adoptable.strategy_candidate_revision = candidate.strategy_candidate_revision
                )
                UNION ALL
                SELECT
                    candidate.candidate_revision AS strategy_candidate_revision,
                    candidate.snapshot_id,
                    candidate.source_revision,
                    candidate.payload,
                    1 AS source_priority
                FROM candidate_target_sets AS candidate
                WHERE NOT EXISTS (
                    SELECT 1
                    FROM adoptable_target_revisions AS adoptable
                    WHERE adoptable.candidate_revision = candidate.candidate_revision
                )
            ) AS combined_advisory
        ) AS advisory
        WHERE row_rank = 1
        ORDER BY strategy_candidate_revision DESC
        "#,
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(map_strategy_candidate_row)
    .collect::<Result<Vec<_>, _>>()?;

    let adoptable_revisions = sqlx::query(
        r#"
        SELECT
            adoptable_strategy_revision,
            strategy_candidate_revision,
            rendered_operator_strategy_revision,
            payload
        FROM (
            SELECT
                adoptable_strategy_revision,
                strategy_candidate_revision,
                rendered_operator_strategy_revision,
                payload,
                ROW_NUMBER() OVER (
                    PARTITION BY adoptable_strategy_revision
                    ORDER BY source_priority ASC
                ) AS row_rank
            FROM (
                SELECT
                    adoptable_strategy_revision,
                    strategy_candidate_revision,
                    rendered_operator_strategy_revision,
                    payload,
                    0 AS source_priority
                FROM adoptable_strategy_revisions
                UNION ALL
                SELECT
                    adoptable_revision AS adoptable_strategy_revision,
                    candidate_revision AS strategy_candidate_revision,
                    rendered_operator_target_revision AS rendered_operator_strategy_revision,
                    payload,
                    1 AS source_priority
                FROM adoptable_target_revisions
            ) AS combined_adoptable
        ) AS adoptable
        WHERE row_rank = 1
        ORDER BY adoptable_strategy_revision DESC
        "#,
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(map_adoptable_strategy_row)
    .collect::<Result<Vec<_>, _>>()?;

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
            .map(|adoptable| adoptable.adoptable_strategy_revision.clone()),
        non_adoptable_reasons: reasons.into_iter().collect(),
    }
}

pub fn configured_operator_strategy_revision(
    config_path: &Path,
) -> Result<Option<String>, Box<dyn Error>> {
    let raw = load_validated_raw_config(config_path)?;
    if is_legacy_explicit_strategy_config(&raw) {
        return Ok(None);
    }

    Ok(raw
        .strategy_control
        .as_ref()
        .and_then(|strategy_control| strategy_control.operator_strategy_revision.as_deref())
        .or_else(|| {
            raw.negrisk
                .as_ref()
                .and_then(|negrisk| negrisk.target_source.as_ref())
                .and_then(|target_source| target_source.operator_target_revision.as_deref())
        })
        .map(ToOwned::to_owned))
}

pub fn compatibility_mode(config_path: &Path) -> Result<Option<String>, Box<dyn Error>> {
    let raw = load_validated_raw_config(config_path)?;
    Ok(is_legacy_explicit_strategy_config(&raw)
        .then(|| LEGACY_EXPLICIT_COMPATIBILITY_MODE.to_owned()))
}

pub fn synthetic_strategy_revision_for_legacy_explicit_config(
    config_path: &Path,
) -> Result<String, Box<dyn Error>> {
    let targets = legacy_explicit_targets(config_path)?;
    let digest = neg_risk_live_target_revision_from_targets(targets.targets());
    let digest = digest.strip_prefix("sha256:").unwrap_or(digest.as_str());
    Ok(format!("strategy-rev-{digest}"))
}

pub fn legacy_explicit_targets(config_path: &Path) -> Result<NegRiskLiveTargetSet, Box<dyn Error>> {
    let raw = load_validated_raw_config(config_path)?;
    if !is_legacy_explicit_strategy_config(&raw) {
        return Err(TargetStateError::new(
            "migration required: legacy explicit targets require explicit negrisk.targets",
        )
        .into());
    }

    let text = std::fs::read_to_string(config_path)?;
    let document = text.parse::<DocumentMut>()?;
    parse_legacy_explicit_targets(&document)
}

fn load_validated_raw_config(config_path: &Path) -> Result<RawAxiomConfig, Box<dyn Error>> {
    let raw = load_raw_config_from_path(config_path)?;
    let _ = ValidatedConfig::new(raw.clone())?;
    Ok(raw)
}

fn is_legacy_explicit_strategy_config(raw: &RawAxiomConfig) -> bool {
    raw.negrisk
        .as_ref()
        .map(|negrisk| negrisk.targets.is_present())
        .unwrap_or(false)
}

fn legacy_target_revision_matches_strategy_revision(
    operator_strategy_revision: &str,
    operator_target_revision: &str,
) -> bool {
    operator_strategy_revision
        .strip_prefix("strategy-rev-")
        .map(|digest| operator_target_revision == format!("sha256:{digest}"))
        .unwrap_or(false)
}

fn parse_legacy_explicit_targets(
    document: &DocumentMut,
) -> Result<NegRiskLiveTargetSet, Box<dyn Error>> {
    let targets = document["negrisk"]["targets"]
        .as_array_of_tables()
        .ok_or_else(|| {
            TargetStateError::new(
                "migration required: legacy explicit targets require explicit negrisk.targets",
            )
        })?;

    let mut family_targets = BTreeMap::new();
    for target in targets.iter() {
        let family_id = target
            .get("family_id")
            .and_then(|item| item.as_str())
            .ok_or_else(|| TargetStateError::new("negrisk.targets.family_id must be present"))?
            .to_owned();

        let members = target
            .get("members")
            .and_then(|item| item.as_array_of_tables())
            .ok_or_else(|| TargetStateError::new("negrisk.targets.members must be present"))?;
        let members = members
            .iter()
            .map(|member| {
                let condition_id = member
                    .get("condition_id")
                    .and_then(|item| item.as_str())
                    .ok_or_else(|| {
                        TargetStateError::new(
                            "negrisk.targets.members.condition_id must be present",
                        )
                    })?;
                let token_id = member
                    .get("token_id")
                    .and_then(|item| item.as_str())
                    .ok_or_else(|| {
                        TargetStateError::new("negrisk.targets.members.token_id must be present")
                    })?;
                let price = member
                    .get("price")
                    .and_then(|item| item.as_str())
                    .ok_or_else(|| {
                        TargetStateError::new("negrisk.targets.members.price must be present")
                    })?;
                let quantity = member
                    .get("quantity")
                    .and_then(|item| item.as_str())
                    .ok_or_else(|| {
                        TargetStateError::new("negrisk.targets.members.quantity must be present")
                    })?;

                Ok(NegRiskMemberLiveTarget {
                    condition_id: condition_id.to_owned(),
                    token_id: token_id.to_owned(),
                    price: Decimal::from_str(price).map_err(|error| {
                        TargetStateError::new(format!(
                            "invalid negrisk.targets.members.price: {error}"
                        ))
                    })?,
                    quantity: Decimal::from_str(quantity).map_err(|error| {
                        TargetStateError::new(format!(
                            "invalid negrisk.targets.members.quantity: {error}"
                        ))
                    })?,
                })
            })
            .collect::<Result<Vec<_>, TargetStateError>>()?;

        if family_targets
            .insert(
                family_id.clone(),
                NegRiskFamilyLiveTarget {
                    family_id: family_id.clone(),
                    members,
                },
            )
            .is_some()
        {
            return Err(TargetStateError::new(format!(
                "duplicate neg-risk family_id in live target config: {family_id}"
            ))
            .into());
        }
    }

    Ok(NegRiskLiveTargetSet::from_targets_with_revision(
        neg_risk_live_target_revision_from_targets(&family_targets),
        family_targets,
    ))
}

async fn selection_from_lineage(
    pool: &PgPool,
    operator_strategy_revision: &str,
    adoptable_revision: &str,
    strategy_candidate_revision: &str,
) -> Result<ResolvedStrategyAdoptionSelection, Box<dyn Error>> {
    let adoptable = StrategyControlArtifactRepo
        .get_adoptable_strategy_revision(pool, adoptable_revision)
        .await?
        .ok_or_else(|| {
            TargetStateError::new(format!(
                "adoptable_revision {adoptable_revision} is unavailable"
            ))
        })?;

    if adoptable.strategy_candidate_revision != strategy_candidate_revision
        || adoptable.rendered_operator_strategy_revision != operator_strategy_revision
    {
        return Err(TargetStateError::new(format!(
            "adoptable_revision {adoptable_revision} does not match operator_strategy_revision {operator_strategy_revision}"
        ))
        .into());
    }

    StrategyControlArtifactRepo
        .get_strategy_candidate_set(pool, strategy_candidate_revision)
        .await?
        .ok_or_else(|| {
            TargetStateError::new(format!(
                "strategy_candidate_revision {strategy_candidate_revision} is unavailable"
            ))
        })?;

    validate_strategy_payload(&adoptable.payload, operator_strategy_revision)?;

    Ok(ResolvedStrategyAdoptionSelection {
        operator_strategy_revision: operator_strategy_revision.to_owned(),
        adoptable_revision: Some(adoptable_revision.to_owned()),
        migration_source: None,
    })
}

async fn latest_history_lineage(
    pool: &PgPool,
    operator_strategy_revision: &str,
) -> Result<Option<(String, String)>, Box<dyn Error>> {
    let rows = sqlx::query(
        r#"
        SELECT action_kind, adoptable_strategy_revision, strategy_candidate_revision
        FROM (
            SELECT
                action_kind,
                adoptable_strategy_revision,
                strategy_candidate_revision,
                history_seq,
                0 AS source_priority
            FROM operator_strategy_adoption_history
            WHERE operator_strategy_revision = $1
            UNION ALL
            SELECT
                action_kind,
                adoptable_revision AS adoptable_strategy_revision,
                candidate_revision AS strategy_candidate_revision,
                history_seq,
                1 AS source_priority
            FROM operator_target_adoption_history
            WHERE operator_target_revision = $1
        ) AS combined_history
        ORDER BY history_seq DESC, source_priority ASC
        "#,
    )
    .bind(operator_strategy_revision)
    .fetch_all(pool)
    .await?;

    let rows = rows
        .into_iter()
        .map(|row| {
            Ok(OperatorStrategyAdoptionHistoryRow {
                adoption_id: String::new(),
                action_kind: row.try_get("action_kind")?,
                operator_strategy_revision: operator_strategy_revision.to_owned(),
                previous_operator_strategy_revision: None,
                adoptable_strategy_revision: row.try_get("adoptable_strategy_revision")?,
                strategy_candidate_revision: row.try_get("strategy_candidate_revision")?,
                adopted_at: chrono::Utc::now(),
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;

    latest_history_lineage_from_rows(operator_strategy_revision, &rows).map_err(Into::into)
}

fn latest_history_lineage_from_rows(
    operator_strategy_revision: &str,
    rows: &[OperatorStrategyAdoptionHistoryRow],
) -> Result<Option<(String, String)>, TargetStateError> {
    if rows.is_empty() {
        return Ok(None);
    }

    for row in rows {
        match (
            row.action_kind.as_str(),
            row.adoptable_strategy_revision.as_deref(),
            row.strategy_candidate_revision.as_deref(),
        ) {
            ("rollback", None, None) => continue,
            ("adopt", Some(adoptable_revision), Some(strategy_candidate_revision)) => {
                return Ok(Some((
                    adoptable_revision.to_owned(),
                    strategy_candidate_revision.to_owned(),
                )))
            }
            _ => {
                return Err(TargetStateError::new(format!(
                    "operator_strategy_revision {operator_strategy_revision} has history but no durable lineage"
                )))
            }
        }
    }

    Err(TargetStateError::new(format!(
        "operator_strategy_revision {operator_strategy_revision} has history but no durable lineage"
    )))
}

fn validate_strategy_payload(
    payload: &serde_json::Value,
    operator_strategy_revision: &str,
) -> Result<(), Box<dyn Error>> {
    if let Some(route_artifacts) = payload.get("route_artifacts") {
        let artifacts = serde_json::from_value::<Vec<ValidatedRouteArtifact>>(route_artifacts.clone())
            .map_err(|error| {
                TargetStateError::new(format!(
                    "invalid route_artifacts for operator_strategy_revision {operator_strategy_revision}: {error}"
                ))
            })?;
        if artifacts.is_empty() {
            return Err(TargetStateError::new(format!(
                "empty route_artifacts for operator_strategy_revision {operator_strategy_revision}"
            ))
            .into());
        }
        if artifacts.iter().any(|artifact| {
            artifact.key.route.trim().is_empty()
                || artifact.key.scope.trim().is_empty()
                || artifact.route_policy_version.trim().is_empty()
                || artifact.semantic_digest.trim().is_empty()
        }) {
            return Err(TargetStateError::new(format!(
                "invalid route_artifacts for operator_strategy_revision {operator_strategy_revision}"
            ))
            .into());
        }
        for artifact in &artifacts {
            validate_live_route_scope(&artifact.key.route, &artifact.key.scope).map_err(|error| {
                TargetStateError::new(format!(
                    "invalid route_artifacts for operator_strategy_revision {operator_strategy_revision}: {error}"
                ))
            })?;
        }
        return Ok(());
    }

    validate_rendered_live_targets(payload, operator_strategy_revision)
}

fn validate_rendered_live_targets(
    payload: &serde_json::Value,
    operator_strategy_revision: &str,
) -> Result<(), Box<dyn Error>> {
    let rendered_live_targets = payload
        .get("rendered_live_targets")
        .ok_or_else(|| {
            TargetStateError::new(format!(
                "missing rendered_live_targets for operator_strategy_revision {operator_strategy_revision}"
            ))
        })?
        .clone();
    let targets = serde_json::from_value::<BTreeMap<String, NegRiskFamilyLiveTarget>>(
        rendered_live_targets,
    )
    .map_err(|error| {
        TargetStateError::new(format!(
            "invalid rendered_live_targets for operator_strategy_revision {operator_strategy_revision}: {error}"
        ))
    })?;

    if targets.is_empty() {
        return Err(TargetStateError::new(format!(
            "empty rendered_live_targets for operator_strategy_revision {operator_strategy_revision}"
        ))
        .into());
    }

    Ok(())
}

#[derive(serde::Deserialize)]
struct ValidatedRouteArtifactKey {
    route: String,
    scope: String,
}

#[derive(serde::Deserialize)]
struct ValidatedRouteArtifact {
    key: ValidatedRouteArtifactKey,
    route_policy_version: String,
    semantic_digest: String,
    #[allow(dead_code)]
    content: serde_json::Value,
}

fn map_strategy_candidate_row(
    row: sqlx::postgres::PgRow,
) -> Result<StrategyCandidateSetRow, sqlx::Error> {
    Ok(StrategyCandidateSetRow {
        strategy_candidate_revision: row.try_get("strategy_candidate_revision")?,
        snapshot_id: row.try_get("snapshot_id")?,
        source_revision: row.try_get("source_revision")?,
        payload: row.try_get("payload")?,
    })
}

fn map_adoptable_strategy_row(
    row: sqlx::postgres::PgRow,
) -> Result<AdoptableStrategyRevisionRow, sqlx::Error> {
    Ok(AdoptableStrategyRevisionRow {
        adoptable_strategy_revision: row.try_get("adoptable_strategy_revision")?,
        strategy_candidate_revision: row.try_get("strategy_candidate_revision")?,
        rendered_operator_strategy_revision: row.try_get("rendered_operator_strategy_revision")?,
        payload: row.try_get("payload")?,
    })
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use persistence::models::OperatorStrategyAdoptionHistoryRow;
    use serde_json::json;

    use super::{
        latest_history_lineage_from_rows, normalize_active_operator_strategy_revision,
        summarize_target_candidates, validate_strategy_payload, TargetCandidatesCatalog,
    };

    fn history_row(
        action_kind: &str,
        operator_strategy_revision: &str,
        adoptable_revision: Option<&str>,
        strategy_candidate_revision: Option<&str>,
    ) -> OperatorStrategyAdoptionHistoryRow {
        OperatorStrategyAdoptionHistoryRow {
            adoption_id: format!("{action_kind}-{operator_strategy_revision}"),
            action_kind: action_kind.to_owned(),
            operator_strategy_revision: operator_strategy_revision.to_owned(),
            previous_operator_strategy_revision: None,
            adoptable_strategy_revision: adoptable_revision.map(str::to_owned),
            strategy_candidate_revision: strategy_candidate_revision.map(str::to_owned),
            adopted_at: Utc::now(),
        }
    }

    #[test]
    fn latest_history_lineage_skips_newer_rollback_rows() {
        let rows = vec![
            history_row("rollback", "strategy-rev-7", None, None),
            history_row(
                "adopt",
                "strategy-rev-7",
                Some("adoptable-7"),
                Some("candidate-7"),
            ),
        ];

        let lineage = latest_history_lineage_from_rows("strategy-rev-7", &rows).unwrap();

        assert_eq!(
            lineage,
            Some(("adoptable-7".to_owned(), "candidate-7".to_owned()))
        );
    }

    #[test]
    fn latest_history_lineage_rejects_rows_without_durable_lineage() {
        let rows = vec![history_row(
            "adopt",
            "strategy-rev-7",
            Some("adoptable-7"),
            None,
        )];

        let error = latest_history_lineage_from_rows("strategy-rev-7", &rows).unwrap_err();

        assert_eq!(
            error.to_string(),
            "operator_strategy_revision strategy-rev-7 has history but no durable lineage"
        );
    }

    #[test]
    fn summarize_target_candidates_counts_validation_states() {
        let catalog = TargetCandidatesCatalog {
            advisory_candidates: vec![persistence::models::StrategyCandidateSetRow {
                strategy_candidate_revision: "candidate-8".to_owned(),
                snapshot_id: "snapshot-8".to_owned(),
                source_revision: "discovery-8".to_owned(),
                payload: json!({
                    "targets": [
                        {
                            "validation": {
                                "status": "deferred",
                                "reason": "discovery backlog"
                            }
                        },
                        {
                            "validation": {
                                "status": "excluded",
                                "reason": "safety filter"
                            }
                        }
                    ]
                }),
            }],
            adoptable_revisions: vec![persistence::models::AdoptableStrategyRevisionRow {
                adoptable_strategy_revision: "adoptable-9".to_owned(),
                strategy_candidate_revision: "candidate-9".to_owned(),
                rendered_operator_strategy_revision: "strategy-rev-9".to_owned(),
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
            vec!["discovery backlog".to_owned(), "safety filter".to_owned()]
        );
    }

    #[test]
    fn normalize_active_operator_strategy_revision_promotes_matching_legacy_digest_anchor() {
        let normalized = normalize_active_operator_strategy_revision(
            Some("strategy-rev-deadbeef"),
            Some("sha256:deadbeef"),
            None,
        );

        assert_eq!(normalized.as_deref(), Some("strategy-rev-deadbeef"));
    }

    #[test]
    fn validate_strategy_payload_rejects_malformed_route_artifacts_entries() {
        let error = validate_strategy_payload(
            &json!({
                "route_artifacts": [
                    {
                        "key": {
                            "route": "neg-risk"
                        }
                    }
                ]
            }),
            "strategy-rev-9",
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("invalid route_artifacts for operator_strategy_revision strategy-rev-9"));
    }

    #[test]
    fn validate_strategy_payload_rejects_invalid_route_scope_for_registered_route() {
        let error = validate_strategy_payload(
            &json!({
                "route_artifacts": [
                    {
                        "key": {
                            "route": "full-set",
                            "scope": "family-a"
                        },
                        "route_policy_version": "full-set-policy-v1",
                        "semantic_digest": "digest",
                        "content": {}
                    }
                ],
                "rendered_live_targets": {
                    "family-a": {
                        "family_id": "family-a",
                        "members": [
                            {
                                "condition_id": "condition-1",
                                "token_id": "token-1",
                                "price": "0.43",
                                "quantity": "5"
                            }
                        ]
                    }
                }
            }),
            "strategy-rev-9",
        )
        .unwrap_err();

        let text = error.to_string();
        assert!(text.contains("full-set"), "{text}");
        assert!(text.contains("default scope"), "{text}");
    }
}
