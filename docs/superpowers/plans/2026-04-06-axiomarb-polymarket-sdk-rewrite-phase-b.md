# AxiomArb Polymarket SDK Rewrite Phase B Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution discipline:** Use `@superpowers:test-driven-development` for every task and `@superpowers:verification-before-completion` before claiming completion.

**Goal:** Cut `app-live` and config/mainline runtime wiring over to the SDK-backed Polymarket gateway, then delete the old transport/auth main path after `strategy-neutral-control-plane` has landed.

**Architecture:** Treat this as post-merge cutover work. Rebase onto the strategy-neutral control-plane result first, then rewire config/auth handling, doctor probes, runtime providers, discovery wiring, and docs to the new gateway. Remove the temporary compatibility shell only after the new app-facing path is fully verified.

**Tech Stack:** Rust, existing `app-live` and `config-schema` crates, Phase A gateway in `venue-polymarket`, `cargo test`, `cargo fmt`, `cargo clippy`

---

## Preconditions

- Phase A has merged and its `venue-polymarket` gateway is available.
- The `strategy-neutral-control-plane` branch has merged, or the overlap files are otherwise stable enough for cutover work.
- Before starting Task 1, review the merged versions of:
  - `crates/config-schema/src/validate.rs`
  - `crates/app-live/src/config.rs`
  - `crates/app-live/src/commands/init/render.rs`
  - `crates/app-live/src/source_tasks.rs`
  - `crates/app-live/src/commands/doctor/connectivity.rs`

## File Map

### New files

- `crates/app-live/src/polymarket_probe.rs`
  - Repo-owned facade that wires `doctor` probe expectations onto gateway capabilities without leaking SDK details.
- `crates/app-live/src/polymarket_runtime_adapter.rs`
  - Shared async adapter for runtime/discovery/provider call sites so the end state does not create a fresh Tokio runtime per provider call.

### Modified files

- `crates/app-live/src/lib.rs`
  - Export any new probe/runtime adapter modules.
- `crates/app-live/src/config.rs`
  - Remove static signer mainline usage and load long-lived credential material for gateway construction.
- `crates/app-live/src/source_tasks.rs`
  - Build runtime/discovery source bundles from the gateway instead of direct legacy REST/WS clients.
- `crates/app-live/src/negrisk_live.rs`
  - Translate route-owned signed submissions into venue-facing DTOs and use the shared runtime adapter.
- `crates/app-live/src/commands/doctor/connectivity.rs`
  - Route authenticated REST + market WS + user WS + relayer probes through the new probe facade.
- `crates/app-live/src/commands/discover.rs`
- `crates/app-live/src/commands/run.rs`
  - Use gateway-backed construction paths.
- `crates/app-live/src/commands/init/render.rs`
  - Stop emitting static signer mainline examples.
- `crates/config-schema/src/raw.rs`
- `crates/config-schema/src/validate.rs`
  - Remove or explicitly reject static signer mainline support once the cutover is complete.
- `crates/venue-polymarket/src/lib.rs`
  - Remove legacy public exports once no callers remain.
- `crates/venue-polymarket/src/auth.rs`
- `crates/venue-polymarket/src/rest.rs`
- `crates/venue-polymarket/src/ws_client.rs`
  - Delete or shrink legacy transport/auth code after cutover.
- `crates/app-live/tests/config.rs`
- `crates/app-live/tests/doctor_command.rs`
- `crates/app-live/tests/discover_command.rs`
- `crates/app-live/tests/bootstrap_command.rs`
- `crates/app-live/tests/real_user_shadow_smoke.rs`
- `crates/app-live/tests/run_command.rs`
- `crates/app-live/tests/init_command.rs`
- `crates/app-live/tests/main_entrypoint.rs`
- `crates/config-schema/tests/validated_views.rs`
- `crates/config-schema/tests/config_roundtrip.rs`
  - Update fixtures and cutover expectations.
