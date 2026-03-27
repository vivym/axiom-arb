# AxiomArb Observability Wave 1C Neg-Risk Control-Plane Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the third executable slice of the observability roadmap by wiring truthful neg-risk control-plane producer observability across metadata refresh, validation/halt persistence boundaries, bootstrap rollout adapters, and replay/postmortem summary output without introducing new business state or any OTel backend work.

**Architecture:** Keep `Wave 1C` boundary-first and producer-driven. Instrument only the control-plane boundaries that already exist today: metadata refresh publication, validation/halt persistence upserts, bootstrap rollout evidence, and persistence-backed replay summaries. Preserve the repo-owned observability facade, use a single emitter per producer boundary, and keep read-side adapters limited to formatting or forwarding already-authoritative data.

**Tech Stack:** Rust, `tracing`, existing `observability` facade and `MetricRegistry`, `tokio`, `sqlx`, `serde_json`, existing local listener mocks, persistence-backed integration tests

---

## Scope Boundary

The observability roadmap remains too large for one executable plan. `Wave 1` must continue to ship in honest slices:

- `Wave 1A` (already done): websocket session + heartbeat producer observability
- `Wave 1B` (already done): execution / recovery / relayer producer observability
- `Wave 1C` (this plan): neg-risk control-plane producer observability
- later plans: multi-process contracts, OTel backend adapter, collector rollout, production operations

This document covers `Wave 1C` only.

- In scope: repo-owned vocabulary for neg-risk control-plane signals; metadata refresh producer signals; validation/exclusion/halt producer signals at validator and persistence boundaries; bootstrap rollout observability alignment in `app-live`; replay summary public-contract alignment in `app-replay`; README alignment.
- Out of scope: new business tables or projections; new stale-halt business semantics; multi-process contracts; dashboards/alerts; OTel exporters or collectors; synthetic metrics emitted from reader-only summaries.
- Known honesty rule: if a signal cannot be emitted at an existing authoritative boundary, do not add it in this plan.

## File Structure Map

### Root Docs

- Modify: `/Users/viv/projs/axiom-arb/README.md`
  Responsibility: explain the local-only `Wave 1C` control-plane observability surface and explicitly avoid implying OTel export or a connected runtime neg-risk pipeline.

### Observability Vocabulary And Metrics

- Modify: `/Users/viv/projs/axiom-arb/crates/observability/src/conventions.rs`
  Responsibility: add stable span names and field keys for metadata refresh, family validation, family halt, bootstrap rollout provenance, and replay neg-risk summary output.
- Modify: `/Users/viv/projs/axiom-arb/crates/observability/src/metrics.rs`
  Responsibility: keep the neg-risk control-plane metric contract honest and lock any helper recorder methods needed by producers already present in this wave.
- Modify: `/Users/viv/projs/axiom-arb/crates/observability/tests/conventions.rs`
  Responsibility: lock the new repo-owned vocabulary.
- Modify: `/Users/viv/projs/axiom-arb/crates/observability/tests/metrics.rs`
  Responsibility: lock metric keys and recorder behavior used by `Wave 1C`.

### Venue Metadata Refresh

- Modify: `/Users/viv/projs/axiom-arb/crates/venue-polymarket/src/instrumentation.rs`
  Responsibility: add metadata refresh producer helpers that record success/failure, revision/snapshot identity, and discovered-family counts at publication time.
- Modify: `/Users/viv/projs/axiom-arb/crates/venue-polymarket/src/rest.rs`
  Responsibility: carry optional venue producer instrumentation through `PolymarketRestClient` constructor surfaces so metadata refresh can actually reach a recorder.
- Modify: `/Users/viv/projs/axiom-arb/crates/venue-polymarket/src/metadata.rs`
  Responsibility: call the new metadata refresh producer instrumentation only at truthful refresh publication boundaries.
- Create: `/Users/viv/projs/axiom-arb/crates/venue-polymarket/tests/metadata_observability.rs`
  Responsibility: verify metadata refresh producer spans and counters using a local listener plus captured tracing output.
- Modify: `/Users/viv/projs/axiom-arb/crates/venue-polymarket/tests/support/mod.rs`
  Responsibility: reuse span capture and local-listener helpers for metadata refresh observability tests.

