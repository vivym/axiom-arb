mod support;

use std::collections::HashMap;

use std::sync::Arc;

use domain::{ConditionId, EventFamilyId, ExecutionMode, ExecutionRequest, TokenId};
use execution::{
    attempt::ExecutionAttemptFactory,
    orchestrator::{ExecutionOrchestrator, ExecutionPlanningInput},
    plans::ExecutionPlan,
    providers::{RouteExecutionAdapter, SubmitProviderError, VenueExecutionProvider},
    sink::{LiveVenueSink, ShadowVenueSink, SignedFamilyHook, SignedFamilyHookError},
    ExecutionAttemptRecord, LiveSubmissionRecord, LiveSubmitOutcome, TestOrderSigner,
};
use rust_decimal::Decimal;
use support::{sample_planning_input, FailingVenueSink};

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
fn live_sink_executes_full_set_plans_with_a_registered_route_execution_adapter() {
    let orchestrator = ExecutionOrchestrator::new(full_set_live_sink());

    let receipt = orchestrator
        .execute(&sample_planning_input(ExecutionMode::Live))
        .unwrap();

    assert_eq!(receipt.outcome, domain::ExecutionAttemptOutcome::Succeeded);
    assert_eq!(
        receipt.submission_ref.as_deref(),
        Some("submission-full-set")
    );
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
    let orchestrator = ExecutionOrchestrator::new(LiveVenueSink::with_order_signer_and_hook(
        Arc::new(TestOrderSigner),
        Arc::new(NoopSignedFamilyHook),
    ));

    let receipt = orchestrator
        .execute(&sample_negrisk_planning_input(ExecutionMode::RecoveryOnly))
        .unwrap();

    assert_eq!(receipt.outcome, domain::ExecutionAttemptOutcome::Succeeded);
}

#[test]
fn live_orchestrator_anchors_provider_submissions_on_the_receipt() {
    let orchestrator = ExecutionOrchestrator::new(LiveVenueSink::with_submit_provider(
        Arc::new(TestOrderSigner),
        Arc::new(RecordingSubmitProvider::accepted("submission-orchestrator")),
    ));

    let receipt = orchestrator
        .execute(&sample_negrisk_planning_input(ExecutionMode::Live))
        .unwrap();

    assert_eq!(receipt.outcome, domain::ExecutionAttemptOutcome::Succeeded);
    assert_eq!(
        receipt.submission_ref.as_deref(),
        Some("submission-orchestrator")
    );
    assert_eq!(receipt.pending_ref, None);
}

#[derive(Debug)]
struct NoopSignedFamilyHook;

impl SignedFamilyHook for NoopSignedFamilyHook {
    fn on_signed_family(
        &self,
        _signed: &execution::signing::SignedFamilySubmission,
        _attempt: &execution::ExecutionAttemptContext,
    ) -> Result<(), SignedFamilyHookError> {
        Ok(())
    }
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
        ExecutionOrchestrator::with_attempt_factory(full_set_live_sink(), attempt_factory);

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

#[test]
fn execute_result_exposes_attempt_metadata_without_reconstructing_from_receipt() {
    let request = ExecutionRequest {
        request_id: "request-detailed".to_owned(),
        decision_input_id: "intent-detailed".to_owned(),
        snapshot_id: "snapshot-detailed".to_owned(),
        route: "full-set".to_owned(),
        scope: "default".to_owned(),
        activation_mode: ExecutionMode::Live,
        matched_rule_id: Some("rule-detailed".to_owned()),
    };
    let plan = execution_plan();
    let expected_plan_id = ExecutionAttemptFactory::request_bound_plan_id(&plan, &request);
    let orchestrator = ExecutionOrchestrator::new(full_set_live_sink());

    let result = orchestrator
        .execute_with_attempt(&ExecutionPlanningInput::new(
            request,
            ExecutionMode::Live,
            plan,
        ))
        .unwrap();

    assert_eq!(
        result,
        ExecutionAttemptRecord {
            attempt: domain::ExecutionAttempt::new(
                "request-bound:16:request-detailed:fullset-buy-merge:condition-1:attempt-1",
                expected_plan_id,
                "snapshot-detailed",
                1,
            ),
            attempt_context: domain::ExecutionAttemptContext {
                attempt_id:
                    "request-bound:16:request-detailed:fullset-buy-merge:condition-1:attempt-1"
                        .to_owned(),
                snapshot_id: "snapshot-detailed".to_owned(),
                execution_mode: ExecutionMode::Live,
                route: "full-set".to_owned(),
                scope: "default".to_owned(),
                matched_rule_id: Some("rule-detailed".to_owned()),
            },
            receipt: domain::ExecutionReceipt {
                attempt_id:
                    "request-bound:16:request-detailed:fullset-buy-merge:condition-1:attempt-1"
                        .to_owned(),
                outcome: domain::ExecutionAttemptOutcome::Succeeded,
                submission_ref: Some("submission-full-set".to_owned()),
                pending_ref: None,
            },
        }
    );
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

#[derive(Debug, Clone)]
struct RecordingSubmitProvider {
    submission_ref: String,
}

impl RecordingSubmitProvider {
    fn accepted(submission_ref: &str) -> Self {
        Self {
            submission_ref: submission_ref.to_owned(),
        }
    }
}

impl VenueExecutionProvider for RecordingSubmitProvider {
    fn submit_family(
        &self,
        signed: &execution::signing::SignedFamilySubmission,
        attempt: &domain::ExecutionAttemptContext,
    ) -> Result<LiveSubmitOutcome, SubmitProviderError> {
        assert_eq!(attempt.route, "neg-risk");
        assert_eq!(attempt.scope, "family-a");
        assert_eq!(signed.members.len(), 2);

        Ok(LiveSubmitOutcome::Accepted {
            submission_record: LiveSubmissionRecord {
                submission_ref: self.submission_ref.clone(),
                attempt_id: attempt.attempt_id.clone(),
                route: attempt.route.clone(),
                scope: attempt.scope.clone(),
                provider: "recording-submit".to_owned(),
            },
        })
    }
}

#[derive(Debug, Clone)]
struct AcceptedRouteExecutionAdapter {
    route: &'static str,
    submission_ref: &'static str,
}

impl RouteExecutionAdapter for AcceptedRouteExecutionAdapter {
    fn route(&self) -> &'static str {
        self.route
    }

    fn submit_live(
        &self,
        _plan: &ExecutionPlan,
        attempt: &domain::ExecutionAttemptContext,
    ) -> Result<LiveSubmitOutcome, SubmitProviderError> {
        Ok(LiveSubmitOutcome::Accepted {
            submission_record: LiveSubmissionRecord {
                submission_ref: self.submission_ref.to_owned(),
                attempt_id: attempt.attempt_id.clone(),
                route: attempt.route.clone(),
                scope: attempt.scope.clone(),
                provider: format!("{}-adapter", self.route),
            },
        })
    }
}

fn full_set_live_sink() -> LiveVenueSink {
    LiveVenueSink::with_route_execution_adapter(Arc::new(AcceptedRouteExecutionAdapter {
        route: "full-set",
        submission_ref: "submission-full-set",
    }))
}
