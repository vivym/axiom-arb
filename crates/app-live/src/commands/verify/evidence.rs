use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use persistence::{
    models::{
        ExecutionAttemptWithCreatedAtRow, JournalEntryRow, LiveExecutionArtifactRow,
        LiveSubmissionRecordRow, ShadowExecutionArtifactRow,
    },
    ExecutionAttemptRepo, JournalRepo, LiveArtifactRepo, LiveSubmissionRepo, Result,
    ShadowArtifactRepo,
};
use sqlx::PgPool;

use super::window::VerifyWindowSelection;

const DEFAULT_RECENT_ATTEMPTS_LIMIT: i64 = 20;

#[derive(Debug, Clone, Default)]
pub struct VerifyEvidenceWindow {
    pub attempts: Vec<ExecutionAttemptWithCreatedAtRow>,
    pub journal: Vec<JournalEntryRow>,
    pub shadow_artifacts: Vec<ShadowExecutionArtifactRow>,
    pub live_artifacts: BTreeMap<String, Vec<LiveExecutionArtifactRow>>,
    pub live_submissions: BTreeMap<String, Vec<LiveSubmissionRecordRow>>,
}

pub async fn load(
    pool: &PgPool,
    selection: &VerifyWindowSelection,
) -> Result<VerifyEvidenceWindow> {
    let attempts = select_attempts(pool, selection).await?;
    let journal = select_journal(pool, selection).await?;
    let attempt_ids = attempts
        .iter()
        .map(|row| row.attempt.attempt_id.clone())
        .collect::<Vec<_>>();

    Ok(VerifyEvidenceWindow {
        shadow_artifacts: ShadowArtifactRepo
            .list_for_attempts(pool, &attempt_ids)
            .await?,
        live_artifacts: LiveArtifactRepo
            .list_for_attempts(pool, &attempt_ids)
            .await?,
        live_submissions: LiveSubmissionRepo
            .list_for_attempts(pool, &attempt_ids)
            .await?,
        attempts,
        journal,
    })
}

async fn select_attempts(
    pool: &PgPool,
    selection: &VerifyWindowSelection,
) -> Result<Vec<ExecutionAttemptWithCreatedAtRow>> {
    match selection {
        VerifyWindowSelection::LatestForScenario => {
            ExecutionAttemptRepo
                .list_recent(pool, DEFAULT_RECENT_ATTEMPTS_LIMIT)
                .await
        }
        VerifyWindowSelection::ExplicitAttemptId(attempt_id) => ExecutionAttemptRepo
            .get_by_attempt_id(pool, attempt_id)
            .await
            .map(|row| row.into_iter().collect()),
        VerifyWindowSelection::ExplicitSince(since) => {
            ExecutionAttemptRepo.list_created_since(pool, *since).await
        }
        VerifyWindowSelection::ExplicitSeqRange { .. } => Ok(Vec::new()),
    }
}

async fn select_journal(
    pool: &PgPool,
    selection: &VerifyWindowSelection,
) -> Result<Vec<JournalEntryRow>> {
    match selection {
        VerifyWindowSelection::ExplicitSeqRange { from_seq, to_seq } => {
            JournalRepo.list_range(pool, *from_seq, *to_seq).await
        }
        VerifyWindowSelection::ExplicitSince(since) => list_journal_since(pool, *since).await,
        _ => Ok(Vec::new()),
    }
}

async fn list_journal_since(pool: &PgPool, since: DateTime<Utc>) -> Result<Vec<JournalEntryRow>> {
    let current = persistence::RuntimeProgressRepo.current(pool).await?;
    let from_seq = current.map(|row| row.last_journal_seq).unwrap_or_default();
    let rows = JournalRepo.list_range(pool, from_seq, None).await?;
    Ok(rows
        .into_iter()
        .filter(|row| row.event_ts >= since || row.ingested_at >= since)
        .collect())
}
