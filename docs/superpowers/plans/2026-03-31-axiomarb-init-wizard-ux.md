# AxiomArb Init Wizard UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn `app-live init` into an operator-facing interactive wizard that generates a valid local TOML, preserves safe existing state, and tells the operator exactly what to run next.

**Architecture:** Keep `app-live` as the only operator entrypoint. Replace the current `init --defaults --mode ...` template flow with a synchronous stdin/stdout wizard that builds a typed `config-schema::RawAxiomConfig`, preserves only safe config-carried startup authority, and prints explicit readiness guidance instead of inventing target anchors or rollout state.

**Tech Stack:** Rust, `clap`, `serde`/`toml`, `config-schema`, existing `app-live` command/test harnesses, standard stdin/stdout prompting.

---

## File Map

### Command surface and wizard internals
- Modify: `crates/app-live/src/cli.rs`
  - Replace the current template-oriented `InitArgs` surface with a wizard-oriented `--config` entry and any minimal explicit safety toggles needed for existing-file handling.
- Modify: `crates/app-live/src/commands/init.rs`
  - Keep this as the public `execute()` entrypoint and module root for the wizard.
- Create: `crates/app-live/src/commands/init/prompt.rs`
  - Small synchronous prompt abstraction for asking mode/auth/reuse questions and reprompting on invalid input.
- Create: `crates/app-live/src/commands/init/wizard.rs`
  - Own the mode-aware question flow, existing-config decisions, and collected answers.
- Create: `crates/app-live/src/commands/init/render.rs`
  - Build `RawAxiomConfig` values with default Polymarket source config, safe empty rollout, and preserved config-carried `operator_target_revision` only.
- Create: `crates/app-live/src/commands/init/summary.rs`
  - Render the `What Was Written` / `What To Run Next` terminal summary.

### Config schema support
- Modify: `crates/config-schema/src/lib.rs`
  - Re-export the raw TOML structs/enums the wizard needs to build typed configs without reaching into private modules.
- Modify: `crates/config-schema/tests/config_roundtrip.rs`
  - Cover the new wizard-generated shapes: safe empty rollout, adopted target source without a fake revision, and live/smoke account/relayer config.

### Integration and regression coverage
- Modify: `crates/app-live/tests/init_command.rs`
  - Replace the current `--defaults --mode` template tests with stdin-driven wizard tests.
- Modify: `crates/app-live/tests/doctor_command.rs`
  - Verify wizard-generated configs remain consumable by `doctor`, especially the empty-rollout / missing-target-anchor guidance paths.
- Modify: `crates/app-live/tests/run_command.rs`
  - Keep paper-mode startup working from the new wizard output.

### Operator docs and examples
- Modify: `config/axiom-arb.example.toml`
  - Match the new wizard baseline: adopted target source without a fake `operator_target_revision`, safe empty rollout, and long-lived credentials only.
- Modify: `README.md`
  - Replace old `init --defaults --mode ...` guidance with the interactive `init -> targets adopt -> doctor -> run` flow.
- Modify: `docs/runbooks/bootstrap-and-ramp.md`
  - Update bootstrap instructions to the new wizard entry and empty-rollout semantics.
- Modify: `docs/runbooks/real-user-shadow-smoke.md`
  - Update smoke setup to use the wizard and remove legacy low-level init flags.

## Implementation Notes

- Do **not** add a second control-plane write path. `init` may preserve an `operator_target_revision` already present in the config file being updated, but it must never copy a new anchor from durable state into the config.
- Do **not** make `init` depend on `DATABASE_URL` in the first implementation. The wizard can report only config-carried target readiness and next steps; leave DB-backed informational summaries for a future pass.
- Do **not** overhaul `doctor` in this plan. Only update tests and operator messaging so wizard outputs remain compatible with the existing doctor contract.
- Prefer standard-library prompting over a new interactive dependency. Keep the first version deterministic and easy to test by piping stdin to the compiled binary.

### Task 1: Replace the template CLI with an interactive paper-mode wizard

**Files:**
- Modify: `crates/app-live/src/cli.rs`
- Modify: `crates/app-live/src/commands/init.rs`
- Create: `crates/app-live/src/commands/init/prompt.rs`
- Create: `crates/app-live/src/commands/init/wizard.rs`
- Modify: `crates/app-live/tests/init_command.rs`

- [ ] **Step 1: Write the failing integration test for the new interactive paper flow**

