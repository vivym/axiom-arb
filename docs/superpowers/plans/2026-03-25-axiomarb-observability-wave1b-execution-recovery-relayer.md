# AxiomArb Observability Wave 1B Execution Recovery And Relayer Producer Wiring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the second executable slice of the observability roadmap by wiring truthful execution, recovery, and relayer producer signals into the existing repo-owned `tracing` and metric surface without introducing any OTel exporter or pretending `app-live` already has full live venue loops.

**Architecture:** Keep this phase narrow and producer-driven. Extend the repo-owned observability vocabulary only for signals that have real producers today, add focused instrumentation adapters in `execution`, `app-live`, and `venue-polymarket`, and verify them through library integration tests that use the existing in-process registry plus captured `tracing` output. Do not fabricate `unknown-order`, `broken-leg`, `stale relayer transaction`, or halt-activation producers until the runtime actually has authoritative emitters for them.

**Tech Stack:** Rust, `tracing`, existing `observability` facade and `MetricRegistry`, `chrono`, `tokio`, `reqwest`, library integration tests with local listener mocks

---

## Scope Boundary

The observability roadmap remains too large for one executable plan. `Wave 1` should continue to be decomposed into honest, shippable slices:

- `Wave 1A` (already done): websocket session + heartbeat producer observability
- `Wave 1B` (this plan): execution / recovery / relayer producer observability
- `Wave 1C` (later): neg-risk metadata discovery / refresh / halt producer observability
- later plans: multi-process contracts, OTel backend adapter, collector integration, production operations

This document covers `Wave 1B` only. It must produce working, testable software on its own.

- In scope: execution-attempt lifecycle spans, truthful `shadow_attempt_count` emission, recovery divergence spans plus `divergence_count`, relayer recent-transaction producer spans plus `relayer_pending_age`, and README alignment.
- Out of scope: `unknown-order`, `broken-leg inventory`, `stale relayer transaction`, and `halt_activation_total` emitters; neg-risk metadata discovery/halt producers; OTel exporters; collector config; dashboards; alerts; and any change to trading behavior or retry policy semantics.
- Known honesty rule: if a signal only exists as a contract today and not a truthful producer, leave it untouched in this plan.

## File Structure Map

### Root Docs

- Modify: `/Users/viv/projs/axiom-arb/README.md`
  Responsibility: explain that the repository now emits execution, recovery, and relayer producer signals locally, while still not claiming OTel export or full live runtime wiring.

### Observability

- Modify: `/Users/viv/projs/axiom-arb/crates/observability/src/conventions.rs`
  Responsibility: add stable span names and field keys for execution attempts, recovery divergence, and relayer polling.
- Modify: `/Users/viv/projs/axiom-arb/crates/observability/tests/conventions.rs`
  Responsibility: lock the new repo-owned vocabulary.

### Execution

- Modify: `/Users/viv/projs/axiom-arb/crates/execution/Cargo.toml`
  Responsibility: add the minimum `observability` and `tracing` dependencies needed for local producer instrumentation.
- Create: `/Users/viv/projs/axiom-arb/crates/execution/src/instrumentation.rs`
  Responsibility: own optional execution observability state and translation into repo-owned spans and metrics.
- Modify: `/Users/viv/projs/axiom-arb/crates/execution/src/lib.rs`
  Responsibility: export the new execution instrumentation surface.
- Modify: `/Users/viv/projs/axiom-arb/crates/execution/src/orchestrator.rs`
  Responsibility: emit truthful execution-attempt spans around plan execution without changing planning or sink semantics.
- Modify: `/Users/viv/projs/axiom-arb/crates/execution/src/sink.rs`
  Responsibility: expose a stable sink identity for instrumentation and record truthful shadow-attempt metrics only when the shadow sink actually accepts an attempt.
- Create: `/Users/viv/projs/axiom-arb/crates/execution/tests/support/mod.rs`
  Responsibility: share span capture plus minimal sample planning-input / failing-sink helpers across execution integration tests.
