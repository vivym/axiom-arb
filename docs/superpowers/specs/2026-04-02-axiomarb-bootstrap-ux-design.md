# AxiomArb Bootstrap UX Design

- Date: 2026-04-02
- Status: Draft for user review
- Project: `AxiomArb`

## 1. Summary

At current `HEAD`, the repository now has a much stronger operator path than it did at the start of this project:

- `app-live init` is an interactive wizard
- `app-live doctor` is a real venue-facing preflight
- `app-live run` is the runtime entrypoint
- `app-live targets ...` provides first-class target adoption and rollback controls

That is a meaningful UX improvement, but the main operator path is still too multi-step:

1. run `init`
2. inspect `targets candidates`
3. explicitly `targets adopt`
4. run `doctor`
5. run `run`

For experienced operators this is now manageable, but it is still not close to “one obvious command”.

The next UX-focused subproject should add a higher-level orchestration command:

- `app-live bootstrap`

The recommended direction is:

- add `bootstrap` as a high-level operator entrypoint
- keep `init`, `doctor`, `run`, and `targets ...` as lower-level first-class commands
- first support only the safest two paths:
  - `paper`
  - `real-user shadow smoke`
- default to `config/axiom-arb.local.toml`
- if the config does not exist, create or complete it inline
- if smoke does not yet have an adopted startup target, prompt the operator to explicitly select and adopt one in the flow
- if smoke does not yet have rollout readiness for the adopted families, prompt the operator to explicitly enable a smoke-only rollout posture for those families
- reuse `doctor` for preflight and `run` for runtime startup
- stop after preflight by default, and only start automatically with `--start`

This is not a control-plane automation project.

It is a high-level startup orchestration project.

## 2. Current Repository Reality

At current `HEAD`, the repository already has the primitives needed for a `bootstrap` flow:

1. A single local TOML truth source
- normal operator config lives in `config/axiom-arb.local.toml`
- `DATABASE_URL` remains the only deployment env var

2. A guided config creator
- `app-live init` already knows how to create and update operator-local config interactively

3. A real preflight
- `app-live doctor` now covers:
  - config validation
  - credential checks
  - venue connectivity probes
  - startup target resolution
  - configured-vs-active state
  - mode-scoped runtime safety checks

4. First-class target control-plane commands
- `app-live targets candidates`
- `app-live targets status`
- `app-live targets show-current`
- `app-live targets adopt`
- `app-live targets rollback`

5. Safe shadow-smoke runtime boundaries
- `real_user_shadow_smoke` already exists
- startup authority remains `operator_target_revision`
- startup target changes remain restart-scoped, not hot-reloaded

So the main remaining UX problem is no longer missing backend capability.

The gap is that the operator still has to manually stitch together several commands into one startup session.

## 3. Goals

This design should guarantee the following:

- `app-live bootstrap` becomes a high-level operator entrypoint
- `bootstrap` first supports:
  - `paper`
  - `real-user shadow smoke`
- `bootstrap` defaults to `config/axiom-arb.local.toml`
- missing local config can be created or completed in-flow
- smoke bootstrap can inline an explicit target adoption step when no startup target anchor exists yet
- smoke bootstrap can inline an explicit smoke rollout enablement step for the adopted family set
- bootstrap-supported smoke startup is adopted-target-only
- bootstrap reuses existing `init`, `targets`, `doctor`, and `run` semantics rather than inventing parallel authority
- bootstrap writes back only long-lived config, startup target anchor changes, and explicitly confirmed smoke rollout changes
- bootstrap does not persist probe output or transient auth material
- bootstrap stops after preflight by default
- `bootstrap --start` may continue into runtime startup only after all blocking steps pass
- startup authority remains the existing `operator_target_revision`
- first-phase smoke bootstrap must distinguish between preflight-only smoke and shadow-work-ready smoke

## 4. Non-Goals

This design does not define:

- automatic candidate adoption
- automatic selection of the latest adoptable revision without operator confirmation
- support for production live bootstrap in the first phase
- hot reload of running target state
- a replacement for `init`, `doctor`, `run`, or `targets ...`
- new runtime/posture semantics
- a parallel control-plane service
- persistence of temporary probe output inside the config file
- first-class bootstrap support for legacy explicit-target startup

## 5. Architecture Decision

### 5.1 Recommended Approach

