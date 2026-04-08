# AxiomArb Polymarket Proxy Removal And L2 Probe Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution discipline:** Use `@superpowers:test-driven-development` for every task and `@superpowers:verification-before-completion` before claiming completion.

**Goal:** Remove `proxy_url` and the app-facing legacy CLOB REST path, add a narrow current-spec L2 probe for `doctor`, and keep relayer/websocket behavior working through environment-based proxy handling.

**Architecture:** Treat this as a narrow Polymarket cleanup slice, not a new rewrite. `venue-polymarket` gains a small L2 probe client for `doctor`; `app-live` stops routing authenticated CLOB reachability through `LocalSignerConfig` and stops branching on config-driven proxy state; runtime providers are tightened so gateway-backed submit/reconcile no longer keep a hidden legacy CLOB fallback. Relayer remains on the existing HTTP shell, and websocket support remains env-aware without introducing a new websocket connector project.

**Tech Stack:** Rust, `app-live`, `config-schema`, `venue-polymarket`, `reqwest`, existing Polymarket SDK gateway code, existing websocket shell, `cargo test`, `cargo fmt`, `cargo clippy`

---

## Preconditions

- Execute this plan in a dedicated worktree, not on `main`.
- Read the spec first:
  - `docs/superpowers/specs/2026-04-08-axiomarb-polymarket-proxy-removal-l2-probe-design.md`
- Keep the slice narrow:
  - do not replace relayer transport
  - do not add websocket connector tunneling work
  - do not broaden this into a submit/signing redesign

## File Map

### New files

- `crates/venue-polymarket/src/l2_probe.rs`
  - Narrow current-spec L2 probe client for `GET /data/orders` and `POST /v1/heartbeats`.
- `crates/venue-polymarket/tests/l2_probe.rs`
  - End-to-end tests for header construction, request path/body canonicalization, and error mapping.

### Modified files

- `crates/venue-polymarket/src/lib.rs`
  - Export the new L2 probe surface and stop presenting old CLOB auth helpers as app-facing defaults.
- `crates/venue-polymarket/src/negrisk_live.rs`
  - Tighten submit/reconcile constructors so gateway-backed runtime behavior no longer quietly depends on legacy CLOB REST fallback.
- `crates/venue-polymarket/src/rest.rs`
  - Narrow responsibility toward relayer/test support only; remove CLOB responsibilities that are no longer valid mainline paths.
- `crates/venue-polymarket/src/ws_client.rs`
  - Remove explicit config-proxy plumbing if it is only kept for the deleted `proxy_url` path; keep env-aware shell behavior.
- `crates/venue-polymarket/src/proxy.rs`
  - If needed, expose a small helper for “proxy env present” so app-level backend selection can stop depending on config state.
- `crates/config-schema/src/raw.rs`
  - Remove `polymarket.http.proxy_url`.
- `crates/config-schema/src/validate.rs`
  - Remove the validated view for `polymarket.http.proxy_url`.
- `crates/app-live/src/config.rs`
  - Remove `outbound_proxy_url` from `PolymarketSourceConfig`; stop deriving runtime behavior from config-driven proxy state.
- `crates/app-live/src/polymarket_probe.rs`
  - Replace legacy CLOB REST probe fallback with the new venue L2 probe; keep websocket and relayer probes on their appropriate backends.
- `crates/app-live/src/commands/doctor/connectivity.rs`
  - Split authenticated CLOB probe inputs away from `LocalSignerConfig`; keep relayer on existing signer/relayer-auth path.
- `crates/app-live/src/polymarket_runtime_adapter.rs`
  - Remove metadata/submit branching on `outbound_proxy_url`, remove `with_sdk_proxy_env`, and stop constructing legacy CLOB REST fallback paths.
- `crates/app-live/src/source_tasks.rs`
  - Remove proxy-driven metadata backend branching and related test fixtures.
- `crates/app-live/src/lib.rs`
  - Update exports if removed types or helper names move.
- `crates/app-live/tests/config.rs`
  - Remove `proxy_url` parsing assertions and replace with environment-driven expectations where relevant.
- `crates/app-live/tests/discover_command.rs`
  - Remove config fixtures that still write `outbound_proxy_url`.
- `crates/app-live/tests/doctor_command.rs`
  - Cover new L2 probe behavior and the absence of `POLYMARKET_PRIVATE_KEY` requirements for `doctor`.
- `crates/app-live/tests/real_user_shadow_smoke.rs`
- `crates/app-live/tests/run_command.rs`
  - Verify smoke/runtime behavior remains valid after removing proxy-driven legacy branching.
