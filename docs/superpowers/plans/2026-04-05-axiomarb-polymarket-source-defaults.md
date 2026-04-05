# Polymarket Source Defaults Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Polymarket source config truly optional for operator-facing live/smoke configs, while preserving existing explicit `source` / `source_overrides` behavior and keeping signer legacy behavior unchanged.

**Architecture:** Add a single defaulted source accessor in the validated config layer and keep raw-presence helpers unchanged. Route `app-live` runtime consumers through that defaulted accessor, then remove default source rendering from `init`, its summary output, and the example config.

**Tech Stack:** Rust, `config-schema`, `app-live`, TOML config rendering, integration tests

---

## File Map

### Config-schema defaulting and validated accessors

- Modify: `crates/config-schema/src/validate.rs`
  - Keep `has_polymarket_source()` and raw `polymarket_source()` semantics unchanged.
  - Add the new defaulted/effective source accessor for operator-facing live/smoke consumers.
  - Relax operator-facing smoke requiredness so missing source/source_overrides becomes legal there.
- Modify: `crates/config-schema/src/lib.rs`
  - Export any new defaulted source view/type needed by `app-live`.
- Test: `crates/config-schema/tests/validated_views.rs`
  - Cover omitted source, `source_overrides` precedence, raw-presence vs effective-source semantics, and signer legacy behavior.

### Runtime source consumption

- Modify: `crates/app-live/src/config.rs`
  - Switch `PolymarketSourceConfig::try_from(&AppLiveConfigView)` to the defaulted validated accessor.
  - Remove the normal missing-source path for operator-facing live/smoke configs.
  - Preserve explicit invalid-source errors.
- Test: `crates/app-live/tests/config.rs`
  - Cover omitted source defaulting, `source_overrides` precedence, bad explicit values, and signer legacy path unchanged.

### Init/example/summary UX cleanup

- Modify: `crates/app-live/src/commands/init/render.rs`
  - Stop emitting the default `[polymarket.source]` block for generated live/smoke configs.
- Modify: `crates/app-live/src/commands/init/summary.rs`
  - Stop advertising `[polymarket.source]` as a default-written section.
  - Update summary wording to mention built-in defaults / optional `source_overrides`.
- Modify: `crates/app-live/tests/init_command.rs`
  - Assert generated config omits the source block and summary output matches the new UX.
- Modify: `config/axiom-arb.example.toml`
  - Remove the default source block.
  - Leave a short comment that `polymarket.source_overrides` is only needed for non-default endpoints/cadence.

## Task 1: Add a Defaulted Validated Source Accessor

**Files:**
- Modify: `crates/config-schema/src/validate.rs`
- Modify: `crates/config-schema/src/lib.rs`
- Test: `crates/config-schema/tests/validated_views.rs`

- [ ] **Step 1: Write the failing validated-view tests**

Add focused tests in `crates/config-schema/tests/validated_views.rs` for:
- operator-facing smoke config with no `[polymarket.source]` and no `[polymarket.source_overrides]` now validates
- raw presence stays false when source is omitted
- a new effective/defaulted accessor returns the built-in source values when omitted
- `[polymarket.source_overrides]` still wins over `[polymarket.source]`
- signer-based legacy live config still fails when explicit source is missing

Example assertions to add:

```rust
#[test]
fn smoke_view_defaults_effective_source_when_omitted() {
    let raw = load_raw_config_from_str(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = true

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
"#,
    )
    .unwrap();

    let validated = ValidatedConfig::new(raw).unwrap();
    let live = validated.for_app_live().unwrap();
    assert!(!live.has_polymarket_source());

    let source = live.effective_polymarket_source().unwrap();
    assert_eq!(source.clob_host(), "https://clob.polymarket.com");
}
```

- [ ] **Step 2: Run the validated-view tests and confirm failure**

Run:

```bash
cargo test -p config-schema --test validated_views -- --test-threads=1
```

Expected:
- FAIL because the new accessor does not exist yet
- FAIL because smoke validation still requires `polymarket.source` / `source_overrides`

- [ ] **Step 3: Implement the minimal validated accessor and smoke requiredness change**

In `crates/config-schema/src/validate.rs`:
- add a small defaulted source type/accessor for operator-facing runtime consumers
- keep `has_polymarket_source()` and raw `polymarket_source()` semantics unchanged
- change the operator-facing smoke path so omitted source becomes legal
- do **not** change signer-path `require_source(...)`

In `crates/config-schema/src/lib.rs`:
- export the new accessor/type if `app-live` needs it

Keep the default values in one place, close to the validated accessor, not duplicated in `init`.

- [ ] **Step 4: Re-run the validated-view tests and confirm pass**

Run:

```bash
cargo test -p config-schema --test validated_views -- --test-threads=1
```

Expected:
- PASS

- [ ] **Step 5: Commit the schema/view change**

```bash
git add crates/config-schema/src/validate.rs crates/config-schema/src/lib.rs crates/config-schema/tests/validated_views.rs
git commit -m "feat: default polymarket source in validated view"
```

## Task 2: Route `app-live` Through the Defaulted Source