### Validator And Persistence Producers

- Modify: `/Users/viv/projs/axiom-arb/crates/strategy-negrisk/Cargo.toml`
  Responsibility: add the minimum `observability` and `tracing` dependencies required for validator-boundary instrumentation.
- Create: `/Users/viv/projs/axiom-arb/crates/strategy-negrisk/src/instrumentation.rs`
  Responsibility: own optional validator producer instrumentation that reports validation verdict fields but does not emit discovered-family counts.
- Modify: `/Users/viv/projs/axiom-arb/crates/strategy-negrisk/src/lib.rs`
  Responsibility: export the validator instrumentation surface.
- Modify: `/Users/viv/projs/axiom-arb/crates/strategy-negrisk/src/validator.rs`
  Responsibility: emit validation-boundary spans with revision, snapshot, verdict, and exclusion reason where applicable.
- Modify: `/Users/viv/projs/axiom-arb/crates/strategy-negrisk/tests/graph_and_validator.rs`
  Responsibility: lock validator observability behavior.

- Modify: `/Users/viv/projs/axiom-arb/crates/persistence/Cargo.toml`
  Responsibility: add the minimum `observability` and `tracing` dependencies required for persistence-boundary instrumentation.
- Create: `/Users/viv/projs/axiom-arb/crates/persistence/src/instrumentation.rs`
  Responsibility: own optional persistence producer instrumentation for validation/halt upserts only.
- Modify: `/Users/viv/projs/axiom-arb/crates/persistence/src/lib.rs`
  Responsibility: export the persistence instrumentation surface.
- Modify: `/Users/viv/projs/axiom-arb/crates/persistence/src/repos.rs`
  Responsibility: emit family validation and family halt producer signals only where persistence establishes authoritative current-view state.
- Modify: `/Users/viv/projs/axiom-arb/crates/persistence/tests/negrisk.rs`
  Responsibility: verify persistence-boundary spans and totals without inventing stale-halt semantics.

### App-Live Bootstrap Rollout Alignment

- Modify: `/Users/viv/projs/axiom-arb/crates/app-live/src/instrumentation.rs`
  Responsibility: add focused helpers for bootstrap rollout provenance and parity-mismatch emission at the correct boundary.
- Modify: `/Users/viv/projs/axiom-arb/crates/app-live/src/supervisor.rs`
  Responsibility: make supervisor the single emitter for bootstrap rollout gauges/counters and attach explicit provenance fields.
- Modify: `/Users/viv/projs/axiom-arb/crates/app-live/src/main.rs`
  Responsibility: stop re-emitting any producer metrics from `SupervisorSummary`; limit main to forwarding structured completion output.
- Modify: `/Users/viv/projs/axiom-arb/crates/app-live/tests/supervisor_observability.rs`
  Responsibility: lock single-emitter rollout metric behavior, bootstrap-completion forwarding behavior, and provenance fields.
- Modify: `/Users/viv/projs/axiom-arb/crates/app-live/tests/main_entrypoint.rs`
  Responsibility: verify the entrypoint still emits structured completion output after producer-metric forwarding is removed.

### App-Replay Summary Contract

- Modify: `/Users/viv/projs/axiom-arb/crates/app-replay/src/negrisk_summary.rs`
  Responsibility: extend the public library summary contract with already-authoritative snapshot identity and keep summary derivation honest.
- Modify: `/Users/viv/projs/axiom-arb/crates/app-replay/src/lib.rs`
  Responsibility: continue exporting the summary types/functions with the expanded contract.
- Modify: `/Users/viv/projs/axiom-arb/crates/app-replay/src/main.rs`
  Responsibility: emit an operator-facing neg-risk summary output that forwards the library summary contract without inventing producer metrics.
- Modify: `/Users/viv/projs/axiom-arb/crates/app-replay/tests/negrisk_summary.rs`
  Responsibility: lock the expanded summary contract.
- Modify: `/Users/viv/projs/axiom-arb/crates/app-replay/tests/main_entrypoint.rs`
  Responsibility: verify operator-facing replay output includes the new neg-risk summary fields.

## Task 1: Add Wave 1C Neg-Risk Vocabulary And Metric Contract

