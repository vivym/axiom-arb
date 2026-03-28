# AxiomArb Unified Config Schema Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace scattered `AXIOM_*` / `POLY_*` business configuration with one validated TOML file owned by a new `config-schema` crate, while keeping `DATABASE_URL` as the only deployment env var.

**Architecture:** Add `crates/config-schema` as the single owner of TOML schema, parsing, and semantic validation. `app-live` and `app-replay` move to `--config <path>` and consume consumer-scoped validated views, then the legacy `crates/config` env model and env-driven business-config entrypoints are removed. Keep runtime authority unchanged: config loading must not introduce a second activation/execution decision path.

**Tech Stack:** Rust 2021, `serde`, `toml`, `clap`, existing workspace crates (`app-live`, `app-replay`, `persistence`, `venue-polymarket`)

---

## File Structure

### New Files

- `crates/config-schema/Cargo.toml`
  - Workspace crate manifest for unified config parsing/validation.
- `crates/config-schema/src/lib.rs`
  - Public exports for raw config, validated views, load helpers, and errors.
- `crates/config-schema/src/error.rs`
  - Parse and semantic validation errors with consumer-aware messaging.
- `crates/config-schema/src/raw.rs`
  - `serde` TOML structs mirroring the public config document.
- `crates/config-schema/src/validate.rs`
  - Semantic validation and consumer-scoped view constructors (`app-live paper`, `app-live live`, `app-live smoke`, `app-replay`).
- `crates/config-schema/tests/load_config.rs`
  - Parse/file-loading coverage using TOML fixtures.
- `crates/config-schema/tests/validated_views.rs`
  - Consumer/mode-scoped requiredness and fail-closed validation coverage.
- `crates/config-schema/tests/fixtures/app-live-paper.toml`
  - Minimal paper-mode fixture.
- `crates/config-schema/tests/fixtures/app-live-live.toml`
  - Minimal live-mode fixture with source/signer/targets.
- `crates/config-schema/tests/fixtures/app-live-smoke.toml`
  - Minimal smoke-safe live fixture.
- `crates/config-schema/tests/fixtures/app-replay.toml`
  - Replay-scoped fixture that intentionally omits live-only sections.
- `crates/app-live/src/cli.rs`
  - `clap` entrypoint for `app-live --config <path>`.
- `crates/app-replay/src/cli.rs`
  - `clap` entrypoint for `app-replay --config <path> --from-seq ... --limit ...`.
- `config/axiom-arb.example.toml`
  - Operator-facing example config for current repository reality.

### Modified Files

- `Cargo.toml`
  - Add `crates/config-schema`; remove `crates/config` once migrated.
- `crates/app-live/Cargo.toml`
  - Add `clap` and `config-schema` dependencies.
- `crates/app-live/src/main.rs`
  - Remove env parsing; load validated config via `--config`.
- `crates/app-live/src/config.rs`
  - Replace JSON/env loader helpers with adapters from validated config views to existing runtime-owned structs.
- `crates/app-live/src/smoke.rs`
  - Replace env-based smoke guard parsing with config-view-based smoke validation.
- `crates/app-live/src/lib.rs`
  - Re-export any needed adapter types/helpers; stop exporting env-specific loaders.
- `crates/app-live/tests/config.rs`
  - Move parser tests from JSON/env payloads to TOML/config-view conversion tests.
- `crates/app-live/tests/main_entrypoint.rs`
  - Replace env-driven entrypoint tests with temp-file `--config` coverage.
- `crates/app-live/tests/ingest_task_groups.rs`
  - Replace helper-based JSON/env setup with typed config fixtures/builders.
- `crates/app-live/tests/candidate_daemon.rs`
  - Replace helper-based JSON/env setup with typed config fixtures/builders.
- `crates/app-replay/Cargo.toml`
  - Add `clap` and `config-schema` dependencies.
- `crates/app-replay/src/main.rs`
  - Use `clap` + replay-scoped validated config.
