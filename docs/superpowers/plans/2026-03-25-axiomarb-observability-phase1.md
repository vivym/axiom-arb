# AxiomArb Observability Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stabilize the local observability contract for `AxiomArb`: typed span and metric conventions, dimension-aware in-process metrics, a single bootstrap surface, and structured entrypoint logging in `app-live` and `app-replay`, without adding any OpenTelemetry exporter yet.

**Architecture:** Keep business crates on `tracing` plus repo-owned observability types. Extend the `observability` crate so it owns conventions, dimension vocabularies, metric storage, and bootstrap setup; then migrate `app-live` and `app-replay` to use that single bootstrap surface and structured tracing output instead of direct terminal-only success paths.

**Tech Stack:** Rust, `tracing`, `tracing-subscriber`, existing in-process `MetricRegistry`, standard integration tests using `std::process::Command`

---

## Scope Boundary

This plan intentionally covers `observability phase 1` only.

- In scope: stable span names and field keys, typed metric dimensions, mode/state signal contract, one repo-owned bootstrap API, structured startup/replay logs, updated tests, and README alignment.
- Out of scope: `tracing-opentelemetry`, OTLP exporters, collector configuration, vendor-specific dashboards, live venue wiring, or changing business logic based on observability state.
- Current runtime reality: `app-live` is still a bootstrap skeleton and `app-replay` is still summary-oriented. This plan must improve observability around those entrypoints without pretending the runtime is already feature-complete.

## File Structure Map

### Root Docs

- Modify: `/Users/viv/projs/axiom-arb/README.md`
  Responsibility: explain that binaries now emit structured tracing output through the unified observability bootstrap instead of a plain success `println!`.

### Observability

- Create: `crates/observability/src/conventions.rs`
  Responsibility: stable span names, field keys, metric dimension enums, and repo-owned vocabulary for future attributes.
- Create: `crates/observability/src/bootstrap.rs`
  Responsibility: single bootstrap surface that owns tracing initialization, service identity, and `Observability` context creation.
- Modify: `crates/observability/src/lib.rs`
  Responsibility: export the new bootstrap surface and conventions; stop making callers stitch observability from separate public entrypoints.
- Modify: `crates/observability/src/metrics.rs`
  Responsibility: add dimension-aware samples and snapshot lookup while preserving current scalar metrics and mode/state storage.
- Create: `crates/observability/tests/conventions.rs`
  Responsibility: stable span-name, field-key, and metric-dimension vocabulary tests.
- Test: `crates/observability/tests/metrics.rs`
  Responsibility: metric key stability, dimensioned registry round-trip, mode/state semantics.
- Create: `crates/observability/tests/bootstrap.rs`
  Responsibility: unified bootstrap API behavior and service identity handling.

### App Live

- Modify: `crates/app-live/Cargo.toml`
  Responsibility: add direct `tracing` dependency for entrypoint spans and structured events.
- Modify: `crates/app-live/src/main.rs`
  Responsibility: replace split bootstrap calls and plain success output with unified bootstrap plus structured tracing fields.
- Modify: `crates/app-live/tests/main_entrypoint.rs`
  Responsibility: assert structured tracing output and removal of the legacy plain success line.

### App Replay

- Modify: `crates/app-replay/Cargo.toml`
  Responsibility: add `observability` and direct `tracing` dependency for replay entrypoint instrumentation.
- Modify: `crates/app-replay/src/main.rs`
  Responsibility: initialize the unified observability bootstrap and emit structured replay summary fields instead of a plain success `println!`.
- Create: `crates/app-replay/tests/main_entrypoint.rs`
  Responsibility: verify the replay binary emits structured tracing output through the same bootstrap surface.

## Task 1: Add Stable Observability Conventions

**Files:**
- Create: `crates/observability/src/conventions.rs`
- Modify: `crates/observability/src/lib.rs`
- Create: `crates/observability/tests/conventions.rs`