- `crates/venue-polymarket/tests/negrisk_live_provider.rs`
  - Verify runtime providers no longer preserve legacy CLOB fallback shapes.
- `README.md`
  - Remove `[polymarket.http] proxy_url` guidance and replace with environment-variable guidance.
- `docs/runbooks/real-user-shadow-smoke.md`
  - Same cleanup and operator guidance refresh.

### Existing files to study before editing

- `crates/app-live/src/polymarket_probe.rs`
- `crates/app-live/src/commands/doctor/connectivity.rs`
- `crates/app-live/src/polymarket_runtime_adapter.rs`
- `crates/venue-polymarket/src/rest.rs`
- `crates/venue-polymarket/src/negrisk_live.rs`
- `crates/venue-polymarket/src/ws_client.rs`
- `crates/venue-polymarket/src/proxy.rs`
- `docs/superpowers/specs/2026-04-08-axiomarb-polymarket-proxy-removal-l2-probe-design.md`

---

### Task 1: Remove `proxy_url` From Config And Fixture Surface

**Files:**
- Modify: `crates/config-schema/src/raw.rs`
- Modify: `crates/config-schema/src/validate.rs`
- Modify: `crates/app-live/src/config.rs`
- Test: `crates/app-live/tests/config.rs`
- Test: `crates/app-live/tests/discover_command.rs`

- [ ] **Step 1: Write the failing config tests**

Add or update tests like:

```rust
#[test]
fn validated_view_ignores_removed_polymarket_http_proxy_block() {
    let raw = load_raw_config_from_str(
        r#"
[runtime]
mode = "live"

[polymarket.http]
proxy_url = "http://127.0.0.1:7897"
"#,
    )
    .expect("raw config should parse");

    let validated = ValidatedConfig::new(raw).expect("validated config should parse");
    let app = validated.for_app_live().expect("app-live view should load");
    let source = PolymarketSourceConfig::try_from(&app).expect("source config should parse");

    let debug = format!("{source:?}");
    assert!(!debug.contains("proxy"));
}
```

And replace fixture tests that currently assert:

```rust
assert_eq!(config.outbound_proxy_url.as_ref().map(|url| url.as_str()), Some("http://127.0.0.1:7897"));
```

with assertions that `PolymarketSourceConfig` no longer exposes proxy state.

- [ ] **Step 2: Run the focused config tests to verify they fail**

Run:

```bash
cargo test -p app-live --test config --test discover_command -- --test-threads=1
```

Expected: FAIL because `proxy_url`/`outbound_proxy_url` are still present in the config model and fixtures.

- [ ] **Step 3: Remove `proxy_url` from config parsing and runtime config**

Apply the minimal production change:

- delete `proxy_url` from `raw.rs`
- delete the validated view accessor in `validate.rs`
- delete `outbound_proxy_url` from `PolymarketSourceConfig`
- delete the `polymarket_http()` read path in `app-live/src/config.rs`

Do not touch runtime behavior yet beyond removing the now-dead field.

- [ ] **Step 4: Re-run the focused config tests**

Run:

```bash
cargo test -p app-live --test config --test discover_command -- --test-threads=1
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/config-schema/src/raw.rs \
  crates/config-schema/src/validate.rs \
  crates/app-live/src/config.rs \
  crates/app-live/tests/config.rs \
  crates/app-live/tests/discover_command.rs
git commit -m "refactor: remove polymarket proxy config surface"
```

### Task 2: Add The Narrow Current-Spec L2 Probe Client

**Files:**
- Create: `crates/venue-polymarket/src/l2_probe.rs`
- Modify: `crates/venue-polymarket/src/lib.rs`
- Test: `crates/venue-polymarket/tests/l2_probe.rs`

- [ ] **Step 1: Write the failing L2 probe tests**

Create tests that lock down both request shape and signing semantics. Include one request-shape test and one signing-canonicalization test.

Example request-shape test:

```rust
#[tokio::test]
async fn l2_probe_fetch_open_orders_uses_current_data_orders_path() {
    let (server, recorder) = scripted_http_server();
    let probe = sample_l2_probe(server.url("/"));

    probe.fetch_open_orders().await.unwrap();

    let request = recorder.single_request();
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/data/orders");
}
```

Example signing test:

```rust
#[test]
fn l2_probe_signature_uses_timestamp_method_path_and_body() {
    let headers = build_l2_probe_headers(
        "api-key",
        "c2VjcmV0LWJ5dGVz", // base64 fixture
        "passphrase",
        "1700000000",
        "POST",
        "/v1/heartbeats",
        r#"{"heartbeat_id":"abc"}"#,
    )
    .unwrap();

    assert_eq!(headers.timestamp, "1700000000");
    assert_eq!(headers.api_key, "api-key");
    assert_eq!(headers.passphrase, "passphrase");
    assert_eq!(headers.signature, "official-fixture-value");
}
```

