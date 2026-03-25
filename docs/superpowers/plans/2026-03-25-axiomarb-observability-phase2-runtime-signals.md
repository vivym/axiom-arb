# AxiomArb Observability Phase 2 Runtime Signals Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add meaningful local runtime observability to `app-live` so reconcile, snapshot publication, resume, dispatch, and rollout-gate state changes emit stable spans and repo-owned metrics without introducing any OpenTelemetry exporter.

**Architecture:** Keep `tracing` and typed metrics as the only instrumentation API. Extend `observability` with the missing runtime vocabulary, add a focused `app-live` instrumentation adapter so runtime/supervisor code can emit spans and metrics without duplicating conversion logic, then wire the `app-live` binary through that adapter. This phase intentionally stops at local structured signals; exporter work remains out of scope.

**Tech Stack:** Rust, `tracing`, `tracing-subscriber`, existing `observability` facade, in-process `MetricRegistry`, existing `app-live` bootstrap/supervisor/runtime tests

---

## Scope Boundary

This plan intentionally covers `observability phase 2` for the `app-live` runtime only.

- In scope: additional span names and field keys, typed `reconcile_attention` dimensions, an `app-live` instrumentation adapter, runtime/supervisor/dispatch signal emission, entrypoint wiring to pass a recorder into the live runtime, focused runtime observability tests, and README alignment.
- Out of scope: `tracing-opentelemetry`, OTLP exporters, collector config, vendor dashboards, `app-replay` feature changes, venue websocket/heartbeat wiring, execution/recovery crate instrumentation, and any live trading behavior change.
- Do not fabricate signals for loops that do not exist yet. `websocket_reconnect_total`, `halt_activation_total`, and heartbeat freshness stay un-emitted until there are real producers.
- Known baseline caveat: if the execution branch still has the existing unrelated `app-live` fault-injection failure around durable rollout evidence, treat that as a separate pre-existing blocker. Do not weaken this plan’s tests to hide it.

## File Structure Map

### Observability

- Modify: `/Users/viv/projs/axiom-arb/crates/observability/src/conventions.rs`
  Responsibility: add runtime-phase span names, field keys, and typed `ReconcileReason` dimension vocabulary.
- Modify: `/Users/viv/projs/axiom-arb/crates/observability/src/lib.rs`
  Responsibility: continue re-exporting repo-owned conventions if new items require root exposure.
- Modify: `/Users/viv/projs/axiom-arb/crates/observability/tests/conventions.rs`
  Responsibility: lock the new runtime span names, field keys, and typed dimension pairs.

### App Live Runtime

- Create: `/Users/viv/projs/axiom-arb/crates/app-live/src/instrumentation.rs`
  Responsibility: one focused adapter that converts `ReconcileAttention`, dispatch summaries, runtime state, and rollout evidence into repo-owned spans and metric writes.
- Modify: `/Users/viv/projs/axiom-arb/crates/app-live/src/lib.rs`
  Responsibility: export the new instrumentation surface and any new instrumented run helpers.
- Modify: `/Users/viv/projs/axiom-arb/crates/app-live/src/runtime.rs`
  Responsibility: emit reconcile/apply/publish snapshot spans and delegate metric recording through the adapter.
- Modify: `/Users/viv/projs/axiom-arb/crates/app-live/src/supervisor.rs`
  Responsibility: emit resume/dispatch/rollout-evidence signals and backlog gauges.
- Modify: `/Users/viv/projs/axiom-arb/crates/app-live/src/main.rs`
  Responsibility: bootstrap observability once, pass the recorder into the instrumented runtime path, and keep entrypoint structured logging behavior intact.

### App Live Tests

- Create: `/Users/viv/projs/axiom-arb/crates/app-live/tests/support/mod.rs`
  Responsibility: shared helpers for capturing local `tracing` output in library-level integration tests.
- Create: `/Users/viv/projs/axiom-arb/crates/app-live/tests/runtime_observability.rs`
  Responsibility: verify reconcile/apply/publish paths emit the expected metrics and runtime spans.
