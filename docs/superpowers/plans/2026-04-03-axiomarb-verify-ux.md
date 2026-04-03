# AxiomArb Verify UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a high-level `app-live verify` command that validates the latest local run result for `paper`, `live`, and `real-user shadow smoke` without requiring SQL, journal-seq reasoning, or venue probes.

**Architecture:** Build `verify` as a new command family under `app-live` with its own operator-facing model, local evidence loader, and window-selection layer. Reuse existing `status` readiness derivation for control-plane context and existing `app-replay` library helpers for replay-visible smoke evidence, while keeping historical-range verdicts conservative unless the evidence window can be tied to the current config anchor.

**Tech Stack:** Rust, `clap`, `sqlx`, existing `config-schema` config model, existing `status` readiness model, existing `app-replay` library helpers, PostgreSQL-backed integration tests, `cargo test`, `cargo clippy`

---

## File Map

### New files

- `crates/app-live/src/commands/verify/mod.rs`
  - Top-level `app-live verify` entrypoint, report rendering, and next-action text.
- `crates/app-live/src/commands/verify/model.rs`
  - Operator-facing enums and structs for scenario, verdict, expectation profile, evidence summary, context summary, and final report.
- `crates/app-live/src/commands/verify/window.rs`
  - Range parsing and window classification (`latest` vs explicit historical range) including `--since` parsing and range conflict checks.
- `crates/app-live/src/commands/verify/context.rs`
  - Control-plane context loader that reuses the high-level `status` derivation and turns it into verify-friendly expectation context.
- `crates/app-live/src/commands/verify/evidence.rs`
  - Local-only evidence loading from persistence and `app-replay`, verdict derivation, and mode-specific replay weighting.
- `crates/app-live/tests/verify_command.rs`
  - CLI-facing verify tests covering output shape, paper/live/smoke verdicts, explicit-range behavior, and unsupported legacy flow.
- `crates/app-live/tests/support/verify_db.rs`
  - Dedicated Postgres helper for seeding execution attempts, artifacts, live submissions, journal rows, adopted targets, runtime progress, and reusable config-profile helpers for verify scenarios.

### Modified files

- `crates/app-live/Cargo.toml`
  - Add the `app-replay` dependency so `verify` can reuse existing local replay helpers instead of shelling out.
- `crates/app-live/src/cli.rs`
  - Add `VerifyArgs` and the fixed `--expect`/range CLI surface.
- `crates/app-live/src/main.rs`
  - Route the new `verify` subcommand.
- `crates/app-live/src/commands/mod.rs`
  - Export the new `verify` module.
- `crates/app-live/tests/support/mod.rs`
  - Export the new `verify_db` helper.
- `crates/persistence/src/models.rs`
  - Add minimal read models that expose attempt timestamps and/or journal range rows needed for verify window resolution.
- `crates/persistence/src/repos.rs`
  - Add focused read APIs for verify windows:
    - attempt lookup by `attempt_id`
    - recent / since-based attempt listing with timestamps
    - bounded journal range loading for `--to-seq`
- `README.md`
  - Document `app-live verify` in the high-level operator flow.
- `docs/runbooks/real-user-shadow-smoke.md`
  - Replace the current “manual SQL + replay” happy path with `app-live verify`.
- `docs/runbooks/operator-target-adoption.md`
  - Add the post-run verification step for adopted-target workflows.

### Existing files to study before editing

- `crates/app-live/src/commands/status/mod.rs`
- `crates/app-live/src/commands/status/evaluate.rs`
- `crates/app-live/src/commands/status/model.rs`
- `crates/app-live/src/commands/doctor/mod.rs`
- `crates/app-live/src/commands/bootstrap/flow.rs`
- `crates/app-live/src/commands/bootstrap/output.rs`
- `crates/app-live/src/commands/run.rs`
- `crates/app-live/src/commands/targets/state.rs`
- `crates/app-live/tests/status_command.rs`
- `crates/app-live/tests/doctor_command.rs`
- `crates/app-live/tests/targets_read_commands.rs`
- `crates/app-replay/src/lib.rs`
- `crates/app-replay/src/main.rs`
- `crates/persistence/src/repos.rs`
- `crates/persistence/src/models.rs`

---

### Task 1: Add the `verify` CLI Surface

