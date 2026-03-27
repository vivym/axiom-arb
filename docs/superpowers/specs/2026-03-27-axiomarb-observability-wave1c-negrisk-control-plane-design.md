# AxiomArb Observability Wave 1C Neg-Risk Control-Plane Design

- Date: 2026-03-27
- Status: Draft for user review
- Project: `AxiomArb`

## 1. Summary

`Wave 1C` completes `Wave 1: Producer Wiring` for the neg-risk control plane.

The core decision is:

- instrument only existing truthful neg-risk control-plane producers
- keep instrumentation repo-owned and local-first
- align `metadata`, `validation`, `halt`, `app-live rollout`, and `app-replay summary` into one observable slice
- do not introduce `OpenTelemetry`, multi-process contracts, dashboards, or new business state surfaces in this wave

This wave exists to make the current neg-risk control-plane boundaries explainable in local development, replay, and postmortem flows without widening trading semantics.

## 2. Scope

### 2.1 In Scope

- metadata refresh producer observability
- family discovery / inclusion / exclusion / halt control-plane observability
- validation-result observability at current authoritative boundaries
- bootstrap-time `app-live` neg-risk rollout and family-gating observability alignment
- `app-replay` postmortem summary alignment over existing persistence and journal evidence, including any required public summary-contract changes
- repo-owned vocabulary, fields, and metrics required for the above
- local verification using captured tracing output and the in-process metric registry

### 2.2 Out Of Scope

- new business state models, projections, or tables created for observability only
- new trading behavior, retry semantics, or control-plane state machines
- `Wave 2` multi-process contracts
- `OpenTelemetry` exporters, collectors, or backend setup
- dashboards, alerts, and runbooks
- synthetic producer metrics computed only at read time
- execution-side signals that still lack truthful producers, such as `unknown-order` or `broken-leg`

## 3. Current Repository Reality

At current `HEAD`, the repository already contains neg-risk control-plane primitives, but they are not yet one connected runtime control-plane pipeline:

- `venue-polymarket` owns metadata refresh, `discovery_revision`, and `metadata_snapshot_hash`
- `strategy-negrisk` already produces validation verdicts with revision and snapshot identity
- `persistence` already persists current validation and halt views plus explainability events
- `app-live` already computes bootstrap-time neg-risk rollout evidence and gate summaries
- `app-replay` already builds persistence-backed neg-risk summaries for postmortem use

The missing piece is not basic control-plane data. The missing piece is truthful, repo-owned observability over these existing boundaries.

Important limitation:

- this wave does **not** assume a fully connected runtime neg-risk control-plane path already exists
- this wave instruments authoritative control-plane boundaries where they already exist
- wiring those boundaries into a future connected runtime path remains later work and is not smuggled into `Wave 1C`

## 4. Architecture Boundary

`Wave 1C` remains a local, producer-driven observability slice with four layers:

1. `Repo-Owned Vocabulary`
   - span names
   - field keys
   - metric names and dimensions
2. `Producer Instrumentation`
   - instrumentation attached at existing authoritative control-plane boundaries
3. `App And Replay Adapters`
   - `app-live` bootstrap rollout/gating summary alignment
   - `app-replay` postmortem summary alignment
4. `Local Verification`
   - captured tracing output
   - in-process metric registry
   - persistence-backed tests

Hard boundary:

- `Wave 1C` may consume only existing truthful sources of control-plane truth
- if a signal cannot be emitted without inventing a new business surface, that signal is out of scope
- adapter layers may format, forward, or summarize existing truth, but they may not originate producer metrics or counters that belong at a lower authoritative boundary

## 5. Truthful Producer Map

### 5.1 Metadata Refresh

Authoritative producer:

- `crates/venue-polymarket/src/metadata.rs`

Signals:

- refresh started / completed / failed
- refresh result classification
- refresh duration
- `discovery_revision`
- `metadata_snapshot_hash`
- discovered family count at successful publication

Rules:

- successful refresh signals may only be emitted after a complete refresh publishes the current view
- failed refresh may emit failure/degraded signals only
- failed refresh must not masquerade as a partial new snapshot

### 5.2 Family Discovery, Validation, And Exclusion