- Modify: `/Users/viv/projs/axiom-arb/crates/execution/tests/orchestrator.rs`
  Responsibility: keep the existing orchestrator test sink compiling after the `VenueSink` instrumentation contract grows a stable sink identity method.
- Create: `/Users/viv/projs/axiom-arb/crates/execution/tests/observability.rs`
  Responsibility: verify execution-attempt spans and `shadow_attempt_count` emission.

### App Live Execution And Recovery

- Modify: `/Users/viv/projs/axiom-arb/crates/app-live/src/negrisk_live.rs`
  Responsibility: route the current bootstrap-time neg-risk live submission path through the instrumented execution surface instead of bypassing the orchestrator directly.
- Modify: `/Users/viv/projs/axiom-arb/crates/app-live/tests/support/mod.rs`
  Responsibility: add reusable span-capture helpers for app-level observability tests.
- Modify: `/Users/viv/projs/axiom-arb/crates/app-live/tests/negrisk_live_rollout.rs`
  Responsibility: lock that the current bootstrap-time neg-risk live path still produces the same artifacts while also emitting an execution-attempt span.

- Modify: `/Users/viv/projs/axiom-arb/crates/app-live/src/instrumentation.rs`
  Responsibility: add focused helpers for recovery divergence signals without pushing conversion logic into supervisor code.
- Modify: `/Users/viv/projs/axiom-arb/crates/app-live/src/supervisor.rs`
  Responsibility: pass optional execution instrumentation into the bootstrap-time neg-risk live path, and emit divergence spans plus `divergence_count` on real resume/rebuild mismatches.
- Modify: `/Users/viv/projs/axiom-arb/crates/app-live/tests/supervisor_observability.rs`
  Responsibility: lock divergence span fields and divergence counter behavior on resume failures.

### Venue Polymarket Relayer

- Modify: `/Users/viv/projs/axiom-arb/crates/venue-polymarket/src/instrumentation.rs`
  Responsibility: translate recent relayer transaction batches into repo-owned relayer spans and `relayer_pending_age`.
- Modify: `/Users/viv/projs/axiom-arb/crates/venue-polymarket/src/relayer.rs`
  Responsibility: expose the smallest truthful relayer observation helpers and optionally an instrumented fetch path over the existing REST client.
- Modify: `/Users/viv/projs/axiom-arb/crates/venue-polymarket/src/lib.rs`
  Responsibility: export any new relayer producer surface.
- Modify: `/Users/viv/projs/axiom-arb/crates/venue-polymarket/tests/support/mod.rs`
  Responsibility: keep shared span capture and add the local-listener mock helpers required by relayer producer tests.
- Create: `/Users/viv/projs/axiom-arb/crates/venue-polymarket/tests/relayer_observability.rs`
  Responsibility: verify relayer producer spans and `relayer_pending_age` using a local listener mock plus captured tracing output.

## Task 1: Add Execution Recovery And Relayer Observability Vocabulary

**Files:**
- Modify: `crates/observability/src/conventions.rs`
- Modify: `crates/observability/tests/conventions.rs`

- [ ] **Step 1: Write the failing conventions test**

```rust
use observability::{field_keys, span_names};

#[test]
fn execution_recovery_and_relayer_conventions_are_repo_owned() {
    assert_eq!(span_names::EXECUTION_ATTEMPT, "axiom.execution.attempt");
    assert_eq!(span_names::APP_RECOVERY_DIVERGENCE, "axiom.app.recovery.divergence");
    assert_eq!(span_names::VENUE_RELAYER_POLL, "axiom.venue.relayer.poll");

    assert_eq!(field_keys::EXECUTION_MODE, "execution_mode");
    assert_eq!(field_keys::ROUTE, "route");
    assert_eq!(field_keys::SCOPE, "scope");
    assert_eq!(field_keys::PLAN_ID, "plan_id");
    assert_eq!(field_keys::ATTEMPT_ID, "attempt_id");
    assert_eq!(field_keys::ATTEMPT_NO, "attempt_no");
    assert_eq!(field_keys::ATTEMPT_OUTCOME, "attempt_outcome");
    assert_eq!(field_keys::SINK_KIND, "sink_kind");
    assert_eq!(field_keys::DIVERGENCE_KIND, "divergence_kind");
    assert_eq!(field_keys::RELAYER_TX_COUNT, "relayer_tx_count");
    assert_eq!(field_keys::PENDING_TX_COUNT, "pending_tx_count");
    assert_eq!(field_keys::PENDING_AGE_SECONDS, "pending_age_seconds");
}
```

