use domain::{OrderId, RuntimeMode, RuntimeOverlay, RuntimePolicy, SignedOrderIdentity};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryClass {
    Transport,
    Business,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusinessErrorKind {
    DuplicateSignedOrder,
    MalformedPayload,
    MinSize,
    TickSize,
    InsufficientBalance,
    InsufficientAllowance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HttpRetryContext {
    pub persistent_rate_limit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryDecision {
    pub class: RetryClass,
    pub reuse_payload: bool,
    pub backoff: bool,
    pub next_mode: Option<RuntimeMode>,
    pub overlay: Option<RuntimeOverlay>,
    pub reconcile_first: bool,
    pub retry_of_order_id: Option<OrderId>,
    pub preserved_identity: Option<SignedOrderIdentity>,
}

impl RetryDecision {
    pub fn for_transport_timeout(identity: &SignedOrderIdentity) -> Self {
        Self {
            class: RetryClass::Transport,
            reuse_payload: true,
            backoff: true,
            next_mode: Some(RuntimeMode::Reconciling),
            overlay: None,
            reconcile_first: false,
            retry_of_order_id: None,
            preserved_identity: Some(identity.clone()),
        }
    }

    pub fn for_http_status(
        status_code: u16,
        restriction: Option<&str>,
        identity: Option<&SignedOrderIdentity>,
    ) -> Self {
        Self::for_http_status_with_context(
            status_code,
            restriction,
            identity,
            HttpRetryContext::default(),
        )
    }

    pub fn for_http_status_with_context(
        status_code: u16,
        restriction: Option<&str>,
        identity: Option<&SignedOrderIdentity>,
        context: HttpRetryContext,
    ) -> Self {
        match status_code {
            425 => Self {
                class: RetryClass::Transport,
                reuse_payload: true,
                backoff: true,
                next_mode: Some(RuntimeMode::Reconciling),
                overlay: None,
                reconcile_first: true,
                retry_of_order_id: None,
                preserved_identity: identity.cloned(),
            },
            429 => Self {
                class: RetryClass::Transport,
                reuse_payload: true,
                backoff: true,
                next_mode: context
                    .persistent_rate_limit
                    .then_some(RuntimeMode::Degraded),
                overlay: None,
                reconcile_first: false,
                retry_of_order_id: None,
                preserved_identity: identity.cloned(),
            },
            500 => Self {
                class: RetryClass::Transport,
                reuse_payload: true,
                backoff: true,
                next_mode: Some(RuntimeMode::Degraded),
                overlay: None,
                reconcile_first: false,
                retry_of_order_id: None,
                preserved_identity: identity.cloned(),
            },
            503 => {
                let policy = map_venue_status(status_code, restriction);
                Self {
                    class: RetryClass::None,
                    reuse_payload: false,
                    backoff: false,
                    next_mode: Some(policy.mode),
                    overlay: policy.overlay,
                    reconcile_first: false,
                    retry_of_order_id: None,
                    preserved_identity: identity.cloned(),
                }
            }
            _ => Self {
                class: RetryClass::None,
                reuse_payload: false,
                backoff: false,
                next_mode: None,
                overlay: None,
                reconcile_first: false,
                retry_of_order_id: None,
                preserved_identity: identity.cloned(),
            },
        }
    }

    pub fn for_duplicate_signed_order(order_id: OrderId, identity: &SignedOrderIdentity) -> Self {
        Self::for_business_error(
            BusinessErrorKind::DuplicateSignedOrder,
            Some(order_id),
            Some(identity),
        )
    }

    pub fn for_business_retry(order_id: OrderId) -> Self {
        Self {
            class: RetryClass::Business,
            reuse_payload: false,
            backoff: false,
            next_mode: Some(RuntimeMode::NoNewRisk),
            overlay: None,
            reconcile_first: false,
            retry_of_order_id: Some(order_id),
            preserved_identity: None,
        }
    }

    pub fn for_business_error(
        error_kind: BusinessErrorKind,
        order_id: Option<OrderId>,
        identity: Option<&SignedOrderIdentity>,
    ) -> Self {
        match error_kind {
            BusinessErrorKind::DuplicateSignedOrder => Self {
                class: RetryClass::Business,
                reuse_payload: false,
                backoff: false,
                next_mode: Some(RuntimeMode::Reconciling),
                overlay: None,
                reconcile_first: true,
                retry_of_order_id: order_id,
                preserved_identity: identity.cloned(),
            },
            BusinessErrorKind::MalformedPayload
            | BusinessErrorKind::MinSize
            | BusinessErrorKind::TickSize => Self {
                class: RetryClass::None,
                reuse_payload: false,
                backoff: false,
                next_mode: None,
                overlay: None,
                reconcile_first: false,
                retry_of_order_id: None,
                preserved_identity: identity.cloned(),
            },
            BusinessErrorKind::InsufficientBalance | BusinessErrorKind::InsufficientAllowance => {
                Self {
                    class: RetryClass::None,
                    reuse_payload: false,
                    backoff: false,
                    next_mode: Some(RuntimeMode::NoNewRisk),
                    overlay: None,
                    reconcile_first: true,
                    retry_of_order_id: None,
                    preserved_identity: identity.cloned(),
                }
            }
        }
    }
}

pub fn map_venue_status(status_code: u16, restriction: Option<&str>) -> RuntimePolicy {
    match status_code {
        503 if is_cancel_only(restriction) => RuntimePolicy {
            mode: RuntimeMode::NoNewRisk,
            overlay: Some(RuntimeOverlay::CancelOnly),
        },
        503 => RuntimePolicy {
            mode: RuntimeMode::GlobalHalt,
            overlay: None,
        },
        _ => RuntimePolicy {
            mode: RuntimeMode::Healthy,
            overlay: None,
        },
    }
}

fn is_cancel_only(restriction: Option<&str>) -> bool {
    restriction
        .map(|value| value.trim().to_ascii_lowercase())
        .is_some_and(|value| matches!(value.as_str(), "cancel-only" | "cancel_only"))
}
