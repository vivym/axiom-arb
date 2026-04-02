# AxiomArb Status And Readiness UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a new high-level `app-live status` command that reports config-and-control-plane readiness for `paper`, `live`, and `real-user shadow smoke` without running venue probes.

**Architecture:** Build a dedicated status aggregation layer that reads validated config, durable control-plane state, and runtime progress, then derives a small set of stable readiness states plus concrete next actions. Keep this new layer separate from `doctor` preflight and from the lower-level `targets status/show-current` explainability commands so later `verify` and `bootstrap v2` can reuse the same readiness model.

**Tech Stack:** Rust, `clap`, existing `config-schema` config model, existing `app-live` command structure, existing persistence/control-plane repos, `cargo test`, `cargo clippy`

---

## File Map

### New files

- `crates/app-live/src/commands/status/mod.rs`
  - Top-level `app-live status` command entrypoint and output rendering.
- `crates/app-live/src/commands/status/model.rs`
  - High-level readiness enums and operator-facing derived state structures.
- `crates/app-live/src/commands/status/evaluate.rs`
  - Pure derivation logic that maps config + durable/runtime state into readiness states and next actions.
- `crates/app-live/tests/status_command.rs`
  - End-to-end CLI-facing tests for `app-live status`.

### Modified files

- `crates/app-live/src/cli.rs`
  - Add the `status` subcommand and `StatusArgs`.
- `crates/app-live/src/main.rs`
  - Route the new subcommand to the status module.
- `crates/app-live/src/commands/mod.rs`
  - Export the new `status` command module.
- `crates/app-live/src/commands/targets/state.rs`
  - Reuse existing control-plane state helpers and, if needed, add one small helper for rollout/config source inspection rather than duplicating reads in the new module.
- `README.md`
  - Document `app-live status` as the new operator homepage for readiness.
- `docs/runbooks/operator-target-adoption.md`
  - Update the workflow to use `status` before/after adopt and rollback.
- `docs/runbooks/real-user-shadow-smoke.md`
  - Update the smoke workflow to use `status` for rollout/config-readiness interpretation.

### Existing files to study before editing

- `crates/app-live/src/commands/doctor/mod.rs`
- `crates/app-live/src/commands/doctor/target_source.rs`
- `crates/app-live/src/commands/bootstrap/flow.rs`
- `crates/app-live/src/commands/bootstrap/output.rs`
- `crates/app-live/src/commands/targets/status.rs`
- `crates/app-live/src/commands/targets/show_current.rs`
- `crates/app-live/src/commands/targets/state.rs`
- `crates/app-live/src/startup.rs`

---

### Task 1: Add the CLI Surface

**Files:**
- Modify: `crates/app-live/src/cli.rs`
- Modify: `crates/app-live/src/main.rs`
- Modify: `crates/app-live/src/commands/mod.rs`
- Test: `crates/app-live/tests/status_command.rs`

- [ ] **Step 1: Write the failing CLI smoke test**

Add a minimal failing test in `crates/app-live/tests/status_command.rs` that invokes:

```rust
Command::new(app_live_binary())
    .arg("status")
    .arg("--config")
    .arg(config_fixture("fixtures/app-live-paper.toml"))
    .output()
```

and asserts the process currently fails because the `status` subcommand does not exist yet.

- [ ] **Step 2: Run the test to verify it fails**

Run:

```bash
cargo test -p app-live --test status_command status_subcommand_is_exposed -- --exact --test-threads=1
```

Expected: FAIL because `app-live status` is not wired into the CLI yet.

- [ ] **Step 3: Add the minimal CLI plumbing**

Make the smallest code change needed:

- add `StatusArgs { config: PathBuf }` to `crates/app-live/src/cli.rs`
- add `Status(StatusArgs)` to `AppLiveCommand`
- export a placeholder `status` module from `crates/app-live/src/commands/mod.rs`
- route the new subcommand in `crates/app-live/src/main.rs`

Use a temporary placeholder implementation that prints a stub message or returns `Ok(())`; do not derive real readiness yet.

- [ ] **Step 4: Run the test to verify it passes**

Run:

```bash
cargo test -p app-live --test status_command status_subcommand_is_exposed -- --exact --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/cli.rs crates/app-live/src/main.rs crates/app-live/src/commands/mod.rs crates/app-live/tests/status_command.rs
git commit -m "feat: add app-live status command surface"
```

---

### Task 2: Define the Status Model

**Files:**
- Create: `crates/app-live/src/commands/status/model.rs`
- Modify: `crates/app-live/src/commands/status/mod.rs`
- Test: `crates/app-live/tests/status_command.rs`

- [ ] **Step 1: Write failing unit-style CLI expectations for the new readiness vocabulary**

Extend `crates/app-live/tests/status_command.rs` with failing assertions for at least these exact summary states:

- `paper-ready`
- `target-adoption-required`
- `restart-required`
- `smoke-rollout-required`
- `smoke-config-ready`
- `live-rollout-required`
- `live-config-ready`
- `blocked`

