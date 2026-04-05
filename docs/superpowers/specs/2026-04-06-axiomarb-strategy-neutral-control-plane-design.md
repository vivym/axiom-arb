# AxiomArb Strategy-Neutral Control Plane Design

- Date: 2026-04-06
- Status: Draft for user review
- Project: `AxiomArb`

## 1. Summary

At current `HEAD`, `AxiomArb` already contains two different strategy realities:

- `full-set`, which is modeled as a single default scope
- `neg-risk`, which is modeled as many family scopes

But the operator-facing control plane and most high-level `app-live` UX still treat `neg-risk` as the system's primary strategy. That coupling now blocks three things:

- explicit strategy selection at the entrypoint
- running `full-set`, `neg-risk`, or both from one `app-live` surface
- making candidate/adoption/startup/verify behavior coherent across routes

The recommended design is:

- keep one operator entrypoint: `app-live`
- make the control plane strategy-neutral
- make the control-plane key `route + scope`
- introduce a neutral revision anchor around that key
- push route-specific payloads and readiness logic into per-route adapters

This design does not try to flatten the strategies into one execution model.

Instead, it makes the control plane and command surface neutral while allowing each strategy to keep its own runtime and verification complexity.

## 2. Current Repository Reality

At current `HEAD`, the repository already has the right runtime seed for a strategy-neutral design:

- `ActivationPolicy` is already keyed by `route + scope`
- execution attempts already record `route`, `scope`, and `execution_mode`
- `full-set` already behaves as a single-scope route with scope `default`
- `neg-risk` already behaves as a multi-scope route with scope `family_id`

At the same time, the operator surfaces remain `neg-risk`-shaped:

- config schema is rooted in `[negrisk.*]`
- startup resolves `NegRiskLiveTargetSet`
- `discover` only materializes `neg-risk` candidate and adoptable artifacts
- `targets` adoption revolves around `operator_target_revision`
- `status` and `doctor` assume adopted targets are a `neg-risk` concern
- `verify` still hardcodes `neg-risk` as the route of interest
- a legacy explicit-target path still exists for `negrisk.targets`

The live execution substrate is also less neutral than the control-plane shape suggests:

- execution identity is already route-aware
- but live signing and submission are still family-shaped and `neg-risk`-specific

That means the repository already has a strategy-neutral execution identifier model, but not a strategy-neutral control plane.

## 3. Goals

This design should guarantee the following:

- `app-live` can operate against a strategy-neutral adopted revision
- the control plane manages a set of `StrategyKey { route, scope }`
- `full-set`, `neg-risk`, and `both` become first-class runtime combinations
- route-neutral candidate generation and discovery become possible
- `targets`, `run`, `status`, `apply`, `doctor`, and `verify` become route-neutral entrypoints
- route-specific rollout and readiness logic remains possible without leaking into the control-plane core
- startup and restore remain fail-closed
- revision lineage stays durable and explainable
- migration from the current `neg-risk`-centric model can happen incrementally

## 4. Non-Goals

This design does not define:

- new arbitrage pricing logic for `full-set` or `neg-risk`
- a new market discovery algorithm
- hot reloading of strategy revisions into a running daemon
- a separate control-plane service or binary
- per-market `full-set` scoping in v1
- unifying the two strategies into one planner or one submission shape
- removing compatibility with existing `neg-risk` data on day one

## 5. Approaches Considered

### 5.1 Option A: Global Singleton Control Plane

Treat the entire runtime as one global strategy selection surface.

Pros:

- smallest apparent UX change
- easiest to explain at the CLI level

Cons:

- not actually strategy-neutral
- still treats `full-set` and `neg-risk` as special cases
- does not align with the repository's existing `route + scope` abstractions
- makes future multi-scope strategies awkward

### 5.2 Option B: Fully Fine-Grained Per-Market Control Plane

Make `full-set` adoptable and rollable out per market or condition.

Pros:

- maximal flexibility
- theoretically uniform granularity across strategies

Cons:

- much larger project
- does not match current `full-set` architecture
- creates significant new operator and verification complexity
- forces a scope model the repository does not yet need

### 5.3 Option C: Route-Neutral Control Plane With Route-Owned Scope Semantics

Make the control plane manage `route + scope`, while each route defines what valid scopes mean.

For v1:

- `full-set` only allows scope `default`
- `neg-risk` uses `family_id`

Pros:

- matches current architecture
- keeps the control-plane kernel small
- allows both strategies to coexist without fake uniformity
- leaves room for future scope expansion without redesigning the core

Cons:

- requires a real control-plane refactor instead of a thin CLI switch
- preserves route complexity inside adapters

### 5.4 Recommendation

Choose Option C.

This is the only option that is actually strategy-neutral without overbuilding the `full-set` side of the system.

## 6. Core Object Model

### 6.1 Strategy Key

Introduce a neutral control-plane identity:

- `StrategyKey { route, scope }`

Rules:

- `route` identifies the strategy route, such as `full-set` or `neg-risk`
- `scope` identifies the route-owned execution unit
- `scope` validation is owned by the route adapter

V1 scope rules:

- `full-set` only accepts `scope = "default"`
- `neg-risk` accepts `scope = <family_id>`

During migration, `default` remains a reserved kernel fallback scope for rollout resolution on any route.

That means:

- `full-set/default` is both the execution scope and the rollout scope
- `neg-risk/<family_id>` remains the execution scope
- `neg-risk/default` may continue to exist as a coarse route-level rollout fallback until per-family rollout is fully neutralized

### 6.2 Neutral Control-Plane Artifacts

The control-plane kernel should reason about these neutral artifacts:

- `StrategyCandidate`
- `StrategyCandidateSet`
- `AdoptableStrategyRevision`
- `OperatorStrategyRevision`
- `ActiveStrategyRevision`

The control plane only needs common fields:

- key
- validation status
- provenance
- route-owned rendered payload

The control plane must not directly understand:

- `neg-risk` family members
- `full-set` pricing legs
- route-specific planner details

### 6.3 Route-Owned Payloads

Each route contributes route artifacts into a revision.

Examples:

- `full-set` contributes a single `full-set/default` artifact
- `neg-risk` contributes one or more `neg-risk/<family_id>` artifacts

The adopted revision becomes a neutral envelope containing one or more route artifacts.

### 6.4 Route-Local Candidate Production and Global Bundling

Candidate generation should happen in two layers:

1. route-local production
- each route generates its own candidate artifacts and route payloads
- unchanged route payloads should be stable and reusable across refreshes
- route artifacts must be derived only from stable control-plane inputs such as authoritative discovery, static route config, and route policy version
- route artifacts must not encode transient readiness facts such as connectivity, approval state, credential health, heartbeat freshness, or venue reachability

2. neutral bundling
- the control-plane kernel assembles those route artifacts into one `StrategyCandidateSet`
- adopted revisions remain globally authoritative
- unchanged route artifacts may be carried forward verbatim into a new global revision

This keeps a single global authority while avoiding unnecessary recomputation or rediscovery of unaffected routes.

Readiness remains a separate concern.

Route readiness facts may block:

- adoption
- apply
- doctor
- startup
- live activation

But they must not mint a different candidate payload or a different adopted revision by themselves.

### 6.5 Deterministic Global Revision Identity

The neutral bundler must produce deterministic global revision identities.

Required rules:

- route artifacts are canonically ordered by `(route, scope)`
- each route artifact exposes a stable semantic content digest over its canonicalized semantic payload and route policy version
- the global candidate or adopted revision identity is derived from:
  - the ordered set of `StrategyKey`
  - the ordered set of route artifact digests
  - the bundler version
  - any explicit global control-plane policy version that affects bundling semantics

The semantic content digest must exclude non-semantic metadata such as:

