use domain::{OrderId, SignedOrderIdentity};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryKind {
    Initial,
    Transport,
    Business,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusinessRetryError {
    NonceUnchanged,
    IdentityNonceMismatch,
    OrderIdReused,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedOrderEnvelope {
    pub order_id: OrderId,
    pub identity: SignedOrderIdentity,
    pub retry_kind: RetryKind,
    pub retry_of_order_id: Option<OrderId>,
    attempt_id: Option<String>,
    used_order_ids: Vec<OrderId>,
}

impl SignedOrderEnvelope {
    pub fn new(order_id: OrderId, identity: SignedOrderIdentity) -> Self {
        let used_order_ids = vec![order_id.clone()];
        Self {
            order_id,
            identity,
            retry_kind: RetryKind::Initial,
            retry_of_order_id: None,
            attempt_id: None,
            used_order_ids,
        }
    }

    pub fn with_attempt_id(mut self, attempt_id: impl Into<String>) -> Self {
        self.attempt_id = Some(attempt_id.into());
        self
    }

    pub fn attempt_id(&self) -> Option<&str> {
        self.attempt_id.as_deref()
    }

    pub fn with_attempt_context(self, attempt: &domain::ExecutionAttemptContext) -> Self {
        self.with_attempt_id(attempt.attempt_id.clone())
    }

    pub fn transport_retry(&self) -> Self {
        Self {
            order_id: self.order_id.clone(),
            identity: self.identity.clone(),
            retry_kind: RetryKind::Transport,
            retry_of_order_id: self.retry_of_order_id.clone(),
            attempt_id: self.attempt_id.clone(),
            used_order_ids: self.used_order_ids.clone(),
        }
    }

    pub fn business_retry(
        &self,
        order_id: OrderId,
        new_nonce: String,
        identity: SignedOrderIdentity,
    ) -> Result<Self, BusinessRetryError> {
        let next_order_id = order_id.clone();
        if self.used_order_ids.iter().any(|used| used == &order_id) {
            return Err(BusinessRetryError::OrderIdReused);
        }

        if new_nonce == self.identity.nonce {
            return Err(BusinessRetryError::NonceUnchanged);
        }

        if identity.nonce != new_nonce {
            return Err(BusinessRetryError::IdentityNonceMismatch);
        }

        Ok(Self {
            order_id,
            identity,
            retry_kind: RetryKind::Business,
            retry_of_order_id: Some(
                self.retry_of_order_id
                    .clone()
                    .unwrap_or_else(|| self.order_id.clone()),
            ),
            attempt_id: self.attempt_id.clone(),
            used_order_ids: {
                let mut used_order_ids = self.used_order_ids.clone();
                used_order_ids.push(next_order_id);
                used_order_ids
            },
        })
    }
}
