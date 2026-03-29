# Operator Startup UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Productize `app-live` startup so operators can initialize config, run preflight checks, and start the runtime from adopted targets without hand-authoring transient auth fields or raw `neg-risk` member payloads.

**Architecture:** Introduce a dedicated `app-live` command layer with `init`, `doctor`, and `run`, backed by a shared startup adapter that resolves built-in Polymarket defaults, derives transient auth material, and loads startup targets from a single `operator_target_revision` anchor. Evolve `config-schema` from the current low-level signer/source/targets TOML toward an operator-centric shape while preserving existing runtime authority boundaries and replay-safe provenance.

**Tech Stack:** Rust workspace (`app-live`, `config-schema`, `venue-polymarket`, `persistence`, `app-replay`), Clap subcommands, Serde/TOML, SQL-backed persistence, Cargo tests, Clippy, structured observability/logging, optional `dialoguer` for guided prompts.

---

## File Map

- Create: `crates/app-live/src/commands/mod.rs`
  - Own subcommand dispatch so `main.rs` stays thin.
- Create: `crates/app-live/src/commands/init.rs`
  - Build guided and non-interactive config scaffolding for operator-local TOML files.
- Create: `crates/app-live/src/commands/doctor.rs`
  - Run mode-scoped startup preflight checks and render `OK/FAIL/SKIP` diagnostics.
- Create: `crates/app-live/src/commands/run.rs`
  - Load validated config plus shared startup adapters, then invoke the existing runtime.
- Create: `crates/app-live/src/startup.rs`
  - Shared startup helpers:
    - built-in Polymarket defaults
    - dynamic auth derivation
    - adopted target resolution via `operator_target_revision`
    - categorized startup errors
- Create: `crates/app-live/tests/startup_resolution.rs`
  - Cover adopted-target resolution, derived auth, and operator-target anchor preservation.
- Create: `crates/app-live/tests/init_command.rs`
  - Cover config generation/update behavior without hitting the network.
- Create: `crates/app-live/tests/doctor_command.rs`
  - Cover checklist output, `SKIP` semantics, and categorized failures.
- Create: `crates/app-live/tests/run_command.rs`
  - Cover the new `app-live run --config ...` entrypoint and failure hints.
- Create: `crates/venue-polymarket/tests/auth_derivation.rs`
  - Pin down timestamp/signature derivation helpers with deterministic clocks/secrets.
- Create: `crates/config-schema/tests/fixtures/app-live-ux-paper.toml`
  - Minimal paper-mode config fixture.
- Create: `crates/config-schema/tests/fixtures/app-live-ux-live.toml`
  - Live-mode config fixture using high-level account/target-source sections.
- Create: `crates/config-schema/tests/fixtures/app-live-ux-smoke.toml`
  - Real-user shadow smoke config fixture with the new operator model.
- Create: `crates/config-schema/tests/fixtures/app-replay-ux.toml`
  - Replay-scoped fixture proving replay does not require live-only sections.

- Modify: `crates/config-schema/src/raw.rs`
  - Replace low-level signer/source/targets sections with high-level operator-facing TOML types.
- Modify: `crates/config-schema/src/validate.rs`
  - Add mode-scoped requiredness and validated views for:
    - `[polymarket.account]`
    - `[polymarket.relayer_auth]`
    - `[negrisk.target_source]`
    - optional `[polymarket.source_overrides]`
- Modify: `crates/config-schema/src/lib.rs`
  - Re-export the new validated views.
- Modify: `crates/config-schema/tests/load_config.rs`
  - Point parse tests at the new fixtures.
- Modify: `crates/config-schema/tests/validated_views.rs`
  - Add requiredness and replay/live/smoke matrix coverage for the new schema.
- Modify: `crates/venue-polymarket/src/auth.rs`
  - Add helpers to derive request-time L2 and builder auth material from long-lived secrets.
- Modify: `crates/venue-polymarket/src/lib.rs`
  - Export the new auth derivation helpers.
- Modify: `crates/app-live/Cargo.toml`
  - Add any small UX-only dependency needed for interactive `init` prompting.