- snapshot ids
- source session ids
- timestamps
- candidate or adoptable revision ids
- rendered provenance-only lineage fields
- other fields whose only purpose is observability, replay context, or operator explainability

If such metadata changes while the canonical semantic payload does not, the route artifact digest must remain unchanged.

A new global revision must be emitted only when at least one of these changes:

- a `StrategyKey` is added or removed
- a route artifact digest changes
- the bundler version changes
- a global bundling policy version changes

Pure readiness drift must not create a new global revision.

When one route changes and another does not:

- the unchanged route artifact should be carried forward byte-for-byte
- the new global revision should still be deterministic from the full ordered bundle
- restart boundaries should follow actual artifact changes, not incidental rerenders

## 7. Architecture

### 7.1 Strategy-Neutral Kernel

The kernel owns:

- revision lineage
- configured vs active revision state
- startup anchor resolution
- activation overlays
- run-session lifecycle anchoring
- route aggregation for status/apply/verify

The kernel must not own strategy-specific decoding or rollout policy.

### 7.2 Route Adapters

Each route implements five adapter surfaces.

1. Control-plane adapter
- produces candidates from route-specific discovery or static config
- validates route-specific scopes
- renders route artifacts for adopted revisions

2. Runtime adapter
- decodes route artifacts from the adopted revision
- registers runtime task groups, intents, planners, and sinks

3. Execution adapter
- owns route-specific live signing, submission, and reconcile integration
- hides strategy-shaped live submit APIs from the control-plane kernel

4. Readiness adapter
- evaluates route-specific rollout and readiness gates
- reports route-level readiness into `status`, `doctor`, and `apply`

5. Verify adapter
- loads route-specific evidence
- produces a route-level verification verdict

This keeps `app-live` responsible for orchestration while route modules remain responsible for domain logic.

### 7.3 Activation and Rollout Ownership

Activation remains kernel-owned.

The neutral kernel continues to own:

- `ActivationPolicy`
- exact `route + scope` mode resolution
- the existing route-level `default` fallback rule semantics

Route adapters own:

- which scopes are meaningful for the route
- which readiness facts gate execution
- how route-specific config becomes rollout input

This means the migration does not require removing the kernel's current fallback behavior on day one.

For v1:

- `full-set/default` remains the normal activation key
- `neg-risk/default` may remain as a coarse route-level fallback rule
- `neg-risk/<family_id>` remains the route's concrete execution scope

### 7.4 Candidate Production, Discover, and Bundling

`discover` should become a route-neutral orchestration command even if route implementations differ.

Expected behavior:

- `neg-risk` continues to have real discovery and candidate generation
- `full-set` may initially contribute deterministic route-local artifacts from static config and route policy state rather than external discovery
- the bundler assembles route-local outputs into one candidate set and one adoptable revision lineage

The bundler should preserve route-local artifact identity.

That means a `neg-risk` refresh may produce a new global candidate or adopted revision, but it should not force `full-set` to rediscover or rerender if its route artifact is unchanged.

### 7.5 Why This Boundary Matters

Without adapters, the neutral control plane would immediately re-absorb `neg-risk`-specific shapes such as:

- family rollout lists
- rendered family target sets
- route-specific startup decoding
- route-specific evidence windows
- route-specific live submit APIs

That would recreate the current problem under different names.

## 8. Command Semantics

### 8.1 `app-live discover`

`discover` becomes the operator-facing candidate refresh command for the neutral control plane.

It should:

- run route-local candidate producers
- collect route-local candidate artifacts
- bundle them into a neutral candidate set and adoptable lineage
- surface route-local diffs so the operator can see which route actually changed

`discover` should not require every route to support the same discovery mechanism.

`discover` must not treat transient readiness as candidate content.

It may report readiness-adjacent warnings separately, but those warnings must not change route artifact identity or bundle identity by themselves.

### 8.2 `app-live targets`

