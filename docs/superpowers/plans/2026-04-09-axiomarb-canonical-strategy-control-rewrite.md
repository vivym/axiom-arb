# AxiomArb Canonical Strategy-Control Rewrite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution discipline:** Use `@superpowers:test-driven-development` for every task and `@superpowers:verification-before-completion` before claiming completion.

**Goal:** Remove legacy explicit-target compatibility mode entirely, make `[strategy_control]` the only live/smoke control-plane shape, auto-migrate old config only at explicit mutation boundaries, and simplify runtime/readiness/operator UX around a single canonical strategy-control resolver.

**Architecture:** Split control-plane handling into a pure read-only `strategy_control::resolver` and a write-owning `strategy_control::migration`. The resolver becomes the single authority for `status`, `doctor`, `bootstrap`, `apply`, `verify`, `run`, `startup`, and `targets`; mutation-boundary commands may call the migration helper to rewrite legacy config into canonical `[strategy_control]`, then re-resolve and continue. Legacy `[[negrisk.targets]]`, `[negrisk.target_source]`, `operator_target_revision`, compatibility mode, and `--adopt-compatibility` are removed from steady-state config, command UX, and tests.

**Tech Stack:** Rust, `config-schema`, `app-live`, SQLx/Postgres persistence repos, TOML editing via `toml_edit`, `cargo test`, `cargo fmt`, `cargo clippy`

---

## Preconditions

- Execute this plan in a dedicated worktree, not on `main`.
- Read the controlling spec first:
  - `docs/superpowers/specs/2026-04-09-axiomarb-canonical-strategy-control-rewrite-design.md`
- Keep this slice focused:
  - do not redesign route adapters
  - do not redesign Polymarket transport/auth
  - do not introduce a second migration path outside the resolver/migration split
- Preserve the existing persistence schema unless a step explicitly calls for a read-path or upsert-path adjustment; this rewrite is about canonical resolution and config migration, not a broad DB rename.

## File Map

### Canonical control-plane core

- `crates/config-schema/src/raw.rs`
  - Keep legacy fields deserializable, but stop treating them as steady-state aliases for app-facing control-plane access.
- `crates/config-schema/src/validate.rs`
  - Remove compatibility-oriented helper methods from validated app-live views and expose only canonical strategy-control facts plus minimal legacy-shape detection needed by resolver/migration.
- `crates/app-live/src/strategy_control/mod.rs`
  - New module entrypoint; re-export canonical resolver and route-scope validation first, then add migration exports in Task 2.
- `crates/app-live/src/strategy_control/route_registry.rs`
  - Move the existing route validation helpers out of the old single-file module.
- `crates/app-live/src/strategy_control/resolver.rs`
  - Pure control-plane read model: classify canonical vs migratable vs invalid, enforce precedence across config/persistence/runtime-progress, and return canonical strategy-control state.
- `crates/app-live/src/strategy_control/migration.rs`
  - Own all legacy-to-canonical conversion, canonical persistence materialization, and config rewrite orchestration.
- `crates/app-live/src/lib.rs`
  - Export the restructured `strategy_control` module surface.

### Config rewrite and target-command entrypoints

- `crates/app-live/src/cli.rs`
  - Remove `--adopt-compatibility` and any remaining `operator-target-revision`-shaped live control-plane UX that is no longer steady-state.
- `crates/app-live/src/commands/targets/config_file.rs`
  - Become the low-level TOML rewrite utility used by `strategy_control::migration`; remove target-shaped steady-state rewrite helpers.
- `crates/app-live/src/commands/targets/adopt.rs`
  - Make `targets adopt --config <config>` the sole explicit migration entrypoint for migratable legacy input.
- `crates/app-live/src/commands/targets/rollback.rs`
- `crates/app-live/src/commands/targets/status.rs`
- `crates/app-live/src/commands/targets/show_current.rs`
- `crates/app-live/src/commands/targets/candidates.rs`
- `crates/app-live/src/commands/targets/state.rs`
  - Remove compatibility-aware summaries, rollback behavior, and target-source fallback logic; everything reads canonical resolver state.

### Startup, runtime, and read-only command consumers

- `crates/app-live/src/startup.rs`
  - Consume only canonical adopted strategy-control state; fail closed when migration is still required.
