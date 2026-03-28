# Real-User Shadow Smoke Entrypoint Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a safe real-user `neg-risk` shadow-smoke entrypoint to `app-live` that can ingest authenticated Polymarket user inputs, force `neg-risk` into `Shadow`, and materialize shadow attempts/artifacts without any live submit side effects.

**Architecture:** Build on the existing `app-live` binary instead of introducing a separate harness. Add one explicit env guard (`AXIOM_REAL_USER_SHADOW_SMOKE=1`) plus one explicit real-source config payload, then route that guard through the existing runtime authority chain so `neg-risk` is forced to `Shadow` before execution. Because the current runtime only surfaces "`stays shadow`" as a mode/result and does not persist a matching `neg-risk` shadow attempt/artifact path, this plan also adds the minimal shadow execution surface needed for the smoke to be observable and replayable.

**Tech Stack:** Rust workspace (`app-live`, `risk`, `execution`, `persistence`, `app-replay`, `venue-polymarket`), SQL-backed persistence, Cargo tests, Clippy, structured observability/logging, startup-scoped operator target config.

---

## File Map

- Create: `crates/app-live/src/smoke.rs`
  - Parse the explicit smoke guard env and carry a small validated `RealUserShadowSmokeConfig`.
  - Keep smoke-only policy decisions out of `main.rs`.
- Create: `crates/app-live/src/negrisk_shadow.rs`
  - Build the minimal `neg-risk` shadow attempt/artifact flow using `ExecutionMode::Shadow` and `ShadowVenueSink`.
  - Persist `ExecutionAttemptRow` + `ShadowExecutionArtifactRow` for smoke-visible auditability.
- Create: `crates/app-live/src/source_tasks.rs`
  - Assemble concrete Polymarket live-source inputs from `PolymarketSourceConfig` and authenticated signer headers.
  - Keep daemon/source wiring separate from CLI env parsing.
- Create: `crates/app-live/tests/real_user_shadow_smoke.rs`
  - Cover smoke-guard authority, source-config wiring, and shadow-only runtime outcomes.
- Create: `crates/app-replay/tests/negrisk_shadow_contract.rs`
  - Verify replay/reporting surfaces can see the new `neg-risk` shadow attempts/artifacts.
- Create: `docs/runbooks/real-user-shadow-smoke.md`
  - Operator-facing runbook for the manual smoke, including envs, commands, SQL checks, and pass/fail rubric.

- Modify: `crates/app-live/src/config.rs`
  - Keep `PolymarketSourceConfig` parsing here.
  - Add a small loader/helper for the new source-config env payload if needed.
- Modify: `crates/app-live/src/lib.rs`
  - Export the new smoke/source helper surfaces.
- Modify: `crates/app-live/src/main.rs`
  - Read `AXIOM_REAL_USER_SHADOW_SMOKE` and `AXIOM_POLYMARKET_SOURCE_CONFIG`.
  - Fail fast when smoke is enabled in invalid modes or with missing config.
- Modify: `crates/app-live/src/daemon.rs`
  - Accept the validated smoke/source bundle and pass it into the runtime/supervisor assembly path.
- Modify: `crates/app-live/src/supervisor.rs`
  - Use the smoke config to force `neg-risk` shadow-only behavior and to surface smoke-safe status in `SupervisorSummary`.
  - Wire the new shadow execution path so startup can persist shadow attempts/artifacts instead of only saying "`negrisk_mode=Shadow`".
- Modify: `crates/app-live/src/negrisk_live.rs`
  - Extract or reuse shared `neg-risk` target-to-plan conversion helpers so the new shadow path does not duplicate business-shape logic.
- Modify: `crates/app-live/tests/config.rs`
  - Add config parsing coverage for smoke/source env behavior.
- Modify: `crates/app-live/tests/main_entrypoint.rs`
  - Add binary-level fail-fast coverage for smoke guard misuse.