Add `app-live bootstrap` as an orchestration layer above the current operator commands.

Hard rules:

- `bootstrap` is a coordinator, not a second implementation of startup logic
- `bootstrap` may guide and sequence existing workflows
- `bootstrap` must not introduce a second startup authority
- `bootstrap` must not bypass explicit adoption confirmation
- `bootstrap` must not silently leave smoke in a preflight-only state and then imply that `--start` will exercise shadow work

### 5.2 Why Not Just Keep Improving `init -> doctor -> run`

That path is now coherent, but it is still not ergonomic enough for first-run or occasional operators.

The remaining friction is not inside each individual command.

It is in the command handoff points:

- config may or may not exist
- smoke may or may not already have an adopted target
- `doctor` may pass or fail with a next action
- the operator still has to decide whether to call `run`

`bootstrap` should own those handoffs.

### 5.3 Why Not Collapse Everything Into A Single Always-Start Command

A one-command implicit start would be too aggressive for current repository reality.

The operator still needs a safe stopping point after:

- config creation
- explicit target adoption
- preflight success

That is why the first phase should:

- stop after readiness by default
- require `--start` for automatic runtime startup

This keeps the path close to one-click without making the first version unsafe.

### 5.4 Why First-Phase Bootstrap Should Not Productize Explicit Targets

The repository still supports explicit-target startup at lower levels today, but that path is now legacy from a UX perspective.

For bootstrap-supported smoke startup, the correct long-term authority is:

- adopted target source
- explicit `operator_target_revision`

Not:

- hand-authored `negrisk.targets`

Hard rule:

- `bootstrap` should not become a polished high-level entry for explicit-target startup
- if bootstrap encounters a smoke config that is still using explicit targets, it should stop and direct the operator toward the adopted-target workflow or the lower-level legacy commands

## 6. Public UX Model

### 6.1 Command Surface

The public entry becomes:

```bash
cargo run -p app-live -- bootstrap
```

Supported first-phase examples:

```bash
cargo run -p app-live -- bootstrap
cargo run -p app-live -- bootstrap --config config/axiom-arb.local.toml
cargo run -p app-live -- bootstrap --start
```

`bootstrap` does not replace the lower-level commands.

The lower-level commands remain:

- `init`
- `targets ...`
- `doctor`
- `run`

### 6.2 Default Config Path

If `--config` is omitted, `bootstrap` should use:

- `config/axiom-arb.local.toml`

If that file does not exist:

- bootstrap enters init-like config creation flow

If that file exists:

- bootstrap attempts to reuse valid long-lived config values
- bootstrap only prompts for missing or invalid required values

### 6.3 First-Phase Mode Coverage

The first version should support only:

- `paper`
- `real-user shadow smoke`

It should not attempt to fully automate production `live` startup in the same phase.

The reason is simple:

- `paper` is the lowest-risk local path
- `real-user shadow smoke` is the most valuable near-one-command verification path
- production `live` still has sharper rollout and readiness semantics that should not be rushed into the same orchestration layer

## 7. Operator Flow

### 7.1 `paper`

Expected flow:

1. load or create config
2. confirm `paper` mode
3. fill or preserve minimal long-lived config
4. run `doctor` in `paper` mode
5. stop with a ready summary, or continue into `run` if `--start` is set

No target adoption step is required.

### 7.2 `real-user shadow smoke`

Expected flow:

1. load or create config
2. confirm smoke mode:
  - `runtime.mode = "live"`
  - `runtime.real_user_shadow_smoke = true`
3. ensure long-lived account, source, and relayer config is present
4. check whether startup-scoped `operator_target_revision` already exists in config
5. if it does not exist, list adoptable revisions and require an explicit operator selection
6. execute adoption through the existing adoption path
7. verify that the resulting smoke config is using adopted target source rather than explicit targets
8. inspect whether rollout readiness already covers the adopted family set
9. if rollout readiness does not yet cover the adopted family set:
  - offer a preflight-only stop path
  - or offer an explicit smoke-only rollout enablement step for those families
10. run `doctor`
11. stop with a ready summary, or continue into `run` if `--start` is set

Hard rule:

- bootstrap may inline an adoption interaction
- bootstrap must still require explicit operator confirmation
- bootstrap may inline a smoke rollout enablement interaction
- bootstrap must still require explicit operator confirmation for rollout changes

