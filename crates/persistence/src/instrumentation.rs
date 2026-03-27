use std::sync::{OnceLock, RwLock};

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

    pub fn install_as_default(&self) {
        let mut slot = default_slot()
            .write()
            .expect("instrumentation lock poisoned");
        *slot = self.clone();
    }

    pub fn current() -> Self {
        default_slot()
            .read()
            .expect("instrumentation lock poisoned")
            .clone()
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

fn default_slot() -> &'static RwLock<NegRiskPersistenceInstrumentation> {
    static SLOT: OnceLock<RwLock<NegRiskPersistenceInstrumentation>> = OnceLock::new();
    SLOT.get_or_init(|| RwLock::new(NegRiskPersistenceInstrumentation::disabled()))
}