**Files:**
- Modify: `crates/app-live/src/config.rs`
- Test: `crates/app-live/tests/config.rs`

- [ ] **Step 1: Write the failing runtime/config tests**

Add focused tests in `crates/app-live/tests/config.rs` for:
- operator-facing live/smoke config omitted source now yields a valid `PolymarketSourceConfig`
- `source_overrides` still wins over `source`
- invalid explicit source values still fail
- signer legacy path behavior remains unchanged where applicable

Example test shape:

```rust
#[test]
fn operator_facing_live_defaults_polymarket_source_config() {
    let config = PolymarketSourceConfig::try_from(&operator_live_view_without_source()).unwrap();
    assert_eq!(config.clob_host.as_str(), "https://clob.polymarket.com/");
}
```

- [ ] **Step 2: Run the config tests and confirm failure**

Run:

```bash
cargo test -p app-live --test config -- --test-threads=1
```

Expected:
- FAIL because `PolymarketSourceConfig::try_from` still errors with `MissingPolymarketSourceConfig`

- [ ] **Step 3: Implement the minimal runtime/config change**

In `crates/app-live/src/config.rs`:
- switch `PolymarketSourceConfig::try_from(&AppLiveConfigView)` to consume the new effective/defaulted accessor
- remove the normal missing-source error path for operator-facing live/smoke configs
- preserve existing explicit validation behavior for malformed URLs and non-positive intervals
- keep error wording focused on invalid explicit source config, not omitted source

- [ ] **Step 4: Re-run the config tests and confirm pass**

Run:

```bash
cargo test -p app-live --test config -- --test-threads=1
```

Expected:
- PASS

- [ ] **Step 5: Commit the runtime/config change**

```bash
git add crates/app-live/src/config.rs crates/app-live/tests/config.rs
git commit -m "feat: use defaulted polymarket source config"
```

## Task 3: Remove Default Source Rendering From Init and Example

**Files:**
- Modify: `crates/app-live/src/commands/init/render.rs`
- Modify: `crates/app-live/src/commands/init/summary.rs`
- Modify: `crates/app-live/tests/init_command.rs`
- Modify: `config/axiom-arb.example.toml`

- [ ] **Step 1: Write the failing init/example tests**

Add or update tests in `crates/app-live/tests/init_command.rs` to assert:
- generated live/smoke config no longer contains `[polymarket.source]`
- wizard output no longer lists `[polymarket.source]` in "What Was Written"
- summary text still points operators to built-in defaults / optional overrides

Use a high-signal assertion pattern like:

```rust
assert!(!text.contains("[polymarket.source]"));
assert!(!combined(&output).contains("[polymarket.source]"));
assert!(combined(&output).contains("polymarket.source_overrides"));
```

- [ ] **Step 2: Run the init command tests and confirm failure**

Run:

```bash
cargo test -p app-live --test init_command -- --test-threads=1
```

Expected:
- FAIL because `render_live_config` still writes `source: Some(default_source())`
- FAIL because `init` summary still lists `[polymarket.source]`

- [ ] **Step 3: Implement the minimal init/example cleanup**

In `crates/app-live/src/commands/init/render.rs`:
- remove default source emission from generated live/smoke configs
- do not delete or rewrite existing explicit source blocks in preserve flows unless the existing file is being explicitly replaced by other wizard behavior

In `crates/app-live/src/commands/init/summary.rs`:
- remove `[polymarket.source]` from "What Was Written"
- add a short line indicating built-in Polymarket defaults are used unless `source_overrides` is supplied

In `config/axiom-arb.example.toml`:
- remove the default source block
- add a short comment showing `polymarket.source_overrides` is optional and only for non-default endpoints/cadence

- [ ] **Step 4: Re-run init tests and validate the example skeleton**

Run:

```bash
cargo test -p app-live --test init_command -- --test-threads=1
cargo test -p config-schema --test validated_views -- --test-threads=1
```

Expected:
- PASS

- [ ] **Step 5: Commit the init/example cleanup**

```bash
git add crates/app-live/src/commands/init/render.rs crates/app-live/src/commands/init/summary.rs crates/app-live/tests/init_command.rs config/axiom-arb.example.toml
git commit -m "docs: remove default polymarket source config"
```

## Task 4: Final Verification Sweep

**Files:**
- Modify: none
- Test: `crates/config-schema/tests/validated_views.rs`
- Test: `crates/app-live/tests/config.rs`
- Test: `crates/app-live/tests/init_command.rs`

- [ ] **Step 1: Run focused verification**

Run:

```bash
cargo test -p config-schema --test validated_views -- --test-threads=1
cargo test -p app-live --test config -- --test-threads=1
cargo test -p app-live --test init_command -- --test-threads=1
```

Expected:
- PASS

- [ ] **Step 2: Run lint/format verification**

Run:

```bash
cargo fmt --all --check
cargo clippy -p app-live -p config-schema --all-targets -- -D warnings
git diff --check
```

Expected:
- PASS

- [ ] **Step 3: Commit any final cleanup if needed**

If verification required tiny follow-up fixes:

```bash
git add -A
git commit -m "test: finalize polymarket source default coverage"
```

Otherwise mark this step complete with no new commit.
