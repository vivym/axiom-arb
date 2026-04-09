# 2026-04-09 AxiomArb Canonical Strategy-Control Rewrite Design

## Goal

Remove the remaining legacy explicit-target compatibility path from `app-live` and collapse live/smoke control-plane behavior onto a single canonical source of truth:

```toml
[strategy_control]
source = "adopted"
operator_strategy_revision = "..."
```

This rewrite is intentionally larger than a narrow compatibility deletion. It should also simplify control-plane modeling, startup resolution, readiness reporting, config validation, and operator UX so the project no longer carries pre-launch historical baggage.

## Why This Rewrite Exists

The current repository still carries a legacy `negrisk.targets` compatibility path that was originally preserved to migrate explicit neg-risk targets into the neutral adopted-revision control plane. That path now creates product and architecture drag:

- `status`, `doctor`, `bootstrap`, `apply`, `verify`, `run`, and `startup` all still contain compatibility-specific branches.
- live/smoke config can still be interpreted through multiple overlapping shapes:
  - canonical `[strategy_control]`
  - legacy `[negrisk.target_source]`
  - legacy explicit `[[negrisk.targets]]`
- config/view logic and command logic duplicate target-source detection.
- target-shaped naming (`operator_target_revision`) still leaks into operator-facing flows even though the neutral control plane is strategy-shaped.
- compatibility detection is currently coarse enough that even an empty `targets = []` can accidentally force read-only compatibility mode.

Because the software is not yet launched, the best long-term choice is to remove this historical baggage instead of continuing to maintain it.

## Non-Goals

This rewrite does not redesign the route-neutral strategy domain itself. It assumes the neutral adopted strategy lineage is already the intended control-plane authority and focuses on removing legacy entry paths and simplifying the app-facing control-plane architecture around it.

This rewrite does not redesign Polymarket transport/auth again; those changes remain separate.

## Desired End State

### Canonical config shape

For live/smoke configurations, the only app-facing control-plane source should be:

```toml
[strategy_control]
source = "adopted"
operator_strategy_revision = "..."
```

These shapes should no longer exist as runtime-facing control-plane inputs after migration:

- `[[negrisk.targets]]`
- `[negrisk.target_source]`
- `operator_target_revision`
- compatibility mode
- `--adopt-compatibility`

Old fields may still be parsed during migration, but they are not part of the post-rewrite steady state.

### Canonical runtime/control-plane model

All high-level live/smoke commands should resolve control-plane state through one canonical resolver. Command layers should no longer embed ad hoc compatibility detection or alias handling.

Commands that should consume the canonical resolved state include:

- `status`
- `doctor`
- `bootstrap`
- `apply`
- `verify`
- `run`
- `startup`

### Canonical naming

App-facing control-plane naming should become strategy-only:

- new code and operator UX should use `operator_strategy_revision`
- `operator_target_revision` should not appear in steady-state config UX, readiness UX, or command guidance
- any remaining `operator_target_revision` handling should be internal migration/provenance glue only

## Architectural Direction

### 1. Introduce a single StrategyControlResolver

Add a canonical control-plane resolution layer, e.g. `strategy_control::resolver`, that becomes the only authority for interpreting live/smoke control-plane config.

The resolver must be a pure read model:

- it may read validated config, persistence state, and runtime-progress state
- it must not rewrite config
- it must not create or mutate persistence records

All config rewriting and migration-related persistence writes should live in a separate migration helper, e.g. `strategy_control::migration`, which is called only by mutation-boundary commands.

#### Resolver vs migration-helper responsibilities

Responsibility should be split cleanly:

- `strategy_control::resolver`
  - parse and classify input shape
  - read canonical config, canonical persistence, and runtime-progress state
  - determine `ResolvedCanonicalStrategyControl` vs `MigrationRequired` vs `InvalidConfig`
  - never derive legacy input into canonical state by mutating storage
  - never rewrite config
- `strategy_control::migration`
  - accept only legacy input that the resolver has already classified as migratable
  - perform the actual legacy-to-canonical mapping
  - derive the canonical `operator_strategy_revision`
  - resolve legacy lineage
  - materialize required canonical persistence rows
  - rewrite config into canonical `[strategy_control]`