**Files:**
- Modify: `crates/app-live/Cargo.toml`
- Modify: `crates/app-live/src/cli.rs`
- Modify: `crates/app-live/src/main.rs`
- Modify: `crates/app-live/src/commands/mod.rs`
- Create: `crates/app-live/src/commands/verify/mod.rs`
- Test: `crates/app-live/tests/verify_command.rs`

- [ ] **Step 1: Write the failing CLI exposure test**

Create `crates/app-live/tests/verify_command.rs` with a minimal failing test:

```rust
mod support;

use std::process::Command;

use support::cli;

#[test]
fn verify_subcommand_is_exposed() {
    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--help")
        .output()
        .expect("app-live verify --help should execute");

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("--config"), "{text}");
    assert!(text.contains("--expect"), "{text}");
    assert!(text.contains("--from-seq"), "{text}");
    assert!(text.contains("--to-seq"), "{text}");
    assert!(text.contains("--attempt-id"), "{text}");
    assert!(text.contains("--since"), "{text}");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run:

```bash
cargo test -p app-live --test verify_command verify_subcommand_is_exposed -- --exact --test-threads=1
```

Expected: FAIL because `verify` is not wired into the CLI yet.

- [ ] **Step 3: Add the minimal CLI plumbing**

Add the new surface without implementing behavior yet:

```rust
#[derive(clap::Args, Debug)]
pub struct VerifyArgs {
    #[arg(long)]
    pub config: PathBuf,

    #[arg(long)]
    pub expect: Option<String>,

    #[arg(long = "from-seq")]
    pub from_seq: Option<i64>,

    #[arg(long = "to-seq")]
    pub to_seq: Option<i64>,

    #[arg(long = "attempt-id")]
    pub attempt_id: Option<String>,

    #[arg(long)]
    pub since: Option<String>,
}
```

Also add:

- `AppLiveCommand::Verify(VerifyArgs)` in `crates/app-live/src/cli.rs`
- `pub mod verify;` in `crates/app-live/src/commands/mod.rs`
- `AppLiveCommand::Verify(args) => verify_execute(args)` in `crates/app-live/src/main.rs`
- a placeholder `execute(args: VerifyArgs) -> Result<(), Box<dyn Error>>` in `crates/app-live/src/commands/verify/mod.rs`
- `app-replay = { path = "../app-replay" }` in `crates/app-live/Cargo.toml`

- [ ] **Step 4: Run the test to verify it passes**

Run:

```bash
cargo test -p app-live --test verify_command verify_subcommand_is_exposed -- --exact --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/Cargo.toml crates/app-live/src/cli.rs crates/app-live/src/main.rs crates/app-live/src/commands/mod.rs crates/app-live/src/commands/verify/mod.rs crates/app-live/tests/verify_command.rs
git commit -m "feat: add app-live verify command surface"
```

---

### Task 2: Define the Verify Model and Report Shape

**Files:**
- Create: `crates/app-live/src/commands/verify/model.rs`
- Modify: `crates/app-live/src/commands/verify/mod.rs`
- Test: `crates/app-live/src/commands/verify/model.rs`
- Test: `crates/app-live/src/commands/verify/mod.rs`

- [ ] **Step 1: Write the failing model and render tests**

Add focused unit tests that lock the operator-facing vocabulary:

```rust
#[test]
fn verify_expectation_labels_use_operator_vocabulary() {
    assert_eq!(VerifyExpectation::Auto.label(), "auto");
    assert_eq!(VerifyExpectation::PaperNoLive.label(), "paper-no-live");
    assert_eq!(VerifyExpectation::SmokeShadowOnly.label(), "smoke-shadow-only");
    assert_eq!(
        VerifyExpectation::LiveConfigConsistent.label(),
        "live-config-consistent"
    );
}

