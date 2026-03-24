use app_live::{run_live, run_paper, AppRuntime, AppRuntimeMode, StaticSnapshotSource};
use chrono::Utc;
use domain::{ConditionId, ExecutionMode, ExternalFactEvent, RuntimeMode, RuntimeOverlay, TokenId};
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
fn app_clears_pending_reconcile_count_after_successful_reconcile() {
    let mut runtime = AppRuntime::new(AppRuntimeMode::Live);
    let bootstrap = runtime.reconcile(RemoteSnapshot::empty());
    assert!(bootstrap.succeeded);

    let follow_up = runtime
        .apply_input(app_live::InputTaskEvent::out_of_order_user_trade(
            1,
            ExternalFactEvent::new("market_ws", "session-2", "trade-1", "v1", Utc::now()),
        ))
        .unwrap();
    assert!(matches!(
        follow_up,
        state::ApplyResult::ReconcileRequired { .. }
    ));
    assert_eq!(runtime.pending_reconcile_count(), 1);
    assert_eq!(runtime.runtime_mode(), RuntimeMode::Reconciling);

    let repaired = runtime.reconcile(RemoteSnapshot::empty());
    assert!(repaired.succeeded);
    assert_eq!(runtime.pending_reconcile_count(), 0);
    assert_eq!(runtime.runtime_mode(), RuntimeMode::Healthy);
}

#[test]
fn app_runtime_distinguishes_paper_and_live_modes() {
    let paper = AppRuntime::new(AppRuntimeMode::Paper);
    let live = AppRuntime::new(AppRuntimeMode::Live);

    assert_eq!(paper.app_mode(), AppRuntimeMode::Paper);
    assert_eq!(live.app_mode(), AppRuntimeMode::Live);
}

#[test]
fn run_paper_bootstraps_runtime_through_reconcile() {
    let result = run_paper(&StaticSnapshotSource::empty());

    assert_eq!(result.runtime.app_mode(), AppRuntimeMode::Paper);
    assert_eq!(result.summary.fullset_mode, ExecutionMode::Live);
    assert_eq!(result.summary.negrisk_mode, ExecutionMode::Shadow);
    assert!(result.report.succeeded);
    assert!(result.report.promoted_from_bootstrap);
    assert_eq!(
        result.runtime.bootstrap_status(),
        app_live::bootstrap::BootstrapStatus::Ready
    );
    assert_eq!(result.runtime.runtime_mode(), RuntimeMode::Healthy);
    assert_eq!(result.runtime.runtime_overlay(), None);
    assert_eq!(
        result.summary.published_snapshot_id.as_deref(),
        Some("snapshot-0")
    );
    assert_eq!(
        result.summary.published_snapshot_committed_journal_seq,
        Some(0)
    );
    assert_eq!(result.summary.pending_reconcile_count, 0);
}

#[test]
fn run_live_bootstraps_runtime_through_reconcile() {
    let result = run_live(&StaticSnapshotSource::empty());

    assert_eq!(result.runtime.app_mode(), AppRuntimeMode::Live);
    assert_eq!(result.summary.fullset_mode, ExecutionMode::Live);
    assert_eq!(result.summary.negrisk_mode, ExecutionMode::Shadow);
    assert!(result.report.succeeded);
    assert!(result.report.promoted_from_bootstrap);
    assert_eq!(
        result.runtime.bootstrap_status(),
        app_live::bootstrap::BootstrapStatus::Ready
    );
    assert_eq!(result.runtime.runtime_mode(), RuntimeMode::Healthy);
    assert_eq!(result.runtime.runtime_overlay(), None);
    assert_eq!(
        result.summary.published_snapshot_id.as_deref(),
        Some("snapshot-0")
    );
    assert_eq!(
        result.summary.published_snapshot_committed_journal_seq,
        Some(0)
    );
    assert_eq!(result.summary.pending_reconcile_count, 0);
}

#[test]
fn run_paper_stays_cancel_only_when_bootstrap_reconcile_fails() {
    let result = run_paper(&StaticSnapshotSource::new(
        RemoteSnapshot::empty().with_attention(ReconcileAttention::IdentifierMismatch {
            token_id: TokenId::from("token-yes"),
            expected_condition_id: ConditionId::from("condition-a"),
            remote_condition_id: ConditionId::from("condition-b"),
        }),
    ));

    assert_eq!(
        result.runtime.bootstrap_status(),
        app_live::bootstrap::BootstrapStatus::CancelOnly
    );
    assert_eq!(result.runtime.runtime_mode(), RuntimeMode::Reconciling);
    assert_eq!(
        result.runtime.runtime_overlay(),
        Some(RuntimeOverlay::CancelOnly)
    );
    assert!(!result.report.promoted_from_bootstrap);
}

#[test]
fn run_live_stays_cancel_only_when_bootstrap_reconcile_fails() {
    let result = run_live(&StaticSnapshotSource::new(
        RemoteSnapshot::empty().with_attention(ReconcileAttention::IdentifierMismatch {
            token_id: TokenId::from("token-yes"),
            expected_condition_id: ConditionId::from("condition-a"),
            remote_condition_id: ConditionId::from("condition-b"),
        }),
    ));

    assert_eq!(
        result.runtime.bootstrap_status(),
        app_live::bootstrap::BootstrapStatus::CancelOnly
    );
    assert_eq!(result.runtime.runtime_mode(), RuntimeMode::Reconciling);
    assert_eq!(
        result.runtime.runtime_overlay(),
        Some(RuntimeOverlay::CancelOnly)
    );
    assert!(!result.report.promoted_from_bootstrap);
}
