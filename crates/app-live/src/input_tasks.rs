use chrono::{DateTime, Utc};
use domain::ExternalFactEvent;
use state::StateFactInput;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputTaskEvent {
    pub journal_seq: i64,
    pub event: ExternalFactEvent,
    pub hint: InputTaskHint,
}

impl InputTaskEvent {
    pub fn new(journal_seq: i64, event: ExternalFactEvent) -> Self {
        Self {
            journal_seq,
            event,
            hint: InputTaskHint::None,
        }
    }

    pub fn out_of_order_user_trade(journal_seq: i64, event: ExternalFactEvent) -> Self {
        Self {
            journal_seq,
            event,
            hint: InputTaskHint::OutOfOrderUserTrade,
        }
    }

    pub fn family_discovery_observed(
        journal_seq: i64,
        source_session_id: impl Into<String>,
        source_event_id: impl Into<String>,
        family_id: impl Into<String>,
        observed_at: DateTime<Utc>,
    ) -> Self {
        Self::new(
            journal_seq,
            ExternalFactEvent::family_discovery_observed(
                source_session_id,
                source_event_id,
                family_id,
                observed_at,
            ),
        )
    }

    pub fn family_backfill_observed(
        journal_seq: i64,
        source_session_id: impl Into<String>,
        source_event_id: impl Into<String>,
        family_id: impl Into<String>,
        cursor: impl Into<String>,
        complete: bool,
        observed_at: DateTime<Utc>,
    ) -> Self {
        Self::new(
            journal_seq,
            ExternalFactEvent::family_backfill_observed(
                source_session_id,
                source_event_id,
                family_id,
                cursor,
                complete,
                observed_at,
            ),
        )
    }

    pub fn into_state_fact_input(self) -> StateFactInput {
        match self.hint {
            InputTaskHint::None => StateFactInput::new(self.event),
            InputTaskHint::OutOfOrderUserTrade => {
                StateFactInput::out_of_order_user_trade(self.event)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputTaskHint {
    None,
    OutOfOrderUserTrade,
}