#[test]
fn render_report_uses_expected_section_order() {
    let report = VerifyReport::fixture_for_render_test();
    let rendered = render_report(&report, Path::new("config/axiom-arb.local.toml"));
    assert!(rendered.find("Scenario").unwrap() < rendered.find("Verdict").unwrap());
    assert!(rendered.find("Verdict").unwrap() < rendered.find("Result Evidence").unwrap());
    assert!(rendered.find("Result Evidence").unwrap() < rendered.find("Control-Plane Context").unwrap());
    assert!(rendered.find("Control-Plane Context").unwrap() < rendered.find("Next Actions").unwrap());
}
```

- [ ] **Step 2: Run the targeted tests to verify they fail**

Run:

```bash
cargo test -p app-live verify:: -- --test-threads=1
```

Expected: FAIL because the verify model and renderer do not exist yet.

- [ ] **Step 3: Create the operator-facing model and render helper**

Add `crates/app-live/src/commands/verify/model.rs` with focused types:

```rust
pub enum VerifyScenario { Paper, Live, RealUserShadowSmoke }
pub enum VerifyVerdict { Pass, PassWithWarnings, Fail }
pub enum VerifyExpectation { Auto, PaperNoLive, SmokeShadowOnly, LiveConfigConsistent }

pub struct VerifyResultEvidence { /* attempts, artifacts, replay, side effects */ }
pub struct VerifyControlPlaneContext { /* mode, target_source, configured/active, restart_needed, rollout_state */ }
pub struct VerifyReport { /* scenario, verdict, evidence, context, next_actions */ }
```

Keep the renderer in `mod.rs` pure and string-based so later integration tests can assert on exact output.

- [ ] **Step 4: Run the targeted tests to verify they pass**

Run:

```bash
cargo test -p app-live verify:: -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/verify/model.rs crates/app-live/src/commands/verify/mod.rs
git commit -m "feat: add verify report model"
```

---

### Task 3: Add Window Selection and Historical-Range Guardrails

**Files:**
- Create: `crates/app-live/src/commands/verify/window.rs`
- Modify: `crates/app-live/src/commands/verify/mod.rs`
- Test: `crates/app-live/src/commands/verify/window.rs`

- [ ] **Step 1: Write the failing window-selection tests**

Add unit tests for the exact first-version rules:

```rust
#[test]
fn parses_since_shorthand_values() {
    assert_eq!(parse_since("10m").unwrap().num_minutes(), 10);
    assert_eq!(parse_since("2h").unwrap().num_hours(), 2);
}

#[test]
fn explicit_history_is_marked_historical() {
    let selection = VerifyWindowSelection::from_args(
        Some(100),
        Some(200),
        None,
        None,
        VerifyScenario::Live,
    )
    .unwrap();
    assert!(selection.is_historical_explicit());
}

#[test]
fn attempt_id_conflicts_with_seq_range() {
    let error = VerifyWindowSelection::from_args(
        Some(100),
        None,
        Some("attempt-1".to_owned()),
        None,
        VerifyScenario::Live,
    )
    .unwrap_err();
    assert!(error.to_string().contains("cannot be combined"));
}
```

- [ ] **Step 2: Run the targeted tests to verify they fail**

Run:

```bash
cargo test -p app-live verify::window -- --test-threads=1
```

Expected: FAIL because `window.rs` does not exist yet.

- [ ] **Step 3: Implement window parsing and classification**

Create `crates/app-live/src/commands/verify/window.rs` with:

```rust
pub enum VerifyWindowSelection {
    LatestForScenario,
    ExplicitAttemptId(String),
    ExplicitSeqRange { from_seq: i64, to_seq: Option<i64> },
    ExplicitSince(DateTime<Utc>),
}

impl VerifyWindowSelection {
    pub fn is_historical_explicit(&self) -> bool { /* true for all explicit overrides */ }
}
```

Implement a small suffix parser for `s`, `m`, `h`, and `d` so `--since 10m` works without dragging in a general-purpose human-time dependency.

- [ ] **Step 4: Run the targeted tests to verify they pass**

Run:

```bash
cargo test -p app-live verify::window -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/verify/window.rs crates/app-live/src/commands/verify/mod.rs
git commit -m "feat: add verify window selection"
```

---

### Task 4: Build Local Evidence and Control-Plane Context Support

**Files:**
- Create: `crates/app-live/src/commands/verify/context.rs`
- Create: `crates/app-live/src/commands/verify/evidence.rs`
- Create: `crates/app-live/tests/support/verify_db.rs`
- Modify: `crates/app-live/tests/support/mod.rs`
- Modify: `crates/persistence/src/models.rs`
- Modify: `crates/persistence/src/repos.rs`
- Test: `crates/app-live/tests/verify_command.rs`

- [ ] **Step 1: Write the failing foundation tests**

Add integration tests that lock the non-negotiable boundaries:

```rust
#[test]
fn verify_explicit_target_config_is_reported_as_legacy_unsupported() {
    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(cli::config_fixture("app-live-live.toml"))
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Verdict: FAIL"), "{text}");
    assert!(text.contains("legacy explicit targets"), "{text}");
}

