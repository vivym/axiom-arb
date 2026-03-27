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