Authoritative producers:

- validator output in `strategy-negrisk`
- current validation materialization in `persistence`

Signals:

- included family count
- excluded family count
- exclusion-reason distribution
- validation status with revision and snapshot identity

Rules:

- discovered family count belongs to metadata/discovery publication boundaries, not validator or persistence emitters
- fetch/discovery and validator verdicts must remain distinct in observability
- producer observability must reflect existing business boundaries, not collapse them into one synthetic count path

### 5.3 Family Halt

Authoritative producers:

- halt current view and associated snapshot relationship in `persistence`

Signals:

- active halt count
- halt reason
- halt snapshot identity

Rules:

- halt semantics must remain as conservative as the current business logic
- observability must not invent a new stale-halt state or metric in this wave
- if later work introduces an authoritative stale-halt surface, that belongs in a separate design
- latest-discovery membership filtering is a read-side replay/reporting concern, not a halt producer signal

### 5.4 App-Live Rollout And Family Gating

Authoritative adapter input:

- bootstrap rollout evidence and gating logic in `crates/app-live/src/supervisor.rs`

Signals:

- live-ready family count
- blocked family count
- parity mismatch count where already truthfully available
- bootstrap rollout summary spans and structured fields
- explicit evidence provenance, including synthetic/bootstrap semantics where present

Rules:

- `app-live` may report only what its existing rollout evidence already knows
- `app-live` must not become a second source of discovery, validation, or halt truth
- this slice is about bootstrap rollout observability, not authoritative neg-risk runtime truth
- any signal sourced from bootstrap or operator-synthesized evidence must remain explicitly labeled as such

### 5.5 Replay And Postmortem Summary

Authoritative reader inputs:

- existing persistence current views
- existing event-journal explainability events
- existing `app-replay` neg-risk summary logic

Outputs:

- discovered / excluded / halted family summary
- latest revision and snapshot identity
- validation and halt explainability fields
- evidence-oriented structured summary for postmortem use
- a structured neg-risk summary surface exposed by the `app-replay` library API
- a corresponding operator-facing `app-replay` binary output that forwards that library summary without inventing new producer metrics

Rules:

- replay may summarize and format existing truth
- replay must not invent producer metrics that never existed at runtime
- missing evidence must remain explicit rather than silently inferred
- if the current public replay summary contract lacks needed fields, `Wave 1C` may extend that public summary output, but only by exposing already-authoritative data
- `Wave 1C` should treat the library summary contract as the primary public contract and the binary output as a thin forwarding/rendering surface over that library contract

## 6. Components

### 6.1 `crates/observability`

Responsibilities:

- add stable neg-risk control-plane span names and field keys
- add only those metric handles that correspond to existing truthful producers
- preserve in-process registry and local testability

Must not:

- introduce OTel SDK types
- predeclare metrics for future incidents that still lack producers

### 6.2 `crates/venue-polymarket`

Responsibilities:

- emit metadata refresh and discovery-boundary producer signals
- attach revision, snapshot, and count fields to successful refresh publication

Must not:

- report successful discovery counts from failed refreshes
- emit refresh-success metrics before publication semantics are satisfied

### 6.3 `crates/strategy-negrisk` And `crates/persistence`

Responsibilities:

- expose truthful validation, exclusion, and halt observability at existing authoritative boundaries
- keep producer signals aligned with persisted current view and explainability events

Must not:

- refactor business ownership just to make observability more convenient
- create a new observability-only family state surface
- invent a stale-halt business surface for the sake of instrumentation

### 6.4 `crates/app-live`

Responsibilities:

- emit rollout and family-gating control-plane observability that matches current rollout evidence

Must not:

- recompute validation or halt truth locally
- widen trading behavior under the cover of instrumentation work
- present bootstrap or operator-synthesized rollout evidence as authoritative runtime neg-risk truth

### 6.5 `crates/app-replay`

Responsibilities:

- align postmortem summary output with existing persistence and journal evidence
- expose structured summary fields that make current control-plane state explainable

Must not:

- become a new source of truth
- re-run business logic and present the result as if it were original runtime telemetry

## 7. Truthfulness And Error-Handling Rules

### 7.1 Refresh Publication Rule

