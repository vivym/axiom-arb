use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};

use app_replay::NegRiskShadowAttemptArtifacts;
use chrono::{DateTime, Utc};
use domain::ExecutionMode;
use persistence::{
    ExecutionAttemptRepo, JournalRepo, LiveArtifactRepo, LiveSubmissionRepo, Result,
    ShadowArtifactRepo,
    models::{
        ExecutionAttemptWithCreatedAtRow, JournalEntryRow, LiveExecutionArtifactRow,
        LiveSubmissionRecordRow, ShadowExecutionArtifactRow,
    },
};
use sqlx::PgPool;

use super::{session::ResolvedVerifySession, window::VerifyWindowSelection};

const DEFAULT_RECENT_ATTEMPTS_LIMIT: i64 = 20;

#[derive(Debug, Clone, Default)]
pub struct VerifyEvidenceWindow {
    pub attempts: Vec<ExecutionAttemptWithCreatedAtRow>,
    pub observed_live_attempts: Vec<ExecutionAttemptWithCreatedAtRow>,
    pub observed_shadow_attempts: Vec<ExecutionAttemptWithCreatedAtRow>,
    pub replay_shadow_attempt_artifacts: Vec<NegRiskShadowAttemptArtifacts>,
    pub journal: Vec<JournalEntryRow>,
    pub shadow_artifacts: Vec<ShadowExecutionArtifactRow>,
    pub live_artifacts: BTreeMap<String, Vec<LiveExecutionArtifactRow>>,
    pub live_submissions: BTreeMap<String, Vec<LiveSubmissionRecordRow>>,
}