### 7.3 Smoke Rollout Readiness

First-phase smoke bootstrap should not silently populate:

- `approved_families`
- `ready_families`

But bootstrap should still be allowed to help the operator complete that decision.

As a result, smoke bootstrap must clearly distinguish two success states:

- `preflight-ready smoke startup`
- `shadow-work-ready smoke startup`

If rollout lists remain empty, bootstrap may still:

- pass config creation
- pass target adoption
- pass doctor
- stop in a preflight-only ready state

But it must tell the operator that the resulting run would remain a preflight-only smoke with zero `neg-risk` shadow rows.

If the operator explicitly enables smoke rollout readiness for the adopted family set, bootstrap may treat the flow as `shadow-work-ready` and allow `--start` to continue.

### 7.4 Default Stop Point

By default, bootstrap should stop after readiness is established.

That means the success path is:

- config ready
- target anchor ready when required
- preflight passed
- runtime not yet started

Only `--start` should continue into:

- `app-live run`

For smoke, `--start` should only be allowed from the `shadow-work-ready smoke startup` branch.

## 8. Internal Orchestration Model

### 8.1 State Machine

The orchestration layer should behave like a small state machine:

- `LoadOrCreateConfig`
- `SelectMode`
- `EnsureLongLivedConfig`
- `EnsureTargetAnchor`
- `EnsureSmokeRolloutReady`
- `RunPreflight`
- `ReadyToStart`
- `RunRuntime`

Not every mode needs every state:

- `paper` skips `EnsureTargetAnchor`
- `paper` skips `EnsureSmokeRolloutReady`
- `RunRuntime` is entered only when `--start` is present

### 8.2 Reuse Of Existing Command Semantics

`bootstrap` should reuse existing command semantics rather than copying logic into a parallel stack.

Specifically:

- config prompting and defaults should reuse `init` logic
- target listing and target adoption should reuse `targets` resolution and write semantics
- preflight should reuse `doctor`
- runtime startup should reuse `run`

This is the most important implementation boundary in the project.

If `bootstrap` becomes a second implementation of those behaviors, UX may appear smoother short-term but repo authority will immediately fragment.

### 8.3 Startup Authority

The only startup target authority remains:

- `[negrisk.target_source].operator_target_revision`

`bootstrap` may help the operator write that anchor by invoking existing adopt semantics.

It may not:

- synthesize a new authority
- silently choose the latest adoptable revision
- bypass provenance checks
- derive startup targets from runtime active state
- elevate legacy explicit targets into a first-class bootstrap authority

The rollout posture remains a separate config surface.

For first-phase smoke bootstrap, rollout may only be changed by:

- deriving family ids from the already adopted startup target
- showing those family ids to the operator
- requiring explicit confirmation before writing rollout lists

## 9. Persistence Boundaries

### 9.1 Values That May Be Written Back

`bootstrap` may write back long-lived, reusable values:

- mode selection
- smoke flag
- long-lived account credentials
- long-lived relayer credentials
- source configuration
- existing rollout configuration when preserved
- adopted `operator_target_revision`
- explicitly confirmed smoke rollout readiness derived from the adopted family set

### 9.2 Values That Must Not Be Persisted

`bootstrap` must not write back transient session state:

- preflight probe outcomes
- websocket or REST connection results
- derived timestamps and signatures
- temporary candidate selections that were not confirmed
- temporary venue-facing diagnostics

This keeps config as config, not a run log.

### 9.3 Target Adoption Writes

When smoke bootstrap must obtain a target anchor, it should:

1. list or summarize adoptable revisions
2. require explicit operator choice
3. execute the existing adopt write path
4. persist the resulting `operator_target_revision`

It should not directly mutate config with a guessed revision.

### 9.4 Smoke Rollout Writes

When smoke bootstrap offers rollout enablement, it should:

1. derive the family ids from the already adopted startup target
2. show the operator which families would be written
3. require explicit confirmation
4. write those family ids into both:
  - `approved_families`
  - `ready_families`

This write is intentionally limited to smoke bootstrap readiness.

It does not claim production live rollout readiness.

## 10. Error Handling

Bootstrap should classify failures by stage:

- `ConfigSetupError`
- `TargetAdoptionError`
- `SmokeRolloutError`
- `PreflightError`
- `StartError`

Each error must include a clear next action.