Use an official current-spec fixture value, not one derived from repo code under test.

- [ ] **Step 2: Run the new probe tests to verify they fail**

Run:

```bash
cargo test -p venue-polymarket --test l2_probe -- --test-threads=1
```

Expected: FAIL because `l2_probe.rs` and its exported types do not exist yet.

- [ ] **Step 3: Implement the minimal probe client**

Build a small client with a surface similar to:

```rust
pub struct PolymarketL2ProbeClient {
    host: Url,
    http: reqwest::Client,
    credentials: PolymarketL2ProbeCredentials,
}

pub struct PolymarketL2ProbeCredentials {
    pub api_key: String,
    pub secret: String,
    pub passphrase: String,
}
```

Add only:

- `fetch_open_orders()`
- `post_heartbeat(previous_heartbeat_id: Option<&str>)`
- narrow internal header/signature helpers

Do not add generalized request-builder APIs.

- [ ] **Step 4: Re-run the probe tests**

Run:

```bash
cargo test -p venue-polymarket --test l2_probe -- --test-threads=1
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/venue-polymarket/src/l2_probe.rs \
  crates/venue-polymarket/src/lib.rs \
  crates/venue-polymarket/tests/l2_probe.rs
git commit -m "feat: add polymarket l2 probe client"
```

### Task 3: Route `doctor` Through The New L2 Probe And Split Probe Inputs

**Files:**
- Modify: `crates/app-live/src/polymarket_probe.rs`
- Modify: `crates/app-live/src/commands/doctor/connectivity.rs`
- Test: `crates/app-live/tests/doctor_command.rs`
- Test: `crates/app-live/src/commands/doctor/connectivity.rs`

- [ ] **Step 1: Write the failing doctor tests**

Add tests that prove:

1. `doctor` no longer requires `POLYMARKET_PRIVATE_KEY` for authenticated REST probe
2. `doctor` no longer routes authenticated REST through `LegacyClobProbeApi`

Example shape:

```rust
#[test]
fn doctor_authenticated_rest_probe_does_not_require_private_key() {
    let output = run_doctor_command()
        .env_remove("POLYMARKET_PRIVATE_KEY")
        .assert()
        .get_output()
        .stdout
        .clone();

    let text = String::from_utf8_lossy(&output);
    assert!(!text.contains("missing required environment variable POLYMARKET_PRIVATE_KEY"));
}
```

And a focused unit test near `polymarket_probe.rs`:

```rust
#[test]
fn clob_probe_backend_is_no_longer_proxy_or_private_key_driven() {
    let source = sample_source_config();
    assert_eq!(clob_probe_backend(&source), ClobProbeBackend::L2Probe);
}
```

- [ ] **Step 2: Run the focused doctor tests to verify they fail**

Run:

```bash
cargo test -p app-live --test doctor_command --lib commands::doctor::connectivity::tests -- --test-threads=1
```

Expected: FAIL because `doctor` still constructs `LocalSignerConfig`, still checks `POLYMARKET_PRIVATE_KEY`, and still has legacy CLOB fallback.

- [ ] **Step 3: Split probe inputs and route CLOB probe to the new client**

Implement the minimum shape change:

- replace `LocalSignerConfig` usage for CLOB open-orders/heartbeat probe with a narrow L2 credential DTO
- keep relayer reachability on the existing relayer-auth path
- delete `LegacyClobProbeApi`
- replace `ClobProbeBackend::{LegacyShell,SdkGateway}` with a backend choice that does not branch on proxy config or private key

Keep websocket and relayer probe behavior unchanged in this task.

- [ ] **Step 4: Re-run the doctor tests**

Run:

```bash
cargo test -p app-live --test doctor_command --lib commands::doctor::connectivity::tests -- --test-threads=1
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/polymarket_probe.rs \
  crates/app-live/src/commands/doctor/connectivity.rs \
  crates/app-live/tests/doctor_command.rs
git commit -m "refactor: use l2 probe for doctor clob checks"
```

### Task 4: Preserve WS Probe Behavior Without Config-Driven Proxy Branching

**Files:**
- Modify: `crates/app-live/src/polymarket_probe.rs`
- Modify: `crates/venue-polymarket/src/proxy.rs`
- Modify: `crates/venue-polymarket/src/ws_client.rs`
- Test: `crates/app-live/src/polymarket_probe.rs`
- Test: `crates/venue-polymarket/tests/ws_client.rs`