- Create: `/Users/viv/projs/axiom-arb/crates/app-live/tests/supervisor_observability.rs`
  Responsibility: verify resume/dispatch/rollout metrics and spans without depending on the binary entrypoint.
- Modify: `/Users/viv/projs/axiom-arb/crates/app-live/tests/main_entrypoint.rs`
  Responsibility: preserve the phase-1 entrypoint contract after the recorder gets threaded into runtime execution.

### Docs

- Modify: `/Users/viv/projs/axiom-arb/README.md`
  Responsibility: describe that `app-live` now emits runtime-local structured signals beyond the entrypoint while remaining local-only and OTel-compatible.

## Task 1: Extend Runtime Observability Vocabulary

**Files:**
- Modify: `crates/observability/src/conventions.rs`
- Modify: `crates/observability/tests/conventions.rs`

- [ ] **Step 1: Write the failing conventions test**

```rust
use observability::{field_keys, metric_dimensions, span_names};

#[test]
fn runtime_observability_conventions_define_runtime_spans_fields_and_reconcile_reasons() {
    assert_eq!(span_names::APP_RUNTIME_RECONCILE, "axiom.app.runtime.reconcile");
    assert_eq!(span_names::APP_RUNTIME_PUBLISH_SNAPSHOT, "axiom.app.runtime.publish_snapshot");
    assert_eq!(span_names::APP_SUPERVISOR_RESUME, "axiom.app.supervisor.resume");
    assert_eq!(span_names::APP_DISPATCH_FLUSH, "axiom.app.dispatch.flush");

    assert_eq!(field_keys::STATE_VERSION, "state_version");
    assert_eq!(field_keys::JOURNAL_SEQ, "journal_seq");
    assert_eq!(field_keys::SNAPSHOT_ID, "snapshot_id");
    assert_eq!(field_keys::PENDING_RECONCILE_COUNT, "pending_reconcile_count");
    assert_eq!(field_keys::ATTENTION_REASON, "attention_reason");
    assert_eq!(field_keys::BACKLOG_COUNT, "backlog_count");

    assert_eq!(
        metric_dimensions::ReconcileReason::IdentifierMismatch.as_pair(),
        ("reason", "identifier_mismatch")
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p observability runtime_observability_conventions_define_runtime_spans_fields_and_reconcile_reasons -- --exact`

Expected: FAIL because the runtime span names, field keys, and `ReconcileReason` vocabulary do not exist yet.

- [ ] **Step 3: Implement the minimal vocabulary additions**

```rust
pub mod span_names {
    pub const APP_RUNTIME_RECONCILE: &str = "axiom.app.runtime.reconcile";
    pub const APP_RUNTIME_APPLY_INPUT: &str = "axiom.app.runtime.apply_input";
    pub const APP_RUNTIME_PUBLISH_SNAPSHOT: &str = "axiom.app.runtime.publish_snapshot";
    pub const APP_SUPERVISOR_RESUME: &str = "axiom.app.supervisor.resume";
    pub const APP_DISPATCH_FLUSH: &str = "axiom.app.dispatch.flush";
}

pub mod field_keys {
    pub const STATE_VERSION: &str = "state_version";
    pub const JOURNAL_SEQ: &str = "journal_seq";
    pub const SNAPSHOT_ID: &str = "snapshot_id";
    pub const PENDING_RECONCILE_COUNT: &str = "pending_reconcile_count";
    pub const ATTENTION_REASON: &str = "attention_reason";
    pub const BACKLOG_COUNT: &str = "backlog_count";
    pub const APPLY_RESULT: &str = "apply_result";
}

pub mod metric_dimensions {
    pub enum ReconcileReason {
        DuplicateSignedOrder,
        IdentifierMismatch,
        MissingRemoteOrder,
        UnexpectedRemoteOrder,
        OrderStateMismatch,
        ApprovalMismatch,
        ResolutionMismatch,
        RelayerTxMismatch,
        InventoryMismatch,
    }
}
```