- Modify: `crates/app-live/src/cli.rs`
  - Replace the single `--config` struct with subcommands and args.
- Modify: `crates/app-live/src/config.rs`
  - Stop treating config as a source of transient auth fields or raw target members in the normal startup path.
  - Add an explicit constructor for `NegRiskLiveTargetSet` that preserves externally supplied `operator_target_revision`.
- Modify: `crates/app-live/src/discovery.rs`
  - Render `rendered_live_targets` into adoptable artifacts so adopted startup can reconstruct target payloads.
- Modify: `crates/app-live/src/lib.rs`
  - Export the new commands/startup helpers.
- Modify: `crates/app-live/src/main.rs`
  - Route through subcommands and stop owning startup logic directly.
- Modify: `crates/app-live/src/smoke.rs`
  - Read the higher-level startup config shape while preserving hard shadow-only safety.
- Modify: `crates/app-live/src/runtime.rs`
  - Reuse the resolved `operator_target_revision`/target set produced by the startup adapter without inventing a second anchor.
- Modify: `crates/app-live/tests/main_entrypoint.rs`
  - Replace the legacy `--config` startup assertions with subcommand-aware binary tests.
- Modify: `crates/app-live/tests/config.rs`
  - Update config-layer tests for the new high-level sections and `NegRiskLiveTargetSet` revision preservation.
- Modify: `crates/app-live/tests/candidate_daemon.rs`
  - Assert that adoptable artifacts carry `rendered_live_targets` and keep provenance consistent.
- Modify: `crates/app-replay/tests/main_entrypoint.rs`
  - Point replay entrypoint tests at the new config fixtures.
- Modify: `config/axiom-arb.example.toml`
  - Replace low-level signer/source/target fields with the new operator-centric shape.
- Modify: `README.md`
  - Rewrite local setup and startup instructions around `app-live init`, `doctor`, and `run`.
- Modify: `docs/runbooks/real-user-shadow-smoke.md`
  - Switch the smoke runbook to the new UX and make it explicit that smoke remains shadow-only.
- Modify: `docs/runbooks/bootstrap-and-ramp.md`
  - Update bootstrap language to use adopted target sources and new commands.
- Modify: `scripts/real-user-shadow-smoke.sh`
  - Call `app-live doctor` and `app-live run` instead of assuming a manually authored low-level config.

## Scope Guard

This plan is intentionally limited to startup UX. It does **not**:

1. auto-adopt the latest candidate revision
2. add a separate `axiomctl` or control-plane binary
3. change runtime execution authority or rollout semantics
4. add hot reload of operator config
5. redesign `app-replay` beyond keeping it compatible with the new config schema

The point is to make startup operator-friendly without creating a second control surface.

### Task 1: Upgrade `config-schema` To An Operator-Centric Model

**Files:**
- Create: `crates/config-schema/tests/fixtures/app-live-ux-paper.toml`
- Create: `crates/config-schema/tests/fixtures/app-live-ux-live.toml`
- Create: `crates/config-schema/tests/fixtures/app-live-ux-smoke.toml`
- Create: `crates/config-schema/tests/fixtures/app-replay-ux.toml`
- Modify: `crates/config-schema/src/raw.rs`
- Modify: `crates/config-schema/src/validate.rs`
- Modify: `crates/config-schema/src/lib.rs`
- Modify: `crates/config-schema/tests/load_config.rs`
- Modify: `crates/config-schema/tests/validated_views.rs`

- [ ] **Step 1: Write the failing schema tests**

```rust
#[test]
fn live_view_accepts_account_and_target_source_without_raw_targets() {
    let raw = load_raw_config_from_str(
        r#"
[runtime]
mode = "live"

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key"
secret = "poly-secret"
passphrase = "poly-passphrase"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key"
address = "0x1111111111111111111111111111111111111111"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"
"#,
    )
    .unwrap();

    let live = ValidatedConfig::new(raw).unwrap().for_app_live().unwrap();
    assert_eq!(live.target_source().unwrap().operator_target_revision(), Some("targets-rev-9"));
}

#[test]
fn paper_view_does_not_require_live_sections() {
    let raw = load_raw_config_from_str("[runtime]\nmode = \"paper\"\n").unwrap();
    let live = ValidatedConfig::new(raw).unwrap().for_app_live().unwrap();
    assert!(live.is_paper());
}
```

