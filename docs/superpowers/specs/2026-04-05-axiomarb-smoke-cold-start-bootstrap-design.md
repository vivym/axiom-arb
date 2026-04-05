# AxiomArb Smoke Cold-Start Bootstrap Design

- Date: 2026-04-05
- Status: Draft for user review
- Project: `AxiomArb`

## 1. Summary

At current `HEAD`, `real-user shadow smoke` cold start deadlocks on a fresh database:

- `bootstrap` is intentionally only an orchestration layer
- `status` and `apply` require an adopted target anchor before they can continue
- candidate generation can persist advisory candidate artifacts
- but adoptable artifacts are not materialized unless an `operator_target_revision` is already present
- startup and restore still correctly treat `operator_target_revision` as the only authority

That creates an invalid sequence:

`need adoptable revision -> need operator_target_revision -> need adoptable revision`

The recommended fix is:

- keep `operator_target_revision` as the only startup authority
- make pre-adoption discovery/materialization a first-class phase
- allow `AdoptableTargetRevision` to be generated before adoption
- keep `targets adopt` as the only command that writes `operator_target_revision` into config
- make `bootstrap` the public Day 0 happy path by orchestrating:
  - config
  - discover
  - adoptable review and explicit confirmation
  - adopt
  - doctor
  - optional startup

In short:

- product surface: `bootstrap` should be the one obvious Day 0 operator entrypoint
- implementation surface: a new low-level `discover` stage must exist under it

## 2. Current Repository Reality

At current `HEAD`, the repository has most of the required pieces, but in the wrong order for cold start:

1. `bootstrap` is designed as orchestration, not a second startup authority
- it should sequence existing workflows, not invent a parallel control plane

2. candidate and adoptable artifacts already exist as durable concepts
- `Phase 3e` already defines:
  - `CandidateTargetSet`
  - `AdoptableTargetRevision`
  - provenance from `operator_target_revision -> adoptable_revision -> candidate_revision`

3. startup already correctly trusts only `operator_target_revision`
- startup/restore must remain anchored on the adopted target revision

4. the current cold-start flow is broken
- `bootstrap` stops and redirects to `targets candidates` / `targets adopt`
- on a fresh database, `targets candidates` is empty
- `status` collapses multiple distinct states into `target-adoption-required`
- `apply` reports `ensure-target-anchor`

5. the root cause is architectural, not merely UX
- discovery can produce candidate artifacts without an adopted target
- but adoptable artifact generation is currently gated on an existing `operator_target_revision`
- that is backwards for Day 0 cold start

## 3. Goals

This design should guarantee the following:

- a fresh database plus valid config can complete a truthful Day 0 smoke cold-start path
- `bootstrap` becomes the public happy path for Day 0 smoke cold start
- `bootstrap` remains an orchestration layer, not a second authority
- `operator_target_revision` remains the only startup and restore authority
- pre-adoption discovery/materialization becomes a first-class lifecycle phase
- `AdoptableTargetRevision` can be generated before adoption
- multiple adoptable revisions may be shown with a recommendation, but operator confirmation remains mandatory
- if discovery yields no adoptable revisions, the flow stops at a truthful non-error readiness boundary with reasons and next actions
- `status`, `apply`, and `bootstrap` use the same state model and do not collapse distinct operator situations into one vague error

## 4. Non-Goals

This design does not define:

- automatic adoption of the latest or best target revision
- automatic runtime startup by default after preflight
- a second durable startup authority besides `operator_target_revision`
- moving candidate generation into `doctor`
- allowing discovery to mutate runtime authority directly
- hot reloading adopted targets into a running daemon
- productizing legacy explicit `[[negrisk.targets]]` as the preferred Day 0 path

## 5. Architecture Decision

### 5.1 Recommended Approach

Expose `bootstrap` as the product-level Day 0 entrypoint, but implement the cold-start closure by introducing a distinct low-level discovery/materialization phase.

The architecture becomes:

- `bootstrap`
  - public Day 0 orchestration layer
- `discover`
  - low-level candidate/adoptable materialization command
- `targets adopt`
  - the only config authority write path
- `doctor`
  - the only preflight gate
- `run`
  - the only runtime entrypoint

Hard rules:

- `bootstrap` must not invent a second startup authority
- discovery must not write `operator_target_revision`
- discovery must not write config
- adoption must remain explicit
- runtime startup must still require an adopted target revision

### 5.2 Why This Is Better Than A Bootstrap-Only Fix

Putting discovery logic directly inside `bootstrap` would appear simpler, but it would blur component boundaries:

- the same discovery/materialization logic would later need reuse from lower-level flows
- `bootstrap` would quietly become a special case implementation surface
- operator tooling would still lack a first-class low-level way to materialize adoptable revisions

The correct split is:

- public UX chooses option `1`: one obvious high-level command
- internal architecture uses option `2`: a first-class discovery/materialization phase that `bootstrap` orchestrates

### 5.3 Why This Is Better Than Falling Back To Explicit Targets

Using explicit `[[negrisk.targets]]` as the Day 0 happy path would keep two competing lifecycle models alive:

- adopted target source
- hand-authored explicit target source

That would preserve the current operator confusion instead of resolving it.

The long-term correct flow remains:

`discover -> adoptable -> adopt -> doctor -> run`

## 6. Identity Model

### 6.1 CandidateTargetSet

`CandidateTargetSet` remains:

- advisory output
- durable and replayable
- non-authoritative for startup

It is explainability and selection context only.

### 6.2 AdoptableTargetRevision

`AdoptableTargetRevision` becomes:

- adoption-ready output of the bridge phase
- durable before adoption
- self-contained enough to tell the operator which future `operator_target_revision` it would activate

It is still not active authority.

### 6.3 operator_target_revision

`operator_target_revision` remains:

- the only startup and restore authority
- written only by explicit adoption or rollback flows
- stored only in `[negrisk.target_source].operator_target_revision`

### 6.4 CandidateAdoptionProvenance

`CandidateAdoptionProvenance` should be written only when adoption occurs.

Discovery/materialization must not pre-write provenance rows because that would blur:

- "this is adoptable"
- "this was adopted"

Those are distinct lifecycle facts.

## 7. Rendering The Future operator_target_revision

The current implementation incorrectly requires an already existing `operator_target_revision` before it can render an adoptable artifact.

The corrected contract should be:

- the bridge itself renders the future `operator_target_revision`
- the future revision is deterministic
- adoption later writes that exact rendered value into config

### 7.1 Determinism Rule

The bridge should deterministically derive `rendered_operator_target_revision` from a canonical bridge input, rather than receiving it from prior adopted state.

The recommended canonical input is:

- canonical `rendered_live_targets`
- `candidate_revision`
- `adoptable_revision`
- `bridge_policy_version`

This preserves two important properties:

1. identical bridge artifacts render the same future revision
2. distinct adoptable artifacts do not accidentally collapse onto one provenance key simply because their final targets happen to match

That second property matters because the durable provenance chain is keyed by `operator_target_revision`.

### 7.2 Recommended Implementation Shape

Add a small helper that canonicalizes the bridge render input and returns:

- `rendered_operator_target_revision`
- optionally a separate `rendered_live_targets_hash` for observability/debuggability

`neg_risk_live_target_revision_from_targets(...)` should remain available as the canonical target-content hash, but it should not be reused as the sole identity of the adoptable bridge artifact unless the provenance model is also redesigned.

## 8. Cold-Start State Machine

The high-level smoke state model should become:

1. `config-ready`
- valid local config exists
- required credentials and source config exist
- no candidate/adoptable artifacts necessarily exist

2. `discovery-required`
- no candidate/adoptable artifacts exist yet
- next action is discovery/materialization

3. `discovery-ready-not-adoptable`
- discovery artifacts exist
- no adoptable revision exists
- reasons must be shown truthfully

4. `adoptable-ready`
- one or more adoptable revisions exist
- config still has no adopted `operator_target_revision`

5. `target-adopted`
- config now carries `operator_target_revision`
- adoption provenance exists

6. `smoke-rollout-required`
- adopted target exists
- rollout does not yet cover adopted families

7. `smoke-config-ready`
- adopted target and rollout are aligned
- ready for preflight

8. `restart-required`
- configured revision differs from active revision

9. `running`
- runtime has started for the selected session

This removes the current deadlock because `adoptable-ready` no longer depends on `target-adopted`.

## 9. Command Surface

### 9.1 Public Happy Path

The recommended public Day 0 path remains:

```bash
cargo run -p app-live -- bootstrap --config config/axiom-arb.local.toml
```

On a fresh database, `bootstrap` should orchestrate:

1. config creation or reuse
2. discovery/materialization
3. adoptable review
4. explicit adoption confirmation
5. adoption
6. doctor
7. optional rollout confirmation
8. optional runtime start

### 9.2 New Low-Level Command

Add:

```bash
cargo run -p app-live -- discover --config config/axiom-arb.local.toml
```

First-version behavior:

- one-shot materialization
- no config writes
- no adoption
- no runtime start
- operator-facing summary of:
  - candidate count
  - adoptable count
  - deferred/excluded summary
  - recommended adoptable revision where applicable

### 9.3 Existing Commands

`targets candidates`
- remains read-only
- shows advisory, adoptable, and adopted views
- should include stronger disposition summaries when adoptable output is absent

`targets adopt`
- remains the only authority write path

`doctor`
- remains preflight-only