Implementation notes:
- Keep this repo-owned and finite.
- Use `("reason", "...")` pairs so the later OTel adapter can translate them without changing call sites.
- Do not add raw backend attribute builders.

- [ ] **Step 4: Run the conventions suite**

Run: `cargo test -p observability conventions -- --nocapture`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/observability/src/conventions.rs crates/observability/tests/conventions.rs
git commit -m "feat: add runtime observability conventions"
```

## Task 2: Add A Focused `app-live` Instrumentation Adapter

**Files:**
- Create: `crates/app-live/src/instrumentation.rs`
- Modify: `crates/app-live/src/lib.rs`
- Create: `crates/app-live/tests/support/mod.rs`
- Create: `crates/app-live/tests/runtime_observability.rs`

- [ ] **Step 1: Write the failing instrumentation test**

```rust
use app_live::AppInstrumentation;
use domain::{ConditionId, TokenId};
use observability::{
    bootstrap_observability,
    metric_dimensions,
    MetricDimension,
    MetricDimensions,
};
use state::ReconcileAttention;

#[test]
fn instrumentation_maps_reconcile_attention_into_repo_owned_dimensions() {
    let observability = bootstrap_observability("app-live-test");
    let instrumentation = AppInstrumentation::enabled(observability.recorder());

    instrumentation.record_reconcile_attention(&ReconcileAttention::IdentifierMismatch {
        token_id: TokenId::from("token-yes"),
        expected_condition_id: ConditionId::from("condition-a"),
        remote_condition_id: ConditionId::from("condition-b"),
    });

    let dims = MetricDimensions::new([MetricDimension::ReconcileReason(
        metric_dimensions::ReconcileReason::IdentifierMismatch,
    )]);

    assert_eq!(
        observability.registry().snapshot().counter_with_dimensions(
            observability.metrics().reconcile_attention_total.key(),
            &dims,
        ),
        Some(1)
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p app-live instrumentation_maps_reconcile_attention_into_repo_owned_dimensions -- --exact`

Expected: FAIL because `AppInstrumentation` does not exist and `MetricDimension::ReconcileReason` is not wired yet.

- [ ] **Step 3: Add the adapter and a tiny tracing-capture helper**

```rust
#[derive(Debug, Clone, Default)]
pub struct AppInstrumentation {
    recorder: Option<RuntimeMetricsRecorder>,
}

impl AppInstrumentation {
    pub fn disabled() -> Self { ... }

    pub fn enabled(recorder: RuntimeMetricsRecorder) -> Self {
        Self { recorder: Some(recorder) }
    }

    pub fn record_reconcile_attention(&self, attention: &ReconcileAttention) {
        let Some(recorder) = &self.recorder else { return; };
        recorder.increment_reconcile_attention_total(
            1,
            MetricDimensions::new([MetricDimension::ReconcileReason(
                reconcile_reason(attention),
            )]),
        );
    }
}
```

Implementation notes:
- Keep all conversion logic in `instrumentation.rs`; do not duplicate `ReconcileAttention -> ReconcileReason` matches in `runtime.rs` and `supervisor.rs`.
- `tests/support/mod.rs` should expose a tiny helper for capturing `tracing` output with `tracing::subscriber::with_default(...)`. Reuse it in later tasks instead of repeating ad hoc subscriber setup.
- Keep the adapter optional so unit/integration tests can still construct no-op runtimes cheaply.

- [ ] **Step 4: Run the focused test**

Run: `cargo test -p app-live instrumentation_maps_reconcile_attention_into_repo_owned_dimensions -- --exact`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/instrumentation.rs crates/app-live/src/lib.rs crates/app-live/tests/support/mod.rs crates/app-live/tests/runtime_observability.rs
git commit -m "feat: add app-live instrumentation adapter"
```

## Task 3: Instrument `AppRuntime` Reconcile, Apply, And Snapshot Publication

**Files:**
- Modify: `crates/app-live/src/runtime.rs`
- Modify: `crates/app-live/tests/runtime_observability.rs`

- [ ] **Step 1: Write the failing runtime observability tests**

```rust
use app_live::{AppInstrumentation, AppRuntime, AppRuntimeMode};
use chrono::Utc;
use domain::{ConditionId, ExternalFactEvent, TokenId};
use observability::{bootstrap_observability, field_keys, span_names};
use state::{ReconcileAttention, RemoteSnapshot};

mod support;

#[test]
fn reconcile_failure_emits_runtime_reconcile_span_and_attention_metric() {
    let observability = bootstrap_observability("app-live-test");
    let instrumentation = AppInstrumentation::enabled(observability.recorder());
    let mut runtime = AppRuntime::new_instrumented(AppRuntimeMode::Live, instrumentation);

    let (captured, report) = support::capture_tracing(|| {
        runtime.reconcile(RemoteSnapshot::empty().with_attention(
            ReconcileAttention::IdentifierMismatch {
                token_id: TokenId::from("token-yes"),
                expected_condition_id: ConditionId::from("condition-a"),
                remote_condition_id: ConditionId::from("condition-b"),
            },
        ))
    });

    assert!(!report.succeeded);
    assert!(captured.contains(span_names::APP_RUNTIME_RECONCILE));
    assert!(captured.contains(field_keys::PENDING_RECONCILE_COUNT));
}

#[test]
fn publish_snapshot_emits_snapshot_span_with_snapshot_identity() {
    let observability = bootstrap_observability("app-live-test");
    let instrumentation = AppInstrumentation::enabled(observability.recorder());
    let mut runtime = AppRuntime::new_instrumented(AppRuntimeMode::Paper, instrumentation);
    runtime.reconcile(RemoteSnapshot::empty());
    runtime
        .apply_input(app_live::InputTaskEvent::new(
            1,
            ExternalFactEvent::new("market_ws", "session-1", "evt-1", "v1", Utc::now()),
        ))
        .unwrap();

    let (captured, snapshot) = support::capture_tracing(|| runtime.publish_snapshot("snapshot-1"));

    assert!(snapshot.is_some());
    assert!(captured.contains(span_names::APP_RUNTIME_PUBLISH_SNAPSHOT));
    assert!(captured.contains("snapshot_id=snapshot-1"));
}
```

- [ ] **Step 2: Run the runtime observability tests to verify failure**

Run: `cargo test -p app-live --test runtime_observability -- --nocapture`

Expected: FAIL because `AppRuntime::new_instrumented` does not exist and runtime methods do not emit spans or metrics yet.

- [ ] **Step 3: Implement minimal runtime instrumentation**

```rust
pub struct AppRuntime {
    store: StateStore,
    app_mode: AppRuntimeMode,
    published_snapshot: Option<PublishedSnapshot>,
    instrumentation: AppInstrumentation,
}

impl AppRuntime {
    pub fn new(app_mode: AppRuntimeMode) -> Self {
        Self::new_instrumented(app_mode, AppInstrumentation::disabled())
    }

    pub fn new_instrumented(app_mode: AppRuntimeMode, instrumentation: AppInstrumentation) -> Self {
        Self { ..., instrumentation }
    }

    pub fn reconcile(&mut self, snapshot: RemoteSnapshot) -> ReconcileReport {
        let span = tracing::info_span!(
            span_names::APP_RUNTIME_RECONCILE,
            app_mode = self.app_mode.as_str(),
            pending_reconcile_count = self.store.pending_reconcile_count(),
        );
        let _guard = span.enter();
        let report = bootstrap::reconcile(&mut self.store, snapshot);
        self.instrumentation.record_reconcile_report(&report, &self.store);
        report
    }
}
```

Implementation notes:
- Keep `run_live` / `run_paper` behavior stable by making them call the existing constructor with `AppInstrumentation::disabled()`.
- Instrument `apply_input` with `span_names::APP_RUNTIME_APPLY_INPUT` and an `apply_result` field. Only record state/dirty-set outcomes that already exist; do not invent new business states.
- Instrument `publish_snapshot` with `span_names::APP_RUNTIME_PUBLISH_SNAPSHOT`, `snapshot_id`, `state_version`, and `journal_seq`.

- [ ] **Step 4: Run the runtime observability tests again**

Run: `cargo test -p app-live --test runtime_observability -- --nocapture`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/runtime.rs crates/app-live/tests/runtime_observability.rs
git commit -m "feat: instrument app-live runtime signals"
```

## Task 4: Instrument Supervisor Resume, Dispatch, And Rollout-Gate Metrics

**Files:**
- Modify: `crates/app-live/src/supervisor.rs`
- Create: `crates/app-live/tests/supervisor_observability.rs`

- [ ] **Step 1: Write the failing supervisor observability tests**

```rust
use app_live::{AppInstrumentation, AppSupervisor, NegRiskRolloutEvidence};
use observability::{bootstrap_observability, metric_dimensions, MetricDimension, MetricDimensions};

mod support;

#[test]
fn resume_records_recovery_backlog_and_rollout_gate_metrics() {
    let observability = bootstrap_observability("app-live-test");
    let instrumentation = AppInstrumentation::enabled(observability.recorder());
    let mut supervisor = AppSupervisor::for_tests_instrumented(instrumentation);
    supervisor.seed_runtime_progress(41, 7, Some("snapshot-7"));
    supervisor.seed_committed_state_version(7);
    supervisor.seed_pending_reconcile_count(0);
    supervisor.seed_neg_risk_rollout_evidence(NegRiskRolloutEvidence {
        snapshot_id: "snapshot-7".to_owned(),
        live_ready_family_count: 3,
        blocked_family_count: 2,
        parity_mismatch_count: 1,
    });

    let (captured, summary) = support::capture_tracing(|| supervisor.resume_once().unwrap());
    let snapshot = observability.registry().snapshot();

    assert!(captured.contains("axiom.app.supervisor.resume"));
    assert!(captured.contains("axiom.app.dispatch.flush"));
    assert_eq!(
        snapshot.gauge(observability.metrics().neg_risk_live_ready_family_count.key()),
        Some(3.0)
    );
    assert_eq!(
        snapshot.gauge(observability.metrics().neg_risk_live_gate_block_count.key()),
        Some(2.0)
    );
    assert_eq!(
        snapshot.counter(observability.metrics().neg_risk_rollout_parity_mismatch_count.key()),
        Some(1)
    );
    assert_eq!(summary.pending_reconcile_count, 0);
}
```

- [ ] **Step 2: Run the supervisor observability test to verify failure**

Run: `cargo test -p app-live --test supervisor_observability -- --nocapture`

Expected: FAIL because there is no instrumented supervisor constructor and resume/dispatch paths do not emit these signals yet.

- [ ] **Step 3: Add supervisor-level signal emission**

```rust
pub struct AppSupervisor {
    dispatcher: DispatchLoop,
    runtime: AppRuntime,
    instrumentation: AppInstrumentation,
    ...
}

impl AppSupervisor {
    pub fn new(app_mode: AppRuntimeMode, bootstrap_snapshot: RemoteSnapshot) -> Self {
        Self::new_instrumented(app_mode, bootstrap_snapshot, AppInstrumentation::disabled())
    }

    pub fn new_instrumented(
        app_mode: AppRuntimeMode,
        bootstrap_snapshot: RemoteSnapshot,
        instrumentation: AppInstrumentation,
    ) -> Self { ... }
}
```

Implementation notes:
- Record `recovery_backlog_count` from `input_tasks.len()` before and after replay/removal boundaries in `resume_once`.
- Record `dispatcher_backlog_count` and `projection_publish_lag_count` from `DispatchSummary` after every flush.
- Record rollout-gate gauges/counters from `NegRiskRolloutEvidence` whenever `publish_current_snapshot()` rebuilds evidence or `summary()` exposes it.
- Emit `span_names::APP_SUPERVISOR_RESUME` and `span_names::APP_DISPATCH_FLUSH` using the field keys added in Task 1.
- Do not change dispatch semantics or rollout-gate rules; instrumentation must be observational only.

- [ ] **Step 4: Run the supervisor observability test**

Run: `cargo test -p app-live --test supervisor_observability -- --nocapture`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/supervisor.rs crates/app-live/tests/supervisor_observability.rs
git commit -m "feat: instrument supervisor and dispatch signals"
```

## Task 5: Wire `app-live` Entrypoint And Refresh README

**Files:**
- Modify: `crates/app-live/src/main.rs`
- Modify: `crates/app-live/tests/main_entrypoint.rs`
- Modify: `README.md`

- [ ] **Step 1: Extend the entrypoint regression test first**

```rust
#[test]
fn binary_entrypoint_keeps_structured_bootstrap_output_after_runtime_instrumentation() {
    let output = app_live_output("paper");
    assert!(output.status.success());

    let combined = format!(
        "{}{}",
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    );

    assert!(combined.contains(span_names::APP_BOOTSTRAP_COMPLETE));
    assert!(combined.contains("pending_reconcile_count=0"));
    assert!(!combined.contains("app-live starting "));
}
```

- [ ] **Step 2: Run the entrypoint test to verify failure**

Run: `cargo test -p app-live --test main_entrypoint -- --nocapture`

Expected: FAIL once the runtime API requires recorder threading and `main.rs` has not been updated yet.

- [ ] **Step 3: Thread the recorder into the instrumented runtime path and update README**

```rust
let observability = bootstrap_observability("app-live");
let result = match app_mode {
    AppRuntimeMode::Paper => run_paper_instrumented(&source, observability.recorder()),
    AppRuntimeMode::Live => run_live_instrumented(&source, observability.recorder()),
};
```

README update requirements:
- say `app-live` now emits runtime-local structured spans and metric-backed signals beyond the entrypoint
- keep the current limitation explicit: this is still local-only and does not add any OTel exporter
- do not claim websocket/heartbeat/order execution loops are instrumented if those loops do not yet exist in the binary

- [ ] **Step 4: Run the entrypoint regression test**

Run: `cargo test -p app-live --test main_entrypoint -- --nocapture`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/main.rs crates/app-live/tests/main_entrypoint.rs README.md
git commit -m "docs: wire app-live runtime observability"
```

## Task 6: Final Verification

**Files:**
- No new files; verification only

- [ ] **Step 1: Run focused observability verification**

Run:

```bash
cargo test -p observability
cargo test -p app-live --test runtime_observability -- --nocapture
cargo test -p app-live --test supervisor_observability -- --nocapture
cargo test -p app-live --test main_entrypoint -- --nocapture
```

Expected: PASS.

- [ ] **Step 2: Run compatibility verification for replay entrypoint**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-replay --test main_entrypoint -- --nocapture
```

Expected: PASS because this phase must not regress the phase-1 replay entrypoint contract.

- [ ] **Step 3: Run package-level verification only if the branch baseline is already green**

Run:

```bash
cargo test -p app-live
```

Expected:
- PASS if the unrelated rollout-gate baseline regression has already been fixed on the execution branch
- otherwise FAIL on the known pre-existing `fault_injection` case; surface that separately instead of attributing it to this plan

- [ ] **Step 4: Commit the verification-only state if needed**

```bash
git status --short
```

Expected: no uncommitted changes beyond already committed task work.

## Notes For Execution

- Use `@superpowers:test-driven-development` on every code task.
- Before claiming a task is done, use `@superpowers:verification-before-completion`.
- Do not introduce any `opentelemetry`, `tracing-opentelemetry`, or OTLP exporter dependency in this plan.
- If you discover during execution that `app-live` runtime instrumentation naturally wants to reach into `state`, `execution`, or `recovery`, stop and split that into a separate follow-on plan instead of broadening this one mid-flight.