- [ ] **Step 2: Run the test to verify failure**

Run:

```bash
cargo test -p observability execution_recovery_and_relayer_conventions_are_repo_owned -- --exact
```

Expected: FAIL because the new span names and field keys do not exist yet.

- [ ] **Step 3: Implement the minimal vocabulary additions**

```rust
pub mod span_names {
    pub const EXECUTION_ATTEMPT: &str = "axiom.execution.attempt";
    pub const APP_RECOVERY_DIVERGENCE: &str = "axiom.app.recovery.divergence";
    pub const VENUE_RELAYER_POLL: &str = "axiom.venue.relayer.poll";
}

pub mod field_keys {
    pub const EXECUTION_MODE: &str = "execution_mode";
    pub const ROUTE: &str = "route";
    pub const SCOPE: &str = "scope";
    pub const PLAN_ID: &str = "plan_id";
    pub const ATTEMPT_ID: &str = "attempt_id";
    pub const ATTEMPT_NO: &str = "attempt_no";
    pub const ATTEMPT_OUTCOME: &str = "attempt_outcome";
    pub const SINK_KIND: &str = "sink_kind";
    pub const DIVERGENCE_KIND: &str = "divergence_kind";
    pub const RELAYER_TX_COUNT: &str = "relayer_tx_count";
    pub const PENDING_TX_COUNT: &str = "pending_tx_count";
    pub const PENDING_AGE_SECONDS: &str = "pending_age_seconds";
}
```

Implementation notes:

- keep these names repo-owned so later OTel export maps from one source of truth
- do not add backend-specific attribute builders
- do not add halt-specific or unknown-order vocabulary in this task; those producers do not exist yet

- [ ] **Step 4: Run the conventions suite**

Run:

```bash
cargo test -p observability conventions -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/observability/src/conventions.rs crates/observability/tests/conventions.rs
git commit -m "feat: add wave1b observability conventions"
```

## Task 2: Add Truthful Execution Producer Instrumentation

**Files:**
- Modify: `crates/execution/Cargo.toml`
- Create: `crates/execution/src/instrumentation.rs`
- Modify: `crates/execution/src/lib.rs`
- Modify: `crates/execution/src/orchestrator.rs`
- Modify: `crates/execution/src/sink.rs`
- Create: `crates/execution/tests/support/mod.rs`
- Modify: `crates/execution/tests/orchestrator.rs`
- Create: `crates/execution/tests/observability.rs`
- Modify: `crates/app-live/src/negrisk_live.rs`
- Modify: `crates/app-live/src/supervisor.rs`
- Modify: `crates/app-live/tests/support/mod.rs`
- Modify: `crates/app-live/tests/negrisk_live_rollout.rs`

- [ ] **Step 1: Write the failing execution observability tests**

