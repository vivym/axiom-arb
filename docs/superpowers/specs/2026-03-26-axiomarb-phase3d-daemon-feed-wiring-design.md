# AxiomArb Phase 3d Daemon Feed Wiring Design

- Date: 2026-03-26
- Status: Draft for user review
- Project: `AxiomArb`

## 1. Summary

`Phase 3c` proved that the unified runtime can perform real `neg-risk` live submits in a controlled `bootstrap + resume` shape.

What it does not yet prove is that `app-live` can operate as a long-running production daemon.

At current `HEAD`, the repository still has these live-runtime gaps:

- fresh live promotion still depends on explicit operator-supplied targets and inputs
- `app-live` is not yet structured as a continuously running, long-lived supervisor with persistent ingestion loops
- market websocket, user websocket, heartbeat, relayer polling, metadata refresh, reconcile, and recovery are not yet wired together as one continuous runtime
- `Phase 3 neg-risk Live` is still not fully production-enabled

`Phase 3d` should close that daemonization gap without expanding scope into pricing or autonomous target discovery.

The recommended direction is:

- keep `operator-supplied live targets` as the only live decision input for this phase
- upgrade `app-live` from a controlled bootstrap runner into a long-running single-process daemon
- keep the unified authority chain unchanged:
  `ExternalFactEvent -> journal append -> StateApplier -> PublishedSnapshot -> ActivationPolicy -> Risk -> Planner -> Attempt -> Submit/Reconcile`
- split the runtime into supervisor layers so ingestion, authoritative state, and decision scheduling do not collapse into one loop
- prefer `fail-closed` degradation over aggressive self-healing whenever runtime truth cannot be proven

In short:

- `Phase 3c` proved real live closure under `bootstrap + resume`
- `Phase 3d` should prove that the same live closure semantics survive continuous operation

## 2. Current Repository Reality

At current `HEAD`, the repository already contains the core live-submit and restart semantics needed for a daemon:

- [`crates/app-live/src/runtime.rs`](/Users/viv/projs/axiom-arb/crates/app-live/src/runtime.rs) already enforces durable startup anchors and fail-closed resume behavior
- [`crates/app-live/src/supervisor.rs`](/Users/viv/projs/axiom-arb/crates/app-live/src/supervisor.rs) already restores rollout evidence, live execution records, and pending reconcile anchors before resuming work
- [`crates/execution/src/orchestrator.rs`](/Users/viv/projs/axiom-arb/crates/execution/src/orchestrator.rs), [`crates/execution/src/sink.rs`](/Users/viv/projs/axiom-arb/crates/execution/src/sink.rs), and [`crates/venue-polymarket/src/negrisk_live.rs`](/Users/viv/projs/axiom-arb/crates/venue-polymarket/src/negrisk_live.rs) already provide real signer, submit, and reconcile edges
- [`crates/persistence/src/repos.rs`](/Users/viv/projs/axiom-arb/crates/persistence/src/repos.rs) and [`crates/app-replay/tests/negrisk_live_contract.rs`](/Users/viv/projs/axiom-arb/crates/app-replay/tests/negrisk_live_contract.rs) already cover durable live submission records and replay contracts
- [`README.md`](/Users/viv/projs/axiom-arb/README.md) now states that `Phase 3c` closes `bootstrap + resume` live submit closure, while explicitly leaving continuous daemonization and production feed wiring as follow-on work

The remaining gap is no longer the execution backbone.

The remaining gap is continuous runtime wiring:

- no persistent task groups for market websocket, user websocket, heartbeat, relayer polling, and metadata refresh are yet wired into one long-lived `app-live`
- no explicit supervisor layering currently separates ingestion, authoritative state publication, and decision scheduling
- no daemon-level backpressure and degradation posture currently governs how loops should react when runtime truth becomes uncertain
- no continuous loop contract currently guarantees that restart first restores truth, then rehydrates schedulers, and only then resumes external ingestion

## 3. Goals

This design should guarantee the following:

- `Phase 3d` upgrades `app-live` into a long-running daemon without inventing a second runtime
- all long-lived external feed loops still enter the same authoritative fact/apply/snapshot chain
- daemonization does not weaken the fail-closed restart semantics already established in `Phase 3c`
- `operator-supplied live targets` remain the only live decision input in this phase
- reconcile and recovery become continuously scheduled subsystems rather than manually stepped closure paths
- supervisor degradation is explicit, observable, and conservative
- a future market-discovered target source can plug into the daemon without requiring another supervisor redesign

## 4. Non-Goals

This design does not define:

- market-discovered `neg-risk` pricing or autonomous target generation
- a new approval policy or automatic family promotion source beyond the existing operator input surface
- remote signer custody product selection beyond the provider abstraction already established
- operator dashboard UX or full promotion-management tooling
- OTel exporters, collectors, or broader observability beyond what is required to safely daemonize the runtime

## 5. Architecture Decision

### 5.1 Recommended Approach

Use a single process with layered supervisors.

Do not split the runtime into multiple services in this phase.

Do not leave the runtime as one oversized `app-live` loop either.

The core architecture should be:

1. a top-level `AppSupervisor` for process lifecycle and global posture
2. an `IngestSupervisor` for external task groups
3. a `StateSupervisor` for the only authoritative mutation chain
4. a `DecisionSupervisor` for dispatch, reconcile, and recovery scheduling

This preserves the current unified runtime semantics while making the long-running daemon understandable and testable.

### 5.2 Rejected Alternatives

`Keep app-live as one monolithic loop`
- rejected because lifecycle, ingestion, state mutation, and scheduling responsibilities would become too entangled to reason about safely

`Split the daemon into multiple processes immediately`
- rejected because the repo does not need distributed coordination yet, and the current live correctness risks are local authority and restart semantics rather than service decomposition

`Add market-discovered pricing in the same phase`
- rejected because daemonization and autonomous target generation are separate projects and would obscure each other's failure modes

`Prefer self-healing over fail-closed recovery`
- rejected for this phase because aggressive automatic recovery makes it too easy to continue live expansion from uncertain truth

## 6. Supervisor Layers

### 6.1 Top-Level AppSupervisor

The top-level `AppSupervisor` should own:

- configuration loading
- provider and repository assembly
- child supervisor startup and shutdown
- global runtime posture
- fatal escalation and break-glass behavior
- human-readable runtime summary and health aggregation

It must not directly implement websocket loops, relayer polling, or route-specific business logic.

### 6.2 IngestSupervisor

`IngestSupervisor` should own all external fact-producing task groups:

- market websocket
- user websocket
- heartbeat
- relayer poll
- metadata refresh

Each task group may observe external state and emit repo-owned `ExternalFactEvent`s.

No ingestion task may directly mutate authoritative runtime state.

### 6.3 StateSupervisor

`StateSupervisor` should be the only owner of:

- journal append
- `StateApplier`
- runtime progress anchors
- snapshot publication
- projection readiness
- dirty-domain and coalescing bookkeeping

This layer preserves the existing invariant:

`source fact -> journal append -> StateApplier -> PublishedSnapshot`

### 6.4 DecisionSupervisor

`DecisionSupervisor` should own continuous internal scheduling for:

- dispatch
- reconcile
- recovery

It should consume:

- published snapshots
- pending reconcile work
- recovery backlog
- operator-supplied live targets and capability inputs

It should continue to drive the same execution chain already used today.

`RecoveryTaskGroup` remains a child scheduling responsibility of `DecisionSupervisor`, not a parallel authority path.

## 7. Daemon Data Flow

### 7.1 Authoritative Runtime Flow

The daemon flow should remain:

`external task -> ExternalFactEvent -> IngressQueue -> journal append -> StateApplier -> PublishedSnapshot -> SnapshotDispatchQueue -> dispatch/reconcile/recovery scheduling`

Hard rule:

- no external task bypasses journal/apply
- no internal scheduler consumes mutable state directly

### 7.2 Queue Boundaries

The runtime should keep at least three distinct work boundaries:

- `IngressQueue`
  - carries external facts into `StateSupervisor`
- `SnapshotDispatchQueue`
  - carries dirty published snapshots into decision scheduling
- `FollowUpQueue`
  - carries pending reconcile and recovery work

This separation matters because:

- external ingress pressure should not starve follow-up recovery
- snapshot publication lag should be diagnosable separately from input backlog
- fail-closed behavior should be able to freeze decision work without losing durable external facts

### 7.3 Coalescing Rule

The daemon may coalesce intermediate dirty snapshots, but it must not drop the latest stable published snapshot for a dirty domain.

The existing unified-runtime rule still applies:

- intermediate versions may be skipped
- the latest stable published version must eventually be consumed

## 8. Runtime Posture And Fail-Closed Degradation

### 8.1 Supervisor Posture

The top-level runtime should aggregate child-supervisor health into explicit posture states such as:

- `Healthy`
- `DegradedIngress`
- `DegradedDispatch`
- `GlobalHalt`

This posture is not a replacement for `ActivationPolicy`.

Instead:

- supervisor posture becomes one input into activation and scheduling
- `ActivationPolicy` remains the only execution-mode authority

`ReconcilingOnly` and `RecoveryOnly` are not top-level posture states in this design.

They are per-scope runtime restriction inputs derived from durable truth, follow-up backlog, and recovery state.

