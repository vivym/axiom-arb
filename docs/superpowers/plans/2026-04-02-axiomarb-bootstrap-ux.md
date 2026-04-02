# AxiomArb Bootstrap UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `app-live bootstrap` as a high-level operator startup command for `paper` and `real-user shadow smoke`, including inline config completion, explicit adopted-target selection, explicit smoke rollout enablement, preflight gating, and optional startup via `--start`.

**Architecture:** Implement `bootstrap` as a thin orchestration layer on top of the existing `init`, `targets`, `doctor`, and `run` command semantics. Keep `[negrisk.target_source].operator_target_revision` as the only startup target authority, reject explicit-target smoke configs at the high-level UX layer, and require explicit confirmation for both target adoption and smoke rollout writes before allowing `--start`.

**Tech Stack:** Rust, `clap`, existing `config-schema` TOML config model, `sqlx`/Postgres persistence, current `app-live` command modules and test harnesses.

---

## File Map

### Existing files to modify

- `crates/app-live/src/cli.rs`
  - Add `BootstrapArgs` and the new `AppLiveCommand::Bootstrap` entry.
- `crates/app-live/src/main.rs`
  - Dispatch the new bootstrap command.
- `crates/app-live/src/commands/mod.rs`
  - Export the new `bootstrap` command module.
- `crates/app-live/src/commands/init.rs`
  - Expose reusable init-wizard entrypoints for bootstrap orchestration instead of only the standalone CLI path.
- `crates/app-live/src/commands/init/prompt.rs`
  - Reuse prompt I/O abstractions for bootstrap interactive flow.
- `crates/app-live/src/commands/init/wizard.rs`
  - Expose reusable wizard hooks for “load/create config” and live/smoke config completion.
- `crates/app-live/src/commands/doctor/mod.rs`
  - Expose reusable doctor execution/report hooks so bootstrap can run preflight without shelling out to itself.
- `crates/app-live/src/commands/targets/adopt.rs`
  - Expose reusable adoption helpers so bootstrap can perform explicit adopt after user selection.
- `crates/app-live/src/commands/targets/candidates.rs`
  - Expose reusable candidate listing/loading helpers for inline adopt selection.
- `crates/app-live/src/commands/run.rs`
  - Expose reusable run execution helper so bootstrap can enter normal startup without forking runtime semantics.
- `crates/app-live/src/commands/targets/config_file.rs`
  - Reuse or extend config rewrite helpers for explicit smoke rollout writes.
- `crates/app-live/src/commands/targets/state.rs`
  - Reuse target control-plane helpers for current configured/active revision context if needed by bootstrap output.
- `README.md`
  - Update startup flow to include `bootstrap`.
- `docs/runbooks/real-user-shadow-smoke.md`
  - Update the runbook to use `bootstrap` and explain preflight-only vs shadow-work-ready smoke.

### New files to create

- `crates/app-live/src/commands/bootstrap.rs`
  - High-level command entrypoint and orchestration state machine.
- `crates/app-live/src/commands/bootstrap/error.rs`
  - Bootstrap stage-specific error types.
- `crates/app-live/src/commands/bootstrap/flow.rs`
  - Orchestration logic for `paper` and `smoke`.
- `crates/app-live/src/commands/bootstrap/output.rs`
  - Success/failure summaries and next-action rendering.
- `crates/app-live/src/commands/bootstrap/prompt.rs`
  - Bootstrap-specific prompt helpers for adopt and rollout confirmation.
- `crates/app-live/tests/bootstrap_command.rs`
  - End-to-end CLI tests for the new bootstrap flow.

### Existing tests to extend

- `crates/app-live/tests/init_command.rs`
  - Keep init wizard reuse covered when bootstrap starts consuming its internals.
- `crates/app-live/tests/doctor_command.rs`
  - Reuse doctor expectations for bootstrap preflight output or next actions.
- `crates/app-live/tests/run_command.rs`
  - Reuse startup expectations for bootstrap `--start`.
- `crates/app-live/tests/targets_write_commands.rs`
  - Preserve direct adopt semantics while bootstrap reuses them.

---

### Task 1: Add Bootstrap CLI Skeleton

**Files:**
- Modify: `crates/app-live/src/cli.rs`
- Modify: `crates/app-live/src/main.rs`
- Modify: `crates/app-live/src/commands/mod.rs`
- Create: `crates/app-live/src/commands/bootstrap.rs`
- Test: `crates/app-live/tests/bootstrap_command.rs`

- [ ] **Step 1: Write the failing CLI test**