```rust
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

    assert_eq!(receipt.outcome, domain::ExecutionAttemptOutcome::ShadowRecorded);
    assert_eq!(
        observability.registry().snapshot().counter(
            observability.metrics().shadow_attempt_count.key()
        ),
        Some(1)
    );

    let attempt_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::EXECUTION_ATTEMPT)
        .expect("execution attempt span missing");
    assert_eq!(
        attempt_span.field(field_keys::EXECUTION_MODE).map(String::as_str),
        Some("\"shadow\"")
    );
    assert_eq!(
        attempt_span.field(field_keys::ATTEMPT_OUTCOME).map(String::as_str),
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
        observability.registry().snapshot().counter(
            observability.metrics().shadow_attempt_count.key()
        ),
        None
    );

    let attempt_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::EXECUTION_ATTEMPT)
        .expect("execution attempt span missing");
    assert_eq!(
        attempt_span.field(field_keys::ATTEMPT_OUTCOME).map(String::as_str),
        Some("\"sink_error\"")
    );
}
```

```rust
mod support;

use std::collections::BTreeMap;

use app_live::{AppRuntimeMode, AppSupervisor, NegRiskFamilyLiveTarget, NegRiskMemberLiveTarget};
use observability::{bootstrap_observability, field_keys, span_names};
use rust_decimal::Decimal;
use state::RemoteSnapshot;
use support::capture_spans;

#[test]
fn bootstrap_neg_risk_live_path_emits_execution_attempt_span_without_changing_artifacts() {
    let observability = bootstrap_observability("app-live-test");
    let mut supervisor =
        AppSupervisor::new_instrumented(AppRuntimeMode::Live, RemoteSnapshot::empty(), observability.recorder());
    supervisor.seed_neg_risk_live_targets(std::collections::BTreeMap::from([(
        "family-a".to_owned(),
        NegRiskFamilyLiveTarget {
            family_id: "family-a".to_owned(),
            members: vec![NegRiskMemberLiveTarget {
                condition_id: "condition-1".to_owned(),
                token_id: "token-1".to_owned(),
                price: Decimal::new(45, 2),
                quantity: Decimal::new(10, 0),
            }],
        },
    )]));
    supervisor.seed_neg_risk_live_approval("family-a");
    supervisor.seed_neg_risk_live_ready_family("family-a");

    let (captured_spans, summary) = capture_spans(|| supervisor.run_once().unwrap());

    assert_eq!(summary.neg_risk_live_attempt_count, 1);
    let attempt_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::EXECUTION_ATTEMPT)
        .expect("execution attempt span missing");
    assert_eq!(
        attempt_span.field(field_keys::EXECUTION_MODE).map(String::as_str),
        Some("\"live\"")
    );
    assert_eq!(
        attempt_span.field(field_keys::SCOPE).map(String::as_str),
        Some("\"family-a\"")
    );
}
```

- [ ] **Step 2: Run the tests to verify failure**

Run:

```bash
cargo test -p execution instrumented_shadow_execution_records_span_fields_and_shadow_counter -- --exact
cargo test -p execution instrumented_execution_failure_records_sink_error_without_shadow_counter_growth -- --exact
cargo test -p app-live bootstrap_neg_risk_live_path_emits_execution_attempt_span_without_changing_artifacts -- --exact
```

Expected: FAIL because `ExecutionInstrumentation`, the instrumented orchestrator path, and the bootstrap-time neg-risk execution wiring do not exist yet.

- [ ] **Step 3: Implement the focused execution instrumentation surface**

```rust
pub trait VenueSink {
    fn sink_kind(&self) -> &'static str;

    fn execute(
        &self,
        plan: &ExecutionPlan,
        attempt: &ExecutionAttemptContext,
    ) -> Result<ExecutionReceipt, VenueSinkError>;
}

#[derive(Debug, Clone, Default)]
pub struct ExecutionInstrumentation {
    recorder: Option<RuntimeMetricsRecorder>,
}

impl ExecutionInstrumentation {
    pub fn disabled() -> Self { ... }
    pub fn enabled(recorder: RuntimeMetricsRecorder) -> Self { ... }

    pub fn record_attempt_start(
        &self,
        plan: &ExecutionPlan,
        attempt: &ExecutionAttemptContext,
        sink_kind: &'static str,
    ) -> tracing::Span { ... }

    pub fn record_attempt_outcome(&self, span: &tracing::Span, outcome: &str) { ... }

    pub fn record_shadow_attempt(&self) {
        if let Some(recorder) = &self.recorder {
            recorder.increment_shadow_attempt_count(1);
        }
    }
}
```

