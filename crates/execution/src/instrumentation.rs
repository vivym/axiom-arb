use domain::{ExecutionAttempt, ExecutionAttemptContext, ExecutionMode};
use observability::{field_keys, span_names, RuntimeMetricsRecorder};
use tracing::field;

#[derive(Debug, Clone, Default)]
pub struct ExecutionInstrumentation {
    recorder: Option<RuntimeMetricsRecorder>,
}

impl ExecutionInstrumentation {
    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn enabled(recorder: RuntimeMetricsRecorder) -> Self {
        Self {
            recorder: Some(recorder),
        }
    }

    pub fn attempt_span(
        &self,
        sink_kind: &'static str,
        attempt: &ExecutionAttempt,
        attempt_context: &ExecutionAttemptContext,
    ) -> Option<tracing::Span> {
        let _ = self.recorder.as_ref()?;

        let span = tracing::info_span!(
            span_names::EXECUTION_ATTEMPT,
            execution_mode = field::Empty,
            route = field::Empty,
            scope = field::Empty,
            plan_id = field::Empty,
            attempt_id = field::Empty,
            attempt_no = field::Empty,
            sink_kind = field::Empty,
            attempt_outcome = field::Empty
        );
        span.record(
            field_keys::EXECUTION_MODE,
            execution_mode_label(attempt_context.execution_mode),
        );
        span.record(field_keys::ROUTE, attempt_context.route.as_str());
        span.record(field_keys::SCOPE, attempt_context.scope.as_str());
        span.record(field_keys::PLAN_ID, attempt.plan_id.as_str());
        span.record(field_keys::ATTEMPT_ID, attempt.attempt_id.as_str());
        span.record(field_keys::ATTEMPT_NO, attempt.attempt_no);
        span.record(field_keys::SINK_KIND, sink_kind);

        Some(span)
    }

    pub fn increment_shadow_attempt_count(&self, amount: u64) {
        let Some(recorder) = &self.recorder else {
            return;
        };

        recorder.increment_shadow_attempt_count(amount);
    }
}

fn execution_mode_label(mode: ExecutionMode) -> &'static str {
    match mode {
        ExecutionMode::Disabled => "disabled",
        ExecutionMode::Shadow => "shadow",
        ExecutionMode::Live => "live",
        ExecutionMode::ReduceOnly => "reduce_only",
        ExecutionMode::RecoveryOnly => "recovery_only",
    }
}
