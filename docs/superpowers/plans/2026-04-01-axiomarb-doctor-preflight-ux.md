# Doctor Preflight UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Upgrade `app-live doctor` from a light startup checker into a sectioned operator preflight that performs real venue-facing readiness probes for `live` and `real-user shadow smoke`.

**Architecture:** Split the current single-file `doctor` command into a small report/orchestration module plus focused helper modules for credentials, connectivity, target-source state, and runtime-safety checks. Keep `doctor` as the only public preflight entrypoint, preserve existing startup-target authority (`operator_target_revision` or explicit targets), and gate all real venue probes behind the existing mode matrix so `paper` remains local and explicit.

**Tech Stack:** Rust, `clap`, `tokio`, `sqlx`, `reqwest`, `serde`, existing `config-schema`, `persistence`, and `venue-polymarket` clients.

---

## File Map

### Existing Files To Modify

- `crates/app-live/src/commands/doctor.rs`
  - Replace the monolithic implementation with a directory module entrypoint or move its contents into `doctor/mod.rs`.
- `crates/app-live/src/commands/mod.rs`
  - Keep the `doctor` module export stable if the file moves from `doctor.rs` to `doctor/mod.rs`.
- `crates/app-live/tests/doctor_command.rs`
  - Expand integration coverage for sectioned output, mode-scoped `SKIP`, target-source branches, and next-action summaries.
- `README.md`
  - Refresh operator-facing `doctor` expectations if command behavior/output materially changes.
- `docs/runbooks/real-user-shadow-smoke.md`
  - Update the runbook to reference the stronger `doctor` preflight before `run`.

### New Files To Create

- `crates/app-live/src/commands/doctor/mod.rs`
  - Command entrypoint, section orchestration, overall result selection, and top-level error mapping.
- `crates/app-live/src/commands/doctor/report.rs`
  - Section/check/result types plus stable text rendering for `Config`, `Credentials`, `Connectivity`, `Target Source`, and `Runtime Safety`.
- `crates/app-live/src/commands/doctor/credentials.rs`
  - Long-lived credential derivation and relayer-auth-shape probes.
- `crates/app-live/src/commands/doctor/connectivity.rs`
  - REST, market ws, user ws, heartbeat, and relayer reachability probes plus probe backend abstraction.
- `crates/app-live/src/commands/doctor/target_source.rs`
  - Startup target resolution and configured-vs-active / explicit-target reporting logic.
- `crates/app-live/src/commands/doctor/runtime_safety.rs`
  - Mode-consistency checks and smoke-safe startup assertions.

### Files Expected To Stay Unchanged

- `crates/app-live/src/commands/run.rs`
  - `doctor` must not change runtime mutation behavior.
- `crates/app-live/src/commands/targets/*.rs`
  - `doctor` must not adopt, roll back, or rewrite config.
- `crates/app-live/src/startup.rs`
  - Startup resolution contract should remain unchanged; `doctor` consumes it as-is.

## Task 1: Split `doctor` Into A Sectioned Report Skeleton

**Files:**
- Create: `crates/app-live/src/commands/doctor/mod.rs`
- Create: `crates/app-live/src/commands/doctor/report.rs`
- Modify: `crates/app-live/src/commands/mod.rs`
- Modify: `crates/app-live/tests/doctor_command.rs`

- [ ] **Step 1: Write the failing integration test for sectioned output**

```rust
#[test]
fn doctor_paper_mode_renders_sectioned_report_and_overall_summary() {
    let output = Command::new(app_live_binary())
        .arg("doctor")
        .arg("--config")
        .arg(config_fixture("fixtures/app-live-paper.toml"))
        .output()
        .expect("app-live doctor should execute");

    let combined = combined(&output);
    assert!(combined.contains("Config: PASS"), "{combined}");
    assert!(combined.contains("Connectivity: PASS WITH SKIPS"), "{combined}");
    assert!(combined.contains("Overall: PASS WITH SKIPS"), "{combined}");
}
```

- [ ] **Step 2: Run the failing test**

Run: `cargo test -p app-live --test doctor_command doctor_paper_mode_renders_sectioned_report_and_overall_summary -- --exact`

Expected: FAIL because current `doctor` prints flat `[OK]/[SKIP]` lines without section summaries or overall result.

