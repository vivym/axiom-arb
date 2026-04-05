# AxiomArb Strategy-Neutral Control Plane Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution discipline:** Use `@superpowers:test-driven-development` for every task and `@superpowers:verification-before-completion` before claiming completion.

**Goal:** Replace the current `neg-risk`-shaped operator control plane with a route-neutral `route + scope` control plane that supports `full-set`, `neg-risk`, and `both`, while preserving fail-closed startup, deterministic adopted revisions, and compatibility for legacy explicit targets.

**Architecture:** Introduce neutral strategy-control domain and persistence lineage, then layer route adapters on top for candidate generation, startup decoding, execution, readiness, and verification. Keep one globally authoritative `operator_strategy_revision`, make global revision identity deterministic from canonical route artifacts, and treat legacy explicit config as a compatibility input that can be explicitly migrated into the neutral lineage model via `targets adopt`.

**Tech Stack:** Rust, `clap`, `serde`/`toml`, `sqlx`/Postgres migrations, existing `app-live` command/runtime modules, `persistence` repos/models, `execution` sink/provider abstractions, `cargo test`, `cargo fmt`, `cargo clippy`

---

## File Map

### New files

- `migrations/0015_strategy_neutral_control_plane.sql`
  - Add neutral strategy-control lineage tables and runtime anchor columns without destroying legacy target-named history.
- `crates/domain/src/strategy_control.rs`
  - Neutral control-plane domain types such as `StrategyKey`, neutral candidate/adoptable/adoption lineage shapes, canonical semantic artifact digests, and compatibility migration markers.
- `crates/persistence/tests/strategy_control.rs`
  - Persistence coverage for neutral lineage tables, deterministic bundle identity storage, compatibility migration rows, and semantic-digest stability.
- `crates/app-live/src/strategy_control.rs`
  - Neutral control-plane orchestration helpers: bundle identity computation, compatibility migration rendering, startup-plan assembly, and shared operator-facing summaries.
- `crates/app-live/src/route_adapters/mod.rs`
  - Neutral adapter traits for control-plane, runtime, execution, readiness, and verify surfaces.
- `crates/app-live/src/route_adapters/fullset.rs`
  - `full-set` route adapter implementation for `default` scope, deterministic route artifact rendering, readiness facts, and verify hooks.
- `crates/app-live/src/route_adapters/negrisk.rs`
  - `neg-risk` route adapter implementation for family scopes, rollout fallback handling, route artifact rendering, readiness facts, and verify hooks.
- `crates/app-live/tests/support/strategy_control_db.rs`
  - Shared Postgres helpers for neutral strategy-control lineage, compatibility-mode config seeding, and multi-route verify/startup fixtures.
- `crates/app-live/tests/strategy_control.rs`
  - Route-adapter and strategy-control orchestration integration tests that do not fit existing single-command test files.

### Modified files

- `crates/domain/src/lib.rs`
  - Export new neutral strategy-control types without breaking current target-named compatibility exports.
- `crates/domain/src/candidates.rs`
  - Keep compatibility types but stop using them as the only source of control-plane truth.
- `crates/persistence/src/models.rs`
  - Add neutral strategy-control rows and neutral runtime anchor fields beside legacy target-named fields.
- `crates/persistence/src/repos.rs`
  - Add neutral strategy-control repos, deterministic bundle lookups, and compatibility-read fallbacks.
- `crates/persistence/src/lib.rs`
  - Export new neutral repos/models and any new persistence errors.
- `crates/persistence/tests/operator_target_adoption.rs`
  - Extend compatibility coverage so legacy target lineage still resolves during the migration window.
- `crates/persistence/tests/phase3e_candidate_generation.rs`
  - Update candidate-generation expectations for neutral bundle storage and digest stability.
- `crates/persistence/tests/run_sessions.rs`
  - Add run-session anchor coverage for `operator_strategy_revision`.
- `crates/config-schema/src/raw.rs`
  - Add `[strategy_control]` and `[strategies.*]` raw config shapes while preserving legacy `negrisk.*`.
- `crates/config-schema/src/validate.rs`
  - Add neutral validated views, compatibility-mode detection, and migration-safe invariants.
- `crates/config-schema/src/lib.rs`
  - Export new raw/view types.
- `crates/config-schema/tests/config_roundtrip.rs`
  - Verify neutral config renders/roundtrips and legacy shapes remain loadable.
- `crates/config-schema/tests/validated_views.rs`
  - Verify neutral views, legacy compatibility views, and migration gating.
- `crates/config-schema/tests/fixtures/app-live-live.toml`
- `crates/config-schema/tests/fixtures/app-live-smoke.toml`
- `crates/config-schema/tests/fixtures/app-live-ux-smoke.toml`
  - Add neutral config fixtures and keep compatibility fixtures realistic.
- `crates/app-live/src/lib.rs`
  - Export strategy-control and route-adapter modules.
- `crates/app-live/src/config.rs`
  - Introduce neutral route artifact decode/encode helpers and keep legacy explicit `NegRiskLiveTargetSet` as compatibility input only.