Implementation notes:

- keep existing `ExecutionOrchestrator::new` and `with_attempt_factory` as non-instrumented convenience constructors
- add `new_instrumented` and `with_attempt_factory_instrumented` instead of forcing instrumentation on all callers
- make `sink_kind` an explicit `VenueSink` contract so generic orchestrator code can emit a stable repo-owned sink label
- extract the minimal span-capture and sample-input helpers into `crates/execution/tests/support/mod.rs`; do not try to import private helpers from `crates/execution/tests/orchestrator.rs`
- update the existing `FailingVenueSink` impl in `crates/execution/tests/orchestrator.rs` to satisfy the new `sink_kind` contract
- emit the execution span once around the actual sink call; do not duplicate the span inside both orchestrator and sink
- increment `shadow_attempt_count` only when `ShadowVenueSink` actually records the attempt
- route `app-live/src/negrisk_live.rs` through `ExecutionOrchestrator::new_instrumented(...)` so the current bootstrap-time live family submission path does not bypass execution producer observability
- do not invent new execution metrics in this task

- [ ] **Step 4: Run the execution crate tests**

Run:

```bash
cargo test -p execution observability -- --nocapture
cargo test -p execution orchestrator -- --nocapture
cargo test -p app-live negrisk_live_rollout -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/execution/Cargo.toml crates/execution/src/instrumentation.rs crates/execution/src/lib.rs crates/execution/src/orchestrator.rs crates/execution/src/sink.rs crates/execution/tests/support/mod.rs crates/execution/tests/orchestrator.rs crates/execution/tests/observability.rs crates/app-live/src/negrisk_live.rs crates/app-live/src/supervisor.rs crates/app-live/tests/support/mod.rs crates/app-live/tests/negrisk_live_rollout.rs
git commit -m "feat: instrument execution producers"
```

## Task 3: Add Recovery Divergence Producer Instrumentation

**Files:**
- Modify: `crates/app-live/src/instrumentation.rs`
- Modify: `crates/app-live/src/supervisor.rs`
- Modify: `crates/app-live/tests/supervisor_observability.rs`

- [ ] **Step 1: Write the failing supervisor divergence test**

```rust
use app_live::AppSupervisor;
use observability::{bootstrap_observability, field_keys, span_names};

#[test]
fn resume_pending_reconcile_mismatch_records_divergence_span_and_counter() {
    let observability = bootstrap_observability("app-live-test");
    let mut supervisor = AppSupervisor::for_tests_instrumented(observability.recorder());
    for journal_seq in 35..=41 {
        supervisor.seed_committed_input(sample_input_task_event(journal_seq));
    }
    supervisor.seed_runtime_progress(41, 7, Some("snapshot-7"));
    supervisor.seed_committed_state_version(7);
    supervisor.seed_pending_reconcile_count(3);

    let (captured_spans, err) = capture_spans(|| supervisor.resume_once().unwrap_err());

    assert!(err.to_string().contains("pending reconcile count"));
    assert_eq!(
        observability
            .registry()
            .snapshot()
            .counter(observability.metrics().divergence_count.key()),
        Some(1)
    );

    let divergence_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::APP_RECOVERY_DIVERGENCE)
        .expect("divergence span missing");
    assert_eq!(
        divergence_span
            .field(field_keys::DIVERGENCE_KIND)
            .map(String::as_str),
        Some("\"pending_reconcile_count_mismatch\"")
    );
}
```

- [ ] **Step 2: Run the test to verify failure**

Run:

```bash
cargo test -p app-live resume_pending_reconcile_mismatch_records_divergence_span_and_counter -- --exact
```

Expected: FAIL because resume divergence currently returns an error without emitting any divergence signal.