`apply`
- remains the preferred Day 1+ smoke orchestration surface
- but it must understand the new discovery-related readiness states

## 10. Operator UX

### 10.1 Day 0 Empty Database

Desired happy path:

`bootstrap -> discover -> show adoptable + recommendation -> wait for explicit confirmation -> adopt -> doctor -> rollout confirmation -> ready`

### 10.2 Multiple Adoptable Revisions

When discovery yields multiple adoptable revisions:

- list all of them
- mark one as recommended
- require the operator to manually choose/confirm

The system must not auto-adopt.

### 10.3 No Adoptable Revisions

When discovery yields no adoptable revisions:

- stop at `discovery-ready-not-adoptable`
- do not pretend adoption is the next valid step
- show why the candidate set is not adoptable
- show concrete next actions

### 10.4 Confirmation Boundary

For Day 0 empty-database bootstrap:

- generating adoptable revisions is not itself authority mutation
- adoption is authority mutation

So the default safe stopping point is:

- after adoptable generation
- before adoption

The operator explicitly requested this behavior in the design review.

## 11. status/apply/bootstrap Output Model

### 11.1 status

`status` must distinguish:

- `discovery-required`
- `discovery-ready-not-adoptable`
- `adoptable-ready`
- `target-adoption-required` should no longer be the catch-all for all of the above

It should also surface precise next actions:

- run `discover`
- inspect discovery reasons
- choose and adopt an adoptable revision

### 11.2 bootstrap

For empty-database smoke cold start, `bootstrap` should not emit the current half-implemented redirection text.

Instead it should emit truthful summaries such as:

- `Discovery completed`
- `Adoptable revisions: ...`
- `Recommended: ...`
- `Waiting for explicit adoption confirmation`

Or:

- `Discovery completed but no adoptable revisions were produced`
- `Reasons: ...`
- `Next: rerun discover after discovery readiness improves`

### 11.3 apply

`apply` should not reduce all pre-adoption states to `ensure-target-anchor`.

Instead:

- `discovery-required` should trigger or request discovery
- `discovery-ready-not-adoptable` should stop with reasons
- `adoptable-ready` should permit inline adoptable selection

### 11.4 targets candidates

When adoptable output is absent, `targets candidates` should show more than:

- `adoptable = none`

It should ideally include:

- candidate/adoptable/deferred/excluded counts
- recommended revision where applicable
- reason summaries for non-adoptable output

## 12. Error Semantics

Separate user-visible outcomes into three classes:

1. `operator-action-required`
- choose an adoptable revision
- confirm adoption
- confirm rollout
- confirm restart boundary

2. `discovery-not-ready`
- no adoptable revision exists yet
- candidate generation deferred or excluded
- output is truthful but not an exceptional system failure

3. `system-failure`
- database errors
- schema errors
- malformed payloads
- persistence corruption

Only the third category should be rendered as true execution failure by default.

The first two should often be expressed as readiness/result states.

## 13. Implementation Plan

### 13.1 Phase 1: Fix Identity And Bridge Contracts

- decouple adoptable rendering from existing adopted state
- make the bridge deterministically render future `operator_target_revision`
- prevent discovery from writing adoption provenance

### 13.2 Phase 2: Add `discover`

- add CLI shape
- add one-shot runtime path for candidate/adoptable materialization
- add operator summary output

### 13.3 Phase 3: Update State Evaluation

- extend `status` readiness states
- extend `apply` transition handling
- update next-action wording

### 13.4 Phase 4: Rework `bootstrap`

- empty-database smoke path should inline discover
- stop at adoptable confirmation boundary by default
- continue into adopt / doctor / rollout only after explicit confirmation

## 14. Verification Strategy

The implementation must add coverage for:

1. Empty database + discovery yields adoptable output
- `bootstrap` reaches adoptable review state
- operator can continue to adoption

2. Empty database + discovery yields only advisory candidate output
- `bootstrap` stops at `discovery-ready-not-adoptable`
- reasons and next actions are truthful

3. Existing adoptable output but no adopted target
- `status`, `apply`, and `bootstrap` all agree on next action

4. Already adopted target
- existing doctor/apply/run/verify path remains compatible

Additional invariants:

- identical bridge input renders identical future `operator_target_revision`
- discovery never writes config
- discovery never writes adoption provenance
- adoption remains the only path that mutates `operator_target_revision`

## 15. Final Recommendation

The correct fix is not a small bootstrap workaround.

The correct fix is to close the lifecycle boundary properly:

- make discovery/materialization an explicit pre-adoption phase
- make adoptable rendering independent of already adopted authority
- keep `operator_target_revision` as the sole startup authority
- let `bootstrap` orchestrate the full Day 0 happy path over those primitives

That preserves the existing architecture while finally making fresh-database smoke cold start truthful, complete, and operator-friendly.
