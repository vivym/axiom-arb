use domain::{DecisionInput, ExecutionMode, ExecutionReceipt, IntentCandidate};
use execution::{
    negrisk::{plan_family_submission, ROUTE},
    plans::ExecutionPlan,
    sink::ShadowVenueSink,
    ExecutionInstrumentation, ExecutionOrchestrator, ExecutionPlanningInput,
};
use observability::RuntimeMetricsRecorder;
use persistence::models::{ExecutionAttemptRow, ShadowExecutionArtifactRow};
use risk::{evaluate_decision, ActivationPolicy, RolloutRule};
use serde_json::json;

use crate::{config::NegRiskFamilyLiveTarget, negrisk_live::to_execution_target};

const SHADOW_ARTIFACT_STREAM: &str = "neg-risk-shadow-plan";

#[derive(Debug, Clone, PartialEq)]
pub struct NegRiskShadowExecutionRecord {
    pub attempt: ExecutionAttemptRow,
    pub artifacts: Vec<ShadowExecutionArtifactRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NegRiskShadowError {
    Planning(String),
    Sink(String),
}

impl std::fmt::Display for NegRiskShadowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Planning(reason) => write!(f, "{reason}"),
            Self::Sink(reason) => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for NegRiskShadowError {}

#[allow(dead_code)]
pub fn eligible_shadow_records(
    snapshot_id: &str,
    targets: &std::collections::BTreeMap<String, NegRiskFamilyLiveTarget>,
    approved_families: &std::collections::BTreeSet<String>,
    ready_families: &std::collections::BTreeSet<String>,
    recorder: Option<RuntimeMetricsRecorder>,
) -> Result<Vec<NegRiskShadowExecutionRecord>, NegRiskShadowError> {
    eligible_shadow_records_with_run_session_id(
        snapshot_id,
        targets,
        approved_families,
        ready_families,
        recorder,
        None,
    )
}

pub fn eligible_shadow_records_with_run_session_id(
    snapshot_id: &str,
    targets: &std::collections::BTreeMap<String, NegRiskFamilyLiveTarget>,
    approved_families: &std::collections::BTreeSet<String>,
    ready_families: &std::collections::BTreeSet<String>,
    recorder: Option<RuntimeMetricsRecorder>,
    run_session_id: Option<&str>,
) -> Result<Vec<NegRiskShadowExecutionRecord>, NegRiskShadowError> {
    let live_rules = approved_families
        .iter()
        .map(|family_id| {
            RolloutRule::new(
                ROUTE,
                family_id,
                ExecutionMode::Live,
                format!("{family_id}-live"),
            )
        })
        .collect::<Vec<_>>();
    let policy = ActivationPolicy::from_rules("phase3b-live-targets", live_rules)
        .with_real_user_shadow_smoke();

    let mut records = Vec::new();
    for (family_id, target) in targets {
        if !ready_families.contains(family_id) {
            continue;
        }

        let activation = policy.activation_for(ROUTE, family_id, snapshot_id);
        let intent = IntentCandidate::new(
            format!("negrisk-shadow-intent:{snapshot_id}:{family_id}"),
            snapshot_id,
            ROUTE,
            family_id,
        );
        let input = DecisionInput::Strategy(intent);
        if evaluate_decision(&input, &activation) != domain::DecisionVerdict::Approved {
            continue;
        }

        records.push(execute_shadow_family(
            snapshot_id,
            target,
            activation
                .matched_rule_id
                .as_deref()
                .unwrap_or("phase3b-negrisk-shadow"),
            match recorder.as_ref() {
                Some(recorder) => ExecutionInstrumentation::enabled(recorder.clone()),
                None => ExecutionInstrumentation::disabled(),
            },
            run_session_id,
        )?);
    }

    Ok(records)
}

fn execute_shadow_family(
    snapshot_id: &str,
    target: &NegRiskFamilyLiveTarget,
    matched_rule_id: &str,
    instrumentation: ExecutionInstrumentation,
    run_session_id: Option<&str>,
) -> Result<NegRiskShadowExecutionRecord, NegRiskShadowError> {
    let request = domain::ExecutionRequest {
        request_id: format!("negrisk-shadow-request:{snapshot_id}:{}", target.family_id),
        decision_input_id: format!("negrisk-shadow-intent:{snapshot_id}:{}", target.family_id),
        snapshot_id: snapshot_id.to_owned(),
        route: ROUTE.to_owned(),
        scope: target.family_id.clone(),
        activation_mode: ExecutionMode::Shadow,
        matched_rule_id: Some(matched_rule_id.to_owned()),
    };
    let target = to_execution_target(target);
    let plan = plan_family_submission(&request, &target).map_err(|err| {
        NegRiskShadowError::Planning(format!("neg-risk shadow planning failed: {err:?}"))
    })?;

    let execution_record =
        ExecutionOrchestrator::new_instrumented(ShadowVenueSink::noop(), instrumentation)
            .execute_with_attempt(&ExecutionPlanningInput::new(
                request.clone(),
                request.activation_mode,
                plan.clone(),
            ))
            .map_err(|err| {
                NegRiskShadowError::Sink(format!("neg-risk shadow sink failed: {err:?}"))
            })?;
    ensure_shadow_recorded(&execution_record.receipt)?;

    let attempt = ExecutionAttemptRow {
        attempt_id: execution_record.receipt.attempt_id.clone(),
        plan_id: execution_record.attempt.plan_id,
        snapshot_id: execution_record.attempt.snapshot_id,
        route: execution_record.attempt_context.route.clone(),
        scope: execution_record.attempt_context.scope.clone(),
        matched_rule_id: execution_record.attempt_context.matched_rule_id.clone(),
        execution_mode: execution_record.attempt_context.execution_mode,
        attempt_no: i32::try_from(execution_record.attempt.attempt_no)
            .expect("attempt number should fit in i32"),
        idempotency_key: format!("idem-{}", execution_record.attempt.attempt_id),
        run_session_id: run_session_id.map(str::to_owned),
    };
    let artifacts = vec![ShadowExecutionArtifactRow {
        attempt_id: attempt.attempt_id.clone(),
        stream: SHADOW_ARTIFACT_STREAM.to_owned(),
        payload: shadow_artifact_payload(&attempt, &plan),
    }];

    Ok(NegRiskShadowExecutionRecord { attempt, artifacts })
}

fn ensure_shadow_recorded(receipt: &ExecutionReceipt) -> Result<(), NegRiskShadowError> {
    if receipt.outcome == domain::ExecutionAttemptOutcome::ShadowRecorded {
        Ok(())
    } else {
        Err(NegRiskShadowError::Sink(format!(
            "unexpected neg-risk shadow receipt outcome: {:?}",
            receipt.outcome
        )))
    }
}

fn shadow_artifact_payload(
    attempt: &ExecutionAttemptRow,
    plan: &ExecutionPlan,
) -> serde_json::Value {
    let members = match plan {
        ExecutionPlan::NegRiskSubmitFamily { members, .. } => members
            .iter()
            .map(|member| {
                json!({
                    "condition_id": member.condition_id.as_str(),
                    "token_id": member.token_id.as_str(),
                    "price": member.price.normalize().to_string(),
                    "quantity": member.quantity.normalize().to_string(),
                })
            })
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };

    json!({
        "attempt_id": attempt.attempt_id,
        "plan_id": attempt.plan_id,
        "snapshot_id": attempt.snapshot_id,
        "route": attempt.route,
        "scope": attempt.scope,
        "matched_rule_id": attempt.matched_rule_id,
        "members": members,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use rust_decimal::Decimal;

    use super::eligible_shadow_records_with_run_session_id;
    use crate::config::{NegRiskFamilyLiveTarget, NegRiskMemberLiveTarget};

    #[test]
    fn eligible_shadow_records_populate_run_session_id_when_provided() {
        let targets = BTreeMap::from([(
            "family-a".to_owned(),
            NegRiskFamilyLiveTarget {
                family_id: "family-a".to_owned(),
                members: vec![NegRiskMemberLiveTarget {
                    condition_id: "condition-a".to_owned(),
                    token_id: "token-a".to_owned(),
                    price: Decimal::new(42, 2),
                    quantity: Decimal::new(1, 0),
                }],
            },
        )]);
        let approved_families = BTreeSet::from(["family-a".to_owned()]);
        let ready_families = BTreeSet::from(["family-a".to_owned()]);

        let records = eligible_shadow_records_with_run_session_id(
            "snapshot-7",
            &targets,
            &approved_families,
            &ready_families,
            None,
            Some("run-session-7"),
        )
        .unwrap();

        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].attempt.run_session_id.as_deref(),
            Some("run-session-7")
        );
    }
}
