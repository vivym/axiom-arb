use std::collections::VecDeque;

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

#[derive(Debug, Default)]
pub struct InputTaskQueue {
    backlog: VecDeque<InputTaskEvent>,
}

impl InputTaskQueue {
    pub fn push(&mut self, input: InputTaskEvent) {
        self.backlog.push_back(input);
        self.backlog
            .make_contiguous()
            .sort_by_key(|entry| entry.journal_seq);
    }

    pub fn next_after(&self, last_journal_seq: Option<i64>) -> Option<InputTaskEvent> {
        self.backlog
            .iter()
            .find(|entry| last_journal_seq.is_none_or(|last| entry.journal_seq > last))
            .cloned()
    }

    pub fn remove(&mut self, input: &InputTaskEvent) -> Option<InputTaskEvent> {
        let index = self.backlog.iter().position(|entry| entry == input)?;
        self.backlog.remove(index)
    }

    pub fn len(&self) -> usize {
        self.backlog.len()
    }
}