- `crates/app-live/src/discovery.rs`
  - Convert from single-route candidate rendering to route-local production plus neutral bundling.
- `crates/app-live/src/startup.rs`
  - Replace `ResolvedTargets` with a neutral startup bundle that can contain both routes and compatibility startup rendering.
- `crates/app-live/src/daemon.rs`
  - Start the runtime from a neutral startup bundle instead of a `NegRiskLiveTargetSet`-only path.
- `crates/app-live/src/runtime.rs`
  - Thread neutral strategy-control anchors and per-route runtime/execution adapters through the runtime path.
- `crates/app-live/src/smoke.rs`
  - Clamp all risk-expanding routes to `Shadow` in `real_user_shadow_smoke`.
- `crates/app-live/src/commands/discover.rs`
  - Orchestrate route-local candidate producers, bundle route artifacts, and report per-route diffs without allowing readiness drift to change bundle identity.
- `crates/app-live/src/commands/run.rs`
  - Resolve neutral startup plans and enforce route-neutral smoke semantics.
- `crates/app-live/src/commands/targets/adopt.rs`
  - Adopt neutral revisions and synthesize the first neutral revision from legacy explicit config when needed.
- `crates/app-live/src/commands/targets/candidates.rs`
  - Render neutral candidate bundles and compatibility metadata.
- `crates/app-live/src/commands/targets/config_file.rs`
  - Rewrite config to the neutral `[strategy_control]` anchor and preserve compatibility semantics.
- `crates/app-live/src/commands/targets/rollback.rs`
  - Disable rollback until neutral adoption history exists and then operate only on neutral lineage.
- `crates/app-live/src/commands/targets/show_current.rs`
- `crates/app-live/src/commands/targets/state.rs`
- `crates/app-live/src/commands/targets/status.rs`
  - Surface compatibility mode explicitly and stop pretending legacy explicit config already has a neutral adopted revision.
- `crates/app-live/src/commands/status/model.rs`
- `crates/app-live/src/commands/status/evaluate.rs`
- `crates/app-live/src/commands/status/mod.rs`
  - Report neutral revision state, route-level readiness, and compatibility-mode guidance.
- `crates/app-live/src/commands/doctor/mod.rs`
- `crates/app-live/src/commands/doctor/target_source.rs`
- `crates/app-live/src/commands/doctor/report.rs`
  - Add route-neutral checks and compatibility-mode operator guidance.
- `crates/app-live/src/commands/apply/mod.rs`
- `crates/app-live/src/commands/apply/model.rs`
- `crates/app-live/src/commands/apply/output.rs`
  - Make `apply` neutral-revision aware and explicitly reject compatibility-mode auto-migration.
- `crates/app-live/src/commands/verify/context.rs`
- `crates/app-live/src/commands/verify/evidence.rs`
- `crates/app-live/src/commands/verify/model.rs`
- `crates/app-live/src/commands/verify/mod.rs`
- `crates/app-live/src/commands/verify/session.rs`
  - Make `verify` revision-aware, route-aware, and aggregate route verdicts deterministically.
- `crates/app-live/src/cli.rs`
  - Update CLI help text if needed to reflect route-neutral control-plane terminology.
- `crates/execution/src/providers.rs`
- `crates/execution/src/sink.rs`
- `crates/execution/src/plans.rs`
  - Remove the assumption that the only real live submit path is family-shaped and `neg-risk`-specific.
- `crates/risk/src/activation.rs`
- `crates/risk/src/rollout.rs`
  - Preserve kernel ownership of `route + scope` activation and `default` fallback while making smoke clamps route-neutral.
- `crates/app-live/tests/discover_command.rs`
- `crates/app-live/tests/discovery_supervisor.rs`
- `crates/app-live/tests/candidate_daemon.rs`
  - Add route-local bundle and deterministic rediscovery coverage.
- `crates/app-live/tests/startup_resolution.rs`
- `crates/app-live/tests/run_command.rs`
- `crates/app-live/tests/real_user_shadow_smoke.rs`
- `crates/app-live/tests/daemon_lifecycle.rs`
  - Cover neutral startup bundles, multi-route runs, and all-route smoke clamping.
- `crates/app-live/tests/targets_read_commands.rs`
- `crates/app-live/tests/targets_write_commands.rs`
- `crates/app-live/tests/targets_config_file.rs`
  - Cover compatibility-mode migration, neutral adoption history, and rollback gating.
- `crates/app-live/tests/status_command.rs`
- `crates/app-live/tests/doctor_command.rs`
- `crates/app-live/tests/apply_command.rs`
  - Cover compatibility-mode UX and neutral route-level readiness.
- `crates/app-live/tests/verify_command.rs`
- `crates/app-live/tests/support/verify_db.rs`
  - Cover route-aware evidence aggregation, compatibility windows, and smoke invariants.
- `crates/app-live/tests/config.rs`
- `crates/app-live/tests/main_entrypoint.rs`
- `crates/app-live/tests/support/mod.rs`
- `crates/app-live/tests/support/discover_db.rs`
- `crates/app-live/tests/support/status_db.rs`
- `crates/app-live/tests/support/apply_db.rs`
  - Keep fixtures aligned with the new neutral control-plane model.
