# AxiomArb Unified Runtime And V1a/V1b Rollout Design

- Date: 2026-03-24
- Status: Draft for user review
- Project: `AxiomArb`

## 1. Summary

`AxiomArb` should not evolve by building a `v1a full-set live` engine first and only later refactoring it into a `v1b neg-risk` engine.

The recommended direction is:

- build one unified runtime, decision, recovery, and execution backbone now
- allow `full-set` to be the first route to run in `Live`
- allow `neg-risk` to enter that same backbone first in `Shadow`
- promote `neg-risk` to `Live` only after parity, recovery, replay, and rollout gates are proven

This design intentionally optimizes for production-shape boundaries over shortest-path implementation speed.

## 2. Current Repository Reality

At current `HEAD`, the repository already contains most of the `v1b foundation` library surface:

- [`crates/strategy-negrisk/src/lib.rs`](/Users/viv/projs/axiom-arb/crates/strategy-negrisk/src/lib.rs) exports graph building, validation, and exposure reconstruction
- [`crates/app-replay/src/lib.rs`](/Users/viv/projs/axiom-arb/crates/app-replay/src/lib.rs) exports persistence-backed neg-risk replay helpers
- [`crates/venue-polymarket/src/lib.rs`](/Users/viv/projs/axiom-arb/crates/venue-polymarket/src/lib.rs) already exposes metadata, websocket, heartbeat, retry, and relayer modules

The missing work is not "neg-risk foundation." The missing work is the production runtime path that will eventually host both routes:

- [`crates/app-live/src/main.rs`](/Users/viv/projs/axiom-arb/crates/app-live/src/main.rs) still runs a static bootstrap skeleton
- [`crates/app-live/src/runtime.rs`](/Users/viv/projs/axiom-arb/crates/app-live/src/runtime.rs) does not yet supervise real venue, persistence, dispatcher, execution, or recovery tasks
- [`crates/risk/src/lib.rs`](/Users/viv/projs/axiom-arb/crates/risk/src/lib.rs) still exposes only the `fullset` module
- [`crates/execution/src/lib.rs`](/Users/viv/projs/axiom-arb/crates/execution/src/lib.rs) does not yet define a unified runtime execution backbone with shadow/live separation

This matches the declared repository status in [`README.md`](/Users/viv/projs/axiom-arb/README.md): `v1b foundation` exists, while `v1b live` does not.

## 3. Goals

This design should guarantee the following:

- `v1a` and `v1b` share one runtime backbone, not two independent engines
- strategy modules do not directly place orders
- risk remains an authority layer that answers safety, not rollout policy
- rollout policy is explicit, replayable, and owned by one authority
- recovery uses the same approval and execution backbone as normal strategy decisions
- `Shadow` execution is a true mirror of live orchestration up to the final venue sink
- replay and postmortem analysis can explain activation, decision, planning, execution, uncertainty, and recovery behavior

## 4. Non-Goals

This design does not define:

- the final `neg-risk` pricing model
- the full implementation details of every future route-specific planner
- a production dashboard layout
- a complete simulation framework for fake fills or fake settlement
- a separate second runtime dedicated to `neg-risk`

## 5. Architecture Decision

### 5.1 Recommended Approach

Use a unified backbone with explicit route activation modes.

The runtime is split into:

1. `Composition root`
   - `app-live` owns assembly and supervision only

2. `Authoritative input and state pipeline`
   - external inputs become standardized fact events
   - a single state applier updates authoritative state
   - immutable snapshots are published for downstream readers

3. `Decision backbone`
   - strategy and recovery both emit structured decision inputs
   - activation, risk, planning, and execution orchestration follow one shared path

4. `Execution sink boundary`
   - `Live` and `Shadow` share all orchestration before the final venue sink

5. `Recovery subsystem`
   - recovery is a first-class coordinator, not execution glue code

### 5.2 Rejected Alternatives

`Build v1a first, refactor later`
- rejected because it would force a second boundary rewrite once `neg-risk` moves from foundation libraries into the live runtime

`Keep separate execution/risk stacks per route`
- rejected because it would create route-specific drift in activation, replay, recovery, and execution semantics

`Use shadow as a lightweight logging-only bypass`
- rejected because it would not prove parity with the eventual live execution path

## 6. Runtime Topology

### 6.1 Composition Root

`app-live` should own:

- configuration loading
- DB pool construction
- Polymarket REST, websocket, heartbeat, and relayer client setup
- state store construction
- task supervision
- strategy registry assembly
- activation policy assembly
- risk engine assembly
- execution planner assembly
- execution orchestrator assembly
- recovery coordinator assembly
- observability and journal sinks