Start with one small paper-mode case and one live/smoke placeholder case so the vocabulary is locked in before implementation.

- [ ] **Step 2: Run the targeted tests to verify they fail**

Run:

```bash
cargo test -p app-live --test status_command -- --test-threads=1
```

Expected: FAIL because `status` still has only a placeholder implementation.

- [ ] **Step 3: Create the model layer**

Add `crates/app-live/src/commands/status/model.rs` with small focused types:

- `StatusReadiness`
- `StatusMode`
- `StatusSummary`
- `StatusDetails`
- `StatusAction`

Keep the model operator-facing. Do not import venue clients or `doctor` concepts into this layer.

- [ ] **Step 4: Run tests again to keep the failure at the behavior layer**

Run:

```bash
cargo test -p app-live --test status_command -- --test-threads=1
```

Expected: still FAIL, but now only because derivation/rendering are not implemented yet.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/status/model.rs crates/app-live/src/commands/status/mod.rs crates/app-live/tests/status_command.rs
git commit -m "feat: add status readiness model"
```

---

### Task 3: Implement Paper Readiness

**Files:**
- Modify: `crates/app-live/src/commands/status/evaluate.rs`
- Modify: `crates/app-live/src/commands/status/mod.rs`
- Test: `crates/app-live/tests/status_command.rs`

- [ ] **Step 1: Write the failing paper readiness tests**

Add tests that assert `app-live status` for a paper config prints:

- `Mode: paper`
- `Readiness: paper-ready`
- a next action that points to `app-live run --config ...`

Also add a failure case for invalid paper config that expects:

- `Readiness: blocked`

- [ ] **Step 2: Run the targeted paper tests to verify they fail**

Run:

```bash
cargo test -p app-live --test status_command paper -- --test-threads=1
```

Expected: FAIL.

- [ ] **Step 3: Implement minimal paper derivation**

Create `crates/app-live/src/commands/status/evaluate.rs` and implement only the logic needed for paper:

- load validated config
- classify mode as paper
- map valid paper config to `paper-ready`
- map invalid/unreadable config to `blocked`
- render `Summary / Key Details / Next Actions`

Do not touch live or smoke behavior yet.

- [ ] **Step 4: Run the targeted paper tests to verify they pass**

Run:

```bash
cargo test -p app-live --test status_command paper -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/status/evaluate.rs crates/app-live/src/commands/status/mod.rs crates/app-live/tests/status_command.rs
git commit -m "feat: derive paper status readiness"
```

---

### Task 4: Implement Adopted-Target Readiness For Live And Smoke

**Files:**
- Modify: `crates/app-live/src/commands/status/evaluate.rs`
- Modify: `crates/app-live/src/commands/targets/state.rs`
- Test: `crates/app-live/tests/status_command.rs`

- [ ] **Step 1: Write failing tests for adopted-target readiness**

Add cases for:

- adopted target source configured but missing `operator_target_revision`
  - expect `target-adoption-required`
- configured revision exists and active revision differs
  - expect `restart-required`
- configured revision exists and active revision unavailable
  - expect a non-blocked state that continues into rollout derivation
- broken durable provenance
  - expect `blocked`

Reuse the existing database fixture style from `doctor_command.rs` and `targets_*` tests rather than inventing a second test harness.

- [ ] **Step 2: Run those targeted tests to verify they fail**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test status_command adopted -- --test-threads=1
```

Expected: FAIL.

- [ ] **Step 3: Implement adopted-target state derivation**

In `evaluate.rs`, derive readiness from:

- validated config
- `load_target_control_plane_state(...)`
- configured vs active
- fail-closed durable provenance handling

Only add a helper to `crates/app-live/src/commands/targets/state.rs` if the current API is too narrow to avoid duplicated control-plane reads.

Hard rules:

- keep `operator_target_revision` as the only startup authority
- do not call `doctor`
- do not call venue probes

- [ ] **Step 4: Run the targeted tests to verify they pass**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test status_command adopted -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/status/evaluate.rs crates/app-live/src/commands/targets/state.rs crates/app-live/tests/status_command.rs
git commit -m "feat: derive adopted target status readiness"
```

---

### Task 5: Implement Rollout Readiness For Smoke And Live

**Files:**
- Modify: `crates/app-live/src/commands/status/evaluate.rs`
- Test: `crates/app-live/tests/status_command.rs`
- Reference: `crates/app-live/src/commands/bootstrap/flow.rs`

- [ ] **Step 1: Write failing rollout readiness tests**

Add tests that lock in these exact states:

- smoke + adopted target + rollout not enabled:
  - `smoke-rollout-required`
- smoke + adopted target + rollout enabled:
  - `smoke-config-ready`
- live + adopted target + rollout not enabled:
  - `live-rollout-required`
- live + adopted target + rollout enabled:
  - `live-config-ready`

Also assert appropriate next actions for each state.

- [ ] **Step 2: Run the targeted tests to verify they fail**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test status_command rollout -- --test-threads=1
```

