# AxiomArb Phase 3c Neg-Risk Live Submit Closure Design

- Date: 2026-03-25
- Status: Draft for user review
- Project: `AxiomArb`

## 1. Summary

`Phase 3b` gave the repository a `neg-risk` live backbone that can:

- promote config-backed, approved, and ready families through the unified runtime
- build route-aware `neg-risk` family submission plans
- create request-bound attempts
- materialize live artifact payloads and venue request bodies
- persist and replay those artifacts during bootstrap and resume

What it does not yet do is close the real live execution loop.

`Phase 3c` should add that missing closure while keeping the unified runtime architecture intact.

The recommended direction is:

- keep `operator-supplied family live targets` as the decision input for this phase
- keep the existing unified decision chain unchanged up to `ExecutionAttempt`
- add real signer, submit, reconcile, and recovery provider boundaries behind that chain
- treat live submit and reconcile as `external facts` that must re-enter the authoritative journal/apply/snapshot path
- support only `bootstrap + resume` operation in this phase, not a full continuous daemon

In short:

- `Phase 3b` proved the runtime can prepare live work
- `Phase 3c` should prove the runtime can safely submit, recover, and resume real live work

## 2. Current Repository Reality

At current `HEAD`, the repository already contains most of the pre-submit `Phase 3b` surface:

- [`crates/execution/src/negrisk.rs`](/Users/viv/projs/axiom-arb/crates/execution/src/negrisk.rs) can build route-aware `neg-risk` family submission plans
- [`crates/execution/src/signing.rs`](/Users/viv/projs/axiom-arb/crates/execution/src/signing.rs) and [`crates/execution/src/sink.rs`](/Users/viv/projs/axiom-arb/crates/execution/src/sink.rs) already define a narrow signing and sink boundary for `neg-risk` family submits
- [`crates/app-live/src/negrisk_live.rs`](/Users/viv/projs/axiom-arb/crates/app-live/src/negrisk_live.rs) can build live artifacts and request bodies during bootstrap-time promotion
- [`crates/venue-polymarket/src/rest.rs`](/Users/viv/projs/axiom-arb/crates/venue-polymarket/src/rest.rs) already knows how to build signed order submission requests
- [`crates/persistence/src/repos.rs`](/Users/viv/projs/axiom-arb/crates/persistence/src/repos.rs) and [`crates/app-replay/tests/negrisk_live_contract.rs`](/Users/viv/projs/axiom-arb/crates/app-replay/tests/negrisk_live_contract.rs) already cover live attempt anchors, pending reconcile rows, and replay contracts

The remaining gap is not planning or artifact generation. The remaining gap is authoritative live closure:

- [`crates/app-live/src/supervisor.rs`](/Users/viv/projs/axiom-arb/crates/app-live/src/supervisor.rs) still treats `neg-risk` live promotion as a bootstrap-time harness flow rather than a true external submit/reconcile cycle
- [`crates/app-live/src/runtime.rs`](/Users/viv/projs/axiom-arb/crates/app-live/src/runtime.rs) still centers on static bootstrap/reconcile snapshots rather than concrete venue-side submit closure
- [`crates/venue-polymarket/src/orders.rs`](/Users/viv/projs/axiom-arb/crates/venue-polymarket/src/orders.rs) and [`crates/venue-polymarket/src/relayer.rs`](/Users/viv/projs/axiom-arb/crates/venue-polymarket/src/relayer.rs) are not yet wired into a `neg-risk` live bootstrap/resume provider loop
- [`README.md`](/Users/viv/projs/axiom-arb/README.md) still correctly states that `app-live` does not yet place `neg-risk` orders and that `Phase 3 neg-risk Live` is not fully enabled

## 3. Goals

This design should guarantee the following:

- `Phase 3c` closes the real live submit path without creating a second runtime
- live `neg-risk` submit still flows through `ActivationPolicy -> Risk -> Planner -> Attempt`
- real signer, submit, reconcile, and recovery behavior are modeled as repo-owned provider contracts
- live submit results do not directly mutate authoritative trading state
- authoritative state changes still happen only through journaled fact application
- ambiguous submit or reconcile outcomes always create durable follow-up work
- restart and resume recover only from durable truth, never from environment or in-memory reconstruction
- a future continuous daemon can reuse these semantics rather than replacing them

## 4. Non-Goals

This design does not define:

- market-discovered `neg-risk` pricing or autonomous live-target generation
- a continuous websocket/heartbeat/relayer-poll production daemon
- a final remote signer, HSM, or custody product choice
- operator dashboard UX for family promotion management
- generalized multi-venue concrete implementations beyond one initial provider

## 5. Architecture Decision

### 5.1 Recommended Approach

Use the existing unified runtime backbone and extend only the live execution closure boundary.

The core idea is:

1. keep `DecisionInput`, activation, risk, planning, and attempt creation unchanged
2. introduce repo-owned provider contracts for:
   - signing
   - venue submission
   - venue reconciliation
3. treat provider outputs as standardized external facts, not direct business-state mutation
4. persist enough durable anchors to resume unresolved live attempts safely
5. let recovery continue to use the same decision backbone as normal route work

### 5.2 Rejected Alternatives

`Direct submit from app-live bootstrap glue`
- rejected because it would create a bypass around the unified execution and recovery chain

`Treat successful HTTP submission as authoritative trading truth`
- rejected because local submit success is not the same as authoritative venue confirmation

`Add discovery pricing and live execution in the same phase`
- rejected because it would merge two independent projects and make both harder to validate

`Jump straight to a continuous daemon`
- rejected because the restart/resume and durable closure contract should be proven first in a controlled `bootstrap + resume` shape

## 6. System Boundaries

### 6.1 Composition Root

`app-live` remains the composition root.

For `Phase 3c`, it should assemble:

- `ActivationPolicy`
- `RiskEngine`
- `ExecutionPlanner`
- `ExecutionOrchestrator`
- `RecoveryCoordinator`
- `SignerProvider`
- `VenueExecutionProvider`
- `VenueReconcileProvider`
- persistence and replay sinks

`app-live` must not grow venue-specific business logic. It may choose concrete providers, but it must not absorb their semantics.

### 6.2 Decision Backbone

The shared chain remains:

`DecisionInput -> ActivationDecision -> RiskVerdict -> ExecutionRequest -> ExecutionPlan -> ExecutionAttempt`

`Phase 3c` deliberately does not change:

- the decision input shape
- the rollout capability model
- the family readiness model
- the upstream route-aware planner inputs already landed in `Phase 3b`

The only new behavior is what happens after a request-bound attempt reaches the live edge.

### 6.3 Recovery Boundary

`RecoveryCoordinator` continues to:

- discover unresolved or ambiguous live work
- emit `RecoveryIntent`
- re-enter the same `ActivationPolicy -> Risk -> Planner -> Attempt` path

It must not gain a direct venue-repair side channel.

## 7. Live Submit And Reconcile Data Flow

### 7.1 Real Submit Flow

The recommended live path is:

`DecisionInput -> Activation -> Risk -> Plan -> Attempt -> SignerProvider -> VenueExecutionProvider -> ExternalFactEvent -> journal append -> StateApplier -> authoritative state -> snapshot publish`

Hard rule:

- real submit may observe and normalize external facts
- only `StateApplier` may mutate authoritative state

### 7.2 Local Acceptance Versus Authoritative Confirmation

`Phase 3c` must explicitly distinguish:

- `SubmitAcceptedLocally`
  - the request was signed and handed to the venue/relayer successfully enough to produce durable references
- `SubmitConfirmedAuthoritatively`
  - later facts prove the attempt entered authoritative trading truth
- `SubmitAmbiguous`
  - the runtime cannot prove success or failure and must create durable follow-up work

This prevents "HTTP success" from being treated as "business success."

### 7.3 Reconcile Flow

