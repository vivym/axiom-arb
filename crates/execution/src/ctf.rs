use std::collections::HashMap;

use domain::ConditionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CtfOperationKind {
    Split,
    Merge,
    Redeem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CtfOperationStatus {
    Planned,
    Submitted,
    Confirmed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CtfOperation {
    pub kind: CtfOperationKind,
    pub condition_id: ConditionId,
    pub relayer_transaction_id: Option<String>,
    pub nonce: Option<String>,
    pub tx_hash: Option<String>,
    pub status: CtfOperationStatus,
    attempt_id: Option<String>,
}

impl CtfOperation {
    pub fn new(
        kind: CtfOperationKind,
        condition_id: ConditionId,
        relayer_transaction_id: Option<String>,
        nonce: Option<String>,
        status: CtfOperationStatus,
    ) -> Self {
        Self {
            kind,
            condition_id,
            relayer_transaction_id,
            nonce,
            tx_hash: None,
            status,
            attempt_id: None,
        }
    }

    pub fn with_attempt_id(mut self, attempt_id: impl Into<String>) -> Self {
        self.attempt_id = Some(attempt_id.into());
        self
    }

    pub fn with_attempt_context(self, attempt: &domain::ExecutionAttemptContext) -> Self {
        self.with_attempt_id(attempt.attempt_id.clone())
    }

    pub fn attempt_id(&self) -> Option<&str> {
        self.attempt_id.as_deref()
    }
}

pub type CtfOperationId = usize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CtfTrackerError {
    OperationNotFound {
        operation_id: CtfOperationId,
    },
    SubmittedRequiresRelayerMetadata,
    ConfirmedRequiresTxHash,
    RelayerMetadataFrozen {
        status: CtfOperationStatus,
    },
    IllegalStatusTransition {
        from: CtfOperationStatus,
        to: CtfOperationStatus,
    },
    RelayerTransactionIdConflict {
        relayer_transaction_id: String,
    },
    RelayerTransactionIdAlreadyBound,
    NonceAlreadyBound {
        existing_nonce: String,
        new_nonce: String,
    },
}

#[derive(Debug, Default)]
pub struct CtfTracker {
    operations: HashMap<CtfOperationId, CtfOperation>,
    relayer_index: HashMap<String, CtfOperationId>,
    next_operation_id: CtfOperationId,
}

impl CtfTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, operation: CtfOperation) -> Result<CtfOperationId, CtfTrackerError> {
        Self::validate_operation_for_status(&operation.status, &operation)?;
        self.ensure_relayer_id_available(operation.relayer_transaction_id.as_deref(), None)?;

        let operation_id = self.next_operation_id;
        self.next_operation_id += 1;

        if let Some(relayer_transaction_id) = operation.relayer_transaction_id.clone() {
            self.relayer_index
                .insert(relayer_transaction_id, operation_id);
        }

        self.operations.insert(operation_id, operation);
        Ok(operation_id)
    }

    pub fn attach_relayer_metadata(
        &mut self,
        operation_id: CtfOperationId,
        relayer_transaction_id: Option<String>,
        nonce: Option<String>,
    ) -> Result<(), CtfTrackerError> {
        let Some(existing_operation) = self.operations.get(&operation_id) else {
            return Err(CtfTrackerError::OperationNotFound { operation_id });
        };

        if existing_operation.status != CtfOperationStatus::Planned {
            return Err(CtfTrackerError::RelayerMetadataFrozen {
                status: existing_operation.status,
            });
        }

        if let Some(ref relayer_transaction_id) = relayer_transaction_id {
            if let Some(existing) = existing_operation.relayer_transaction_id.as_deref() {
                if existing != relayer_transaction_id {
                    return Err(CtfTrackerError::RelayerTransactionIdAlreadyBound);
                }
            }

            self.ensure_relayer_id_available(Some(relayer_transaction_id), Some(operation_id))?;
        }

        if let Some(ref nonce) = nonce {
            if let Some(existing_nonce) = existing_operation.nonce.as_ref() {
                if existing_nonce != nonce {
                    return Err(CtfTrackerError::NonceAlreadyBound {
                        existing_nonce: existing_nonce.clone(),
                        new_nonce: nonce.clone(),
                    });
                }
            }
        }

        let operation = self
            .operations
            .get_mut(&operation_id)
            .expect("operation exists after validation");

        if let Some(relayer_transaction_id) = relayer_transaction_id {
            self.relayer_index
                .insert(relayer_transaction_id.clone(), operation_id);
            operation.relayer_transaction_id = Some(relayer_transaction_id);
        }

        if let Some(nonce) = nonce {
            if operation.nonce.is_none() {
                operation.nonce = Some(nonce);
            }
        }

        Ok(())
    }

    pub fn update_status(
        &mut self,
        operation_id: CtfOperationId,
        status: CtfOperationStatus,
        tx_hash: Option<String>,
    ) -> Result<(), CtfTrackerError> {
        let Some(existing_operation) = self.operations.get(&operation_id) else {
            return Err(CtfTrackerError::OperationNotFound { operation_id });
        };

        Self::validate_transition(existing_operation.status, status)?;

        let mut candidate = existing_operation.clone();
        candidate.status = status;

        if let Some(tx_hash) = tx_hash.clone() {
            candidate.tx_hash = Some(tx_hash);
        }

        Self::validate_operation_for_status(&status, &candidate)?;

        let operation = self
            .operations
            .get_mut(&operation_id)
            .expect("operation exists after validation");

        operation.status = status;

        if let Some(tx_hash) = tx_hash {
            operation.tx_hash = Some(tx_hash);
        }

        Ok(())
    }

    pub fn operation(&self, operation_id: CtfOperationId) -> Option<&CtfOperation> {
        self.operations.get(&operation_id)
    }

    pub fn operation_by_relayer_transaction_id(
        &self,
        relayer_transaction_id: &str,
    ) -> Option<&CtfOperation> {
        self.relayer_index
            .get(relayer_transaction_id)
            .and_then(|operation_id| self.operations.get(operation_id))
    }

    fn validate_transition(
        from: CtfOperationStatus,
        to: CtfOperationStatus,
    ) -> Result<(), CtfTrackerError> {
        if to == CtfOperationStatus::Planned && from != CtfOperationStatus::Planned {
            return Err(CtfTrackerError::IllegalStatusTransition { from, to });
        }

        Ok(())
    }

    fn validate_operation_for_status(
        status: &CtfOperationStatus,
        operation: &CtfOperation,
    ) -> Result<(), CtfTrackerError> {
        if matches!(
            status,
            CtfOperationStatus::Submitted | CtfOperationStatus::Confirmed
        ) && (operation.relayer_transaction_id.is_none() || operation.nonce.is_none())
        {
            return Err(CtfTrackerError::SubmittedRequiresRelayerMetadata);
        }

        if *status == CtfOperationStatus::Confirmed && operation.tx_hash.is_none() {
            return Err(CtfTrackerError::ConfirmedRequiresTxHash);
        }

        Ok(())
    }

    fn ensure_relayer_id_available(
        &self,
        relayer_transaction_id: Option<&str>,
        operation_id: Option<CtfOperationId>,
    ) -> Result<(), CtfTrackerError> {
        let Some(relayer_transaction_id) = relayer_transaction_id else {
            return Ok(());
        };

        if let Some(existing_operation_id) = self.relayer_index.get(relayer_transaction_id) {
            if Some(*existing_operation_id) != operation_id {
                return Err(CtfTrackerError::RelayerTransactionIdConflict {
                    relayer_transaction_id: relayer_transaction_id.to_owned(),
                });
            }
        }

        Ok(())
    }
}