- `crates/app-live/src/commands/run.rs`
  - Refuse migratable legacy input; no implicit migration side path.
- `crates/app-live/src/runtime.rs`
- `crates/app-live/src/run_session.rs`
- `crates/app-live/src/supervisor.rs`
- `crates/app-live/src/source_tasks.rs`
- `crates/app-live/src/daemon.rs`
  - Route all live/smoke control-plane reads through the canonical resolver contract.
- `crates/app-live/src/commands/status/model.rs`
- `crates/app-live/src/commands/status/evaluate.rs`
- `crates/app-live/src/commands/status/mod.rs`
- `crates/app-live/src/commands/doctor/target_source.rs`
- `crates/app-live/src/commands/doctor/connectivity.rs`
- `crates/app-live/src/commands/bootstrap/flow.rs`
- `crates/app-live/src/commands/bootstrap/error.rs`
- `crates/app-live/src/commands/apply/*.rs`
- `crates/app-live/src/commands/verify/model.rs`
- `crates/app-live/src/commands/verify/*.rs`
  - Remove compatibility-specific branching, blocked reasons, and remediation text.

### Init, docs, and operator-facing config output

- `crates/app-live/src/commands/init/wizard.rs`
- `crates/app-live/src/commands/init/render.rs`
- `crates/app-live/src/commands/init/summary.rs`
  - Emit only canonical `[strategy_control]`; preserve mode must rewrite old control-plane shapes rather than carry them forward.
- `config/axiom-arb.example.toml`
- `README.md`
- `docs/runbooks/real-user-shadow-smoke.md`
- `docs/runbooks/operator-target-adoption.md`
  - Rewrite operator guidance so compatibility mode and target-shaped control-plane anchors disappear from normal UX.

### Tests and fixtures

- Create: `crates/app-live/tests/strategy_control_resolver.rs`
- Create: `crates/app-live/tests/strategy_control_migration.rs`
- Modify: `crates/config-schema/tests/validated_views.rs`
- Modify: `crates/config-schema/tests/config_roundtrip.rs`
- Modify: `crates/app-live/tests/status_command.rs`
- Modify: `crates/app-live/tests/doctor_command.rs`
- Modify: `crates/app-live/tests/bootstrap_command.rs`
- Modify: `crates/app-live/tests/apply_command.rs`
- Modify: `crates/app-live/tests/run_command.rs`
- Modify: `crates/app-live/tests/verify_command.rs`
- Modify: `crates/app-live/tests/init_command.rs`
- Modify: `crates/app-live/tests/startup_resolution.rs`
- Modify: `crates/app-live/tests/targets_read_commands.rs`
- Modify: `crates/app-live/tests/targets_write_commands.rs`
- Modify: `crates/app-live/tests/targets_config_file.rs`
- Modify: `crates/app-live/tests/real_user_shadow_smoke.rs`
- Modify: startup/targets-related support fixtures under `crates/app-live/tests/support/*`
- Modify: any config-schema or app-live fixture that still encodes `[[negrisk.targets]]`, `[negrisk.target_source]`, or `operator_target_revision` as a normal live/smoke shape

---

### Task 1: Build The Canonical Strategy-Control Core

**Files:**
- Modify: `crates/config-schema/src/raw.rs`
- Modify: `crates/config-schema/src/validate.rs`
- Delete/Move: `crates/app-live/src/strategy_control.rs`
- Create: `crates/app-live/src/strategy_control/mod.rs`
- Create: `crates/app-live/src/strategy_control/route_registry.rs`
- Create: `crates/app-live/src/strategy_control/resolver.rs`
- Modify: `crates/app-live/src/lib.rs`
- Test: `crates/config-schema/tests/validated_views.rs`
- Test: `crates/config-schema/tests/config_roundtrip.rs`
- Test: `crates/app-live/tests/strategy_control_resolver.rs`

- [ ] **Step 1: Write the failing resolver/config-schema tests**

Add tests that lock the steady-state control-plane truth:

```rust
#[test]
fn canonical_strategy_control_resolves_without_legacy_aliases() {
    let resolved = resolve_strategy_control(test_state().with_config(
        r#"
[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-12"
"#,
    ))
    .unwrap();

    assert_eq!(
        resolved.operator_strategy_revision,
        "strategy-rev-12"
    );
}

#[test]
fn explicit_targets_are_not_reported_as_steady_state_control_plane() {
    let result = resolve_strategy_control(test_state().with_config(
        r#"
[[negrisk.targets]]
family_id = "family-a"

[[negrisk.targets.members]]
condition_id = "condition-1"
token_id = "token-1"
price = "0.43"
quantity = "5"
"#,
    ));

    assert!(matches!(result, Err(ResolveStrategyControlError::MigrationRequired(_))));
}

#[test]
fn mixed_canonical_and_legacy_input_fails_closed() {
    let result = resolve_strategy_control(test_state().with_config(
        r#"
[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-12"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"
"#,
    ));

    assert!(matches!(result, Err(ResolveStrategyControlError::InvalidConfig(_))));
}

#[test]
fn empty_targets_array_is_invalid_legacy_input() {
    let result = resolve_strategy_control(test_state().with_config(
        r#"
[negrisk]
targets = []
"#,
    ));

    assert!(matches!(result, Err(ResolveStrategyControlError::InvalidConfig(_))));
}

#[test]
fn canonical_config_without_matching_canonical_persistence_fails_closed() {
    let result = resolve_strategy_control(test_state().with_config(
        r#"
[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-missing"
"#,
    ));

    assert!(matches!(
        result,
        Err(ResolveStrategyControlError::MissingCanonicalPersistence(_))
    ));
}

#[test]
fn malformed_legacy_target_source_is_invalid_config() {
    let result = resolve_strategy_control(test_state().with_config(
        r#"
[negrisk.target_source]
source = "adopted"
"#,
    ));

    assert!(matches!(result, Err(ResolveStrategyControlError::InvalidConfig(_))));
}

#[test]
fn conflicting_runtime_progress_does_not_override_configured_strategy_revision() {
    let resolved = resolve_strategy_control(
        test_state()
            .with_config(
                r#"
[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-12"
"#,
            )
            .with_runtime_progress("strategy-rev-99"),
    )
    .unwrap();

    assert_eq!(resolved.operator_strategy_revision, "strategy-rev-12");
    assert_eq!(resolved.restart_needed, Some(true));
}

#[test]
fn validated_live_view_exposes_only_canonical_strategy_revision() {
    let live = load_app_live_view(
        r#"
[runtime]
mode = "live"

[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-12"
"#,
    );

    assert_eq!(live.operator_strategy_revision(), Some("strategy-rev-12"));
    assert!(!live.has_target_source());
}
```

- [ ] **Step 2: Run focused tests to verify they fail**

Run:

```bash
cargo test -p config-schema --test validated_views --test config_roundtrip -- --test-threads=1
cargo test -p app-live --test strategy_control_resolver -- --test-threads=1
```

Expected: FAIL because validated views still expose target-source compatibility helpers and `app-live` has no canonical resolver module yet.

- [ ] **Step 3: Create the new `strategy_control` module structure**

Move the existing route helpers into `route_registry.rs` and add a real module surface:

```rust
// crates/app-live/src/strategy_control/mod.rs
mod resolver;
mod route_registry;

pub use resolver::{
    resolve_strategy_control, CanonicalStrategyControlState, MigrationRequiredReason,
    ResolveStrategyControlError,
};
pub use route_registry::{live_route_registry, validate_live_route_scope};
```

- [ ] **Step 4: Implement the pure resolver contract**

Implement a read-only resolver that:

- reads canonical config first
- classifies old `[negrisk.target_source]` and `[[negrisk.targets]]` as migratable or invalid
- refuses mixed canonical + legacy input
- never rewrites config
- never materializes persistence

The shape should read like:

```rust
pub enum ResolveStrategyControlError {
    MigrationRequired(MigrationRequiredReason),
    InvalidConfig(String),
    MissingCanonicalPersistence(String),
}

pub fn resolve_strategy_control(
    state: StrategyControlResolutionInput<'_>,
) -> Result<CanonicalStrategyControlState, ResolveStrategyControlError> {
    match classify_control_plane_input(state.config)? {
        ControlPlaneInput::Canonical(canonical) => resolve_canonical(canonical, state.persistence),
        ControlPlaneInput::Legacy(legacy) => Err(ResolveStrategyControlError::MigrationRequired(
            MigrationRequiredReason::from(legacy),
        )),
        ControlPlaneInput::Invalid(message) => Err(ResolveStrategyControlError::InvalidConfig(message)),
    }
}
```