**Files:**
- Modify: `crates/observability/src/conventions.rs`
- Modify: `crates/observability/src/metrics.rs`
- Modify: `crates/observability/tests/conventions.rs`
- Modify: `crates/observability/tests/metrics.rs`

- [ ] **Step 1: Write the failing conventions and metrics tests**

```rust
use observability::{field_keys, span_names, RuntimeMetrics};

#[test]
fn wave1c_negrisk_conventions_are_repo_owned() {
    assert_eq!(span_names::VENUE_METADATA_REFRESH, "axiom.venue.metadata.refresh");
    assert_eq!(span_names::NEG_RISK_FAMILY_VALIDATION, "axiom.neg_risk.family.validation");
    assert_eq!(span_names::NEG_RISK_FAMILY_HALT, "axiom.neg_risk.family.halt");
    assert_eq!(span_names::REPLAY_NEGRISK_SUMMARY, "axiom.app_replay.negrisk_summary");

    assert_eq!(field_keys::DISCOVERY_REVISION, "discovery_revision");
    assert_eq!(field_keys::METADATA_SNAPSHOT_HASH, "metadata_snapshot_hash");
    assert_eq!(field_keys::REFRESH_RESULT, "refresh_result");
    assert_eq!(field_keys::REFRESH_DURATION_MS, "refresh_duration_ms");
    assert_eq!(field_keys::DISCOVERED_FAMILY_COUNT, "discovered_family_count");
    assert_eq!(field_keys::VALIDATION_STATUS, "validation_status");
    assert_eq!(field_keys::EXCLUSION_REASON, "exclusion_reason");
    assert_eq!(field_keys::HALTED, "halted");
    assert_eq!(field_keys::EVIDENCE_SOURCE, "evidence_source");
}

#[test]
fn runtime_metrics_keep_wave1c_control_plane_contracts() {
    let metrics = RuntimeMetrics::default();
    assert_eq!(
        metrics.neg_risk_family_discovered_count.key().as_str(),
        "axiom_neg_risk_family_discovered_count"
    );
    assert_eq!(
        metrics.neg_risk_rollout_parity_mismatch_count.key().as_str(),
        "axiom_neg_risk_rollout_parity_mismatch_total"
    );
}
```

- [ ] **Step 2: Run the targeted tests to verify failure**

Run:

```bash
cargo test -p observability wave1c_negrisk_conventions_are_repo_owned -- --exact
cargo test -p observability runtime_metrics_keep_wave1c_control_plane_contracts -- --exact
```

Expected: FAIL because the new span names / field keys do not exist yet.

- [ ] **Step 3: Implement the minimal vocabulary additions**

```rust
pub mod span_names {
    pub const VENUE_METADATA_REFRESH: &str = "axiom.venue.metadata.refresh";
    pub const NEG_RISK_FAMILY_VALIDATION: &str = "axiom.neg_risk.family.validation";
    pub const NEG_RISK_FAMILY_HALT: &str = "axiom.neg_risk.family.halt";
    pub const REPLAY_NEGRISK_SUMMARY: &str = "axiom.app_replay.negrisk_summary";
}

pub mod field_keys {
    pub const DISCOVERY_REVISION: &str = "discovery_revision";
    pub const METADATA_SNAPSHOT_HASH: &str = "metadata_snapshot_hash";
    pub const REFRESH_RESULT: &str = "refresh_result";
    pub const REFRESH_DURATION_MS: &str = "refresh_duration_ms";
    pub const DISCOVERED_FAMILY_COUNT: &str = "discovered_family_count";
    pub const VALIDATION_STATUS: &str = "validation_status";
    pub const EXCLUSION_REASON: &str = "exclusion_reason";
    pub const HALTED: &str = "halted";
    pub const EVIDENCE_SOURCE: &str = "evidence_source";
}
```

Implementation notes:

- do not add stale-halt vocabulary
- do not add OTel-specific attribute helpers
- keep the current metric handles; this task is about locking the contract, not inventing new surfaces

- [ ] **Step 4: Run the observability contract suite**

Run:

```bash
cargo test -p observability --test conventions -- --nocapture
cargo test -p observability --test metrics -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/observability/src/conventions.rs crates/observability/src/metrics.rs crates/observability/tests/conventions.rs crates/observability/tests/metrics.rs
git commit -m "feat: add wave1c control-plane observability contract"
```