- `config/axiom-arb.example.toml`
- `README.md`
- `docs/runbooks/real-user-shadow-smoke.md`
  - Remove static signer guidance and describe the gateway-backed mainline path.

### Existing files to study before editing

- `docs/superpowers/specs/2026-04-06-axiomarb-polymarket-sdk-rewrite-design.md`
- `docs/superpowers/specs/2026-04-06-axiomarb-strategy-neutral-control-plane-design.md`
- `docs/superpowers/plans/2026-04-06-axiomarb-polymarket-sdk-rewrite-phase-a.md`
- `crates/app-live/src/config.rs`
- `crates/app-live/src/source_tasks.rs`
- `crates/app-live/src/negrisk_live.rs`
- `crates/app-live/src/commands/doctor/connectivity.rs`
- `crates/app-live/src/commands/init/render.rs`
- `crates/config-schema/src/validate.rs`

---

### Task 1: Rebaseline Config/Auth Handling After Strategy-Neutral Merge

**Files:**
- Create: `crates/app-live/src/polymarket_runtime_adapter.rs`
- Modify: `crates/app-live/src/lib.rs`
- Modify: `crates/app-live/src/config.rs`
- Modify: `crates/config-schema/src/raw.rs`
- Modify: `crates/config-schema/src/validate.rs`
- Test: `crates/app-live/tests/config.rs`
- Test: `crates/config-schema/tests/validated_views.rs`

- [ ] **Step 1: Write the failing config-cutover tests**

```rust
#[test]
fn live_config_rejects_polymarket_signer_after_cutover() {
    let error = load_live_config_with_signer().unwrap_err();
    assert!(error.to_string().contains("polymarket.signer is no longer supported"));
}

#[test]
fn live_config_builds_gateway_credentials_from_polymarket_account() {
    let config = load_live_config_with_account().unwrap();
    let credentials = config.polymarket_gateway_credentials().unwrap();
    assert_eq!(credentials.api_key, "poly-api-key");
}
```

- [ ] **Step 2: Run the config-cutover tests to verify they fail**

Run:

```bash
cargo test -p app-live --test config -- --test-threads=1
cargo test -p config-schema --test validated_views -- --test-threads=1
```

Expected: FAIL because static signer support is still accepted and gateway credentials are not the mainline path.

- [ ] **Step 3: Implement the cutover config model**

Target shape:

```rust
pub struct PolymarketGatewayCredentials {
    pub address: String,
    pub funder_address: String,
    pub signature_type: String,
    pub wallet_route: String,
    pub api_key: String,
    pub secret: String,
    pub passphrase: String,
}
```

Keep this focused on long-lived credential material only. Do not persist timestamp/signature values in the new mainline path.

- [ ] **Step 4: Re-run the config-cutover tests**

Run:

```bash
cargo test -p app-live --test config -- --test-threads=1
cargo test -p config-schema --test validated_views -- --test-threads=1
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/polymarket_runtime_adapter.rs \
  crates/app-live/src/lib.rs \
  crates/app-live/src/config.rs \
  crates/config-schema/src/raw.rs \
  crates/config-schema/src/validate.rs \
  crates/app-live/tests/config.rs \
  crates/config-schema/tests/validated_views.rs
git commit -m "refactor: cut app config over to gateway credentials"
```

### Task 2: Cut `doctor` Over To Gateway Probe Facade

**Files:**
- Create: `crates/app-live/src/polymarket_probe.rs`
- Modify: `crates/app-live/src/lib.rs`
- Modify: `crates/app-live/src/commands/doctor/connectivity.rs`
- Test: `crates/app-live/tests/doctor_command.rs`
- Test: `crates/app-live/tests/main_entrypoint.rs`

- [ ] **Step 1: Write the failing doctor-cutover tests**

```rust
#[test]
fn doctor_reports_ws_and_rest_probe_failures_via_gateway_categories() {
    let output = run_doctor_with_scripted_gateway_failure("Connectivity");
    assert!(output.contains("market websocket probe"));
    assert!(output.contains("user websocket probe"));
    assert!(output.contains("authenticated REST probe"));
}
```