- [ ] **Step 5: Strip compatibility helpers out of validated app-live views**

Change validated config accessors so app-facing code stops treating legacy target-source aliases as steady-state truth. Keep only the minimal raw access the resolver/migration layer needs for classification.

- [ ] **Step 6: Re-run the focused tests**

Run:

```bash
cargo test -p config-schema --test validated_views --test config_roundtrip -- --test-threads=1
cargo test -p app-live --test strategy_control_resolver -- --test-threads=1
```

Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/config-schema/src/raw.rs \
  crates/config-schema/src/validate.rs \
  crates/app-live/src/strategy_control.rs \
  crates/app-live/src/strategy_control/mod.rs \
  crates/app-live/src/strategy_control/route_registry.rs \
  crates/app-live/src/strategy_control/resolver.rs \
  crates/app-live/src/lib.rs \
  crates/config-schema/tests/validated_views.rs \
  crates/config-schema/tests/config_roundtrip.rs \
  crates/app-live/tests/strategy_control_resolver.rs
git commit -m "refactor: add canonical strategy control resolver"
```

### Task 2: Implement Legacy-To-Canonical Migration And Config Rewrite

**Files:**
- Create: `crates/app-live/src/strategy_control/migration.rs`
- Modify: `crates/app-live/src/commands/targets/config_file.rs`
- Modify: `crates/app-live/src/commands/targets/adopt.rs`
- Modify: `crates/app-live/src/cli.rs`
- Modify: `crates/app-live/src/discovery.rs`
- Test: `crates/app-live/tests/strategy_control_migration.rs`
- Test: `crates/app-live/tests/targets_write_commands.rs`
- Test: `crates/app-live/tests/targets_config_file.rs`

- [ ] **Step 1: Write the failing migration tests**

Cover both legacy shapes and the deterministic digest contract:

```rust
#[tokio::test]
async fn adopt_without_selectors_migrates_legacy_target_source_to_strategy_control() {
    let harness = MigrationHarness::legacy_target_source("targets-rev-9");

    let outcome = adopt_selected_revision(
        &harness.pool,
        harness.config_path(),
        None,
        None,
    )
    .await
    .unwrap();

    assert_eq!(outcome.selection.migration_source.as_deref(), Some("legacy-target-source"));
    assert_rewritten_to_strategy_control(harness.config_path(), "strategy-rev-9");
}

#[tokio::test]
async fn adopt_without_selectors_migrates_explicit_targets_using_deterministic_digest() {
    let harness = MigrationHarness::legacy_explicit_targets();
    let outcome = adopt_selected_revision(
        &harness.pool,
        harness.config_path(),
        None,
        None,
    )
    .await
    .unwrap();

    assert!(outcome.selection.operator_strategy_revision.starts_with("strategy-rev-"));
    assert_rewritten_without_legacy_targets(harness.config_path());
}

