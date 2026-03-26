use std::sync::{Arc, Mutex};

use domain::{DecisionInput, ExecutionMode, ExecutionReceipt, IntentCandidate};
use execution::{
    negrisk::{plan_family_submission, NegRiskFamilyTarget, NegRiskMemberTarget, ROUTE},
    signing::{SignedFamilySubmission, TestOrderSigner},
    sink::{LiveVenueSink, SignedFamilyHook, SignedFamilyHookError},
    ExecutionInstrumentation, ExecutionOrchestrator, ExecutionPlanningInput,
};
use observability::RuntimeMetricsRecorder;
use risk::{evaluate_decision, ActivationPolicy, RolloutRule};
use serde_json::{json, Value};
use venue_polymarket::{
    build_post_order_request_from_signed_member, OrderType, PostOrderTransport,
};

use crate::config::{NegRiskFamilyLiveTarget, NegRiskMemberLiveTarget};

const LIVE_ARTIFACT_STREAM: &str = "neg-risk-live-orders";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegRiskLiveArtifact {
    pub stream: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegRiskLiveExecutionRecord {
    pub attempt_id: String,
    pub plan_id: String,
    pub snapshot_id: String,
    pub execution_mode: ExecutionMode,
    pub attempt_no: u32,
    pub idempotency_key: String,
    pub route: String,
    pub scope: String,
    pub matched_rule_id: Option<String>,
    pub submission_ref: Option<String>,
    pub pending_ref: Option<String>,
    pub artifacts: Vec<NegRiskLiveArtifact>,
    pub order_requests: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NegRiskLiveError {
    Planning(String),
    SigningHook(String),
    Sink(String),
}

impl std::fmt::Display for NegRiskLiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Planning(reason) => write!(f, "{reason}"),
            Self::SigningHook(reason) => write!(f, "{reason}"),
            Self::Sink(reason) => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for NegRiskLiveError {}

pub fn eligible_live_records(
    snapshot_id: &str,
    targets: &std::collections::BTreeMap<String, NegRiskFamilyLiveTarget>,
    approved_families: &std::collections::BTreeSet<String>,
    ready_families: &std::collections::BTreeSet<String>,
    recorder: Option<RuntimeMetricsRecorder>,
) -> Result<Vec<NegRiskLiveExecutionRecord>, NegRiskLiveError> {
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
    let policy = ActivationPolicy::from_rules("phase3b-live-targets", live_rules);

    let mut records = Vec::new();
    for (family_id, target) in targets {
        if !ready_families.contains(family_id) {
            continue;
        }

        let activation = policy.activation_for(ROUTE, family_id, snapshot_id);
        let intent = IntentCandidate::new(
            format!("negrisk-live-intent:{snapshot_id}:{family_id}"),
            snapshot_id,
            ROUTE,
            family_id,
        );
        let input = DecisionInput::Strategy(intent);
        if evaluate_decision(&input, &activation) != domain::DecisionVerdict::Approved {
            continue;
        }

        records.push(execute_live_family(
            snapshot_id,
            target,
            activation
                .matched_rule_id
                .as_deref()
                .unwrap_or("phase3b-negrisk-live"),
            match recorder.as_ref() {
                Some(recorder) => ExecutionInstrumentation::enabled(recorder.clone()),
                None => ExecutionInstrumentation::disabled(),
            },
        )?);
    }

    Ok(records)
}

fn execute_live_family(
    snapshot_id: &str,
    target: &NegRiskFamilyLiveTarget,
    matched_rule_id: &str,
    instrumentation: ExecutionInstrumentation,
) -> Result<NegRiskLiveExecutionRecord, NegRiskLiveError> {
    let request = domain::ExecutionRequest {
        request_id: format!("negrisk-live-request:{snapshot_id}:{}", target.family_id),
        decision_input_id: format!("negrisk-live-intent:{snapshot_id}:{}", target.family_id),
        snapshot_id: snapshot_id.to_owned(),
        route: ROUTE.to_owned(),
        scope: target.family_id.clone(),
        activation_mode: ExecutionMode::Live,
        matched_rule_id: Some(matched_rule_id.to_owned()),
    };
    let target = to_execution_target(target);
    let plan = plan_family_submission(&request, &target).map_err(|err| {
        NegRiskLiveError::Planning(format!("neg-risk live planning failed: {err:?}"))
    })?;

    let hook = Arc::new(RecordingSignedFamilyHook::default());
    let sink = LiveVenueSink::with_order_signer_and_hook(Arc::new(TestOrderSigner), hook.clone());
    let execution_record = ExecutionOrchestrator::new_instrumented(sink, instrumentation)
        .execute_with_attempt(&ExecutionPlanningInput::new(
            request.clone(),
            request.activation_mode,
            plan.clone(),
        ))
        .map_err(|err| NegRiskLiveError::Sink(format!("neg-risk live sink failed: {err:?}")))?;
    ensure_success(&execution_record.receipt)?;

    Ok(NegRiskLiveExecutionRecord {
        idempotency_key: format!("idem-{}", execution_record.attempt.attempt_id),
        attempt_id: execution_record.receipt.attempt_id,
        plan_id: execution_record.attempt.plan_id,
        snapshot_id: execution_record.attempt.snapshot_id,
        execution_mode: execution_record.attempt_context.execution_mode,
        attempt_no: execution_record.attempt.attempt_no,
        route: execution_record.attempt_context.route,
        scope: execution_record.attempt_context.scope,
        matched_rule_id: execution_record.attempt_context.matched_rule_id,
        submission_ref: execution_record.receipt.submission_ref,
        pending_ref: execution_record.receipt.pending_ref,
        artifacts: hook.artifacts(),
        order_requests: hook.order_requests(),
    })
}

fn ensure_success(receipt: &ExecutionReceipt) -> Result<(), NegRiskLiveError> {
    if receipt.outcome == domain::ExecutionAttemptOutcome::Succeeded {
        Ok(())
    } else {
        Err(NegRiskLiveError::Sink(format!(
            "unexpected neg-risk live receipt outcome: {:?}",
            receipt.outcome
        )))
    }
}

fn to_execution_target(target: &NegRiskFamilyLiveTarget) -> NegRiskFamilyTarget {
    NegRiskFamilyTarget {
        family_id: target.family_id.clone().into(),
        members: target.members.iter().map(to_member_target).collect(),
    }
}

fn to_member_target(member: &NegRiskMemberLiveTarget) -> NegRiskMemberTarget {
    NegRiskMemberTarget {
        condition_id: member.condition_id.clone().into(),
        token_id: member.token_id.clone().into(),
        price: member.price,
        quantity: member.quantity,
    }
}

#[derive(Default)]
struct RecordingSignedFamilyHook {
    artifacts: Mutex<Vec<NegRiskLiveArtifact>>,
    order_requests: Mutex<Vec<Value>>,
}

impl RecordingSignedFamilyHook {
    fn artifacts(&self) -> Vec<NegRiskLiveArtifact> {
        self.artifacts
            .lock()
            .expect("signed-family artifacts lock should not be poisoned")
            .clone()
    }

    fn order_requests(&self) -> Vec<Value> {
        self.order_requests
            .lock()
            .expect("signed-family requests lock should not be poisoned")
            .clone()
    }
}

impl SignedFamilyHook for RecordingSignedFamilyHook {
    fn on_signed_family(
        &self,
        signed: &SignedFamilySubmission,
        attempt: &domain::ExecutionAttemptContext,
    ) -> Result<(), SignedFamilyHookError> {
        let transport = PostOrderTransport {
            owner: format!("owner-{}", attempt.scope),
            order_type: OrderType::Gtc,
            defer_exec: false,
        };
        let order_requests = signed
            .members
            .iter()
            .map(|member| {
                build_post_order_request_from_signed_member(member, &transport).map_err(|err| {
                    SignedFamilyHookError {
                        reason: format!("post-order build failed: {err:?}"),
                    }
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let order_requests = order_requests
            .into_iter()
            .map(|request| {
                serde_json::to_value(request).expect("post order request should serialize")
            })
            .collect::<Vec<_>>();
        self.order_requests
            .lock()
            .expect("signed-family requests lock should not be poisoned")
            .extend(order_requests.iter().cloned());
        self.artifacts
            .lock()
            .expect("signed-family artifacts lock should not be poisoned")
            .push(NegRiskLiveArtifact {
                stream: LIVE_ARTIFACT_STREAM.to_owned(),
                payload: json!({
                    "attempt_id": attempt.attempt_id,
                    "route": attempt.route,
                    "scope": attempt.scope,
                    "matched_rule_id": attempt.matched_rule_id,
                    "plan_id": signed.plan_id,
                    "requests": order_requests,
                }),
            });

        Ok(())
    }
}
