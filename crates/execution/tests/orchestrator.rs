use domain::{ExecutionMode, ExecutionRequest};
use execution::{
    orchestrator::{ExecutionOrchestrator, ExecutionPlanningInput},
    sink::{LiveVenueSink, ShadowVenueSink},
};

#[test]
fn shadow_and_live_share_the_same_plan_before_the_final_sink() {
    let live = ExecutionOrchestrator::new(LiveVenueSink::noop());
    let shadow = ExecutionOrchestrator::new(ShadowVenueSink::noop());
    let live_input = sample_planning_input(ExecutionMode::Live);
    let shadow_input = sample_planning_input(ExecutionMode::Shadow);

    let live_plan = live.plan(&live_input).unwrap();
    let shadow_plan = shadow.plan(&shadow_input).unwrap();

    assert_eq!(live_plan, shadow_plan);
}

#[test]
fn shadow_sink_records_attempt_without_authoritative_fill_effect() {
    let shadow_sink = ShadowVenueSink::noop();
    let orchestrator = ExecutionOrchestrator::new(shadow_sink.clone());

    let receipt = orchestrator
        .execute(&sample_planning_input(ExecutionMode::Shadow))
        .unwrap();

    assert_eq!(
        receipt.outcome,
        domain::ExecutionAttemptOutcome::ShadowRecorded
    );
    assert_eq!(shadow_sink.recorded_attempt_ids(), vec![receipt.attempt_id]);
}

#[test]
fn reduce_only_mode_refuses_plans_that_expand_risk() {
    let orchestrator = ExecutionOrchestrator::new(LiveVenueSink::noop());

    let err = orchestrator
        .plan(&sample_reduce_only_explicit_input())
        .unwrap_err();

    assert!(err.is_mode_violation());
}

#[test]
fn sink_failure_is_reported_separately_from_mode_violations() {
    let orchestrator = ExecutionOrchestrator::new(FailingVenueSink);
    let err = orchestrator
        .execute(&sample_planning_input(ExecutionMode::Live))
        .unwrap_err();

    assert!(matches!(err, execution::ExecutionError::Sink { .. }));
}

fn sample_planning_input(execution_mode: ExecutionMode) -> ExecutionPlanningInput {
    ExecutionPlanningInput::new(
        ExecutionRequest {
            request_id: "request-1".to_owned(),
            decision_input_id: "intent-1".to_owned(),
            snapshot_id: "snapshot-1".to_owned(),
        },
        execution_mode,
        execution_plan(),
    )
}

fn sample_reduce_only_explicit_input() -> ExecutionPlanningInput {
    ExecutionPlanningInput::new(
        ExecutionRequest {
            request_id: "request-reduce-only".to_owned(),
            decision_input_id: "intent-1".to_owned(),
            snapshot_id: "snapshot-2".to_owned(),
        },
        ExecutionMode::ReduceOnly,
        execution_plan(),
    )
}

fn execution_plan() -> execution::plans::ExecutionPlan {
    execution::plans::ExecutionPlan::FullSetBuyThenMerge {
        condition_id: domain::ConditionId::from("condition-1"),
    }
}

#[derive(Debug, Clone, Copy)]
struct FailingVenueSink;

impl execution::sink::VenueSink for FailingVenueSink {
    fn execute(
        &self,
        _plan: &execution::plans::ExecutionPlan,
        _attempt: &domain::ExecutionAttemptContext,
    ) -> Result<domain::ExecutionReceipt, execution::sink::VenueSinkError> {
        Err(execution::sink::VenueSinkError::Rejected {
            reason: "planned sink failure".to_owned(),
        })
    }
}