- [ ] **Step 2: Run the doctor-cutover tests to verify they fail**

Run:

```bash
cargo test -p app-live --test doctor_command --test main_entrypoint -- --test-threads=1
```

Expected: FAIL because `doctor` still constructs legacy REST/WS clients directly.

- [ ] **Step 3: Implement the probe facade and wire `doctor` through it**

Use a repo-owned facade so `doctor` sees capability probes, not SDK types:

```rust
pub trait PolymarketProbeFacade {
    fn probe_rest<'a>(&'a self) -> ProbeFuture<'a>;
    fn probe_market_ws<'a>(&'a self, token_ids: &'a [String]) -> ProbeFuture<'a>;
    fn probe_user_ws<'a>(&'a self, condition_ids: &'a [String]) -> ProbeFuture<'a>;
    fn probe_relayer<'a>(&'a self) -> ProbeFuture<'a>;
}
```

- [ ] **Step 4: Re-run the doctor-cutover tests**

Run:

```bash
cargo test -p app-live --test doctor_command --test main_entrypoint -- --test-threads=1
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/polymarket_probe.rs \
  crates/app-live/src/lib.rs \
  crates/app-live/src/commands/doctor/connectivity.rs \
  crates/app-live/tests/doctor_command.rs \
  crates/app-live/tests/main_entrypoint.rs
git commit -m "refactor: route doctor probes through polymarket gateway"
```

### Task 3: Cut Runtime, Discovery, And Submit/Reconcile Over To Shared Gateway Adapter

**Files:**
- Modify: `crates/app-live/src/polymarket_runtime_adapter.rs`
- Modify: `crates/app-live/src/source_tasks.rs`
- Modify: `crates/app-live/src/negrisk_live.rs`
- Modify: `crates/app-live/src/commands/discover.rs`
- Modify: `crates/app-live/src/commands/run.rs`
- Test: `crates/app-live/tests/discover_command.rs`
- Test: `crates/app-live/tests/bootstrap_command.rs`
- Test: `crates/app-live/tests/run_command.rs`
- Test: `crates/app-live/tests/real_user_shadow_smoke.rs`

- [ ] **Step 1: Write the failing runtime-cutover tests**

```rust
#[test]
fn discover_uses_gateway_backed_metadata_refresh() {
    let output = run_discover_with_scripted_gateway_metadata();
    assert!(output.contains("Discovery completed"));
}

#[test]
fn runtime_submit_reconcile_path_does_not_spawn_runtime_per_provider_call() {
    let stats = run_smoke_runtime_with_scripted_gateway();
    assert_eq!(stats.runtime_spawn_count, 1);
}
```

- [ ] **Step 2: Run the runtime-cutover tests to verify they fail**

Run:

```bash
cargo test -p app-live --test discover_command --test bootstrap_command --test run_command --test real_user_shadow_smoke -- --test-threads=1
```

Expected: FAIL because runtime/discovery still use legacy client construction and per-call async bridging.

- [ ] **Step 3: Implement the shared gateway adapter**

Target shape:

```rust
pub struct PolymarketRuntimeAdapter {
    gateway: venue_polymarket::PolymarketGateway,
    runtime: tokio::runtime::Handle,
}
```

Rules:

- no fresh `Runtime::new()` per provider call
- route-owned signed payloads get translated into `PolymarketSignedOrder` before crossing the gateway
- discovery/metadata and stream tasks all use the same gateway-backed construction path

- [ ] **Step 4: Re-run the runtime-cutover tests**

Run:

```bash
cargo test -p app-live --test discover_command --test bootstrap_command --test run_command --test real_user_shadow_smoke -- --test-threads=1
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/polymarket_runtime_adapter.rs \
  crates/app-live/src/source_tasks.rs \
  crates/app-live/src/negrisk_live.rs \
  crates/app-live/src/commands/discover.rs \
  crates/app-live/src/commands/run.rs \
  crates/app-live/tests/discover_command.rs \
  crates/app-live/tests/bootstrap_command.rs \
  crates/app-live/tests/run_command.rs \
  crates/app-live/tests/real_user_shadow_smoke.rs
git commit -m "refactor: cut runtime and discovery over to polymarket gateway"
```

### Task 4: Remove Legacy Surface And Update Operator Docs

**Files:**
- Modify: `crates/venue-polymarket/src/lib.rs`
- Modify: `crates/venue-polymarket/src/auth.rs`
- Modify: `crates/venue-polymarket/src/rest.rs`
- Modify: `crates/venue-polymarket/src/ws_client.rs`
- Modify: `crates/app-live/src/commands/init/render.rs`
- Modify: `crates/app-live/tests/init_command.rs`
- Modify: `crates/app-live/tests/config.rs`
- Modify: `config/axiom-arb.example.toml`
- Modify: `README.md`
- Modify: `docs/runbooks/real-user-shadow-smoke.md`

- [ ] **Step 1: Write the failing removal/docs tests**

```rust
#[test]
fn init_output_no_longer_emits_polymarket_signer_block() {
    let text = render_init_output();
    assert!(!text.contains("[polymarket.signer]"));
}
```

- [ ] **Step 2: Run the removal/docs tests to verify they fail**

Run:

```bash
cargo test -p app-live --test init_command --test config -- --test-threads=1
```

Expected: FAIL because legacy signer output/support still exists.

- [ ] **Step 3: Delete the old mainline path and update docs**

Rules:

- remove stale legacy exports from `venue-polymarket::lib`
- stop using static signer config in mainline code
- update examples and runbooks to describe only the account-backed gateway path

- [ ] **Step 4: Re-run the removal/docs tests**

Run:

```bash
cargo test -p app-live --test init_command --test config -- --test-threads=1
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/venue-polymarket/src/lib.rs \
  crates/venue-polymarket/src/auth.rs \
  crates/venue-polymarket/src/rest.rs \
  crates/venue-polymarket/src/ws_client.rs \
  crates/app-live/src/commands/init/render.rs \
  crates/app-live/tests/init_command.rs \
  crates/app-live/tests/config.rs \
  config/axiom-arb.example.toml \
  README.md \
  docs/runbooks/real-user-shadow-smoke.md
git commit -m "refactor: remove legacy polymarket mainline path"
```

### Task 5: Run Post-Cutover End-To-End Verification

**Files:**
- Modify: `crates/app-live/tests/doctor_command.rs`
- Modify: `crates/app-live/tests/bootstrap_command.rs`
- Modify: `crates/app-live/tests/real_user_shadow_smoke.rs`
- Modify: `crates/app-live/tests/run_command.rs`

- [ ] **Step 1: Add final end-to-end assertions**

```rust
#[test]
fn apply_start_uses_gateway_backed_submit_and_stream_setup() {
    let output = run_apply_start_with_scripted_gateway();
    assert!(output.contains("Starting runtime in the foreground."));
}
```

- [ ] **Step 2: Run the final end-to-end verification suite**

Run:

```bash
cargo fmt --all
cargo test -p app-live --test doctor_command --test bootstrap_command --test real_user_shadow_smoke --test run_command -- --test-threads=1
cargo test -p config-schema --test validated_views --test config_roundtrip -- --test-threads=1
cargo test -p venue-polymarket -- --test-threads=1
cargo clippy -p app-live -p config-schema -p venue-polymarket --tests -- -D warnings
```

Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/app-live/tests/doctor_command.rs \
  crates/app-live/tests/bootstrap_command.rs \
  crates/app-live/tests/real_user_shadow_smoke.rs \
  crates/app-live/tests/run_command.rs
git commit -m "test: verify polymarket gateway cutover end to end"
```