Reconciliation must follow the same fact-ingest rule as submit.

The recommended path is:

`PendingReconcileItem -> VenueReconcileProvider -> ExternalFactEvent -> journal append -> StateApplier -> authoritative state -> snapshot publish`

Hard rules:

- reconcile may query venue or relayer state, but it may not directly mutate authoritative business state
- reconcile success is represented by journaled facts, not by in-memory provider side effects
- restart safety depends on durable pending-reconcile anchors rather than ad hoc "resume from wherever the provider left off" behavior

### 7.4 Durable Anchors

`Phase 3c` should make four live objects durable and replayable:

- `ExecutionPlan`
- `ExecutionAttempt`
- `LiveSubmissionRecord`
- `PendingReconcileItem` or equivalent durable reconcile cursor

These anchors define restart truth:

- what the runtime intended to do
- what it already attempted
- which external references were observed
- what still requires reconciliation or recovery

### 7.5 Ambiguous Outcomes

Every `FailedAmbiguous` or `SubmitAmbiguous` outcome must create at least one durable follow-up artifact:

- `StateConfidence::Uncertain`
- a pending reconcile item
- a `RecoveryIntent`
- or a scope-level overlay such as `ReduceOnly` or `RecoveryOnly`

Warning-only handling is not acceptable.

## 8. Provider Contracts

### 8.1 Signer Provider

`SignerProvider` is responsible only for turning signable submission material into signed submission material.

It should accept:

- canonical request material
- route and family references
- attempt identity
- signer identity requirement
- signing-schema version

It should return repo-owned outcomes, not raw signer-library output.

The interface should be provider-shaped from day one, even if the first implementation is a local signer.

### 8.2 Venue Execution Provider

`VenueExecutionProvider` is responsible only for performing a real submit and normalizing the observable result.

It must not:

- update positions directly
- update authoritative order state directly
- bypass journaling

It should return outcomes shaped roughly as:

- `Accepted { submission_record }`
- `RejectedDefinitive { reason }`
- `AcceptedButUnconfirmed { submission_record }`
- `Ambiguous { pending_ref, reason }`

### 8.3 Venue Reconcile Provider

`VenueReconcileProvider` is responsible for converting unresolved live work into authoritative follow-up facts.

It should consume durable reconcile work items, not ad hoc in-memory attempt state.

Its outcomes should distinguish:

- `ConfirmedAuthoritative`
- `StillPending`
- `NeedsRecovery`
- `FailedAmbiguous`
- `FailedDefinitive`

### 8.4 Concrete Provider Placement

The first concrete implementation may be `Polymarket`, but the abstraction boundary should remain venue-agnostic:

- `domain`, `risk`, and strategy crates must not import concrete venue clients
- `execution` depends on traits and repo-owned provider outcomes
- `app-live` chooses which concrete providers to assemble

## 9. Authoritative State And Recovery Precedence

### 9.1 Authoritative Live State

`Phase 3c` should add explicit runtime truth for:

- live submission status per attempt
- pending reconcile items
- scope-level uncertainty markers
- recovery overlays and locks
- family-level eligibility to promote or remain constrained

Live closure must not rely only on artifact streams.

### 9.2 Authority Chain

The precedence chain remains:

`durable overlays + family halt + recovery scope lock + rollout capability -> ActivationPolicy -> Risk -> Planner -> Attempt -> Submit/Reconcile`

Rules:

- `ActivationPolicy` remains the only execution-mode authority
- `Risk` answers admissibility inside that mode
- recovery may tighten overlays, but may not bypass activation or risk

### 9.3 Convergence Rule

Recovery is complete only when convergence invariants are satisfied.

At minimum:

- the relevant attempt is no longer unresolved
- the pending reconcile item is gone or terminal
- the scope lock can be released
- the affected exposure is no longer `Uncertain`
- the family is no longer constrained by temporary recovery overlays unless a real safety issue remains

## 10. Bootstrap And Resume Behavior

