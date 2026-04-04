# AxiomArb Apply Flow UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a high-level `app-live apply` command that orchestrates the Day 1+ `real-user shadow smoke` flow by reusing `status`, `targets`, `doctor`, and `run` while keeping target authority and runtime truth unchanged.

**Architecture:** Build `apply` as a smoke-only orchestration layer above the existing operator primitives. Reuse `status` for readiness, `targets adopt` for target-anchor writes, bootstrap-style smoke rollout enablement for rollout writes, `doctor` for preflight, and `run` for foreground startup. Keep `apply` read-mostly, fail-closed, and explicit about the manual restart boundary.

**Tech Stack:** Rust, `clap`, existing `app-live` command modules, `config-schema`, `sqlx`/Postgres-backed integration tests, `cargo test`, `cargo clippy`

---

## File Map

### New files

- `crates/app-live/src/commands/apply/mod.rs`
  - Top-level `app-live apply` entrypoint, orchestration state machine, and output sequencing.
- `crates/app-live/src/commands/apply/model.rs`
  - Operator-facing state machine enums, orchestration context, and stage/result labels.
- `crates/app-live/src/commands/apply/output.rs`
  - Rendering for `Current State`, `Planned Actions`, `Execution`, `Outcome`, and `Next Actions`.
- `crates/app-live/src/commands/apply/prompt.rs`
  - Explicit confirmation prompts for adopt selection, smoke rollout enablement, and restart-boundary confirmation.
- `crates/app-live/tests/apply_command.rs`
  - CLI-facing integration tests for the smoke-only apply flow.
- `crates/app-live/tests/support/apply_db.rs`
  - Dedicated Postgres/config helpers for apply scenarios, wrapping or reusing existing support helpers where practical.

### Modified files

- `crates/app-live/src/cli.rs`
  - Add `ApplyArgs` and the new `apply` subcommand.
- `crates/app-live/src/main.rs`
  - Route the new `apply` subcommand.
- `crates/app-live/src/commands/mod.rs`
  - Export the new `apply` module.
- `crates/app-live/src/commands/status/mod.rs`
  - Change smoke Day 1+ next-action rendering from `bootstrap` to `apply` where the spec requires it.
- `crates/app-live/src/commands/status/model.rs`
  - If needed, add or tighten action labeling so `apply` ownership is explicit for smoke rollout progression.
- `crates/app-live/src/commands/bootstrap/prompt.rs`
  - Reuse or extract prompt behavior if the new apply prompts should match bootstrap choices.
- `crates/app-live/src/commands/bootstrap/flow.rs`
  - Study-only unless a small shared helper extraction is needed to avoid duplicating rollout write logic.
- `crates/app-live/tests/status_command.rs`
  - Add regression coverage for smoke next actions now pointing to `apply`.
- `README.md`
  - Document `apply` as the Day 1+ smoke progression path.
- `docs/runbooks/real-user-shadow-smoke.md`
  - Update the operator flow to use `apply` after bootstrap.
- `docs/runbooks/operator-target-adoption.md`
  - Update smoke-oriented follow-up guidance where readiness now points to `apply`.

### Existing files to study before editing

- `crates/app-live/src/commands/bootstrap/flow.rs`
- `crates/app-live/src/commands/bootstrap/prompt.rs`
- `crates/app-live/src/commands/bootstrap/output.rs`
- `crates/app-live/src/commands/status/evaluate.rs`
- `crates/app-live/src/commands/status/model.rs`
- `crates/app-live/src/commands/status/mod.rs`
- `crates/app-live/src/commands/doctor/mod.rs`
- `crates/app-live/src/commands/run.rs`
- `crates/app-live/src/commands/targets/adopt.rs`
- `crates/app-live/src/commands/targets/config_file.rs`
- `crates/app-live/tests/bootstrap_command.rs`
- `crates/app-live/tests/status_command.rs`
- `crates/app-live/tests/doctor_command.rs`
- `crates/app-live/tests/run_command.rs`
- `crates/app-live/tests/verify_command.rs`
- `docs/superpowers/specs/2026-04-03-axiomarb-apply-flow-ux-design.md`

---

### Task 1: Add the `apply` CLI Surface

**Files:**
- Modify: `crates/app-live/src/cli.rs`
- Modify: `crates/app-live/src/main.rs`
- Modify: `crates/app-live/src/commands/mod.rs`
- Create: `crates/app-live/src/commands/apply/mod.rs`
- Create: `crates/app-live/tests/apply_command.rs`

- [ ] **Step 1: Write the failing CLI exposure test**

Create `crates/app-live/tests/apply_command.rs` with:

```rust
mod support;

use std::process::Command;

use support::cli;

#[test]
fn apply_subcommand_is_exposed() {
    let output = Command::new(cli::app_live_binary())
        .arg("apply")
        .arg("--help")
        .output()
        .expect("app-live apply --help should execute");

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("--config"), "{text}");
    assert!(text.contains("--start"), "{text}");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run:

```bash
cargo test -p app-live --test apply_command apply_subcommand_is_exposed -- --exact --test-threads=1
```

Expected: FAIL because the CLI does not expose `apply` yet.

- [ ] **Step 3: Add the minimal CLI plumbing**

Add:

```rust
#[derive(clap::Args, Debug)]
pub struct ApplyArgs {
    #[arg(long)]
    pub config: PathBuf,

    #[arg(long)]
    pub start: bool,
}
```

Also add:

- `AppLiveCommand::Apply(ApplyArgs)` in `crates/app-live/src/cli.rs`
- `pub mod apply;` in `crates/app-live/src/commands/mod.rs`
- command routing in `crates/app-live/src/main.rs`
- a placeholder `execute(args: ApplyArgs) -> Result<(), Box<dyn Error>>` in `crates/app-live/src/commands/apply/mod.rs`

- [ ] **Step 4: Run the test to verify it passes**

Run:

```bash
cargo test -p app-live --test apply_command apply_subcommand_is_exposed -- --exact --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/cli.rs crates/app-live/src/main.rs crates/app-live/src/commands/mod.rs crates/app-live/src/commands/apply/mod.rs crates/app-live/tests/apply_command.rs
git commit -m "feat: add app-live apply command surface"
```

---

### Task 2: Define the Apply Model and Smoke-Only Guard

**Files:**
- Create: `crates/app-live/src/commands/apply/model.rs`
- Modify: `crates/app-live/src/commands/apply/mod.rs`
- Test: `crates/app-live/src/commands/apply/model.rs`
- Test: `crates/app-live/tests/apply_command.rs`

- [ ] **Step 1: Write the failing model and unsupported-scenario tests**

Add unit/integration tests for:

```rust
#[test]
fn apply_stage_labels_use_operator_vocabulary() {
    assert_eq!(ApplyStage::LoadReadiness.label(), "load-readiness");
    assert_eq!(ApplyStage::EnsureTargetAnchor.label(), "ensure-target-anchor");
    assert_eq!(ApplyStage::EnsureSmokeRollout.label(), "ensure-smoke-rollout");
    assert_eq!(ApplyStage::ConfirmManualRestartBoundary.label(), "confirm-manual-restart-boundary");
    assert_eq!(ApplyStage::RunPreflight.label(), "run-preflight");
    assert_eq!(ApplyStage::Ready.label(), "ready");
    assert_eq!(ApplyStage::RunRuntime.label(), "run-runtime");
}