- [ ] **Step 3: Implement the report model and renderer**

```rust
pub enum DoctorOverallResult {
    Pass,
    Fail,
    PassWithSkips,
}

pub struct DoctorSectionReport {
    pub title: &'static str,
    pub checks: Vec<DoctorCheckReport>,
}

pub struct DoctorCheckReport {
    pub status: CheckStatus,
    pub label: String,
    pub detail: Option<String>,
}
```

Implementation notes:
- Move `execute` into `doctor/mod.rs`.
- Keep the public callsite unchanged: `use app_live::commands::doctor::execute`.
- Make `report.rs` responsible only for rendering and overall aggregation; do not mix probe logic into it.
- Preserve the current behavior for actual checks for now; just render them through the new model.

- [ ] **Step 4: Re-run the targeted test**

Run: `cargo test -p app-live --test doctor_command doctor_paper_mode_renders_sectioned_report_and_overall_summary -- --exact`

Expected: PASS

- [ ] **Step 5: Run the full doctor command test file**

Run: `cargo test -p app-live --test doctor_command -- --test-threads=1`

Expected: PASS for existing tests except the ones that intentionally still cover old missing functionality.

- [ ] **Step 6: Commit**

```bash
git add crates/app-live/src/commands/mod.rs \
        crates/app-live/src/commands/doctor/mod.rs \
        crates/app-live/src/commands/doctor/report.rs \
        crates/app-live/tests/doctor_command.rs
git commit -m "refactor: add sectioned doctor reporting"
```

## Task 2: Add Credential, Target Source, And Runtime Safety Sections

**Files:**
- Create: `crates/app-live/src/commands/doctor/credentials.rs`
- Create: `crates/app-live/src/commands/doctor/target_source.rs`
- Create: `crates/app-live/src/commands/doctor/runtime_safety.rs`
- Modify: `crates/app-live/src/commands/doctor/mod.rs`
- Modify: `crates/app-live/tests/doctor_command.rs`

- [ ] **Step 1: Write failing tests for mode matrix and explicit-target handling**

```rust
#[tokio::test]
async fn doctor_live_mode_keeps_explicit_targets_and_skips_control_plane_checks() {
    let database = TestDatabase::new().await;
    let output = Command::new(app_live_binary())
        .arg("doctor")
        .arg("--config")
        .arg(config_fixture("fixtures/app-live-live.toml"))
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live doctor should execute");

    let combined = combined(&output);
    assert!(combined.contains("Target Source: PASS"), "{combined}");
    assert!(combined.contains("[SKIP] control-plane checks not required for explicit targets"), "{combined}");
}
```

Add companion failures for:
- adopted target source missing `operator_target_revision`
- smoke mode reporting `Runtime Safety: PASS`
- paper mode reporting `Credentials: PASS WITH SKIPS`

