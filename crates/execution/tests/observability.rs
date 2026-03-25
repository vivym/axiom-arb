mod support;

use execution::{
    sink::ShadowVenueSink, ExecutionInstrumentation, ExecutionMode, ExecutionOrchestrator,
};
use observability::{bootstrap_observability, field_keys, span_names};
use support::{capture_spans, sample_planning_input, FailingVenueSink};

#[test]
fn instrumented_shadow_execution_records_span_fields_and_shadow_counter() {
    let observability = bootstrap_observability("execution-test");
    let orchestrator = ExecutionOrchestrator::new_instrumented(
        ShadowVenueSink::noop(),
        ExecutionInstrumentation::enabled(observability.recorder()),
    );

    let (captured_spans, receipt) = capture_spans(|| {
        orchestrator
            .execute(&sample_planning_input(ExecutionMode::Shadow))
            .unwrap()
    });

    assert_eq!(
        receipt.outcome,
        domain::ExecutionAttemptOutcome::ShadowRecorded
    );
    assert_eq!(
        observability
            .registry()
            .snapshot()
            .counter(observability.metrics().shadow_attempt_count.key()),
        Some(1)
    );

    let attempt_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::EXECUTION_ATTEMPT)
        .expect("execution attempt span missing");
    assert_eq!(
        attempt_span
            .field(field_keys::EXECUTION_MODE)
            .map(String::as_str),
        Some("\"shadow\"")
    );
    assert_eq!(
        attempt_span
            .field(field_keys::ATTEMPT_OUTCOME)
            .map(String::as_str),
        Some("\"shadow_recorded\"")
    );
}

#[test]
fn instrumented_execution_failure_records_sink_error_without_shadow_counter_growth() {
    let observability = bootstrap_observability("execution-test");
    let orchestrator = ExecutionOrchestrator::new_instrumented(
        FailingVenueSink,
        ExecutionInstrumentation::enabled(observability.recorder()),
    );

    let (captured_spans, err) = capture_spans(|| {
        orchestrator
            .execute(&sample_planning_input(ExecutionMode::Live))
            .expect_err("sink failure should bubble up")
    });

    assert!(matches!(err, execution::ExecutionError::Sink { .. }));
    assert_eq!(
        observability
            .registry()
            .snapshot()
            .counter(observability.metrics().shadow_attempt_count.key()),
        None
    );

    let attempt_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::EXECUTION_ATTEMPT)
        .expect("execution attempt span missing");
    assert_eq!(
        attempt_span
            .field(field_keys::ATTEMPT_OUTCOME)
            .map(String::as_str),
        Some("\"sink_error\"")
    );
}
