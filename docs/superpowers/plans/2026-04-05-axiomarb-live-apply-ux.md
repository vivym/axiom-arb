# Live Apply UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend `app-live apply` from smoke-only to a unified Day 1+ high-level entry that also supports conservative normal `live` orchestration.

**Architecture:** Reuse the existing `status` readiness model, `doctor` preflight, and `run` foreground runtime startup under one `apply` command surface. Add a conservative live branch that stays fail-closed for adoption/rollout gaps, keeps manual restart boundaries explicit, and updates `status` so live-ready operators are routed into `apply` instead of the old multi-command path.

**Tech Stack:** Rust, Tokio, Postgres-backed persistence, Cargo integration tests, existing `app-live` command modules.

---

## File Map

- Modify: `crates/app-live/src/commands/apply/mod.rs`
  - Add the conservative live `apply` flow and shared routing.
- Modify: `crates/app-live/src/commands/apply/model.rs`
  - Add/adjust live-specific failure/stage text so unsupported live becomes a first-class flow.
- Modify: `crates/app-live/src/commands/apply/output.rs`
  - Reuse existing rendering helpers if live flow needs scenario-specific wording.
- Modify: `crates/app-live/src/commands/status/model.rs`
  - Add a high-level action contract for routing live-ready operators into `apply`.
- Modify: `crates/app-live/src/commands/status/evaluate.rs`
  - Emit the right `StatusAction` combinations for live-ready and restart-ready paths without disturbing rollout/adoption blockers.
- Modify: `crates/app-live/src/commands/status/mod.rs`
  - Render the new/updated live next actions to `app-live apply --config ...`.
- Test: `crates/app-live/tests/apply_command.rs`
  - Cover live routing, readiness gates, restart-boundary behavior, and foreground-start behavior.
- Test: `crates/app-live/tests/status_command.rs`
  - Cover live-ready/restart-ready next-action ownership changes.
- Modify: `crates/app-live/src/commands/verify/context.rs`
  - Keep `verify` aligned with the updated high-level live status actions it consumes from `status`.
- Modify: `crates/app-live/src/commands/verify/mod.rs`
  - Update source tests or action-driven wording expectations that pin the current live/restart status actions.
- Test: `crates/app-live/tests/verify_command.rs`
  - Update any assertions that depend on `status` next-action wording.
- Docs: `README.md`
  - Update the high-level operator flow so normal `live` uses `status -> apply [--start] -> verify`.
- Docs: `docs/runbooks/operator-target-adoption.md`
  - Update live Day 1+ guidance to point back into `apply` once target adoption is complete.
- Docs: `docs/runbooks/real-user-shadow-smoke.md`
  - Remove the now-stale “apply is smoke-only” wording and keep shared apply terminology aligned.

## Task 1: Add live scenario routing tests for `apply`

**Files:**
- Modify: `crates/app-live/tests/apply_command.rs`
- Read: `crates/app-live/tests/support/apply_db.rs`
- Read: `crates/app-live/tests/support/status_db.rs`

- [ ] **Step 1: Write the failing tests for live scenario routing and gates**

Add focused tests for:
- `live apply` no longer returning the generic unsupported error
- `target-adoption-required` stopping with adopt guidance
- `live-rollout-required` stopping before doctor/run
- `restart-required + rollout required` stopping before doctor/run
- generic `blocked` stopping with existing blocking guidance
- legacy explicit-target `blocked` stopping with migration-specific guidance
- `restart-required + --start + non-interactive` failing closed at the manual boundary
- `restart-required + interactive decline` stopping cleanly
- conflicting active running session + `--start` stopping at the boundary