- [ ] **Step 2: Run the targeted config-schema tests to verify they fail**

Run:
```bash
cargo test -p config-schema --test validated_views live_view_accepts_account_and_target_source_without_raw_targets -- --exact
```

Expected: FAIL because the raw schema and validated views still require low-level signer/source/target fields.

- [ ] **Step 3: Write the failing replay-compatibility test**

```rust
#[test]
fn replay_view_accepts_new_operator_facing_schema_without_live_only_sections() {
    let raw = load_raw_config_from_str(
        r#"
[runtime]
mode = "live"
"#,
    )
    .unwrap();

    let replay = ValidatedConfig::new(raw).unwrap().for_app_replay().unwrap();
    assert_eq!(replay.mode(), RuntimeModeToml::Live);
}
```

- [ ] **Step 4: Run the replay test to verify it fails**

Run:
```bash
cargo test -p config-schema --test validated_views replay_view_accepts_new_operator_facing_schema_without_live_only_sections -- --exact
```

Expected: FAIL because the current raw schema still assumes low-level polymarket/negrisk sections.

- [ ] **Step 5: Implement the minimal operator-facing schema and validated views**

Update `crates/config-schema/src/raw.rs` and `crates/config-schema/src/validate.rs` toward this shape:

```rust
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct PolymarketAccountToml {
    pub address: String,
    #[serde(default)]
    pub funder_address: Option<String>,
    pub signature_type: SignatureTypeToml,
    pub wallet_route: WalletRouteToml,
    pub api_key: String,
    pub secret: String,
    pub passphrase: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct NegRiskTargetSourceToml {
    pub source: NegRiskTargetSourceKindToml,
    #[serde(default)]
    pub operator_target_revision: Option<String>,
}
```

Add mode-scoped requiredness so:
- `paper` requires only `[runtime]`
- `live` requires account, relayer auth, and target source
- `live + real_user_shadow_smoke` additionally requires source defaults or overrides
- `app-replay` still validates against the shared schema but ignores live-only requiredness

- [ ] **Step 6: Re-run the config-schema test suite**

Run:
```bash
cargo test -p config-schema
```

Expected: PASS with the new fixtures and mode-scoped requiredness matrix.

- [ ] **Step 7: Commit**

```bash
git add crates/config-schema/src/raw.rs crates/config-schema/src/validate.rs crates/config-schema/src/lib.rs crates/config-schema/tests/load_config.rs crates/config-schema/tests/validated_views.rs crates/config-schema/tests/fixtures/app-live-ux-paper.toml crates/config-schema/tests/fixtures/app-live-ux-live.toml crates/config-schema/tests/fixtures/app-live-ux-smoke.toml crates/config-schema/tests/fixtures/app-replay-ux.toml
git commit -m "feat: add operator-facing startup config schema"
```

### Task 2: Make Adopted Targets And Dynamic Auth Usable At Startup

**Files:**
- Create: `crates/app-live/src/startup.rs`
- Create: `crates/app-live/tests/startup_resolution.rs`
- Create: `crates/venue-polymarket/tests/auth_derivation.rs`
- Modify: `crates/venue-polymarket/src/auth.rs`
- Modify: `crates/venue-polymarket/src/lib.rs`
- Modify: `crates/app-live/src/config.rs`
- Modify: `crates/app-live/src/discovery.rs`
- Modify: `crates/app-live/src/lib.rs`
- Modify: `crates/app-live/tests/config.rs`
- Modify: `crates/app-live/tests/candidate_daemon.rs`

- [ ] **Step 1: Write the failing auth-derivation tests**

