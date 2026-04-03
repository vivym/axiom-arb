# AxiomArb Apply Flow UX Design

- Date: 2026-04-03
- Status: Draft for user review
- Project: `AxiomArb`

## 1. Summary

At current `HEAD`, the operator-facing command set is much stronger than it was at the start of the project:

- `app-live bootstrap` handles first-run `paper` and `real-user shadow smoke`
- `app-live status` is the high-level readiness homepage
- `app-live doctor` is the real venue-facing preflight gate
- `app-live run` is the runtime entrypoint
- `app-live verify` is the post-run local outcome check
- `app-live targets ...` provides first-class target adoption and rollback controls

That closes most of the missing capability, but the high-level daily smoke workflow is still fragmented.

After first-run bootstrap, the operator still has to manually stitch together:

1. check `status`
2. decide whether a target still needs adoption
3. decide whether smoke rollout still needs enablement
4. run `doctor`
5. decide whether to run
6. optionally run `verify`

The next UX-focused subproject should add a high-level orchestration command for this Day 1+ workflow:

- `app-live apply`

The recommended direction is:

- keep `bootstrap` scoped to Day 0 / first-run setup
- add `apply` as the high-level “advance current smoke intent to the next real system state” command
- first support only `real-user shadow smoke`
- reuse the existing `status`, `targets`, `doctor`, `run`, and `verify` semantics
- inline explicit target adoption when the smoke config still lacks an adopted startup target
- inline explicit smoke-only rollout enablement when the adopted family set is not yet rollout-ready
- stop at “ready to start” by default
- only enter `run` when `--start` is present

This is not a new startup authority.

It is a high-level orchestration layer above the current operator primitives.

## 2. Current Repository Reality

At current `HEAD`, the repository already has the core primitives needed for an `apply` flow:

1. First-run orchestration already exists
- `bootstrap` defaults to `config/axiom-arb.local.toml`
- it can drive config creation, target adoption, smoke rollout enablement, `doctor`, and optional startup

2. Readiness is now centralized
- `status` answers the high-level config-and-control-plane question:
  - do I still need adoption
  - do I still need rollout enablement
  - do I need a controlled restart
  - is the next step `doctor`
  - is the next step `run`

3. Smoke startup authority is already clear
- startup authority remains `[negrisk.target_source].operator_target_revision`
- target adoption remains explicit and startup-scoped
- target changes are still restart-scoped, not hot-reloaded

4. Preflight and runtime are already separate
- `doctor` performs venue-facing preflight
- `run` performs runtime startup
- `verify` evaluates local post-run evidence without venue probes

So the missing piece is not a new subsystem.

The missing piece is a high-level operator flow that can take “current config + current readiness” and move the system to the next valid state without forcing the operator to drop back into multiple lower-level commands.

## 3. Goals

This design should guarantee the following:

- add a new high-level `app-live apply` command
- first support only `real-user shadow smoke`
- keep `bootstrap` focused on Day 0 / first-run setup
- make `apply` the Day 1+ smoke progression flow
- let `apply` consume existing readiness from `status`
- let `apply` inline explicit target adoption when no adopted startup target exists yet
- let `apply` inline explicit smoke-only rollout enablement when the adopted family set is not rollout-ready
- run `doctor` as the required preflight gate before any optional startup
- stop at “ready to start” by default
- allow `--start` to continue into `run`
- keep target authority on `operator_target_revision`
- keep runtime active truth owned by `run`, not by `apply`
- persist only long-lived config and control-plane changes

## 4. Non-Goals

This design does not define:

- production `live` apply orchestration in the first phase
- a replacement for `bootstrap`
- a replacement for `status`
- a replacement for `doctor`
- a replacement for `run`
- a replacement for `verify`
- automatic candidate adoption
- silent or automatic rollout enablement
- detached or background process supervision
- automatic verification chaining after `run` in the first phase
- hot reload of runtime target state
- persistence of probe or verify output
- productization of legacy explicit-target startup in the high-level flow

## 5. Architecture Decision

### 5.1 Recommended Approach

Add `app-live apply` as an orchestration layer above the current high-level and low-level operator commands.

Hard rules:

- `apply` is a coordinator, not a second implementation of smoke startup logic
- `apply` must reuse `status`, `targets`, `doctor`, `run`, and `verify` semantics rather than inventing new ones
- `apply` must not introduce a second startup authority
- `apply` must not silently perform adoption
- `apply` must not silently perform rollout enablement
- `apply` must not directly mark configured state as active
- `apply` must not claim to stop or replace an already-running daemon on its own
- `apply` must not write transient probe or verify state into config

### 5.2 Why `apply` Instead Of Growing `bootstrap`

`bootstrap` is now naturally aligned with Day 0 semantics:

- missing config
- missing long-lived operator credentials
- missing first adopted target
- first smoke rollout enablement
- first preflight