- `crates/app-replay/src/lib.rs`
  - Remove env-coupled helpers where they are only thin wrappers around `connect_pool_from_env`; keep replay logic separate from CLI parsing.
- `crates/app-replay/tests/main_entrypoint.rs`
  - Add `--config` coverage and assert replay does not require live signer/source sections.
- `.env.example`
  - Reduce to `DATABASE_URL` plus comments pointing to `config/axiom-arb.example.toml`.
- `README.md`
  - Replace env-heavy startup docs with `--config` usage.
- `docs/runbooks/bootstrap-and-ramp.md`
  - Update bootstrap instructions to use unified TOML config.
- `docs/runbooks/real-user-shadow-smoke.md`
  - Replace inline JSON env examples with `--config`.
- `scripts/real-user-shadow-smoke.sh`
  - Consume a TOML config path instead of inline env JSON blobs.

### Deleted Files

- `crates/config/Cargo.toml`
- `crates/config/src/lib.rs`
- `crates/config/src/settings.rs`
- `crates/config/tests/load_settings.rs`

Delete `crates/config` only after both binaries are migrated and tests are green.

## Task 1: Scaffold `config-schema`

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/config-schema/Cargo.toml`
- Create: `crates/config-schema/src/lib.rs`
- Create: `crates/config-schema/src/error.rs`
- Create: `crates/config-schema/src/raw.rs`
- Test: `crates/config-schema/tests/load_config.rs`
- Test: `crates/config-schema/tests/fixtures/app-live-paper.toml`

- [ ] **Step 1: Write the failing crate/bootstrap tests**

```rust
use config_schema::load_raw_config_from_path;

#[test]
fn load_raw_config_from_path_parses_minimal_paper_fixture() {
    let fixture = fixture_path("app-live-paper.toml");
    let raw = load_raw_config_from_path(&fixture).expect("fixture should parse");

    assert_eq!(raw.runtime.mode.as_str(), "paper");
}
```

- [ ] **Step 2: Run the targeted test and verify the crate is missing**

Run: `cargo test -p config-schema --test load_config -- --nocapture`
Expected: FAIL because `config-schema` is not yet a workspace member/crate.

- [ ] **Step 3: Add the new workspace crate and minimal load API**

```toml
# Cargo.toml
[workspace]
members = [
  "crates/app-live",
  "crates/app-replay",
  "crates/config-schema",
  # ...
]
```

```rust
// crates/config-schema/src/lib.rs
mod error;
mod raw;

pub use error::ConfigSchemaError;
pub use raw::{RawAxiomConfig, RuntimeModeToml};

pub fn load_raw_config_from_path(path: &std::path::Path) -> Result<RawAxiomConfig, ConfigSchemaError> {
    let text = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&text)?)
}
```

- [ ] **Step 4: Run the crate tests again**

Run: `cargo test -p config-schema --test load_config -- --nocapture`
Expected: PASS for the minimal parse fixture.

- [ ] **Step 5: Commit the scaffold**

```bash
git add Cargo.toml crates/config-schema
git commit -m "feat: scaffold config schema crate"
```

## Task 2: Add semantic validation and consumer-scoped views

**Files:**
- Modify: `crates/config-schema/src/lib.rs`
- Create: `crates/config-schema/src/validate.rs`
- Create: `crates/config-schema/tests/validated_views.rs`
- Create: `crates/config-schema/tests/fixtures/app-live-live.toml`
- Create: `crates/config-schema/tests/fixtures/app-live-smoke.toml`
- Create: `crates/config-schema/tests/fixtures/app-replay.toml`

- [ ] **Step 1: Write failing validation tests for mode/binary-scoped requiredness**

```rust
use config_schema::{load_raw_config_from_path, ValidatedConfig};

#[test]
fn replay_view_does_not_require_live_signer_or_source() {
    let raw = load_raw_config_from_path(&fixture_path("app-replay.toml")).unwrap();
    let validated = ValidatedConfig::new(raw).unwrap();

    validated.for_app_replay().expect("replay view should validate");
}

