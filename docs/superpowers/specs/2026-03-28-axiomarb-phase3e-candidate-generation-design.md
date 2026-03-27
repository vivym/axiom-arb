# AxiomArb Phase 3e Candidate Generation Design

- Date: 2026-03-28
- Status: Draft for user review
- Project: `AxiomArb`

## 1. Summary

`Phase 3d` upgraded `app-live` into a layered single-process daemon with repo-owned ingress, authoritative apply/snapshot publication, continuous scheduling, and fail-closed lifecycle posture.

What it still does not solve is the source of `neg-risk` live targets.

At current `HEAD`, the repository can:

- run the unified runtime continuously
- perform real `neg-risk` live submits
- restore live attempts and follow-up truth durably
- keep `operator-supplied live targets` as the single live decision input

What it still cannot do is:

- continuously discover the family universe
- validate and price discovered families into trade candidates
- explain and persist a candidate-generation result as a first-class repo-owned artifact
- bridge those candidates into an operator-adoptable target revision without turning discovery directly into trading authority

`Phase 3e` should close that gap.

The recommended direction is:

- add a dedicated `DiscoverySupervisor` and discovery pipeline alongside the existing daemon supervisors
- continue to route all discovery/backfill observations through repo-owned facts and the authoritative journal/apply/snapshot chain
- generate conservative, replayable `CandidateTargetSet` artifacts on published snapshots
- generate an `AdoptableTargetRevision` bridge artifact that can be explicitly adopted by operator/control-plane workflows
- keep live execution authority unchanged: discovery generates candidates, but does not directly promote them into live trading

In short:

- `Phase 3d` proved continuous runtime wiring
- `Phase 3e` should prove continuous candidate generation and adoption-ready target rendering

## 2. Current Repository Reality

At current `HEAD`, the repository already contains the runtime pieces that make `Phase 3e` feasible:

- [`crates/app-live/src/daemon.rs`](/Users/viv/projs/axiom-arb/crates/app-live/src/daemon.rs) and [`crates/app-live/src/supervisor.rs`](/Users/viv/projs/axiom-arb/crates/app-live/src/supervisor.rs) now provide the layered daemon entrypoint and truth-first startup ordering
- [`crates/app-live/src/runtime.rs`](/Users/viv/projs/axiom-arb/crates/app-live/src/runtime.rs) already enforces the authoritative `journal -> StateApplier -> PublishedSnapshot` chain and durable startup anchors
- [`crates/venue-polymarket/src/ws_client.rs`](/Users/viv/projs/axiom-arb/crates/venue-polymarket/src/ws_client.rs), [`crates/venue-polymarket/src/ws_market.rs`](/Users/viv/projs/axiom-arb/crates/venue-polymarket/src/ws_market.rs), [`crates/venue-polymarket/src/ws_user.rs`](/Users/viv/projs/axiom-arb/crates/venue-polymarket/src/ws_user.rs), and [`crates/venue-polymarket/src/heartbeat.rs`](/Users/viv/projs/axiom-arb/crates/venue-polymarket/src/heartbeat.rs) now provide the daemon feed edges
- [`crates/state/src/apply.rs`](/Users/viv/projs/axiom-arb/crates/state/src/apply.rs), [`crates/state/src/facts.rs`](/Users/viv/projs/axiom-arb/crates/state/src/facts.rs), and [`crates/state/src/store.rs`](/Users/viv/projs/axiom-arb/crates/state/src/store.rs) already encode daemon attention and follow-up truth through repo-owned facts
- [`README.md`](/Users/viv/projs/axiom-arb/README.md) now explicitly states that live target selection still comes from operator inputs and that market-discovered target generation remains follow-on work

The remaining gap is no longer daemon lifecycle or live submit closure.

The remaining gap is target generation:

- no dedicated discovery pipeline continuously produces a repo-owned family universe
- no repo-owned candidate artifact exists between discovery/validation/pricing and operator target adoption
- no bridge artifact exists for turning candidate output into an adoptable target revision
- no replay/drift contract yet explains how a candidate was generated from a particular snapshot and policy set