- [ ] **Step 2: Run the targeted tests to verify they fail**

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test apply_command -- --test-threads=1`
Expected: FAIL in the new live-apply tests because `ApplyScenario::Live` is still unsupported.

- [ ] **Step 3: Implement minimal live branch routing in `apply`**

In `crates/app-live/src/commands/apply/mod.rs`:
- replace the current `ApplyScenario::Live => UnsupportedScenario::Live` branch with a new live execution path
- keep smoke flow unchanged
- share only orchestration helpers that do not blur smoke/live authority boundaries

In `crates/app-live/src/commands/apply/model.rs`:
- remove or narrow the generic live unsupported error path so paper remains unsupported but live is routed into the new flow

- [ ] **Step 4: Implement live readiness gating and manual-boundary semantics**

In `crates/app-live/src/commands/apply/mod.rs` implement a live flow that:
- loads `status::evaluate::evaluate(config_path)`
- stops immediately for `TargetAdoptionRequired`
- stops immediately for `LiveRolloutRequired`
- stops immediately for generic `Blocked`
- preserves legacy explicit-target blocked handling as a migration-specific stop, not a generic rollout/adoption stop
- treats `RestartRequired + rollout_state = Required` as rollout-blocked before doctor/run
- runs `doctor::run_report` before any restart confirmation
- if `--start` is absent, stops with `Ready to start`
- if `--start` is present and `RestartRequired` is false, goes directly into `run::run_from_config_path_with_invoked_by(..., "apply")`
- if `--start` is present and `RestartRequired` is true:
  - fail-closed when stdin is non-interactive
  - on interactive prompt decline, return `Ok(())` after rendering a clean stop
  - if there is a conflicting active running session in `summary.details.conflicting_active_run_session_id`, stop at the boundary even if interactive
  - otherwise allow foreground `run`

- [ ] **Step 5: Run the targeted tests to verify they pass**

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test apply_command -- --test-threads=1`
Expected: PASS for the new live-apply tests and existing smoke tests.

- [ ] **Step 6: Commit**

```bash
git add crates/app-live/src/commands/apply/mod.rs crates/app-live/src/commands/apply/model.rs crates/app-live/tests/apply_command.rs
git commit -m "feat: add conservative live apply flow"
```

## Task 2: Route live-ready `status` actions into `apply`

**Files:**
- Modify: `crates/app-live/src/commands/status/model.rs`
- Modify: `crates/app-live/src/commands/status/evaluate.rs`
- Modify: `crates/app-live/src/commands/status/mod.rs`
- Modify: `crates/app-live/src/commands/verify/context.rs`
- Modify: `crates/app-live/src/commands/verify/mod.rs`
- Test: `crates/app-live/tests/status_command.rs`
- Test: `crates/app-live/tests/verify_command.rs`

- [ ] **Step 1: Write the failing status tests**

Add targeted tests covering:
- `LiveConfigReady` now rendering `Next: app-live apply --config ...`
- `RestartRequired` with rollout ready now rendering `Next: app-live apply --config ...`
- `LiveRolloutRequired` continuing to render manual rollout guidance
- `TargetAdoptionRequired` continuing to render `targets adopt`
- legacy explicit-target `Blocked` continuing to render migration guidance

- [ ] **Step 2: Run the targeted tests to verify they fail**

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test status_command -- --test-threads=1`
Expected: FAIL because live-ready and restart-ready next actions still point to `doctor` / generic restart wording.

- [ ] **Step 3: Introduce an explicit status action contract for high-level apply routing**

In `crates/app-live/src/commands/status/model.rs`, choose one explicit mechanism:
- either add a new `StatusAction::RunApply`, or
- add a narrowly-scoped equivalent enum value with a clear label for high-level Day 1+ progression

Do not rely on prose-only rendering hacks.

- [ ] **Step 4: Emit the new action only for the intended live-ready paths**

In `crates/app-live/src/commands/status/evaluate.rs`:
- `LiveConfigReady` should emit the new apply action instead of `RunDoctor`
- `RestartRequired` with rollout ready should emit the new apply action instead of generic `PerformControlledRestart`
- `RestartRequired` with rollout required should still keep rollout-first guidance
- `LiveRolloutRequired`, `TargetAdoptionRequired`, and legacy explicit-target `Blocked` must keep their existing lower-level actions
- smoke mappings to `apply` must remain intact

- [ ] **Step 5: Render the new action to the correct command string**

In `crates/app-live/src/commands/status/mod.rs`:
- render the new action as `app-live apply --config {config}`
- keep existing command quoting behavior
- do not disturb paper, rollout-prep, or legacy migration guidance

- [ ] **Step 6: Update any verify assertions that depend on status next-action wording**

In `crates/app-live/src/commands/verify/context.rs` and `crates/app-live/src/commands/verify/mod.rs`:
- update any source-level action expectations that assume live-ready means `RunDoctor` or generic restart wording
- keep verify’s production consumption of `status` actions aligned with the new live-apply routing

In `crates/app-live/tests/verify_command.rs`, update only the external assertions impacted by the new live-ready status action routing.

- [ ] **Step 7: Run the targeted tests to verify they pass**

Run:
- `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test status_command -- --test-threads=1`
- `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test verify_command -- --test-threads=1`
- `cargo test -p app-live --lib verify:: -- --test-threads=1`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/app-live/src/commands/status/model.rs crates/app-live/src/commands/status/evaluate.rs crates/app-live/src/commands/status/mod.rs crates/app-live/src/commands/verify/context.rs crates/app-live/src/commands/verify/mod.rs crates/app-live/tests/status_command.rs crates/app-live/tests/verify_command.rs
git commit -m "feat: route live status actions into apply"
```