### 8.2 Scope Restriction Inputs

The daemon may maintain per-scope restriction inputs such as:

- `ReconcilingOnly`
- `RecoveryOnly`

These restrictions:

- are derived from authoritative facts, pending reconcile work, and recovery state
- are consumed by `ActivationPolicy` and scheduling
- do not create a second execution-mode authority outside the unified chain

Hard rule:

- `AppSupervisor` owns only global posture
- scope restriction inputs feed `ActivationPolicy`
- `ActivationPolicy` remains the only component that turns those inputs into execution-mode decisions

### 8.3 Fail-Closed Rules

The daemon should conservatively degrade under these conditions:

- market or user feed gaps that break continuity guarantees
  - suppress related dispatch and move affected scopes toward `ReconcilingOnly`
- relayer truth that cannot explain unresolved pending work
  - move affected scopes toward `RecoveryOnly`
- journal append, apply, or snapshot publish failures
  - freeze downstream decision work until authoritative state is healthy again
- projection readiness lag beyond allowed thresholds
  - stop route scheduling that depends on stale projections
- metadata freshness expiration
  - block new `neg-risk` live expansion until freshness is restored

### 8.4 Recovery Priority

The daemon must preserve the existing recovery-first contract:

- pending reconcile and recovery work have priority over new live expansion
- `RecoveryScopeLock` still blocks same-scope risk expansion
- convergence invariants still determine when scopes may return to normal promotion behavior

## 9. Task Group Responsibilities

### 9.1 MarketDataTaskGroup

Responsibilities:

- maintain market feed sessions
- normalize market/orderbook facts
- detect reconnect and continuity gaps
- emit gap-related facts into the authoritative path

Non-responsibilities:

- no direct family promotion decisions
- no direct store mutation
- no direct execution triggering

### 9.2 UserStateTaskGroup

Responsibilities:

- normalize user/order/fill/account facts
- detect duplicates, gaps, and ordering uncertainty
- emit facts that force `reconcile-required` or scope uncertainty when truth cannot be proven

Non-responsibilities:

- no direct repair behavior
- no direct runtime-mode authority

### 9.3 HeartbeatTaskGroup

In this phase, `heartbeat` means the existing order-heartbeat truth path, not generic daemon process liveness.

Responsibilities:

- monitor order-heartbeat freshness and validity
- normalize heartbeat success and attention facts into the authoritative fact path
- detect missed, stale, or invalid heartbeat thresholds
- emit facts that force reconcile follow-up or more conservative scope restrictions when heartbeat truth cannot be proven

Non-responsibilities:

- no generic process-liveness health checks
- no direct provider side effects
- no direct live promotion authority

### 9.4 RelayerTaskGroup

Responsibilities:

- poll recent transactions, nonce, and related relayer truth
- normalize relayer-side pending, confirmed, and terminal facts
- support durable reconcile work cadence

Non-responsibilities:

- no direct clearing of pending reconcile work
- no direct declaration that recovery is complete

### 9.5 MetadataTaskGroup

Responsibilities:

- refresh family/member metadata and validation state
- emit metadata freshness and validation facts
- keep `neg-risk` inclusion/exclusion truth current

Non-responsibilities:

- no autonomous live target generation
- no independent promotion to `Live`

### 9.6 DecisionTaskGroup

Responsibilities:

- consume published snapshots and dirty-domain notifications
- schedule dispatch, reconcile, and recovery work
- apply backlog budgets and coalescing
- suppress work when supervisor posture or activation inputs require it

Non-responsibilities:

- no raw external fact production
- no mutable-state shortcuts around `StateSupervisor`

### 9.7 RecoveryTaskGroup

Responsibilities:

- prioritize unresolved and uncertain follow-up work
- maintain `RecoveryScopeLock`
- enforce retry and budget rules
- release locks only after convergence invariants hold

Non-responsibilities:

- no direct provider side effects outside the unified decision chain

## 10. Operator Inputs In Phase 3d

### 10.1 Scope

`Phase 3d` keeps operator inputs as the only source of:

- which families are candidates for live promotion
- what member-level live target parameters exist for those families

The daemon is responsible for deciding when existing live targets may safely advance.

It is not responsible for inventing new live targets.

### 10.2 Why This Boundary Matters

Keeping operator inputs in place for this phase:

- isolates daemonization from pricing/discovery concerns
- keeps failure analysis readable
- lets `Phase 3e` plug in a new target source without changing daemon lifecycle semantics

### 10.3 Runtime Representation

Operator inputs should not remain ad hoc environment-only truth.

They should be represented through a repo-owned runtime input/config surface with clear revision identity so the system can answer:

- which target revision was active
- why a family was considered eligible at that moment
- which restart resumed against which operator-supplied target set