- [ ] **Step 1: Write the failing conventions tests**

```rust
use observability::{field_keys, metric_dimensions, span_names};

#[test]
fn observability_conventions_define_stable_span_names_and_field_keys() {
    assert_eq!(span_names::APP_BOOTSTRAP, "axiom.app.bootstrap");
    assert_eq!(span_names::REPLAY_RUN, "axiom.app_replay.run");
    assert_eq!(field_keys::SERVICE_NAME, "service.name");
    assert_eq!(field_keys::RUNTIME_MODE, "runtime_mode");
}

#[test]
fn metric_dimension_vocabularies_are_repo_owned_and_finite() {
    assert_eq!(
        metric_dimensions::Channel::User.as_pair(),
        ("channel", "user")
    );
    assert_eq!(
        metric_dimensions::HaltScope::Family.as_pair(),
        ("scope", "family")
    );
}
```

- [ ] **Step 2: Run the new test to verify failure**

Run: `cargo test -p observability observability_conventions_define_stable_span_names_and_field_keys -- --exact`

Expected: FAIL because `conventions.rs` and the exported symbols do not exist yet.

- [ ] **Step 3: Implement stable conventions**

```rust
pub mod span_names {
    pub const APP_BOOTSTRAP: &str = "axiom.app.bootstrap";
    pub const APP_BOOTSTRAP_COMPLETE: &str = "axiom.app.bootstrap.complete";
    pub const REPLAY_RUN: &str = "axiom.app_replay.run";
    pub const REPLAY_SUMMARY: &str = "axiom.app_replay.summary";
}

pub mod field_keys {
    pub const SERVICE_NAME: &str = "service.name";
    pub const APP_MODE: &str = "app_mode";
    pub const RUNTIME_MODE: &str = "runtime_mode";
    pub const BOOTSTRAP_STATUS: &str = "bootstrap_status";
    pub const PROCESSED_COUNT: &str = "processed_count";
    pub const LAST_JOURNAL_SEQ: &str = "last_journal_seq";
}

pub mod metric_dimensions {
    pub enum Channel {
        Market,
        User,
    }

    pub enum HaltScope {
        Global,
        Family,
        Market,
        Strategy,
    }
}
```

Implementation notes:
- keep vocabulary repo-owned and finite
- do not expose raw OTel attribute builders
- keep conventions in a dedicated module so later OTel export can translate from one source of truth

- [ ] **Step 4: Re-export conventions from the crate root**

```rust
mod conventions;

pub use conventions::{field_keys, metric_dimensions, span_names};
```

- [ ] **Step 5: Run the conventions-focused tests**

Run: `cargo test -p observability observability_conventions_define_stable_span_names_and_field_keys -- --exact`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/observability/src/conventions.rs crates/observability/src/lib.rs crates/observability/tests/conventions.rs
git commit -m "feat: add observability conventions"
```

## Task 2: Add Dimension-Aware Metrics And Preserve Mode/State Signals

**Files:**
- Modify: `crates/observability/src/metrics.rs`
- Modify: `crates/observability/src/lib.rs`
- Test: `crates/observability/tests/metrics.rs`

- [ ] **Step 1: Write the failing metric registry tests**

```rust
use observability::{
    metric_dimensions::{Channel, HaltScope},
    metrics::{MetricDimension, MetricDimensions, MetricKey},
    Observability,
};

#[test]
fn registry_round_trips_dimensioned_counter_samples() {
    let observability = Observability::new("app-live");
    let metrics = observability.metrics();
    let dims = MetricDimensions::new([
        MetricDimension::Channel(Channel::User),
    ]);

    observability
        .registry()
        .record_counter_with_dimensions(
            metrics.websocket_reconnect_total.increment_with_dimensions(2, dims.clone()),
        );

    let snapshot = observability.registry().snapshot();
    assert_eq!(
        snapshot.counter_with_dimensions(metrics.websocket_reconnect_total.key(), &dims),
        Some(2)
    );
}