#[test]
fn verify_historical_attempt_window_degrades_when_it_cannot_be_tied_to_current_config() {
    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(config_path)
        .arg("--attempt-id")
        .arg("attempt-old")
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Verdict: PASS WITH WARNINGS") || text.contains("Verdict: FAIL"), "{text}");
    assert!(text.contains("historical window is not provably tied to the current config anchor"), "{text}");
}
```

- [ ] **Step 2: Run the targeted tests to verify they fail**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test verify_command verify_explicit_target_config_is_reported_as_legacy_unsupported -- --exact --test-threads=1
```

Expected: FAIL because there is no verify context/evidence layer yet.

- [ ] **Step 3: Add the shared support layer**

Implement the smallest reusable foundation:

- `context.rs`
  - call the existing high-level `status::evaluate::evaluate`
  - convert `StatusSummary` into the fixed verify control-plane context
  - derive `auto` expectation from mode + readiness
  - own the single “can this evidence window be compared to the current config anchor?” decision so later live/history tasks do not duplicate that logic in render code or tests
- `verify_db.rs`
  - seed adopted targets, runtime progress, execution attempts, shadow artifacts, live artifacts, live submissions, and journal rows
  - add one shared helper module for config shapes used by multiple verify tests:
    - `live_ready_config()`
    - `live_ready_config_for(operator_target_revision)`
    - `smoke_rollout_required_config()`
- `repos.rs` / `models.rs`
  - add minimal read models for verify windows, for example:

```rust
pub struct ExecutionAttemptWithCreatedAtRow {
    pub attempt: ExecutionAttemptRow,
    pub created_at: DateTime<Utc>,
}
```

  - add focused readers like:
    - `ExecutionAttemptRepo::get_by_attempt_id`
    - `ExecutionAttemptRepo::list_created_since`
    - `ExecutionAttemptRepo::list_recent`
    - `JournalRepo::list_range`

Keep the persistence APIs generic. Do not add `verify`-named repo methods.

- [ ] **Step 4: Run the targeted tests to verify they pass**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test verify_command verify_explicit_target_config_is_reported_as_legacy_unsupported -- --exact --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/verify/context.rs crates/app-live/src/commands/verify/evidence.rs crates/app-live/tests/support/verify_db.rs crates/app-live/tests/support/mod.rs crates/persistence/src/models.rs crates/persistence/src/repos.rs crates/app-live/tests/verify_command.rs
git commit -m "feat: add verify local evidence foundation"
```

---

### Task 5: Implement Conservative Paper Verification

**Files:**
- Modify: `crates/app-live/src/commands/verify/mod.rs`
- Modify: `crates/app-live/src/commands/verify/context.rs`
- Modify: `crates/app-live/src/commands/verify/evidence.rs`
- Modify: `crates/app-live/tests/verify_command.rs`
- Modify: `crates/app-live/tests/support/verify_db.rs`

- [ ] **Step 1: Write the failing paper verify tests**

Add paper-mode cases that lock the conservative contract:

```rust
#[test]
fn verify_paper_passes_with_warnings_when_basic_run_evidence_is_incomplete_but_no_live_attempts_exist(
) {
    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(cli::config_fixture("app-live-paper.toml"))
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Scenario: paper"), "{text}");
    assert!(text.contains("Verdict: PASS WITH WARNINGS"), "{text}");
    assert!(text.contains("Expectation: paper-no-live"), "{text}");
}

#[test]
fn verify_paper_fails_when_live_attempts_are_observed() {
    verify_db.seed_live_attempt("attempt-live-1");

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(cli::config_fixture("app-live-paper.toml"))
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Verdict: FAIL"), "{text}");
    assert!(text.contains("forbidden live side effects"), "{text}");
}
```

- [ ] **Step 2: Run the targeted tests to verify they fail**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test verify_command paper -- --test-threads=1
```

Expected: FAIL because paper-specific verdict rules are not implemented yet.