## Task 2: Instrument Metadata Refresh Producer Boundaries

**Files:**
- Modify: `crates/venue-polymarket/src/instrumentation.rs`
- Modify: `crates/venue-polymarket/src/rest.rs`
- Modify: `crates/venue-polymarket/src/metadata.rs`
- Modify: `crates/venue-polymarket/tests/support/mod.rs`
- Create: `crates/venue-polymarket/tests/metadata_observability.rs`

- [ ] **Step 1: Write the failing metadata observability tests**

```rust
#[tokio::test]
async fn successful_metadata_refresh_records_revision_snapshot_and_discovered_count() {
    let observability = bootstrap_observability("venue-metadata-test");
    let client = sample_client_with_instrumentation(observability.recorder());

    let (captured_spans, rows) = capture_spans_async(|| async {
        client.try_fetch_neg_risk_metadata_rows().await.unwrap()
    })
    .await;

    assert!(!rows.is_empty());
    let refresh_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::VENUE_METADATA_REFRESH)
        .expect("metadata refresh span missing");
    assert_eq!(
        refresh_span.field(field_keys::DISCOVERY_REVISION).map(String::as_str),
        Some("1")
    );
    assert!(refresh_span.field(field_keys::METADATA_SNAPSHOT_HASH).is_some());
    assert_eq!(
        refresh_span.field(field_keys::REFRESH_RESULT).map(String::as_str),
        Some("\"success\"")
    );
    assert!(refresh_span.field(field_keys::REFRESH_DURATION_MS).is_some());
    assert!(refresh_span.field(field_keys::DISCOVERED_FAMILY_COUNT).is_some());
    assert_eq!(
        observability.registry().snapshot().counter(
            observability.metrics().neg_risk_metadata_refresh_count.key()
        ),
        Some(1)
    );
}

#[tokio::test]
async fn failed_metadata_refresh_does_not_publish_new_discovered_family_gauge() {
    let observability = bootstrap_observability("venue-metadata-test");
    let client = sample_failing_client_with_instrumentation(observability.recorder());

    let (captured_spans, result) = capture_spans_async(|| async {
        client.try_fetch_neg_risk_metadata_rows().await
    })
    .await;

    assert!(result.is_err());
    let refresh_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::VENUE_METADATA_REFRESH)
        .expect("metadata refresh span missing");
    assert_eq!(
        refresh_span.field(field_keys::REFRESH_RESULT).map(String::as_str),
        Some("\"failure\"")
    );
    assert!(refresh_span.field(field_keys::REFRESH_DURATION_MS).is_some());

    assert_eq!(
        observability.registry().snapshot().gauge(
            observability.metrics().neg_risk_family_discovered_count.key()
        ),
        None
    );
}
```

- [ ] **Step 2: Run the targeted tests to verify failure**

Run:

```bash
cargo test -p venue-polymarket successful_metadata_refresh_records_revision_snapshot_and_discovered_count -- --exact
cargo test -p venue-polymarket failed_metadata_refresh_does_not_publish_new_discovered_family_gauge -- --exact
```

Expected: FAIL because metadata refresh currently emits no dedicated control-plane producer signals.

- [ ] **Step 3: Add metadata refresh producer helpers**

```rust
pub fn record_metadata_refresh_started(&self) { /* attempt counter + start timing */ }

pub fn record_metadata_refresh_success(
    &self,
    discovery_revision: i64,
    metadata_snapshot_hash: &str,
    discovered_family_count: usize,
    refresh_duration_ms: u64,
) { /* completed span fields + discovered gauge */ }

pub fn record_metadata_refresh_failure(&self, refresh_duration_ms: u64) { /* failed span fields */ }
```

Implementation notes:

- `neg_risk_metadata_refresh_count` is the refresh-attempt counter in this wave
- increment it exactly once at refresh start
- success and failure are distinguished by span fields, not ambiguous counter semantics
- record refresh duration and result classification on the completion path
- success is emitted only after `cache.current.replace(snapshot.clone())`
- do not emit discovered-family gauges from fallback-to-cache reads