#[test]
fn mode_samples_remain_queryable_without_forcing_numeric_encoding() {
    let observability = Observability::new("app-live");
    observability.recorder().record_runtime_mode("healthy");

    let snapshot = observability.registry().snapshot();
    assert_eq!(
        snapshot.mode(observability.metrics().runtime_mode.key()),
        Some("healthy")
    );
}
```

- [ ] **Step 2: Run the metric registry test to verify failure**

Run: `cargo test -p observability registry_round_trips_dimensioned_counter_samples -- --exact`

Expected: FAIL because the registry has no dimension-aware sample types or lookup helpers.

- [ ] **Step 3: Add typed dimension support to the metric model**

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MetricDimension {
    Channel(metric_dimensions::Channel),
    HaltScope(metric_dimensions::HaltScope),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct MetricDimensions(Vec<MetricDimension>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterSampleWithDimensions {
    key: MetricKey,
    amount: u64,
    dimensions: MetricDimensions,
}
```

Implementation notes:
- preserve the existing scalar gauge/counter/mode API
- add dimension-aware variants rather than replacing scalar samples
- store dimensions canonically so snapshot comparisons stay deterministic

- [ ] **Step 4: Extend `RuntimeMetrics` with the first dimension-ready handles**

```rust
pub struct RuntimeMetrics {
    pub websocket_reconnect_total: CounterHandle,
    pub halt_activation_total: CounterHandle,
    pub reconcile_attention_total: CounterHandle,
    // keep existing scalar handles unchanged
}
```

Also add recorder helpers only where call sites already exist or are immediately needed; do not invent fake runtime emissions just to populate metrics.

- [ ] **Step 5: Add snapshot query helpers and keep mode/state semantics**

```rust
impl MetricRegistrySnapshot {
    pub fn counter_with_dimensions(
        &self,
        key: MetricKey,
        dimensions: &MetricDimensions,
    ) -> Option<u64> {
        // lookup by canonicalized key + dimensions
    }
}
```

Contract notes:
- `runtime_mode` remains stored as a mode/state signal in the local registry
- do not force it into a numeric instrument just because an eventual OTel backend exists

- [ ] **Step 6: Run the observability test suite**

Run: `cargo test -p observability`

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/observability/src/metrics.rs crates/observability/src/lib.rs crates/observability/tests/metrics.rs
git commit -m "feat: add dimension-aware observability metrics"
```

## Task 3: Create A Single Observability Bootstrap Surface

**Files:**
- Create: `crates/observability/src/bootstrap.rs`
- Modify: `crates/observability/src/lib.rs`
- Modify: `crates/observability/src/tracing_bootstrap.rs`
- Test: `crates/observability/tests/bootstrap.rs`

- [ ] **Step 1: Write the failing bootstrap tests**

```rust
use observability::bootstrap_observability;

#[test]
fn bootstrap_surface_returns_service_identity_metrics_and_registry() {
    let bootstrapped = bootstrap_observability("app-live");

    assert_eq!(bootstrapped.service_name(), "app-live");
    bootstrapped.recorder().record_runtime_mode("paper");
    assert_eq!(
        bootstrapped
            .registry()
            .snapshot()
            .mode(bootstrapped.metrics().runtime_mode.key()),
        Some("paper")
    );
}

#[test]
fn bootstrap_surface_initializes_tracing_only_once() {
    let first = bootstrap_observability("app-live");
    let second = bootstrap_observability("app-live");

    assert_eq!(first.service_name(), second.service_name());
}
```

- [ ] **Step 2: Run the bootstrap test to verify failure**

Run: `cargo test -p observability bootstrap_surface_returns_service_identity_metrics_and_registry -- --exact`

Expected: FAIL because there is no unified bootstrap API yet.

- [ ] **Step 3: Implement the new bootstrap surface**

```rust
pub struct BootstrappedObservability {
    tracing: TracingBootstrap,
    observability: Observability,
}

