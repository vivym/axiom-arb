use std::collections::HashMap;

use domain::{ExecutionMode, ExecutionRequest};
use execution::{
    attempt::ExecutionAttemptFactory,
    orchestrator::{ExecutionOrchestrator, ExecutionPlanningInput},
    sink::{LiveVenueSink, ShadowVenueSink},
};

#[test]
fn live_and_shadow_share_the_same_plan_before_sink_dispatch() {
    let live = ExecutionOrchestrator::new(LiveVenueSink::noop());
    let shadow = ExecutionOrchestrator::new(ShadowVenueSink::noop());
    let live_input = sample_planning_input(ExecutionMode::Live);
    let shadow_input = sample_planning_input(ExecutionMode::Shadow);

    let live_plan = live.plan(&live_input).unwrap();
    let shadow_plan = shadow.plan(&shadow_input).unwrap();

    assert_eq!(live_plan, shadow_plan);
}

#[test]
fn live_sink_accepts_reduce_only_for_non_risk_expanding_plans() {
    let orchestrator = ExecutionOrchestrator::new(LiveVenueSink::noop());

    let receipt = orchestrator
        .execute(&sample_non_risk_expanding_input(ExecutionMode::ReduceOnly))
        .unwrap();

    assert_eq!(receipt.outcome, domain::ExecutionAttemptOutcome::Succeeded);
}

#[test]
fn live_sink_accepts_recovery_only_plans() {
    let orchestrator = ExecutionOrchestrator::new(LiveVenueSink::noop());

    let receipt = orchestrator
        .execute(&sample_non_risk_expanding_input(ExecutionMode::RecoveryOnly))
        .unwrap();

    assert_eq!(receipt.outcome, domain::ExecutionAttemptOutcome::Succeeded);
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
fn sink_rejects_attempts_with_the_wrong_execution_mode() {
    let live_orchestrator = ExecutionOrchestrator::new(LiveVenueSink::noop());
    let shadow_err = live_orchestrator
        .execute(&sample_planning_input(ExecutionMode::Shadow))
        .unwrap_err();

    assert!(matches!(
        shadow_err,
        execution::ExecutionError::Sink {
            error: execution::VenueSinkError::ModeMismatch { .. }
        }
    ));

    let shadow_orchestrator = ExecutionOrchestrator::new(ShadowVenueSink::noop());
    let live_err = shadow_orchestrator
        .execute(&sample_planning_input(ExecutionMode::Live))
        .unwrap_err();

    assert!(matches!(
        live_err,
        execution::ExecutionError::Sink {
            error: execution::VenueSinkError::ModeMismatch { .. }
        }
    ));
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
fn seeded_orchestrator_continues_attempt_numbering_after_resume() {
    let request = ExecutionRequest {
        request_id: "request-seeded".to_owned(),
        decision_input_id: "intent-seeded".to_owned(),
        snapshot_id: "snapshot-seeded".to_owned(),
    };
    let plan = execution_plan();
    let plan_key = ExecutionAttemptFactory::request_bound_plan_id(&plan, &request);
    let attempt_factory =
        ExecutionAttemptFactory::with_seeded_attempt_numbers(HashMap::from([(plan_key, 7)]));
    let orchestrator = ExecutionOrchestrator::with_attempt_factory(
        LiveVenueSink::noop(),
        attempt_factory,
    );

    let receipt = orchestrator
        .execute(&ExecutionPlanningInput::new(
            request,
            ExecutionMode::Live,
            plan,
        ))
        .unwrap();

    assert_eq!(
        receipt.attempt_id,
        "request-bound:14:request-seeded:fullset-buy-merge:condition-1:attempt-8"
    );
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

fn sample_non_risk_expanding_input(execution_mode: ExecutionMode) -> ExecutionPlanningInput {
    ExecutionPlanningInput::new(
        ExecutionRequest {
            request_id: "request-non-risk".to_owned(),
            decision_input_id: "intent-non-risk".to_owned(),
            snapshot_id: "snapshot-non-risk".to_owned(),
        },
        execution_mode,
        non_risk_expanding_plan(),
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

fn non_risk_expanding_plan() -> execution::plans::ExecutionPlan {
    execution::plans::ExecutionPlan::CancelStale {
        order_id: domain::OrderId::from("order-non-risk"),
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
