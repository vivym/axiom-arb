use persistence::{Result, RunSessionRepo, RunSessionRow};
use sqlx::PgPool;

use super::{context::VerifyContext, window::VerifyWindowSelection};

#[derive(Debug, Clone, Default)]
pub struct ResolvedVerifySession {
    pub relevant_session: Option<RunSessionRow>,
    pub historical_window_session: Option<RunSessionRow>,
    pub historical_window_unique: bool,
}

impl ResolvedVerifySession {
    pub fn selected_session_id(&self, selection: &VerifyWindowSelection) -> Option<&str> {
        if selection.is_historical_explicit() {
            if self.historical_window_unique {
                return self
                    .historical_window_session
                    .as_ref()
                    .map(|row| row.run_session_id.as_str())
                    .or_else(|| {
                        self.relevant_session
                            .as_ref()
                            .map(|row| row.run_session_id.as_str())
                    });
            }

            return None;
        }

        self.relevant_session
            .as_ref()
            .map(|row| row.run_session_id.as_str())
    }
}

pub async fn resolve_session_window(
    pool: &PgPool,
    context: &VerifyContext,
    selection: &VerifyWindowSelection,
) -> Result<ResolvedVerifySession> {
    let relevant_session = match context.control_plane.run_session_id.as_deref() {
        Some(run_session_id) => RunSessionRepo.get(pool, run_session_id).await?,
        None => None,
    };

    let historical_window_session = match selection {
        VerifyWindowSelection::LatestForScenario(_) => None,
        VerifyWindowSelection::ExplicitAttemptId(attempt_id) => {
            RunSessionRepo
                .resolve_unique_for_attempt_id(pool, attempt_id)
                .await?
        }
        VerifyWindowSelection::ExplicitSince(since) => {
            RunSessionRepo
                .resolve_unique_for_since(pool, *since)
                .await?
        }
        VerifyWindowSelection::ExplicitSeqRange { from_seq, to_seq } => {
            RunSessionRepo
                .resolve_unique_for_seq_range(pool, *from_seq, *to_seq)
                .await?
        }
    };

    let historical_window_unique = match selection {
        VerifyWindowSelection::LatestForScenario(_) => true,
        _ => historical_window_session.is_some(),
    };

    Ok(ResolvedVerifySession {
        relevant_session,
        historical_window_session,
        historical_window_unique,
    })
}