Only a fully completed refresh that successfully publishes the current view may emit success signals and current counts.

Failed refreshes:

- may emit failure or degraded signals
- may not mint a new successful snapshot identity
- may not overwrite prior successful control-plane truth

### 7.2 Authoritative Count Rule

`discovered`, `included`, `excluded`, and `halted` counts must be sampled from authoritative boundaries already used by the business flow.

Reader-side scans or ad hoc re-aggregation must not be promoted into producer metrics.

### 7.3 Replay Honesty Rule

Replay and postmortem summary may:

- aggregate
- format
- enrich readability

Replay and postmortem summary may not:

- fabricate missing evidence
- treat current recomputation as historical producer truth

If evidence is absent, the output must preserve that absence.

### 7.4 Single-Emitter Rule

Each producer metric must have one authoritative emission boundary.

- producer adapters may emit metrics and spans at the boundary where the underlying truth is established
- higher-level readers, summaries, and binary entrypoints may log or format existing values
- higher-level readers, summaries, and binary entrypoints may not increment producer counters or re-emit producer gauges derived only from read-side summaries

This rule exists to prevent duplicate emission and summary-derived metric drift.

### 7.5 No Semantic Drift Rule

Instrumentation work in `Wave 1C` must not change:

- refresh publish order
- validation semantics
- halt semantics
- rollout gate decisions

If a proposed instrumentation change requires modifying control-plane behavior, that change is out of scope and needs a separate design.

### 7.6 Counter And Gauge Rule

Recommended semantics:

- refresh attempts and refresh failures: counters
- current discovered / included / excluded / halted counts: gauges
- rollout current counts: gauges
- replay summary: structured output, not a hidden producer counter

`stale-halt` is intentionally excluded from this list in `Wave 1C` because no authoritative stale-halt surface exists today.

## 8. Testing Strategy

`Wave 1C` should be accepted only if all three layers pass.

### 8.1 Vocabulary Contract Tests

Purpose:

- prove all new span names, field keys, metric names, and dimensions remain repo-owned and stable

Expected form:

- `observability` crate contract tests

### 8.2 Producer Truthfulness Tests

Purpose:

- prove metadata, validation, halt, and rollout signals come from their declared authoritative paths

Expected form:

- metadata refresh integration tests
- validation and halt current-view tests
- `app-live` bootstrap rollout observability tests
- captured tracing plus in-process metric registry assertions

Hard gate:

- a signal that can only be produced by re-reading state in a non-authoritative layer does not belong in `Wave 1C`

### 8.3 Replay And Postmortem Usability Tests

Purpose:

- prove replay summaries are actually useful for operator inspection and postmortem analysis

Expected form:

- persistence-backed `app-replay` tests
- assertions over revision, snapshot, counts, reasons, and evidence-oriented fields
- public summary-contract assertions where new output fields are exposed

Hard gate:

- missing evidence must appear as missing, not silently inferred

## 9. Recommended Cut Line

Recommended delivery shape:

- one executable `Wave 1C` plan that lands the entire neg-risk control-plane observability slice

Recommended task decomposition:

1. vocabulary and metric contract
2. metadata refresh producer instrumentation
3. validation / exclusion / halt producer instrumentation
4. `app-live` bootstrap rollout and gating observability alignment
5. `app-replay` postmortem summary alignment and verification

This wave should stop there.

It should not expand into:

- `Wave 2` multi-process contracts
- OTel backend work
- collector integration
- dashboards and alerts
- new control-plane business surfaces

## 10. Recommendation

The recommended approach is a single `Wave 1C` control-plane slice:

- broad enough to make neg-risk control-plane behavior observable end-to-end
- narrow enough to stay truthful and avoid platform drift

This is the highest-value remaining observability work on current `main` because:

- `Wave 1A` and `Wave 1B` are already merged
- `phase3c` is now merged, so neg-risk live submit control-plane behavior is real
- the remaining high-value gap is now explainability of neg-risk metadata, validation, halt, rollout, and replay flows

Once `Wave 1C` is complete, the repository can credibly move on to:

- `Wave 2: Multi-Process Contracts`
- and only later to OTel backend and collector rollout