- `README.md`
  - Update operator-facing command descriptions and migration guidance.
- `config/axiom-arb.example.toml`
  - Add neutral config examples and compatibility comments.
- `docs/runbooks/real-user-shadow-smoke.md`
  - Document route-neutral smoke behavior and verify expectations.

### Existing files to study before editing

- `docs/superpowers/specs/2026-04-06-axiomarb-strategy-neutral-control-plane-design.md`
- `crates/domain/src/candidates.rs`
- `crates/persistence/src/models.rs`
- `crates/persistence/src/repos.rs`
- `crates/config-schema/src/raw.rs`
- `crates/config-schema/src/validate.rs`
- `crates/app-live/src/discovery.rs`
- `crates/app-live/src/startup.rs`
- `crates/app-live/src/commands/discover.rs`
- `crates/app-live/src/commands/targets/state.rs`
- `crates/app-live/src/commands/status/evaluate.rs`
- `crates/app-live/src/commands/doctor/target_source.rs`
- `crates/app-live/src/commands/apply/model.rs`
- `crates/app-live/src/commands/verify/evidence.rs`
- `crates/execution/src/sink.rs`
- `crates/execution/src/providers.rs`
- `crates/risk/src/activation.rs`

---

### Task 1: Add Neutral Strategy-Control Domain and Persistence Lineage

**Files:**
- Create: `migrations/0015_strategy_neutral_control_plane.sql`
- Create: `crates/domain/src/strategy_control.rs`
- Modify: `crates/domain/src/lib.rs`
- Modify: `crates/persistence/src/models.rs`
- Modify: `crates/persistence/src/repos.rs`
- Modify: `crates/persistence/src/lib.rs`
- Create: `crates/persistence/tests/strategy_control.rs`
- Modify: `crates/persistence/tests/run_sessions.rs`

- [ ] **Step 1: Write the failing persistence tests**

Add tests covering neutral tables, neutral runtime anchors, and semantic-digest stability:

```rust
#[tokio::test]
async fn strategy_control_migration_creates_neutral_lineage_tables() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let tables: Vec<String> = sqlx::query_scalar(
        "select table_name from information_schema.tables where table_schema = current_schema()"
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();

    assert!(tables.iter().any(|name| name == "strategy_candidate_sets"));
    assert!(tables.iter().any(|name| name == "adoptable_strategy_revisions"));
    assert!(tables.iter().any(|name| name == "strategy_adoption_provenance"));
}

#[test]
fn semantic_digest_ignores_provenance_only_metadata() {
    let left = canonical_route_artifact_digest(json!({
        "route": "full-set",
        "scope": "default",
        "payload": {"mode": "default"},
        "snapshot_id": "snapshot-a",
        "source_session_id": "session-a",
    }));
    let right = canonical_route_artifact_digest(json!({
        "route": "full-set",
        "scope": "default",
        "payload": {"mode": "default"},
        "snapshot_id": "snapshot-b",
        "source_session_id": "session-b",
    }));

    assert_eq!(left, right);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
cargo test -p persistence --test strategy_control strategy_control_migration_creates_neutral_lineage_tables -- --exact --test-threads=1
cargo test -p persistence --test strategy_control semantic_digest_ignores_provenance_only_metadata -- --exact --test-threads=1
```

Expected: FAIL because the neutral tables, neutral anchors, and digest helpers do not exist yet.

- [ ] **Step 3: Implement the migration, domain shapes, and repos**

Add the neutral types and persistence layer:

```rust
pub struct StrategyKey {
    pub route: String,
    pub scope: String,
}

pub struct StrategyArtifactDigest {
    pub route: String,
    pub scope: String,
    pub digest: String,
}

pub fn canonical_route_artifact_digest(value: &serde_json::Value) -> String {
    // Strip non-semantic metadata, canonicalize key order, then hash.
}
```

```sql
CREATE TABLE strategy_candidate_sets (...);
CREATE TABLE adoptable_strategy_revisions (...);
CREATE TABLE strategy_adoption_provenance (...);
CREATE TABLE operator_strategy_adoption_history (...);
ALTER TABLE runtime_progress ADD COLUMN operator_strategy_revision TEXT;
ALTER TABLE run_sessions ADD COLUMN configured_operator_strategy_revision TEXT;
ALTER TABLE run_sessions ADD COLUMN active_operator_strategy_revision_at_start TEXT;
```

- [ ] **Step 4: Run the persistence tests to verify they pass**

Run:

```bash
cargo test -p persistence --test strategy_control -- --test-threads=1
cargo test -p persistence --test run_sessions -- --test-threads=1
```

Expected: PASS with the new tables, digest behavior, and neutral runtime anchors present.

- [ ] **Step 5: Commit**

```bash
git add migrations/0015_strategy_neutral_control_plane.sql crates/domain/src/strategy_control.rs crates/domain/src/lib.rs crates/persistence/src/models.rs crates/persistence/src/repos.rs crates/persistence/src/lib.rs crates/persistence/tests/strategy_control.rs crates/persistence/tests/run_sessions.rs
git commit -m "feat: add neutral strategy control lineage"
```