- [ ] **Step 4: Wire the instrumentation into `try_fetch_neg_risk_metadata_rows`**

Minimal shape:

```rust
let snapshot = NegRiskDiscoverySnapshot { ... };
cache.current.replace(snapshot.clone());
instrumentation.record_metadata_refresh_success(
    snapshot.discovery_revision,
    &snapshot.metadata_snapshot_hash,
    count_distinct_families(&snapshot.rows),
    refresh_duration_ms,
);
```

Also wire the optional instrumentation through `PolymarketRestClient::new(...)` and `with_http_client(...)` in `rest.rs` so the metadata path can actually reach the recorder.

- [ ] **Step 5: Run the venue metadata tests**

Run:

```bash
cargo test -p venue-polymarket --test metadata_observability -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/venue-polymarket/src/instrumentation.rs crates/venue-polymarket/src/rest.rs crates/venue-polymarket/src/metadata.rs crates/venue-polymarket/tests/support/mod.rs crates/venue-polymarket/tests/metadata_observability.rs
git commit -m "feat: instrument wave1c metadata refresh producers"
```

## Task 3: Instrument Validation And Halt Producer Boundaries

**Files:**
- Modify: `crates/strategy-negrisk/Cargo.toml`
- Create: `crates/strategy-negrisk/src/instrumentation.rs`
- Modify: `crates/strategy-negrisk/src/lib.rs`
- Modify: `crates/strategy-negrisk/src/validator.rs`
- Modify: `crates/strategy-negrisk/tests/graph_and_validator.rs`
- Modify: `crates/persistence/Cargo.toml`
- Create: `crates/persistence/src/instrumentation.rs`
- Modify: `crates/persistence/src/lib.rs`
- Modify: `crates/persistence/src/repos.rs`
- Modify: `crates/persistence/tests/negrisk.rs`

- [ ] **Step 1: Write the failing validator and persistence observability tests**

```rust
#[test]
fn validator_records_verdict_without_emitting_discovered_family_count() {
    let observability = bootstrap_observability("validator-test");
    let instrumentation = NegRiskValidatorInstrumentation::enabled(observability.recorder());

    let (_spans, verdict) = capture_spans(|| {
        validate_family_instrumented(&sample_family(), 7, "sha256:snapshot-7", &instrumentation)
    });

    assert_eq!(verdict.discovery_revision, 7);
    assert_eq!(
        observability.registry().snapshot().gauge(
            observability.metrics().neg_risk_family_discovered_count.key()
        ),
        None
    );
}

#[tokio::test]
async fn persistence_validation_and_halt_upserts_emit_authoritative_current_view_metrics() {
    let observability = bootstrap_observability("persistence-test");
    let repo = NegRiskFamilyRepo::with_instrumentation(
        NegRiskPersistenceInstrumentation::enabled(observability.recorder())
    );

    repo.upsert_validation(&pool, &sample_validation_row("family-1", "included")).await.unwrap();
    repo.upsert_validation(&pool, &sample_validation_row("family-2", "excluded")).await.unwrap();
    repo.upsert_halt(&pool, &sample_halt_row("family-1")).await.unwrap();
    reconcile_current_family_view(&pool, 7).await.unwrap();

    let snapshot = observability.registry().snapshot();
    assert_eq!(
        snapshot.gauge(observability.metrics().neg_risk_family_included_count.key()),
        Some(1.0)
    );
    assert_eq!(
        snapshot.gauge(observability.metrics().neg_risk_family_excluded_count.key()),
        Some(1.0)
    );
    assert_eq!(
        snapshot.gauge(observability.metrics().neg_risk_family_halt_count.key()),
        Some(1.0)
    );
}
```

- [ ] **Step 2: Run the targeted tests to verify failure**

Run:

```bash
cargo test -p strategy-negrisk --test graph_and_validator validator_records_verdict_without_emitting_discovered_family_count -- --exact
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test negrisk persistence_validation_and_halt_upserts_emit_authoritative_current_view_metrics -- --exact
```

Expected: FAIL because neither boundary currently emits these control-plane signals.

- [ ] **Step 3: Add validator instrumentation**

