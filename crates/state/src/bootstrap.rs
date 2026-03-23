use domain::{RuntimeMode, RuntimeOverlay, RuntimePolicy};

pub fn bootstrap_policy() -> RuntimePolicy {
    RuntimePolicy {
        mode: RuntimeMode::Bootstrapping,
        overlay: Some(RuntimeOverlay::CancelOnly),
    }
}

pub fn reconcile_attention_policy() -> RuntimePolicy {
    RuntimePolicy {
        mode: RuntimeMode::Reconciling,
        overlay: Some(RuntimeOverlay::CancelOnly),
    }
}

pub fn reconciled_policy() -> RuntimePolicy {
    RuntimePolicy {
        mode: RuntimeMode::Healthy,
        overlay: None,
    }
}

pub fn allows_automatic_repair(first_reconcile_succeeded: bool, mode: RuntimeMode) -> bool {
    first_reconcile_succeeded
        && !matches!(
            mode,
            RuntimeMode::Bootstrapping | RuntimeMode::Reconciling | RuntimeMode::GlobalHalt
        )
}