If the same command keeps expanding to cover daily control-plane application, controlled restart, optional runtime startup, and optional result verification, it stops meaning “bootstrap” and becomes an overloaded do-everything command.

`apply` creates a cleaner lifecycle split:

- `bootstrap`: first-run / initialization / initial viable path
- `apply`: apply the current smoke intent to the next actual system state

### 5.3 Why Not Leave The Flow Split Across Existing Commands

The repo already has the capability surface, but the operator still has to compose it manually.

That leaves several UX gaps:

- `status` can say “rollout required”, but the operator still has to decide which exact command comes next
- `restart-required` is visible, but there is no high-level “apply configured state now” flow
- `verify` exists, but only as a separate post-run command

`apply` should own those handoff points.

### 5.4 Why First-Phase `apply` Should Stay Smoke-Only

The highest UX leverage is in `real-user shadow smoke`.

That path already has:

- adopted-target semantics
- smoke-only rollout semantics
- preflight semantics
- post-run verification semantics

By contrast, production `live` still has sharper rollout and readiness semantics, and paper has much lower orchestration value.

The first version should therefore:

- optimize smoke flow
- keep `live` for a later phase

## 6. Public UX Model

### 6.1 Command Surface

The public entry becomes:

```bash
cargo run -p app-live -- apply --config config/axiom-arb.local.toml
cargo run -p app-live -- apply --config config/axiom-arb.local.toml --start
```

The command does not replace lower-level commands.

It becomes the high-level Day 1+ smoke entry.

### 6.2 Supported First-Phase Scenario

The first version should support only:

- `real-user shadow smoke`

If `apply` is invoked against a non-smoke config, the unsupported path should be explicit:

- `paper`
  - direct the operator back to `bootstrap` or `run`
- `live`
  - direct the operator back to `status -> doctor -> run`

It should not give a generic “bootstrap or lower-level path” message.

### 6.3 Relationship To Existing Commands

- `bootstrap`
  - first-run config/bootstrap path

- `status`
  - current readiness truth

- `targets ...`
  - lower-level target inspection, adoption, rollback

- `doctor`
  - preflight gate

- `run`
  - runtime startup

- `verify`
  - post-run local outcome validation

`apply` sits above those commands as a smoke-focused workflow coordinator.

### 6.4 Status Action Ownership

The first implementation should also tighten smoke next-action ownership at the high-level layer.

Today, `status` still points smoke rollout enablement back to `bootstrap`.

Once `apply` exists, the high-level smoke flow should change to:

- smoke adoption / rollout / start progression
  - `status` points to `apply`
- first-run config/bootstrap work
  - `status` may still point to `bootstrap`

That keeps the Day 0 / Day 1+ split consistent:

- `bootstrap` owns first-run setup
- `apply` owns ongoing smoke progression

## 7. Operator Flow

### 7.1 Default `apply`

Expected flow:

1. load current smoke readiness
2. if no adopted startup target exists, inline explicit adoption
3. if rollout is not smoke-ready, inline explicit smoke-only rollout enablement
4. run `doctor`
5. stop after preflight success and report “ready to start”

This is the safe default.

### 7.2 `apply --start`

Expected flow:

1. perform the same readiness / adoption / rollout / preflight flow
2. if all blocking steps pass, continue into `run`
3. if `restart-required` was the main remaining blocker, `apply --start` may continue only after explicit operator confirmation that the old process has already been or is being manually replaced

This is still a foreground startup path.

It is not a detached process manager and it is not a remote restart controller.

### 7.3 Why First-Phase `apply` Should Not Chain `verify`

The first phase should stop at successful `run`.

Reason:

- `run` is the current long-lived foreground daemon entrypoint
- once `run` takes over successfully, control does not naturally return to `apply`
- claiming that `apply --start --verify` can run `verify` after startup would require a separate detached-session or supervisor contract that does not exist yet

So the correct first-phase boundary is:

- `apply` may stop at “ready to start”
- `apply --start` may continue into foreground `run`
- `verify` remains a separate follow-up command until the repo has a real run-session identity and post-start orchestration contract

## 8. Status Transition Matrix

`apply` should not invent its own readiness interpretation.

It should map directly from high-level `status` outputs to orchestration states:

- `blocked`
  - stop immediately with `ReadinessError`
  - print the blocking reason and next action

- `blocked` + legacy explicit-target indicators
  - if `status` details/action indicate legacy explicit targets or migration is required
  - stop immediately with migration guidance
  - do not productize this path inside `apply`

- `target-adoption-required`
  - enter `EnsureTargetAnchor`

- `smoke-rollout-required`
  - enter `EnsureSmokeRollout`

- `smoke-config-ready`
  - enter `RunPreflight`

- `restart-required`
  - if rollout is still not ready, enter `EnsureSmokeRollout` first
  - otherwise surface a controlled-restart confirmation gate
  - only `--start` may continue beyond that gate

The key rule is:

- `apply` only advances when `status` already provides a valid next action
- it does not reinterpret `blocked` as recoverable on its own

