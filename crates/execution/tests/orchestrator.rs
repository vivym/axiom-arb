use std::collections::HashMap;

use std::sync::Arc;

use domain::{ConditionId, EventFamilyId, ExecutionMode, ExecutionRequest, TokenId};
use execution::{
    attempt::ExecutionAttemptFactory,
    orchestrator::{ExecutionOrchestrator, ExecutionPlanningInput},
    plans::ExecutionPlan,
    sink::{LiveVenueSink, ShadowVenueSink},
    TestOrderSigner,
};
use rust_decimal::Decimal;

#[test]
fn live_and_shadow_share_the_same_plan_before_sink_dispatch() {
    let live = ExecutionOrchestrator::new(LiveVenueSink::noop());
    let shadow = ExecutionOrchestrator::new(ShadowVenueSink::noop());
    let live_input = sample_planning_input(ExecutionMode::Live);
    let shadow_input = sample_planning_input(ExecutionMode::Shadow);

    let live_plan = live.plan(&live_input).unwrap();
    let shadow_plan = shadow.plan(&shadow_input).unwrap();

    assert_eq!(live_plan, shadow_plan);
    assert_eq!(
        ExecutionAttemptFactory::request_bound_plan_id(&live_plan, &live_input.request),
        ExecutionAttemptFactory::request_bound_plan_id(&shadow_plan, &shadow_input.request),
    );
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
        .execute(&sample_non_risk_expanding_input(
            ExecutionMode::RecoveryOnly,
        ))
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
fn reduce_only_mode_refuses_neg_risk_family_submission_plans() {
    let orchestrator = ExecutionOrchestrator::new(LiveVenueSink::noop());

    let err = orchestrator
        .plan(&sample_negrisk_planning_input(ExecutionMode::ReduceOnly))
        .unwrap_err();

    assert!(matches!(
        err,
        execution::ExecutionError::ModeViolation {
            execution_mode: ExecutionMode::ReduceOnly,
            plan: ExecutionPlan::NegRiskSubmitFamily { .. },
        }
    ));
}

#[test]
fn recovery_only_mode_allows_neg_risk_family_submission_once_it_reaches_execution() {
    let orchestrator =
        ExecutionOrchestrator::new(LiveVenueSink::with_order_signer(Arc::new(TestOrderSigner)));

    let receipt = orchestrator
        .execute(&sample_negrisk_planning_input(ExecutionMode::RecoveryOnly))
        .unwrap();

    assert_eq!(receipt.outcome, domain::ExecutionAttemptOutcome::Succeeded);
}

#[test]
fn live_and_shadow_share_the_same_neg_risk_family_plan_before_sink_dispatch() {
    let orchestrator = ExecutionOrchestrator::new(LiveVenueSink::noop());

    let live_plan = orchestrator
        .plan(&sample_negrisk_planning_input(ExecutionMode::Live))
        .unwrap();
    let shadow_plan = orchestrator
        .plan(&sample_negrisk_planning_input(ExecutionMode::Shadow))
        .unwrap();

    assert_eq!(live_plan, shadow_plan);
}

#[test]
fn seeded_orchestrator_continues_attempt_numbering_after_resume() {
    let request = ExecutionRequest {
        request_id: "request-seeded".to_owned(),
        decision_input_id: "intent-seeded".to_owned(),
        snapshot_id: "snapshot-seeded".to_owned(),
        route: "full-set".to_owned(),
        scope: "default".to_owned(),
        activation_mode: ExecutionMode::Live,
        matched_rule_id: None,
    };
    let plan = execution_plan();
    let plan_key = ExecutionAttemptFactory::request_bound_plan_id(&plan, &request);
    let attempt_factory =
        ExecutionAttemptFactory::with_seeded_attempt_numbers(HashMap::from([(plan_key, 7)]));
    let orchestrator =
        ExecutionOrchestrator::with_attempt_factory(LiveVenueSink::noop(), attempt_factory);

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

#[test]
fn plan_rejects_activation_mode_mismatch_between_request_and_input() {
    let orchestrator = ExecutionOrchestrator::new(LiveVenueSink::noop());
    let err = orchestrator
        .plan(&ExecutionPlanningInput::new(
            ExecutionRequest {
                request_id: "request-mismatch".to_owned(),
                decision_input_id: "intent-mismatch".to_owned(),
                snapshot_id: "snapshot-mismatch".to_owned(),
                route: "full-set".to_owned(),
                scope: "default".to_owned(),
                activation_mode: ExecutionMode::Live,
                matched_rule_id: None,
            },
            ExecutionMode::Shadow,
            execution_plan(),
        ))
        .unwrap_err();

    assert!(matches!(
        err,
        execution::ExecutionError::ModeViolation {
            execution_mode: ExecutionMode::Live,
            ..
        }
    ));
}

fn sample_planning_input(execution_mode: ExecutionMode) -> ExecutionPlanningInput {
    ExecutionPlanningInput::new(
        ExecutionRequest {
            request_id: "request-1".to_owned(),
            decision_input_id: "intent-1".to_owned(),
            snapshot_id: "snapshot-1".to_owned(),
            route: "full-set".to_owned(),
            scope: "default".to_owned(),
            activation_mode: execution_mode,
            matched_rule_id: None,
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
            route: "full-set".to_owned(),
            scope: "default".to_owned(),
            activation_mode: execution_mode,
            matched_rule_id: None,
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
            route: "full-set".to_owned(),
            scope: "default".to_owned(),
            activation_mode: ExecutionMode::ReduceOnly,
            matched_rule_id: None,
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

fn neg_risk_plan() -> execution::plans::ExecutionPlan {
    execution::plans::ExecutionPlan::NegRiskSubmitFamily {
        family_id: EventFamilyId::from("family-a"),
        members: vec![
            execution::plans::NegRiskMemberOrderPlan {
                condition_id: ConditionId::from("condition-1"),
                token_id: TokenId::from("token-1"),
                price: Decimal::new(45, 2),
                quantity: Decimal::new(10, 0),
            },
            execution::plans::NegRiskMemberOrderPlan {
                condition_id: ConditionId::from("condition-2"),
                token_id: TokenId::from("token-2"),
                price: Decimal::new(55, 2),
                quantity: Decimal::new(8, 0),
            },
        ],
    }
}

fn non_risk_expanding_plan() -> execution::plans::ExecutionPlan {
    execution::plans::ExecutionPlan::CancelStale {
        order_id: domain::OrderId::from("order-non-risk"),
    }
}

fn sample_negrisk_planning_input(execution_mode: ExecutionMode) -> ExecutionPlanningInput {
    ExecutionPlanningInput::new(
        ExecutionRequest {
            request_id: format!("request-negrisk-{execution_mode:?}"),
            decision_input_id: "intent-negrisk-1".to_owned(),
            snapshot_id: "snapshot-negrisk-1".to_owned(),
            route: "neg-risk".to_owned(),
            scope: "family-a".to_owned(),
            activation_mode: execution_mode,
            matched_rule_id: Some("rule-negrisk-family-a".to_owned()),
        },
        execution_mode,
        neg_risk_plan(),
    )
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