- Modify: `crates/app-live/tests/negrisk_live_rollout.rs`
  - Keep current live-rollout behavior green and add assertions that smoke guard refuses live promotion.
- Modify: `crates/app-live/tests/daemon_lifecycle.rs`
  - Verify smoke startup still restores truth before resuming work.
- Modify: `crates/risk/src/activation.rs`
  - Add the smallest possible policy hook for forcing `neg-risk` to `Shadow` without inventing a second execution authority.
- Modify: `crates/risk/tests/activation_policy.rs`
  - Prove the smoke clamp applies only to `neg-risk` and only when explicitly enabled.
- Modify: `crates/app-replay/src/lib.rs`
  - Load `neg-risk` shadow attempts/artifacts for smoke reporting.
- Modify: `crates/app-replay/src/main.rs`
  - Emit a small shadow-smoke summary span/log when such rows exist.
- Modify: `README.md`
  - Document the new smoke-safe mode and explicitly state that it is shadow-only and manual.

## Scope Guard

This plan is intentionally narrow. It does **not**:

1. add automatic candidate adoption or hot reload
2. add a second standalone smoke binary
3. allow `neg-risk` live submit while the smoke guard is enabled
4. claim end-to-end happy-path trading on a zero-balance account

The point of this work is to make `real user upstream + shadow-only downstream` verifiable through the existing runtime.

### Task 1: Smoke Guard Config Contract

**Files:**
- Create: `crates/app-live/src/smoke.rs`
- Modify: `crates/app-live/src/config.rs`
- Modify: `crates/app-live/src/lib.rs`
- Modify: `crates/app-live/src/main.rs`
- Test: `crates/app-live/tests/config.rs`
- Test: `crates/app-live/tests/main_entrypoint.rs`

- [ ] **Step 1: Write the failing config/unit tests**

```rust
#[test]
fn parses_real_user_shadow_smoke_guard_when_enabled() {
    let smoke = load_real_user_shadow_smoke_config(
        Some("1"),
        Some(valid_polymarket_source_config_json()),
    )
    .unwrap();

    assert!(smoke.enabled);
    assert_eq!(
        smoke.source_config.market_ws_url.as_str(),
        "wss://ws-subscriptions-clob.polymarket.com/ws/market"
    );
}

#[test]
fn enabling_real_user_shadow_smoke_requires_source_config() {
    let err = load_real_user_shadow_smoke_config(Some("1"), None).unwrap_err();

    assert!(err.to_string().contains("missing polymarket source config"));
}
```

- [ ] **Step 2: Run the targeted config test to verify it fails**

Run:
```bash
cargo test -p app-live --test config parses_real_user_shadow_smoke_guard_when_enabled -- --exact
```

Expected: FAIL because the smoke config loader does not exist yet.

- [ ] **Step 3: Write the failing binary-entrypoint tests**

```rust
#[test]
fn paper_entrypoint_rejects_real_user_shadow_smoke() {
    let output = app_live_output_raw_env(
        "paper",
        Some(("AXIOM_REAL_USER_SHADOW_SMOKE", "1")),
        Some(("AXIOM_POLYMARKET_SOURCE_CONFIG", valid_polymarket_source_config_json())),
    );

    assert!(!output.status.success());
}

#[test]
fn live_entrypoint_rejects_real_user_shadow_smoke_without_source_config() {
    let output = app_live_output_raw_env(
        "live",
        Some(("AXIOM_REAL_USER_SHADOW_SMOKE", "1")),
        None,
    );

    assert!(!output.status.success());
}
```

- [ ] **Step 4: Run the targeted entrypoint test to verify it fails**

Run:
```bash
cargo test -p app-live --test main_entrypoint live_entrypoint_rejects_real_user_shadow_smoke_without_source_config -- --exact
```

Expected: FAIL because `main.rs` does not yet understand the smoke env or source-config env.

- [ ] **Step 5: Implement the minimal smoke config surface**