#[test]
fn apply_rejects_non_smoke_config_with_specific_guidance() {
    // run against a paper fixture and assert the output references bootstrap/run
    // run against a live fixture and assert the output references status -> doctor -> run
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
cargo test -p app-live --test apply_command apply_rejects_non_smoke_config_with_specific_guidance -- --exact --test-threads=1
```

Expected: FAIL because `apply` does not enforce the smoke-only matrix yet.

- [ ] **Step 3: Add the minimal model and unsupported-scenario path**

Implement:

- `ApplyStage`
- `ApplyFailureKind`
- `ApplyScenario`
- smoke-only scenario detection based on validated config
- explicit unsupported guidance:
  - paper -> `bootstrap` or `run`
  - live -> `status -> doctor -> run`

- [ ] **Step 4: Run the tests to verify they pass**

Run:

```bash
cargo test -p app-live --test apply_command apply_rejects_non_smoke_config_with_specific_guidance -- --exact --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/apply/model.rs crates/app-live/src/commands/apply/mod.rs crates/app-live/tests/apply_command.rs
git commit -m "feat: add smoke-only apply flow guard"
```

---

### Task 3: Reuse `status` Readiness and Lock the Transition Matrix

**Files:**
- Modify: `crates/app-live/src/commands/apply/mod.rs`
- Modify: `crates/app-live/src/commands/status/mod.rs`
- Modify: `crates/app-live/src/commands/status/model.rs` (only if action labeling needs tightening)
- Modify: `crates/app-live/tests/status_command.rs`
- Modify: `crates/app-live/tests/apply_command.rs`

- [ ] **Step 1: Write the failing readiness-mapping tests**

Add tests that cover:

- `blocked` -> `ReadinessError`
- `target-adoption-required` -> `EnsureTargetAnchor`
- `smoke-rollout-required` -> `EnsureSmokeRollout`
- `smoke-config-ready` -> `RunPreflight`
- `restart-required` -> restart-boundary gate
- smoke `status` next action points to `apply`, not `bootstrap`

Example assertions:

```rust
#[test]
fn status_smoke_rollout_required_points_to_apply() {
    let output = /* run app-live status against smoke config with rollout missing */;
    let text = cli::combined(&output);
    assert!(text.contains("Next: app-live apply --config"), "{text}");
    assert!(!text.contains("Next: app-live bootstrap --config"), "{text}");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test status_command status_smoke_rollout_required_points_to_apply -- --exact --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test apply_command -- --test-threads=1
```

Expected: FAIL because `status` still points smoke rollout enablement to bootstrap and `apply` does not consume the matrix yet.

- [ ] **Step 3: Implement the transition mapping**

Implement in `apply`:

- call the existing `status::evaluate::evaluate(...)`
- interpret only the current high-level status truth
- do **not** invent a parallel readiness model

Tighten `status` smoke next-action ownership:

- smoke Day 1+ actions point to `apply`
- first-run bootstrap guidance stays with `bootstrap`

- [ ] **Step 4: Run the tests to verify they pass**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test status_command -- --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test apply_command -- --test-threads=1
```

Expected: PASS for the new readiness-routing coverage.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/apply/mod.rs crates/app-live/src/commands/status/mod.rs crates/app-live/src/commands/status/model.rs crates/app-live/tests/status_command.rs crates/app-live/tests/apply_command.rs
git commit -m "feat: route smoke readiness progression through apply"
```

---

### Task 4: Inline Explicit Target Adoption

**Files:**
- Create: `crates/app-live/src/commands/apply/prompt.rs`
- Modify: `crates/app-live/src/commands/apply/mod.rs`
- Modify: `crates/app-live/tests/support/apply_db.rs`
- Modify: `crates/app-live/tests/apply_command.rs`

- [ ] **Step 1: Write the failing adopt-flow tests**

Add tests for:

- missing adopted target enters inline adopt selection
- canceling adoption stops without writes
- selecting an adoptable revision writes `operator_target_revision`

Suggested integration test shape:

```rust
#[test]
fn apply_can_inline_smoke_target_adoption() {
    // seed adoptable revision but no configured operator_target_revision
    // feed stdin selection into app-live apply
    // assert config now contains operator_target_revision
    // assert output reports adoption before returning to readiness
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test apply_command apply_can_inline_smoke_target_adoption -- --exact --test-threads=1
```

Expected: FAIL because `apply` does not yet orchestrate adoption.

- [ ] **Step 3: Implement prompt + adoption orchestration**

Implement:

- prompt helper for adoptable revision choice
- reuse `targets::state::load_target_candidates_catalog`
- reuse `targets::adopt::adopt_selected_revision`
- on success, return to `LoadReadiness`

Do not:

- auto-pick newest revision
- bypass explicit operator selection

- [ ] **Step 4: Run the tests to verify they pass**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test apply_command apply_can_inline_smoke_target_adoption -- --exact --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/apply/mod.rs crates/app-live/src/commands/apply/prompt.rs crates/app-live/tests/support/apply_db.rs crates/app-live/tests/apply_command.rs
git commit -m "feat: inline target adoption in apply flow"
```

---

### Task 5: Inline Explicit Smoke Rollout Enablement

**Files:**
- Modify: `crates/app-live/src/commands/apply/mod.rs`
- Modify: `crates/app-live/src/commands/apply/prompt.rs`
- Modify: `crates/app-live/tests/support/apply_db.rs`
- Modify: `crates/app-live/tests/apply_command.rs`
- Study: `crates/app-live/src/commands/bootstrap/flow.rs`

- [ ] **Step 1: Write the failing rollout-enable tests**

Add tests for:

- rollout-missing smoke config enters explicit confirmation
- confirm writes smoke-only rollout readiness for adopted family set
- decline keeps rollout unchanged and stops cleanly

Example:

```rust
#[test]
fn apply_can_inline_smoke_rollout_enablement() {
    // seed adopted target, rollout missing
    // confirm rollout in stdin
    // assert approved_families and ready_families now cover adopted family ids
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test apply_command apply_can_inline_smoke_rollout_enablement -- --exact --test-threads=1
```

Expected: FAIL because apply does not yet write smoke rollout readiness.

- [ ] **Step 3: Implement explicit smoke-only rollout enablement**

Reuse existing bootstrap logic where possible, but keep ownership in `apply`:

- resolve adopted family ids from current target source
- prompt for explicit confirmation
- write both rollout lists for those family ids
- return to `LoadReadiness`

Do not:

- silently enable rollout
- support live rollout in this task

- [ ] **Step 4: Run the tests to verify they pass**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test apply_command apply_can_inline_smoke_rollout_enablement -- --exact --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/apply/mod.rs crates/app-live/src/commands/apply/prompt.rs crates/app-live/tests/support/apply_db.rs crates/app-live/tests/apply_command.rs
git commit -m "feat: inline smoke rollout enablement in apply flow"
```

---

### Task 6: Add the Manual Restart Boundary and Foreground `--start`

**Files:**
- Modify: `crates/app-live/src/commands/apply/mod.rs`
- Create: `crates/app-live/src/commands/apply/output.rs`
- Modify: `crates/app-live/src/commands/apply/prompt.rs`
- Modify: `crates/app-live/tests/apply_command.rs`
- Modify: `crates/app-live/tests/support/apply_db.rs`

- [ ] **Step 1: Write the failing restart/start tests**

Add tests for:

- `restart-required` + no `--start` stops at ready with restart messaging
- `restart-required` + `--start` requires explicit confirmation
- declining restart confirmation stops without invoking `run`
- accepting restart confirmation enters `run`

Example:

```rust
#[test]
fn apply_start_requires_restart_confirmation_when_active_differs() {
    // seed configured != active
    // run apply --start without confirming
    // assert output reports restart boundary and process does not enter run
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test apply_command -- --test-threads=1
```

Expected: FAIL for the new restart/start coverage.

- [ ] **Step 3: Implement the restart-boundary gate and output renderer**

Implement:

- `Current State`
- `Planned Actions`
- `Execution`
- `Outcome`
- `Next Actions`

And add the explicit restart-boundary confirmation:

- only when `status` says `restart-required`
- only when `--start` is present
- no claim of automatic process supervision

Reuse:

- `doctor::run_report(...)`
- `run::run_from_config_path(...)`

- [ ] **Step 4: Run the tests to verify they pass**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test apply_command -- --test-threads=1
```

Expected: PASS for the restart/start orchestration coverage.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/apply/mod.rs crates/app-live/src/commands/apply/output.rs crates/app-live/src/commands/apply/prompt.rs crates/app-live/tests/support/apply_db.rs crates/app-live/tests/apply_command.rs
git commit -m "feat: add apply start orchestration"
```

---

### Task 7: Update Docs and Smoke Runbooks

**Files:**
- Modify: `README.md`
- Modify: `docs/runbooks/real-user-shadow-smoke.md`
- Modify: `docs/runbooks/operator-target-adoption.md`

- [ ] **Step 1: Write the failing docs assertions**

Add a lightweight docs checklist in the task notes and verify the old strings still exist before editing:

```bash
rg -n "bootstrap -> status -> doctor -> run -> verify|Next: app-live bootstrap --config|targets adopt" README.md docs/runbooks/real-user-shadow-smoke.md docs/runbooks/operator-target-adoption.md
```

Expected: existing docs still point smoke Day 1+ flow back through lower-level commands or bootstrap.

- [ ] **Step 2: Update the docs**

Make these changes:

- README:
  - document `app-live apply`
  - clarify that `bootstrap` is Day 0 and `apply` is Day 1+ smoke progression
- smoke runbook:
  - replace post-bootstrap Day 1+ smoke progression with `status -> apply`
  - keep `verify` as a separate follow-up after foreground `run`
- operator-target-adoption runbook:
  - where smoke progression is discussed, point to `apply`

- [ ] **Step 3: Verify docs reflect the new high-level flow**

Run:

```bash
rg -n "app-live apply|Day 1\\+ smoke|bootstrap.*Day 0|verify remains a separate follow-up" README.md docs/runbooks/real-user-shadow-smoke.md docs/runbooks/operator-target-adoption.md
git diff --check
```

Expected: docs mention `apply` and `git diff --check` is clean.

- [ ] **Step 4: Commit**

```bash
git add README.md docs/runbooks/real-user-shadow-smoke.md docs/runbooks/operator-target-adoption.md
git commit -m "docs: add apply flow guidance"
```

---

### Task 8: Final Verification and Integration Commit

**Files:**
- Verify all modified files from Tasks 1-7

- [ ] **Step 1: Run focused test suites**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test apply_command -- --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test status_command -- --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test doctor_command -- --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test run_command -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 2: Run static verification**

Run:

```bash
cargo fmt --all --check
cargo clippy -p app-live -p config-schema -p persistence --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 3: Run any newly added unit tests directly if needed**

Run:

```bash
cargo test -p app-live apply:: -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 4: Commit the integrated result**

```bash
git add crates/app-live/src/cli.rs crates/app-live/src/main.rs crates/app-live/src/commands/mod.rs crates/app-live/src/commands/apply crates/app-live/src/commands/status/mod.rs crates/app-live/src/commands/status/model.rs crates/app-live/tests/apply_command.rs crates/app-live/tests/status_command.rs crates/app-live/tests/support/apply_db.rs README.md docs/runbooks/real-user-shadow-smoke.md docs/runbooks/operator-target-adoption.md
git commit -m "feat: add smoke apply orchestration flow"
```