```rust
#[test]
fn derives_l2_auth_from_long_lived_secret_and_clock() {
    let derived = derive_l2_auth_material(
        "poly-api-key",
        "poly-secret",
        "poly-passphrase",
        Utc.with_ymd_and_hms(2026, 3, 29, 8, 0, 0).unwrap(),
    )
    .unwrap();

    assert_eq!(derived.api_key, "poly-api-key");
    assert_eq!(derived.passphrase, "poly-passphrase");
    assert_eq!(derived.timestamp, "1774771200");
    assert!(!derived.signature.is_empty());
}

#[test]
fn derives_builder_relayer_auth_from_long_lived_secret() {
    let derived = derive_builder_relayer_auth_material(
        "builder-api-key",
        "builder-secret",
        "builder-passphrase",
        Utc.with_ymd_and_hms(2026, 3, 29, 8, 0, 1).unwrap(),
    )
    .unwrap();

    assert_eq!(derived.api_key, "builder-api-key");
    assert_eq!(derived.passphrase, "builder-passphrase");
    assert_eq!(derived.timestamp, "1774771201");
    assert!(!derived.signature.is_empty());
}
```

- [ ] **Step 2: Run the targeted venue auth test to verify it fails**

Run:
```bash
cargo test -p venue-polymarket --test auth_derivation derives_l2_auth_from_long_lived_secret_and_clock -- --exact
```

Expected: FAIL because the venue crate currently only knows how to build headers from already-derived values.

- [ ] **Step 3: Write the failing adopted-target resolution tests**

```rust
#[tokio::test]
async fn resolves_adopted_target_source_to_operator_target_revision() {
    let db = TestDatabase::new();
    db.seed_adoptable_target_with_rendered_live_targets(
        "adoptable-9",
        "candidate-9",
        "targets-rev-9",
        sample_rendered_live_targets_json(),
    )
    .await;

    let resolved = resolve_startup_targets(&db.pool, sample_live_view("targets-rev-9")).await.unwrap();

    assert_eq!(resolved.targets.revision(), "targets-rev-9");
    assert!(resolved.targets.targets().contains_key("family-a"));
}

#[tokio::test]
async fn adopted_target_resolution_fails_closed_when_provenance_is_missing() {
    let db = TestDatabase::new();
    let err = resolve_startup_targets(&db.pool, sample_live_view("targets-rev-missing"))
        .await
        .unwrap_err();

    assert!(err.to_string().contains("operator_target_revision"));
}
```

- [ ] **Step 4: Run the targeted startup-resolution test to verify it fails**

Run:
```bash
cargo test -p app-live --test startup_resolution resolves_adopted_target_source_to_operator_target_revision -- --exact
```

Expected: FAIL because `app-live` cannot currently resolve startup targets from adoption artifacts and `NegRiskLiveTargetSet` cannot preserve an externally supplied revision.

- [ ] **Step 5: Implement minimal shared startup primitives**

Add a focused startup adapter in `crates/app-live/src/startup.rs`:

```rust
pub struct StartupBundle {
    pub source_config: PolymarketSourceConfig,
    pub signer_config: Option<LocalSignerConfig>,
    pub targets: NegRiskLiveTargetSet,
    pub operator_target_revision: Option<String>,
}

pub async fn resolve_startup_targets(
    pool: &PgPool,
    config: &AppLiveConfigView<'_>,
) -> Result<ResolvedTargets, StartupError> {
    // resolve source=adopted -> operator_target_revision
    // load provenance if candidate-derived
    // parse rendered_live_targets from adoptable payload
}
```

Update `crates/app-live/src/config.rs` so `NegRiskLiveTargetSet` can be built with an explicit revision:

```rust
impl NegRiskLiveTargetSet {
    pub fn from_targets_with_revision(
        revision: impl Into<String>,
        targets: BTreeMap<String, NegRiskFamilyLiveTarget>,
    ) -> Self {
        Self { revision: revision.into(), targets }
    }
}
```

Update `crates/app-live/src/discovery.rs` so adoptable payloads actually carry `rendered_live_targets`, matching the Phase 3e contract and providing enough data for startup resolution.

- [ ] **Step 6: Re-run the targeted startup and venue tests**