```rust
#[test]
fn bootstrap_help_lists_command() {
    let command = support::cargo_bin("app-live")
        .arg("--help")
        .assert()
        .failure();
    // Replace with a real assertion against stdout once helper shape is confirmed.
}
```

- [ ] **Step 2: Replace the placeholder with a real failing assertion**

Run: `cargo test -p app-live --test bootstrap_command bootstrap_help_lists_command -- --exact`

Expected: FAIL because `bootstrap` does not yet exist in the CLI output.

- [ ] **Step 3: Add the minimal CLI surface**

Implement:
- `BootstrapArgs { config: Option<PathBuf>, start: bool }`
- `AppLiveCommand::Bootstrap(BootstrapArgs)`
- `main.rs` dispatch to `commands::bootstrap::execute`
- `commands/mod.rs` export `bootstrap`
- `commands/bootstrap.rs` temporary stub that returns a clear `not implemented` error

- [ ] **Step 4: Update the test to assert the real help output**

Use a real CLI assertion that `bootstrap` appears in `app-live --help`.

- [ ] **Step 5: Run the focused test**

Run: `cargo test -p app-live --test bootstrap_command bootstrap_help_lists_command -- --exact`

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/app-live/src/cli.rs crates/app-live/src/main.rs crates/app-live/src/commands/mod.rs crates/app-live/src/commands/bootstrap.rs crates/app-live/tests/bootstrap_command.rs
git commit -m "feat: add bootstrap cli entrypoint"
```

### Task 2: Add Default Config Path And Paper Bootstrap

**Files:**
- Modify: `crates/app-live/src/commands/bootstrap.rs`
- Create: `crates/app-live/src/commands/bootstrap/error.rs`
- Create: `crates/app-live/src/commands/bootstrap/flow.rs`
- Create: `crates/app-live/src/commands/bootstrap/output.rs`
- Test: `crates/app-live/tests/bootstrap_command.rs`

- [ ] **Step 1: Write a failing paper-default-path test**

Add a test that:
- runs `app-live bootstrap` without `--config`
- feeds a `paper` mode selection through stdin
- asserts that `config/axiom-arb.local.toml` is created in a temp repo fixture
- asserts output says bootstrap stopped before runtime start

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p app-live --test bootstrap_command bootstrap_defaults_to_local_config_for_paper -- --exact --test-threads=1`

Expected: FAIL because bootstrap does not yet implement default config or interactive paper flow.

- [ ] **Step 3: Implement minimal paper bootstrap**

Implement:
- default config path resolution to `config/axiom-arb.local.toml`
- bootstrap paper flow:
  - load/create config
  - reuse init wizard for paper config creation
  - run doctor via a reusable internal helper
  - emit ready summary without starting runtime

- [ ] **Step 4: Run the focused test**

Run: `cargo test -p app-live --test bootstrap_command bootstrap_defaults_to_local_config_for_paper -- --exact --test-threads=1`

Expected: PASS

- [ ] **Step 5: Add a failing `--start` paper test**

Add a test asserting `bootstrap --start` enters the same paper startup path as `run`.

- [ ] **Step 6: Run the paper `--start` test to verify it fails**

Run: `cargo test -p app-live --test bootstrap_command bootstrap_paper_start_runs_runtime_after_preflight -- --exact --test-threads=1`

Expected: FAIL before `--start` wiring exists.

- [ ] **Step 7: Implement `--start` for paper**

Reuse the current `run` execution helper instead of duplicating startup logic.

- [ ] **Step 8: Run both paper tests**

Run: `cargo test -p app-live --test bootstrap_command bootstrap_paper_ -- --test-threads=1`

Expected: PASS

- [ ] **Step 9: Commit**

```bash
git add crates/app-live/src/commands/bootstrap.rs crates/app-live/src/commands/bootstrap/error.rs crates/app-live/src/commands/bootstrap/flow.rs crates/app-live/src/commands/bootstrap/output.rs crates/app-live/tests/bootstrap_command.rs
git commit -m "feat: add paper bootstrap flow"
```

### Task 3: Refactor Init/Doctor/Run For Reuse

**Files:**
- Modify: `crates/app-live/src/commands/init.rs`
- Modify: `crates/app-live/src/commands/init/wizard.rs`
- Modify: `crates/app-live/src/commands/init/prompt.rs`
- Modify: `crates/app-live/src/commands/doctor/mod.rs`
- Modify: `crates/app-live/src/commands/run.rs`
- Test: `crates/app-live/tests/init_command.rs`
- Test: `crates/app-live/tests/doctor_command.rs`
- Test: `crates/app-live/tests/run_command.rs`

