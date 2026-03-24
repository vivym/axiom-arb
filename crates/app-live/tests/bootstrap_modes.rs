use app_live::{AppRuntime, AppRuntimeMode};
use domain::{ConditionId, RuntimeMode, RuntimeOverlay, TokenId};
use state::{ReconcileAttention, RemoteSnapshot};

#[test]
fn app_stays_in_bootstrap_cancel_only_without_successful_reconcile() {
    let runtime = AppRuntime::new(AppRuntimeMode::Paper);

    assert_eq!(runtime.runtime_mode(), RuntimeMode::Bootstrapping);
    assert_eq!(runtime.runtime_overlay(), Some(RuntimeOverlay::CancelOnly));
}

#[test]
fn app_leaves_bootstrap_only_after_successful_reconcile() {
    let mut runtime = AppRuntime::new(AppRuntimeMode::Paper);

    let report = runtime.reconcile(RemoteSnapshot::empty());
    assert!(report.succeeded);
    assert_eq!(runtime.runtime_mode(), RuntimeMode::Healthy);
    assert_eq!(runtime.runtime_overlay(), None);
}

#[test]
fn app_failed_reconcile_keeps_cancel_only_until_first_success() {
    let mut runtime = AppRuntime::new(AppRuntimeMode::Live);
    let report = runtime.reconcile(RemoteSnapshot::empty().with_attention(
        ReconcileAttention::IdentifierMismatch {
            token_id: TokenId::from("token-yes"),
            expected_condition_id: ConditionId::from("condition-a"),
            remote_condition_id: ConditionId::from("condition-b"),
        },
    ));

    assert!(!report.succeeded);
    assert_eq!(runtime.runtime_mode(), RuntimeMode::Reconciling);
    assert_eq!(runtime.runtime_overlay(), Some(RuntimeOverlay::CancelOnly));
}

#[test]
fn app_runtime_distinguishes_paper_and_live_modes() {
    let paper = AppRuntime::new(AppRuntimeMode::Paper);
    let live = AppRuntime::new(AppRuntimeMode::Live);

    assert_eq!(paper.app_mode(), AppRuntimeMode::Paper);
    assert_eq!(live.app_mode(), AppRuntimeMode::Live);
}