Legacy-to-canonical mapping should have one owner: the migration helper. The resolver may identify that a config is migratable, but it should not independently implement conversion logic.

The resolver should:

1. parse the validated config plus any required persistence state
2. identify whether the input is already canonical, migratable legacy input, or invalid/contradictory
3. return canonical strategy-control state that downstream commands can consume directly

The resolver may internally recognize legacy input categories, but app-facing command code should not keep explicit `legacy explicit targets` or `compatibility mode` enums as part of steady-state behavior.

Recommended result shape:

- `ResolvedCanonicalStrategyControl`
  - `operator_strategy_revision`
  - route artifacts / startup targets needed by command consumers
  - any additional adopted control-plane facts needed by status/readiness
- `MigrationRequired`
  - legacy input exists and must be rewritten at an approved mutation boundary before the command can proceed
- `InvalidConfig`
  - contradictory or non-mappable control-plane input

#### Persistence authority for canonical resolution

Canonical adopted resolution should be strategy-shaped. The authoritative persistence inputs for steady-state canonical resolution are:

- `strategy_adoption_provenance`
- `adoptable_strategy_revisions`
- `strategy_candidate_sets`
- strategy-shaped run-session/runtime-progress fields when commands need active/configured control-plane state

Legacy target-shaped persistence is not canonical control-plane truth after this rewrite. Existing target-shaped provenance/artifact tables may still be read only as migration input while converting old config or old persisted lineage into canonical strategy-shaped state, but steady-state command resolution should not depend on target-shaped fallback.

This rewrite does not require a broad database schema rename. It does require persistence read-path cleanup so canonical resolution prefers strategy-shaped provenance/artifacts and treats target-shaped provenance as migration-only compatibility data.

#### Precedence across config, persistence, and runtime-progress

The canonical precedence rules should be:

1. canonical config is the desired control-plane authority
   - if `[strategy_control]` exists, it defines the configured `operator_strategy_revision`
2. canonical persistence validates and materializes that configured strategy revision
   - `strategy_adoption_provenance`, `adoptable_strategy_revisions`, and `strategy_candidate_sets` must support the configured revision
3. runtime-progress and run-session records are observational only
   - they may report active/applied state
   - they may drive restart/conflict reporting
   - they must never replace or synthesize missing configured strategy control
4. legacy target-shaped persistence may be read only by the migration helper
   - never by steady-state canonical resolution

Therefore:

- config + missing canonical persistence => fail closed
- config + conflicting runtime-progress => report restart/conflict, but keep config as desired truth
- missing config cannot be repaired from runtime-progress alone

### 2. Remove duplicated target-source detection

Current target/control-plane source detection is duplicated across:

- `crates/config-schema/src/validate.rs`
- `crates/app-live/src/commands/status/evaluate.rs`
- `crates/app-live/src/commands/targets/state.rs`
- `crates/app-live/src/commands/bootstrap/flow.rs`
- `crates/app-live/src/startup.rs`

This rewrite should eliminate those parallel checks and centralize source interpretation inside the resolver.

The validated config view should expose canonical control-plane facts, not compatibility detection helpers.

### 3. Collapse startup onto canonical strategy control

`crates/app-live/src/startup.rs` currently supports both canonical adopted strategy control and compatibility-derived startup targets.

After this rewrite:

- startup should only consume canonical adopted strategy-control state
- compatibility-derived target rendering should be removed from startup truth
- any legacy config must be migrated before startup is allowed to proceed

### 4. Simplify status/apply/verify models

Delete app-facing compatibility concepts from status/apply/verify models and UX.

This includes removing concepts like:

- `LegacyExplicitTargets`
- `MigrateLegacyExplicitTargets`
- compatibility-specific `Blocked` reasons/actions
- verify’s special-case rejection path for legacy explicit targets
- apply’s compatibility-specific stop text

The resulting readiness/action models should describe only canonical control-plane states.

## Config Migration Strategy

### Legacy inputs that should be auto-migrated

The rewrite should recognize and migrate these old shapes:

- `[negrisk.target_source]`
- `operator_target_revision`
- explicit `[[negrisk.targets]]`

#### Legacy `[negrisk.target_source]` migration rules

`[negrisk.target_source]` is migratable only when all of the following are true:

- `source = "adopted"`
- `operator_target_revision` is present and non-empty
- canonical `[strategy_control]` is absent
- explicit `[[negrisk.targets]]` is absent

Migration contract:

- the migration helper must look up canonical lineage for the legacy `operator_target_revision`
- it must derive one unique canonical `operator_strategy_revision`
- if strategy-shaped canonical provenance/artifacts are missing but target-shaped legacy provenance/artifacts exist and can be deterministically converted, the migration helper must materialize the required canonical rows
- if the legacy revision cannot be linked to one unique canonical strategy revision, migration fails closed

Malformed legacy target-source input is invalid and not migratable, including:

- `source = "adopted"` with missing `operator_target_revision`
- `source = "adopted"` with empty `operator_target_revision`
- any mixed presence with canonical `[strategy_control]`
- any mixed presence with explicit `[[negrisk.targets]]`

#### Explicit `[[negrisk.targets]]` migration rules

Legacy explicit targets must follow deterministic migration rules:

- `targets = []`
  - invalid legacy input
  - not migratable
  - fail closed
- one or more explicit target rows
  - migratable only if the full target set parses successfully under the current strict rules:
    - every family has a `family_id`
    - family ids are unique
    - every member has `condition_id`, `token_id`, `price`, and `quantity`
  - the full parsed target set is authoritative for migration
  - the canonical `operator_strategy_revision` is synthesized deterministically from the full target-set digest, not from a partial row or first family

Multi-row target sets are valid migration input if and only if they parse into one deterministic full target set. There is no per-row migration and no “pick the first target” fallback.

At a mutation boundary, successful migration of explicit targets must materialize or upsert the canonical strategy-shaped persistence needed for later steady-state resolution:

- `strategy_candidate_sets`
- `adoptable_strategy_revisions`
- `strategy_adoption_provenance`

After successful rewrite, later canonical resolution should not need target-shaped fallback for that migrated config.

### Migration output

Migration must rewrite config into canonical `[strategy_control]` form:

```toml
[strategy_control]
source = "adopted"
operator_strategy_revision = "..."
```

Migration must remove old control-plane fields from the written config:

- `[[negrisk.targets]]`
- `[negrisk.target_source]`
- `operator_target_revision`

It must preserve unrelated config state, for example:

- rollout sections
- account/L2 auth sections
- source overrides
- ws/source timing settings
- route-owned runtime config

### Mutation boundaries that may auto-migrate

Automatic rewrite should only happen at explicit mutation boundaries that already own config mutation or control-plane advancement:

- `bootstrap`
- `apply`
- `targets adopt`
- `init` / config-writing flows

These commands may:

1. detect a legacy but migratable control-plane input
2. resolve the canonical strategy revision
3. materialize any required canonical strategy-shaped persistence rows
4. rewrite the config into canonical `[strategy_control]`
4. continue with the canonical flow

### Read-only commands must not auto-rewrite

These commands must remain read-only:

- `status`
- `doctor`
- `verify`

They should report that migration is required, but they must not mutate the TOML.

The canonical operator remediation path for read-only failures should be:

```bash
app-live targets adopt --config <config>
```

In this rewrite, `targets adopt` becomes the single explicit migration entrypoint for migratable old control-plane input. Read-only commands should all converge on that next step rather than emitting divergent migration guidance.

### Fail-closed migration rules

Automatic migration must fail closed when:

- legacy config cannot be mapped to one unique canonical adopted strategy revision
- required persistence lineage/artifacts are unavailable
- both canonical and legacy control-plane sources are simultaneously present
- multiple old shapes conflict with each other

In these cases:

- no config file rewrite occurs
- the command stops with a clear error
- the error explains which control-plane inputs are contradictory or missing

### Migration precedence for mixed inputs

Resolver behavior should be explicit when multiple control-plane shapes appear in the same config:

1. canonical `[strategy_control]` plus any legacy control-plane field
   - treat as contradictory
   - fail closed
   - do not attempt automatic preference or merge
2. legacy `[negrisk.target_source]` plus legacy explicit `[[negrisk.targets]]`
   - treat as contradictory
   - fail closed
3. canonical `[strategy_control]` alone
   - resolve as canonical
