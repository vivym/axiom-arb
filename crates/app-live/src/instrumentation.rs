use observability::{
    metric_dimensions, span_names, MetricDimension, MetricDimensions, RuntimeMetricsRecorder,
};
use state::ReconcileAttention;

use crate::{runtime::AppRunResult, NegRiskLiveStateSource, SupervisorSummary};

#[derive(Debug, Clone, Default)]
pub struct AppInstrumentation {
    recorder: Option<RuntimeMetricsRecorder>,
}

impl AppInstrumentation {
    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn enabled(recorder: RuntimeMetricsRecorder) -> Self {
        Self {
            recorder: Some(recorder),
        }
    }

    pub fn recorder(&self) -> Option<RuntimeMetricsRecorder> {
        self.recorder.clone()
    }

    pub fn record_reconcile_attention(&self, attention: &ReconcileAttention) {
        let Some(recorder) = &self.recorder else {
            return;
        };

        let reason = reconcile_reason(attention);
        tracing::debug!(
            attention_reason = reason.as_pair().1,
            "recorded reconcile attention"
        );
        recorder.increment_reconcile_attention_total(
            1,
            MetricDimensions::new([MetricDimension::ReconcileReason(reason)]),
        );
    }

    pub fn record_divergence(&self, divergence_kind: &'static str) {
        let Some(recorder) = &self.recorder else {
            return;
        };

        tracing::warn_span!(
            span_names::APP_RECOVERY_DIVERGENCE,
            divergence_kind = divergence_kind
        )
        .in_scope(|| {
            recorder.increment_divergence_count(1);
        });
    }

    pub fn record_runtime_attention_fact(&self, source: &str, attention_kind: &str) {
        if self.recorder.is_none() {
            return;
        }

        tracing::debug!(
            attention_source = source,
            attention_kind = attention_kind,
            "recorded runtime attention fact"
        );
    }
}

pub fn emit_bootstrap_completion_observability(
    recorder: &RuntimeMetricsRecorder,
    result: &AppRunResult,
) {
    recorder.record_runtime_mode(runtime_mode_label(result.runtime.runtime_mode()));
    let evidence_source = bootstrap_evidence_source(&result.summary);

    let completion_span = tracing::info_span!(
        span_names::APP_BOOTSTRAP_COMPLETE,
        app_mode = %result.runtime.app_mode().as_str(),
        bootstrap_status = ?result.runtime.bootstrap_status(),
        promoted_from_bootstrap = result.report.promoted_from_bootstrap,
        runtime_mode = ?result.runtime.runtime_mode(),
        fullset_mode = ?result.summary.fullset_mode,
        negrisk_mode = ?result.summary.negrisk_mode,
        neg_risk_live_attempt_count = result.summary.neg_risk_live_attempt_count,
        neg_risk_live_state_source = result.summary.neg_risk_live_state_source.as_str(),
        evidence_source = evidence_source,
        pending_reconcile_count = result.summary.pending_reconcile_count,
        published_snapshot_id = %result
            .summary
            .published_snapshot_id
            .as_deref()
            .unwrap_or("none")
    );
    let _completion_guard = completion_span.enter();
    tracing::info!("app-live bootstrap complete");
}

fn bootstrap_evidence_source(summary: &SupervisorSummary) -> &'static str {
    if summary.neg_risk_rollout_evidence.is_none() {
        return "none";
    }

    if summary.neg_risk_live_state_source == NegRiskLiveStateSource::SyntheticBootstrap {
        "bootstrap"
    } else {
        "snapshot"
    }
}

fn runtime_mode_label(mode: domain::RuntimeMode) -> &'static str {
    match mode {
        domain::RuntimeMode::Bootstrapping => "bootstrapping",
        domain::RuntimeMode::Healthy => "healthy",
        domain::RuntimeMode::Reconciling => "reconciling",
        domain::RuntimeMode::Degraded => "degraded",
        domain::RuntimeMode::NoNewRisk => "no_new_risk",
        domain::RuntimeMode::GlobalHalt => "global_halt",
    }
}

fn reconcile_reason(attention: &ReconcileAttention) -> metric_dimensions::ReconcileReason {
    match attention {
        ReconcileAttention::DuplicateSignedOrder { .. } => {
            metric_dimensions::ReconcileReason::DuplicateSignedOrder
        }
        ReconcileAttention::IdentifierMismatch { .. } => {
            metric_dimensions::ReconcileReason::IdentifierMismatch
        }
        ReconcileAttention::MissingRemoteOrder { .. } => {
            metric_dimensions::ReconcileReason::MissingRemoteOrder
        }
        ReconcileAttention::UnexpectedRemoteOrder { .. } => {
            metric_dimensions::ReconcileReason::UnexpectedRemoteOrder
        }
        ReconcileAttention::OrderStateMismatch { .. } => {
            metric_dimensions::ReconcileReason::OrderStateMismatch
        }
        ReconcileAttention::ApprovalMismatch { .. } => {
            metric_dimensions::ReconcileReason::ApprovalMismatch
        }
        ReconcileAttention::ResolutionMismatch { .. } => {
            metric_dimensions::ReconcileReason::ResolutionMismatch
        }
        ReconcileAttention::RelayerTxMismatch { .. } => {
            metric_dimensions::ReconcileReason::RelayerTxMismatch
        }
        ReconcileAttention::InventoryMismatch { .. } => {
            metric_dimensions::ReconcileReason::InventoryMismatch
        }
    }
}