## 3. Goals

This design should guarantee the following:

- `Phase 3e` adds a continuous candidate-generation subsystem without inventing a second runtime
- discovery and backfill observations still enter the same authoritative fact/apply/snapshot chain as the rest of the daemon
- candidate generation consumes published truth, not mutable in-memory state
- `CandidateTargetSet` is repo-owned, durable, replayable, and observable
- candidate generation remains conservative and advisory in this phase
- a bridge artifact exists to render candidates into an operator-adoptable target revision
- operator adoption remains explicit and separate from candidate generation
- future ranking and budget-aware selection can be added later without redesigning `Phase 3e` core objects

## 4. Non-Goals

This design does not define:

- direct auto-promotion from discovered candidates into live trading
- market-making or alpha-optimized ranking
- budget-aware portfolio-wide candidate competition
- hot-reloaded operator target mutation inside the running daemon
- a dashboard or full operator-control UX

## 5. Architecture Decision

### 5.1 Recommended Approach

Add a distinct `DiscoverySupervisor` and discovery pipeline.

Do not embed candidate generation ad hoc inside existing runtime loops.

Do not directly connect discovery output to live execution.

The core architecture should be:

1. `AppSupervisor` remains the top-level composition root
2. `DiscoverySupervisor` runs alongside `IngestSupervisor`, `StateSupervisor`, and `DecisionSupervisor`
3. `DiscoverySupervisor` owns family discovery, validation, conservative pricing, and candidate publishing
4. `CandidateBridge` renders an adoptable revision artifact for operator/control-plane use
5. the live execution path remains unchanged and still consumes only adopted operator-target revisions

This keeps target generation and trading authority separate while still making them composable.

### 5.2 Rejected Alternatives

`Embed candidate generation directly inside app-live decision scheduling`
- rejected because discovery, validation, pricing, and adoption bridging would become entangled with existing execution/recovery lifecycle code

`Auto-feed candidates directly into live execution`
- rejected because it would collapse candidate generation and rollout authority into one step, violating the selected phase boundary

`Ship only a discovery universe without candidate artifacts`
- rejected because it would create a low-value intermediate state that still lacks an adoption-ready output

`Do full ranking and budget-aware selection immediately`
- rejected because this phase should first prove reliable candidate generation before adding more aggressive selection semantics

## 6. Supervisor And Component Boundaries

### 6.1 AppSupervisor

`AppSupervisor` remains the top-level composition root.

It continues to own:

- configuration loading
- provider and repository assembly
- lifecycle of the existing daemon supervisors
- global posture and break-glass behavior

In `Phase 3e`, it additionally assembles and starts a `DiscoverySupervisor`.

### 6.2 DiscoverySupervisor

`DiscoverySupervisor` owns the continuous candidate-generation subsystem.

It is responsible for:

- family universe discovery
- discovery/backfill data refresh
- validation and path feasibility checks
- conservative pricing and size hints
- publishing `CandidateTargetSet`
- producing `AdoptableTargetRevision`

It must not:

- modify execution modes
- submit orders
- override `ActivationPolicy`
- replace explicit operator approval

### 6.3 FamilyDiscoveryEngine

This component is responsible for maintaining the repo-owned family universe view.

It should:

- discover family/member sets from daemon inputs plus dedicated backfill sources
- produce durable `FamilyDiscoveryRecord`s
- keep provenance and revision anchors for each record

It should not infer trade-worthiness or execution intent.

### 6.4 CandidateValidationEngine

This component is responsible for structured candidate admissibility.

It should evaluate:

- metadata completeness
- inclusion/exclusion rules
- path feasibility
- family/member semantic validity
- safety preconditions that must hold before pricing

It should emit machine-consumable verdicts, not free-text-only explanations.

### 6.5 CandidatePricingEngine

This component is responsible for conservative advisory output.

It should:

- compute advisory entry price or price band
- compute advisory size cap
- attach explanation and confidence metadata

It should not produce:

- order bodies
- execution requests
- competitive ranking
- portfolio budget allocation

### 6.6 CandidateBridge

