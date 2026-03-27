use crate::{
    facts::{DirtyDomain, DirtySet, FactApplyHint, PendingRef, StateFactInput},
    store::{JournalConsumption, StateStore},
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
        fact: impl Into<StateFactInput>,
    ) -> Result<ApplyResult, ApplyError> {
        let fact = fact.into();
        let fact_key = fact.fact_key();

        match self.store.consume_journal_seq(journal_seq, &fact_key) {
            JournalConsumption::Consumed | JournalConsumption::AlreadyBoundToSameFact => {}
            JournalConsumption::AlreadyBoundToDifferentFact => {
                return Err(ApplyError::new(format!(
                    "journal sequence {journal_seq} is already bound to a different fact"
                )));
            }
            JournalConsumption::OutOfOrder => {
                return Err(ApplyError::new(format!(
                    "journal sequence {journal_seq} must be greater than last consumed sequence {}",
                    self.store
                        .last_consumed_journal_seq()
                        .expect("out-of-order journal consumption requires a consumed sequence")
                )));
            }
        }

        if let Some(duplicate_of_journal_seq) = self.store.duplicate_journal_seq(&fact_key) {
            return Ok(ApplyResult::Duplicate {
                journal_seq,
                duplicate_of_journal_seq,
                state_version: self.store.state_version(),
            });
        }

        match fact.apply_hint() {
            FactApplyHint::None => {}
            FactApplyHint::FamilyDiscovery { record } => {
                self.store.record_family_discovery(record.clone());
                let state_version = self.store.record_applied_fact(journal_seq, fact_key);
                return Ok(ApplyResult::Applied {
                    journal_seq,
                    state_version,
                    dirty_set: DirtySet::new([DirtyDomain::Candidates]),
                });
            }
            FactApplyHint::FamilyBackfill {
                family_id,
                cursor,
                completed_at,
            } => {
                self.store
                    .record_family_backfill(family_id, cursor.clone(), *completed_at);
                let state_version = self.store.record_applied_fact(journal_seq, fact_key);
                return Ok(ApplyResult::Applied {
                    journal_seq,
                    state_version,
                    dirty_set: DirtySet::new([DirtyDomain::Candidates]),
                });
            }
            FactApplyHint::PendingReconcile { anchor } => {
                self.store.record_pending_reconcile(anchor.clone());
                self.store.mark_reconcile_required();

                return Ok(ApplyResult::ReconcileRequired {
                    journal_seq,
                    pending_ref: Some(PendingRef(anchor.pending_ref.clone())),
                    reason: anchor.reason.clone(),
                });
            }
            FactApplyHint::RuntimeAttention { anchor } => {
                self.store.record_runtime_attention(anchor.clone());
                let state_version = self.store.record_applied_fact(journal_seq, fact_key);
                return Ok(ApplyResult::Applied {
                    journal_seq,
                    state_version,
                    dirty_set: DirtySet::new([DirtyDomain::Runtime, DirtyDomain::NegRiskFamilies]),
                });
            }
            FactApplyHint::LiveReconcileObserved {
                pending_ref,
                terminal,
            } => {
                if *terminal {
                    let _ = self.store.clear_pending_reconcile(pending_ref);
                    let state_version = self.store.record_applied_fact(journal_seq, fact_key);
                    return Ok(ApplyResult::Applied {
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
                    });
                }

                self.store.mark_reconcile_required();
                return Ok(ApplyResult::ReconcileRequired {
                    journal_seq,
                    pending_ref: Some(pending_ref.clone()),
                    reason: "live reconcile remains pending".to_owned(),
                });
            }
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
}
