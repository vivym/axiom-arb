use std::collections::BTreeMap;

use app_replay::NegRiskShadowAttemptArtifacts;
use chrono::{DateTime, Utc};
use domain::ExecutionMode;
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
    pub observed_live_attempts: Vec<ExecutionAttemptWithCreatedAtRow>,
    pub replay_shadow_attempt_artifacts: Vec<NegRiskShadowAttemptArtifacts>,
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
    let observed_live_attempts = select_observed_live_attempts(pool, selection).await?;
    let replay_shadow_attempt_artifacts =
        load_replay_shadow_attempt_artifacts(pool, selection, &attempts).await;
    let journal = select_journal(pool, selection).await?;
    let attempt_ids = attempts
        .iter()
        .map(|row| row.attempt.attempt_id.clone())
        .collect::<Vec<_>>();

    Ok(VerifyEvidenceWindow {
        shadow_artifacts: ShadowArtifactRepo
            .list_for_attempts(pool, &attempt_ids)
            .await?,
        observed_live_attempts,
        replay_shadow_attempt_artifacts,
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

async fn load_replay_shadow_attempt_artifacts(
    pool: &PgPool,
    selection: &VerifyWindowSelection,
    attempts: &[ExecutionAttemptWithCreatedAtRow],
) -> Vec<NegRiskShadowAttemptArtifacts> {
    let Ok(rows) = app_replay::load_negrisk_shadow_attempt_artifacts(pool).await else {
        return Vec::new();
    };

    if attempts.is_empty() {
        return match selection {
            VerifyWindowSelection::LatestForScenario(
                super::model::VerifyScenario::RealUserShadowSmoke,
            ) => rows,
            _ => Vec::new(),
        };
    }

    let attempt_ids = attempts
        .iter()
        .map(|row| row.attempt.attempt_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();

    rows.into_iter()
        .filter(|row| attempt_ids.contains(row.attempt.attempt_id.as_str()))
        .collect()
}

async fn select_observed_live_attempts(
    pool: &PgPool,
    selection: &VerifyWindowSelection,
) -> Result<Vec<ExecutionAttemptWithCreatedAtRow>> {
    match selection {
        VerifyWindowSelection::LatestForScenario(super::model::VerifyScenario::Paper) => {
            ExecutionAttemptRepo
                .list_recent_by_mode(
                    pool,
                    Some(ExecutionMode::Live),
                    DEFAULT_RECENT_ATTEMPTS_LIMIT,
                )
                .await
        }
        _ => Ok(Vec::new()),
    }
}

async fn select_attempts(
    pool: &PgPool,
    selection: &VerifyWindowSelection,
) -> Result<Vec<ExecutionAttemptWithCreatedAtRow>> {
    match selection {
        VerifyWindowSelection::LatestForScenario(scenario) => {
            select_latest_attempts_for_scenario(pool, *scenario).await
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
    let rows = JournalRepo.list_range(pool, 0, None).await?;
    Ok(rows
        .into_iter()
        .filter(|row| row.event_ts >= since || row.ingested_at >= since)
        .collect())
}

async fn select_latest_attempts_for_scenario(
    pool: &PgPool,
    scenario: super::model::VerifyScenario,
) -> Result<Vec<ExecutionAttemptWithCreatedAtRow>> {
    match scenario {
        super::model::VerifyScenario::Paper => Ok(Vec::new()),
        super::model::VerifyScenario::Live => {
            ExecutionAttemptRepo
                .list_recent_by_mode(
                    pool,
                    Some(ExecutionMode::Live),
                    DEFAULT_RECENT_ATTEMPTS_LIMIT,
                )
                .await
        }
        super::model::VerifyScenario::RealUserShadowSmoke => {
            ExecutionAttemptRepo
                .list_recent_by_mode(
                    pool,
                    Some(ExecutionMode::Shadow),
                    DEFAULT_RECENT_ATTEMPTS_LIMIT,
                )
                .await
        }
    }
}
