use domain::{ConditionId, EventFamilyId, ExecutionMode, ExecutionRequest, TokenId};
use execution::plans::ExecutionPlan;
use rust_decimal::Decimal;

#[test]
fn planner_builds_family_submission_plan_from_live_target_config() {
    let request = sample_negrisk_request(ExecutionMode::Live, "family-a");
    let config = sample_family_target("family-a");

    let plan = execution::negrisk::plan_family_submission(&request, &config).unwrap();

    match plan {
        ExecutionPlan::NegRiskSubmitFamily { family_id, members } => {
            assert_eq!(family_id.as_str(), "family-a");
            assert_eq!(members.len(), 2);
            assert_eq!(members[0].token_id.as_str(), "token-1");
        }
        other => panic!("unexpected plan: {other:?}"),
    }
}

#[test]
fn planner_builds_the_same_family_submission_plan_for_shadow_mode() {
    let request = sample_negrisk_request(ExecutionMode::Shadow, "family-a");
    let config = sample_family_target("family-a");

    let plan = execution::negrisk::plan_family_submission(&request, &config).unwrap();

    assert!(matches!(
        plan,
        ExecutionPlan::NegRiskSubmitFamily { family_id, members }
            if family_id == EventFamilyId::from("family-a")
                && members.len() == 2
                && members[0].condition_id == ConditionId::from("condition-1")
                && members[0].token_id == TokenId::from("token-1")
    ));
}

#[test]
fn planner_rejects_non_negrisk_route() {
    let mut request = sample_negrisk_request(ExecutionMode::Live, "family-a");
    request.route = "full-set".to_owned();

    let err =
        execution::negrisk::plan_family_submission(&request, &sample_family_target("family-a"))
            .expect_err("planner should reject non-neg-risk routes");

    assert!(matches!(
        err,
        execution::negrisk::NegRiskPlanningError::RouteMismatch { route }
            if route == "full-set"
    ));
}

#[test]
fn planner_rejects_scope_mismatch() {
    let request = sample_negrisk_request(ExecutionMode::Live, "family-b");

    let err =
        execution::negrisk::plan_family_submission(&request, &sample_family_target("family-a"))
            .expect_err("planner should reject mismatched family scope");

    assert!(matches!(
        err,
        execution::negrisk::NegRiskPlanningError::ScopeMismatch { request_scope, family_id }
            if request_scope == "family-b" && family_id == EventFamilyId::from("family-a")
    ));
}

#[test]
fn neg_risk_family_plan_id_is_canonical_across_member_order_and_decimal_scale() {
    let canonical = ExecutionPlan::NegRiskSubmitFamily {
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
    };
    let reordered_and_scaled = ExecutionPlan::NegRiskSubmitFamily {
        family_id: EventFamilyId::from("family-a"),
        members: vec![
            execution::plans::NegRiskMemberOrderPlan {
                condition_id: ConditionId::from("condition-2"),
                token_id: TokenId::from("token-2"),
                price: Decimal::new(5500, 4),
                quantity: Decimal::new(800, 2),
            },
            execution::plans::NegRiskMemberOrderPlan {
                condition_id: ConditionId::from("condition-1"),
                token_id: TokenId::from("token-1"),
                price: Decimal::new(4500, 4),
                quantity: Decimal::new(1000, 2),
            },
        ],
    };

    assert_eq!(canonical.plan_id(), reordered_and_scaled.plan_id());
}

fn sample_negrisk_request(execution_mode: ExecutionMode, scope: &str) -> ExecutionRequest {
    ExecutionRequest {
        request_id: format!("request-{scope}-{execution_mode:?}"),
        decision_input_id: format!("intent-{scope}"),
        snapshot_id: "snapshot-negrisk-1".to_owned(),
        route: "neg-risk".to_owned(),
        scope: scope.to_owned(),
        activation_mode: execution_mode,
        matched_rule_id: Some("rule-family-a".to_owned()),
    }
}

fn sample_family_target(family_id: &str) -> execution::negrisk::NegRiskFamilyTarget {
    execution::negrisk::NegRiskFamilyTarget {
        family_id: EventFamilyId::from(family_id),
        members: vec![
            execution::negrisk::NegRiskMemberTarget {
                condition_id: ConditionId::from("condition-1"),
                token_id: TokenId::from("token-1"),
                price: Decimal::new(45, 2),
                quantity: Decimal::new(10, 0),
            },
            execution::negrisk::NegRiskMemberTarget {
                condition_id: ConditionId::from("condition-2"),
                token_id: TokenId::from("token-2"),
                price: Decimal::new(55, 2),
                quantity: Decimal::new(8, 0),
            },
        ],
    }
}