- [ ] **Step 1: Write a failing bootstrap-driven reuse test**

In `bootstrap_command.rs`, add a test that expects bootstrap-created paper config to match the same schema-valid output shape as `init`.

- [ ] **Step 2: Run the focused test to capture the current mismatch**

Run: `cargo test -p app-live --test bootstrap_command bootstrap_reuses_init_doctor_run_semantics_for_paper -- --exact --test-threads=1`

Expected: FAIL until reusable hooks exist.

- [ ] **Step 3: Extract reusable init helpers**

Expose:
- init wizard execution with injected prompt/config path
- rendered-config validation

Do not change CLI behavior.

- [ ] **Step 4: Extract reusable doctor helpers**

Expose:
- an internal doctor execution API that returns the structured report and overall result

Do not change CLI behavior or output for the standalone command.

- [ ] **Step 5: Extract reusable run helper**

Expose:
- a helper callable by bootstrap for normal startup entry

Keep `run` CLI behavior unchanged.

- [ ] **Step 6: Run the focused bootstrap reuse test**

Run: `cargo test -p app-live --test bootstrap_command bootstrap_reuses_init_doctor_run_semantics_for_paper -- --exact --test-threads=1`

Expected: PASS

- [ ] **Step 7: Run regression tests for the reused commands**

Run:
```bash
cargo test -p app-live --test init_command -- --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test doctor_command -- --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test run_command -- --test-threads=1
```

Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add crates/app-live/src/commands/init.rs crates/app-live/src/commands/init/wizard.rs crates/app-live/src/commands/init/prompt.rs crates/app-live/src/commands/doctor/mod.rs crates/app-live/src/commands/run.rs crates/app-live/tests/bootstrap_command.rs
git commit -m "refactor: expose startup command helpers for bootstrap"
```

### Task 4: Add Smoke Bootstrap Config Completion

**Files:**
- Modify: `crates/app-live/src/commands/bootstrap.rs`
- Modify: `crates/app-live/src/commands/bootstrap/flow.rs`
- Create: `crates/app-live/src/commands/bootstrap/prompt.rs`
- Test: `crates/app-live/tests/bootstrap_command.rs`

- [ ] **Step 1: Write a failing smoke-config bootstrap test**

Add a test that:
- runs `app-live bootstrap`
- selects smoke mode
- provides long-lived account/relayer answers
- asserts the resulting config uses:
  - `runtime.mode = "live"`
  - `runtime.real_user_shadow_smoke = true`
  - adopted target source skeleton

- [ ] **Step 2: Run the focused test to verify it fails**

Run: `cargo test -p app-live --test bootstrap_command bootstrap_smoke_completes_local_config -- --exact --test-threads=1`

Expected: FAIL

- [ ] **Step 3: Implement smoke config completion**

Reuse init’s live/smoke wizard semantics inside bootstrap.

Do not yet implement adopt or rollout.

- [ ] **Step 4: Run the focused smoke-config test**

Run: `cargo test -p app-live --test bootstrap_command bootstrap_smoke_completes_local_config -- --exact --test-threads=1`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/bootstrap.rs crates/app-live/src/commands/bootstrap/flow.rs crates/app-live/src/commands/bootstrap/prompt.rs crates/app-live/tests/bootstrap_command.rs
git commit -m "feat: add smoke bootstrap config flow"
```

### Task 5: Inline Explicit Target Adoption For Smoke

**Files:**
- Modify: `crates/app-live/src/commands/targets/adopt.rs`
- Modify: `crates/app-live/src/commands/targets/candidates.rs`
- Modify: `crates/app-live/src/commands/bootstrap/flow.rs`
- Modify: `crates/app-live/src/commands/bootstrap/output.rs`
- Test: `crates/app-live/tests/bootstrap_command.rs`
- Test: `crates/app-live/tests/targets_write_commands.rs`

- [ ] **Step 1: Write a failing missing-anchor bootstrap test**

Add a test that:
- creates a smoke config without `operator_target_revision`
- seeds one adoptable revision in the test database
- runs bootstrap and selects that revision
- asserts the config now contains the adopted `operator_target_revision`

- [ ] **Step 2: Run the focused test to verify it fails**

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test bootstrap_command bootstrap_smoke_inlines_adopt_when_target_anchor_missing -- --exact --test-threads=1`

Expected: FAIL

- [ ] **Step 3: Expose reusable candidate/adopt helpers**

Add minimal reusable functions for:
- listing adoptable revisions for prompt display
- performing the existing adopt write path after explicit selection

Do not change standalone `targets` command behavior.

- [ ] **Step 4: Implement bootstrap adopt step**

Implement:
- prompt operator with adoptable revisions
- require explicit selection
- call reusable adopt helper
- persist resulting `operator_target_revision`

- [ ] **Step 5: Run the focused bootstrap adopt test**

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test bootstrap_command bootstrap_smoke_inlines_adopt_when_target_anchor_missing -- --exact --test-threads=1`

