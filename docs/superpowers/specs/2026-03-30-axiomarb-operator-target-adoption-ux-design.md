# AxiomArb Operator Target Adoption UX Design

- Date: 2026-03-30
- Status: Draft for user review
- Project: `AxiomArb`

## 1. Summary

At current `HEAD`, the repository can:

- continuously generate advisory `CandidateTargetSet` artifacts
- render `AdoptableTargetRevision` bridge artifacts
- preserve startup provenance through `operator_target_revision -> adoptable_revision -> candidate_revision`
- start `app-live` from a startup-scoped `operator_target_revision`

But the operator experience between those stages is still weak.

The repository does not yet provide a clean operator-facing workflow to:

- inspect current candidate and adoptable revisions
- see which `operator_target_revision` is currently configured or active
- explicitly adopt a new target revision
- roll back to a prior adopted target revision
- understand whether a controlled restart is required after a control-plane change

The next UX-focused subproject should turn those existing durable surfaces into a productized control-plane workflow inside `app-live`.

The core decision is:

- keep `app-live` as the only operator entrypoint
- add a `targets` command namespace under `app-live`
- make `operator_target_revision` the only control-plane authority
- keep the configured target anchor in the operator TOML at `[negrisk.target_source].operator_target_revision`
- keep candidate and adoptable revisions as provenance and selection context, never as a second startup authority
- keep adoption and rollback startup-scoped / controlled-restart-scoped rather than in-process hot reload

This is not an automatic target selection project.

It is a control-plane UX project for explicit operator adoption.

## 2. Current Repository Reality

At current `HEAD`, the backend contracts already exist:

1. `Phase 3e` candidate generation is real
- candidate artifacts are durable
- adoptable revisions are durable
- provenance from `operator_target_revision` back to candidate/adoptable context is durable

2. startup and restore already trust `operator_target_revision`
- runtime restore and fail-closed startup checks are anchored on `operator_target_revision`
- candidate/adoptable provenance is contextual, not the startup authority

3. startup UX has improved
- `app-live init`
- `app-live doctor`
- `app-live run`

4. operator adoption UX is still incomplete
- there is no first-class CLI to list candidates
- there is no first-class CLI to show the current adopted target state
- there is no explicit `adopt` command
- there is no explicit `rollback` command
- operators still need to infer control-plane state from low-level persistence/replay surfaces

This leaves a gap between:

- discovery/candidate generation
- startup/runtime execution

The missing layer is an operator-facing adoption/control-plane workflow.

## 3. Goals

This design should guarantee the following:

- `app-live` exposes operator-facing target control-plane commands
- the control-plane command surface is grouped under one namespace
- `operator_target_revision` remains the only durable authority for startup and restore
- candidate and adoptable revisions remain explainability and selection context only
- operators can inspect candidate/adoptable/current state without querying raw tables
- operators can explicitly adopt a target revision
- operators can roll back to the previous adopted target revision or an explicitly named older revision
- `configured` vs `active` target state is made explicit
- the system clearly reports when a restart is required for an adopted target change to take effect
- adoption history becomes durable and operator-visible
- `doctor` and runtime summaries align with adoption state rather than treating it as an implicit side channel

## 4. Non-Goals

This design does not define:

- automatic adoption of the latest candidate or adoptable revision
- automatic ranking or budget-aware candidate competition
- hot reloading of a running daemon's target revision
- a separate `axiomctl` or `control-plane` binary
- new target-discovery logic
- changes to activation/risk/planner/execution semantics beyond surfacing their already existing startup anchor
- a new multi-process control-plane service

## 5. Architecture Decision

### 5.1 Recommended Approach

Use `app-live` itself as the operator-facing control-plane surface.

Add a `targets` command namespace under `app-live`:

- `app-live targets status`
- `app-live targets candidates`
- `app-live targets show-current`
- `app-live targets adopt`
- `app-live targets rollback`

This keeps:

- startup UX
- control-plane UX
- runtime safety checks

in one operator entrypoint.

### 5.2 Why Not A Separate Control CLI

Creating a separate control binary would split operator truth across:

- startup docs
- adoption docs
- default behaviors
- safety checks
- restart guidance

The repository already has a single operator surface:

- `app-live`

The control-plane UX should be improved there instead of introducing another entrypoint.

### 5.3 Why The Control Plane Must Still Center `operator_target_revision`

The repository already treats `operator_target_revision` as the durable startup and restore anchor.

