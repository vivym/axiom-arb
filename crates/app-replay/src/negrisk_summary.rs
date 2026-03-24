use std::{
    collections::{HashMap, HashSet},
    error::Error as StdError,
    fmt,
};

use persistence::{models::NegRiskFamilyMemberRow, NegRiskFamilyRepo, PersistenceError};
use serde_json::Value;
use sqlx::{PgPool, Row};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegRiskFoundationSummary {
    pub discovered_family_count: u64,
    pub validated_family_count: u64,
    pub excluded_family_count: u64,
    pub halted_family_count: u64,
    pub recent_validation_event_count: u64,
    pub recent_halt_event_count: u64,
    pub latest_discovery_revision: i64,
    pub families: Vec<NegRiskFoundationFamilySummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegRiskFoundationFamilySummary {
    pub event_family_id: String,
    pub validation_status: Option<String>,
    pub exclusion_reason: Option<String>,
    pub validation_metadata_snapshot_hash: Option<String>,
    pub halted: bool,
    pub halt_reason: Option<String>,
    pub halt_metadata_snapshot_hash: Option<String>,
    pub validation_member_vector_path: Option<NegRiskMemberVectorPath>,
    pub halt_member_vector_path: Option<NegRiskMemberVectorPath>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegRiskMemberVectorPath {
    pub journal_seq: i64,
    pub event_type: String,
    pub event_family_id: String,
}

#[derive(Debug)]
pub enum NegRiskSummaryError {
    Persistence(PersistenceError),
    Sqlx(sqlx::Error),
    MissingDiscoverySnapshot,
    MissingJournalEntry {
        journal_seq: i64,
        event_type: String,
    },
    InvalidPayload {
        field: &'static str,
        value: String,
    },
}

impl fmt::Display for NegRiskSummaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Persistence(err) => write!(f, "{err}"),
            Self::Sqlx(err) => write!(f, "{err}"),
            Self::MissingDiscoverySnapshot => {
                write!(f, "missing neg-risk discovery snapshot event")
            }
            Self::MissingJournalEntry {
                journal_seq,
                event_type,
            } => write!(
                f,
                "missing journal entry seq={journal_seq} event_type={event_type}"
            ),
            Self::InvalidPayload { field, value } => {
                write!(f, "invalid {field} payload value: {value}")
            }
        }
    }
}

impl StdError for NegRiskSummaryError {}

impl From<PersistenceError> for NegRiskSummaryError {
    fn from(value: PersistenceError) -> Self {
        Self::Persistence(value)
    }
}

impl From<sqlx::Error> for NegRiskSummaryError {
    fn from(value: sqlx::Error) -> Self {
        Self::Sqlx(value)
    }
}

