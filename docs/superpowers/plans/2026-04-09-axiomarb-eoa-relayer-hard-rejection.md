# AxiomArb EOA Relayer Hard Rejection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution discipline:** Use `@superpowers:test-driven-development` for every task and `@superpowers:verification-before-completion` before claiming completion.

**Goal:** Make `EOA + [polymarket.relayer_auth]` an invalid configuration combination everywhere, remove relayer as an EOA default requirement, and align validation, runtime behavior, init UX, docs, and fixtures to that single truth.

**Architecture:** Treat wallet kind as the source of truth for relayer requiredness. `config-schema` must reject EOA configs that include relayer auth and require relayer auth only for non-EOA wallet kinds; `app-live` must split account/L2 runtime config from relayer runtime config so EOA `doctor`, `discover`, `startup`, `bootstrap`, and `real_user_shadow_smoke` can run without relayer material. Runtime entrypoints must fail closed before entering relayer-backed non-shadow live work, and operator-facing init/example/runbook output must stop teaching EOA operators to configure relayer auth.

**Tech Stack:** Rust, `config-schema`, `app-live`, `venue-polymarket`, TOML config rendering, `cargo test`, `cargo fmt`, `cargo clippy`

---

## Preconditions

- Execute this plan in a dedicated worktree, not on `main`.
- Read the controlling spec first:
  - `docs/superpowers/specs/2026-04-09-axiomarb-eoa-relayer-hard-rejection-design.md`
- Do not mix in unrelated dirty worktree changes:
  - `crates/venue-polymarket/src/l2_probe.rs`
  - `crates/venue-polymarket/tests/l2_probe.rs`
- Keep this slice focused:
  - do not redesign relayer transport
  - do not redesign websocket transport
  - do not broaden into a full runtime execution rewrite

## File Map

### Core validation and config seams

- `crates/config-schema/src/validate.rs`
  - Enforce wallet-kind-based relayer requiredness.
- `crates/app-live/src/config.rs`
  - Replace `LocalSignerConfig` as the universal live credential seam with separate account/L2 and relayer runtime config objects.
- `crates/app-live/src/lib.rs`
  - Export the new runtime config types and stop re-exporting `LocalSignerConfig` as the main app-facing input.

### Runtime entrypoints and probe behavior

- `crates/app-live/src/commands/doctor/credentials.rs`
  - Stop treating relayer-bearing signer config as the universal live credential requirement.
- `crates/app-live/src/commands/doctor/connectivity.rs`
  - Make probe sets wallet-kind aware; EOA skips relayer reachability entirely.
- `crates/app-live/src/polymarket_probe.rs`
  - Accept relayer runtime config separately from account/L2 probe credentials.
- `crates/app-live/src/commands/discover.rs`
  - Switch from `LocalSignerConfig` to account/L2 runtime config.
- `crates/app-live/src/source_tasks.rs`
  - Stop storing/constructing a relayer-bearing signer object for EOA smoke/startup paths.
- `crates/app-live/src/startup.rs`
  - Carry account runtime config without forcing relayer config for EOA.
- `crates/app-live/src/commands/bootstrap/flow.rs`
  - Update bootstrap-facing expectations/tests that still embed EOA relayer auth.
- `crates/app-live/src/commands/run.rs`
  - Add the explicit EOA non-shadow-live fail-closed boundary at runtime entry.

### Operator UX and docs

- `crates/app-live/src/commands/init/wizard.rs`
  - Collect wallet kind first; only ask for relayer auth on non-EOA paths.
- `crates/app-live/src/commands/init/render.rs`
  - Render EOA without relayer section; preserve mode must respect newly selected wallet kind and actively drop stale EOA relayer sections.
- `crates/app-live/src/commands/init/summary.rs`
  - Stop claiming live/smoke always writes `[polymarket.relayer_auth]`.
- `config/axiom-arb.example.toml`
  - Present EOA without relayer auth and keep non-EOA relayer examples separate.
- `README.md`
  - Remove generic guidance that implies relayer auth is part of the default EOA live shape.
- `docs/runbooks/real-user-shadow-smoke.md`
  - State explicitly that EOA smoke does not require relayer auth and that relayer belongs to non-EOA flows.

