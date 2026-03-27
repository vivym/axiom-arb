use observability::{span_names, RuntimeMetricsRecorder};

use crate::models::{FamilyHaltRow, NegRiskFamilyValidationRow};

#[derive(Debug, Clone, Default)]
pub struct NegRiskPersistenceInstrumentation {
    recorder: Option<RuntimeMetricsRecorder>,
}

impl NegRiskPersistenceInstrumentation {
    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn enabled(recorder: RuntimeMetricsRecorder) -> Self {
        Self {
            recorder: Some(recorder),
        }
    }

    pub fn record_validation_upsert(&self, row: &NegRiskFamilyValidationRow) {
        let Some(_recorder) = &self.recorder else {
            return;
        };

        tracing::info_span!(
            span_names::NEG_RISK_FAMILY_VALIDATION,
            validation_status = row.validation_status.as_str(),
            exclusion_reason = row.exclusion_reason.as_deref(),
            discovery_revision = row.last_seen_discovery_revision,
            metadata_snapshot_hash = row.metadata_snapshot_hash.as_str(),
        )
        .in_scope(|| {});
    }

    pub fn record_halt_upsert(&self, row: &FamilyHaltRow) {
        let Some(_recorder) = &self.recorder else {
            return;
        };

        tracing::info_span!(
            span_names::NEG_RISK_FAMILY_HALT,
            halted = row.halted,
            discovery_revision = row.last_seen_discovery_revision,
            metadata_snapshot_hash = row.metadata_snapshot_hash.as_deref(),
            evidence_source = "upsert",
        )
        .in_scope(|| {});
    }

    pub fn record_authoritative_current_view_counts(
        &self,
        included_count: u64,
        excluded_count: u64,
        halt_count: u64,
    ) {
        let Some(recorder) = &self.recorder else {
            return;
        };

        recorder.record_neg_risk_family_included_count(included_count as f64);
        recorder.record_neg_risk_family_excluded_count(excluded_count as f64);
        recorder.record_neg_risk_family_halt_count(halt_count as f64);
    }
}