`app-live` must not become a business-rules module.

### 6.2 Runtime Tasks

The runtime should supervise these input tasks:

- metadata refresh
- market websocket
- user websocket
- order heartbeat
- periodic reconciliation
- relayer status polling

Each task may observe facts and normalize them, but no task may directly mutate authoritative business state.

### 6.3 Single Write Path

All external inputs must follow this path:

`external source -> ExternalFactEvent -> journal append -> StateApplier -> authoritative state -> snapshot publish`

`StateApplier` is the only component allowed to apply external facts into `StateStore`.

## 7. Runtime Data Flow

### 7.1 External Facts

Input tasks emit `ExternalFactEvent`, not strategy decisions and not direct state mutations.

Every such event must carry enough information for deterministic replay and drift diagnosis, including:

- source kind and stream
- source event identity or stable dedupe key
- observed time
- payload schema version
- standardization version and/or raw payload fingerprint
- structured payload

### 7.2 Authoritative Apply Order

`StateApplier` must apply events in authoritative local order.

Hard rules:

- duplicate events must be idempotent
- replay order is local authoritative order, not external timestamp order
- out-of-order facts must either be buffered/reordered safely or explicitly converted into uncertainty and reconciliation work

### 7.3 Snapshot Publish Contract

State mutation and snapshot publication are separate boundaries.

Rules:

- state commit happens first
- typed projections are derived from one committed version
- a snapshot is not publishable until both `FullSetView` and `NegRiskView` for that same version are ready
- downstream components may consume only published snapshots

### 7.4 Dispatcher Contract

The dispatcher must not evaluate every route on every event.

Rules:

- event application produces a dirty set or relevant change set
- dispatcher may debounce and coalesce high-frequency changes
- coalescing may skip intermediate versions
- coalescing must never drop the latest stable version
- if a domain remains dirty, the dispatcher must eventually evaluate on the newest published snapshot

### 7.5 Activation Layering

Activation is split into two layers:

- coarse filter before strategy dispatch
- fine-grained activation decision inside the shared backbone

Examples:

- `neg-risk Disabled` may suppress strategy dispatch entirely
- `Shadow`, `Live`, `ReduceOnly`, and `RecoveryOnly` still proceed through the decision backbone and affect downstream behavior explicitly

## 8. Route Rollout And Capability Model

### 8.1 Activation Authority

`ActivationPolicy` is the only authority allowed to answer route enablement and execution mode.

No strategy, planner, or executor may maintain its own route enablement truth.

### 8.2 Execution Modes

The system should model activation explicitly, not as booleans.

Minimum modes:

- `Disabled`
- `Shadow`
- `Live`
- `ReduceOnly`
- `RecoveryOnly`

### 8.3 Capability Matrix

Rollout should use a capability matrix at minimum keyed by:

- route
- family or market scope
- execution mode

This allows states such as:

- `full-set -> Live`
- `neg-risk family A -> Shadow`
- `neg-risk family B -> Disabled`
- `neg-risk family C -> ReduceOnly`

### 8.4 Replayable Activation Decisions

Every activation decision must be explainable later.

The activation result should therefore carry replay anchors such as:

- policy or capability revision
- matched rule identifier, when available
- evaluated snapshot identifier

## 9. Decision Backbone

### 9.1 Input Symmetry

Strategy and recovery must feed one shared decision backbone.

Recommended shape:

- `DecisionInput::Strategy(IntentCandidate)`
- `DecisionInput::Recovery(RecoveryIntent)`

The naming of downstream types must remain input-neutral. Do not reintroduce strategy-only naming after the unified entrypoint.

### 9.2 Intent Contracts

`IntentCandidate` must remain an opportunity-layer object, not a hidden order object.

It may include:

- route
- target scope
- advisory pricing
- advisory sizing
- machine-checkable state assumptions
- structured explanation payload
- source snapshot identity

It must not include:

- final venue order parameters
- retry semantics
- transport payload details
- signature or nonce generation

Advisory price/edge/size fields must always carry explicit units or normalization semantics and remain advisory only.

### 9.3 Recovery Contracts

`RecoveryIntent` is not a bypass path for repairs.

It should carry:

- recovery kind
- scope lock
- required state assumptions
- priority
- budget class
- explanation payload
- source snapshot identity

It must still go through:

`ActivationPolicy -> Risk -> Planner -> ExecutionOrchestrator`

## 10. Core Type Contracts

The recommended long-lived object chain is:

`ExternalFactEvent -> ApplyResult -> PublishedSnapshot -> DecisionInput -> ActivationDecision -> RiskVerdict -> ExecutionRequest -> ExecutionPlan -> ExecutionAttempt -> ExecutionReceipt`

### 10.1 `ApplyResult`

`ApplyResult` must distinguish:

- applied changes
- duplicates
- deferred work
- reconcile-required work

Each nontrivial result must carry a stable reference anchor such as:

- `journal_seq`
- duplicate reference
- pending work reference

This is required so that replay, recovery, and operator tooling can reason about the same object graph without guessing from free text.

### 10.2 `PublishedSnapshot`

Each published snapshot must carry:

- snapshot identifier
- state version
- committed journal position
- dirty set
- `FullSetView`
- `NegRiskView`

All downstream artifacts must reference the snapshot they were based on.

### 10.3 `RiskVerdict`

Risk is an authority layer and should return structured verdicts rather than directly constructing final execution actions.

Risk should answer:

- approved
- rejected
- deferred
- reconcile-required

Activation answers rollout mode.
Risk answers safety and admissibility.
Those concerns must remain separate.

### 10.4 `ExecutionRequest` And `ExecutionPlan`

`ExecutionRequest` is the approved request entering planning.

`ExecutionPlan` is the business-level plan to be attempted.

`ExecutionPlan` should contain business actions such as:

- order submission
- cancel
- split
- merge
- redeem

It should not carry every piece of transient execution context.

### 10.5 `ExecutionAttempt`

`ExecutionAttempt` must be first-class and separate from `ExecutionPlan`.

This distinction is required because:

- one plan may require multiple attempts
- retries and resumption must be attributable
- shadow and live both need attempt-level artifacts
- idempotency is attempt-sensitive even when business intent is plan-stable

### 10.6 `ExecutionReceipt`

Receipts must bind to attempt identity, not just plan identity.

`ShadowRecorded` may indicate that orchestration artifacts were produced for an attempt.
It must not imply that venue fills, settlement, or relayer success occurred.

### 10.7 `VenueSink`

The sink boundary should consume:

- the business-level `ExecutionPlan`
- a separate attempt context carrying execution-time metadata

This keeps the plan business-shaped while keeping attempt/runtime metadata out of the plan object itself.

## 11. Execution Backbone

### 11.1 Shared Orchestration

`Live` and `Shadow` must share the same orchestration path for:

- activation decision consumption
- risk verdict consumption
- planning
- journaling
- metrics
- replay artifacts
- attempt identity and idempotency handling

The only intended fork is the final sink:

- `LiveVenueSink`
- `ShadowVenueSink`

### 11.2 Shadow Rules

Shadow must remain a mirror of live orchestration, not a fake authoritative trading system.

Shadow may:

- produce plans
- produce attempts
- write journal artifacts
- write metrics
- write replay artifacts

Shadow must not:

- write authoritative orders or fills current view
- fabricate relayer success
- fabricate settlement completion
- alter authoritative inventory truth

If simulated outcomes are ever added, they must live in separate shadow or simulated namespaces and require explicit opt-in to read.

## 12. Error And State Semantics

### 12.1 Separate Semantic Layers

The system should not collapse all failures into one enum.

Recommended separation:

- `DecisionVerdict`
  - `Approved`
  - `Rejected`
  - `Deferred`
  - `ReconcileRequired`

- `StateConfidence`
  - `Certain`
  - `Uncertain`

- `ExecutionAttemptOutcome`
  - `Succeeded`
  - `FailedDefinitive`
  - `FailedAmbiguous`
  - `RetryExhausted`

### 12.2 `Uncertain` Is Authoritative State, Not Logging

Any unresolved order, relayer effect, or exposure state that cannot be proven must become a first-class state concept.

It is not acceptable for uncertainty to exist only in logs or operator intuition.

### 12.3 Ambiguous Attempt Results

Transport ambiguity, relayer uncertainty, and similar outcomes must not be mis-modeled as definitive business failure.

Ambiguous attempt outcomes should usually drive:

- `StateConfidence::Uncertain`
- reconciliation requirements
- recovery work

not silent drop or optimistic continuation.

## 13. Recovery Model

### 13.1 Recovery Is A Convergence System

`RecoveryCoordinator` should:

1. detect unresolved or divergent situations
2. create structured recovery intents
3. feed those intents back into the main decision backbone

### 13.2 Recovery Scope Locks

Recovery must own strong scope locks, not advisory hints.

Suggested lock scopes include:

- market
- condition
- family
- inventory set
- execution path

Inheritance rule:

