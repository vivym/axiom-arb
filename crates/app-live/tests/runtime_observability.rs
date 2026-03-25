use app_live::AppInstrumentation;
use domain::{ConditionId, TokenId};
use observability::{
    bootstrap_observability, metric_dimensions, MetricDimension, MetricDimensions,
};
use state::ReconcileAttention;

mod support;

#[test]
fn instrumentation_maps_reconcile_attention_into_repo_owned_dimensions() {
    let observability = bootstrap_observability("app-live-test");
    let instrumentation = AppInstrumentation::enabled(observability.recorder());

    let (captured, _) = support::capture_tracing(|| {
        instrumentation.record_reconcile_attention(&ReconcileAttention::IdentifierMismatch {
            token_id: TokenId::from("token-yes"),
            expected_condition_id: ConditionId::from("condition-a"),
            remote_condition_id: ConditionId::from("condition-b"),
        });
    });

    let dims = MetricDimensions::new([MetricDimension::ReconcileReason(
        metric_dimensions::ReconcileReason::IdentifierMismatch,
    )]);

    assert_eq!(
        observability
            .registry()
            .snapshot()
            .counter_with_dimensions(observability.metrics().reconcile_attention_total.key(), &dims),
        Some(1)
    );
    assert!(captured.contains("recorded reconcile attention"));
    assert!(captured.contains("attention_reason=\"identifier_mismatch\""));
}