### Task 2: Add Neutral Config Schema and Compatibility Detection

**Files:**
- Modify: `crates/config-schema/src/raw.rs`
- Modify: `crates/config-schema/src/validate.rs`
- Modify: `crates/config-schema/src/lib.rs`
- Modify: `crates/config-schema/tests/config_roundtrip.rs`
- Modify: `crates/config-schema/tests/validated_views.rs`
- Modify: `crates/config-schema/tests/fixtures/app-live-live.toml`
- Modify: `crates/config-schema/tests/fixtures/app-live-smoke.toml`
- Modify: `crates/config-schema/tests/fixtures/app-live-ux-smoke.toml`
- Modify: `crates/app-live/tests/config.rs`

- [ ] **Step 1: Write the failing config tests**

Add tests for the new neutral config shape and compatibility detection:

```rust
#[test]
fn validated_config_accepts_strategy_control_and_route_sections() {
    let raw = load_raw_config_from_str(r#"
[runtime]
mode = "live"

[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-12"

[strategies.full_set]
enabled = true
"#).unwrap();

    let validated = ValidatedConfig::new(raw).unwrap();
    let view = validated.for_app_live().unwrap();
    assert_eq!(view.operator_strategy_revision(), Some("strategy-rev-12"));
}

#[test]
fn validated_config_marks_legacy_explicit_targets_as_compatibility_mode() {
    let raw = load_raw_config_from_str(r#"
[runtime]
mode = "live"

[negrisk]
targets = []
"#).unwrap();

    let validated = ValidatedConfig::new(raw).unwrap();
    let view = validated.for_app_live().unwrap();
    assert!(view.is_legacy_explicit_strategy_config());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
cargo test -p config-schema validated_config_accepts_strategy_control_and_route_sections -- --exact
cargo test -p config-schema validated_config_marks_legacy_explicit_targets_as_compatibility_mode -- --exact
```

Expected: FAIL because the neutral config sections and compatibility helpers do not exist yet.

- [ ] **Step 3: Implement the neutral raw structs and validated views**

Add the neutral config model:

```rust
pub struct StrategyControlToml {
    pub source: StrategyControlSourceToml,
    pub operator_strategy_revision: Option<String>,
}

pub struct StrategiesToml {
    pub full_set: Option<FullSetToml>,
    pub neg_risk: Option<NegRiskStrategyToml>,
}
```

Expose validated helpers such as:

```rust
impl<'a> AppLiveConfigView<'a> {
    pub fn operator_strategy_revision(&self) -> Option<&'a str> { ... }
    pub fn is_legacy_explicit_strategy_config(&self) -> bool { ... }
}
```

- [ ] **Step 4: Run the config tests to verify they pass**

Run:

```bash
cargo test -p config-schema -- --test-threads=1
cargo test -p app-live --test config -- --test-threads=1
```

Expected: PASS with both neutral and compatibility config shapes loading correctly.

- [ ] **Step 5: Commit**

```bash
git add crates/config-schema/src/raw.rs crates/config-schema/src/validate.rs crates/config-schema/src/lib.rs crates/config-schema/tests/config_roundtrip.rs crates/config-schema/tests/validated_views.rs crates/config-schema/tests/fixtures/app-live-live.toml crates/config-schema/tests/fixtures/app-live-smoke.toml crates/config-schema/tests/fixtures/app-live-ux-smoke.toml crates/app-live/tests/config.rs
git commit -m "feat: add neutral strategy control config schema"
```

### Task 3: Introduce Route Adapters and Route-Neutral Execution Seams

**Files:**
- Create: `crates/app-live/src/strategy_control.rs`
- Create: `crates/app-live/src/route_adapters/mod.rs`
- Create: `crates/app-live/src/route_adapters/fullset.rs`
- Create: `crates/app-live/src/route_adapters/negrisk.rs`
- Modify: `crates/app-live/src/lib.rs`
- Modify: `crates/execution/src/providers.rs`
- Modify: `crates/execution/src/sink.rs`
- Modify: `crates/execution/src/plans.rs`
- Create: `crates/app-live/tests/strategy_control.rs`

- [ ] **Step 1: Write the failing adapter and live-submit tests**

Add tests proving the live path is no longer hardcoded to `NegRiskSubmitFamily`:

```rust
#[test]
fn live_route_registry_exposes_fullset_and_negrisk_adapters() {
    let registry = route_registry_for_tests();
    assert!(registry.adapter("full-set").is_some());
    assert!(registry.adapter("neg-risk").is_some());
}

#[test]
fn live_sink_rejects_missing_route_execution_adapter() {
    let sink = LiveVenueSink::noop();
    let attempt = sample_attempt_context("full-set", "default", ExecutionMode::Live);
    let err = sink.execute(&ExecutionPlan::FullSetBuyThenMerge { condition_id: "condition-1".into() }, &attempt).unwrap_err();
    assert!(err.to_string().contains("execution adapter"));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
cargo test -p app-live --test strategy_control live_route_registry_exposes_fullset_and_negrisk_adapters -- --exact
cargo test -p execution live_sink_rejects_missing_route_execution_adapter -- --exact
```