- [ ] **Step 2: Run the failing doctor tests**

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test doctor_command -- --test-threads=1`

Expected: FAIL because the new section/matrix expectations are not implemented yet.

- [ ] **Step 3: Implement credential, target-source, and runtime-safety helpers**

```rust
pub fn run_credentials_section(config: &AppLiveConfigView<'_>) -> DoctorSectionReport
pub async fn run_target_source_section(pool: &PgPool, config: &AppLiveConfigView<'_>, config_path: &Path) -> Result<DoctorSectionReport, DoctorFailure>
pub fn run_runtime_safety_section(config: &AppLiveConfigView<'_>, smoke: Option<&RealUserShadowSmokeConfig>) -> DoctorSectionReport
```

Implementation notes:
- `credentials.rs` should validate the long-lived account/replayer shapes by calling existing conversion paths (`LocalSignerConfig::try_from`, `load_real_user_shadow_smoke_config`) without mutating config.
- `target_source.rs` must preserve both current startup-input branches:
  - adopted source: report configured/active/restart-needed
  - explicit targets: report resolution success and `SKIP` control-plane-specific checks
- `runtime_safety.rs` should only claim what `doctor` can prove:
  - smoke-safe startup config is valid
  - shadow-only path will be requested on startup
  - not “guard already observed in effect”

- [ ] **Step 4: Re-run the doctor tests**

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test doctor_command -- --test-threads=1`

Expected: PASS for section/mode-matrix regressions.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/doctor/mod.rs \
        crates/app-live/src/commands/doctor/credentials.rs \
        crates/app-live/src/commands/doctor/target_source.rs \
        crates/app-live/src/commands/doctor/runtime_safety.rs \
        crates/app-live/tests/doctor_command.rs
git commit -m "feat: add doctor mode matrix and target checks"
```

## Task 3: Add Connectivity Probes Through A Reusable Probe Backend

**Files:**
- Create: `crates/app-live/src/commands/doctor/connectivity.rs`
- Modify: `crates/app-live/src/commands/doctor/mod.rs`
- Modify: `crates/app-live/tests/doctor_command.rs`

- [ ] **Step 1: Write failing unit tests for probe orchestration**

```rust
#[tokio::test]
async fn live_connectivity_probe_uses_resolved_startup_targets_for_ws_scope() {
    let backend = FakeProbeBackend::success();
    let resolved = sample_resolved_targets_with_one_member();

    let report = run_connectivity_section(&backend, &sample_live_config(), &resolved)
        .await
        .expect("section should run");

    assert!(backend.market_asset_ids_seen().contains(&"token-1".to_string()));
    assert!(backend.user_market_ids_seen().contains(&"condition-1".to_string()));
    assert_eq!(report.title, "Connectivity");
}
```

Add companion failures for:
- no usable token IDs / condition IDs => ws probes `SKIP` with next action
- heartbeat probe performs the allowed request path
- relayer probe never calls submit-like code

- [ ] **Step 2: Run the failing connectivity tests**

Run: `cargo test -p app-live doctor::connectivity -- --test-threads=1`

Expected: FAIL because `connectivity.rs` and the fake backend do not exist yet.

- [ ] **Step 3: Implement the probe backend and production probe runner**

```rust
#[async_trait::async_trait]
trait DoctorProbeBackend {
    async fn probe_authenticated_rest(&self, auth: &LocalL2AuthHeaders) -> Result<(), ProbeError>;
    async fn probe_market_ws(&self, asset_ids: &[String]) -> Result<ProbeOutcome, ProbeError>;
    async fn probe_user_ws(&self, auth: &LocalSignerConfig, markets: &[String]) -> Result<ProbeOutcome, ProbeError>;
    async fn probe_heartbeat(&self, auth: &LocalL2AuthHeaders) -> Result<(), ProbeError>;
    async fn probe_relayer(&self, auth: &LocalRelayerAuth) -> Result<(), ProbeError>;
}
```

Implementation notes:
- Production backend should wrap existing `venue-polymarket` clients.
- Use:
  - `fetch_open_orders` for authenticated REST
  - `subscribe_market_assets` for market ws
  - `subscribe_user_markets` for user ws
  - `post_order_heartbeat` for heartbeat
  - `fetch_recent_transactions` for relayer reachability
- Derive ws subscription scope only from `ResolvedTargets`.
- No metadata/discovery fallback and no order-submit calls.

- [ ] **Step 4: Re-run connectivity unit tests**

Run: `cargo test -p app-live doctor::connectivity -- --test-threads=1`

Expected: PASS

- [ ] **Step 5: Add an integration regression for live/smoke reporting**

Add a `doctor_command.rs` test that uses a fake/injected backend or a test seam to assert:
- `Connectivity: FAIL` when one probe fails
- `Connectivity: PASS` when all probes pass
- next action says rerun doctor after fixing connectivity

- [ ] **Step 6: Run the doctor command tests again**

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test doctor_command -- --test-threads=1`

Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/app-live/src/commands/doctor/mod.rs \
        crates/app-live/src/commands/doctor/connectivity.rs \
        crates/app-live/tests/doctor_command.rs
git commit -m "feat: add doctor venue connectivity probes"
```

## Task 4: Wire Overall Results, Next Actions, And Smoke-Safe Summaries

**Files:**
- Modify: `crates/app-live/src/commands/doctor/mod.rs`
- Modify: `crates/app-live/src/commands/doctor/report.rs`
- Modify: `crates/app-live/tests/doctor_command.rs`

- [ ] **Step 1: Write failing tests for overall results and next actions**

```rust
#[test]
fn doctor_reports_targets_adopt_as_next_action_when_adopted_source_is_unset() {
    let output = Command::new(app_live_binary())
        .arg("doctor")
        .arg("--config")
        .arg(config_fixture("fixtures/app-live-ux-live.toml"))
        .env("DATABASE_URL", default_test_database_url())
        .output()
        .expect("doctor command should execute");

    let combined = combined(&output);
    assert!(combined.contains("Overall: FAIL"), "{combined}");
    assert!(combined.contains("Next: run app-live -- targets candidates"), "{combined}");
}
```

Add companion failures for:
- `PASS WITH SKIPS` in paper mode
- `Next: run app-live -- run --config ...` on a clean pass
- smoke mode summary explicitly saying the startup path is shadow-only

- [ ] **Step 2: Run the failing tests**

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test doctor_command -- --test-threads=1`

Expected: FAIL because overall-result and next-action synthesis are not yet complete.

- [ ] **Step 3: Implement overall aggregation and next-action selection**

```rust
fn next_actions(report: &DoctorRunReport, config_path: &Path) -> Vec<String> {
    if report.has_target_source_failure() {
        return vec![
            "run app-live -- targets candidates --config ...".to_string(),
            "run app-live -- targets adopt --config ...".to_string(),
        ];
    }
    if report.has_connectivity_failure() || report.has_credential_failure() {
        return vec!["fix the reported issue and rerun doctor".to_string()];
    }
    vec![format!("run app-live -- run --config {}", config_path.display())]
}
```

Implementation notes:
- `Restart required` remains informational, not a failure.
- `runtime state unavailable` remains informational, not a failure.
- Smoke wording must stay at startup-config scope, not daemon-runtime-proof scope.

- [ ] **Step 4: Re-run the doctor command tests**

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test doctor_command -- --test-threads=1`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/doctor/mod.rs \
        crates/app-live/src/commands/doctor/report.rs \
        crates/app-live/tests/doctor_command.rs
git commit -m "feat: finalize doctor preflight summaries"
```

## Task 5: Update Operator Docs And Run Verification

**Files:**
- Modify: `README.md`
- Modify: `docs/runbooks/real-user-shadow-smoke.md`
- Modify: `docs/runbooks/operator-target-adoption.md`

- [ ] **Step 1: Write the failing doc drift checklist**

Add a short scratch checklist in the commit message or local notes:
- README startup flow mentions `init -> doctor -> run`
- smoke runbook says `doctor` now runs real venue preflight before `run`
- adoption runbook references `doctor` as the gating preflight after adopt/rollback when appropriate

- [ ] **Step 2: Update docs to match the new doctor behavior**

Example README snippet:

```markdown
1. `app-live init --config config/axiom-arb.local.toml`
2. `app-live targets candidates` / `app-live targets adopt ...` if needed
3. `app-live doctor --config config/axiom-arb.local.toml`
4. `app-live run --config config/axiom-arb.local.toml`
```

- [ ] **Step 3: Run focused verification**

Run:

```bash
cargo fmt --all --check
cargo clippy -p app-live -p config-schema --all-targets -- -D warnings
cargo test -p app-live --test doctor_command -- --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test run_command -- --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test init_command -- --test-threads=1
```

Expected:
- all commands PASS

- [ ] **Step 4: Run broader regression before handoff**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live -- --test-threads=1
```

Expected:
- PASS
- if unrelated suites fail, capture them explicitly before claiming completion

- [ ] **Step 5: Commit**

```bash
git add README.md \
        docs/runbooks/real-user-shadow-smoke.md \
        docs/runbooks/operator-target-adoption.md
git commit -m "docs: refresh doctor preflight workflow"
```

## Final Verification And Handoff

- [ ] **Step 1: Run final repository checks for touched scope**

```bash
git status --short
cargo fmt --all --check
cargo clippy -p app-live -p config-schema --all-targets -- -D warnings
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live -- --test-threads=1
```

Expected:
- clean working tree
- all checks PASS

- [ ] **Step 2: Prepare review**

Use `@superpowers/requesting-code-review` before merging.

- [ ] **Step 3: Summarize operator-facing outcome**

The final handoff should explicitly state:
- which modes now get real external preflight
- which probes remain `SKIP` in `paper`
- that `doctor` still performs no order submission or control-plane mutation