### 10.1 Bootstrap

Fresh bootstrap must first restore durable truth before deciding whether to send any new live submit.

Rules:

- if there are unresolved attempts, pending reconcile items, or active recovery locks in a scope, that scope may not emit a new live submit
- only families that are config-backed, approved, ready, and free of unresolved live truth may promote to new live submit

### 10.2 Resume

Resume must prioritize recovery over fresh promotion.

Order of operations:

1. restore durable live anchors
2. restore and publish authoritative snapshot state
3. rebuild active overlays and scope locks
4. reconcile unresolved live work
5. only then consider new live promotion

Resume must never reconstruct live attempts from:

- environment variables
- operator input alone
- stale in-memory sets

### 10.3 No Continuous-Daemon Requirement

This phase intentionally stops short of a full daemon.

That means:

- provider and reconcile steps may be supervisor-driven and stepwise
- successful closure must not depend on websocket loops already existing
- later daemonization should reuse these provider and durability semantics rather than redefining them

## 11. Persistence And Replay Contracts

### 11.1 Repo-Owned Fact Shapes

All signer, submit, and reconcile observations must be converted into repo-owned fact schemas before entering:

- journal storage
- persistence tables
- replay surfaces

Raw third-party payloads may be retained as supplemental evidence, but they must not become the contract boundary.

### 11.2 Replayability

Replay must be able to explain:

- which family was eligible for live submit
- which attempt was created
- what was signed
- what was submitted
- which references were durable
- whether the outcome was definitive, pending, or ambiguous
- which reconcile or recovery work followed

### 11.3 Restart Safety

If durable anchors are missing or inconsistent, the system must fail closed rather than inferring a probably-correct state.

## 12. Testing Strategy

### 12.1 Provider Contract Tests

Test:

- signer canonicalization and identity binding
- definitive versus ambiguous signer failures
- submit accepted/rejected/ambiguous semantics
- reconcile confirmed/pending/recovery-required semantics

### 12.2 Live Backbone Integration Tests

Test the full chain:

`DecisionInput -> Activation -> Risk -> Plan -> Attempt -> Sign -> Submit -> ExternalFactEvent -> Apply -> Snapshot -> Reconcile -> Recovery/Convergence`

Cover at least:

- happy-path authoritative confirmation
- ambiguous submit generating reconcile work
- definitive reject leaving no authoritative exposure side effects
- unresolved scope suppressing new live promotion

### 12.3 Durable Resume Tests

Test:

- restart with submission records but no final confirmation
- restart with pending reconcile work
- restart refusing to re-submit when durable live truth already exists
- missing-anchor failure paths

### 12.4 Recovery Precedence Tests

Test:

- active recovery scope lock blocks new live expansion
- ambiguous outcomes force durable follow-up artifacts
- recovery overlays suppress `Live` until convergence
- convergence releases the lock and restores eligible promotion behavior

### 12.5 Replay And Drift Tests

Test:

- live attempt and submission artifacts replay deterministically
- resume does not fabricate live attempts
- versioned provider normalization can be diagnosed if drift appears

## 13. Acceptance Criteria

This design is successful when:

- `app-live` can perform a real `neg-risk` live submit in `bootstrap + resume` mode for config-backed, approved, ready families
- real signer, submit, reconcile, and recovery all use the unified runtime backbone rather than a side path
- real submit does not directly mutate authoritative trading state
- every authoritative state change still comes from journaled fact application
- ambiguous outcomes always create durable reconcile or recovery work
- resume restores only from durable truth
- unresolved scopes cannot expand live risk until convergence is re-established
- a future continuous daemon can reuse these contracts without rewriting the architecture

## 14. Follow-On Work

If this design is implemented successfully, separate later plans may address:

- market-discovered `neg-risk` pricing and live-target generation
- continuous daemonization of websocket, heartbeat, relayer-poll, and dispatch loops
- remote signer and custody integrations
- operator tooling and dashboards for family promotion management