Add a focused config type in `crates/app-live/src/smoke.rs`:

```rust
pub struct RealUserShadowSmokeConfig {
    pub enabled: bool,
    pub source_config: PolymarketSourceConfig,
}

pub fn load_real_user_shadow_smoke_config(
    guard: Option<&str>,
    source_json: Option<&str>,
) -> Result<Option<RealUserShadowSmokeConfig>, ConfigError> {
    // enabled only when guard == "1"
    // if enabled, source_json is required
}
```

Update `main.rs` to:
- read `AXIOM_REAL_USER_SHADOW_SMOKE`
- read `AXIOM_POLYMARKET_SOURCE_CONFIG`
- reject `paper + smoke`
- keep non-smoke behavior unchanged

- [ ] **Step 6: Re-run the config and entrypoint tests**

Run:
```bash
cargo test -p app-live --test config
cargo test -p app-live --test main_entrypoint
```

Expected: PASS with new smoke-env coverage and no regressions in existing live/paper env behavior.

- [ ] **Step 7: Commit**

```bash
git add crates/app-live/src/smoke.rs crates/app-live/src/config.rs crates/app-live/src/lib.rs crates/app-live/src/main.rs crates/app-live/tests/config.rs crates/app-live/tests/main_entrypoint.rs
git commit -m "feat: add real-user shadow smoke config contract"
```

### Task 2: Shadow-Only Authority And Minimal Neg-Risk Shadow Surface

**Files:**
- Create: `crates/app-live/src/negrisk_shadow.rs`
- Modify: `crates/app-live/src/negrisk_live.rs`
- Modify: `crates/app-live/src/supervisor.rs`
- Modify: `crates/risk/src/activation.rs`
- Test: `crates/risk/tests/activation_policy.rs`
- Test: `crates/app-live/tests/negrisk_live_rollout.rs`
- Test: `crates/app-live/tests/real_user_shadow_smoke.rs`

- [ ] **Step 1: Write the failing activation-policy test**

```rust
#[test]
fn real_user_shadow_smoke_forces_negrisk_shadow_without_touching_fullset() {
    let policy = ActivationPolicy::from_rules(
        "smoke-test",
        vec![
            RolloutRule::new("neg-risk", "family-a", ExecutionMode::Live, "family-a-live"),
            RolloutRule::new("full-set", "default", ExecutionMode::Live, "fullset-live"),
        ],
    )
    .with_real_user_shadow_smoke();

    assert_eq!(
        policy.mode_for_route("neg-risk", "family-a"),
        ExecutionMode::Shadow
    );
    assert_eq!(
        policy.mode_for_route("full-set", "default"),
        ExecutionMode::Live
    );
}
```

- [ ] **Step 2: Run the targeted activation test to verify it fails**

Run:
```bash
cargo test -p risk --test activation_policy real_user_shadow_smoke_forces_negrisk_shadow_without_touching_fullset -- --exact
```

Expected: FAIL because the policy has no smoke-specific clamp.

- [ ] **Step 3: Write the failing supervisor/smoke tests**

Add one focused smoke integration test:

```rust
#[test]
fn smoke_guard_turns_live_eligible_family_into_shadow_attempt_and_never_live_attempt() {
    let mut supervisor = AppSupervisor::for_tests().with_real_user_shadow_smoke();
    supervisor.seed_neg_risk_live_targets(BTreeMap::from([(
        "family-a".to_owned(),
        sample_live_target("family-a"),
    )]));
    supervisor.seed_neg_risk_live_approval("family-a");
    supervisor.seed_neg_risk_live_ready_family("family-a");

    let summary = supervisor.run_once().unwrap();

    assert_eq!(summary.negrisk_mode, ExecutionMode::Shadow);
    assert_eq!(summary.neg_risk_live_attempt_count, 0);
    assert!(supervisor.neg_risk_shadow_execution_records().len() >= 1);
}
```