- [ ] **Step 3: Implement the minimal paper verdict path**

Implement only the paper branch:

- `auto` resolves to `paper-no-live`
- no live attempts is the core invariant
- weak or missing paper evidence may still produce `PASS WITH WARNINGS`
- paper does **not** claim smoke-like replay certainty

Render at least:

```text
Scenario: paper
Verdict: PASS WITH WARNINGS
Result Evidence
...
Control-Plane Context
Mode: paper
...
Next Actions
...
```

- [ ] **Step 4: Run the targeted tests to verify they pass**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test verify_command paper -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/verify/mod.rs crates/app-live/src/commands/verify/context.rs crates/app-live/src/commands/verify/evidence.rs crates/app-live/tests/verify_command.rs crates/app-live/tests/support/verify_db.rs
git commit -m "feat: add conservative paper verify"
```

---

### Task 6: Implement Smoke Verification and Replay Weighting

**Files:**
- Modify: `crates/app-live/src/commands/verify/mod.rs`
- Modify: `crates/app-live/src/commands/verify/evidence.rs`
- Modify: `crates/app-live/tests/verify_command.rs`
- Modify: `crates/app-live/tests/support/verify_db.rs`

- [ ] **Step 1: Write the failing smoke verify tests**

Add smoke-mode cases for the three critical outcomes:

```rust
#[test]
fn verify_smoke_passes_when_shadow_only_evidence_is_complete() {
    verify_db.seed_shadow_attempt_with_artifacts("attempt-shadow-1");
    verify_db.seed_smoke_runtime_progress("targets-rev-9");

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(cli::config_fixture("app-live-ux-smoke.toml"))
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Scenario: real-user shadow smoke"), "{text}");
    assert!(text.contains("Verdict: PASS"), "{text}");
    assert!(text.contains("shadow attempts: 1"), "{text}");
}

#[test]
fn verify_smoke_requires_real_run_evidence_before_it_can_be_any_pass_variant() {
    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(cli::config_fixture("app-live-ux-smoke.toml"))
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Verdict: FAIL"), "{text}");
    assert!(text.contains("no credible run evidence exists"), "{text}");
}

#[test]
fn verify_smoke_preflight_only_run_can_warn_without_failing() {
    verify_db.seed_smoke_runtime_progress("targets-rev-9");
    verify_db.seed_non_working_smoke_run_window();

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(smoke_rollout_required_config())
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Verdict: PASS WITH WARNINGS"), "{text}");
    assert!(text.contains("rollout not ready"), "{text}");
}
```

- [ ] **Step 2: Run the targeted tests to verify they fail**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test verify_command smoke -- --test-threads=1
```

Expected: FAIL.

- [ ] **Step 3: Implement smoke evidence and mode-specific replay handling**

Use local-only helpers:

- execution attempts + shadow artifacts from persistence
- `app_replay::load_negrisk_shadow_attempt_artifacts`
- optional journal replay summary only as an additional strengthening signal

Lock in these rules:

- zero run evidence -> `FAIL`
- real run evidence + no live side effects + rollout not ready -> `PASS WITH WARNINGS`
- shadow evidence + consistent artifacts + no live side effects -> `PASS`
- replay is **strong** for smoke, not mandatory for every successful verdict

- [ ] **Step 4: Run the targeted tests to verify they pass**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test verify_command smoke -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/verify/mod.rs crates/app-live/src/commands/verify/evidence.rs crates/app-live/tests/verify_command.rs crates/app-live/tests/support/verify_db.rs
git commit -m "feat: add shadow smoke verify"
```

---

### Task 7: Implement Live Verification and Historical-Range Degradation

**Files:**
- Modify: `crates/app-live/src/commands/verify/mod.rs`
- Modify: `crates/app-live/src/commands/verify/context.rs`
- Modify: `crates/app-live/src/commands/verify/evidence.rs`
- Modify: `crates/app-live/src/commands/verify/window.rs`
- Modify: `crates/app-live/tests/verify_command.rs`
- Modify: `crates/app-live/tests/support/verify_db.rs`

- [ ] **Step 1: Write the failing live and historical-range tests**

Add at least these cases:

```rust
#[test]
fn verify_live_passes_when_local_results_match_current_config_and_control_plane() {
    verify_db.seed_live_attempt_with_artifacts("attempt-live-1");
    verify_db.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-9"));

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(live_ready_config())
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Scenario: live"), "{text}");
    assert!(text.contains("Verdict: PASS"), "{text}");
    assert!(text.contains("Expectation: live-config-consistent"), "{text}");
}