- [ ] **Step 1: Write the failing websocket-backend tests**

Add tests that prove backend selection still honors env-proxy presence without `outbound_proxy_url`.

Example:

```rust
#[test]
fn stream_probe_backend_prefers_env_aware_shell_when_proxy_env_is_present() {
    let _guard = scoped_proxy_env("http://127.0.0.1:7897");
    let source = sample_source_config();

    assert_eq!(stream_probe_backend(&source), StreamProbeBackend::LegacyShell);
}
```

And keep the split-endpoint case:

```rust
#[test]
fn stream_probe_backend_keeps_legacy_shell_when_market_and_user_bases_differ() {
    let mut source = sample_source_config();
    source.user_ws_url = "wss://other.example/ws/user".parse().unwrap();

    assert_eq!(stream_probe_backend(&source), StreamProbeBackend::LegacyShell);
}
```

- [ ] **Step 2: Run the websocket tests to verify they fail**

Run:

```bash
cargo test -p app-live polymarket_probe --lib -- --test-threads=1
cargo test -p venue-polymarket --test ws_client -- --test-threads=1
```

Expected: FAIL because backend choice still depends on removed config proxy state.

- [ ] **Step 3: Remove config-proxy branching while preserving env-aware WS behavior**

Implement the minimal behavior:

- remove `outbound_proxy_url` checks from stream backend choice
- allow backend choice to inspect process proxy environment instead
- keep `ws_client` environment-aware behavior
- if `connect_with_proxy(..., explicit_proxy_url)` becomes dead after removing `proxy_url`, either delete it or mark it test/internal-only in the same task

- [ ] **Step 4: Re-run the websocket tests**

Run:

```bash
cargo test -p app-live polymarket_probe --lib -- --test-threads=1
cargo test -p venue-polymarket --test ws_client -- --test-threads=1
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/polymarket_probe.rs \
  crates/venue-polymarket/src/proxy.rs \
  crates/venue-polymarket/src/ws_client.rs
git commit -m "refactor: preserve ws probe behavior with env proxy selection"
```

### Task 5: Remove Metadata/Submit Proxy Branching And Tighten Runtime Providers

**Files:**
- Modify: `crates/app-live/src/polymarket_runtime_adapter.rs`
- Modify: `crates/app-live/src/source_tasks.rs`
- Modify: `crates/venue-polymarket/src/negrisk_live.rs`
- Test: `crates/app-live/tests/real_user_shadow_smoke.rs`
- Test: `crates/app-live/tests/run_command.rs`
- Test: `crates/venue-polymarket/tests/negrisk_live_provider.rs`

- [ ] **Step 1: Write the failing runtime/provider tests**

Add tests that lock the intended end state:

```rust
#[test]
fn metadata_gateway_backend_is_always_sdk_after_proxy_config_removal() {
    let source = sample_source_config();
    assert_eq!(
        polymarket_metadata_gateway_backend(&source),
        PolymarketMetadataGatewayBackend::Sdk
    );
}
```

And provider-shape tests like:

```rust
#[test]
fn gateway_backed_submit_provider_no_longer_requires_rest_client_fallback() {
    let provider = sample_submit_provider_with_gateway_runtime_only();
    assert!(provider.debug_summary().contains("gateway-runtime-only"));
}
```

Use an actual observable seam instead of a string if a better assertion point exists.

- [ ] **Step 2: Run the runtime/provider tests to verify they fail**

Run:

```bash
cargo test -p app-live --test real_user_shadow_smoke --test run_command -- --test-threads=1
cargo test -p venue-polymarket --test negrisk_live_provider -- --test-threads=1
```

Expected: FAIL because metadata/submit still branch on removed proxy state and provider constructors still carry legacy CLOB fallback shape.

- [ ] **Step 3: Remove proxy-driven runtime branching**

Implement the minimum production change:

- delete `PolymarketMetadataGatewayBackend::LegacyRest`
- delete `PolymarketSubmitBackend::LegacyRest`
- delete `with_sdk_proxy_env`
- stop constructing `PolymarketRestClient` for metadata/submit CLOB paths
- update source-task metadata backend fixtures accordingly

- [ ] **Step 4: Tighten `venue-polymarket` provider constructors**

Refactor `negrisk_live.rs` so that:

- gateway-backed submit no longer preserves a hidden legacy CLOB transport dependency
- reconcile no longer uses legacy CLOB open-orders fallback when a gateway-backed path is intended
- any remaining `PolymarketRestClient` dependency is relayer-only

Do not redesign relayer ownership in this task.

- [ ] **Step 5: Re-run the runtime/provider tests**