pub async fn load_neg_risk_foundation_summary(
    pool: &PgPool,
) -> Result<NegRiskFoundationSummary, NegRiskSummaryError> {
    let discovery = latest_discovery_snapshot(pool)
        .await?
        .ok_or(NegRiskSummaryError::MissingDiscoverySnapshot)?;
    let authoritative_family_ids = discovery.family_ids.clone();
    let authoritative_family_set = authoritative_family_ids
        .iter()
        .cloned()
        .collect::<HashSet<_>>();

    let validation_rows = NegRiskFamilyRepo
        .list_validations(pool)
        .await?
        .into_iter()
        .filter(|row| {
            authoritative_family_set.contains(&row.event_family_id)
                && row.last_seen_discovery_revision == discovery.latest_discovery_revision
        })
        .collect::<Vec<_>>();
    let halt_rows = NegRiskFamilyRepo
        .list_halts(pool)
        .await?
        .into_iter()
        .filter(|row| {
            authoritative_family_set.contains(&row.event_family_id)
                && row.last_seen_discovery_revision == discovery.latest_discovery_revision
        })
        .collect::<Vec<_>>();

    let member_paths = latest_member_vector_paths(pool).await?;
    let recent_validation_event_count = count_recent_family_events(
        pool,
        "family_validation",
        discovery.latest_discovery_revision,
    )
    .await?;
    let recent_halt_event_count =
        count_recent_family_events(pool, "family_halt", discovery.latest_discovery_revision)
            .await?;

    let validated_family_count = validation_rows.len() as u64;
    let excluded_family_count = validation_rows
        .iter()
        .filter(|row| row.validation_status.eq_ignore_ascii_case("excluded"))
        .count() as u64;
    let halted_family_count = halt_rows.iter().filter(|row| row.halted).count() as u64;

    let mut families_by_id = authoritative_family_ids
        .into_iter()
        .map(|family_id| {
            (
                family_id.clone(),
                NegRiskFoundationFamilySummary {
                    event_family_id: family_id,
                    validation_status: None,
                    exclusion_reason: None,
                    validation_metadata_snapshot_hash: None,
                    halted: false,
                    halt_reason: None,
                    halt_metadata_snapshot_hash: None,
                    validation_member_vector_path: None,
                    halt_member_vector_path: None,
                },
            )
        })
        .collect::<HashMap<_, _>>();

    for row in &validation_rows {
        let entry = families_by_id
            .entry(row.event_family_id.clone())
            .or_insert_with(|| NegRiskFoundationFamilySummary {
                event_family_id: row.event_family_id.clone(),
                validation_status: None,
                exclusion_reason: None,
                validation_metadata_snapshot_hash: None,
                halted: false,
                halt_reason: None,
                halt_metadata_snapshot_hash: None,
                validation_member_vector_path: None,
                halt_member_vector_path: None,
            });
        entry.validation_status = Some(row.validation_status.clone());
        entry.exclusion_reason = row.exclusion_reason.clone();
        entry.validation_metadata_snapshot_hash = Some(row.metadata_snapshot_hash.clone());
        entry.validation_member_vector_path = member_paths
            .validation
            .get(&MemberVectorPathKey {
                event_family_id: row.event_family_id.clone(),
                metadata_snapshot_hash: Some(row.metadata_snapshot_hash.clone()),
            })
            .cloned();
    }

    for row in &halt_rows {
        let entry = families_by_id
            .entry(row.event_family_id.clone())
            .or_insert_with(|| NegRiskFoundationFamilySummary {
                event_family_id: row.event_family_id.clone(),
                validation_status: None,
                exclusion_reason: None,
                validation_metadata_snapshot_hash: None,
                halted: false,
                halt_reason: None,
                halt_metadata_snapshot_hash: None,
                validation_member_vector_path: None,
                halt_member_vector_path: None,
            });
        entry.halted = row.halted;
        entry.halt_reason = row.reason.clone();
        entry.halt_metadata_snapshot_hash = row.metadata_snapshot_hash.clone();
        entry.halt_member_vector_path = member_paths
            .halt
            .get(&MemberVectorPathKey {
                event_family_id: row.event_family_id.clone(),
                metadata_snapshot_hash: row.metadata_snapshot_hash.clone(),
            })
            .cloned();
    }

    let mut families = families_by_id.into_values().collect::<Vec<_>>();
    families.sort_by(|left, right| left.event_family_id.cmp(&right.event_family_id));

    Ok(NegRiskFoundationSummary {
        discovered_family_count: discovery.discovered_family_count,
        validated_family_count,
        excluded_family_count,
        halted_family_count,
        recent_validation_event_count,
        recent_halt_event_count,
        latest_discovery_revision: discovery.latest_discovery_revision,
        families,
    })
}

pub async fn load_member_vector_from_journal(
    pool: &PgPool,
    path: &NegRiskMemberVectorPath,
) -> Result<Vec<NegRiskFamilyMemberRow>, NegRiskSummaryError> {
    let payload = sqlx::query_scalar(
        r#"
        SELECT payload
        FROM event_journal
        WHERE journal_seq = $1 AND event_type = $2
        "#,
    )
    .bind(path.journal_seq)
    .bind(&path.event_type)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| NegRiskSummaryError::MissingJournalEntry {
        journal_seq: path.journal_seq,
        event_type: path.event_type.clone(),
    })?;

    let event_family_id = required_str(&payload, "event_family_id")?;
    if event_family_id != path.event_family_id {
        return Err(NegRiskSummaryError::InvalidPayload {
            field: "event_family_id",
            value: payload.to_string(),
        });
    }

    let vector = payload
        .get("member_vector")
        .and_then(Value::as_array)
        .ok_or_else(|| NegRiskSummaryError::InvalidPayload {
            field: "member_vector",
            value: payload.to_string(),
        })?;

    vector
        .iter()
        .map(|member| {
            Ok(NegRiskFamilyMemberRow {
                condition_id: required_str(member, "condition_id")?.to_owned(),
                token_id: required_str(member, "token_id")?.to_owned(),
                outcome_label: required_str(member, "outcome_label")?.to_owned(),
                is_placeholder: required_bool(member, "is_placeholder")?,
                is_other: required_bool(member, "is_other")?,
                neg_risk_variant: required_str(member, "neg_risk_variant")?.to_owned(),
            })
        })
        .collect()
}

