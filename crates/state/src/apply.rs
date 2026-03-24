use domain::ExternalFactEvent;

use crate::{
    facts::{DirtyDomain, DirtySet, FactKey, PendingRef},
    store::StateStore,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyResult {
    Applied {
        journal_seq: i64,
        state_version: u64,
        dirty_set: DirtySet,
    },
    Duplicate {
        journal_seq: i64,
        duplicate_of_journal_seq: i64,
        state_version: u64,
    },
    Deferred {
        journal_seq: i64,
        pending_ref: PendingRef,
        reason: String,
    },
    ReconcileRequired {
        journal_seq: i64,
        pending_ref: Option<PendingRef>,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyError {
    message: String,
}

impl ApplyError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ApplyError {}

pub struct StateApplier<'a> {
    store: &'a mut StateStore,
}

impl<'a> StateApplier<'a> {
    pub fn new(store: &'a mut StateStore) -> Self {
        Self { store }
    }

    pub fn apply(
        &mut self,
        journal_seq: i64,
        event: ExternalFactEvent,
    ) -> Result<ApplyResult, ApplyError> {
        self.ensure_journal_seq_is_monotonic(journal_seq)?;

        let fact_key = FactKey::from_event(&event);

        if let Some(duplicate_of_journal_seq) = self.store.duplicate_journal_seq(&fact_key) {
            return Ok(ApplyResult::Duplicate {
                journal_seq,
                duplicate_of_journal_seq,
                state_version: self.store.state_version(),
            });
        }

        if is_out_of_order_user_trade(&event) {
            let pending_ref = PendingRef(format!(
                "pending:{}:{}:{}",
                event.source_kind, event.source_session_id, event.source_event_id
            ));
            self.store.record_pending_ref(pending_ref.clone());

            return Ok(ApplyResult::ReconcileRequired {
                journal_seq,
                pending_ref: Some(pending_ref),
                reason: "user trade arrived out of authoritative order".to_owned(),
            });
        }

        let state_version = self.store.record_applied_fact(journal_seq, fact_key);

        Ok(ApplyResult::Applied {
            journal_seq,
            state_version,
            dirty_set: DirtySet::new([
                DirtyDomain::Runtime,
                DirtyDomain::Orders,
                DirtyDomain::Inventory,
                DirtyDomain::Approvals,
                DirtyDomain::Resolution,
                DirtyDomain::Relayer,
                DirtyDomain::NegRiskFamilies,
            ]),
        })
    }

    fn ensure_journal_seq_is_monotonic(&self, journal_seq: i64) -> Result<(), ApplyError> {
        if let Some(last_applied_journal_seq) = self.store.last_applied_journal_seq() {
            if journal_seq <= last_applied_journal_seq {
                return Err(ApplyError::new(format!(
                    "journal sequence {journal_seq} must be greater than last applied sequence {last_applied_journal_seq}"
                )));
            }
        }

        Ok(())
    }
}

fn is_out_of_order_user_trade(event: &ExternalFactEvent) -> bool {
    event.source_kind == "user_trade_out_of_order"
}
