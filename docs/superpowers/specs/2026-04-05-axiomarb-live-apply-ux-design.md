# AxiomArb Live Apply UX Design

- Date: 2026-04-05
- Status: Draft for user review
- Project: `AxiomArb`

## 1. Summary

`app-live apply` currently exists as the Day 1+ high-level operator flow for `real-user shadow smoke`.

At current `HEAD`, the repository now also has the other ingredients that a higher-level normal-`live` flow needs:

- `status` is the config-and-control-plane readiness homepage
- `doctor` is the venue-facing preflight gate
- `run` owns runtime startup and durable `run_session` lifecycle truth
- `verify` is the post-run local outcome check

What is still missing is a high-level `live` orchestration path.

Today, a normal live operator still has to manually stitch together:

1. `status`
2. decide whether target adoption is still missing
3. decide whether live rollout posture is still missing
4. `doctor`
5. `run`
6. optional `verify`

The next UX subproject should extend the existing `app-live apply` command so it becomes a single high-level Day 1+ entry for both smoke and conservative live.

The recommended first phase is intentionally narrow:

- keep one `apply` command
- keep smoke semantics unchanged
- add a conservative `live` path
- only support live configs that are already adopted and already have rollout posture
- fail closed when live still needs target adoption or rollout work
- reuse `status`, `doctor`, and `run`
- allow `--start` to continue into foreground `run`
- keep `verify` separate in the first phase

This is not a new authority layer.

It is a scenario-aware orchestration extension on top of the current operator primitives.

## 2. Current Repository Reality

At current `HEAD`, the repository already has the core pieces needed for conservative live `apply`:

1. `apply` already exists as a high-level command.
- It already has scenario routing.
- It already owns high-level output sections:
  - `Current State`
  - `Planned Actions`
  - `Execution`
  - `Outcome`
  - `Next Actions`

2. `status` already computes the high-level readiness truth.
- It already distinguishes:
  - `target-adoption-required`
  - `smoke-rollout-required`
  - `live-rollout-required`
  - `restart-required`
  - `smoke-config-ready`
  - `live-config-ready`
  - `blocked`

3. `doctor` already owns preflight.
- `apply` does not need its own connectivity or credential checks.

4. `run` already owns runtime truth.
- `run` creates and updates durable `run_session`.
- `apply` does not need to invent a second runtime lifecycle model.

5. `verify` already owns post-run result checking.
- `apply` does not need to absorb that in the first live phase.

So the missing work is not new low-level capability.

The missing work is a high-level live path that consumes already-existing readiness and applies the next safe operator action without introducing new mutation authority.

## 3. Goals

This design should guarantee the following:

- extend the existing `app-live apply` command instead of adding a second high-level live command
- preserve the current smoke `apply` behavior
- add a conservative `live` apply path
- let live `apply` consume existing `status` readiness
- fail closed when live still requires target adoption
- fail closed when live still requires rollout posture
- reuse `doctor` as the only preflight gate
- stop at `ready to start` by default
- allow `--start` to continue into foreground `run`
- keep `restart-required` as an explicit manual boundary
- keep `verify` as a separate explicit command in the first phase
- keep `operator_target_revision` as the only startup authority
- keep rollout mutation authority outside live `apply`

## 4. Non-Goals

This design does not define:

- inline live target adoption
- inline live rollout enablement
- silent rollout mutation
- automatic verification chaining after live startup
- process supervision or process replacement
- daemon stop / replace management
- hot reload
- a second live-specific apply command
- any change to smoke `apply` semantics unless needed for shared routing cleanup

## 5. Architecture Decision

### 5.1 Recommended Approach

Extend the existing `app-live apply` command so it supports two first-class high-level Day 1+ scenarios:

- `real-user shadow smoke`
- conservative `live`

Hard rules:

- `apply` remains an orchestration layer
- `apply` does not become a second implementation of adoption, rollout, preflight, or runtime startup
- `status` remains the readiness truth source
- `doctor` remains the only preflight gate
- `run` remains the only runtime authority
- `verify` remains separate in the first live phase

### 5.2 Why One `apply` Command

The repo already has too many operator concepts in flight to justify another top-level “live-only” high-level command.

Using a single `apply` keeps the operator vocabulary cleaner:

- Day 0: `bootstrap`
- Day 1+: `status` then `apply`

Scenario differences should stay inside the command, not in the public command surface.

### 5.3 Why Conservative Live First

Smoke `apply` got the highest UX payoff from inline mutation because Day 1+ smoke progression is mostly controlled validation work.

Normal `live` is different:

- target adoption is a sharper control-plane decision
- rollout posture has higher operational sensitivity
- the current process-management story is still foreground-run only

So the right first step is not to make live `apply` more powerful.

It is to make live `apply` more coherent while keeping mutation boundaries explicit.

## 6. Public UX Model

### 6.1 Command Surface

The public command remains:

```bash
cargo run -p app-live -- apply --config config/axiom-arb.local.toml
cargo run -p app-live -- apply --config config/axiom-arb.local.toml --start
```

No new top-level command is added.