### Tests and fixtures

- `crates/config-schema/tests/validated_views.rs`
- `crates/config-schema/tests/config_roundtrip.rs`
- `crates/config-schema/tests/fixtures/app-live-live.toml`
- `crates/config-schema/tests/fixtures/app-live-ux-live.toml`
- `crates/config-schema/tests/fixtures/app-live-ux-smoke.toml`
- `crates/config-schema/tests/fixtures/app-replay-malformed-live.toml`
- `crates/app-live/tests/config.rs`
- `crates/app-live/tests/doctor_command.rs`
- `crates/app-live/tests/init_command.rs`
- `crates/app-live/tests/real_user_shadow_smoke.rs`
- `crates/app-live/tests/run_command.rs`
- `crates/app-live/tests/discover_command.rs`
- `crates/app-live/tests/bootstrap_command.rs`
- `crates/app-live/tests/startup_resolution.rs`
- `crates/app-live/tests/support/verify_db.rs`
- `crates/app-live/tests/apply_command.rs`
- `crates/app-live/tests/ingest_task_groups.rs`
- `crates/app-live/tests/targets_read_commands.rs`
- `crates/app-live/tests/targets_write_commands.rs`
- `crates/app-live/tests/targets_config_file.rs`
- `crates/app-live/tests/candidate_daemon.rs`
- `crates/app-live/src/daemon.rs`
- `crates/app-live/src/run_session.rs`
  - Sweep all fixtures and helper configs so no test still treats `EOA + relayer_auth` as a valid default.

---

### Task 1: Enforce Wallet-Kind Relayer Rules In `config-schema`

**Files:**
- Modify: `crates/config-schema/src/validate.rs`
- Test: `crates/config-schema/tests/validated_views.rs`
- Test: `crates/config-schema/tests/config_roundtrip.rs`
- Test: `crates/config-schema/tests/fixtures/app-live-live.toml`
- Test: `crates/config-schema/tests/fixtures/app-live-ux-live.toml`
- Test: `crates/config-schema/tests/fixtures/app-live-ux-smoke.toml`
- Test: `crates/config-schema/tests/fixtures/app-replay-malformed-live.toml`

- [ ] **Step 1: Write the failing validation tests**

Add or update tests like:

```rust
#[test]
fn live_eoa_with_relayer_auth_is_rejected() {
    let error = ValidatedConfig::new(load_raw_config_from_str(
        r#"
[runtime]
mode = "live"

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-key"
secret = "poly-secret"
passphrase = "poly-pass"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key"
address = "0x2222222222222222222222222222222222222222"
"#,
    ).unwrap())
    .and_then(|validated| validated.for_app_live())
    .unwrap_err();

    assert!(error.to_string().contains("EOA"));
    assert!(error.to_string().contains("polymarket.relayer_auth"));
}

#[test]
fn live_non_eoa_without_relayer_auth_is_rejected() {
    // proxy/safe path should still require relayer auth
}

#[test]
fn live_eoa_without_relayer_auth_is_valid() {
    // EOA account/L2-only live config should pass
}
```

Update fixture-based expectations so `EOA + relayer_auth` no longer parses as a valid live shape.

- [ ] **Step 2: Run focused config-schema tests to verify they fail**

Run:

```bash
cargo test -p config-schema --test validated_views --test config_roundtrip -- --test-threads=1
```

Expected: FAIL because live validation still requires relayer auth unconditionally and still accepts `EOA + relayer_auth`.

- [ ] **Step 3: Implement the validation rule**

Add a small wallet-kind helper and enforce the rule early. The implementation should read like:

```rust
fn account_requires_relayer(account: AppLivePolymarketAccountView<'_>) -> bool {
    !matches!(
        (account.signature_type(), account.wallet_route()),
        (SignatureTypeToml::Eoa, WalletRouteToml::Eoa)
    )
}
```

Then change live validation to:

- reject EOA if `polymarket.relayer_auth` exists
- require and validate `polymarket.relayer_auth` only when wallet kind is non-EOA

- [ ] **Step 4: Update config-schema fixtures to the new truth**

Make fixture intent explicit:

- EOA live/smoke fixtures remove `[polymarket.relayer_auth]`
- non-EOA fixtures keep relayer auth
- malformed fixtures that intentionally test invalid combos should now assert the hard rejection

- [ ] **Step 5: Re-run the focused config-schema tests**

Run:

```bash
cargo test -p config-schema --test validated_views --test config_roundtrip -- --test-threads=1
```

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/config-schema/src/validate.rs \
  crates/config-schema/tests/validated_views.rs \
  crates/config-schema/tests/config_roundtrip.rs \
  crates/config-schema/tests/fixtures/app-live-live.toml \
  crates/config-schema/tests/fixtures/app-live-ux-live.toml \
  crates/config-schema/tests/fixtures/app-live-ux-smoke.toml \
  crates/config-schema/tests/fixtures/app-replay-malformed-live.toml
git commit -m "fix: reject relayer auth for eoa wallets"
```

### Task 2: Split Account/L2 Runtime Config From Relayer Runtime Config

**Files:**
- Modify: `crates/app-live/src/config.rs`
- Modify: `crates/app-live/src/lib.rs`
- Modify: `crates/app-live/src/polymarket_probe.rs`
- Modify: `crates/app-live/src/source_tasks.rs`
- Modify: `crates/app-live/src/startup.rs`
- Test: `crates/app-live/tests/config.rs`

- [ ] **Step 1: Write the failing app config tests**

Replace the current `LocalSignerConfig` assumptions with tests for split runtime inputs:

```rust
#[test]
fn eoa_live_view_builds_account_runtime_config_without_relayer_auth() {
    let config = live_view_without_relayer();

    let account = LocalAccountRuntimeConfig::try_from(&config).unwrap();
    assert_eq!(account.signer.wallet_route, "eoa");
}

#[test]
fn eoa_live_view_does_not_build_relayer_runtime_config() {
    let config = live_view_without_relayer();

    let relayer = LocalRelayerRuntimeConfig::optional_from(&config).unwrap();
    assert!(relayer.is_none());
}

#[test]
fn non_eoa_live_view_requires_relayer_runtime_config() {
    let config = proxy_live_view_without_relayer();
    assert!(LocalRelayerRuntimeConfig::required_from(&config).is_err());
}
```

- [ ] **Step 2: Run the app config tests to verify they fail**

Run:

```bash
cargo test -p app-live --test config -- --test-threads=1
```

Expected: FAIL because `LocalSignerConfig` is still the only runtime credential seam.

- [ ] **Step 3: Introduce the split runtime config types**

In `crates/app-live/src/config.rs`, introduce explicit app-side seams:

```rust
pub struct LocalAccountRuntimeConfig {
    pub signer: LocalSignerIdentity,
    pub l2_auth: LocalL2AuthHeaders,
}

pub struct LocalRelayerRuntimeConfig {
    pub auth: LocalRelayerAuth,
}
```

Guidelines:

- `PolymarketGatewayCredentials` stays the long-lived account credential source
- account/L2 conversion must succeed for EOA live/smoke flows without relayer auth
- relayer runtime config must be conditional/explicit
- stop using `LocalSignerConfig` as the app-facing runtime seam in the entrypoints touched by this slice

- [ ] **Step 4: Update compile-time consumers to the new seam**

Change the immediate consumers to accept only what they need:

- `source_tasks.rs` should carry account runtime config, not a bundled relayer-bearing signer
- `startup.rs` should hold optional/separate relayer runtime config instead of `Option<LocalSignerConfig>`
- `polymarket_probe.rs` should accept relayer runtime config only for relayer-specific operations

Do the minimal rewiring needed to restore compilation; do not add new behavior changes yet.

- [ ] **Step 5: Re-run app config tests and a no-run compile check**

Run:

```bash
cargo test -p app-live --test config -- --test-threads=1
cargo test -p app-live --test doctor_command --test discover_command --test run_command --no-run
```

Expected: PASS for `config`; `--no-run` compile succeeds.

- [ ] **Step 6: Commit**

```bash
git add crates/app-live/src/config.rs \
  crates/app-live/src/lib.rs \
  crates/app-live/src/polymarket_probe.rs \
  crates/app-live/src/source_tasks.rs \
  crates/app-live/src/startup.rs \
  crates/app-live/tests/config.rs