## Task 3: Polish live apply output and shared wording

**Files:**
- Modify: `crates/app-live/src/commands/apply/output.rs`
- Modify: `crates/app-live/src/commands/apply/mod.rs`
- Test: `crates/app-live/tests/apply_command.rs`

- [ ] **Step 1: Write the failing output assertions**

Add/expand assertions proving live `apply` renders:
- `Current State`
- `Planned Actions`
- `Execution`
- `Outcome`
- `Next Actions`

and that planned actions do not promise foreground start when:
- `--start` was not passed
- a conflicting active running session blocks the boundary

- [ ] **Step 2: Run the targeted tests to verify they fail**

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test apply_command live_ -- --test-threads=1`
Expected: FAIL in the new output assertions.

- [ ] **Step 3: Tighten live output rendering**

In `crates/app-live/src/commands/apply/mod.rs` and, if needed, `output.rs`:
- render `Current State` using the live summary details already provided by `status`
- only list foreground-start planned actions when `--start` is actually present, `doctor` has passed, and no conflicting active session is blocking the boundary
- render manual-boundary stop outcomes distinctly from runtime-start failures
- keep smoke wording unchanged unless shared helper extraction is required

- [ ] **Step 4: Run the targeted tests to verify they pass**

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test apply_command -- --test-threads=1`
Run: `cargo test -p app-live --lib apply:: -- --test-threads=1`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/apply/mod.rs crates/app-live/src/commands/apply/output.rs crates/app-live/tests/apply_command.rs
git commit -m "fix: tighten live apply output semantics"
```

## Task 4: Update docs to make live `apply` the Day 1+ path

**Files:**
- Modify: `README.md`
- Modify: `docs/runbooks/operator-target-adoption.md`
- Modify: `docs/runbooks/real-user-shadow-smoke.md`

- [ ] **Step 1: Write the doc assertions to satisfy**

Before editing, list the exact statements that must be true after the code changes:
- README live operator flow points to `status -> apply [--start] -> verify`
- target adoption runbook says that once adoption is complete and rollout posture exists, Day 1+ live progression returns to `apply`
- smoke runbook no longer claims `apply` is smoke-only

- [ ] **Step 2: Update docs minimally**

Edit the docs so they match the implemented command surface and authority boundaries:
- live `apply` is now supported but conservative
- it does not inline adoption or live rollout mutation
- `verify` remains separate
- manual restart boundary remains explicit

- [ ] **Step 3: Run grep-based verification**

Run:
- `rg -n "apply|doctor|run|verify|controlled restart|targets adopt" README.md docs/runbooks/operator-target-adoption.md docs/runbooks/real-user-shadow-smoke.md`
Expected: output matches the updated high-level flow without reintroducing the old live-only wording.

- [ ] **Step 4: Commit**

```bash
git add README.md docs/runbooks/operator-target-adoption.md docs/runbooks/real-user-shadow-smoke.md
git commit -m "docs: route live day-1 flow through apply"
```

## Task 5: Final verification sweep

**Files:**
- Verify only; no intended code changes

- [ ] **Step 1: Run focused integration tests**

Run:
- `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test apply_command -- --test-threads=1`
- `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test status_command -- --test-threads=1`
- `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test verify_command -- --test-threads=1`
- `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test doctor_command -- --test-threads=1`
- `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test run_command -- --test-threads=1`
- `cargo test -p app-live --lib -- --test-threads=1`

Expected: PASS.

- [ ] **Step 2: Run repo hygiene checks**

Run:
- `cargo fmt --all --check`
- `cargo clippy -p app-live --all-targets -- -D warnings`
- `git diff --check`

Expected: PASS.

- [ ] **Step 3: Commit final touch-ups if needed**

```bash
git add -A
git commit -m "chore: polish live apply flow"
```

Only do this if verification required a real code/doc change.