```rust
pub struct NegRiskValidatorInstrumentation { /* optional recorder */ }

impl NegRiskValidatorInstrumentation {
    pub fn record_validation(&self, verdict: &FamilyValidation) {
        tracing::info_span!(
            span_names::NEG_RISK_FAMILY_VALIDATION,
            validation_status = %verdict.status.as_str(),
            exclusion_reason = verdict.exclusion_reason.as_deref(),
            discovery_revision = verdict.discovery_revision,
            metadata_snapshot_hash = %verdict.metadata_snapshot_hash,
        )
        .in_scope(|| {});
    }
}
```

Implementation notes:

- validator may emit spans and fields
- validator must not emit discovered-family gauges
- keep the existing `validate_family()` API; add a thin instrumented variant only if necessary

- [ ] **Step 4: Add persistence instrumentation and wire authoritative gauges**

Minimal shape:

```rust
pub fn record_validation_current_view(&self, row: &NegRiskFamilyValidationRow) { /* included/excluded gauges */ }
pub fn record_halt_current_view(&self, row: &FamilyHaltRow) { /* halt gauge */ }
```

Implementation notes:

- do not emit discovered-family gauges from persistence in this wave
- emit included/excluded/halted gauges from authoritative current-view totals, not one-row `1.0` writes
- reuse the existing multi-row reconcile scenarios in `crates/persistence/tests/negrisk.rs`
- do not add any stale-halt metric

- [ ] **Step 5: Run the validator and persistence suites**

Run:

```bash
cargo test -p strategy-negrisk --test graph_and_validator -- --nocapture
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test negrisk -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/strategy-negrisk/Cargo.toml crates/strategy-negrisk/src/instrumentation.rs crates/strategy-negrisk/src/lib.rs crates/strategy-negrisk/src/validator.rs crates/strategy-negrisk/tests/graph_and_validator.rs crates/persistence/Cargo.toml crates/persistence/src/instrumentation.rs crates/persistence/src/lib.rs crates/persistence/src/repos.rs crates/persistence/tests/negrisk.rs
git commit -m "feat: instrument wave1c validation and halt producers"
```

## Task 4: Align App-Live Bootstrap Rollout Observability With Single-Emitter Rules

**Files:**
- Modify: `crates/app-live/src/instrumentation.rs`
- Modify: `crates/app-live/src/supervisor.rs`
- Modify: `crates/app-live/src/main.rs`
- Modify: `crates/app-live/tests/supervisor_observability.rs`
- Modify: `crates/app-live/tests/main_entrypoint.rs`

- [ ] **Step 1: Write the failing single-emitter rollout tests**

```rust
#[test]
fn supervisor_records_bootstrap_rollout_metrics_with_explicit_provenance() {
    let observability = bootstrap_observability("app-live-test");
    let mut supervisor = AppSupervisor::for_tests_instrumented(observability.recorder());
    for journal_seq in 35..=41 {
        supervisor.seed_committed_input(sample_input_task_event(journal_seq));
    }
    supervisor.seed_runtime_progress(41, 7, Some("snapshot-7"));
    supervisor.seed_committed_state_version(7);
    supervisor.seed_pending_reconcile_count(0);
    supervisor.seed_neg_risk_rollout_evidence(sample_rollout_evidence("snapshot-7"));

    let (captured_spans, summary) = capture_spans(|| supervisor.resume_once().unwrap());

    let snapshot = observability.registry().snapshot();
    assert_eq!(
        snapshot.gauge(observability.metrics().neg_risk_live_ready_family_count.key()),
        Some(summary.neg_risk_rollout_evidence.as_ref().unwrap().live_ready_family_count as f64)
    );
    let completion_span = captured_spans
        .iter()
        .find(|span| span.name == span_names::APP_SUPERVISOR_RESUME)
        .expect("resume span missing");
    assert_eq!(
        completion_span.field(field_keys::EVIDENCE_SOURCE).map(String::as_str),
        Some("\"bootstrap\"")
    );
}

#[test]
fn bootstrap_completion_forwarder_does_not_reemit_neg_risk_producer_metrics() {
    let observability = bootstrap_observability("app-live-test");
    let recorder = observability.recorder();
    recorder.record_neg_risk_live_attempt_count(9.0);
    recorder.record_neg_risk_live_ready_family_count(4.0);
    recorder.record_neg_risk_live_gate_block_count(2.0);
    recorder.increment_neg_risk_rollout_parity_mismatch_count(3);

    let result = sample_bootstrap_result_with_rollout_evidence();
    emit_bootstrap_completion_observability(&recorder, &result);

    let snapshot = observability.registry().snapshot();
    assert_eq!(
        snapshot.gauge(observability.metrics().neg_risk_live_attempt_count.key()),
        Some(9.0)
    );
    assert_eq!(
        snapshot.gauge(
            observability
                .metrics()
                .neg_risk_live_ready_family_count
                .key()
        ),
        Some(4.0)
    );
    assert_eq!(
        snapshot.gauge(observability.metrics().neg_risk_live_gate_block_count.key()),
        Some(2.0)
    );
    assert_eq!(
        snapshot.counter(
            observability
                .metrics()
                .neg_risk_rollout_parity_mismatch_count
                .key()
        ),
        Some(3)
    );
}

#[test]
fn binary_entrypoint_emits_structured_bootstrap_log_after_metric_removal() {
    let output = app_live_output("paper", None);
    let combined = format!(
        "{}{}",
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap()
    );

    assert!(combined.contains("app-live bootstrap complete"));
    assert!(combined.contains("neg_risk_live_attempt_count"));
}
```