#[test]
fn verify_live_fails_when_results_conflict_with_current_mode_and_readiness() {
    verify_db.seed_shadow_attempt_with_artifacts("attempt-shadow-1");
    verify_db.seed_adopted_target_with_active_revision("targets-rev-9", Some("targets-rev-9"));

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(live_ready_config())
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(text.contains("Verdict: FAIL"), "{text}");
    assert!(text.contains("contradictory local outcomes"), "{text}");
}

#[test]
fn verify_explicit_historical_window_never_claims_current_config_consistency() {
    verify_db.seed_historical_live_attempt("attempt-old");
    verify_db.seed_newer_config_anchor("targets-rev-10");

    let output = Command::new(cli::app_live_binary())
        .arg("verify")
        .arg("--config")
        .arg(live_ready_config_for("targets-rev-10"))
        .arg("--attempt-id")
        .arg("attempt-old")
        .env("DATABASE_URL", verify_db.database_url())
        .output()
        .unwrap();

    let text = cli::combined(&output);
    assert!(!text.contains("Verdict: PASS\n"), "{text}");
    assert!(text.contains("historical window"), "{text}");
}
```

- [ ] **Step 2: Run the targeted tests to verify they fail**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test verify_command live -- --test-threads=1
```

Expected: FAIL.

- [ ] **Step 3: Implement live consistency verdicts and historical-range guardrails**

Implement:

- `live-config-consistent` as a conservative, local-only consistency check
- explicit historical range windows that degrade if they cannot be proven current
- `--expect ...` overrides using the fixed profile set
- `Next Actions` derived from `verdict + scenario + readiness context`

Keep these boundaries hard:

- no “trade success” certification
- no current-config consistency verdict for historical windows unless provably tied
- no legacy explicit-target high-level verify path

- [ ] **Step 4: Run the targeted tests to verify they pass**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test verify_command live -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/verify/mod.rs crates/app-live/src/commands/verify/context.rs crates/app-live/src/commands/verify/evidence.rs crates/app-live/src/commands/verify/window.rs crates/app-live/tests/verify_command.rs crates/app-live/tests/support/verify_db.rs
git commit -m "feat: add live and historical verify verdicts"
```

---

### Task 8: Document the High-Level Verify Workflow and Run Full Regression

**Files:**
- Modify: `README.md`
- Modify: `docs/runbooks/real-user-shadow-smoke.md`
- Modify: `docs/runbooks/operator-target-adoption.md`
- Test: `crates/app-live/tests/verify_command.rs`
- Test: `crates/app-live/tests/status_command.rs`
- Test: `crates/app-live/tests/doctor_command.rs`
- Test: `crates/app-live/tests/bootstrap_command.rs`

- [ ] **Step 1: Write the failing doc assertions in the implementation notes**

Before editing docs, write down the exact operator flow that must become true:

```text
bootstrap -> status -> doctor -> run -> verify
```

and ensure the docs changes explicitly replace the current manual smoke-verification path:

```text
app-live verify --config config/axiom-arb.local.toml
```

- [ ] **Step 2: Update the docs to match the new high-level flow**

At minimum:

- `README.md`
  - add `verify` to the operator command list
  - add at least one example per mode
- `docs/runbooks/real-user-shadow-smoke.md`
  - replace manual SQL/replay as the default happy path with `app-live verify`
  - keep manual SQL only as a deeper debugging fallback
- `docs/runbooks/operator-target-adoption.md`
  - add `verify` after controlled restart / post-run checks

- [ ] **Step 3: Run the targeted verify regression suite**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test verify_command -- --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test status_command -- --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test doctor_command -- --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test bootstrap_command -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 4: Run formatting and linting**

Run:

```bash
cargo fmt --all --check
cargo clippy -p app-live -p persistence --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add README.md docs/runbooks/real-user-shadow-smoke.md docs/runbooks/operator-target-adoption.md crates/app-live/tests/verify_command.rs
git commit -m "docs: add verify workflow"
```