#[tokio::test]
async fn empty_targets_array_is_not_migratable() {
    let harness = MigrationHarness::legacy_empty_targets();
    let error = adopt_selected_revision(&harness.pool, harness.config_path(), None, None)
        .await
        .unwrap_err();

    assert!(error.to_string().contains("targets = []"));
}
```

- [ ] **Step 2: Run the focused migration tests to verify they fail**

Run:

```bash
cargo test -p app-live --test strategy_control_migration --test targets_write_commands --test targets_config_file -- --test-threads=1
```

Expected: FAIL because `targets adopt` still requires selectors or `--adopt-compatibility`, and config rewrite logic still preserves target-shaped compatibility helpers.

- [ ] **Step 3: Implement `strategy_control::migration`**

Give migration a single public entrypoint that performs:

```rust
pub async fn migrate_legacy_strategy_control(
    pool: &PgPool,
    config_path: &Path,
    legacy: LegacyStrategyControlInput,
) -> Result<MigrationOutcome, MigrationError> {
    let canonical = derive_canonical_strategy_revision(pool, &legacy).await?;
    materialize_canonical_artifacts(pool, &legacy, &canonical).await?;
    rewrite_strategy_control_config(config_path, &canonical)?;
    Ok(MigrationOutcome { operator_strategy_revision: canonical })
}
```

It must:

- deterministically derive `strategy-rev-<digest>` from explicit targets
- materialize `strategy_candidate_sets`, `adoptable_strategy_revisions`, and `strategy_adoption_provenance`
- fail closed on contradictory, malformed, or non-unique lineage

- [ ] **Step 4: Replace `--adopt-compatibility` with implicit legacy migration in `targets adopt`**

Change `targets adopt` semantics to:

- canonical config: still require one selector
- migratable legacy config: `targets adopt --config <config>` with no selectors migrates
- no `--adopt-compatibility` flag anywhere in CLI or help text

- [ ] **Step 5: Rewrite config only into canonical `[strategy_control]`**

Update config rewrite helpers so they:

- write `[strategy_control]`
- delete `[[negrisk.targets]]`
- delete `[negrisk.target_source]`
- delete `operator_target_revision`
- preserve unrelated rollout/account/source settings

- [ ] **Step 6: Re-run focused migration tests**

Run:

```bash
cargo test -p app-live --test strategy_control_migration --test targets_write_commands --test targets_config_file -- --test-threads=1
```

Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/app-live/src/strategy_control/migration.rs \
  crates/app-live/src/commands/targets/config_file.rs \
  crates/app-live/src/commands/targets/adopt.rs \
  crates/app-live/src/cli.rs \
  crates/app-live/src/discovery.rs \
  crates/app-live/tests/strategy_control_migration.rs \
  crates/app-live/tests/targets_write_commands.rs \
  crates/app-live/tests/targets_config_file.rs
git commit -m "refactor: migrate legacy control plane to strategy control"
```

### Task 3: Convert Mutation-Boundary Commands To Detect, Migrate, Rewrite, Re-Resolve

**Files:**
- Modify: `crates/app-live/src/commands/bootstrap/flow.rs`
- Modify: `crates/app-live/src/commands/bootstrap/error.rs`
- Modify: `crates/app-live/src/commands/apply/*.rs`
- Modify: `crates/app-live/src/commands/init/wizard.rs`
- Modify: `crates/app-live/src/commands/init/render.rs`
- Modify: `crates/app-live/src/commands/init/summary.rs`
- Test: `crates/app-live/tests/bootstrap_command.rs`
- Test: `crates/app-live/tests/apply_command.rs`
- Test: `crates/app-live/tests/init_command.rs`

- [ ] **Step 1: Write the failing mutation-boundary tests**

Add tests that prove only mutation-boundary commands auto-rewrite:

```rust
#[test]
fn bootstrap_rewrites_legacy_control_plane_before_continuing() {
    let config = legacy_target_source_config_path();
    let output = run_bootstrap(&config);

    assert!(output.contains("migrated legacy control-plane config"));
    assert_rewritten_to_strategy_control(&config, "strategy-rev-9");
}

#[test]
fn apply_rewrites_legacy_explicit_targets_then_continues_canonically() {
    let config = legacy_explicit_targets_config_path();
    let output = run_apply(&config);

    assert!(output.contains("operator_strategy_revision"));
    assert!(!output.contains("compatibility mode"));
}

#[test]
fn init_preserve_rewrites_old_target_source_into_strategy_control() {
    let rendered = render_live_init_from_existing(legacy_target_source_config());

    assert!(rendered.contains("[strategy_control]"));
    assert!(!rendered.contains("[negrisk.target_source]"));
}
```

- [ ] **Step 2: Run the mutation-boundary test slice and verify it fails**

Run:

```bash
cargo test -p app-live --test bootstrap_command --test apply_command --test init_command -- --test-threads=1
```

Expected: FAIL because bootstrap/apply/init still preserve compatibility-specific paths and old target-shaped config output.

- [ ] **Step 3: Make `bootstrap` and `apply` follow the fixed sequence**

Implement:

`detect -> migrate -> rewrite -> re-resolve -> continue`

Do not let either command continue from partially migrated in-memory state.

- [ ] **Step 4: Rewrite init preserve/render/summary to canonical-only output**

Init must:

- emit `[strategy_control]` for live/smoke
- never emit `[negrisk.target_source]` as steady state
- drop old explicit targets/target_source when preserving
- keep unrelated config such as rollout/source overrides