```rust
#[test]
fn init_interactive_paper_writes_minimal_config_and_next_steps() {
    let temp = tempfile::NamedTempFile::new().expect("temp file");
    let mut child = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("app-live init should spawn");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"paper\n")
        .expect("wizard answers should write");

    let output = child.wait_with_output().expect("output");
    assert!(output.status.success(), "{}", combined(&output));

    let text = fs::read_to_string(temp.path()).expect("generated config should exist");
    assert!(text.contains("[runtime]"));
    assert!(text.contains("mode = \"paper\""));
    assert!(combined(&output).contains("What Was Written"));
    assert!(combined(&output).contains("What To Run Next"));
}
```

- [ ] **Step 2: Run the new paper test to verify the old template flow fails**

Run: `cargo test -p app-live --test init_command init_interactive_paper_writes_minimal_config_and_next_steps -- --exact`
Expected: FAIL because `init` still requires `--defaults --mode ...` and does not read stdin or print a summary.

- [ ] **Step 3: Replace `InitArgs` with the wizard-oriented CLI surface**

Update `crates/app-live/src/cli.rs` so `InitArgs` only exposes the config path plus any minimal explicit safety knob needed for existing-file replacement. Remove user-facing `--defaults`, `--mode`, and `--real-user-shadow-smoke` from the primary operator entry.

```rust
#[derive(clap::Args, Debug)]
pub struct InitArgs {
    #[arg(long)]
    pub config: PathBuf,
}
```

- [ ] **Step 4: Add the synchronous prompt abstraction and wizard entrypoint**

Create `crates/app-live/src/commands/init/prompt.rs` with a tiny reusable interface for `select_one`, `confirm`, and `ask_nonempty`, then create `crates/app-live/src/commands/init/wizard.rs` with a paper-only first pass.

```rust
pub trait PromptIo {
    fn read_line(&mut self) -> Result<String, InitError>;
    fn println(&mut self, line: &str) -> Result<(), InitError>;
}

pub enum WizardMode {
    Paper,
    Live,
    RealUserShadowSmoke,
}
```

- [ ] **Step 5: Wire `init::execute()` to the new paper-mode wizard path**

Keep `crates/app-live/src/commands/init.rs` as the command root. It should construct a terminal prompt session, run the wizard, and write the rendered config to disk.

- [ ] **Step 6: Re-run the paper-mode init test**

Run: `cargo test -p app-live --test init_command init_interactive_paper_writes_minimal_config_and_next_steps -- --exact`
Expected: PASS with a minimal paper TOML and end-of-flow summary output.

- [ ] **Step 7: Commit the paper wizard foundation**

```bash
git add crates/app-live/src/cli.rs \
  crates/app-live/src/commands/init.rs \
  crates/app-live/src/commands/init/prompt.rs \
  crates/app-live/src/commands/init/wizard.rs \
  crates/app-live/tests/init_command.rs
git commit -m "feat: add interactive init paper wizard"
```

### Task 2: Generate live and smoke configs from long-lived credentials only

**Files:**
- Modify: `crates/config-schema/src/lib.rs`
- Create: `crates/app-live/src/commands/init/render.rs`
- Modify: `crates/app-live/src/commands/init/wizard.rs`
- Modify: `crates/app-live/tests/init_command.rs`
- Modify: `crates/config-schema/tests/config_roundtrip.rs`

- [ ] **Step 1: Write the failing live and smoke wizard tests**

Add two integration tests to `crates/app-live/tests/init_command.rs`.

```rust
#[test]
fn init_interactive_live_writes_account_relayer_target_source_and_safe_empty_rollout() {
    // Answers: live, replace, address, blank funder, relayer_api_key, relayer key, relayer address
    // Assert no timestamp/signature fields, target_source = adopted, and rollout lists are empty.
}

#[test]
fn init_interactive_smoke_sets_live_mode_plus_shadow_guard() {
    // Answers: smoke, replace, account credentials, relayer credentials.
    // Assert mode = "live" and real_user_shadow_smoke = true.
}
```

- [ ] **Step 2: Run the new live/smoke tests to verify they fail**

Run: `cargo test -p app-live --test init_command init_interactive_live_writes_account_relayer_target_source_and_safe_empty_rollout init_interactive_smoke_sets_live_mode_plus_shadow_guard -- --exact`
Expected: FAIL because the current wizard does not collect long-lived account/relayer credentials or emit safe empty rollout sections.

- [ ] **Step 3: Export the raw config types needed by the wizard renderer**

Update `crates/config-schema/src/lib.rs` to re-export the raw TOML structs/enums the wizard needs to build a `RawAxiomConfig` without duplicating schema types in `app-live`.

```rust
pub use raw::{
    NegRiskRolloutToml, NegRiskToml, PolymarketToml, PolymarketRelayerAuthToml,
    PolymarketSourceToml, RelayerAuthKindToml, RuntimeToml,
};
```

