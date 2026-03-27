use domain::FamilyExclusionReason;
use observability::{span_names, RuntimeMetricsRecorder};

use crate::FamilyValidation;

#[derive(Debug, Clone, Default)]
pub struct NegRiskValidatorInstrumentation {
    recorder: Option<RuntimeMetricsRecorder>,
}

impl NegRiskValidatorInstrumentation {
    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn enabled(recorder: RuntimeMetricsRecorder) -> Self {
        Self {
            recorder: Some(recorder),
        }
    }

    pub fn record_validation(&self, verdict: &FamilyValidation) {
        let Some(_recorder) = &self.recorder else {
            return;
        };

        tracing::info_span!(
            span_names::NEG_RISK_FAMILY_VALIDATION,
            validation_status = verdict.status.as_str(),
            exclusion_reason = verdict.reason.as_ref().map(exclusion_reason_label),
            discovery_revision = verdict.discovery_revision,
            metadata_snapshot_hash = verdict.metadata_snapshot_hash.as_str(),
        )
        .in_scope(|| {});
    }
}

fn exclusion_reason_label(reason: &FamilyExclusionReason) -> &'static str {
    match reason {
        FamilyExclusionReason::PlaceholderOutcome => "placeholder_outcome",
        FamilyExclusionReason::OtherOutcome => "other_outcome",
        FamilyExclusionReason::AugmentedVariant => "augmented_variant",
        FamilyExclusionReason::MissingNamedOutcomes => "missing_named_outcomes",
        FamilyExclusionReason::NonNegRiskRoute => "non_negrisk_route",
    }
}
