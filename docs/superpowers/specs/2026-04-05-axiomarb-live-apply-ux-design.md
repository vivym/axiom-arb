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

Important combined-state rule:

- `restart-required` does not override missing rollout posture
- if `status` reports `restart-required` and `rollout_state = required`, live `apply` must stop before `doctor` and before `run`
- in that case the next action remains “finish live rollout preparation outside `apply`”, then return to `apply`

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
- with `--start`, it may continue into a new foreground `run` only after `doctor` succeeds and after explicit manual-boundary confirmation
- it must not claim to stop or replace an already-running daemon

This is the same operational honesty that smoke `apply` already uses.

The first live phase should distinguish two restart cases:

1. `restart-required` with no conflicting active running session
- interactive `--start` may ask for explicit confirmation and then continue into foreground `run`
- non-interactive `--start` must fail closed because the manual boundary cannot be confirmed

2. `restart-required` with a conflicting active running session
- `apply --start` must stop at the manual boundary
- it must not continue into foreground `run`
- it must instruct the operator to resolve the existing active runtime outside `apply`

This keeps `apply` out of process-management authority while still giving a high-level path for non-conflicting restart work.

The ordering matters:

- `restart-required` without `--start` still runs `doctor` first and then stops at `Ready to start`
- `restart-required` with `--start` runs `doctor` first and only then enters `ConfirmManualRestartBoundary`
- live should not ask for restart-boundary confirmation before preflight has passed

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
- `RejectIfBlocked`
- `RunPreflight`
- `Ready`
- `ConfirmManualRestartBoundary`
- `RunRuntime`

The live path must not include smoke-only stages:

- no `EnsureTargetAnchor`
- no `EnsureSmokeRollout`

Those stay smoke-only.

### 8.3 Readiness-to-Stage Mapping

The live path should map current `status` truth to stages as follows:

- `target-adoption-required`
  - stop in `RejectIfAdoptionRequired`
  - next action: `targets candidates` / `targets adopt`

- `live-rollout-required`
  - stop in `RejectIfRolloutRequired`
  - next action: finish live rollout preparation outside `apply`

- `restart-required` with `rollout_state = required`
  - stop in `RejectIfRolloutRequired`
  - next action: finish live rollout preparation outside `apply`, then return to `apply`

- `blocked` with legacy explicit-target details/action
  - stop in `RejectIfBlocked`
  - next action: migrate to adopted target source or fall back to lower-level commands

- generic `blocked`
  - stop in `RejectIfBlocked`
  - next action: follow the existing blocking guidance from `status`

- `restart-required` with rollout ready
  - continue into `RunPreflight`

- `live-config-ready`
  - continue directly into `RunPreflight`

- post-`RunPreflight`, if `--start` is not present
  - stop in `Ready`

- post-`RunPreflight`, if `--start` is present and `restart-required` was true
  - continue into `ConfirmManualRestartBoundary`

- post-`RunPreflight`, if `--start` is present and `restart-required` was false
  - continue directly into `RunRuntime`

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

For live first phase, valid planned actions are:

- stop because target adoption is still required
- stop because live rollout preparation is still required
- stop because legacy explicit targets must be migrated
- run doctor preflight
- explicitly confirm the manual restart boundary when applicable
- optionally start foreground runtime when and only when:
  - `--start` is present
  - `doctor` has passed
  - no conflicting active running session is blocking the manual boundary

### 9.3 Outcome

At minimum, live should produce:

- `Blocked`
- `Ready to start`
- `Started`

Outcome semantics should be explicit:

- adoption missing / rollout missing / legacy blocked / generic blocked => `Blocked`
- manual restart boundary declined after successful preflight => clean stop with `Ready to start`, not failure
- non-interactive `--start` when manual confirmation is required => fail-closed with `Blocked`
- successful preflight without `--start` => `Ready to start`
- successful foreground startup => `Started`

### 9.4 Next Actions

Next actions should remain aligned with `status` vocabulary:

- adoption required => use `targets candidates` / `targets adopt`
- rollout required => finish live rollout preparation outside `apply`
- legacy explicit targets => migrate to adopted target source or use lower-level commands
- ready => rerun with `--start` if you want foreground run
- conflicting active running session => resolve the existing runtime outside `apply`, then rerun `apply --start`
- doctor failed => fix doctor-reported issue and rerun `apply`

`apply` must not invent a second operator language.

### 9.5 `status` Ownership Changes

This live-apply feature is only coherent if `status` stops routing ready live operators back into the old multi-command path.

The first live phase should therefore move next-action ownership as follows:

- `StatusReadiness::LiveConfigReady`
  - preferred high-level next action becomes `app-live apply --config {config}`
  - not `app-live doctor --config {config}`

- `StatusReadiness::RestartRequired` with `rollout_state = ready`
  - preferred high-level next action becomes `app-live apply --config {config}`
  - not a generic `perform controlled restart`

- `StatusReadiness::TargetAdoptionRequired`
  - stays on `targets candidates` / `targets adopt`

- `StatusReadiness::LiveRolloutRequired`
  - stays on explicit rollout preparation outside `apply`

- `StatusReadiness::Blocked` with legacy explicit targets
  - stays on migration/lower-level guidance

This preserves a single high-level Day 1+ entry for live while keeping adoption and live rollout posture outside `apply`.

Implementation contract:

- the implementation must make live-ready `status` outputs route to `app-live apply --config {config}` through one explicit mechanism
- acceptable first-phase mechanisms are:
  - add a dedicated `RunApply`/equivalent status action for high-level live/smoke progression, or
  - keep the existing action enum but make `status` evaluation/rendering details-aware enough to rewrite the live-ready and restart-ready next actions to `apply`
- the implementation must not rely on ambiguous prose-only rewrites in docs while leaving `status` action rendering unchanged
- whatever mechanism is chosen, it must preserve the existing low-level actions for:
  - `TargetAdoptionRequired`
  - `LiveRolloutRequired`
  - legacy explicit-target migration

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
- manual restart boundary stop
- runtime startup failure

The output should always include a concrete next action.

Manual restart-boundary semantics should be mode-aligned with current smoke behavior:

- operator decline after an interactive confirmation prompt is a clean stop, not a runtime failure
- non-interactive `--start` when confirmation is required is fail-closed
- conflicting active running sessions are treated as boundary stops, not as situations `apply` can manage away

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
- `restart-required` + `rollout_state = required` => stop before doctor/run
- `blocked` => fail closed
- `blocked` + legacy explicit targets => migration-specific next action
- `live-config-ready` => reaches doctor
- `restart-required` + interactive `--start` + no conflicting active running session => respects manual boundary and may continue
- `restart-required` + interactive decline => clean stop
- `restart-required` + non-interactive `--start` => fail closed
- conflicting active running session + `--start` => stop at the boundary and do not enter run

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

### 12.5 Status Integration

Tests should verify the live-ready status path now points into `apply`:

- `live-config-ready` => `app-live apply --config {config}`
- `restart-required` with rollout ready => `app-live apply --config {config}`
- `live-rollout-required` => still points to manual rollout preparation
- `target-adoption-required` => still points to `targets candidates` / `targets adopt`

## 13. Acceptance Criteria

This design is complete when:

- `app-live apply` supports smoke and conservative live under a single command surface
- live `apply` no longer returns a generic unsupported-path error
- live `apply` fails closed when target adoption or rollout posture is still missing
- live `apply` fails closed when legacy explicit-target high-level flow is detected
- live `apply` reuses `doctor` and optionally `run`
- `--start` continues into foreground `run`
- `restart-required` remains an explicit manual boundary
- `doctor` runs before any restart-boundary confirmation
- conflicting active running sessions are surfaced and not auto-overridden by `apply`
- `status` routes live-ready Day 1+ operators into `apply`, not back into the old `doctor`/manual-restart wording
- no live control-plane mutation authority is added
- no process-management authority is added
- smoke behavior remains intact

## 14. Recommendation

This should be the next UX feature after run-session lifecycle and Polymarket source defaults.

It is the highest-leverage remaining step because it turns the current high-level primitives into a more coherent operator path without expanding authority boundaries prematurely.