In other words, legacy explicit-target handling must come from the existing `status` truth:

- target source
- readiness details
- migration action

It must not invent a new readiness enum branch just for `apply`.
## 9. Internal State Machine

The first version should be built as an explicit orchestration state machine.

Recommended states:

1. `LoadReadiness`
- reuse `status`
- get current readiness, scenario, and next-action context

2. `EnsureTargetAnchor`
- smoke-only
- if no adopted startup target exists, prompt for explicit adoption
- on success, return to `LoadReadiness`

3. `EnsureSmokeRollout`
- smoke-only
- if rollout is not ready, prompt for explicit smoke-only rollout enablement
- on success, return to `LoadReadiness`

4. `ConfirmManualRestartBoundary`
- entered only when readiness says `restart-required`
- only relevant when `--start` is present
- requires explicit operator confirmation that any existing process using the old revision is being handled outside `apply`

5. `RunPreflight`
- reuse `doctor`
- fail closed on any blocking preflight result

6. `Ready`
- default terminal state
- only reached when the system is ready to start

7. `RunRuntime`
- entered only with `--start`
- reuse `run`

## 10. Persistence Boundaries

### 9.1 Allowed Persistent Writes

The first version of `apply` may persist only:

1. target anchor updates
- via the existing `targets adopt` semantics
- including current adoption history / provenance writes

2. smoke rollout readiness updates
- only for the adopted family set
- only after explicit operator confirmation

### 9.2 Forbidden Persistent Writes

`apply` must not persist:

- preflight probe output
- dynamic auth material
- verify verdicts
- runtime transient state
- any synthetic “apply succeeded” durable marker

### 9.3 Active Runtime Truth

`apply` must not directly write active runtime truth.

If `configured != active`, then:

- `status` reports that drift
- `apply --start` may orchestrate the high-level action needed to close it
- but the actual active-state update still belongs to runtime startup and progress persistence inside `run`

That preserves the current authority boundary:

- config/control-plane express intent
- runtime startup makes that intent active

## 11. Interaction Model

### 10.1 Current State

At the start of every `apply` run, print:

- current scenario
- current readiness
- current target anchor state
- current rollout state
- restart-needed status

### 10.2 Planned Actions

Before making any write or transition, print the planned high-level actions, such as:

- adopt target revision
- enable smoke-only rollout
- confirm manual restart boundary
- run doctor
- start runtime

### 10.3 Explicit Confirmation

The first version requires explicit confirmation for:

- target adoption
- smoke-only rollout enablement
- manual restart boundary when `restart-required` and `--start` are combined

It must not auto-select the newest adoptable revision and must not silently enable rollout.

### 10.4 Outcome

At the end of the flow, print a concise high-level outcome such as:

- `Ready to start`
- `Started`
- `Blocked`

### 10.5 Next Actions

Every path must end with explicit next actions.

Examples:

- rerun `app-live apply --start`
- inspect `app-live doctor --config ...`
- run `app-live verify --config ...`
- no action required

## 12. Error Model

The first version should classify failures by stage:

- `ReadinessError`
- `AdoptionError`
- `RolloutEnablementError`
- `RestartBoundaryError`
- `PreflightError`
- `StartError`

The CLI does not need to expose those type names directly to operators, but it must preserve that stage boundary in output and next actions.

## 13. Testing Requirements

The first implementation should include:

1. state-machine tests
- missing target anchor
- rollout missing
- blocked readiness
- legacy explicit-target rejection
- restart-required confirmation gate
- preflight failure
- ready-without-start
- start path

2. orchestration-boundary tests
- `apply` reuses existing command semantics
- `apply` does not introduce new authority
- `apply` does not persist transient state
- `apply` does not pretend to supervise or stop an already-running daemon

3. interaction tests
- explicit adoption prompt
- explicit rollout confirmation
- explicit restart-boundary confirmation
- default stop-at-ready behavior
- `--start` only starts after successful preflight

4. smoke-specific regression tests
- restart-required flow
- adopted-target flow
- rollout enablement flow

## 14. Acceptance Criteria

This project is complete when:

- `app-live apply` exists
- first-phase support is limited to `real-user shadow smoke`
- `apply` can inline explicit target adoption
- `apply` can inline explicit smoke-only rollout enablement
- default `apply` stops at “ready to start”
- `apply --start` continues into `run`
- `apply` writes only long-lived target/control-plane changes
- `apply` does not write transient probe or verify state
- `apply` does not introduce a second startup authority
- `apply` does not claim background or remote restart management it does not actually provide
- the smoke Day 1+ operator path is materially simpler than manual `status -> targets -> doctor -> run -> verify`

## 15. Recommended Next Step

If this spec is approved, the next step should be an implementation plan focused on:

1. CLI surface and orchestration state machine
2. adoption and rollout confirmation plumbing
3. restart-boundary handling and `doctor` / `run` chaining
4. smoke-specific regression coverage