Run:
```bash
cargo test -p venue-polymarket --test auth_derivation
cargo test -p app-live --test startup_resolution
cargo test -p app-live --test candidate_daemon
```

Expected: PASS with dynamic auth derivation, rendered live target payloads, and adopted target resolution anchored on `operator_target_revision`.

- [ ] **Step 7: Commit**

```bash
git add crates/venue-polymarket/src/auth.rs crates/venue-polymarket/src/lib.rs crates/venue-polymarket/tests/auth_derivation.rs crates/app-live/src/startup.rs crates/app-live/src/config.rs crates/app-live/src/discovery.rs crates/app-live/src/lib.rs crates/app-live/tests/startup_resolution.rs crates/app-live/tests/config.rs crates/app-live/tests/candidate_daemon.rs
git commit -m "feat: add adopted target startup resolution"
```

### Task 3: Add `app-live run` And Move Startup Through The Shared Adapter

**Files:**
- Create: `crates/app-live/src/commands/mod.rs`
- Create: `crates/app-live/src/commands/run.rs`
- Create: `crates/app-live/tests/run_command.rs`
- Modify: `crates/app-live/src/cli.rs`
- Modify: `crates/app-live/src/main.rs`
- Modify: `crates/app-live/src/lib.rs`
- Modify: `crates/app-live/src/runtime.rs`
- Modify: `crates/app-live/tests/main_entrypoint.rs`

- [ ] **Step 1: Write the failing CLI-dispatch tests**

```rust
#[test]
fn binary_entrypoint_requires_a_subcommand() {
    let output = Command::new(app_live_binary()).output().unwrap();

    assert!(!output.status.success());
    assert!(combined(&output).contains("Usage: app-live <COMMAND>"));
}

#[test]
fn run_subcommand_starts_paper_mode_from_operator_config() {
    let output = Command::new(app_live_binary())
        .arg("run")
        .arg("--config")
        .arg(config_fixture("app-live-ux-paper.toml"))
        .env("DATABASE_URL", default_test_database_url())
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(combined(&output).contains("app_mode=paper"));
}
```

- [ ] **Step 2: Run the targeted binary test to verify it fails**

Run:
```bash
cargo test -p app-live --test main_entrypoint binary_entrypoint_requires_a_subcommand -- --exact
```

Expected: FAIL because `AppLiveCli` still only supports a single `--config` path and no subcommands.

- [ ] **Step 3: Implement the minimal subcommand shell**

Reshape `crates/app-live/src/cli.rs` around subcommands:

```rust
#[derive(clap::Parser, Debug)]
pub struct AppLiveCli {
    #[command(subcommand)]
    pub command: AppLiveCommand,
}

#[derive(clap::Subcommand, Debug)]
pub enum AppLiveCommand {
    Init(InitArgs),
    Doctor(DoctorArgs),
    Run(RunArgs),
}
```

Move the current startup path into `crates/app-live/src/commands/run.rs`, and make it consume the `StartupBundle` from `startup.rs` before calling the existing runtime functions.

- [ ] **Step 4: Re-run the entrypoint and run-command tests**

Run:
```bash
cargo test -p app-live --test main_entrypoint
cargo test -p app-live --test run_command
```

Expected: PASS with `run --config ...` as the new primary startup command and no orphaned startup logic in `main.rs`.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/cli.rs crates/app-live/src/commands/mod.rs crates/app-live/src/commands/run.rs crates/app-live/src/main.rs crates/app-live/src/lib.rs crates/app-live/src/runtime.rs crates/app-live/tests/main_entrypoint.rs crates/app-live/tests/run_command.rs
git commit -m "feat: add app-live run command"
```

### Task 4: Implement Guided `app-live init`

**Files:**
- Create: `crates/app-live/src/commands/init.rs`
- Create: `crates/app-live/tests/init_command.rs`
- Modify: `crates/app-live/Cargo.toml`
- Modify: `crates/app-live/src/cli.rs`
- Modify: `config/axiom-arb.example.toml`

- [ ] **Step 1: Write the failing init-command tests**

```rust
#[test]
fn init_defaults_write_minimal_paper_config() {
    let temp = tempfile::NamedTempFile::new().unwrap();

    let output = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(temp.path())
        .arg("--defaults")
        .arg("--mode")
        .arg("paper")
        .output()
        .unwrap();

    assert!(output.status.success());
    let text = std::fs::read_to_string(temp.path()).unwrap();
    assert!(text.contains("[runtime]"));
    assert!(text.contains("mode = \"paper\""));
    assert!(!text.contains("timestamp ="));
    assert!(!text.contains("[[negrisk.targets]]"));
}