Expected: FAIL because the neutral route registry and route-owned live submit seams do not exist yet.

- [ ] **Step 3: Add the adapter traits and route-neutral execution wiring**

Create route-neutral traits and move strategy-shaped submit details behind adapters:

```rust
pub trait RouteAdapter {
    fn route(&self) -> &'static str;
    fn runtime_artifacts(&self, revision: &OperatorStrategyRevision) -> Result<Vec<RouteRuntimeArtifact>, String>;
    fn readiness(&self, ctx: &RouteReadinessContext) -> RouteReadinessReport;
}

pub trait RouteExecutionAdapter {
    fn submit_live(&self, plan: &ExecutionPlan, attempt: &ExecutionAttemptContext) -> Result<ExecutionReceipt, VenueSinkError>;
}
```

- [ ] **Step 4: Run the adapter tests to verify they pass**

Run:

```bash
cargo test -p app-live --test strategy_control -- --test-threads=1
cargo test -p execution -- --test-threads=1
```

Expected: PASS with explicit route adapters registered and the live sink no longer pretending non-neg-risk plans succeeded without route-owned handling.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/strategy_control.rs crates/app-live/src/route_adapters/mod.rs crates/app-live/src/route_adapters/fullset.rs crates/app-live/src/route_adapters/negrisk.rs crates/app-live/src/lib.rs crates/execution/src/providers.rs crates/execution/src/sink.rs crates/execution/src/plans.rs crates/app-live/tests/strategy_control.rs
git commit -m "feat: add route adapters and neutral execution seams"
```

### Task 4: Convert `discover` to Route-Local Candidate Production and Deterministic Bundling

**Files:**
- Modify: `crates/app-live/src/discovery.rs`
- Modify: `crates/app-live/src/commands/discover.rs`
- Modify: `crates/app-live/src/queues.rs`
- Modify: `crates/app-live/tests/discover_command.rs`
- Modify: `crates/app-live/tests/discovery_supervisor.rs`
- Modify: `crates/app-live/tests/candidate_daemon.rs`
- Modify: `crates/app-live/tests/support/discover_db.rs`
- Modify: `crates/persistence/tests/phase3e_candidate_generation.rs`

- [ ] **Step 1: Write the failing discover tests**

Add tests for route-local diffs and no-op rediscovery:

```rust
#[test]
fn discover_reports_route_local_diffs_without_rehashing_unchanged_routes() {
    let first = run_discover_fixture("neg-risk changed");
    let second = run_discover_fixture("neg-risk changed again but full-set unchanged");

    assert_ne!(first.recommended_adoptable_revision, second.recommended_adoptable_revision);
    assert_eq!(first.fullset_artifact_digest, second.fullset_artifact_digest);
}