This component is the only bridge from candidate output to operator-adoptable live target revisions.

It should:

- render `CandidateTargetSet` into an `AdoptableTargetRevision`
- preserve policy versions, warnings, and compatibility information
- stay explicit about the fact that the rendered output is adoptable, not automatically adopted

## 7. Data Flow And Authoritative Boundaries

### 7.1 Discovery Input Flow

All new discovery/backfill sources must remain within the repo-owned fact boundary:

`external discovery source -> ExternalFactEvent -> journal append -> StateApplier -> PublishedSnapshot`

Hard rule:

- discovery/backfill sources may not directly maintain an authoritative in-memory candidate truth outside the journal/apply path

### 7.2 Candidate Generation Flow

Candidate generation should follow:

`PublishedSnapshot -> FamilyDiscoveryEngine -> CandidateValidationEngine -> CandidatePricingEngine -> CandidateTargetSet`

The discovery pipeline must consume published snapshots and repo-owned projections.

It must not consume half-applied mutable runtime state.

### 7.3 Bridge Flow

The bridge path should be:

`CandidateTargetSet -> CandidateBridge -> AdoptableTargetRevision -> operator/control-plane adoption -> startup-scoped operator-target revision`

This boundary is intentional:

- `CandidateTargetSet` is not a live trading input
- `AdoptableTargetRevision` is not automatically active
- live execution still begins only after explicit adoption into the existing operator-target surface

### 7.4 Replay And Drift Anchors

Every candidate-generation output must be attributable to stable anchors.

At minimum, `CandidateTargetSet` and `AdoptableTargetRevision` should carry:

- `snapshot_id`
- `candidate_revision`
- `source_revision`
- `discovery_policy_version`
- `validation_policy_version`
- `pricing_policy_version`
- `bridge_policy_version`

This keeps candidate-generation drift diagnosable:

- source drift
- validation drift
- pricing drift
- bridge-render drift

## 8. Core Objects And Contracts

### 8.1 FamilyDiscoveryRecord

This is the repo-owned record of family universe discovery.

It should include:

- `family_id`
- `member set`
- `source provenance`
- `discovered_at`
- `snapshot_id`
- `discovery_revision`

It answers:

- what family did we see
- what members did we think it had
- on what published truth and input revision did we conclude that

It does not answer whether the family should be traded.

### 8.2 CandidateValidationResult

This is the structured validation verdict for a discovered family.

It should include verdict classes such as:

- `Included`
- `Excluded`
- `Deferred`
- `InsufficientEvidence`

It should also include:

- reason code
- metadata completeness flags
- path feasibility summary
- unmet predicates

This object must be machine-consumable and replay-stable.

### 8.3 CandidateTarget

This is the core family/member-level candidate object.

It should include:

- `family_id`
- `members`
- `advisory_price` or advisory price band
- `advisory_size`
- `path_summary`
- `validation_revision`
- `pricing_policy_version`
- `based_on_snapshot_id`
- `explanation_envelope`

It must remain an advisory candidate object, not an execution object.

### 8.4 CandidateTargetSet

This is the primary `Phase 3e` output artifact.

It should include:

- `candidate_revision`
- `based_on_snapshot_id`
- `generated_at`
- `family_candidates`
- `excluded_families`
- `deferred_families`
- `policy_versions`
- `source_revision`

This object must be:

- durable
- replayable
- observable
- explicitly separate from authoritative trading state

### 8.5 AdoptableTargetRevision

This is the bridge artifact emitted by `CandidateBridge`.

It should include:

- `candidate_revision`
- `adoptable_revision`
- `rendered_live_targets`
- `rendered_at`
- `compatibility_summary`
- `adoption_warnings`

Its role is to provide a stable render target for operator/control-plane adoption.

It is not itself an execution authorization.

## 9. Conservative Pricing And Selection Policy

### 9.1 Eligibility First

The first version of `Phase 3e` should be eligibility-first rather than alpha-first.

The pipeline order should be:

1. family discovered
2. metadata sufficiently complete
3. member semantics interpretable
4. path feasibility proven
5. exclusion rules not triggered
6. conservative pricing generated