For `Phase 3d`, this input surface is startup-scoped and restart-scoped only.

It is explicitly not an in-process hot-reload surface in this phase.

That means:

- operator target revisions are loaded during startup or controlled restart
- the active revision must be recorded through durable runtime metadata or startup anchors sufficient for restart, summary, and postmortem inspection
- resume must be able to prove which operator-target revision it resumed against
- changing live targets mid-process requires a later phase rather than ad hoc mutation in `Phase 3d`

This does not require full operator tooling in `Phase 3d`, but it does require the daemon to stop treating operator input as invisible ambient state.

### 10.4 Validation Contract

`Phase 3d` should test this boundary explicitly:

- startup reports the operator-target revision it loaded
- restart resumes against that same revision unless a controlled restart loads a new revision
- the daemon does not support arbitrary in-process target mutation
- postmortem/runtime summary can explain which revision made a family eligible

## 11. Restart And Resume

### 11.1 Startup Order

The daemon should restart in this order:

1. restore durable runtime truth
2. restore `StateSupervisor`
3. restore `DecisionSupervisor`
4. resume external ingestion task groups

This prevents fresh external facts from racing ahead of restored reconcile or recovery truth.

### 11.2 Resume Rule

Resume must remain truth-first:

- restore committed progress anchors
- restore pending reconcile and live submission anchors
- restore the active operator-target revision anchor
- rebuild published snapshot state
- re-establish recovery posture
- only then resume continuous loops

### 11.3 No Duplicate Live Promotion

Restart must never re-submit live attempts simply because loops restarted.

If unresolved live truth already exists for a scope:

- that scope must stay constrained until reconcile or recovery converges
- new live expansion must remain suppressed

## 12. Testing Strategy

### 12.1 Supervisor Lifecycle Tests

Test:

- layered startup ordering
- layered shutdown ordering
- child-task crash propagation
- fatal escalation and fail-closed posture changes
- restart sequencing that restores truth before resuming loops

### 12.2 Ingestion And State Pipeline Tests

Test:

- every feed task re-enters the same `ExternalFactEvent -> journal -> apply` path
- heartbeat facts re-enter that same authoritative path rather than bypassing it
- queue backpressure does not violate authoritative ordering
- snapshot coalescing does not lose the latest stable publication
- projection lag blocks dependent scheduling rather than permitting stale execution

### 12.3 Daemon Degradation Tests

Test:

- market/user feed gaps
- missed or invalid order heartbeat facts
- relayer uncertainty
- metadata staleness
- apply/publish failures
- dispatch suppression under degraded posture

### 12.4 Decision And Recovery Scheduling Tests

Test:

- reconcile and recovery priority over fresh live expansion
- `RecoveryScopeLock` behavior under continuous scheduling
- suppression of live targets while global posture or scope restriction inputs remain restrictive
- recovery convergence releasing scopes back to eligible promotion

### 12.5 Operator Input Lifecycle Tests

Test:

- startup records and reports the active operator-target revision
- restart resumes against the same revision unless a controlled restart provides a new one
- missing or inconsistent operator-target revision anchors fail closed
- in-process live-target mutation is not supported in this phase

### 12.6 End-To-End Daemon Tests

Test at least:

- happy-path continuous ingest -> publish -> dispatch -> submit -> reconcile
- degraded-path feed gap or ambiguous relayer truth leading to fail-closed posture
- restart from non-empty durable state without duplicate live submit

## 13. Acceptance Criteria

`Phase 3d` is complete when all of the following are true:

- `app-live` operates as a long-running single-process daemon with layered supervisors
- market websocket, user websocket, heartbeat, relayer poll, and metadata refresh are continuously wired through the authoritative fact chain
- missed or invalid order heartbeat truth causes reconcile-required work or more conservative runtime posture for affected scopes
- reconcile, recovery, and dispatch are continuously scheduled rather than manually stepped
- `operator-supplied live targets` remain the only live decision input in this phase
- `operator-supplied live targets` are represented by a startup/restart-scoped repo-owned revisioned input surface rather than invisible ambient config
- runtime summary and restart behavior can identify which operator-target revision was active
- critical runtime uncertainty causes conservative fail-closed posture transitions or per-scope restriction inputs
- restart restores truth before resuming loops and does not duplicate live promotion
- `Phase 3e` can be added later by changing the live target source rather than redesigning the daemon core

## 14. Follow-On Work

This design intentionally leaves later phases for:

- market-discovered `neg-risk` pricing and autonomous live target generation
- richer operator control and promotion management tooling
- broader observability and exporter-backed production telemetry
- any future multi-process decomposition, if the single-process layered daemon later proves insufficient