Expected new behavior:
- `neg-risk` remains `Shadow`
- at least one shadow attempt/artifact is materialized
- no live submit record is synthesized

- [ ] **Step 4: Run the targeted supervisor test to verify it fails**

Run:
```bash
cargo test -p app-live --test real_user_shadow_smoke smoke_guard_turns_live_eligible_family_into_shadow_attempt_and_never_live_attempt -- --exact
```

Expected: FAIL because `app-live` currently has no `neg-risk` shadow execution path.

- [ ] **Step 5: Implement the minimal shadow execution helper**

Create `crates/app-live/src/negrisk_shadow.rs` with the smallest mirror of the existing live helper:

```rust
pub struct NegRiskShadowExecutionRecord {
    pub attempt_id: String,
    pub plan_id: String,
    pub snapshot_id: String,
    pub execution_mode: ExecutionMode,
    pub route: String,
    pub scope: String,
    pub artifacts: Vec<ShadowExecutionArtifactRow>,
}

pub fn eligible_shadow_records(...) -> Result<Vec<NegRiskShadowExecutionRecord>, NegRiskShadowError> {
    // plan via the existing neg-risk planner
    // execute via ShadowVenueSink
    // return attempt/artifact rows for persistence
}
```

Use `ExecutionMode::Shadow` plus `ShadowVenueSink`, and keep the planning shape shared with `negrisk_live.rs` rather than duplicating target canonicalization logic.

- [ ] **Step 6: Thread the smoke clamp through the existing authority chain**

Modify:
- `crates/risk/src/activation.rs` to add a small `with_real_user_shadow_smoke()` style helper
- `crates/app-live/src/supervisor.rs` to:
  - remember that smoke guard is enabled
  - call the shadow helper instead of `eligible_live_records(...)` when smoke is on
  - persist `ExecutionAttemptRow` + `ShadowExecutionArtifactRow`
  - keep `negrisk_mode=Shadow`

Do **not** implement this as a sink-level override. The policy/result must already be `Shadow` before execution.

- [ ] **Step 7: Re-run the targeted tests**

Run:
```bash
cargo test -p risk --test activation_policy
cargo test -p app-live --test negrisk_live_rollout
cargo test -p app-live --test real_user_shadow_smoke
```

Expected: PASS with:
- `neg-risk` forced to `Shadow` under smoke
- no live attempt records
- new shadow attempt/artifact rows created

- [ ] **Step 8: Commit**

```bash
git add crates/app-live/src/negrisk_shadow.rs crates/app-live/src/negrisk_live.rs crates/app-live/src/supervisor.rs crates/risk/src/activation.rs crates/risk/tests/activation_policy.rs crates/app-live/tests/negrisk_live_rollout.rs crates/app-live/tests/real_user_shadow_smoke.rs
git commit -m "feat: add neg-risk shadow smoke execution path"
```

### Task 3: Real Source Wiring For Smoke-Safe Daemon Startup

**Files:**
- Create: `crates/app-live/src/source_tasks.rs`
- Modify: `crates/app-live/src/daemon.rs`
- Modify: `crates/app-live/src/main.rs`
- Modify: `crates/app-live/src/supervisor.rs`
- Modify: `crates/app-live/src/lib.rs`
- Test: `crates/app-live/tests/ingest_task_groups.rs`
- Test: `crates/app-live/tests/daemon_lifecycle.rs`
- Test: `crates/app-live/tests/real_user_shadow_smoke.rs`

- [ ] **Step 1: Write the failing daemon/source wiring test**

```rust
#[test]
fn smoke_startup_uses_real_source_config_and_reports_smoke_safe_summary() {
    let report = run_smoke_startup_for_tests(
        valid_polymarket_source_config(),
        valid_local_signer_config(),
        sample_shadow_smoke_targets(),
    )
    .unwrap();

    assert!(report.summary.real_user_shadow_smoke);
    assert_eq!(report.summary.negrisk_mode, ExecutionMode::Shadow);
    assert!(report.startup_order.contains(&"ingest".to_owned()));
}
```

