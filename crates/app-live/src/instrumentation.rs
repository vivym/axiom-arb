use observability::{metric_dimensions, MetricDimension, MetricDimensions, RuntimeMetricsRecorder};
use state::ReconcileAttention;

use crate::supervisor::NegRiskRolloutEvidence;

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

    pub fn record_dispatcher_backlog_count(&self, count: usize) {
        let Some(recorder) = &self.recorder else {
            return;
        };

        recorder.record_dispatcher_backlog_count(count as f64);
    }

    pub fn record_recovery_backlog_count(&self, count: usize) {
        let Some(recorder) = &self.recorder else {
            return;
        };

        recorder.record_recovery_backlog_count(count as f64);
    }

    pub fn record_rollout_evidence(&self, evidence: &NegRiskRolloutEvidence) {
        let Some(recorder) = &self.recorder else {
            return;
        };

        recorder.record_neg_risk_live_ready_family_count(evidence.live_ready_family_count as f64);
        recorder.record_neg_risk_live_gate_block_count(evidence.blocked_family_count as f64);
    }

    pub fn record_reconcile_attention(&self, attention: &ReconcileAttention) {
        let Some(recorder) = &self.recorder else {
            return;
        };

        let reason = reconcile_reason(attention);
        tracing::debug!(attention_reason = reason.as_pair().1, "recorded reconcile attention");
        recorder.increment_reconcile_attention_total(
            1,
            MetricDimensions::new([MetricDimension::ReconcileReason(reason)]),
        );
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