pub async fn load(
    pool: &PgPool,
    selection: &VerifyWindowSelection,
    resolved_session: &ResolvedVerifySession,
    active_routes: &[String],
) -> Result<VerifyEvidenceWindow> {
    let session_id = resolved_session.selected_session_id(selection);
    let attempts = select_attempts(pool, selection, session_id, active_routes).await?;
    let observed_live_attempts =
        select_observed_live_attempts(pool, selection, session_id, &attempts, active_routes)
            .await?;
    let observed_shadow_attempts =
        select_observed_shadow_attempts(pool, selection, session_id, &attempts, active_routes)
            .await?;
    let replay_shadow_attempt_artifacts =
        load_replay_shadow_attempt_artifacts(pool, selection, &attempts).await;
    let journal = select_journal(pool, selection).await?;
    let attempt_ids = attempts
        .iter()
        .map(|row| row.attempt.attempt_id.clone())
        .chain(
            observed_live_attempts
                .iter()
                .map(|row| row.attempt.attempt_id.clone()),
        )
        .chain(
            observed_shadow_attempts
                .iter()
                .map(|row| row.attempt.attempt_id.clone()),
        )
        .collect::<Vec<_>>();

    Ok(VerifyEvidenceWindow {
        shadow_artifacts: ShadowArtifactRepo
            .list_for_attempts(pool, &attempt_ids)
            .await?,
        observed_live_attempts,
        observed_shadow_attempts,
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
    session_id: Option<&str>,
    attempts: &[ExecutionAttemptWithCreatedAtRow],
    active_routes: &[String],
) -> Result<Vec<ExecutionAttemptWithCreatedAtRow>> {
    if session_id.is_some() {
        return Ok(filter_attempts_by_active_routes(
            attempts
                .iter()
                .filter(|row| matches!(row.attempt.execution_mode, ExecutionMode::Live))
                .cloned()
                .collect(),
            active_routes,
        ));
    }

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
        VerifyWindowSelection::LatestForScenario(
            super::model::VerifyScenario::RealUserShadowSmoke,
        ) => ExecutionAttemptRepo
            .list_by_mode_with_created_at(pool, ExecutionMode::Live)
            .await
            .map(|rows| filter_attempts_by_active_routes(rows, active_routes)),
        _ => Ok(Vec::new()),
    }
}

async fn select_observed_shadow_attempts(
    pool: &PgPool,
    selection: &VerifyWindowSelection,
    session_id: Option<&str>,
    attempts: &[ExecutionAttemptWithCreatedAtRow],
    active_routes: &[String],
) -> Result<Vec<ExecutionAttemptWithCreatedAtRow>> {
    if session_id.is_some() {
        return Ok(filter_attempts_by_active_routes(
            attempts
                .iter()
                .filter(|row| matches!(row.attempt.execution_mode, ExecutionMode::Shadow))
                .cloned()
                .collect(),
            active_routes,
        ));
    }

    match selection {
        VerifyWindowSelection::LatestForScenario(super::model::VerifyScenario::Live) => {
            ExecutionAttemptRepo
                .list_recent_by_mode(
                    pool,
                    Some(ExecutionMode::Shadow),
                    DEFAULT_RECENT_ATTEMPTS_LIMIT,
                )
                .await
                .map(|rows| filter_attempts_by_active_routes(rows, active_routes))
        }
        _ => Ok(Vec::new()),
    }
}

async fn select_attempts(
    pool: &PgPool,
    selection: &VerifyWindowSelection,
    session_id: Option<&str>,
    active_routes: &[String],
) -> Result<Vec<ExecutionAttemptWithCreatedAtRow>> {
    if let Some(session_id) = session_id {
        return ExecutionAttemptRepo
            .list_by_run_session_id(pool, session_id)
            .await
            .map(|rows| filter_attempts_by_active_routes(rows, active_routes));
    }

    match selection {
        VerifyWindowSelection::LatestForScenario(scenario) => {
            select_latest_attempts_for_scenario(pool, *scenario, active_routes).await
        }
        VerifyWindowSelection::ExplicitAttemptId(attempt_id) => ExecutionAttemptRepo
            .get_by_attempt_id(pool, attempt_id)
            .await
            .map(|row| filter_attempts_by_active_routes(row.into_iter().collect(), active_routes)),
        VerifyWindowSelection::ExplicitSince(since) => ExecutionAttemptRepo
            .list_created_since(pool, *since)
            .await
            .map(|rows| filter_attempts_by_active_routes(rows, active_routes)),
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
    JournalRepo.list_since(pool, since).await
}

async fn select_latest_attempts_for_scenario(
    pool: &PgPool,
    scenario: super::model::VerifyScenario,
    active_routes: &[String],
) -> Result<Vec<ExecutionAttemptWithCreatedAtRow>> {
    match scenario {
        super::model::VerifyScenario::Paper => Ok(Vec::new()),
        super::model::VerifyScenario::Live => ExecutionAttemptRepo
            .list_recent_by_mode(
                pool,
                Some(ExecutionMode::Live),
                DEFAULT_RECENT_ATTEMPTS_LIMIT,
            )
            .await
            .map(|rows| filter_attempts_by_active_routes(rows, active_routes)),
        super::model::VerifyScenario::RealUserShadowSmoke => {
            Ok(select_latest_smoke_run_attempts(pool, active_routes)
                .await?
                .into_iter()
                .filter(|row| matches!(row.attempt.execution_mode, ExecutionMode::Shadow))
                .fold(
                    BTreeMap::<String, ExecutionAttemptWithCreatedAtRow>::new(),
                    |mut latest_by_scope, row| {
                        match latest_by_scope.entry(row.attempt.scope.clone()) {
                            Entry::Vacant(entry) => {
                                entry.insert(row);
                            }
                            Entry::Occupied(mut entry) => {
                                let existing = entry.get();
                                if row.created_at > existing.created_at
                                    || (row.created_at == existing.created_at
                                        && row.attempt.attempt_id > existing.attempt.attempt_id)
                                {
                                    entry.insert(row);
                                }
                            }
                        }
                        latest_by_scope
                    },
                )
                .into_values()
                .collect())
        }
    }
}

async fn select_latest_smoke_run_attempts(
    pool: &PgPool,
    active_routes: &[String],
) -> Result<Vec<ExecutionAttemptWithCreatedAtRow>> {
    let latest_shadow_attempt = ExecutionAttemptRepo
        .list_recent_by_mode(
            pool,
            Some(ExecutionMode::Shadow),
            DEFAULT_RECENT_ATTEMPTS_LIMIT,
        )
        .await?
        .into_iter()
        .filter(|row| route_is_active(&row.attempt.route, active_routes))
        .next();

    let Some(latest_shadow_attempt) = latest_shadow_attempt else {
        return Ok(Vec::new());
    };

    ExecutionAttemptRepo
        .list_by_snapshot_id(pool, &latest_shadow_attempt.attempt.snapshot_id)
        .await
        .map(|rows| filter_attempts_by_active_routes(rows, active_routes))
}

pub fn routes_with_evidence(
    evidence: &VerifyEvidenceWindow,
    active_routes: &[String],
) -> BTreeSet<String> {
    let mut routes = BTreeSet::new();
    for row in &evidence.attempts {
        routes.insert(row.attempt.route.clone());
    }
    for row in &evidence.observed_live_attempts {
        routes.insert(row.attempt.route.clone());
    }
    for row in &evidence.observed_shadow_attempts {
        routes.insert(row.attempt.route.clone());
    }
    for row in &evidence.replay_shadow_attempt_artifacts {
        routes.insert(row.attempt.route.clone());
    }

    routes
        .into_iter()
        .filter(|route| route_is_active(route, active_routes))
        .collect()
}

pub fn window_for_route(evidence: &VerifyEvidenceWindow, route: &str) -> VerifyEvidenceWindow {
    let attempt_ids = evidence
        .attempts
        .iter()
        .chain(evidence.observed_live_attempts.iter())
        .chain(evidence.observed_shadow_attempts.iter())
        .filter(|row| row.attempt.route == route)
        .map(|row| row.attempt.attempt_id.clone())
        .collect::<BTreeSet<_>>();

    VerifyEvidenceWindow {
        attempts: evidence
            .attempts
            .iter()
            .filter(|row| row.attempt.route == route)
            .cloned()
            .collect(),
        observed_live_attempts: evidence
            .observed_live_attempts
            .iter()
            .filter(|row| row.attempt.route == route)
            .cloned()
            .collect(),
        observed_shadow_attempts: evidence
            .observed_shadow_attempts
            .iter()
            .filter(|row| row.attempt.route == route)
            .cloned()
            .collect(),
        replay_shadow_attempt_artifacts: evidence
            .replay_shadow_attempt_artifacts
            .iter()
            .filter(|row| row.attempt.route == route)
            .cloned()
            .collect(),
        journal: evidence.journal.clone(),
        shadow_artifacts: evidence
            .shadow_artifacts
            .iter()
            .filter(|row| attempt_ids.contains(&row.attempt_id))
            .cloned()
            .collect(),
        live_artifacts: evidence
            .live_artifacts
            .iter()
            .filter(|(attempt_id, _)| attempt_ids.contains(*attempt_id))
            .map(|(attempt_id, rows)| (attempt_id.clone(), rows.clone()))
            .collect(),
        live_submissions: evidence
            .live_submissions
            .iter()
            .filter(|(_, rows)| rows.iter().any(|row| row.route == route))
            .map(|(attempt_id, rows)| {
                (
                    attempt_id.clone(),
                    rows.iter()
                        .filter(|row| row.route == route)
                        .cloned()
                        .collect(),
                )
            })
            .collect(),
    }
}

fn filter_attempts_by_active_routes(
    rows: Vec<ExecutionAttemptWithCreatedAtRow>,
    active_routes: &[String],
) -> Vec<ExecutionAttemptWithCreatedAtRow> {
    rows.into_iter()
        .filter(|row| route_is_active(&row.attempt.route, active_routes))
        .collect()
}

fn route_is_active(route: &str, active_routes: &[String]) -> bool {
    active_routes.is_empty() || active_routes.iter().any(|active| active == route)
}
