# AxiomArb Observability Design

- Date: 2026-03-24
- Status: Draft for user review
- Project: `AxiomArb`

## 1. Summary

`AxiomArb` should adopt `OpenTelemetry` as its eventual observability backend, but not as its immediate programming model.

The core design decision is:

- application and domain crates continue to instrument through `tracing` plus repo-owned typed observability interfaces
- the `observability` crate owns backend adaptation
- `OpenTelemetry` is introduced behind that boundary once live runtime wiring exists and there is a real export target

In short:

- `observability` should be `OTel-compatible`
- `observability` should not become `OTel-first` today

## 2. Current Repository Reality

At current `HEAD`, the repository has only a minimal observability skeleton:

- [`crates/observability/src/lib.rs`](/Users/viv/projs/axiom-arb/crates/observability/src/lib.rs) exposes a repo-local `Observability` facade
- [`crates/observability/src/metrics.rs`](/Users/viv/projs/axiom-arb/crates/observability/src/metrics.rs) defines typed metric handles and an in-process `MetricRegistry`
- [`crates/observability/src/tracing_bootstrap.rs`](/Users/viv/projs/axiom-arb/crates/observability/src/tracing_bootstrap.rs) installs a basic `tracing_subscriber::fmt()` subscriber
- [`crates/app-live/src/main.rs`](/Users/viv/projs/axiom-arb/crates/app-live/src/main.rs) records only `runtime_mode` during bootstrap and does not yet wire real venue feeds, heartbeat, reconciliation, or exporter setup

This means the current gap is not "we forgot to import OTel." The real gap is that the live runtime does not yet emit enough structured signals for an OTel backend to provide meaningful leverage.

## 3. Goals

This design should guarantee the following:

- business logic remains decoupled from any specific telemetry SDK
- runtime-critical signals have stable names and semantics before exporters are introduced
- the system can graduate from local development output to collector-backed telemetry without rewriting execution, state, or strategy code
- traces, metrics, and logs can eventually explain a full causal path across opportunity detection, risk, execution, recovery, and replay

## 4. Non-Goals

This design does not define:

- a production dashboard layout
- a vendor commitment such as Datadog, Grafana Cloud, Tempo, or Jaeger
- production alert thresholds
- app-live venue wiring
- replay state reconstruction beyond what existing specs already require

## 5. Architecture Decision

### 5.1 Recommended Approach

Use a three-layer model:

1. `Instrumentation Surface`
   - `tracing` spans and events
   - typed metric handles from `RuntimeMetrics`
   - repo-owned `Observability` facade

2. `Observability Backend Adapter`
   - implemented inside the `observability` crate
   - maps repo-level metrics and tracing to backend-specific exporters
   - can remain local-only today and gain OTel export later

3. `Export Target`
   - local text output during current bootstrap development
   - collector-backed `OpenTelemetry` pipeline in a later phase

### 5.2 Rejected Alternatives

`OTel-first in application code`
- rejected because it would leak SDK types into app and domain crates before telemetry semantics are stable

`Stay custom forever`
- rejected because future live debugging, distributed correlation, and external tooling will benefit from OTel traces and metrics export

## 6. Boundary Rules

The following boundaries are hard requirements:

- `domain`, `state`, `execution`, `strategy-*`, `venue-*`, `app-live`, and `app-replay` must not depend directly on OTel SDK types
- those crates may emit `tracing` spans and use repo-owned metric handles only
- only the `observability` crate may own backend initialization and exporter configuration
- typed metric names remain authoritative at the repo boundary even after OTel export is added
- metric dimensions, if added, must also remain repo-owned and typed rather than exposing raw OTel attribute construction in business crates
- disabling OTel export must not change business behavior

## 7. Signal Model

### 7.1 Traces

Tracing should become the primary causal graph for control flow. At minimum, future instrumentation should cover:

- metadata refresh cycle
- market websocket session and reconnects
- user websocket session and reconnects
- order-heartbeat loop
- reconciliation cycle
- opportunity evaluation
- risk approval or rejection
- execution-plan lifecycle
- relayer transaction tracking
- replay run and summary generation

Each span should carry stable identifiers where available:

- `runtime_mode`
- `app_mode`
- `market_id`
- `condition_id`
- `event_family_id`
- `order_id`
- `client_order_key`
- `execution_plan_id`
- `ctf_operation_id`
- `relayer_tx_id`
- `journal_seq`
- `discovery_revision`
- `metadata_snapshot_hash`

`service.name` should be treated as resource-level metadata configured by the `observability` crate, not as a field that every span must redundantly emit.

### 7.2 Metrics

Metrics should remain repo-owned and typed. Current handles are acceptable as the seed set, but future live wiring should preserve three categories:

- `Runtime health`
  - heartbeat freshness
  - runtime mode
  - websocket reconnect counts
  - divergence counts
  - relayer pending age