- [ ] **Step 3: Implement the smallest truthful divergence producer**

```rust
impl AppInstrumentation {
    pub fn record_divergence(&self, divergence_kind: &'static str) {
        let Some(recorder) = &self.recorder else {
            return;
        };

        tracing::warn_span!(
            span_names::APP_RECOVERY_DIVERGENCE,
            divergence_kind = divergence_kind,
        )
        .in_scope(|| {
            recorder.increment_divergence_count(1);
        });
    }
}
```

Implementation notes:

- call this only on real resume/rebuild mismatches, not on every reconcile attention
- wire it into the mismatch branches that already return `SupervisorError` from `resume_once`, `validate_rollout_evidence_anchor`, and `validate_neg_risk_live_execution_anchor`
- do not reinterpret these errors as runtime-mode transitions or halt activations in this task

- [ ] **Step 4: Run the supervisor observability suite**

Run:

```bash
cargo test -p app-live supervisor_observability -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/instrumentation.rs crates/app-live/src/supervisor.rs crates/app-live/tests/supervisor_observability.rs
git commit -m "feat: instrument recovery divergence producers"
```

## Task 4: Add Relayer Pending-Age Producer Instrumentation

**Files:**
- Modify: `crates/venue-polymarket/src/instrumentation.rs`
- Modify: `crates/venue-polymarket/src/relayer.rs`
- Modify: `crates/venue-polymarket/src/lib.rs`
- Modify: `crates/venue-polymarket/tests/support/mod.rs`
- Create: `crates/venue-polymarket/tests/relayer_observability.rs`

- [ ] **Step 1: Write the failing relayer observability test**

```rust
mod support;

use chrono::{TimeZone, Utc};
use observability::{bootstrap_observability, field_keys, span_names};
use venue_polymarket::{PolymarketRestClient, VenueProducerInstrumentation};
use support::{capture_spans, sample_builder_relayer_auth, sample_client_for, MockServer};

#[test]
fn instrumented_recent_transactions_record_oldest_pending_age() {
    let observability = bootstrap_observability("venue-polymarket-test");
    let instrumentation = VenueProducerInstrumentation::enabled(observability.recorder());
    let server = MockServer::spawn(
        "200 OK",
        r#"[{"transactionID":"tx-pending","state":"STATE_PENDING","createdAt":"2026-03-25T10:00:00Z"},{"transactionID":"tx-confirmed","state":"STATE_CONFIRMED","createdAt":"2026-03-25T10:00:10Z"}]"#,
    );
    let client = sample_client_for(server.base_url());

    let now = Utc.with_ymd_and_hms(2026, 3, 25, 10, 0, 30).single().unwrap();
    let (captured_spans, transactions) = support::capture_spans(|| {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async {
                client
                    .fetch_recent_transactions_instrumented(
                        &sample_builder_relayer_auth(),
                        &instrumentation,
                        now,
                    )
                    .await
                    .unwrap()
            })
    });

    assert_eq!(transactions.len(), 2);
    assert_eq!(
        observability
            .registry()
            .snapshot()
            .gauge(observability.metrics().relayer_pending_age.key()),
        Some(30.0)
    );

    let relayer_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::VENUE_RELAYER_POLL)
        .expect("relayer poll span missing");
    assert_eq!(
        relayer_span
            .field(field_keys::RELAYER_TX_COUNT)
            .map(String::as_str),
        Some("2")
    );
    assert_eq!(
        relayer_span
            .field(field_keys::PENDING_TX_COUNT)
            .map(String::as_str),
        Some("1")
    );
    assert_eq!(
        relayer_span
            .field(field_keys::PENDING_AGE_SECONDS)
            .map(String::as_str),
        Some("30.0")
    );
}
```

- [ ] **Step 2: Run the test to verify failure**

Run:

```bash
cargo test -p venue-polymarket instrumented_recent_transactions_record_oldest_pending_age -- --exact
```

Expected: FAIL because there is no instrumented relayer fetch path or relayer producer span yet.