This means the phase prioritizes reliable candidate generation over opportunity maximization.

### 9.2 Advisory Pricing Only

The pricing surface should stay conservative.

It may emit:

- advisory entry price
- advisory price band
- advisory size cap
- confidence tier

It should not emit:

- aggressive point-estimate strategy ranking
- venue-specific execution parameters
- request bodies or order instructions

### 9.3 Size Caps, Not Competitive Budgeting

The first version should produce:

- per-family size cap
- per-member size hint
- optional confidence tier

It should not yet solve:

- cross-family budget competition
- portfolio-wide ranking
- opportunity ordering under limited capital

Those belong in a later phase.

### 9.4 Conservative Exclusion Bias

`Phase 3e` should prefer `Deferred` or `Excluded` over weak inclusion.

Specifically:

- incomplete metadata should default to `Deferred`
- infeasible or unproven paths should default to `Deferred`
- placeholder, augmented, direct `Other`, or semantically invalid families should default to `Excluded`
- ambiguity that prevents stable interpretation should suppress candidate output

This keeps the candidate pipeline honest.

## 10. Error Handling And Replay Semantics

Candidate-generation failures should remain structured and diagnosable.

Suggested layered semantics:

- discovery source failure -> repo-owned attention fact and degraded candidate generation
- validation insufficiency -> `Deferred` / `InsufficientEvidence`
- pricing inability -> `Deferred` rather than fake candidate output
- bridge-render incompatibility -> explicit warning or rejection in `AdoptableTargetRevision`

Hard rule:

- no candidate-generation failure may silently produce a misleading candidate

Replay must be able to reconstruct:

- which inputs were present
- which policies were used
- which candidates were included, excluded, or deferred
- which adoptable revision was rendered from which candidate revision

## 11. Testing Strategy

### 11.1 Discovery Contract Tests

Verify:

- continuous family discovery and deduplication
- repo-owned fact ingestion for discovery/backfill sources
- stable `FamilyDiscoveryRecord` revision and snapshot binding

### 11.2 Validation Contract Tests

Verify:

- `Included / Excluded / Deferred / InsufficientEvidence` verdicts
- structured reason-code stability
- metadata completeness and path-feasibility outcomes

### 11.3 Pricing Contract Tests

Verify:

- advisory pricing remains advisory
- units and normalization stay explicit
- policy-version changes create explainable candidate-revision changes

### 11.4 Bridge Contract Tests

Verify:

- stable rendering from `CandidateTargetSet` to `AdoptableTargetRevision`
- compatibility warnings are preserved
- rendered output maps cleanly onto the existing operator-target surface

### 11.5 Daemon Integration Tests

Verify:

- new discovery/backfill sources still use `ExternalFactEvent -> journal -> apply -> snapshot`
- `DiscoverySupervisor` consumes published snapshots, not mutable store state
- candidate generation does not mutate authoritative trading truth
- degraded runtime posture fail-closes candidate generation as appropriate

### 11.6 Replay And Drift Tests

Verify:

- same journal and policy versions reproduce the same `CandidateTargetSet`
- source, validation, pricing, and bridge drift are distinguishable
- adoption bridge revisions remain explainable and replayable

## 12. Acceptance Criteria

`Phase 3e` is complete when:

- the daemon continuously discovers family universe truth using repo-owned facts
- the system continuously produces conservative `CandidateTargetSet` artifacts on published snapshots
- candidates contain structured validation, path feasibility, advisory pricing, and advisory sizing
- candidate generation never directly promotes targets into live trading
- the system can render an `AdoptableTargetRevision` bridge artifact suitable for operator/control-plane adoption
- candidate generation is durable, replayable, and observable
- future ranking and budget-aware selection can layer on top without redesigning the `Phase 3e` objects

## 13. Follow-On Work

This phase intentionally leaves later work for follow-on plans:

- complex opportunity ranking
- budget-aware candidate competition
- hot-reloaded operator-target management
- direct control-plane adoption tooling
- future optional automation from adopted candidates into broader rollout workflows