#[test]
fn smoke_view_requires_live_source_and_signer() {
    let raw = load_raw_config_from_path(&fixture_path("app-live-smoke.toml")).unwrap();
    let err = ValidatedConfig::new(raw)
        .unwrap()
        .for_app_live()
        .expect_err("smoke fixture missing signer should fail");

    assert!(err.to_string().contains("polymarket.signer"));
}

#[test]
fn invalid_toml_and_invalid_fields_fail_closed() {
    assert!(load_raw_config_from_str("runtime = [").is_err());
    assert!(validated_err("runtime.mode = \"invalid\"").contains("runtime.mode"));
    assert!(validated_err("heartbeat_interval_seconds = 0").contains("heartbeat_interval_seconds"));
    assert!(validated_err("signature_type = \"EOA\"\nwallet_route = \"Safe\"").contains("wallet_route"));
}
```

- [ ] **Step 2: Run validation tests to capture the missing consumer-view layer**

Run: `cargo test -p config-schema --test validated_views -- --nocapture`
Expected: FAIL because semantic validation and per-consumer views do not exist yet.

- [ ] **Step 3: Implement `ValidatedConfig` and explicit consumer views**

```rust
pub struct ValidatedConfig {
    raw: RawAxiomConfig,
}

impl ValidatedConfig {
    pub fn new(raw: RawAxiomConfig) -> Result<Self, ConfigSchemaError> {
        validate_global_invariants(&raw)?;
        Ok(Self { raw })
    }

    pub fn for_app_live(&self) -> Result<AppLiveConfigView, ConfigSchemaError> {
        validate_app_live_requiredness(&self.raw)
    }

    pub fn for_app_replay(&self) -> Result<AppReplayConfigView, ConfigSchemaError> {
        validate_app_replay_requiredness(&self.raw)
    }
}
```

- [ ] **Step 4: Add fail-closed semantic rules**

Implement and test:
- `paper + real_user_shadow_smoke=true` rejects
- `live smoke` requires source + signer
- invalid TOML syntax rejects before semantic validation
- missing required sections/fields reject per consumer view
- invalid URL/enums/numeric values reject
- `approved/ready` families must exist in targets
- duplicate `family_id` rejects
- malformed or duplicate target members reject
- incompatible `signature_type` / `wallet_route` rejects
- replay view never requires live signer/source/relayer auth

- [ ] **Step 5: Run the config-schema test suite**

Run: `cargo test -p config-schema --test load_config --test validated_views -- --nocapture`
Expected: PASS for both parse and validation suites.

- [ ] **Step 6: Commit the validation layer**

```bash
git add crates/config-schema
git commit -m "feat: validate unified config views"
```

## Task 3: Migrate `app-live` to `--config`

**Files:**
- Modify: `crates/app-live/Cargo.toml`
- Create: `crates/app-live/src/cli.rs`
- Modify: `crates/app-live/src/daemon.rs`
- Modify: `crates/app-live/src/main.rs`
- Modify: `crates/app-live/src/config.rs`
- Modify: `crates/app-live/src/smoke.rs`
- Modify: `crates/app-live/src/lib.rs`
- Test: `crates/app-live/tests/main_entrypoint.rs`
- Test: `crates/app-live/tests/config.rs`
- Test: `crates/app-live/tests/ingest_task_groups.rs`
- Test: `crates/app-live/tests/candidate_daemon.rs`

- [ ] **Step 1: Write failing `app-live` entrypoint tests for `--config`**

```rust
#[test]
fn binary_entrypoint_reads_paper_mode_from_config_file() {
    let output = app_live_output_with_config("fixtures/app-live-paper.toml");

    assert!(output.status.success());
    assert!(combined(&output).contains("app_mode=paper"));
}

#[test]
fn binary_entrypoint_rejects_missing_config_path() {
    let output = Command::new(app_live_binary()).output().unwrap();

    assert!(!output.status.success());
    assert!(combined(&output).contains("--config"));
}