### 6.2 Scenario Routing

`apply` should route by scenario:

- `paper`
  - remain unsupported
  - point the operator back to `bootstrap` or `run`

- `real-user shadow smoke`
  - keep current smoke semantics

- `live`
  - use the new conservative live flow

### 6.3 Live Readiness Contract

The first live phase only supports configs that are already:

- adopted
- not blocked
- not legacy explicit-target high-level flow
- not missing rollout posture

That means:

- `target-adoption-required` => stop
- `live-rollout-required` => stop
- `blocked` => stop
- `live-config-ready` => continue to `doctor`
- `restart-required` => continue only through the explicit manual-boundary flow

## 7. Live Apply Flow

### 7.1 Default Flow

Default `live apply` should do:

1. load readiness from `status`
2. reject unsupported or incomplete live states
3. run `doctor`
4. stop at `ready to start`

It should not start runtime unless `--start` is present.

### 7.2 `--start`

If `--start` is present and preflight succeeds:

- `apply` may continue into foreground `run`

This does not mean process management exists.

It only means `apply` can continue into the same foreground `run` path that an operator could have invoked directly.

### 7.3 Restart Boundary

If `status` says `restart-required`, live `apply` should still respect a manual boundary:

- it may explain that the currently active runtime is not aligned with current config
- with `--start`, it may continue into a new foreground `run`
- it must not claim to stop or replace an already-running daemon

This is the same operational honesty that smoke `apply` already uses.

## 8. State Machine

### 8.1 Top-Level Scenario Split

`apply` should branch into:

- `PaperUnsupported`
- `SmokeApplyFlow`
- `LiveApplyFlow`

### 8.2 LiveApplyFlow

The live-specific internal stages should be:

- `LoadReadiness`
- `RejectIfAdoptionRequired`
- `RejectIfRolloutRequired`
- `ConfirmManualRestartBoundary`
- `RunPreflight`
- `Ready`
- `RunRuntime`

The live path must not include smoke-only stages:

- no `EnsureTargetAnchor`
- no `EnsureSmokeRollout`

Those stay smoke-only.

## 9. Output and Next Actions

`apply` should keep the existing high-level output frame:

- `Current State`
- `Planned Actions`
- `Execution`
- `Outcome`
- `Next Actions`

But live-specific content should be conservative.

### 9.1 Current State

For live, emphasize:

- scenario = `live`
- readiness
- configured vs active
- restart required
- relevant run session
- conflicting active run session

### 9.2 Planned Actions

For live first phase, valid planned actions are only:

- run doctor preflight
- optionally start foreground runtime

### 9.3 Outcome

At minimum, live should produce:

- `Blocked`
- `Ready to start`
- `Started`

### 9.4 Next Actions

Next actions should remain aligned with `status` vocabulary:

- adoption required => use `targets candidates` / `targets adopt`
- rollout required => finish live rollout preparation outside `apply`
- ready => rerun with `--start` if you want foreground run
- doctor failed => fix doctor-reported issue and rerun `apply`

`apply` must not invent a second operator language.

## 10. Authority Boundaries

The first live phase must keep these boundaries hard:

- `apply` does not mutate live target adoption
- `apply` does not mutate live rollout posture
- `doctor` remains the only preflight authority
- `run` remains the only runtime authority
- `verify` remains separate

This is the core reason the design is “conservative”.

## 11. Error Handling

Live `apply` should classify failures into the same broad families as current high-level flow handling:

- unsupported scenario
- readiness blocker
- preflight failure
- manual restart boundary refusal
- runtime startup failure

The output should always include a concrete next action.

## 12. Testing Strategy

### 12.1 Scenario Routing

Tests should prove:

- `paper` remains unsupported
- smoke behavior does not regress
- `live` no longer returns the generic unsupported path

### 12.2 Live Readiness Gates

Tests should cover:

- `target-adoption-required` => fail closed
- `live-rollout-required` => fail closed
- `blocked` => fail closed
- `live-config-ready` => reaches doctor
- `restart-required` + `--start` => respects manual boundary

### 12.3 Orchestration Boundaries

Tests should prove live `apply` does not:

- inline adopt
- write rollout posture
- chain verify
- bypass doctor

### 12.4 Output

Tests should verify the live path still renders:

- `Current State`
- `Planned Actions`
- `Execution`
- `Outcome`
- `Next Actions`

and that those outputs use the same operator vocabulary as `status`.

## 13. Acceptance Criteria

This design is complete when:

- `app-live apply` supports smoke and conservative live under a single command surface
- live `apply` no longer returns a generic unsupported-path error
- live `apply` fails closed when target adoption or rollout posture is still missing
- live `apply` reuses `doctor` and optionally `run`
- `--start` continues into foreground `run`
- `restart-required` remains an explicit manual boundary
- no live control-plane mutation authority is added
- no process-management authority is added
- smoke behavior remains intact

## 14. Recommendation

This should be the next UX feature after run-session lifecycle and Polymarket source defaults.

It is the highest-leverage remaining step because it turns the current high-level primitives into a more coherent operator path without expanding authority boundaries prematurely.