- [ ] **Step 2: Run the targeted tests to verify failure**

Run:

```bash
cargo test -p app-live --test supervisor_observability supervisor_records_bootstrap_rollout_metrics_with_explicit_provenance -- --exact
cargo test -p app-live --test supervisor_observability bootstrap_completion_forwarder_does_not_reemit_neg_risk_producer_metrics -- --exact
cargo test -p app-live --test main_entrypoint binary_entrypoint_emits_structured_bootstrap_log_after_metric_removal -- --exact
```

Expected: FAIL because the current bootstrap-completion path still re-emits neg-risk producer metrics from `SupervisorSummary`.

- [ ] **Step 3: Move the single producer boundary into supervisor instrumentation**

Implementation notes:

- supervisor remains the only metric emitter for:
  - `neg_risk_live_ready_family_count`
  - `neg_risk_live_gate_block_count`
  - `neg_risk_rollout_parity_mismatch_count`
- extract a small bootstrap-completion forwarding helper in `app-live/src/instrumentation.rs` if needed so tests can verify the no-re-emission contract without introducing a new binary-only export path
- `main.rs` may still log/trace summary values, but it must stop incrementing or recording producer metrics derived from `SupervisorSummary`, including `neg_risk_live_attempt_count` and `neg_risk_rollout_parity_mismatch_count`
- add an `evidence_source` field or equivalent provenance field so synthetic/bootstrap evidence is explicit in spans

- [ ] **Step 4: Run the app-live observability tests**

Run:

```bash
cargo test -p app-live --test supervisor_observability -- --nocapture
cargo test -p app-live --test main_entrypoint -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/instrumentation.rs crates/app-live/src/supervisor.rs crates/app-live/src/main.rs crates/app-live/tests/supervisor_observability.rs crates/app-live/tests/main_entrypoint.rs
git commit -m "fix: align wave1c bootstrap rollout emitters"
```

## Task 5: Extend App-Replay Neg-Risk Summary Public Contract

**Files:**
- Modify: `crates/app-replay/src/negrisk_summary.rs`
- Modify: `crates/app-replay/src/lib.rs`
- Modify: `crates/app-replay/src/main.rs`
- Modify: `crates/app-replay/tests/negrisk_summary.rs`
- Modify: `crates/app-replay/tests/main_entrypoint.rs`

- [ ] **Step 1: Write the failing replay summary tests**

```rust
#[tokio::test]
async fn negrisk_summary_exposes_latest_snapshot_identity() {
    let summary = load_neg_risk_foundation_summary(&pool).await.unwrap();
    assert_eq!(summary.latest_discovery_revision, 7);
    assert_eq!(
        summary.latest_metadata_snapshot_hash.as_deref(),
        Some("sha256:discovery-7")
    );
}

#[tokio::test]
async fn app_replay_main_emits_operator_facing_negrisk_summary_without_new_metrics() {
    let output = run_replay_main_with_negrisk_summary_fixture().await;
    assert!(output.contains("app-replay neg-risk summary"));
    assert!(output.contains("latest_metadata_snapshot_hash"));
}
```