pub fn bootstrap_observability(service_name: impl Into<String>) -> BootstrappedObservability {
    let service_name = service_name.into();
    let tracing = bootstrap_tracing(service_name.clone());
    let observability = Observability::new(service_name);
    BootstrappedObservability {
        tracing,
        observability,
    }
}
```

Required methods:
- `service_name()`
- `metrics()`
- `registry()`
- `recorder()`

- [ ] **Step 4: Narrow the public surface**

Implementation notes:
- keep `bootstrap_tracing` as an internal helper if it still simplifies implementation
- stop making binaries compose observability from separate public calls
- if old public exports remain temporarily for test compatibility, mark them as transitional and remove direct call sites in the same task

- [ ] **Step 5: Run the bootstrap-focused tests**

Run: `cargo test -p observability bootstrap_surface_returns_service_identity_metrics_and_registry -- --exact`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/observability/src/bootstrap.rs crates/observability/src/lib.rs crates/observability/src/tracing_bootstrap.rs crates/observability/tests/bootstrap.rs
git commit -m "feat: add unified observability bootstrap"
```

## Task 4: Migrate `app-live` To Structured Bootstrap Logging

**Files:**
- Modify: `crates/app-live/Cargo.toml`
- Modify: `crates/app-live/src/main.rs`
- Modify: `crates/app-live/tests/main_entrypoint.rs`

- [ ] **Step 1: Write the failing entrypoint test**

```rust
#[test]
fn binary_entrypoint_emits_structured_bootstrap_log() {
    let output = Command::new(app_live_binary())
        .env("AXIOM_MODE", "paper")
        .output()
        .expect("app-live should run");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert!(!stdout.contains("app-live starting"));
    assert!(stderr.contains("app-live bootstrap complete"));
    assert!(stderr.contains("app_mode=paper"));
    assert!(stderr.contains("runtime_mode=Healthy"));
}
```

- [ ] **Step 2: Run the entrypoint test to verify failure**

Run: `cargo test -p app-live binary_entrypoint_emits_structured_bootstrap_log -- --exact`

Expected: FAIL because the binary still prints its success summary with `println!`.

- [ ] **Step 3: Add direct `tracing` usage and unified bootstrap setup**

```toml
[dependencies]
tracing = "0.1.41"
observability = { path = "../observability" }
```

```rust
let observability = bootstrap_observability("app-live");
let _span = tracing::info_span!(
    span_names::APP_BOOTSTRAP,
    app_mode = app_mode.as_str(),
).entered();
```

- [ ] **Step 4: Replace plain success output with structured tracing**

```rust
observability
    .recorder()
    .record_runtime_mode(runtime_mode_label(result.runtime.runtime_mode()));

let _complete_span = tracing::info_span!(
    span_names::APP_BOOTSTRAP_COMPLETE,
    app_mode = result.runtime.app_mode().as_str(),
    bootstrap_status = ?result.runtime.bootstrap_status(),
    runtime_mode = ?result.runtime.runtime_mode(),
    fullset_mode = ?result.summary.fullset_mode,
    negrisk_mode = ?result.summary.negrisk_mode,
    published_snapshot_id = result.summary.published_snapshot_id.as_deref().unwrap_or("none"),
).entered();

tracing::info!(
    "app-live bootstrap complete"
);
```

Also log the fatal error path with `tracing::error!` before exiting so the binary no longer bypasses structured tracing on failure.

- [ ] **Step 5: Run the app-live entrypoint tests**