Examples:

- `ConfigSetupError`: rerun `bootstrap` or `init --update`
- `TargetAdoptionError`: inspect `targets candidates` and retry
- `SmokeRolloutError`: confirm or adjust rollout readiness, then retry bootstrap
- `PreflightError`: run `doctor --config ...` for full sectioned output
- `StartError`: inspect runtime logs from `run`

Hard rule:

- bootstrap must fail closed
- `--start` never overrides a failed adoption, failed rollout-readiness step, or failed preflight step

## 11. Output Model

Successful bootstrap output should summarize:

- config path
- chosen mode
- whether a startup target anchor exists
- whether startup is using adopted target source
- whether preflight passed
- whether rollout readiness is present or still empty
- whether runtime was started
- the next command, if runtime was not started

For smoke, output must explicitly state:

- this is `real-user shadow smoke`
- startup will request a shadow-only `neg-risk` path
- rollout readiness may still be empty
- this is not production live submit readiness

Bootstrap should still point expert users back to lower-level commands when useful:

- `init`
- `targets ...`
- `doctor`
- `run`

The high-level UX should not make the system opaque.

## 12. Testing Boundaries

### 12.1 `paper` Flow

Tests must verify:

- default config path behavior
- config creation when the file is missing
- safe reuse when the file exists
- `paper` skips target adoption
- `--start` enters existing `run` startup semantics only after successful preflight

### 12.2 Smoke Flow

Tests must verify:

- smoke mode selection sets the correct long-lived config intent
- missing target anchor triggers inline adopt workflow
- explicit operator choice is required before adoption
- successful adoption writes `operator_target_revision`
- missing rollout readiness triggers explicit smoke rollout choice
- explicit operator choice is required before rollout writes
- successful smoke rollout enablement writes adopted family ids into both rollout lists
- explicit-target smoke configs are rejected from the high-level bootstrap path with migration guidance
- failed preflight blocks startup even with `--start`
- empty rollout state is surfaced as a preflight-only smoke outcome rather than misreported as shadow-work-ready
- `--start` does not proceed while smoke remains in a preflight-only rollout state

### 12.3 Orchestration Boundaries

Tests must verify:

- bootstrap does not fork new startup authority
- bootstrap does not persist probe output
- bootstrap reuses existing `init`, `targets`, `doctor`, and `run` behavior
- bootstrap can stop at `ReadyToStart` without starting runtime

### 12.4 Output And Error Paths

Tests must verify:

- sectioned preflight results are surfaced clearly inside bootstrap
- failure stages map to the correct high-level error class
- next actions are present on both success and failure paths
- smoke output never misrepresents itself as production live readiness

## 13. Acceptance Criteria

This project is complete when:

- `app-live bootstrap` exists
- it defaults to `config/axiom-arb.local.toml`
- it supports `paper` and `real-user shadow smoke`
- it can create or complete local config in-flow
- smoke bootstrap can inline explicit target adoption when no startup target anchor exists
- smoke bootstrap can either stop in a clearly preflight-only state or explicitly enable smoke rollout readiness for adopted families
- smoke bootstrap only supports adopted target source as its high-level startup contract
- bootstrap reuses existing startup authority and lower-level command semantics
- bootstrap stops after successful readiness by default
- `bootstrap --start` enters runtime only after readiness passes, including smoke rollout readiness when shadow work is expected
- no transient probe or derived auth material is written into config
- rollout-empty smoke runs are reported as preflight-ready rather than shadow-work-ready
- the operator can use either:
  - `bootstrap`
  - `bootstrap --start`

as the normal high-level startup path for the first supported modes

## 14. Summary

The repository now has strong lower-level startup pieces, but the operator still has to manually bridge them.

The next UX step should add `app-live bootstrap` as a high-level orchestration command that composes:

- config creation or completion
- explicit target adoption when needed
- explicit smoke rollout enablement when needed
- sectioned preflight
- optional startup

The first phase should intentionally stay narrow:

- support `paper`
- support `real-user shadow smoke`
- default to `config/axiom-arb.local.toml`
- stop after readiness unless `--start` is passed

This keeps the project on the right side of its architecture:

- no new startup authority
- no automatic adoption
- no hot reload
- no duplicate runtime implementation

But it meaningfully improves the operator experience by turning a multi-command startup ceremony into one guided entrypoint.