#[test]
fn init_live_smoke_defaults_write_target_source_not_raw_targets() {
    let temp = tempfile::NamedTempFile::new().unwrap();

    let output = Command::new(app_live_binary())
        .arg("init")
        .arg("--config")
        .arg(temp.path())
        .arg("--defaults")
        .arg("--mode")
        .arg("live")
        .arg("--real-user-shadow-smoke")
        .output()
        .unwrap();

    assert!(output.status.success());
    let text = std::fs::read_to_string(temp.path()).unwrap();
    assert!(text.contains("[negrisk.target_source]"));
    assert!(text.contains("source = \"adopted\""));
}
```

- [ ] **Step 2: Run the targeted init test to verify it fails**

Run:
```bash
cargo test -p app-live --test init_command init_defaults_write_minimal_paper_config -- --exact
```

Expected: FAIL because there is no `init` subcommand yet.

- [ ] **Step 3: Implement the minimal init flow**

Use a small prompt abstraction that supports defaults and non-interactive testing. Keep tests deterministic by supporting explicit flags plus `--defaults`.

```rust
pub struct InitConfigTemplate {
    pub runtime: RuntimeInitTemplate,
    pub polymarket: Option<PolymarketInitTemplate>,
    pub negrisk: Option<NegRiskInitTemplate>,
}

pub fn write_init_config(args: &InitArgs) -> Result<(), InitError> {
    // gather values from flags or prompt
    // write high-level TOML
    // never write transient timestamp/signature fields
}
```

Update `config/axiom-arb.example.toml` to match the new high-level shape produced by `init`.

- [ ] **Step 4: Re-run the init tests**

Run:
```bash
cargo test -p app-live --test init_command
```

Expected: PASS with deterministic config generation for paper/live/smoke defaults.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/Cargo.toml crates/app-live/src/commands/init.rs crates/app-live/tests/init_command.rs crates/app-live/src/cli.rs config/axiom-arb.example.toml
git commit -m "feat: add app-live init command"
```

### Task 5: Implement Mode-Scoped `app-live doctor`

**Files:**
- Create: `crates/app-live/src/commands/doctor.rs`
- Create: `crates/app-live/tests/doctor_command.rs`
- Modify: `crates/app-live/src/cli.rs`
- Modify: `crates/app-live/src/lib.rs`
- Modify: `crates/app-live/src/smoke.rs`
- Modify: `crates/venue-polymarket/src/lib.rs`

- [ ] **Step 1: Write the failing doctor checklist tests**

```rust
#[test]
fn doctor_paper_mode_marks_live_checks_as_skip() {
    let output = Command::new(app_live_binary())
        .arg("doctor")
        .arg("--config")
        .arg(config_fixture("app-live-ux-paper.toml"))
        .output()
        .unwrap();

    assert!(output.status.success());
    let combined = combined(&output);
    assert!(combined.contains("[OK] config parsed"));
    assert!(combined.contains("[SKIP] REST authentication not required in paper mode"));
}

#[test]
fn doctor_live_mode_reports_missing_adopted_target_as_target_source_error() {
    let output = Command::new(app_live_binary())
        .arg("doctor")
        .arg("--config")
        .arg(config_fixture("app-live-ux-live.toml"))
        .env("DATABASE_URL", default_test_database_url())
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(combined(&output).contains("TargetSourceError"));
}
```

- [ ] **Step 2: Run the targeted doctor test to verify it fails**

Run:
```bash
cargo test -p app-live --test doctor_command doctor_paper_mode_marks_live_checks_as_skip -- --exact
```