`targets` becomes a pure control-plane namespace.

It should manage:

- candidate sets
- adoptable revisions
- current configured revision
- rollback history

`targets adopt` and `targets rollback` should write a neutral `operator_strategy_revision`, not a `neg-risk`-specific anchor.

Default operator output should explain:

- which `StrategyKey` entries are in the revision
- which route artifacts were adopted
- whether restart is required

Legacy explicit compatibility rules:

- `targets adopt` is the explicit migration path from legacy explicit config into the neutral adopted-revision model
- in compatibility mode, `targets adopt` must synthesize the first deterministic neutral revision from the current legacy explicit config when no neutral adopted revision history exists yet
- that synthetic migration revision must be persisted through the same neutral lineage model as any other adopted revision, with explicit provenance marking it as compatibility migration input
- after creating that first neutral revision, `targets adopt` must rewrite config from the legacy explicit shape to the neutral control-plane anchor
- `targets rollback` is unavailable until at least one neutral adopted revision exists in durable history
- `targets show-current` and `targets status` should render compatibility state explicitly rather than pretending a neutral adopted revision already exists

### 8.3 `app-live run`

`run` should resolve a neutral startup plan from the configured revision, then dispatch route artifacts to route runtime adapters.

`run` should no longer directly assume that startup resolution yields only `NegRiskLiveTargetSet`.

The runtime may start:

- `full-set` only
- `neg-risk` only
- both

`real_user_shadow_smoke` becomes route-neutral.

For v1, smoke mode must clamp all risk-expanding strategy routes to `Shadow`.

That means:

- no live `full-set` execution in smoke mode
- no live `neg-risk` execution in smoke mode
- `both` in smoke mode means shadow-only strategy execution across both routes

Future exceptions for route-specific smoke-safe live behavior require a separate design and must not be assumed here.

### 8.4 `app-live status`

`status` should answer:

- which revision is configured
- which revision is active
- whether restart is required
- which routes and scopes are included
- which route-level readiness gates are blocking execution

It should report a revision-level summary plus route-level readiness details.

Legacy explicit-target startup should remain visible during migration.

`status` should surface it as compatibility mode rather than as a first-class neutral control-plane mode, and should guide operators toward adopted revisions.

### 8.5 `app-live apply`

`apply` should become a route-neutral operator helper instead of a smoke-specific helper.

Its job becomes:

- read current readiness
- ensure an adopted revision exists
- ensure route-specific rollout and preflight gates are satisfied
- optionally start the runtime

It should operate on the configured revision and route readiness, not on hardcoded `paper/smoke/live` assumptions alone.

Compatibility rule:

- `apply` must not silently auto-migrate legacy explicit config into the neutral adopted-revision model
- if `apply` requires control-plane mutation while the operator is still on legacy explicit config, it should stop with explicit migration guidance
- `apply` does not support compatibility-mode runtime start
- operators in compatibility mode must either use the read-only compatibility path (`status` / `doctor` / `verify` / `run`) or explicitly migrate via `targets adopt`

### 8.6 `app-live doctor`

`doctor` should report:

- global checks
- route-specific checks

Global checks remain:

- config
- credentials
- connectivity
- runtime safety

Route checks become adapter-owned:

- `full-set` readiness
- `neg-risk` rollout and artifact readiness

Legacy explicit-target configs should be reported as compatibility input, not silently treated as a permanent equal alternative to adopted revisions.

### 8.7 `app-live verify`

`verify` should become revision-aware and route-aware.

It must no longer hardcode `neg-risk` as the route of interest.

The flow becomes:

- resolve the relevant run session and control-plane anchor
- determine which routes were active in that revision
- ask each route verify adapter to load route evidence and produce a route verdict
- aggregate route verdicts into one session-level verify report

## 9. Persistence Design

### 9.1 What Is Already Neutral Enough To Reuse

The identity side of execution is already close enough to neutral:

- execution attempts already store `route`
- execution attempts already store `scope`
- activation already works on `route + scope`

That should be preserved.

### 9.2 What Is Not Neutral Yet

The live execution submit path is still route-shaped around `neg-risk`.

Today:

- the live sink only performs real submit work for the family submit plan shape
- signer and submit provider interfaces are explicitly family-oriented
- `full-set` plans exist, but are not wired into a real route-neutral live submit path

This must be treated as a first-class refactor workstream, not as incidental cleanup.

The desired end state is:

- route-neutral orchestration at the kernel
- route-owned execution adapters below it
- no family-shaped live submit API exposed as the only general live execution path

### 9.3 New Neutral Lineage Tables

The current control-plane lineage naming is target-specific and semantically biased toward `neg-risk`.

Introduce new neutral lineage tables and repos:

- `strategy_candidate_sets`
- `adoptable_strategy_revisions`
- `strategy_adoption_provenance`
- `operator_strategy_adoption_history`

Promote runtime anchors accordingly:

- `runtime_progress.operator_strategy_revision`
- `run_sessions.configured_operator_strategy_revision`
- `run_sessions.active_operator_strategy_revision_at_start`

### 9.4 Migration Strategy

Do not perform a big-bang rename.

Migration should proceed in four stages:

1. Add neutral tables and columns
- backfill from existing `target`-named data
- keep read compatibility with old data

2. Add route-neutral execution adapter seams
- remove family-shaped assumptions from the only live submit path
- preserve route-aware execution identity and attempt storage

3. Move high-level commands to neutral repos
- `discover`
- `targets`
- `status`
- `run`
- `apply`
- `doctor`
- `verify`

4. Retire old `target`-specific control-plane APIs
- keep compatibility readers for a limited migration window
- remove old names after the operator path is fully neutral

Avoid long-lived dual-write behavior. Backfill plus controlled cutover is preferred.

## 10. Config Schema Design

### 10.1 Target State

The current config schema is the strongest `neg-risk` coupling point because the operator anchor is stored under `[negrisk.target_source]`.

The target state should move to a neutral top-level control-plane section.

Representative shape:

```toml
[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-12"
```

### 10.2 Route-Specific Sections

Route-specific config should move under route-owned sections.

Representative shape:

```toml
[strategies.full_set]
enabled = true

[strategies.neg_risk]
enabled = true

[strategies.neg_risk.rollout]
approved_scopes = ["family-a", "family-b"]
ready_scopes = ["family-a"]
```

Exact naming may vary, but the split should be:

- neutral control-plane anchor at the top
- route-specific operational config under route-owned sections

### 10.3 Legacy Explicit Targets

Legacy explicit target config is a compatibility path, not the long-term neutral control-plane model.

During migration:

- legacy explicit config may still be read
- startup may still materialize a compatibility startup bundle from it
- `status`, `doctor`, and `verify` should label that mode as compatibility or legacy-explicit
- `targets adopt` is the required explicit migration action out of compatibility mode when the operator wants to enter the neutral adopted-revision model
- if no neutral adopted revision history exists yet, `targets adopt` must synthesize the first neutral revision deterministically from the legacy explicit config and persist normal neutral lineage for it
- `targets rollback` is unavailable until neutral adoption history exists
- `apply` must not auto-migrate this mode silently

The design does not promote explicit static targets into a new permanent, strategy-neutral operator mode.

Instead, the operator-facing neutral model remains:

- route-local candidate production
- adopted neutral revisions
- revision-anchored startup and verify

### 10.4 Compatibility

During migration, the loader may continue reading legacy `negrisk.target_source` and legacy `negrisk.rollout`.

It may also continue reading legacy explicit `negrisk.targets` for compatibility startup.

But write paths for new operator actions should switch to the neutral schema once available.

## 11. Startup, Failure, and Isolation Rules

### 11.1 Startup

Startup remains fail-closed.

If any route artifact in the configured revision:

- cannot be decoded
- fails provenance checks
- fails required readiness gates
- is missing required route-owned state

then the entire revision fails startup.

The runtime must not partially activate a configured revision.

### 11.2 Real-User Shadow Smoke

`real_user_shadow_smoke` is revision-aware but route-neutral.

For this design:

- all risk-expanding strategy routes are clamped to `Shadow`
- `verify` for smoke mode continues to expect no credible live strategy attempts
- if a route cannot support shadow execution under smoke, startup should fail rather than silently run it live

### 11.3 Runtime Isolation

Once startup succeeds, route failures should be isolated where possible.

Examples:

- a `neg-risk` family rollout or reconcile fault should not automatically disable `full-set/default`
- a route-local verification or planning error should remain route-local when safe

Shared infrastructure failures remain global runtime failures:

- signer failures
- DB failures
- shared source failures
- journal corruption or restore failure

### 11.4 Verification Aggregation

Each active route produces a route-level verdict.

Session-level aggregation rule:

- any route `Fail` => session `Fail`
- otherwise any route warning => session `PassWithWarnings`
- otherwise => session `Pass`

## 12. Testing Strategy

Testing should be split into four layers.

### 12.1 Schema and Migration Tests

Cover:

- neutral lineage tables and columns
- backfill from legacy target-named lineage
- compatibility reads during migration
- deterministic revision identity and carry-forward behavior
- semantic digests remaining stable under provenance-only metadata changes

### 12.2 Adapter Unit Tests

Cover:

- `full-set` adapter with `default` scope
- `neg-risk` adapter with family scope
- execution adapter behavior for both routes
- artifact rendering and decoding
- separation between candidate content and transient readiness facts
- readiness logic
- route-specific verify evidence loading

### 12.3 Command Integration Tests

Every high-level command should cover:

- `discover`
- `full-set` only
- `neg-risk` only
- `both`
- legacy explicit compatibility mode
- deterministic no-op rediscovery where unchanged route artifacts do not create a new revision

Priority commands:

- `discover`
- `targets`
- `status`
- `run`
- `apply`
- `doctor`
- `verify`

### 12.4 Compatibility Tests

Cover:

- legacy config shapes still load
- legacy adoption lineage still resolves
- historical run-session and verify windows remain comparable before and after migration
- `targets adopt` creating the first synthetic neutral revision from legacy explicit config

## 13. Implementation Order

The recommended implementation order is:

1. Introduce neutral control-plane domain types and persistence lineage
2. Add neutral config schema and compatibility reads
3. Build route adapter interfaces and define activation ownership boundaries
4. Refactor the live execution submit path into route-neutral orchestration plus route-owned execution adapters
5. Convert `discover` to route-local candidate production plus neutral bundling
6. Convert startup and `run` to neutral startup resolution and route-neutral smoke semantics
7. Convert `targets` to neutral revision operations
8. Convert `status`, `doctor`, and `apply` to route-neutral readiness reporting with explicit compatibility handling
9. Convert `verify` to multi-route evidence aggregation
10. Remove old target-specific operator paths after compatibility coverage is in place

This keeps the highest-risk refactor points early:

- lineage
- live execution substrate
- startup
- control-plane anchor

and pushes cleanup work after the new path is already operational.

## 14. Decision

The repository should move to a strategy-neutral control plane built around `route + scope` and a neutral adopted revision anchor.

`full-set` and `neg-risk` should remain distinct route implementations, but they should no longer force the control plane, config schema, or high-level `app-live` commands to be `neg-risk`-shaped.

This still assumes one globally authoritative adopted revision.

That global revision should be assembled from route-local candidate artifacts through a neutral bundling step, with unchanged route artifacts carried forward when possible.

This is the smallest design that:

- makes the operator surface honest
- keeps startup and recovery rigorous
- allows `full-set`, `neg-risk`, and `both`
- does not overbuild `full-set` scope granularity