- [ ] **Step 2: Run the targeted daemon/source test to verify it fails**

Run:
```bash
cargo test -p app-live --test daemon_lifecycle smoke_startup_uses_real_source_config_and_reports_smoke_safe_summary -- --exact
```

Expected: FAIL because the current daemon path still boots from `StaticSnapshotSource::empty()` and does not expose smoke-safe status.

- [ ] **Step 3: Implement the source-task assembly module**

Create `crates/app-live/src/source_tasks.rs` with one focused responsibility:

```rust
pub struct RealUserShadowSmokeSources {
    pub market: MarketDataTaskGroup,
    pub user: UserStateTaskGroup,
    pub heartbeat: HeartbeatTaskGroup<...>,
    pub relayer: RelayerTaskGroup,
    pub metadata: MetadataTaskGroup,
}

pub fn build_real_user_shadow_smoke_sources(
    source: &PolymarketSourceConfig,
    signer: &LocalSignerConfig,
) -> Result<RealUserShadowSmokeSources, String> {
    // only assemble concrete source adapters
}
```

Keep assembly out of `main.rs`.

- [ ] **Step 4: Wire the smoke sources into daemon startup**

Modify `daemon.rs` / `supervisor.rs` so that when smoke is enabled:
- real source config is loaded and attached to startup
- `SupervisorSummary` gains an explicit `real_user_shadow_smoke: bool`
- startup logs/summary cannot claim an ordinary live run

Do **not** change non-smoke startup behavior.

- [ ] **Step 5: Re-run the daemon/source tests**

Run:
```bash
cargo test -p app-live --test ingest_task_groups
cargo test -p app-live --test daemon_lifecycle
cargo test -p app-live --test real_user_shadow_smoke
```

Expected: PASS with smoke startup clearly distinct from ordinary live startup.

- [ ] **Step 6: Commit**

```bash
git add crates/app-live/src/source_tasks.rs crates/app-live/src/daemon.rs crates/app-live/src/main.rs crates/app-live/src/supervisor.rs crates/app-live/src/lib.rs crates/app-live/tests/ingest_task_groups.rs crates/app-live/tests/daemon_lifecycle.rs crates/app-live/tests/real_user_shadow_smoke.rs
git commit -m "feat: wire real-user shadow smoke daemon sources"
```

### Task 4: Replay Visibility, Operator Runbook, And README

**Files:**
- Create: `crates/app-replay/tests/negrisk_shadow_contract.rs`
- Create: `docs/runbooks/real-user-shadow-smoke.md`
- Modify: `crates/app-replay/src/lib.rs`
- Modify: `crates/app-replay/src/main.rs`
- Modify: `README.md`

- [ ] **Step 1: Write the failing replay contract test**

```rust
#[tokio::test]
async fn replay_lists_neg_risk_shadow_attempts_and_artifacts_for_smoke_runs() {
    let rows = load_negrisk_shadow_attempt_artifacts(&db.pool).await.unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].attempt.execution_mode, ExecutionMode::Shadow);
    assert_eq!(rows[0].artifacts[0].stream, "shadow.execution");
}
```

- [ ] **Step 2: Run the targeted replay test to verify it fails**

Run:
```bash
cargo test -p app-replay --test negrisk_shadow_contract replay_lists_neg_risk_shadow_attempts_and_artifacts_for_smoke_runs -- --exact
```

Expected: FAIL because `app-replay` only loads live neg-risk attempt artifacts today.

- [ ] **Step 3: Implement the minimal replay surface**

Add to `crates/app-replay/src/lib.rs`:

```rust
pub struct NegRiskShadowAttemptArtifacts {
    pub attempt: ExecutionAttemptRow,
    pub artifacts: Vec<ShadowExecutionArtifactRow>,
}

pub async fn load_negrisk_shadow_attempt_artifacts(...) -> Result<Vec<NegRiskShadowAttemptArtifacts>, PersistenceError> {
    // load shadow attempts
    // load shadow artifacts
    // filter route == "neg-risk"
}
```

Then update `main.rs` to emit a small structured replay summary when shadow rows exist.

- [ ] **Step 4: Document the manual smoke**

Add `docs/runbooks/real-user-shadow-smoke.md` with:
- required env vars
- startup command
- SQL checks for `event_journal`, `execution_attempts`, `shadow_execution_artifacts`, `live_submission_records`
- pass/fail rubric

Update `README.md` to state:
- this repository now supports a real-user shadow smoke guard
- the mode is shadow-only and manual
- it still does not imply fully production-enabled `neg-risk Live`

- [ ] **Step 5: Re-run the targeted replay/docs-adjacent tests**

Run:
```bash
cargo test -p app-replay --test negrisk_shadow_contract
cargo test -p app-replay --test replay_app
```

Expected: PASS with replay-visible shadow smoke artifacts and no regressions in existing live/candidate reporting.

- [ ] **Step 6: Commit**

```bash
git add crates/app-replay/src/lib.rs crates/app-replay/src/main.rs crates/app-replay/tests/negrisk_shadow_contract.rs docs/runbooks/real-user-shadow-smoke.md README.md
git commit -m "feat: document and replay real-user shadow smoke runs"
```

### Task 5: Final Verification And Manual Smoke Checklist

**Files:**
- Modify: `docs/runbooks/real-user-shadow-smoke.md` (only if the verification reveals missing commands)

- [ ] **Step 1: Run formatter**

Run:
```bash
cargo fmt --all
```

Expected: formatting applies cleanly.

- [ ] **Step 2: Verify formatting is stable**

Run:
```bash
cargo fmt --all --check
```

Expected: PASS with no diff.

- [ ] **Step 3: Run lint**

Run:
```bash
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 4: Run targeted Rust test suites**

Run:
```bash
cargo test -p risk --test activation_policy
cargo test -p app-live --test config --test main_entrypoint --test ingest_task_groups --test daemon_lifecycle --test negrisk_live_rollout --test real_user_shadow_smoke
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test runtime_backbone --test negrisk_live --test migrations
cargo test -p app-replay --test replay_app --test negrisk_live_contract --test negrisk_shadow_contract
cargo test -p venue-polymarket --test ws_client --test ws_feeds --test heartbeat --test negrisk_live_provider --test order_submission --test status_and_retry
```

Expected: PASS.

- [ ] **Step 5: Run the full workspace suite once**

Run:
```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test --workspace
```

Expected: PASS.

- [ ] **Step 6: Execute the manual smoke checklist against the real account**

Runbook source:
`docs/runbooks/real-user-shadow-smoke.md`

Minimum pass criteria:
- authenticated REST and ws inputs succeed
- runtime produces at least one `execution_attempts.execution_mode = 'shadow'`
- `shadow_execution_artifacts` contains at least one row
- `live_submission_records` remains empty

- [ ] **Step 7: Commit final cleanup if needed**

```bash
git add docs/runbooks/real-user-shadow-smoke.md
git commit -m "chore: finalize real-user shadow smoke verification"
```

## Notes For The Implementer

- Keep the smoke guard **explicit**. Do not infer it from source-config presence.
- Keep authority **single-path**. Do not let `LiveVenueSink` see a `neg-risk` plan when smoke is on.
- Keep this mode **startup-scoped**. Do not add hot reload or a second harness.
- Prefer one small shared helper between `negrisk_live.rs` and `negrisk_shadow.rs` over copy/paste target canonicalization.
- If you discover that current `task_groups` are still too stubbed to support concrete Polymarket wiring cleanly, stop and split the assembly into `source_tasks.rs` instead of bloating `main.rs` or `daemon.rs`.