#[test]
fn paper_mode_still_requires_database_url_after_config_load() {
    let output = app_live_output_with_config_and_database("fixtures/app-live-paper.toml", None);

    assert!(!output.status.success());
    assert!(combined(&output).contains("DATABASE_URL"));
}
```

- [ ] **Step 2: Run the targeted `app-live` tests**

Run: `cargo test -p app-live --test main_entrypoint --test config -- --nocapture`
Expected: FAIL because `app-live` still reads env vars and has no `--config` CLI contract.

- [ ] **Step 3: Add `clap` entrypoint parsing and load `config-schema`**

```rust
// crates/app-live/src/cli.rs
#[derive(clap::Parser, Debug)]
pub struct AppLiveCli {
    #[arg(long)]
    pub config: std::path::PathBuf,
}
```

```rust
// crates/app-live/src/main.rs
let cli = AppLiveCli::parse();
let raw = config_schema::load_raw_config_from_path(&cli.config)?;
let validated = config_schema::ValidatedConfig::new(raw)?;
let config = validated.for_app_live()?;
let database_url = require_database_url_env()?;
```

- [ ] **Step 4: Replace env-JSON loaders with typed config adapters**

Convert the current runtime-owned structs from `crates/app-live/src/config.rs` to accept validated view data:

```rust
impl TryFrom<&config_schema::AppLiveConfigView> for NegRiskLiveTargetSet { /* ... */ }
impl TryFrom<&config_schema::AppLiveConfigView> for LocalSignerConfig { /* ... */ }
impl TryFrom<&config_schema::AppLiveConfigView> for PolymarketSourceConfig { /* ... */ }
```

Delete/stop exporting:
- `load_neg_risk_live_targets(...)`
- `load_local_signer_config(...)`
- `load_real_user_shadow_smoke_config_from_env(...)`

The runtime should still build the same `SmokeSafeStartupSource` / `NegRiskLiveTargetSet` / signer/source objects, just from validated TOML instead of env JSON.
Migrate every `app-live` test file that still imports those helpers in the same task:
- `crates/app-live/src/daemon.rs` (`#[cfg(test)]` module)
- `crates/app-live/tests/config.rs`
- `crates/app-live/tests/main_entrypoint.rs`
- `crates/app-live/tests/ingest_task_groups.rs`
- `crates/app-live/tests/candidate_daemon.rs`

Move `DATABASE_URL` validation to the shared startup path so it executes after config parsing and before the `paper` / `live` branch.

- [ ] **Step 5: Re-run the `app-live` targeted tests**

Run: `cargo test -p app-live --test main_entrypoint --test config --test ingest_task_groups --test candidate_daemon -- --nocapture`
Expected: PASS with config-file startup and unchanged runtime semantics.

- [ ] **Step 6: Commit the `app-live` migration**

```bash
git add crates/app-live/Cargo.toml crates/app-live/src crates/app-live/tests
git commit -m "feat: load app-live from unified config"
```

## Task 4: Migrate `app-replay` to `--config`

**Files:**
- Modify: `crates/app-replay/Cargo.toml`
- Create: `crates/app-replay/src/cli.rs`
- Modify: `crates/app-replay/src/main.rs`
- Modify: `crates/app-replay/src/lib.rs`
- Test: `crates/app-replay/tests/main_entrypoint.rs`

- [ ] **Step 1: Write failing `app-replay` tests for replay-scoped config**

```rust
#[test]
fn binary_entrypoint_accepts_replay_scoped_config_without_live_signer() {
    let output = Command::new(app_replay_binary())
        .env("DATABASE_URL", test_database_url())
        .args(["--config", fixture("app-replay.toml"), "--from-seq", "0", "--limit", "1"])
        .output()
        .unwrap();

    assert!(output.status.success());
}

#[test]
fn replay_rejects_missing_database_url_even_with_valid_config() {
    let output = Command::new(app_replay_binary())
        .args(["--config", fixture("app-replay.toml"), "--from-seq", "0", "--limit", "1"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(combined(&output).contains("DATABASE_URL"));
}
```