- [ ] **Step 4: Implement typed config rendering for live and smoke**

Create `crates/app-live/src/commands/init/render.rs` with helpers that build a `RawAxiomConfig` from wizard answers, using default Polymarket endpoints and cadence plus safe empty rollout.

```rust
fn default_source() -> PolymarketSourceToml {
    PolymarketSourceToml {
        clob_host: "https://clob.polymarket.com".into(),
        data_api_host: "https://data-api.polymarket.com".into(),
        relayer_host: "https://relayer-v2.polymarket.com".into(),
        market_ws_url: "wss://ws-subscriptions-clob.polymarket.com/ws/market".into(),
        user_ws_url: "wss://ws-subscriptions-clob.polymarket.com/ws/user".into(),
        heartbeat_interval_seconds: 15,
        relayer_poll_interval_seconds: 5,
        metadata_refresh_interval_seconds: 60,
    }
}
```

- [ ] **Step 5: Extend the wizard question flow for `live` and `real-user shadow smoke`**

In `crates/app-live/src/commands/init/wizard.rs`, add mode-aware question branches that collect only:
- account address
- optional funder address
- relayer auth type
- long-lived relayer credentials

Do not prompt for timestamps, signatures, URLs, or raw target members.

- [ ] **Step 6: Add config-schema roundtrip coverage for the wizard shapes**

Expand `crates/config-schema/tests/config_roundtrip.rs` with at least one live/adopted skeleton case.

```rust
#[test]
fn raw_config_round_trips_safe_empty_rollout_and_adopted_target_source() {
    let raw = load_raw_config_from_str(r#"
[runtime]
mode = "live"

[negrisk.target_source]
source = "adopted"

[negrisk.rollout]
approved_families = []
ready_families = []
"#).unwrap();

    let text = render_raw_config_to_string(&raw).unwrap();
    assert!(text.contains("approved_families = []"));
    assert!(text.contains("ready_families = []"));
}
```

- [ ] **Step 7: Re-run the live/smoke init tests and config roundtrip test**

Run:
- `cargo test -p app-live --test init_command init_interactive_live_writes_account_relayer_target_source_and_safe_empty_rollout -- --exact`
- `cargo test -p app-live --test init_command init_interactive_smoke_sets_live_mode_plus_shadow_guard -- --exact`
- `cargo test -p config-schema --test config_roundtrip raw_config_round_trips_safe_empty_rollout_and_adopted_target_source -- --exact`

Expected: PASS with no transient auth fields and no fake `operator_target_revision`.

- [ ] **Step 8: Commit the live/smoke rendering work**

```bash
git add crates/config-schema/src/lib.rs \
  crates/config-schema/tests/config_roundtrip.rs \
  crates/app-live/src/commands/init/render.rs \
  crates/app-live/src/commands/init/wizard.rs \
  crates/app-live/tests/init_command.rs
git commit -m "feat: generate live and smoke init configs"
```

### Task 3: Preserve safe existing config state without inventing startup authority

**Files:**
- Modify: `crates/app-live/src/commands/init/wizard.rs`
- Modify: `crates/app-live/src/commands/init/render.rs`
- Modify: `crates/app-live/tests/init_command.rs`
- Modify: `crates/config-schema/tests/config_roundtrip.rs`

- [ ] **Step 1: Write failing tests for preserve-vs-replace behavior**

Add integration coverage for existing config reuse.

```rust
#[test]
fn init_preserve_updates_credentials_but_keeps_config_carried_operator_target_revision_and_rollout() {
    // Seed an existing config with operator_target_revision = "targets-rev-9"
    // and non-empty rollout; choose preserve/update in the wizard.
    // Assert the revision and rollout survive the rewrite.
}

#[test]
fn init_replace_discards_existing_target_anchor_and_resets_rollout_to_safe_empty_lists() {
    // Seed an existing config with operator_target_revision and non-empty rollout;
    // choose replace in the wizard.
    // Assert operator_target_revision is absent and rollout lists are empty.
}
```

- [ ] **Step 2: Run the preserve/replace tests to verify they fail**

Run: `cargo test -p app-live --test init_command init_preserve_updates_credentials_but_keeps_config_carried_operator_target_revision_and_rollout init_replace_discards_existing_target_anchor_and_resets_rollout_to_safe_empty_lists -- --exact`
Expected: FAIL because the current wizard does not load, patch, or safely replace existing configs.

- [ ] **Step 3: Implement existing-config loading and preserve/replace decisions**

In `crates/app-live/src/commands/init/wizard.rs`, detect whether `--config` already exists. Prompt for:
- `preserve` (default) to patch missing/updated operator inputs
- `replace` to generate a fresh skeleton