#[test]
fn discover_ignores_readiness_only_changes_for_bundle_identity() {
    let first = discover_bundle_id_with_warning("connectivity-ok");
    let second = discover_bundle_id_with_warning("connectivity-degraded");
    assert_eq!(first, second);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
cargo test -p app-live --test discover_command discover_reports_route_local_diffs_without_rehashing_unchanged_routes -- --exact
cargo test -p app-live --test discovery_supervisor discover_ignores_readiness_only_changes_for_bundle_identity -- --exact
```

Expected: FAIL because discover is still `neg-risk`-only and bundle identity is not deterministic across route-local carry-forward.

- [ ] **Step 3: Implement route-local candidate producers and the neutral bundler**

Make `discover` orchestrate both routes:

```rust
let route_candidates = vec![
    fullset_adapter.produce_candidates(&ctx)?,
    negrisk_adapter.produce_candidates(&ctx)?,
];
let bundle = StrategyBundleBuilder::new("bundle-v1").bundle(route_candidates)?;
```

Keep readiness warnings out of candidate content and artifact digests.

- [ ] **Step 4: Run the discover tests to verify they pass**

Run:

```bash
cargo test -p app-live --test discover_command -- --test-threads=1
cargo test -p app-live --test discovery_supervisor -- --test-threads=1
cargo test -p persistence --test phase3e_candidate_generation -- --test-threads=1
```

Expected: PASS with deterministic bundle identity and route-local carry-forward behavior.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/discovery.rs crates/app-live/src/commands/discover.rs crates/app-live/src/queues.rs crates/app-live/tests/discover_command.rs crates/app-live/tests/discovery_supervisor.rs crates/app-live/tests/candidate_daemon.rs crates/app-live/tests/support/discover_db.rs crates/persistence/tests/phase3e_candidate_generation.rs
git commit -m "feat: bundle route-local strategy candidates"
```

### Task 5: Convert Startup, `run`, and Smoke Semantics to Neutral Startup Bundles

**Files:**
- Modify: `crates/app-live/src/config.rs`
- Modify: `crates/app-live/src/startup.rs`
- Modify: `crates/app-live/src/daemon.rs`
- Modify: `crates/app-live/src/runtime.rs`
- Modify: `crates/app-live/src/smoke.rs`
- Modify: `crates/app-live/src/commands/run.rs`
- Modify: `crates/risk/src/activation.rs`
- Modify: `crates/risk/src/rollout.rs`
- Modify: `crates/app-live/tests/startup_resolution.rs`
- Modify: `crates/app-live/tests/run_command.rs`
- Modify: `crates/app-live/tests/real_user_shadow_smoke.rs`
- Modify: `crates/app-live/tests/daemon_lifecycle.rs`

- [ ] **Step 1: Write the failing startup and smoke tests**

Add tests for multi-route startup and all-route shadow clamp:

```rust
#[tokio::test]
async fn adopted_strategy_revision_can_resolve_fullset_and_negrisk_artifacts() {
    let db = TestDatabase::new();
    db.seed_adopted_strategy_revision("strategy-rev-12", sample_multi_route_revision());

    let resolved = resolve_startup_strategy_revision(&db.pool, &sample_live_view("strategy-rev-12"))
        .await
        .unwrap();

    assert!(resolved.route_artifacts.contains_key("full-set"));
    assert!(resolved.route_artifacts.contains_key("neg-risk"));
}

#[test]
fn smoke_mode_clamps_all_risk_expanding_routes_to_shadow() {
    let policy = ActivationPolicy::phase_one_defaults().with_real_user_shadow_smoke();
    assert_eq!(policy.mode_for_route("full-set", "default"), ExecutionMode::Shadow);
    assert_eq!(policy.mode_for_route("neg-risk", "family-a"), ExecutionMode::Shadow);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
cargo test -p app-live --test startup_resolution adopted_strategy_revision_can_resolve_fullset_and_negrisk_artifacts -- --exact
cargo test -p risk smoke_mode_clamps_all_risk_expanding_routes_to_shadow -- --exact
```

Expected: FAIL because startup only resolves `NegRiskLiveTargetSet` and smoke only clamps `neg-risk`.

- [ ] **Step 3: Implement neutral startup bundles and route-neutral smoke**

Add a neutral startup shape:

```rust
pub struct ResolvedStrategyRevision {
    pub operator_strategy_revision: Option<String>,
    pub route_artifacts: BTreeMap<String, Vec<RouteRuntimeArtifact>>,
    pub compatibility_mode: bool,
}
```

Change smoke handling so every risk-expanding route is shadowed in smoke mode.

- [ ] **Step 4: Run the startup and run tests to verify they pass**

Run:

```bash
cargo test -p app-live --test startup_resolution -- --test-threads=1
cargo test -p app-live --test run_command -- --test-threads=1
cargo test -p app-live --test real_user_shadow_smoke -- --test-threads=1
cargo test -p app-live --test daemon_lifecycle -- --test-threads=1
```

Expected: PASS with neutral startup bundles and route-neutral smoke semantics.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/config.rs crates/app-live/src/startup.rs crates/app-live/src/daemon.rs crates/app-live/src/runtime.rs crates/app-live/src/smoke.rs crates/app-live/src/commands/run.rs crates/risk/src/activation.rs crates/risk/src/rollout.rs crates/app-live/tests/startup_resolution.rs crates/app-live/tests/run_command.rs crates/app-live/tests/real_user_shadow_smoke.rs crates/app-live/tests/daemon_lifecycle.rs
git commit -m "feat: resolve neutral startup bundles"
```

### Task 6: Convert `targets` to Neutral Adoption and Compatibility Migration

**Files:**
- Modify: `crates/app-live/src/commands/targets/adopt.rs`
- Modify: `crates/app-live/src/commands/targets/candidates.rs`
- Modify: `crates/app-live/src/commands/targets/config_file.rs`
- Modify: `crates/app-live/src/commands/targets/rollback.rs`
- Modify: `crates/app-live/src/commands/targets/show_current.rs`
- Modify: `crates/app-live/src/commands/targets/state.rs`
- Modify: `crates/app-live/src/commands/targets/status.rs`
- Modify: `crates/app-live/tests/targets_config_file.rs`
- Modify: `crates/app-live/tests/targets_read_commands.rs`
- Modify: `crates/app-live/tests/targets_write_commands.rs`
- Modify: `crates/persistence/tests/operator_target_adoption.rs`
- Modify: `crates/app-live/tests/support/strategy_control_db.rs`

- [ ] **Step 1: Write the failing `targets` tests**

Add tests for synthetic first revision and rollback gating:

```rust
#[test]
fn targets_adopt_migrates_legacy_explicit_config_into_first_neutral_revision() {
    let db = TestDatabase::new();
    let config_path = db.legacy_explicit_config_path();

    let output = run_targets_adopt(&config_path, &["--adopt-compatibility"]);
    assert!(output.contains("operator_strategy_revision = strategy-rev-"));
    assert!(output.contains("migration_source = legacy-explicit"));
}

#[test]
fn targets_rollback_rejects_compatibility_mode_without_neutral_history() {
    let db = TestDatabase::new();
    let err = run_targets_rollback(&db.legacy_explicit_config_path()).unwrap_err();
    assert!(err.to_string().contains("neutral adoption history"));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
cargo test -p app-live --test targets_write_commands targets_adopt_migrates_legacy_explicit_config_into_first_neutral_revision -- --exact
cargo test -p app-live --test targets_write_commands targets_rollback_rejects_compatibility_mode_without_neutral_history -- --exact
```

Expected: FAIL because `targets` still operates on `operator_target_revision` and cannot synthesize neutral revisions from compatibility inputs.

- [ ] **Step 3: Implement neutral `targets` lineage and config rewrites**

Persist neutral adoption rows and rewrite config to `[strategy_control]`:

```rust
pub struct ResolvedStrategyAdoptionSelection {
    pub operator_strategy_revision: String,
    pub adoptable_revision: Option<String>,
    pub migration_source: Option<String>,
}
```

```rust
rewrite_operator_strategy_revision(path, "strategy-rev-12")?;
```

- [ ] **Step 4: Run the `targets` tests to verify they pass**

Run:

```bash
cargo test -p app-live --test targets_write_commands -- --test-threads=1
cargo test -p app-live --test targets_read_commands -- --test-threads=1
cargo test -p app-live --test targets_config_file -- --test-threads=1
cargo test -p persistence --test operator_target_adoption -- --test-threads=1
```

Expected: PASS with compatibility migration, neutral adoption history, and rollback gating working as specified.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/targets/adopt.rs crates/app-live/src/commands/targets/candidates.rs crates/app-live/src/commands/targets/config_file.rs crates/app-live/src/commands/targets/rollback.rs crates/app-live/src/commands/targets/show_current.rs crates/app-live/src/commands/targets/state.rs crates/app-live/src/commands/targets/status.rs crates/app-live/tests/targets_config_file.rs crates/app-live/tests/targets_read_commands.rs crates/app-live/tests/targets_write_commands.rs crates/persistence/tests/operator_target_adoption.rs crates/app-live/tests/support/strategy_control_db.rs
git commit -m "feat: migrate targets commands to neutral revisions"
```

### Task 7: Convert `status`, `doctor`, and `apply` to Neutral Readiness Reporting

**Files:**
- Modify: `crates/app-live/src/commands/status/model.rs`
- Modify: `crates/app-live/src/commands/status/evaluate.rs`
- Modify: `crates/app-live/src/commands/status/mod.rs`
- Modify: `crates/app-live/src/commands/doctor/mod.rs`
- Modify: `crates/app-live/src/commands/doctor/report.rs`
- Modify: `crates/app-live/src/commands/doctor/target_source.rs`
- Modify: `crates/app-live/src/commands/apply/mod.rs`
- Modify: `crates/app-live/src/commands/apply/model.rs`
- Modify: `crates/app-live/src/commands/apply/output.rs`
- Modify: `crates/app-live/tests/status_command.rs`
- Modify: `crates/app-live/tests/doctor_command.rs`
- Modify: `crates/app-live/tests/apply_command.rs`
- Modify: `crates/app-live/tests/support/status_db.rs`
- Modify: `crates/app-live/tests/support/apply_db.rs`

- [ ] **Step 1: Write the failing operator UX tests**

Add tests for route-level readiness and compatibility-mode guidance:

```rust
#[test]
fn status_reports_compatibility_mode_explicitly() {
    let output = run_status(&legacy_explicit_config());
    assert!(output.contains("target source: compatibility"));
    assert!(output.contains("run targets adopt"));
}

#[test]
fn apply_refuses_compatibility_mode_auto_migration() {
    let err = run_apply(&legacy_explicit_config()).unwrap_err();
    assert!(err.to_string().contains("migrate via targets adopt"));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
cargo test -p app-live --test status_command status_reports_compatibility_mode_explicitly -- --exact
cargo test -p app-live --test apply_command apply_refuses_compatibility_mode_auto_migration -- --exact
```

Expected: FAIL because readiness and next actions are still `neg-risk`-shaped and compatibility mode is not a first-class branch.

- [ ] **Step 3: Implement neutral readiness summaries**

Add neutral summary shapes:

```rust
pub struct StatusRouteReadiness {
    pub route: String,
    pub scope: String,
    pub readiness: String,
    pub reason: Option<String>,
}
```

Ensure `apply` stops with explicit migration guidance in compatibility mode.

- [ ] **Step 4: Run the operator UX tests to verify they pass**

Run:

```bash
cargo test -p app-live --test status_command -- --test-threads=1
cargo test -p app-live --test doctor_command -- --test-threads=1
cargo test -p app-live --test apply_command -- --test-threads=1
```

Expected: PASS with route-neutral readiness output and explicit compatibility-mode behavior.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/status/model.rs crates/app-live/src/commands/status/evaluate.rs crates/app-live/src/commands/status/mod.rs crates/app-live/src/commands/doctor/mod.rs crates/app-live/src/commands/doctor/report.rs crates/app-live/src/commands/doctor/target_source.rs crates/app-live/src/commands/apply/mod.rs crates/app-live/src/commands/apply/model.rs crates/app-live/src/commands/apply/output.rs crates/app-live/tests/status_command.rs crates/app-live/tests/doctor_command.rs crates/app-live/tests/apply_command.rs crates/app-live/tests/support/status_db.rs crates/app-live/tests/support/apply_db.rs
git commit -m "feat: neutralize operator readiness commands"
```

### Task 8: Convert `verify` to Route-Aware Evidence Aggregation

**Files:**
- Modify: `crates/app-live/src/commands/verify/context.rs`
- Modify: `crates/app-live/src/commands/verify/evidence.rs`
- Modify: `crates/app-live/src/commands/verify/model.rs`
- Modify: `crates/app-live/src/commands/verify/mod.rs`
- Modify: `crates/app-live/src/commands/verify/session.rs`
- Modify: `crates/app-live/tests/verify_command.rs`
- Modify: `crates/app-live/tests/support/verify_db.rs`
- Modify: `crates/app-live/src/route_adapters/fullset.rs`
- Modify: `crates/app-live/src/route_adapters/negrisk.rs`

- [ ] **Step 1: Write the failing verify tests**

Add tests for multi-route session verdicts:

```rust
#[test]
fn verify_fails_if_any_active_route_fails() {
    let report = run_verify_fixture("fullset-pass-negrisk-fail");
    assert!(report.contains("verdict = fail"));
    assert!(report.contains("route full-set = pass"));
    assert!(report.contains("route neg-risk = fail"));
}

#[test]
fn verify_smoke_rejects_live_attempts_from_any_risk_expanding_route() {
    let err = run_verify_fixture("smoke-fullset-live-attempt");
    assert!(err.contains("credible live strategy attempts"));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
cargo test -p app-live --test verify_command verify_fails_if_any_active_route_fails -- --exact
cargo test -p app-live --test verify_command verify_smoke_rejects_live_attempts_from_any_risk_expanding_route -- --exact
```

Expected: FAIL because `verify` still hardcodes `neg-risk` as the route of interest.

- [ ] **Step 3: Implement route-aware verify aggregation**

Model the report per route:

```rust
pub struct VerifyRouteReport {
    pub route: String,
    pub verdict: VerifyVerdict,
    pub evidence: VerifyResultEvidence,
}
```

Aggregate route verdicts into one session verdict exactly as the spec requires.

- [ ] **Step 4: Run the verify tests to verify they pass**

Run:

```bash
cargo test -p app-live --test verify_command -- --test-threads=1
```

Expected: PASS with route-aware session aggregation and smoke invariants enforced across all risk-expanding routes.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/verify/context.rs crates/app-live/src/commands/verify/evidence.rs crates/app-live/src/commands/verify/model.rs crates/app-live/src/commands/verify/mod.rs crates/app-live/src/commands/verify/session.rs crates/app-live/tests/verify_command.rs crates/app-live/tests/support/verify_db.rs crates/app-live/src/route_adapters/fullset.rs crates/app-live/src/route_adapters/negrisk.rs
git commit -m "feat: aggregate verify evidence by strategy route"
```

### Task 9: Update Docs, Examples, and Run the Full Regression Set

**Files:**
- Modify: `README.md`
- Modify: `config/axiom-arb.example.toml`
- Modify: `docs/runbooks/real-user-shadow-smoke.md`
- Modify: `crates/app-live/tests/main_entrypoint.rs`

- [ ] **Step 1: Write the failing doc and smoke-regression checks**

Add or update tests/assertions for the new terminology and example config:

```rust
#[test]
fn example_config_mentions_strategy_control_anchor() {
    let text = std::fs::read_to_string("config/axiom-arb.example.toml").unwrap();
    assert!(text.contains("[strategy_control]"));
    assert!(text.contains("operator_strategy_revision"));
}
```

- [ ] **Step 2: Run the checks to verify they fail**

Run:

```bash
cargo test -p app-live --test main_entrypoint -- --test-threads=1
rg -n "\\[strategy_control\\]|operator_strategy_revision" README.md config/axiom-arb.example.toml docs/runbooks/real-user-shadow-smoke.md
```

Expected: tests or grep checks reveal the docs and examples still use the old target-centric language.

- [ ] **Step 3: Update docs and examples**

Refresh docs to reflect:

```toml
[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-12"
```

Also document:

- compatibility-mode read-only behavior
- explicit migration via `app-live targets adopt`
- all-route smoke shadowing

- [ ] **Step 4: Run the full regression set**

Run:

```bash
cargo fmt --all --check
cargo test -p persistence -- --test-threads=1
cargo test -p config-schema -- --test-threads=1
cargo test -p execution -- --test-threads=1
cargo test -p risk -- --test-threads=1
cargo test -p app-live -- --test-threads=1
cargo clippy -p persistence -p config-schema -p execution -p risk -p app-live --all-targets -- -D warnings
```

Expected: PASS across formatting, targeted crates, and linting with no route-specific regressions.

- [ ] **Step 5: Commit**

```bash
git add README.md config/axiom-arb.example.toml docs/runbooks/real-user-shadow-smoke.md crates/app-live/tests/main_entrypoint.rs
git commit -m "docs: document neutral strategy control plane"
```