- [ ] **Step 2: Run the targeted replay tests**

Run: `cargo test -p app-replay --test main_entrypoint -- --nocapture`
Expected: FAIL because `app-replay` has no `--config` contract and still parses args manually.

- [ ] **Step 3: Introduce `clap` CLI parsing and a replay-scoped config view**

```rust
// crates/app-replay/src/cli.rs
#[derive(clap::Parser, Debug)]
pub struct AppReplayCli {
    #[arg(long)]
    pub config: std::path::PathBuf,
    #[arg(long = "from-seq")]
    pub from_seq: i64,
    #[arg(long)]
    pub limit: Option<i64>,
}
```

Keep replay logic in `lib.rs`, but make `main.rs` load and validate the shared config before calling the existing replay helpers.

- [ ] **Step 4: Keep replay requiredness intentionally small**

Explicitly assert in code and tests:
- `DATABASE_URL` still comes from env
- replay config validation does not require `polymarket.signer`
- replay config validation does not require `polymarket.source`
- replay checks `DATABASE_URL` immediately after config load and before replay execution
- replay keeps its current journal/summary behavior unchanged

- [ ] **Step 5: Re-run replay entrypoint tests**

Run: `cargo test -p app-replay --test main_entrypoint -- --nocapture`
Expected: PASS with `--config` and replay-scoped validation.

- [ ] **Step 6: Commit the replay migration**

```bash
git add crates/app-replay/Cargo.toml crates/app-replay/src crates/app-replay/tests
git commit -m "feat: load app-replay from unified config"
```

## Task 5: Remove the legacy env config model

**Files:**
- Modify: `Cargo.toml`
- Delete: `crates/config/Cargo.toml`
- Delete: `crates/config/src/lib.rs`
- Delete: `crates/config/src/settings.rs`
- Delete: `crates/config/tests/load_settings.rs`

- [ ] **Step 1: Write one repo-level regression test before deleting the crate**

Add a targeted assertion in binary tests that legacy business env vars alone are no longer enough to start either binary:

```rust
#[test]
fn legacy_env_without_config_is_rejected() {
    let output = Command::new(app_live_binary())
        .env("AXIOM_MODE", "live")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(combined(&output).contains("--config"));
}
```

- [ ] **Step 2: Run the targeted binary tests**

Run: `cargo test -p app-live --test main_entrypoint --test config -- --nocapture && cargo test -p app-replay --test main_entrypoint -- --nocapture`
Expected: PASS, proving the new CLI contract exists before deleting the old crate.

- [ ] **Step 3: Remove `crates/config` from the workspace**

Delete the crate files and remove it from `Cargo.toml`.

- [ ] **Step 4: Run focused build/test verification after deletion**

Run: `cargo test -p config-schema -p app-live -p app-replay --all-targets -- --nocapture`
Expected: PASS with no references to `crates/config` or `POLY_*` runtime parsing left in code.

- [ ] **Step 5: Commit the removal**

```bash
git add Cargo.toml crates/config-schema crates/app-live crates/app-replay
git rm -r crates/config
git commit -m "refactor: remove legacy env config crate"
```

## Task 6: Replace docs, examples, and smoke tooling

**Files:**
- Create: `config/axiom-arb.example.toml`
- Modify: `.env.example`
- Modify: `README.md`
- Modify: `docs/runbooks/bootstrap-and-ramp.md`
- Modify: `docs/runbooks/real-user-shadow-smoke.md`
- Modify: `scripts/real-user-shadow-smoke.sh`

- [ ] **Step 1: Write the failing operator-surface checks**

Use cheap verification instead of unit tests:

```bash
rg -n "AXIOM_NEG_RISK_LIVE_TARGETS|AXIOM_LOCAL_SIGNER_CONFIG|AXIOM_POLYMARKET_SOURCE_CONFIG" \
  README.md docs/runbooks scripts/real-user-shadow-smoke.sh .env.example
```