- [ ] **Step 2: Run the targeted tests to verify failure**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-replay negrisk_summary_exposes_latest_snapshot_identity -- --exact
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-replay app_replay_main_emits_operator_facing_negrisk_summary_without_new_metrics -- --exact
```

Expected: FAIL because the library summary does not yet expose snapshot identity and the binary does not emit a neg-risk summary surface.

- [ ] **Step 3: Extend the library summary contract**

Minimal shape:

```rust
pub struct NegRiskFoundationSummary {
    pub discovered_family_count: u64,
    pub validated_family_count: u64,
    pub excluded_family_count: u64,
    pub halted_family_count: u64,
    pub recent_validation_event_count: u64,
    pub recent_halt_event_count: u64,
    pub latest_discovery_revision: i64,
    pub latest_metadata_snapshot_hash: Option<String>,
    pub families: Vec<NegRiskFoundationFamilySummary>,
}
```

Implementation notes:

- derive `latest_metadata_snapshot_hash` from the existing discovery snapshot journal payload, not from a new table
- do not add new counters or gauges in replay

- [ ] **Step 4: Make the binary a thin forwarding surface**

Implementation notes:

- keep `REPLAY_RUN` / `REPLAY_SUMMARY`
- add a neg-risk-specific structured log or span payload that forwards the library summary fields
- do not increment producer metrics from `main.rs`

- [ ] **Step 5: Run the replay tests**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-replay --test negrisk_summary -- --nocapture
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-replay --test main_entrypoint -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/app-replay/src/negrisk_summary.rs crates/app-replay/src/lib.rs crates/app-replay/src/main.rs crates/app-replay/tests/negrisk_summary.rs crates/app-replay/tests/main_entrypoint.rs
git commit -m "feat: expose wave1c replay control-plane summary"
```

## Task 6: Align README And Run Wave 1C Verification

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Write the failing documentation expectation**

Document the exact scope change before editing:

- README should say local observability now covers neg-risk control-plane producers
- README should still say there is no OTel backend or collector
- README should not claim a connected runtime neg-risk control-plane pipeline

- [ ] **Step 2: Update README minimally**

Add or revise wording so it matches the shipped state after Tasks 1-5.

- [ ] **Step 3: Run focused verification**

Run:

```bash
cargo fmt --all --check
cargo clippy -p observability -p venue-polymarket -p strategy-negrisk -p persistence -p app-live -p app-replay --all-targets --offline -- -D warnings
cargo test -p observability --offline
cargo test -p venue-polymarket --test metadata_observability -- --nocapture
cargo test -p strategy-negrisk --test graph_and_validator -- --nocapture
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test negrisk -- --nocapture
cargo test -p app-live --test supervisor_observability -- --nocapture
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-replay --test negrisk_summary -- --nocapture
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-replay --test main_entrypoint -- --nocapture
```

Expected: PASS.

- [ ] **Step 4: Run the full touched-crate sweep**

Run:

```bash
cargo test -p venue-polymarket --offline
cargo test -p strategy-negrisk --offline
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence
cargo test -p app-live --offline
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-replay
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add README.md
git commit -m "docs: align wave1c control-plane observability scope"
```

## Final Verification

Before handing off or executing, re-run:

```bash
cargo fmt --all --check
cargo clippy -p observability -p venue-polymarket -p strategy-negrisk -p persistence -p app-live -p app-replay --all-targets --offline -- -D warnings
cargo test -p observability --offline
cargo test -p venue-polymarket --offline
cargo test -p strategy-negrisk --offline
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence
cargo test -p app-live --offline
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-replay
```

Expected: PASS.

## Notes For The Implementer

- Preserve the spec’s `single-emitter` rule. Do not reintroduce summary-derived counter increments in `main.rs` or other read-side surfaces.
- Treat bootstrap rollout evidence as bootstrap/synthetic unless the existing code already proves otherwise.
- Do not add `stale-halt` metrics or semantics.
- Keep replay summary changes honest: expose authoritative data that already exists, but do not create new runtime producer metrics in replay.
