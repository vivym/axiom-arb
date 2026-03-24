use domain::{ExecutionMode, ExecutionRequest};
use execution::{
    orchestrator::{ExecutionOrchestrator, ExecutionPlanningRequest},
    sink::{LiveVenueSink, ShadowVenueSink},
};

#[test]
fn shadow_and_live_share_the_same_plan_before_the_final_sink() {
    let request = sample_execution_request();
    let live = ExecutionOrchestrator::new(LiveVenueSink::noop());
    let shadow = ExecutionOrchestrator::new(ShadowVenueSink::noop());

    let live_plan = live.plan(&request).unwrap();
    let shadow_plan = shadow.plan(&request).unwrap();

    assert_eq!(live_plan, shadow_plan);
}

#[test]
fn shadow_sink_records_attempt_without_authoritative_fill_effect() {
    let orchestrator = ExecutionOrchestrator::new(ShadowVenueSink::noop());

    let receipt = orchestrator.execute(&sample_execution_request()).unwrap();

    assert!(receipt.is_shadow_recorded());
    assert!(!receipt.has_authoritative_fill_effect());
}

#[test]
fn reduce_only_mode_refuses_plans_that_expand_risk() {
    let orchestrator = ExecutionOrchestrator::new(LiveVenueSink::noop());

    let err = orchestrator
        .plan(&sample_reduce_only_expanding_request())
        .unwrap_err();

    assert!(err.is_mode_violation());
}

fn sample_execution_request() -> ExecutionRequest {
    ExecutionRequest {
        request_id: "request-1".to_owned(),
        decision_input_id: "intent-1".to_owned(),
        snapshot_id: "snapshot-1".to_owned(),
    }
}

fn sample_reduce_only_expanding_request() -> ExecutionPlanningRequest {
    ExecutionPlanningRequest::new(
        ExecutionRequest {
            request_id: "request-reduce-only".to_owned(),
            decision_input_id: "intent-expand-risk".to_owned(),
            snapshot_id: "snapshot-2".to_owned(),
        },
        ExecutionMode::ReduceOnly,
    )
}