Use `config_schema::load_raw_config_from_path()` only for config-file reuse; do not inspect the database here.

- [ ] **Step 4: Implement safe reuse rules in the renderer**

In `crates/app-live/src/commands/init/render.rs`, preserve only low-risk reusable sections and fields:
- current `operator_target_revision` if present in the config file
- current rollout section if present and preserve mode is chosen
- existing account/relayer fields unless the wizard explicitly replaces them

Never synthesize a new target anchor from external or durable state.

- [ ] **Step 5: Add a config roundtrip test for preserved `operator_target_revision`**

Extend `crates/config-schema/tests/config_roundtrip.rs` so preserved config-carried anchors remain serializable after wizard rewrites.

```rust
#[test]
fn raw_config_round_trips_preserved_operator_target_revision_and_rollout() {
    // Load a live config with operator_target_revision + rollout,
    // render it, and assert both survive.
}
```

- [ ] **Step 6: Re-run the preserve/replace tests and new roundtrip coverage**

Run:
- `cargo test -p app-live --test init_command init_preserve_updates_credentials_but_keeps_config_carried_operator_target_revision_and_rollout -- --exact`
- `cargo test -p app-live --test init_command init_replace_discards_existing_target_anchor_and_resets_rollout_to_safe_empty_lists -- --exact`
- `cargo test -p config-schema --test config_roundtrip raw_config_round_trips_preserved_operator_target_revision_and_rollout -- --exact`

Expected: PASS with no new startup authority path introduced.

- [ ] **Step 7: Commit the safe-reuse behavior**

```bash
git add crates/app-live/src/commands/init/wizard.rs \
  crates/app-live/src/commands/init/render.rs \
  crates/app-live/tests/init_command.rs \
  crates/config-schema/tests/config_roundtrip.rs
git commit -m "feat: preserve safe init config state"
```

### Task 4: Print readiness-aware summaries and explicit next-step guidance

**Files:**
- Create: `crates/app-live/src/commands/init/summary.rs`
- Modify: `crates/app-live/src/commands/init.rs`
- Modify: `crates/app-live/src/commands/init/wizard.rs`
- Modify: `crates/app-live/tests/init_command.rs`
- Modify: `crates/app-live/tests/doctor_command.rs`

- [ ] **Step 1: Write failing tests for end-of-wizard summary output**

Add integration tests that assert the terminal summary is explicit about readiness and next steps.

```rust
#[test]
fn init_without_operator_target_revision_points_operator_to_candidates_then_adopt() {
    // Assert summary contains:
    // - `targets candidates`
    // - `targets adopt`
    // - `doctor`
    // - `run`
}

#[test]
fn init_with_empty_rollout_warns_that_negrisk_work_remains_inactive() {
    // Assert summary says rollout is still empty and neg-risk work will remain inactive.
}

#[test]
fn init_paper_summary_only_points_to_doctor_then_run() {
    // Assert no target/adoption guidance is printed for paper mode.
}
```

- [ ] **Step 2: Run the summary tests to verify they fail**

Run: `cargo test -p app-live --test init_command init_without_operator_target_revision_points_operator_to_candidates_then_adopt init_with_empty_rollout_warns_that_negrisk_work_remains_inactive init_paper_summary_only_points_to_doctor_then_run -- --exact`
Expected: FAIL because `init` still writes files without explicit readiness summaries.

- [ ] **Step 3: Implement the summary renderer**

Create `crates/app-live/src/commands/init/summary.rs` with a small view model that prints two sections:
- `What Was Written`
- `What To Run Next`

```rust
pub struct InitSummary<'a> {
    pub mode: WizardMode,
    pub config_path: &'a Path,
    pub configured_operator_target_revision: Option<&'a str>,
    pub rollout_is_empty: bool,
}
```

- [ ] **Step 4: Wire summary generation into the wizard result**

After writing the config file in `crates/app-live/src/commands/init.rs`, print the summary and the exact next commands. The live/smoke path must distinguish:
- anchor present vs missing
- rollout non-empty vs safe empty

- [ ] **Step 5: Update doctor regression coverage to accept wizard-generated safe-empty rollout configs**

Add or update a `crates/app-live/tests/doctor_command.rs` case that proves a wizard-generated live config without an `operator_target_revision` still fails with a clear `TargetSourceError`, while paper-mode configs keep returning `SKIP` for live-only checks.

- [ ] **Step 6: Re-run the summary tests and the relevant doctor regression**