That must remain true because:

- restore safety already validates against it
- startup truth already resolves around it
- candidate/adoptable revisions are already explicitly modeled as provenance, not primary authority

Hard rule:

- no command may introduce a second primary control-plane identity
- any adoption flow must resolve to a single `operator_target_revision` before state is written
- no command in this phase may introduce a second durable configured-target store outside the operator config file

## 6. Public UX Model

### 6.1 Command Surface

The operator-facing target control-plane becomes:

- `app-live targets status`
- `app-live targets candidates`
- `app-live targets show-current`
- `app-live targets adopt`
- `app-live targets rollback`

The command namespace is organized around the revision lifecycle rather than raw persistence tables.

This phase intentionally does not add a dedicated `targets history` command.

History exists to support:

- latest action summaries
- current target explainability
- rollback resolution

Full operator-facing history browsing remains later work.

### 6.2 `targets status`

Purpose:

- show the current operator-visible target state summary

This is the compact summary view.

It should surface:

- configured `operator_target_revision`
- active `operator_target_revision`
- whether the configured revision differs from the active revision
- whether `restart_needed` is true
- whether provenance is complete
- what the last control-plane action was

It is the default health/status command for target control-plane state.

### 6.3 `targets candidates`

Purpose:

- show advisory candidates and adoptable revisions in a way an operator can understand

It should clearly distinguish:

- advisory candidate revisions
- adoptable revisions
- already adopted operator target revisions

It must not blur those into one generic “target” list.

### 6.4 `targets show-current`

Purpose:

- show the currently configured and currently active target revision in one explainable view

This is the detailed explainability view over the same state that `targets status` summarizes.

It should include:

- `configured_operator_target_revision`
- `active_operator_target_revision`
- `restart_needed`
- provenance chain when available
- target summary sufficient for operator verification

### 6.5 `targets adopt`

Purpose:

- explicitly write a new adopted `operator_target_revision`

It may accept operator input that refers to:

- `--operator-target-revision <revision>`
- `--adoptable-revision <revision>` that resolves to a single `operator_target_revision`

But its durable effect must always be:

- one adopted `operator_target_revision`

Exactly one selector flag must be provided.

This phase should not use a free-form positional revision argument that could ambiguously mean either revision type.

When the operator supplies `operator_target_revision` directly:

- that revision must already resolve through durable provenance or prior adoption history
- the command must not accept an arbitrary opaque revision string with no durable linkage
- if the revision cannot be linked back to a known provenance/history chain, the command must fail closed

If the requested revision is already the configured revision:

- the command should succeed as an explicit no-op
- it should not append a misleading new adoption transition
- it should still report whether the running daemon already matches that configured revision or whether restart is still required

### 6.6 `targets rollback`

Purpose:

- explicitly revert the configured target revision to a prior adopted state

It should support:

- default rollback to the previous adopted `operator_target_revision`
- explicit rollback to `--to-operator-target-revision <operator_target_revision>`

Default rollback should choose the previous distinct adopted `operator_target_revision`, not merely the immediately prior history row when repeated no-op adoptions of the same revision exist.

Rollback is not a UI-only undo.

It is a durable control-plane transition with history.

## 7. Data Model And Authority Boundaries

### 7.1 Single Authority Rule

`operator_target_revision` remains the only durable target authority for:

- startup
- restore
- doctor target resolution
- runtime configured-vs-active comparison

Candidate and adoptable revisions remain:

- provenance
- explainability
- operator selection context

They are not startup authority.

### 7.2 Adoption History Must Become First-Class

To support:

- `show-current`
- `rollback`
- auditability
- operator-readable provenance

the repository should gain an explicit durable adoption history surface.

That history should record at least:

- `adoption_id`
- `action_kind` = `adopt | rollback`
- `operator_target_revision`
- `previous_operator_target_revision`
- `adoptable_revision` when applicable
- `candidate_revision` when applicable
- adoption timestamp
- optional operator reason/context if already supportable

### 7.3 Configured Target State Must Remain In The Operator Config File

This phase must not introduce a new database-backed configured target pointer.

The configured target authority remains:

- the operator TOML passed to `app-live --config`
- specifically `[negrisk.target_source].operator_target_revision` when `source = "adopted"`

Adoption history and provenance remain durable persistence surfaces for:

- auditability
- explainability
- rollback traversal
- lineage reconstruction

They do not replace the operator config file as the configured target source of truth.

