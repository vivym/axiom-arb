use persistence::{
    models::{AdoptableTargetRevisionRow, CandidateAdoptionProvenanceRow, CandidateTargetSetRow},
    CandidateAdoptionRepo, CandidateArtifactRepo, PersistenceError, RuntimeProgressRepo,
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

fn fail_closed_candidate_summary(
    candidate_target_sets: &[CandidateTargetSetRow],
    adoptable_target_revisions: &[AdoptableTargetRevisionRow],
    adoption_provenance: &[CandidateAdoptionProvenanceRow],
) -> NegRiskCandidateSummary {
    NegRiskCandidateSummary {
        candidate_target_set_count: candidate_target_sets.len() as u64,
        adoptable_target_revision_count: adoptable_target_revisions.len() as u64,
        adoption_provenance_count: adoption_provenance.len() as u64,
        latest_candidate_revision: None,
        latest_adoptable_revision: None,
        operator_target_revision: None,
    }
}

pub fn summarize_negrisk_candidate_chain(
    candidate_target_sets: &[CandidateTargetSetRow],
    adoptable_target_revisions: &[AdoptableTargetRevisionRow],
    adoption_provenance: &[CandidateAdoptionProvenanceRow],
) -> NegRiskCandidateSummary {
    let (latest_candidate_revision, latest_adoptable_revision, operator_target_revision) =
        if candidate_target_sets.len() == 1 {
            let candidate = &candidate_target_sets[0];
            let adoptable = if adoptable_target_revisions.len() == 1
                && adoptable_target_revisions[0].candidate_revision == candidate.candidate_revision
            {
                Some(&adoptable_target_revisions[0])
            } else {
                None
            };
            let operator_target_revision = match adoptable {
                Some(adoptable) if adoption_provenance.len() == 1 => {
                    let provenance = &adoption_provenance[0];
                    if provenance.candidate_revision == candidate.candidate_revision
                        && provenance.adoptable_revision == adoptable.adoptable_revision
                        && adoptable.rendered_operator_target_revision
                            == provenance.operator_target_revision
                    {
                        Some(provenance.operator_target_revision.clone())
                    } else {
                        None
                    }
                }
                _ => None,
            };

            (
                Some(candidate.candidate_revision.clone()),
                adoptable.map(|row| row.adoptable_revision.clone()),
                operator_target_revision,
            )
        } else {
            (None, None, None)
        };

    NegRiskCandidateSummary {
        candidate_target_set_count: candidate_target_sets.len() as u64,
        adoptable_target_revision_count: adoptable_target_revisions.len() as u64,
        adoption_provenance_count: adoption_provenance.len() as u64,
        latest_candidate_revision,
        latest_adoptable_revision,
        operator_target_revision,
    }
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

    if let Some(operator_target_revision) = RuntimeProgressRepo
        .current(pool)
        .await?
        .and_then(|progress| progress.operator_target_revision)
    {
        if let Some(provenance) = CandidateAdoptionRepo
            .get_by_operator_target_revision(pool, &operator_target_revision)
            .await?
        {
            let artifacts = CandidateArtifactRepo;
            let candidate = artifacts
                .get_candidate_target_set(pool, &provenance.candidate_revision)
                .await?;
            let adoptable = artifacts
                .get_adoptable_target_revision(pool, &provenance.adoptable_revision)
                .await?;

            if let (Some(candidate), Some(adoptable)) = (candidate, adoptable) {
                return Ok(NegRiskCandidateSummary {
                    candidate_target_set_count: candidate_target_sets.len() as u64,
                    adoptable_target_revision_count: adoptable_target_revisions.len() as u64,
                    adoption_provenance_count: adoption_provenance.len() as u64,
                    latest_candidate_revision: Some(candidate.candidate_revision),
                    latest_adoptable_revision: Some(adoptable.adoptable_revision),
                    operator_target_revision: Some(operator_target_revision),
                });
            }
        }

        return Ok(fail_closed_candidate_summary(
            &candidate_target_sets,
            &adoptable_target_revisions,
            &adoption_provenance,
        ));
    }

    Ok(summarize_negrisk_candidate_chain(
        &candidate_target_sets,
        &adoptable_target_revisions,
        &adoption_provenance,
    ))
}