- `Execution and recovery`
  - unknown-order count
  - reconcile attention count
  - broken-leg inventory count
  - stale relayer transaction count
  - cancel-all or halt activations

- `Neg-risk foundation and later live rollout`
  - discovered family count
  - included family count
  - excluded family count
  - family halt count
  - metadata refresh count

### 7.3 Metric Dimensions

The repo should explicitly support dimensions for signals that are not faithfully representable as one global scalar.

Examples:

- websocket reconnect count by `channel`
- halt activation count by `scope`
- reconcile attention count by `reason`
- broken-leg inventory count by `strategy`

Rules:

- dimensions must be defined as repo-owned typed enums or constrained keys
- business crates must not construct arbitrary backend-specific label maps
- OTel attributes, if used later, are derived from repo-level typed dimensions inside `observability`
- if a metric remains dimensionless in Phase 1, that is a deliberate contract and not an invitation to encode dimensions into metric names ad hoc

### 7.4 Mode And State Signals

`runtime_mode` and similar state-like signals are first-class repo semantics, but they should not be treated as requiring a one-to-one OTel metric instrument mapping.

The contract is:

- repo code may continue to record `runtime_mode` through a typed observability interface
- the local in-process registry may store it as a string-like state sample
- when OTel export is introduced, `observability` may map this signal to the most appropriate backend representation
- acceptable backend representations include a structured log field, a trace field, or a numeric/state metric with typed attributes

The important invariant is call-site stability at the repo boundary, not identity of the backend representation.

### 7.5 Logs

Logs should be treated as rendered trace events, not as a separate ad hoc channel. Human-readable logs may continue to use `tracing_subscriber::fmt()` locally, but structured fields must remain authoritative.

## 8. OpenTelemetry Adoption Strategy

### 8.1 Phase 0: Current State

Keep:

- local `tracing_subscriber::fmt()`
- in-process `MetricRegistry`
- repo-owned `Observability` facade

Do not add:

- direct OTel dependencies to business crates
- mandatory collectors
- vendor-specific configuration in app crates

### 8.2 Phase 1: Stable Instrumentation Contracts

Before any OTel exporter is added, the repository should stabilize:

- span names
- required span fields
- metric names and semantic meaning
- metric dimensions and typed dimension vocabularies
- mode and state signal mapping rules
- initialization ownership inside the `observability` crate
- binary entrypoint policy so `app-live` and `app-replay` stop bypassing structured tracing with direct terminal-only output on their main success paths

This is the minimum stage required before exporter work is worth doing.

### 8.3 Phase 2: Optional OTel Backend

Add an optional backend inside `observability`:

- `tracing` remains the source instrumentation API
- OTel setup is configured in `observability`
- the backend can export traces and metrics to a collector when enabled
- local development may continue to use plain formatted tracing output without a collector

The backend must be opt-in and safe to disable.

### 8.4 Phase 3: Production Collector Integration

Once `app-live` runs real venue loops, add:

- resource metadata including `service.name`, environment, instance, and run identifiers
- collector export configuration
- sampling policy
- failure handling rules for exporter backpressure or collector unavailability

Exporter failure must degrade observability, not trading correctness.

## 9. OpenTelemetry-Specific Decisions

When OTel is introduced, the following rules should apply:

- use `tracing` as the instrumentation API rather than raw OTel spans in application code
- map typed repo metrics onto OTel instruments inside `observability`
- prefer one-way adaptation from repo semantics to OTel, not the reverse
- keep exporter configuration runtime-driven and isolated from strategy or execution code
- preserve local testability with the in-process registry even after OTel export exists

## 10. Rollout Gates

The repository should not add OTel exporter work until all of the following are true:

- `app-live` emits real runtime signals beyond bootstrap mode
- websocket, heartbeat, reconcile, and execution loops exist in the binary entrypoint
- there is a known export destination or collector contract
- the team agrees on required trace fields for execution and recovery analysis

If these conditions are not met, the correct next step is stronger local instrumentation, not backend expansion.

## 11. Acceptance Criteria

This design is successful if it produces the following outcomes:

- observability stays decoupled from business logic
- current custom metrics remain valid and testable
- future OTel adoption does not require rewriting instrumentation call sites across the workspace
- the repo can support both local developer output and collector-backed export through the same facade
- execution and recovery paths can eventually be traced end-to-end with stable identifiers

## 12. Recommended Next Step

If this design is accepted, the next document should be an implementation plan for `observability phase 1`, not a full exporter rollout.

That plan should cover:

- required spans and fields by crate
- metric ownership and emission points
- metric dimensions and mode-signal contracts
- bootstrap API shape in `observability`
- entrypoint migration for `app-live` and `app-replay`
- compatibility path for later `tracing-opentelemetry` integration