git commit -m "refactor: split polymarket account and relayer runtime config"
```

### Task 3: Make Runtime Entry Points Wallet-Kind Aware And Fail Closed Correctly

**Files:**
- Modify: `crates/app-live/src/commands/doctor/credentials.rs`
- Modify: `crates/app-live/src/commands/doctor/connectivity.rs`
- Modify: `crates/app-live/src/commands/discover.rs`
- Modify: `crates/app-live/src/commands/run.rs`
- Modify: `crates/app-live/src/source_tasks.rs`
- Modify: `crates/app-live/src/startup.rs`
- Modify: `crates/app-live/src/commands/bootstrap/flow.rs`
- Test: `crates/app-live/tests/doctor_command.rs`
- Test: `crates/app-live/tests/discover_command.rs`
- Test: `crates/app-live/tests/real_user_shadow_smoke.rs`
- Test: `crates/app-live/tests/run_command.rs`
- Test: `crates/app-live/tests/bootstrap_command.rs`
- Test: `crates/app-live/tests/startup_resolution.rs`

- [ ] **Step 1: Write the failing runtime/doctor tests**

Add or update tests that lock down the corrected behavior:

```rust
#[test]
fn doctor_eoa_live_skips_relayer_probe() {
    let report = run_doctor_with_eoa_live_config();
    assert!(report.contains("authenticated REST probe succeeded"));
    assert!(!report.contains("relayer reachability probe"));
}

#[test]
fn discover_eoa_live_runs_without_relayer_auth() {
    run_discover_from_config(eoa_live_config_path()).unwrap();
}

#[test]
fn non_shadow_eoa_live_fails_closed_before_runtime_backend_start() {
    let error = run_from_config_path(non_shadow_eoa_live_config_path()).unwrap_err();
    assert!(error.to_string().contains("EOA"));
    assert!(error.to_string().contains("non-shadow"));
}
```

- [ ] **Step 2: Run the focused runtime tests to verify they fail**

Run:

```bash
cargo test -p app-live --test doctor_command --test discover_command --test bootstrap_command --test real_user_shadow_smoke --test run_command --test startup_resolution -- --test-threads=1
```

Expected: FAIL because `doctor`, `discover`, and `run` still route EOA through relayer-bearing runtime config and still probe relayer unconditionally.

- [ ] **Step 3: Make `doctor` wallet-kind aware**

Implement the minimal behavior change:

- `doctor/credentials.rs` validates account/L2 credentials for EOA without requiring relayer auth
- `doctor/connectivity.rs` builds probe sets by wallet kind
- EOA probe set = REST + market ws + user ws + heartbeat + database
- non-EOA probe set = above plus relayer

Representative control flow:

```rust
match wallet_kind(config) {
    WalletKind::Eoa => run_account_only_live_probes(...),
    WalletKind::Proxy | WalletKind::Safe => run_relayer_backed_live_probes(...),
}
```

- [ ] **Step 4: Update `discover`, `startup`, and bootstrap-owned entrypoints**

These paths must stop reconstructing the old truth:

- `discover.rs` should consume `LocalAccountRuntimeConfig`
- `source_tasks.rs` / `startup.rs` should no longer require relayer-bearing runtime input for EOA smoke/startup
- `bootstrap/flow.rs` expectations and fixtures should stop embedding EOA relayer sections

- [ ] **Step 5: Add the explicit EOA non-shadow live fail-closed seam**

Place the gate in runtime entry, not as incidental config fallout. The smallest acceptable seam is `build_live_neg_risk_execution_backend` or a helper called immediately before it:

```rust
if wallet_kind == WalletKind::Eoa && !real_user_shadow_smoke {
    return Err(ConfigError::InvalidPolymarketRuntimeConfig {
        message: "EOA live submit/reconcile requires a non-EOA relayer-backed wallet path".into(),
    });
}
```

Do not silently let EOA enter a relayer-backed runtime shape.

- [ ] **Step 6: Re-run the focused runtime tests**

Run:

```bash
cargo test -p app-live --test doctor_command --test discover_command --test bootstrap_command --test real_user_shadow_smoke --test run_command --test startup_resolution -- --test-threads=1
```

Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/app-live/src/commands/doctor/credentials.rs \
  crates/app-live/src/commands/doctor/connectivity.rs \
  crates/app-live/src/commands/discover.rs \
  crates/app-live/src/commands/run.rs \
  crates/app-live/src/source_tasks.rs \
  crates/app-live/src/startup.rs \
  crates/app-live/src/commands/bootstrap/flow.rs \
  crates/app-live/tests/doctor_command.rs \
  crates/app-live/tests/discover_command.rs \
  crates/app-live/tests/bootstrap_command.rs \
  crates/app-live/tests/real_user_shadow_smoke.rs \
  crates/app-live/tests/run_command.rs \
  crates/app-live/tests/startup_resolution.rs
git commit -m "fix: make eoa live paths account-only by default"
```