Hard rules:

- `targets adopt` updates the operator config file so that `[negrisk.target_source].operator_target_revision` points at the newly adopted revision
- `targets rollback` updates the same config key to the destination revision
- `app-live run` and `app-live doctor` continue to resolve configured startup authority from the config file
- runtime progress continues to represent the active/runtime anchor, not the configured/startup pointer

### 7.4 Current Target State Must Be Readable As A Single Surface

In addition to history, the repository should expose a concise current-state read model:

- configured operator target revision
- active operator target revision
- whether restart is required
- provenance chain
- latest action metadata

This should be optimized for `targets status` and `targets show-current`, not for append-only history queries.

This phase only requires enough history surface to support:

- latest action metadata
- rollback traversal
- provenance reconstruction for the current configured revision

It does not require a full history browser.

### 7.5 No In-Process Target Hot Reload

Adopting or rolling back a target revision must not directly mutate the running daemon's execution input in place.

Hard rule:

- adopt/rollback update startup-scoped control-plane state only
- the running daemon may report that a new configured revision exists
- the running daemon may report `restart_needed = true`
- the running daemon may not silently switch to a new active target revision without a controlled restart path

## 8. Command Behavior Contracts

### 8.1 `targets status`

Rules:

- read-only
- never mutates durable state
- may succeed when no active runtime state is available by explicitly reporting `active_state = unavailable`
- may report `restart_needed = unknown` when active runtime state is unavailable rather than inferred
- must not synthesize a last-known active revision from adoption history, provenance, or prior startup records
- must fail closed when configured/adoption provenance required for its summary is contradictory or partially corrupt
- must explicitly report `configured` vs `active`

### 8.2 `targets candidates`

Rules:

- read-only
- may sort or group for readability
- must not implicitly adopt or mutate control-plane state
- must clearly label advisory/adoptable/already-adopted states

### 8.3 `targets show-current`

Rules:

- read-only
- must provide a single explainable summary of the current control-plane state
- must explicitly show `restart_needed`
- may succeed when active runtime state is unavailable, but must say so explicitly instead of inventing an active revision
- must not substitute adoption history or provenance for active runtime state
- must not silently hide provenance gaps or contradictory configured/adoption state

### 8.4 `targets adopt`

Rules:

- explicit write operation
- must resolve to exactly one `operator_target_revision`
- must require exactly one of:
  - `--operator-target-revision`
  - `--adoptable-revision`
- must fail closed on:
  - ambiguous resolution
  - missing provenance
  - missing rendered target data
  - rendered target mismatch
- must write history
- must update configured target state by rewriting `[negrisk.target_source].operator_target_revision` in the operator config file supplied to the command
- must not hot-reload the active runtime target

On success it should clearly report:

- adopted `operator_target_revision`
- previous `operator_target_revision`
- provenance chain
- whether restart is required

`restart required` is a success-state/reporting outcome, not an adoption failure.

### 8.5 `targets rollback`

Rules:

- explicit write operation
- default behavior reverts to the previous adopted `operator_target_revision`
- `--to-operator-target-revision <operator_target_revision>` performs an explicit rollback-to-known-revision
- must fail closed if:
  - there is no previous revision
  - the requested destination revision is unknown
  - provenance/history is inconsistent
- must write a history record
- must update configured target state by rewriting `[negrisk.target_source].operator_target_revision` in the operator config file supplied to the command
- must not hot-reload the active runtime target

`restart required` after rollback is also a success-state/reporting outcome, not a rollback failure.

## 9. Runtime Linkage

### 9.1 Active Vs Configured Must Be First-Class

The operator UX should explicitly surface:

- `active_operator_target_revision`
- `configured_operator_target_revision`

For this phase:

- `configured_operator_target_revision` is read from the operator config file
- `active_operator_target_revision` is read from durable runtime state when available

If they differ:

- `restart_needed = true`

If active runtime state cannot be loaded at all:

- `configured_operator_target_revision` may still be shown
- `active_operator_target_revision` must be rendered as unavailable/unknown rather than guessed
- `restart_needed` may be rendered as unknown
- `status`, `show-current`, and `doctor` must not substitute persisted history/provenance for missing active runtime state

This must be visible through:

- `targets status`
- `targets show-current`
- runtime summary where appropriate

### 9.2 `doctor` Should Become Adoption-Aware

`app-live doctor` should report:

- whether the configured target revision resolves cleanly
- whether startup provenance is complete
- whether active runtime state is available
- whether active and configured state differ when active runtime state is available
- whether a restart is required before the daemon will actually use the configured revision when active runtime state is available
- whether restart-needed is unknown because runtime state is unavailable

It should not perform adopt/rollback.

It should only validate control-plane readiness.

### 9.3 `run` Must Continue To Start From Startup Authority

`app-live run` should continue to:

- resolve a single `operator_target_revision` from `[negrisk.target_source].operator_target_revision` in the supplied operator config file
- validate startup safety against that anchor
- carry that revision into runtime state

This design should not create a second runtime entry path.

It only makes the configured target state easier for operators to inspect and change safely.

### 9.4 `init` Must Continue To Seed The Configured Startup Anchor

`app-live init` should continue to render the startup-scoped operator target anchor in config.

That means:

- `init` seeds `[negrisk.target_source]`
- `init` writes `source = "adopted"`
- `init` writes an explicit `operator_target_revision` placeholder

Adoption UX then mutates that same config location rather than introducing a second configured-state surface.

## 10. Error Model

The operator-facing control-plane UX should classify errors into operator-readable categories such as:

- `NoAdoptableRevision`
- `NoPreviousOperatorTargetRevision`
- `AmbiguousRevisionResolution`
- `ProvenanceMissing`
- `RenderedTargetMismatch`
- `RuntimeStateUnavailable`

Error output should include a next step, for example:

- inspect candidates
- choose a specific revision
- rerun after restart
- resolve provenance drift before retrying

The command layer should not leak raw persistence errors without contextualizing them.

Important distinction:

- `RestartRequired` is not an error category
- it is a normal status/result field on successful `status`, `show-current`, `adopt`, and `rollback` flows
- `RuntimeStateUnavailable` should be treated as an informational state for `status`, `show-current`, and `doctor` when the daemon simply is not active
- `RuntimeStateUnavailable` becomes an error only when a command tries to validate runtime state that should be readable but is contradictory or partially corrupt rather than absent

## 11. Testing

### 11.1 `targets status` / `show-current`

Verify:

- configured vs active revisions are reported correctly
- provenance chains are rendered correctly
- missing or contradictory configured/adoption provenance fails closed
- absence of active runtime state renders as explicit unavailable/unknown state rather than command failure
- `restart_needed` is correct

### 11.2 `targets candidates`

Verify:

- advisory candidates, adoptable revisions, and already-adopted revisions are distinguished
- list output does not mislabel candidate state as active runtime state

### 11.3 `targets adopt`

Verify:

- adoption resolves to exactly one `operator_target_revision`
- exactly one of `--operator-target-revision` or `--adoptable-revision` is required
- ambiguous or inconsistent provenance fails closed
- successful adoption writes history and rewrites `[negrisk.target_source].operator_target_revision` in the operator config file
- successful adoption does not hot-reload active runtime state

### 11.4 `targets rollback`

Verify:

- default rollback chooses the previous adopted `operator_target_revision`
- explicit `--to-operator-target-revision` rollback works
- missing previous revision fails closed
- rollback writes history and rewrites `[negrisk.target_source].operator_target_revision` in the operator config file
- rollback does not hot-reload active runtime state

### 11.5 Runtime Linkage

Verify:

- `doctor` reports adoption-related readiness and restart-needed state
- `run` continues to consume only the config-file `operator_target_revision` startup anchor
- runtime summaries remain consistent with control-plane state

## 12. Acceptance Criteria

This project is complete when:

- `app-live` exposes a first-class `targets` control-plane namespace
- operators can inspect candidates, inspect current target state, adopt, and roll back through `app-live`
- `operator_target_revision` remains the only durable startup and restore authority
- `[negrisk.target_source].operator_target_revision` in the operator config file remains the only configured target authority
- provenance `operator_target_revision -> adoptable_revision -> candidate_revision` is operator-visible and restorable
- adopt/rollback remain startup-scoped and controlled-restart-scoped rather than hot-reloaded
- the system explicitly reports `configured` vs `active` target state and `restart_needed`
- `doctor` and runtime status align with the new control-plane state instead of leaving adoption as an implicit side channel

## 13. Follow-On Work

This design intentionally leaves later work for separate specs and plans:

- automatic adoption policy
- candidate ranking and budget-aware competition
- in-process hot reload or broader control-plane automation
- a richer multi-step operator UI beyond the command-line surface