struct LatestDiscoverySnapshot {
    discovered_family_count: u64,
    latest_discovery_revision: i64,
    family_ids: Vec<String>,
}

async fn latest_discovery_snapshot(
    pool: &PgPool,
) -> Result<Option<LatestDiscoverySnapshot>, NegRiskSummaryError> {
    let payload = sqlx::query_scalar(
        r#"
        SELECT payload
        FROM event_journal
        WHERE event_type = 'neg_risk_discovery_snapshot'
        ORDER BY journal_seq DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await?;

    payload
        .map(|payload| {
            let latest_discovery_revision = required_i64(&payload, "discovery_revision")?;
            let family_ids = payload
                .get("family_ids")
                .and_then(Value::as_array)
                .ok_or_else(|| NegRiskSummaryError::InvalidPayload {
                    field: "family_ids",
                    value: payload.to_string(),
                })?
                .iter()
                .map(|item| {
                    item.as_str().map(str::to_owned).ok_or_else(|| {
                        NegRiskSummaryError::InvalidPayload {
                            field: "family_ids",
                            value: item.to_string(),
                        }
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;

            Ok(LatestDiscoverySnapshot {
                discovered_family_count: family_ids.len() as u64,
                latest_discovery_revision,
                family_ids,
            })
        })
        .transpose()
}

async fn count_recent_family_events(
    pool: &PgPool,
    event_type: &str,
    latest_discovery_revision: i64,
) -> Result<u64, NegRiskSummaryError> {
    let count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM event_journal
        WHERE event_type = $1
          AND payload ->> 'discovery_revision' = $2
        "#,
    )
    .bind(event_type)
    .bind(latest_discovery_revision.to_string())
    .fetch_one(pool)
    .await?;

    Ok(count.max(0) as u64)
}

struct MemberVectorPaths {
    validation: HashMap<MemberVectorPathKey, NegRiskMemberVectorPath>,
    halt: HashMap<MemberVectorPathKey, NegRiskMemberVectorPath>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MemberVectorPathKey {
    event_family_id: String,
    metadata_snapshot_hash: Option<String>,
}

async fn latest_member_vector_paths(
    pool: &PgPool,
) -> Result<MemberVectorPaths, NegRiskSummaryError> {
    let rows = sqlx::query(
        r#"
        SELECT journal_seq, event_type, payload
        FROM event_journal
        WHERE event_type IN ('family_validation', 'family_halt')
        ORDER BY journal_seq DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut validation = HashMap::new();
    let mut halt = HashMap::new();

    for row in rows {
        let journal_seq: i64 = row
            .try_get("journal_seq")
            .map_err(NegRiskSummaryError::Sqlx)?;
        let event_type: String = row
            .try_get("event_type")
            .map_err(NegRiskSummaryError::Sqlx)?;
        let payload: Value = row.try_get("payload").map_err(NegRiskSummaryError::Sqlx)?;
        let family_id = required_str(&payload, "event_family_id")?.to_owned();
        let key = MemberVectorPathKey {
            event_family_id: family_id.clone(),
            metadata_snapshot_hash: optional_str(&payload, "metadata_snapshot_hash")
                .map(str::to_owned),
        };

        let path = NegRiskMemberVectorPath {
            journal_seq,
            event_type: event_type.clone(),
            event_family_id: family_id,
        };

        if event_type == "family_validation" {
            validation.entry(key).or_insert(path);
        } else if event_type == "family_halt" {
            halt.entry(key).or_insert(path);
        }
    }

    Ok(MemberVectorPaths { validation, halt })
}

fn required_i64(payload: &Value, field: &'static str) -> Result<i64, NegRiskSummaryError> {
    payload
        .get(field)
        .and_then(Value::as_i64)
        .ok_or_else(|| NegRiskSummaryError::InvalidPayload {
            field,
            value: payload.to_string(),
        })
}

fn required_str<'a>(
    payload: &'a Value,
    field: &'static str,
) -> Result<&'a str, NegRiskSummaryError> {
    payload
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| NegRiskSummaryError::InvalidPayload {
            field,
            value: payload.to_string(),
        })
}

fn required_bool(payload: &Value, field: &'static str) -> Result<bool, NegRiskSummaryError> {
    payload
        .get(field)
        .and_then(Value::as_bool)
        .ok_or_else(|| NegRiskSummaryError::InvalidPayload {
            field,
            value: payload.to_string(),
        })
}

fn optional_str<'a>(payload: &'a Value, field: &'static str) -> Option<&'a str> {
    payload.get(field).and_then(Value::as_str)
}
