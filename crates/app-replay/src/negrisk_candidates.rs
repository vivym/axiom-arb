use persistence::{
    models::{AdoptableTargetRevisionRow, CandidateAdoptionProvenanceRow, CandidateTargetSetRow},
    PersistenceError,
};
use sqlx::{PgPool, Row};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NegRiskCandidateSummary {
    pub candidate_target_set_count: u64,
    pub adoptable_target_revision_count: u64,
    pub adoption_provenance_count: u64,
    pub latest_candidate_revision: Option<String>,
    pub latest_adoptable_revision: Option<String>,
    pub operator_target_revision: Option<String>,
}

pub async fn load_negrisk_candidate_target_sets(
    pool: &PgPool,
) -> Result<Vec<CandidateTargetSetRow>, PersistenceError> {
    let rows = sqlx::query(
        r#"
        SELECT candidate_revision, snapshot_id, source_revision, payload
        FROM candidate_target_sets
        ORDER BY candidate_revision ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(CandidateTargetSetRow {
                candidate_revision: row.try_get("candidate_revision")?,
                snapshot_id: row.try_get("snapshot_id")?,
                source_revision: row.try_get("source_revision")?,
                payload: row.try_get("payload")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(Into::into)
}

pub async fn load_negrisk_adoptable_target_revisions(
    pool: &PgPool,
) -> Result<Vec<AdoptableTargetRevisionRow>, PersistenceError> {
    let rows = sqlx::query(
        r#"
        SELECT
            adoptable_revision,
            candidate_revision,
            rendered_operator_target_revision,
            payload
        FROM adoptable_target_revisions
        ORDER BY adoptable_revision ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(AdoptableTargetRevisionRow {
                adoptable_revision: row.try_get("adoptable_revision")?,
                candidate_revision: row.try_get("candidate_revision")?,
                rendered_operator_target_revision: row
                    .try_get("rendered_operator_target_revision")?,
                payload: row.try_get("payload")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(Into::into)
}

pub async fn load_negrisk_candidate_adoption_provenance(
    pool: &PgPool,
) -> Result<Vec<CandidateAdoptionProvenanceRow>, PersistenceError> {
    let rows = sqlx::query(
        r#"
        SELECT operator_target_revision, adoptable_revision, candidate_revision
        FROM candidate_adoption_provenance
        ORDER BY operator_target_revision ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(CandidateAdoptionProvenanceRow {
                operator_target_revision: row.try_get("operator_target_revision")?,
                adoptable_revision: row.try_get("adoptable_revision")?,
                candidate_revision: row.try_get("candidate_revision")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(Into::into)
}

pub async fn load_negrisk_candidate_summary(
    pool: &PgPool,
) -> Result<NegRiskCandidateSummary, PersistenceError> {
    let candidate_target_sets = load_negrisk_candidate_target_sets(pool).await?;
    let adoptable_target_revisions = load_negrisk_adoptable_target_revisions(pool).await?;
    let adoption_provenance = load_negrisk_candidate_adoption_provenance(pool).await?;

    Ok(NegRiskCandidateSummary {
        candidate_target_set_count: candidate_target_sets.len() as u64,
        adoptable_target_revision_count: adoptable_target_revisions.len() as u64,
        adoption_provenance_count: adoption_provenance.len() as u64,
        latest_candidate_revision: candidate_target_sets
            .last()
            .map(|row| row.candidate_revision.clone()),
        latest_adoptable_revision: adoptable_target_revisions
            .last()
            .map(|row| row.adoptable_revision.clone()),
        operator_target_revision: adoption_provenance
            .last()
            .map(|row| row.operator_target_revision.clone()),
    })
}