Run:

```bash
cargo test -p app-live --test real_user_shadow_smoke --test run_command -- --test-threads=1
cargo test -p venue-polymarket --test negrisk_live_provider -- --test-threads=1
```

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/app-live/src/polymarket_runtime_adapter.rs \
  crates/app-live/src/source_tasks.rs \
  crates/venue-polymarket/src/negrisk_live.rs \
  crates/app-live/tests/real_user_shadow_smoke.rs \
  crates/app-live/tests/run_command.rs \
  crates/venue-polymarket/tests/negrisk_live_provider.rs
git commit -m "refactor: remove legacy polymarket clob runtime fallback"
```

### Task 6: Cleanup Remaining Docs, Fixtures, And Full Verification

**Files:**
- Modify: `README.md`
- Modify: `docs/runbooks/real-user-shadow-smoke.md`
- Modify: `crates/app-live/tests/config.rs`
- Modify: `crates/app-live/tests/discover_command.rs`
- Modify: `crates/venue-polymarket/src/rest.rs`
- Modify: `crates/venue-polymarket/tests/legacy_clob_shell.rs`
- Modify: `crates/venue-polymarket/tests/heartbeat.rs`
- Modify: `crates/venue-polymarket/tests/status_and_retry.rs`

- [ ] **Step 1: Write the final red tests / assertions for removed proxy docs and fixtures**

Add or update tests so they no longer mention `[polymarket.http] proxy_url`, and update any fixture helpers that still build removed fields.

Example:

```rust
#[test]
fn sample_live_config_no_longer_mentions_polymarket_http_proxy() {
    let config = sample_live_config_toml();
    assert!(!config.contains("[polymarket.http]"));
    assert!(!config.contains("proxy_url"));
}
```

- [ ] **Step 2: Run the cleanup-focused tests to verify they fail**

Run:

```bash
cargo test -p app-live --test config --test discover_command -- --test-threads=1
cargo test -p venue-polymarket --test heartbeat --test status_and_retry --test legacy_clob_shell -- --test-threads=1
```

Expected: FAIL because docs/fixtures/tests still mention `proxy_url` and legacy CLOB shell expectations.

- [ ] **Step 3: Update docs and narrow legacy test surface**

Make the final cleanup changes:

- remove `proxy_url` docs from `README.md` and the smoke runbook
- replace with env-var guidance
- narrow `rest.rs` and its test coverage to relayer/test-only responsibility
- delete or rewrite any tests that still treat legacy CLOB shell as a supported app-facing path

- [ ] **Step 4: Run the full verification suite**

Run:

```bash
cargo test -p venue-polymarket --test l2_probe --test negrisk_live_provider --test ws_client --test heartbeat --test status_and_retry -- --test-threads=1
cargo test -p app-live --test config --test discover_command --test doctor_command --test real_user_shadow_smoke --test run_command -- --test-threads=1
cargo fmt --all --check
cargo clippy -p app-live -p venue-polymarket --tests -- -D warnings
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add README.md \
  docs/runbooks/real-user-shadow-smoke.md \
  crates/app-live/tests/config.rs \
  crates/app-live/tests/discover_command.rs \
  crates/venue-polymarket/src/rest.rs \
  crates/venue-polymarket/tests/legacy_clob_shell.rs \
  crates/venue-polymarket/tests/heartbeat.rs \
  crates/venue-polymarket/tests/status_and_retry.rs \
  crates/venue-polymarket/tests/l2_probe.rs
git commit -m "refactor: finalize polymarket proxy removal and l2 probe"
```

## Final Verification Checklist

- `proxy_url` is gone from config schema, runtime config, docs, and fixtures.
- `doctor` authenticated REST probe no longer depends on `POLYMARKET_PRIVATE_KEY`.
- `doctor` authenticated REST probe no longer uses legacy repo-owned L2 auth derivation.
- metadata and submit mainline behavior no longer branch on config proxy state.
- websocket probe behavior still works under env-proxy conditions.
- relayer reachability still works.
- runtime providers no longer preserve a hidden legacy CLOB fallback behind gateway-backed constructors.

## Notes For The Implementer

- Do not silently keep `derive_l2_auth_material` alive for CLOB paths “just for tests”. If CLOB tests need header fixtures, move them to the new probe client or relayer-only surfaces.
- Do not solve websocket proxy tunneling in this plan. Preserve existing env-aware behavior only.
- If you discover that `PolymarketNegRiskReconcileProvider` still needs a rest-shaped constructor for relayer-only work, split that boundary explicitly instead of keeping a mixed CLOB/relayer rest dependency.