Run:
- `cargo test -p app-live --test init_command init_without_operator_target_revision_points_operator_to_candidates_then_adopt -- --exact`
- `cargo test -p app-live --test init_command init_with_empty_rollout_warns_that_negrisk_work_remains_inactive -- --exact`
- `cargo test -p app-live --test init_command init_paper_summary_only_points_to_doctor_then_run -- --exact`
- `cargo test -p app-live --test doctor_command -- --test-threads=1`

Expected: PASS with stable summary wording and unchanged doctor semantics.

- [ ] **Step 7: Commit the summary and readiness guidance**

```bash
git add crates/app-live/src/commands/init/summary.rs \
  crates/app-live/src/commands/init.rs \
  crates/app-live/src/commands/init/wizard.rs \
  crates/app-live/tests/init_command.rs \
  crates/app-live/tests/doctor_command.rs
git commit -m "feat: add init readiness summaries"
```

### Task 5: Update example config and operator docs to the wizard workflow

**Files:**
- Modify: `config/axiom-arb.example.toml`
- Modify: `README.md`
- Modify: `docs/runbooks/bootstrap-and-ramp.md`
- Modify: `docs/runbooks/real-user-shadow-smoke.md`

- [ ] **Step 1: Write down the doc deltas before editing**

Capture the old command strings that must disappear:
- `app-live init --config ... --defaults --mode live`
- `--real-user-shadow-smoke`
- placeholder `operator_target_revision = "YOUR_OPERATOR_TARGET_REVISION"` in the example config
- placeholder rollout family ids in the example config

- [ ] **Step 2: Update the example config to match the wizard baseline**

Make `config/axiom-arb.example.toml` reflect the new generated default for the adopted path:
- keep long-lived account/relayer credentials
- keep `target_source = "adopted"`
- remove the fake `operator_target_revision` placeholder line
- replace placeholder rollout ids with safe empty lists

- [ ] **Step 3: Update README to the new init flow**

Rewrite the operator startup section to use:
- `app-live init --config ...`
- `app-live targets candidates`
- `app-live targets adopt`
- `app-live doctor`
- `app-live run`

Explicitly explain that `init` now asks questions interactively and that missing target anchors or rollout readiness are surfaced as next steps.

- [ ] **Step 4: Update both runbooks to the wizard flow**

Edit:
- `docs/runbooks/bootstrap-and-ramp.md`
- `docs/runbooks/real-user-shadow-smoke.md`

Remove legacy `--defaults --mode ... --real-user-shadow-smoke` guidance and replace it with the wizard-driven flow, including the empty-rollout safety note.

- [ ] **Step 5: Verify the docs and example config stay in sync with the CLI**

Run:
- `rg -n "--defaults|--mode live|--real-user-shadow-smoke" README.md docs/runbooks config/axiom-arb.example.toml`
- `git diff --check`

Expected:
- `rg` only matches historical/spec docs, not operator-facing README/runbooks/example config
- `git diff --check` reports no whitespace or conflict-marker issues

- [ ] **Step 6: Commit the operator-facing docs refresh**

```bash
git add config/axiom-arb.example.toml README.md \
  docs/runbooks/bootstrap-and-ramp.md \
  docs/runbooks/real-user-shadow-smoke.md
git commit -m "docs: update init wizard operator flow"
```

### Task 6: Run focused verification before execution handoff

**Files:**
- Verify only

- [ ] **Step 1: Run formatter and lints for the touched crates**

Run:
- `cargo fmt --all --check`
- `cargo clippy -p app-live -p config-schema --all-targets -- -D warnings`

Expected: PASS.

- [ ] **Step 2: Run the focused init/config regression suite**

Run:
- `cargo test -p app-live --test init_command -- --test-threads=1`
- `cargo test -p app-live --test doctor_command -- --test-threads=1`
- `cargo test -p app-live --test run_command -- --test-threads=1`
- `cargo test -p config-schema --test config_roundtrip -- --test-threads=1`

Expected: PASS.

- [ ] **Step 3: If local Postgres is available, run the app-live smoke of the config path**

Run:
- `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test run_command -- --test-threads=1`

Expected: PASS with the paper-mode startup path still working from the unified TOML.

- [ ] **Step 4: Record any residual gaps before handoff**

If anything remains intentionally out of scope, note it explicitly in the handoff:
- no DB-backed `init` target-state inspection yet
- no doctor connectivity overhaul yet
- no automatic adoption or rollout inference

- [ ] **Step 5: Prepare execution handoff**

At handoff time, point implementers to:
- this plan document
- the spec: `docs/superpowers/specs/2026-03-31-axiomarb-init-wizard-ux-design.md`
- `@superpowers:subagent-driven-development` or `@superpowers:executing-plans`