Expected: FAIL.

- [ ] **Step 3: Implement rollout derivation**

Reuse the same family-set semantics already used by bootstrap:

- adopted family ids determine what rollout must cover
- smoke readiness should map to `smoke-rollout-required` vs `smoke-config-ready`
- live readiness should map to `live-rollout-required` vs `live-config-ready`

Do not duplicate bootstrap’s file rewrite logic; only reuse the same readiness concept.

- [ ] **Step 4: Run the targeted tests to verify they pass**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test status_command rollout -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/status/evaluate.rs crates/app-live/tests/status_command.rs
git commit -m "feat: derive rollout readiness status"
```

---

### Task 6: Reject Legacy Explicit Targets In The High-Level Flow

**Files:**
- Modify: `crates/app-live/src/commands/status/evaluate.rs`
- Test: `crates/app-live/tests/status_command.rs`

- [ ] **Step 1: Write the failing explicit-target legacy test**

Add a case that uses explicit `[[negrisk.targets]]` without adopted target source and assert:

- high-level flow is marked unsupported or blocked
- output clearly says the config is using legacy explicit targets
- next action points toward adopted-target migration or lower-level commands

- [ ] **Step 2: Run the targeted test to verify it fails**

Run:

```bash
cargo test -p app-live --test status_command explicit_target -- --test-threads=1
```

Expected: FAIL.

- [ ] **Step 3: Implement the legacy-path handling**

In `evaluate.rs`, make the high-level flow fail closed for explicit targets:

- do not render explicit-target startup as normal high-level readiness
- provide a clear operator-facing reason
- provide migration-oriented next actions

- [ ] **Step 4: Run the targeted test to verify it passes**

Run:

```bash
cargo test -p app-live --test status_command explicit_target -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/status/evaluate.rs crates/app-live/tests/status_command.rs
git commit -m "feat: mark explicit target flow unsupported in status"
```

---

### Task 7: Finish Rendering And Next Actions

**Files:**
- Modify: `crates/app-live/src/commands/status/mod.rs`
- Modify: `crates/app-live/tests/status_command.rs`

- [ ] **Step 1: Write failing output-shape tests**

Add CLI-facing tests that assert the final rendering order is:

1. Summary
2. Key Details
3. Next Actions

and that next actions are concrete, for example:

- `app-live targets adopt --config ...`
- `app-live doctor --config ...`
- `perform a controlled restart`
- `app-live run --config ...`

- [ ] **Step 2: Run the targeted rendering tests to verify they fail**

Run:

```bash
cargo test -p app-live --test status_command output -- --test-threads=1
```

Expected: FAIL.

- [ ] **Step 3: Implement final rendering**

Update `status/mod.rs` so the command:

- prints high-level summary first
- prints key detail lines next
- prints next actions last
- keeps wording concise and operator-facing

Do not dump raw internal structs.

- [ ] **Step 4: Run the targeted rendering tests to verify they pass**

Run:

```bash
cargo test -p app-live --test status_command output -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/status/mod.rs crates/app-live/tests/status_command.rs
git commit -m "feat: render high-level status output"
```

---

### Task 8: Update Docs To Make `status` The Operator Homepage

**Files:**
- Modify: `README.md`
- Modify: `docs/runbooks/operator-target-adoption.md`
- Modify: `docs/runbooks/real-user-shadow-smoke.md`

- [ ] **Step 1: Write the failing docs expectations**

Create a short checklist in the commit message or scratch notes before editing:

- README mentions `app-live status`
- adoption runbook uses `status` before/after adopt and rollback
- smoke runbook uses `status` to explain rollout/config-readiness before `doctor`

This step is about locking the docs scope before editing.

- [ ] **Step 2: Update the docs minimally**

Edit the docs so they reflect the new high-level flow:

- `status` is the operator homepage
- `doctor` remains the preflight gate
- `targets ...` remains the detailed control-plane surface

Do not rewrite unrelated project sections.

- [ ] **Step 3: Run doc hygiene checks**

Run:

```bash
git diff --check
```

Expected: PASS with no whitespace or conflict-marker issues.

- [ ] **Step 4: Commit**

```bash
git add README.md docs/runbooks/operator-target-adoption.md docs/runbooks/real-user-shadow-smoke.md
git commit -m "docs: add status readiness workflow"
```

---

### Task 9: Final Verification

**Files:**
- Verify all changes from previous tasks

- [ ] **Step 1: Run focused command tests**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test status_command -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 2: Run adjacent regression suites**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test doctor_command -- --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test run_command -- --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test targets_read_commands -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 3: Run formatting and lint**

Run:

```bash
cargo fmt --all --check
cargo clippy -p app-live -p config-schema --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 4: Commit the final verification state if needed**

If the last functional/doc commits already include all code changes, create only a small final commit if verification forced any fixes:

```bash
git add -A
git commit -m "fix: polish status readiness workflow"
```

If no changes were needed after verification, skip this commit.