Expected: FAIL right now because these files still document env-driven startup.

- [ ] **Step 2: Add the example TOML and rewrite the docs/scripts**

The example config should cover current, real fields only:

```toml
[runtime]
mode = "live"
real_user_shadow_smoke = false

[polymarket.source]
clob_host = "https://clob.polymarket.com"
# ...

[polymarket.signer]
address = "0xYOUR_ADDRESS"
# ...

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-a"]

[[negrisk.targets]]
family_id = "family-a"
```

Update the smoke helper script to take a config path:

```bash
CONFIG_PATH="${1:-config/axiom-arb.example.toml}"
cargo run -p app-live -- --config "$CONFIG_PATH"
```

- [ ] **Step 3: Reduce `.env.example` to the deployment env**

Keep only:

```dotenv
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb
```

Optionally add one comment line pointing operators at `config/axiom-arb.example.toml`.

- [ ] **Step 4: Re-run the operator-surface grep and script syntax checks**

Run:

```bash
rg -n "AXIOM_NEG_RISK_LIVE_TARGETS|AXIOM_LOCAL_SIGNER_CONFIG|AXIOM_POLYMARKET_SOURCE_CONFIG|POLY_" \
  README.md docs/runbooks scripts/real-user-shadow-smoke.sh .env.example
bash -n scripts/real-user-shadow-smoke.sh
```

Expected:
- `rg` returns no hits in current operator docs/scripts
- `bash -n` exits 0

- [ ] **Step 5: Commit the docs/tooling migration**

```bash
git add config/axiom-arb.example.toml .env.example README.md docs/runbooks scripts/real-user-shadow-smoke.sh
git commit -m "docs: switch operator workflows to unified config"
```

## Task 7: Full repository verification and cleanup

**Files:**
- Modify: any small follow-up files found by `fmt`, `clippy`, or targeted test drift

- [ ] **Step 1: Run formatter**

Run: `cargo fmt --all`
Expected: no diff after formatting is applied.

- [ ] **Step 2: Run lints**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 3: Run focused test suites first**

Run:

```bash
cargo test -p config-schema --all-targets -- --nocapture
cargo test -p app-live --test config --test main_entrypoint -- --nocapture
cargo test -p app-replay --test main_entrypoint -- --nocapture
```

Expected: PASS.

- [ ] **Step 4: Run full workspace verification**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test --workspace
```

Expected: PASS.

- [ ] **Step 5: Run one final legacy-config grep**

Run:

```bash
rg -n "AXIOM_MODE|AXIOM_NEG_RISK_LIVE_TARGETS|AXIOM_LOCAL_SIGNER_CONFIG|AXIOM_REAL_USER_SHADOW_SMOKE|AXIOM_POLYMARKET_SOURCE_CONFIG|POLY_CLOB_HOST|POLY_DATA_API_HOST|POLY_RELAYER_HOST|POLY_SIGNATURE_TYPE" \
  crates/app-live crates/app-replay crates/config-schema README.md docs/runbooks scripts .env.example
```

Expected:
- only historical/spec/plan docs outside this grep scope may still mention the removed env vars
- active codepaths and operator docs should not

- [ ] **Step 6: Final commit**

```bash
git add -A
git commit -m "chore: finalize unified config migration"
```

## Notes For The Implementer

- Use `@superpowers:test-driven-development` for every code task. Do not start with implementation; start with the failing targeted test.
- Use `@superpowers:verification-before-completion` before claiming the migration is done.
- Keep `DATABASE_URL` as the only env dependency. Do not reintroduce `AXIOM_*` or `POLY_*` compatibility shims.
- `Shadow` stays an execution/activation result. Do not invent `runtime.mode = "shadow"`.
- `app-replay` must keep a replay-scoped validated view. Do not force replay to require live signer/source/relayer sections.
- Do not let config loading mutate runtime authority. `ActivationPolicy`, route execution mode, and live/shadow semantics must remain where they already live.