Run: `cargo test -p app-live main_entrypoint -- --nocapture`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/app-live/Cargo.toml crates/app-live/src/main.rs crates/app-live/tests/main_entrypoint.rs
git commit -m "feat: instrument app-live bootstrap logs"
```

## Task 5: Migrate `app-replay` To Structured Replay Logging

**Files:**
- Modify: `crates/app-replay/Cargo.toml`
- Modify: `crates/app-replay/src/main.rs`
- Create: `crates/app-replay/tests/main_entrypoint.rs`

- [ ] **Step 1: Write the failing replay entrypoint test**

```rust
#[test]
fn binary_entrypoint_emits_structured_replay_summary() {
    let Some(database_url) = std::env::var_os("DATABASE_URL") else {
        return;
    };

    let output = Command::new(app_replay_binary())
        .arg("--from-seq")
        .arg("0")
        .arg("--limit")
        .arg("1")
        .env("DATABASE_URL", database_url)
        .output()
        .expect("app-replay should run");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert!(!stdout.contains("app-replay "));
    assert!(stderr.contains("app-replay summary"));
    assert!(stderr.contains("processed_count="));
}
```

- [ ] **Step 2: Run the replay entrypoint test to verify failure**

Run: `cargo test -p app-replay binary_entrypoint_emits_structured_replay_summary -- --exact`

Expected: FAIL because `app-replay` still prints its success line with `println!` and does not initialize observability.

- [ ] **Step 3: Add the missing dependencies**

```toml
[dependencies]
observability = { path = "../observability" }
tracing = "0.1.41"
```

- [ ] **Step 4: Bootstrap observability and emit structured replay fields**

```rust
let _observability = bootstrap_observability("app-replay");
let _span = tracing::info_span!(span_names::REPLAY_RUN, after_seq = range.after_seq).entered();
let _summary_span = tracing::info_span!(
    span_names::REPLAY_SUMMARY,
    processed_count = consumer.summary().processed_count,
    last_journal_seq = ?consumer.summary().last_journal_seq,
)
.entered();

tracing::info!(
    "app-replay summary"
);
```

Implementation notes:
- keep the replay library behavior unchanged
- migrate the binary entrypoint only
- log errors with `tracing::error!` before `process::exit(1)`

- [ ] **Step 5: Run the replay tests**

Run: `cargo test -p app-replay main_entrypoint -- --nocapture`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/app-replay/Cargo.toml crates/app-replay/src/main.rs crates/app-replay/tests/main_entrypoint.rs
git commit -m "feat: instrument app-replay entrypoint logs"
```

## Task 6: Update Docs And Run Final Verification

**Files:**
- Modify: `/Users/viv/projs/axiom-arb/README.md`

- [ ] **Step 1: Update the README runtime notes**

Add a short section clarifying:

- `app-live` and `app-replay` now bootstrap observability through one repo-owned entrypoint
- successful startup and replay summaries are emitted through local structured tracing output
- this is still Phase 1 only and does not add any OTel exporter

- [ ] **Step 2: Verify the docs against runtime reality**

Check that README wording still matches:

- `app-live` is a bootstrap skeleton
- `app-replay` is summary-oriented
- observability is still local-only and OTel-compatible, not OTel-enabled

- [ ] **Step 3: Run targeted verification**

Run:

```bash
cargo test -p observability
cargo test -p app-live
cargo test -p app-replay
```

Expected:

- `observability` tests pass with the new conventions, bootstrap, and dimension-aware registry
- `app-live` binary tests pass with structured tracing output
- `app-replay` binary and replay tests pass with structured tracing output

- [ ] **Step 4: Run workspace linting for touched crates**

Run:

```bash
cargo clippy -p observability -p app-live -p app-replay --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add README.md crates/observability crates/app-live crates/app-replay
git commit -m "docs: finalize observability phase1 plan surface"
```

## Final Verification Checklist

- [ ] `cargo test -p observability`
- [ ] `cargo test -p app-live`
- [ ] `cargo test -p app-replay`
- [ ] `cargo clippy -p observability -p app-live -p app-replay --all-targets -- -D warnings`
- [ ] confirm `app-live` no longer emits its legacy plain success `println!`
- [ ] confirm `app-replay` no longer emits its legacy plain success `println!`
- [ ] confirm no OTel exporter dependency was added