4. one legacy shape alone
   - resolve as migratable legacy input

The resolver should never silently prioritize one source over another when mixed control-plane shapes coexist.

## More Ambitious Cleanup Included In Scope

Because this rewrite is intentionally broader than a narrow deletion, it should also include the following architectural cleanup.

### A. Remove target-shaped app-facing naming

Where possible, app-facing code, output, and docs should stop speaking in terms of `target` revisions. Strategy control should be the operator-facing truth.

This means:

- canonical command UX should use `operator_strategy_revision`
- docs and examples should stop presenting `operator_target_revision` as the normal live/smoke control-plane anchor
- remaining `operator_target_revision` references should be confined to internal persistence/provenance paths that still require them

### B. Move alias handling out of validated views

`config-schema` currently participates in old/new alias interpretation. The rewrite should reduce that responsibility.

Preferred layering:

- parser/raw config can still deserialize old shapes
- canonical resolver decides whether old shapes are migratable or invalid
- validated app-facing views expose canonical control-plane semantics only

### C. Shrink readiness surface area

With compatibility removed, readiness and action models should get smaller. The goal is to avoid a broad matrix of legacy-specific blocked states and next actions.

## Command-Level Behavior After Rewrite

### status

- reads canonical resolver result
- if config is already canonical, report canonical readiness only
- if migration is required, report a clear migration-required state without calling it compatibility mode
- no legacy explicit target labels or compatibility wording
- remediation should point to `app-live targets adopt --config <config>`

### doctor

- reads canonical resolver result
- if migration is required, report that live/smoke control-plane migration must happen before full checks can proceed
- no compatibility skip text
- no `--adopt-compatibility` guidance
- remediation should point to `app-live targets adopt --config <config>`

### bootstrap

- if it encounters migratable old control-plane input, it may auto-rewrite config into canonical form and continue
- should not preserve a “legacy explicit targets” branch as a first-class bootstrap mode

### apply

- if it encounters migratable old control-plane input, it may auto-rewrite config into canonical form and continue
- should no longer stop with compatibility-specific wording

### verify

- should not contain legacy-explicit-specific branches in steady state
- if migration is still pending, it should fail as a generic control-plane migration precondition, not as a legacy explicit special case
- remediation should point to `app-live targets adopt --config <config>`

### run / startup

- should only start from canonical adopted strategy-control state
- legacy shapes must be migrated before startup

### targets adopt

- remove `--adopt-compatibility`
- against canonical input, continue to support canonical revision adoption flows
- against migratable old control-plane input, `targets adopt --config <config>` with no selectors becomes the one explicit migration entrypoint
- after migration succeeds, the config must be canonical `[strategy_control]` and the required canonical strategy-shaped persistence rows must exist

### targets rollback

- remove compatibility-aware rollback behavior
- rollback should operate only on canonical neutral/adopted strategy history
- if config is still using migratable old control-plane input, rollback should require migration first rather than synthesizing compatibility state

### other targets commands

The rest of the `targets` command surface should also stop surfacing compatibility concepts:

- `targets status`
- `targets show-current`
- `targets candidates`

After the rewrite they should:

- read canonical resolver/state only
- stop printing `compatibility_mode = ...`
- stop rendering compatibility-derived adopted/source summaries
- treat old control-plane input as migration-required or invalid, not as a long-lived read path

## Init, Example Config, and Docs

This rewrite should also clean operator UX so the project no longer teaches deprecated control-plane shapes.

Required updates:

- `init` wizard/render/summary should emit canonical `[strategy_control]`
- example config should show only canonical strategy-control shape for live/smoke
- README and runbooks should stop mentioning compatibility mode or legacy explicit targets as a normal path
- smoke runbook should assume canonical `[strategy_control]` and mutation-boundary migration if old configs are encountered
- `docs/runbooks/operator-target-adoption.md` should be rewritten or replaced so it teaches the canonical strategy-shaped control plane and the new explicit migration/remediation path

## Tests and Fixtures

Tests and fixtures that currently encode compatibility or legacy explicit targets as normal/expected behavior must be migrated or deleted.

This includes:

- config-schema validated view tests for legacy explicit compatibility
- status/apply/doctor/verify tests asserting compatibility mode behavior
- bootstrap tests that currently recognize legacy explicit mode
- targets adopt tests for `--adopt-compatibility`
- targets rollback/status/show-current/candidates tests asserting compatibility output or behavior
- fixture configs using `[[negrisk.targets]]` or `[negrisk.target_source]` as normal live/smoke setup

New tests should cover:

1. canonical `[strategy_control]` only live/smoke config
2. legacy old-shape config that is detected as migratable
3. mutation-boundary commands rewriting old config into canonical form
4. read-only commands refusing to rewrite while surfacing migration-required status
5. contradictory old+new control-plane input failing closed
6. startup/run requiring canonical adopted strategy-control state

The rewrite should also add direct resolver-level tests for the shared matrix:

- canonical input
- migratable `[negrisk.target_source]`
- migratable explicit `[[negrisk.targets]]`
- mixed canonical + legacy input
- missing canonical persistence
- conflicting runtime-progress vs configured strategy revision
- malformed legacy target-source input
- empty `targets = []`

## File-Level Impact

High-impact areas likely include:

- `crates/config-schema/src/validate.rs`
- `crates/app-live/src/cli.rs`
- `crates/app-live/src/daemon.rs`
- `crates/app-live/src/discovery.rs`
- `crates/app-live/src/runtime.rs`
- `crates/app-live/src/run_session.rs`
- `crates/app-live/src/supervisor.rs`
- `crates/app-live/src/queues.rs`
- `crates/app-live/src/source_tasks.rs`
- `crates/app-live/src/startup.rs`
- `crates/app-live/src/commands/status/*`
- `crates/app-live/src/commands/doctor/*`
- `crates/app-live/src/commands/doctor/target_source.rs`
- `crates/app-live/src/commands/bootstrap/*`
- `crates/app-live/src/commands/bootstrap/error.rs`
- `crates/app-live/src/commands/apply/*`
- `crates/app-live/src/commands/run.rs`
- `crates/app-live/src/commands/verify/*`
- `crates/app-live/src/commands/targets/adopt.rs`
- `crates/app-live/src/commands/targets/config_file.rs`
- `crates/app-live/src/commands/targets/rollback.rs`
- `crates/app-live/src/commands/targets/state.rs`
- `crates/app-live/src/commands/targets/status.rs`
- `crates/app-live/src/commands/targets/show_current.rs`
- `crates/app-live/src/commands/targets/candidates.rs`
- `crates/app-live/src/commands/init/*`
- `crates/app-live/src/commands/init/wizard.rs`
- `crates/app-live/src/commands/init/summary.rs`
- `config/axiom-arb.example.toml`
- `README.md`
- `docs/runbooks/real-user-shadow-smoke.md`
- `docs/runbooks/operator-target-adoption.md`

Likely test churn includes:

- `crates/config-schema/tests/*`
- `crates/app-live/tests/status_command.rs`
- `crates/app-live/tests/doctor_command.rs`
- `crates/app-live/tests/bootstrap_command.rs`
- `crates/app-live/tests/apply_command.rs`
- `crates/app-live/tests/run_command.rs`
- `crates/app-live/tests/verify_command.rs`
- `crates/app-live/tests/init_command.rs`
- startup/targets-related fixtures and support helpers

## Acceptance Criteria

This rewrite is complete when all of the following are true:

1. app-facing live/smoke control-plane config has one canonical shape:
   - `[strategy_control]`
   - `source = "adopted"`
   - `operator_strategy_revision = ...`
2. app-facing compatibility mode no longer exists.
3. `status`, `doctor`, `bootstrap`, `apply`, `verify`, `run`, and `startup` all consume the same canonical strategy-control resolver.
4. `[[negrisk.targets]]`, `[negrisk.target_source]`, and `operator_target_revision` are not steady-state control-plane inputs.
5. mutation-boundary commands can auto-migrate old config into canonical `[strategy_control]`.
6. read-only commands never rewrite config files.
7. contradictory old/new control-plane input fails closed.
8. app-facing UX no longer references compatibility mode, legacy explicit targets, or `--adopt-compatibility`.
9. fixtures/docs/examples no longer teach legacy explicit targets as a supported normal path.
10. command/test architecture is simpler because target-source detection is centralized and duplicated legacy checks are removed.