- if a parent scope is locked, child scopes may not expand risk
- child scopes may be stricter than parent scopes, but not looser

### 13.3 Recovery Priority And Budgets

Recovery should:

- have higher default priority than normal strategy intents
- own explicit retry and concurrency budgets
- be able to raise overlay severity such as `NoNewRisk`, `ReduceOnly`, or `RecoveryOnly`

Strategy evaluation must not expand risk on the same locked scope while recovery is active.

### 13.4 Recovery Success Criteria

Recovery is successful only when convergence invariants are satisfied.

At minimum, these invariants should be expressible per recovery kind:

- no unresolved attempt remains for the relevant scope
- the relevant recovery lock can be released
- related exposure is no longer uncertain
- order, relayer, inventory, and CTF state agree in authoritative views
- no blocking reconcile item remains for that scope

For `neg-risk`, family-level recovery readiness is a launch gate. `neg-risk` must not move from `Shadow` to `Live` before those invariants are testable.

## 14. Replay And Drift Contracts

### 14.1 Deterministic Replay

Replay must consume local authoritative order, not external timestamp order.

### 14.2 Drift Diagnosis

Replay drift must be diagnosable, not merely observable.

Minimum drift dimensions:

- source drift
- normalization drift
- policy drift
- code or logic drift
- missing-history drift

To support this, artifacts should preserve anchors such as:

- journal sequence
- payload schema version
- normalizer version or raw payload hash
- policy version or matched rule id
- snapshot identity
- execution attempt identity

### 14.3 Drift Response

When drift is discovered, the system must not silently proceed as if nothing happened.

Drift must escalate into one or more of:

- replay warnings
- contract test failures
- runtime reconciliation requirements
- operator-visible alerts

## 15. Testing Strategy

Testing should be organized across these layers:

1. domain tests
   - execution modes
   - state predicates
   - halt precedence
   - recovery scope lock semantics

2. state applier and snapshot contract tests
   - idempotency
   - ordering
   - duplicate handling
   - deferred and reconcile-required handling
   - projection publish atomicity
   - coalescing without loss of the latest stable snapshot

3. unified pipeline tests
   - shared handling of strategy and recovery inputs
   - activation and risk separation
   - full decision-to-attempt object chain stability

4. shadow/live parity tests
   - identical plans from identical inputs
   - identical pre-sink artifacts
   - no authoritative trading-state pollution from shadow

5. recovery contract tests
   - unresolved attempts entering recovery
   - lock behavior
   - overlay escalation
   - convergence invariant satisfaction

6. replay and drift contract tests
   - deterministic replay from the same journal
   - version anchor preservation
   - capability history explainability
   - shadow stream isolation

7. supervisor and fault-injection tests
   - task crash and restart
   - websocket reconnect gaps
   - relayer poll timeouts
   - snapshot publish stalls
   - dispatcher backlog growth
   - recovery and strategy contention under load
   - capability changes during backlog drain

## 16. Rollout Sequence

### 16.1 Phase 1: Unified Runtime With `full-set` Live

Deliver:

- real `app-live` supervisor wiring
- external fact normalization
- single state applier
- published snapshot dispatch
- activation policy backbone
- shared execution orchestrator
- recovery coordinator skeleton
- `full-set` route in `Live`

Keep `neg-risk` limited to foundation libraries and authoritative family state.

### 16.2 Phase 2: `neg-risk` In Shared `Shadow`

Deliver:

- `neg-risk` decision inputs from published snapshots
- `neg-risk` route-specific risk and planning contracts
- shared orchestration through shadow sink
- shadow/live parity artifacts
- family-scoped recovery hooks and rollout control

Keep `neg-risk` out of authoritative live order placement and settlement effects.

### 16.3 Phase 3: Capability-Matrix Live Rollout

Promote `neg-risk` to `Live` only through capability matrix changes after readiness is proven per family or scope.

Readiness requires at minimum:

- shadow/live parity confidence
- recovery contract coverage
- replay drift diagnostics
- supervisor and fault-injection resilience
- family-level readiness for path feasibility, conversion support, and halt semantics

## 17. Acceptance Criteria

This design is successful when:

- the repository has one runtime backbone for `full-set` and `neg-risk`
- `ActivationPolicy` is the sole authority for execution mode
- strategy and recovery feed one decision path
- `Shadow` and `Live` differ only at the final sink boundary
- uncertainty is modeled as authoritative state, not ad hoc logging
- recovery success is defined by convergence invariants
- replay can explain activation, planning, attempts, and drift causes
- `neg-risk` can move from `Disabled` to `Shadow` to selective `Live` without rewriting runtime architecture