- [ ] **Step 3: Implement the minimal relayer producer path**

```rust
impl VenueProducerInstrumentation {
    pub fn record_relayer_transactions(
        &self,
        transactions: &[RelayerTransaction],
        observed_at: DateTime<Utc>,
    ) {
        let Some(recorder) = &self.recorder else {
            return;
        };

        let pending = summarize_pending_transactions(transactions, observed_at);
        recorder.record_relayer_pending_age(pending.oldest_pending_age_seconds);

        tracing::info_span!(
            span_names::VENUE_RELAYER_POLL,
            relayer_tx_count = transactions.len(),
            pending_tx_count = pending.pending_tx_count,
            pending_age_seconds = pending.oldest_pending_age_seconds,
        )
        .in_scope(|| {});
    }
}

impl PolymarketRestClient {
    pub async fn fetch_recent_transactions_instrumented(
        &self,
        auth: &RelayerAuth<'_>,
        instrumentation: &VenueProducerInstrumentation,
        observed_at: DateTime<Utc>,
    ) -> Result<Vec<RelayerTransaction>, RestError> {
        let transactions = self.fetch_recent_transactions(auth).await?;
        instrumentation.record_relayer_transactions(&transactions, observed_at);
        Ok(transactions)
    }
}
```

Implementation notes:

- keep the pending-age rule explicit and conservative: only rows whose relayer state is explicitly classified as pending by a helper such as `is_pending_relayer_state(...)` contribute to age
- in this phase, initialize that helper with a narrow allowlist headed by `STATE_PENDING`; terminal, unknown, and unclassified states must not inflate `relayer_pending_age`
- if there are no pending transactions, record `0.0`
- move the local-listener mock and sample relayer auth/client helpers into `crates/venue-polymarket/tests/support/mod.rs`; do not depend on private helpers from `status_and_retry.rs`
- do not invent stale/unknown relayer classifications in this task; that contract remains for a later phase with a real authoritative producer
- keep using a local listener mock; do not add a web framework test dependency

- [ ] **Step 4: Run the venue-polymarket relayer tests**

Run:

```bash
cargo test -p venue-polymarket relayer_observability -- --nocapture
cargo test -p venue-polymarket status_and_retry -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/venue-polymarket/src/instrumentation.rs crates/venue-polymarket/src/relayer.rs crates/venue-polymarket/src/lib.rs crates/venue-polymarket/tests/support/mod.rs crates/venue-polymarket/tests/relayer_observability.rs
git commit -m "feat: instrument relayer producers"
```

## Task 5: Align README And Run Wave 1B Verification

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Update README scope language**

```md
`execution` now emits repo-owned execution-attempt spans and truthful `shadow_attempt_count`.
`app-live` now emits recovery divergence signals for resume/rebuild mismatches.
`venue-polymarket` now exposes relayer recent-transaction producer observability, including local `relayer_pending_age`.

The observability path remains local-only and OTel-compatible rather than OTel-enabled.
This repository state still does not claim `unknown-order`, `broken-leg`, or collector-backed signals.
```

- [ ] **Step 2: Run the targeted verification suite**

Run:

```bash
cargo fmt --all --check
cargo clippy -p observability -p execution -p app-live -p venue-polymarket --all-targets --offline -- -D warnings
cargo test -p observability --offline
cargo test -p execution --offline
cargo test -p app-live --offline
cargo test -p venue-polymarket --offline
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: align wave1b observability scope"
```

## Final Notes

- Keep the implementation TDD-first and resist the temptation to widen scope into `Wave 1C`.
- Do not emit `halt_activation_total` until there is a real runtime producer rather than a pure policy mapping helper.
- Do not add `unknown-order`, `broken-leg inventory`, or `stale relayer transaction` metric handles unless this branch also lands truthful producers for them.
- Preserve local testability: all new signals must remain verifiable with the in-process registry and captured tracing output, with no collector dependency.