### Task 4: Fix Init Wizard, Preserve Mode, Example Config, And Operator Docs

**Files:**
- Modify: `crates/app-live/src/commands/init/wizard.rs`
- Modify: `crates/app-live/src/commands/init/render.rs`
- Modify: `crates/app-live/src/commands/init/summary.rs`
- Modify: `config/axiom-arb.example.toml`
- Modify: `README.md`
- Modify: `docs/runbooks/real-user-shadow-smoke.md`
- Test: `crates/app-live/tests/init_command.rs`

- [ ] **Step 1: Write the failing init/doc-facing tests**

Add or update tests like:

```rust
#[test]
fn init_interactive_eoa_omits_relayer_section() {
    let rendered = run_init_with_input(
        b"live\nreplace\neoa\n0x1111111111111111111111111111111111111111\n\npoly-api-key\npoly-secret\npoly-passphrase\n",
    );

    assert!(!rendered.contains("[polymarket.relayer_auth]"));
}

#[test]
fn init_preserve_rewrites_stale_eoa_relayer_section_out_of_config() {
    let rendered = render_live_config(
        eoa_answers(),
        false,
        Some(&existing_eoa_config_with_relayer()),
    ).unwrap();

    assert!(!rendered.contains("[polymarket.relayer_auth]"));
    assert!(rendered.contains("signature_type = \"eoa\""));
}

#[test]
fn init_non_eoa_path_still_collects_and_renders_relayer_auth() {
    // proxy/safe/builder path remains real
}
```

- [ ] **Step 2: Run the init-focused tests to verify they fail**

Run:

```bash
cargo test -p app-live --test init_command -- --test-threads=1
```

Expected: FAIL because wizard/render/summary still assume every live/smoke config includes relayer auth and preserve mode blindly copies stale fields.

- [ ] **Step 3: Make wallet kind explicit in init wizard**

Change the wizard order to:

1. choose `paper | live | smoke`
2. choose wallet kind for live/smoke (`eoa`, `proxy`, `safe` or the supported non-EOA subset)
3. collect account credentials
4. only collect relayer auth for non-EOA wallet kinds

This slice should make the non-EOA path real, not implicit:

- wizard explicitly collects wallet kind
- render path generates matching non-EOA account shape
- relayer auth prompts only appear for non-EOA selections

- [ ] **Step 4: Make preserve mode respect the newly selected wallet kind**

`render.rs` must stop blindly copying prior `signature_type` / `wallet_route`.

Required behavior:

- new wallet kind wins
- stale EOA relayer sections are removed
- valid non-EOA relayer sections survive preserve mode when wallet kind remains non-EOA

- [ ] **Step 5: Update summary/example/runbook/README**

Make operator truth explicit everywhere:

- EOA live/smoke summary shows `[polymarket.account]` but not `[polymarket.relayer_auth]`
- example config presents EOA without relayer auth and moves non-EOA relayer examples into separate commented sections
- README and smoke runbook state that EOA smoke does not require relayer auth and that relayer applies only to non-EOA flows

- [ ] **Step 6: Re-run the init-focused tests**

Run:

```bash
cargo test -p app-live --test init_command -- --test-threads=1
```

Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/app-live/src/commands/init/wizard.rs \
  crates/app-live/src/commands/init/render.rs \
  crates/app-live/src/commands/init/summary.rs \
  config/axiom-arb.example.toml \
  README.md \
  docs/runbooks/real-user-shadow-smoke.md \
  crates/app-live/tests/init_command.rs
git commit -m "fix: align init and docs with eoa relayer rules"
```

### Task 5: Sweep Remaining EOA+Relayer Fixtures And Run Final Verification

**Files:**
- Modify: `crates/app-live/tests/support/verify_db.rs`
- Modify: `crates/app-live/tests/apply_command.rs`
- Modify: `crates/app-live/tests/ingest_task_groups.rs`
- Modify: `crates/app-live/tests/targets_read_commands.rs`
- Modify: `crates/app-live/tests/targets_write_commands.rs`
- Modify: `crates/app-live/tests/targets_config_file.rs`
- Modify: `crates/app-live/tests/candidate_daemon.rs`
- Modify: `crates/app-live/src/daemon.rs`
- Modify: `crates/app-live/src/run_session.rs`
- Modify: any remaining file returned by the EOA-relayer sweep command

- [ ] **Step 1: Write or tighten one repository-wide guard test**

Add a narrow guard where it belongs best, for example in `crates/app-live/tests/config.rs` or `crates/config-schema/tests/validated_views.rs`:

```rust
#[test]
fn repository_fixtures_do_not_treat_eoa_relayer_as_valid_default_shape() {
    // assert specific canonical fixtures are EOA-without-relayer or non-EOA-with-relayer
}
```

This test should force future fixture additions to respect the new truth.

- [ ] **Step 2: Sweep remaining hotspots**

Run:

```bash
rg -n 'signature_type = "eoa"|wallet_route = "eoa"|\\[polymarket\\.relayer_auth\\]|kind = "(relayer_api_key|builder_api_key)"' \
  crates/app-live crates/config-schema config README.md docs/runbooks
```

Expected: remaining matches are either:

- valid non-EOA fixtures
- negative tests that explicitly assert hard rejection
- comments/docs that explain the non-EOA path

Any remaining “EOA fixture with relayer auth as harmless default” must be rewritten.

- [ ] **Step 3: Run the targeted cleanup tests**

Run:

```bash
cargo test -p app-live --test config --test doctor_command --test init_command --test real_user_shadow_smoke --test run_command --test discover_command --test startup_resolution --test apply_command -- --test-threads=1
cargo test -p config-schema --test validated_views --test config_roundtrip -- --test-threads=1
```

Expected: PASS

- [ ] **Step 4: Run repo-level formatting and linting**

Run:

```bash
cargo fmt --all --check
cargo clippy -p app-live -p config-schema --tests -- -D warnings
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/tests/support/verify_db.rs \
  crates/app-live/tests/apply_command.rs \
  crates/app-live/tests/ingest_task_groups.rs \
  crates/app-live/tests/targets_read_commands.rs \
  crates/app-live/tests/targets_write_commands.rs \
  crates/app-live/tests/targets_config_file.rs \
  crates/app-live/tests/candidate_daemon.rs \
  crates/app-live/src/daemon.rs \
  crates/app-live/src/run_session.rs \
  crates/app-live/tests/config.rs \
  crates/config-schema/tests/validated_views.rs
git add -u
git commit -m "test: align remaining fixtures with eoa relayer rejection"
```

## Final Verification Checklist

- [ ] `EOA + [polymarket.relayer_auth]` fails validation with an explicit error
- [ ] EOA live/smoke config validates without relayer auth
- [ ] non-EOA live config still requires relayer auth
- [ ] `doctor` does not run relayer reachability for EOA
- [ ] `discover` and startup-owned flows no longer require a relayer-bearing runtime config for EOA
- [ ] `real_user_shadow_smoke` works without relayer auth
- [ ] EOA non-shadow live fails closed before entering relayer-backed runtime work
- [ ] init wizard/render/summary no longer emit relayer auth for EOA
- [ ] preserve mode removes stale EOA relayer sections and preserves valid non-EOA relayer sections
- [ ] README/example/runbook all match the same operator truth
- [ ] no fixture continues to normalize `EOA + relayer_auth` as a valid default