Expected: FAIL because there is no `doctor` command or checklist renderer yet.

- [ ] **Step 3: Implement the minimal preflight engine**

In `crates/app-live/src/commands/doctor.rs`, add a small checklist model:

```rust
pub enum CheckStatus {
    Ok,
    Fail,
    Skip,
}

pub struct DoctorCheck {
    pub label: &'static str,
    pub status: CheckStatus,
    pub detail: String,
    pub next_step: Option<String>,
}
```

Implement mode-scoped checks so:
- `paper` reports `SKIP` for live-only probes
- `live` runs credential/target-source checks
- `live + smoke` additionally verifies smoke safety and real upstream connectivity requirements

Do not add a second runtime authority path; `doctor` should only inspect and report.

- [ ] **Step 4: Re-run the doctor tests**

Run:
```bash
cargo test -p app-live --test doctor_command
```

Expected: PASS with explicit `OK/FAIL/SKIP` output and categorized errors.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/doctor.rs crates/app-live/tests/doctor_command.rs crates/app-live/src/cli.rs crates/app-live/src/lib.rs crates/app-live/src/smoke.rs crates/venue-polymarket/src/lib.rs
git commit -m "feat: add app-live doctor command"
```

### Task 6: Migrate Templates, Scripts, And Operator Docs To The New UX

**Files:**
- Modify: `README.md`
- Modify: `docs/runbooks/real-user-shadow-smoke.md`
- Modify: `docs/runbooks/bootstrap-and-ramp.md`
- Modify: `scripts/real-user-shadow-smoke.sh`
- Modify: `crates/app-replay/tests/main_entrypoint.rs`

- [ ] **Step 1: Write the failing doc/script-facing tests**

Add or update focused assertions so the smoke script and replay tests expect the new command surface:

```rust
#[test]
fn replay_binary_still_accepts_the_new_shared_config_fixture() {
    let config = config_fixture("app-replay-ux.toml");

    let output = Command::new(app_replay_binary())
        .env("DATABASE_URL", default_test_database_url())
        .arg("--config")
        .arg(&config)
        .args(["--from-seq", "0", "--limit", "1"])
        .output()
        .unwrap();

    assert!(output.status.success());
}
```

- [ ] **Step 2: Run the targeted replay/doc-adjacent test to verify it fails where appropriate**

Run:
```bash
cargo test -p app-replay --test main_entrypoint replay_binary_still_accepts_the_new_shared_config_fixture -- --exact
```

Expected: FAIL if the old fixtures/docs assumptions still leak low-level config fields.

- [ ] **Step 3: Update docs, template, and helper script**

Make the public UX consistent everywhere:

- README local setup:
  - `app-live init --config ...`
  - `app-live doctor --config ...`
  - `app-live run --config ...`
- smoke runbook:
  - use `doctor` then `run`
  - clearly state shadow-only guarantee
- helper script:
  - call `cargo run -p app-live -- doctor --config "$CONFIG_PATH"`
  - then `cargo run -p app-live -- run --config "$CONFIG_PATH"`
  - stop assuming hand-authored transient auth or raw target members

- [ ] **Step 4: Run the full targeted regression for this project**

Run:
```bash
cargo test -p config-schema
cargo test -p venue-polymarket --test auth_derivation
cargo test -p app-live --test config --test startup_resolution --test init_command --test doctor_command --test run_command --test main_entrypoint --test real_user_shadow_smoke
cargo test -p app-replay --test main_entrypoint
```

Expected: PASS with the new startup UX, config schema, and replay compatibility.

- [ ] **Step 5: Run repo hygiene verification**

Run:
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test --workspace
```

Expected: PASS. The workspace should no longer document or require manual `timestamp/signature` or raw `negrisk.targets` payloads for the normal adopted-target startup path.

- [ ] **Step 6: Commit**

```bash
git add README.md docs/runbooks/real-user-shadow-smoke.md docs/runbooks/bootstrap-and-ramp.md scripts/real-user-shadow-smoke.sh crates/app-replay/tests/main_entrypoint.rs
git commit -m "docs: finalize operator startup ux flow"
```
