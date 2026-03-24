use domain::{OrderId, SignedOrderIdentity};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryKind {
    Initial,
    Transport,
    Business,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusinessRetryError {
    IdentityUnchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedOrderEnvelope {
    pub order_id: OrderId,
    pub identity: SignedOrderIdentity,
    pub retry_kind: RetryKind,
    pub retry_of_order_id: Option<OrderId>,
}

impl SignedOrderEnvelope {
    pub fn new(order_id: OrderId, identity: SignedOrderIdentity) -> Self {
        Self {
            order_id,
            identity,
            retry_kind: RetryKind::Initial,
            retry_of_order_id: None,
        }
    }

    pub fn transport_retry(&self) -> Self {
        Self {
            order_id: self.order_id.clone(),
            identity: self.identity.clone(),
            retry_kind: RetryKind::Transport,
            retry_of_order_id: self.retry_of_order_id.clone(),
        }
    }

    pub fn business_retry(
        &self,
        order_id: OrderId,
        identity: SignedOrderIdentity,
    ) -> Result<Self, BusinessRetryError> {
        if identity == self.identity {
            return Err(BusinessRetryError::IdentityUnchanged);
        }

        Ok(Self {
            order_id,
            identity,
            retry_kind: RetryKind::Business,
            retry_of_order_id: Some(self.order_id.clone()),
        })
    }
}