Expected: PASS

- [ ] **Step 6: Add a failing explicit-target rejection test**

Add a test asserting bootstrap refuses a smoke config that still relies on explicit `negrisk.targets`.

- [ ] **Step 7: Run the explicit-target rejection test to verify it fails**

Run: `cargo test -p app-live --test bootstrap_command bootstrap_smoke_rejects_explicit_target_configs -- --exact --test-threads=1`

Expected: FAIL

- [ ] **Step 8: Implement explicit-target rejection**

Bootstrap should stop with migration guidance rather than productizing the legacy path.

- [ ] **Step 9: Run both smoke target tests**

Run:
```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test bootstrap_command bootstrap_smoke_inlines_adopt_when_target_anchor_missing -- --exact --test-threads=1
cargo test -p app-live --test bootstrap_command bootstrap_smoke_rejects_explicit_target_configs -- --exact --test-threads=1
```

Expected: PASS

- [ ] **Step 10: Commit**

```bash
git add crates/app-live/src/commands/targets/adopt.rs crates/app-live/src/commands/targets/candidates.rs crates/app-live/src/commands/bootstrap/flow.rs crates/app-live/src/commands/bootstrap/output.rs crates/app-live/tests/bootstrap_command.rs crates/app-live/tests/targets_write_commands.rs
git commit -m "feat: inline smoke target adoption in bootstrap"
```

### Task 6: Inline Smoke Rollout Enablement

**Files:**
- Modify: `crates/app-live/src/commands/bootstrap/flow.rs`
- Modify: `crates/app-live/src/commands/bootstrap/prompt.rs`
- Modify: `crates/app-live/src/commands/targets/config_file.rs`
- Modify: `crates/app-live/src/commands/bootstrap/output.rs`
- Test: `crates/app-live/tests/bootstrap_command.rs`
- Test: `crates/app-live/tests/targets_config_file.rs`

- [ ] **Step 1: Write a failing rollout-enable test**

Add a bootstrap test that:
- starts from a smoke config with adopted `operator_target_revision`
- has empty rollout lists
- confirms rollout enablement
- asserts the adopted family ids are written into both `approved_families` and `ready_families`

- [ ] **Step 2: Run the focused rollout test to verify it fails**

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test bootstrap_command bootstrap_smoke_enables_rollout_for_adopted_families -- --exact --test-threads=1`

Expected: FAIL

- [ ] **Step 3: Implement reusable rollout config rewrite**

Add or extend config rewrite helpers to:
- derive family ids from the adopted startup targets
- rewrite both rollout lists atomically

- [ ] **Step 4: Implement bootstrap rollout prompt**

Implement:
- detect empty or incomplete rollout readiness
- offer either:
  - stop as preflight-only smoke
  - enable smoke rollout now
- require explicit confirmation before writing

- [ ] **Step 5: Run the focused rollout-enable test**

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test bootstrap_command bootstrap_smoke_enables_rollout_for_adopted_families -- --exact --test-threads=1`

Expected: PASS

- [ ] **Step 6: Add a failing preflight-only stop test**

Add a test asserting bootstrap surfaces preflight-only smoke and refuses `--start` when rollout remains empty.

- [ ] **Step 7: Run the focused preflight-only test to verify it fails**

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test bootstrap_command bootstrap_smoke_start_requires_rollout_readiness -- --exact --test-threads=1`

Expected: FAIL

- [ ] **Step 8: Implement preflight-only gating**

Implement:
- `--start` blocked while rollout remains preflight-only
- output clearly distinguishes `preflight-ready` vs `shadow-work-ready`

- [ ] **Step 9: Run both rollout tests**

Run:
```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test bootstrap_command bootstrap_smoke_enables_rollout_for_adopted_families -- --exact --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test bootstrap_command bootstrap_smoke_start_requires_rollout_readiness -- --exact --test-threads=1
```

Expected: PASS

- [ ] **Step 10: Commit**

```bash
git add crates/app-live/src/commands/bootstrap/flow.rs crates/app-live/src/commands/bootstrap/prompt.rs crates/app-live/src/commands/targets/config_file.rs crates/app-live/src/commands/bootstrap/output.rs crates/app-live/tests/bootstrap_command.rs crates/app-live/tests/targets_config_file.rs
git commit -m "feat: add smoke rollout readiness to bootstrap"
```

### Task 7: Wire Bootstrap Into Doctor And Run

**Files:**
- Modify: `crates/app-live/src/commands/bootstrap.rs`
- Modify: `crates/app-live/src/commands/bootstrap/flow.rs`
- Modify: `crates/app-live/src/commands/bootstrap/output.rs`
- Test: `crates/app-live/tests/bootstrap_command.rs`
- Test: `crates/app-live/tests/doctor_command.rs`
- Test: `crates/app-live/tests/run_command.rs`

- [ ] **Step 1: Write a failing bootstrap preflight test**

Add a test asserting bootstrap surfaces doctor section results and next actions on preflight failure.

- [ ] **Step 2: Run the focused test to verify it fails**

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test bootstrap_command bootstrap_surfaces_doctor_report_and_next_actions -- --exact --test-threads=1`