- [ ] **Step 5: Re-run the mutation-boundary tests**

Run:

```bash
cargo test -p app-live --test bootstrap_command --test apply_command --test init_command -- --test-threads=1
```

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/app-live/src/commands/bootstrap/flow.rs \
  crates/app-live/src/commands/bootstrap/error.rs \
  crates/app-live/src/commands/apply \
  crates/app-live/src/commands/init/wizard.rs \
  crates/app-live/src/commands/init/render.rs \
  crates/app-live/src/commands/init/summary.rs \
  crates/app-live/tests/bootstrap_command.rs \
  crates/app-live/tests/apply_command.rs \
  crates/app-live/tests/init_command.rs
git commit -m "refactor: migrate legacy control plane at mutation boundaries"
```

### Task 4: Cut Read-Only Commands, Startup, And Runtime Over To Canonical Resolution

**Files:**
- Modify: `crates/app-live/src/startup.rs`
- Modify: `crates/app-live/src/commands/run.rs`
- Modify: `crates/app-live/src/runtime.rs`
- Modify: `crates/app-live/src/run_session.rs`
- Modify: `crates/app-live/src/supervisor.rs`
- Modify: `crates/app-live/src/source_tasks.rs`
- Modify: `crates/app-live/src/daemon.rs`
- Modify: `crates/app-live/src/commands/status/model.rs`
- Modify: `crates/app-live/src/commands/status/evaluate.rs`
- Modify: `crates/app-live/src/commands/status/mod.rs`
- Modify: `crates/app-live/src/commands/doctor/target_source.rs`
- Modify: `crates/app-live/src/commands/doctor/connectivity.rs`
- Modify: `crates/app-live/src/commands/verify/model.rs`
- Modify: `crates/app-live/src/commands/verify/*.rs`
- Modify: `crates/app-live/src/commands/targets/state.rs`
- Modify: `crates/app-live/src/commands/targets/status.rs`
- Modify: `crates/app-live/src/commands/targets/show_current.rs`
- Modify: `crates/app-live/src/commands/targets/candidates.rs`
- Modify: `crates/app-live/src/commands/targets/rollback.rs`
- Test: `crates/app-live/tests/status_command.rs`
- Test: `crates/app-live/tests/doctor_command.rs`
- Test: `crates/app-live/tests/verify_command.rs`
- Test: `crates/app-live/tests/run_command.rs`
- Test: `crates/app-live/tests/startup_resolution.rs`
- Test: `crates/app-live/tests/targets_read_commands.rs`
- Test: `crates/app-live/tests/real_user_shadow_smoke.rs`

- [ ] **Step 1: Write the failing read-only/runtime tests**

Add assertions like:

```rust
#[test]
fn status_reports_migration_required_without_compatibility_wording() {
    let output = run_status(legacy_target_source_config_path());

    assert!(output.contains("migration required"));
    assert!(!output.contains("compatibility"));
    assert!(!output.contains("--adopt-compatibility"));
}

#[test]
fn doctor_refuses_legacy_input_without_rewriting_config() {
    let config = legacy_explicit_targets_config_path();
    let before = std::fs::read_to_string(&config).unwrap();
    let output = run_doctor(&config);
    let after = std::fs::read_to_string(&config).unwrap();

    assert!(output.contains("app-live targets adopt --config"));
    assert_eq!(before, after);
}

#[test]
fn doctor_reports_rebuild_guidance_for_canonical_config_without_persistence() {
    let output = run_doctor(canonical_config_without_persistence());

    assert!(output.contains("rebuild canonical lineage"));
    assert!(!output.contains("app-live targets adopt --config"));
}

#[test]
fn run_fails_closed_when_legacy_migration_is_still_required() {
    let error = run_command(legacy_target_source_config_path()).unwrap_err();
    assert!(error.to_string().contains("migrate"));
}
```

- [ ] **Step 2: Run the focused command/runtime tests and verify they fail**

Run:

```bash
cargo test -p app-live --test status_command --test doctor_command --test verify_command --test run_command --test startup_resolution --test targets_read_commands --test real_user_shadow_smoke -- --test-threads=1
```

Expected: FAIL because compatibility mode still leaks through `status`, `doctor`, `verify`, `run`, `startup`, and `targets` read commands.

- [ ] **Step 3: Make `startup` and runtime consumers canonical-only**

`startup` must:

- call the resolver
- accept only `ResolvedCanonicalStrategyControl`
- refuse `MigrationRequired`
- stop deriving route artifacts from explicit targets as a steady-state path

All runtime/session/task helpers must consume the same resolved canonical state rather than inspecting raw config directly.

- [ ] **Step 4: Remove compatibility-specific status/doctor/verify/targets UX**

Delete or replace:

- `LegacyExplicitTargets`
- `MigrateLegacyExplicitTargets`
- `compatibility_mode = ...`
- compatibility-specific blocked reasons/actions
- compatibility-specific rollback or show-current summaries

Read-only remediation should collapse to:

```text
app-live targets adopt --config <config>
```

for migratable legacy input. Preserve the other two read-only branches from the spec:

- contradictory or malformed control-plane input => manual TOML repair guidance
- canonical `[strategy_control]` with missing or contradictory canonical persistence => rebuild canonical lineage guidance, not `targets adopt`

Do not let `status`, `doctor`, or `verify` route every closed state to the same next step.

- [ ] **Step 5: Re-run the focused command/runtime tests**

Run:

```bash
cargo test -p app-live --test status_command --test doctor_command --test verify_command --test run_command --test startup_resolution --test targets_read_commands --test real_user_shadow_smoke -- --test-threads=1
```

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/app-live/src/startup.rs \
  crates/app-live/src/commands/run.rs \
  crates/app-live/src/runtime.rs \
  crates/app-live/src/run_session.rs \
  crates/app-live/src/supervisor.rs \
  crates/app-live/src/source_tasks.rs \
  crates/app-live/src/daemon.rs \
  crates/app-live/src/commands/status \
  crates/app-live/src/commands/doctor \
  crates/app-live/src/commands/verify \
  crates/app-live/src/commands/targets/state.rs \
  crates/app-live/src/commands/targets/status.rs \
  crates/app-live/src/commands/targets/show_current.rs \
  crates/app-live/src/commands/targets/candidates.rs \
  crates/app-live/src/commands/targets/rollback.rs \
  crates/app-live/tests/status_command.rs \
  crates/app-live/tests/doctor_command.rs \
  crates/app-live/tests/verify_command.rs \
  crates/app-live/tests/run_command.rs \
  crates/app-live/tests/startup_resolution.rs \
  crates/app-live/tests/targets_read_commands.rs \
  crates/app-live/tests/real_user_shadow_smoke.rs
git commit -m "refactor: remove compatibility mode from runtime commands"
```

### Task 5: Rewrite Operator UX, Example Config, And Runbooks

**Files:**
- Modify: `config/axiom-arb.example.toml`
- Modify: `README.md`
- Modify: `docs/runbooks/real-user-shadow-smoke.md`
- Modify: `docs/runbooks/operator-target-adoption.md`
- Test: `crates/app-live/tests/main_entrypoint.rs`
- Test: `crates/app-live/tests/init_command.rs`

- [ ] **Step 1: Write the failing operator-UX tests**

Add assertions that the shipped artifacts are canonical-only:

```rust
#[test]
fn example_config_uses_strategy_control_and_no_target_source() {
    let text = std::fs::read_to_string("config/axiom-arb.example.toml").unwrap();
    assert!(text.contains("[strategy_control]"));
    assert!(!text.contains("[negrisk.target_source]"));
    assert!(!text.contains("operator_target_revision"));
}

#[test]
fn init_output_no_longer_mentions_compatibility_or_target_source() {
    let output = render_live_init(...);
    assert!(!output.contains("[negrisk.target_source]"));
    assert!(output.contains("[strategy_control]"));
}

#[test]
fn operator_docs_stop_teaching_compatibility_mode() {
    let readme = std::fs::read_to_string("README.md").unwrap();
    let smoke = std::fs::read_to_string("docs/runbooks/real-user-shadow-smoke.md").unwrap();
    let adoption = std::fs::read_to_string("docs/runbooks/operator-target-adoption.md").unwrap();

    assert!(!readme.contains("--adopt-compatibility"));
    assert!(!smoke.contains("compatibility mode"));
    assert!(!adoption.contains("[negrisk.target_source]"));
}
```

- [ ] **Step 2: Run the focused UX/docs tests and verify they fail**

Run:

```bash
cargo test -p app-live --test main_entrypoint --test init_command -- --test-threads=1
```

Expected: FAIL because the example config and init output still reference target-shaped live control-plane fields.

- [ ] **Step 3: Rewrite the operator artifacts**

Update:

- example config to show only canonical `[strategy_control]`
- README and runbooks to remove compatibility mode, explicit targets, and `--adopt-compatibility`
- operator-target-adoption runbook to document the canonical migration/remediation flow:
  - read-only commands report migration required
  - `targets adopt --config <config>` is the explicit migration entrypoint

- [ ] **Step 4: Re-run the focused UX/docs tests**

Run:

```bash
cargo test -p app-live --test main_entrypoint --test init_command -- --test-threads=1
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add config/axiom-arb.example.toml \
  README.md \
  docs/runbooks/real-user-shadow-smoke.md \
  docs/runbooks/operator-target-adoption.md \
  crates/app-live/tests/main_entrypoint.rs \
  crates/app-live/tests/init_command.rs
git commit -m "docs: adopt canonical strategy control terminology"
```

### Task 6: Sweep Remaining Fixtures And Run Full Verification

**Files:**
- Modify: remaining target/control-plane fixtures under `crates/config-schema/tests/*`
- Modify: remaining command/support fixtures under `crates/app-live/tests/*` and `crates/app-live/tests/support/*`
- Modify: any leftover dead compatibility helpers discovered during the full test pass

- [ ] **Step 1: Write the final fixture sweeps as failing assertions**

Add grep-style or direct fixture assertions where needed so the suite rejects leftover compatibility artifacts:

```rust
#[test]
fn no_live_fixture_uses_legacy_target_source_as_normal_shape() {
    for fixture in live_fixture_paths() {
        let text = std::fs::read_to_string(fixture).unwrap();
        assert!(!text.contains("[negrisk.target_source]"));
        assert!(!text.contains("operator_target_revision"));
    }
}
```

- [ ] **Step 2: Run the full targeted verification suite**

Run:

```bash
cargo test -p config-schema --test validated_views --test config_roundtrip -- --test-threads=1
cargo test -p app-live --lib -- --test-threads=1
cargo test -p app-live --tests -- --test-threads=1
```

Expected: FAIL until the remaining stale fixtures/support helpers are cleaned up.

- [ ] **Step 3: Clean the last compatibility remnants**

Remove any remaining:

- compatibility wording
- target-shaped steady-state config
- hidden resolver bypasses
- stale test helpers that still synthesize `operator_target_revision` as the normal anchor

- [ ] **Step 4: Run formatting, lint, and the same full test suite**

Run:

```bash
cargo fmt --all --check
cargo clippy -p app-live -p config-schema --tests -- -D warnings
cargo test -p config-schema --test validated_views --test config_roundtrip -- --test-threads=1
cargo test -p app-live --lib -- --test-threads=1
cargo test -p app-live --tests -- --test-threads=1
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/config-schema/tests \
  crates/app-live/tests \
  crates/app-live/tests/support \
  crates/app-live/src \
  config/axiom-arb.example.toml \
  README.md \
  docs/runbooks/real-user-shadow-smoke.md \
  docs/runbooks/operator-target-adoption.md
git commit -m "test: remove legacy strategy control fixtures"
```

## Final Acceptance Checklist

- [ ] No app-facing live/smoke command or doc references compatibility mode.
- [ ] `[strategy_control]` is the only steady-state live/smoke control-plane shape.
- [ ] `targets adopt --config <config>` is the single explicit migration entrypoint for migratable legacy input.
- [ ] `run` and `startup` fail closed on migratable legacy input instead of preserving hidden compatibility logic.
- [ ] Read-only commands never rewrite config files.
- [ ] Mutation-boundary commands follow `detect -> migrate -> rewrite -> re-resolve -> continue`.
- [ ] `status`, `doctor`, `bootstrap`, `apply`, `verify`, `run`, `startup`, and `targets` read the same canonical resolver contract.
- [ ] Legacy `[[negrisk.targets]]`, `[negrisk.target_source]`, and `operator_target_revision` survive only as migration input, not steady-state UX.