Expected: FAIL

- [ ] **Step 3: Implement bootstrap preflight integration**

Use reusable doctor execution/report helpers and render bootstrap-stage summaries without losing doctor meaning.

- [ ] **Step 4: Run the focused preflight integration test**

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test bootstrap_command bootstrap_surfaces_doctor_report_and_next_actions -- --exact --test-threads=1`

Expected: PASS

- [ ] **Step 5: Add a failing `--start` smoke success test**

Add a test asserting bootstrap enters the normal `run` path only after:
- adopted target exists
- rollout readiness exists
- doctor passes

- [ ] **Step 6: Run the focused `--start` test to verify it fails**

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test bootstrap_command bootstrap_start_runs_after_smoke_ready -- --exact --test-threads=1`

Expected: FAIL

- [ ] **Step 7: Implement `--start` runtime handoff**

Call the reusable `run` helper only from the fully ready path.

- [ ] **Step 8: Run both bootstrap runtime tests**

Run:
```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test bootstrap_command bootstrap_surfaces_doctor_report_and_next_actions -- --exact --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test bootstrap_command bootstrap_start_runs_after_smoke_ready -- --exact --test-threads=1
```

Expected: PASS

- [ ] **Step 9: Commit**

```bash
git add crates/app-live/src/commands/bootstrap.rs crates/app-live/src/commands/bootstrap/flow.rs crates/app-live/src/commands/bootstrap/output.rs crates/app-live/tests/bootstrap_command.rs crates/app-live/tests/doctor_command.rs crates/app-live/tests/run_command.rs
git commit -m "feat: wire bootstrap through doctor and run"
```

### Task 8: Update Docs And Regression Coverage

**Files:**
- Modify: `README.md`
- Modify: `docs/runbooks/real-user-shadow-smoke.md`
- Test: `crates/app-live/tests/bootstrap_command.rs`

- [ ] **Step 1: Write a failing doc-aligned smoke UX test**

Add a bootstrap output test that asserts the final smoke summary explicitly distinguishes:
- `preflight-ready smoke`
- `shadow-work-ready smoke`

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p app-live --test bootstrap_command bootstrap_smoke_summary_distinguishes_preflight_and_shadow_ready -- --exact --test-threads=1`

Expected: FAIL

- [ ] **Step 3: Implement the final output polish**

Adjust bootstrap summary rendering so operator-facing terms match the spec and docs.

- [ ] **Step 4: Update README**

Document the new preferred flows:
- `app-live bootstrap`
- `app-live bootstrap --start`
- fallback lower-level commands for expert/debug paths

- [ ] **Step 5: Update smoke runbook**

Document:
- adopted-only smoke bootstrap
- inline rollout enablement
- preflight-only vs shadow-work-ready states

- [ ] **Step 6: Run the focused summary test**

Run: `cargo test -p app-live --test bootstrap_command bootstrap_smoke_summary_distinguishes_preflight_and_shadow_ready -- --exact --test-threads=1`

Expected: PASS

- [ ] **Step 7: Run the full targeted regression set**

Run:
```bash
cargo fmt --all --check
cargo clippy -p app-live -p config-schema --all-targets -- -D warnings
cargo test -p app-live --test bootstrap_command -- --test-threads=1
cargo test -p app-live --test init_command -- --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test doctor_command -- --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test run_command -- --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test targets_write_commands -- --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test targets_config_file -- --test-threads=1
```

Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add README.md docs/runbooks/real-user-shadow-smoke.md crates/app-live/tests/bootstrap_command.rs
git commit -m "docs: add bootstrap operator workflow"
```

