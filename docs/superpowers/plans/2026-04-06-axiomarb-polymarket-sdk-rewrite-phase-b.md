# AxiomArb Polymarket SDK Rewrite Phase B Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution discipline:** Use `@superpowers:test-driven-development` for every task and `@superpowers:verification-before-completion` before claiming completion.

**Goal:** Rebaseline the in-flight Phase B branch onto current `main`, finish the remaining app-facing gateway cutover, remove legacy Polymarket transport surfaces from the mainline path, and close with end-to-end verification.

**Architecture:** Treat the remaining work as four explicit slices. First, align `polymarket-sdk-rewrite-phase-b` with the post-`strategy-neutral-control-plane` `main` tip without adding new behavior. Second, finish the last app-facing construction ownership cutover so `app-live` mainline paths no longer directly construct legacy Polymarket REST/WS clients. Third, delete stale legacy surfaces only after the app-facing path is clean. Fourth, run the full app/venue verification pass and update operator-facing docs. The Phase A websocket compatibility shell remains an internal seam until the stateful SDK subscription model is the only app-facing path.

**Tech Stack:** Rust, `app-live`, `config-schema`, `venue-polymarket`, `cargo test`, `cargo fmt`, `cargo clippy`, git rebase/merge tooling

---

## Preconditions

- Phase A is already merged.
- `strategy-neutral-control-plane` is already merged into `main`.
- This plan executes in the dedicated worktree branch:
  - `/Users/viv/projs/axiom-arb/.worktrees/polymarket-sdk-rewrite-phase-b`
  - branch `polymarket-sdk-rewrite-phase-b`
- Commit this refreshed plan before executing Task 1 so the worktree is clean again.
- Before starting Task 1, confirm the branch is still behind current `main` and inspect duplicate patches:

```bash
git rev-list --left-right --count main...polymarket-sdk-rewrite-phase-b
git cherry -v main HEAD | sed -n '1,40p'
```

Expected:
- non-zero divergence
- duplicate patches may appear; let rebase drop them instead of reintroducing them manually

## File Map

### Alignment hotspots

- `crates/app-live/Cargo.toml`
- `crates/app-live/src/config.rs`
- `crates/app-live/src/lib.rs`
- `crates/app-live/src/commands/discover.rs`
- `crates/app-live/src/commands/doctor/connectivity.rs`
- `crates/app-live/src/commands/run.rs`
- `crates/app-live/src/daemon.rs`
- `crates/app-live/src/negrisk_live.rs`
- `crates/app-live/src/polymarket_probe.rs`
- `crates/app-live/src/polymarket_runtime_adapter.rs`
- `crates/app-live/src/supervisor.rs`
- `crates/app-live/tests/bootstrap_command.rs`
- `crates/app-live/tests/candidate_daemon.rs`
- `crates/app-live/tests/config.rs`
- `crates/app-live/tests/discover_command.rs`
- `crates/app-live/tests/doctor_command.rs`
- `crates/app-live/tests/main_entrypoint.rs`
- `crates/app-live/tests/support/apply_db.rs`
- `crates/app-live/tests/support/discover_db.rs`
- `crates/config-schema/src/validate.rs`
- `crates/config-schema/tests/validated_views.rs`
- `crates/execution/src/signing.rs`
- `crates/venue-polymarket/src/metadata.rs`
- `crates/venue-polymarket/src/negrisk_live.rs`
- `crates/venue-polymarket/src/sdk_backend/clob.rs`
- `crates/venue-polymarket/tests/metadata.rs`
- `crates/venue-polymarket/tests/negrisk_live_provider.rs`
- `crates/venue-polymarket/tests/sdk_clob_gateway.rs`
- `crates/venue-polymarket/tests/support/mod.rs`

### Mainline cutover files

- `crates/app-live/src/source_tasks.rs`
  - Remaining app-facing entry point that still exports legacy REST client construction.
- `crates/app-live/src/polymarket_runtime_adapter.rs`
  - Owns the gateway-backed construction helpers and hidden legacy fallbacks.
- `crates/app-live/src/commands/run.rs`
  - Consumes smoke/live source bundles.
- `crates/app-live/tests/discover_command.rs`
- `crates/app-live/tests/real_user_shadow_smoke.rs`
  - Verify the source bundle and command paths still work after construction ownership moves.

### Legacy surface removal files

- `crates/venue-polymarket/src/lib.rs`
- `crates/app-live/src/lib.rs`
- `crates/app-live/src/polymarket_probe.rs`
- `crates/app-live/src/polymarket_runtime_adapter.rs`
- `crates/venue-polymarket/src/metadata.rs`
- `crates/venue-polymarket/src/negrisk_live.rs`
- `crates/app-live/src/commands/init/render.rs`
- `crates/app-live/tests/init_command.rs`
- `crates/app-live/tests/config.rs`
- `config/axiom-arb.example.toml`
- `README.md`
- `docs/runbooks/real-user-shadow-smoke.md`

### Verification files

- `crates/app-live/tests/apply_command.rs`
- `crates/app-live/tests/bootstrap_command.rs`
- `crates/app-live/tests/candidate_daemon.rs`
- `crates/app-live/tests/doctor_command.rs`
- `crates/app-live/tests/discover_command.rs`
- `crates/app-live/tests/ingest_task_groups.rs`
- `crates/app-live/tests/real_user_shadow_smoke.rs`
- `crates/app-live/tests/run_command.rs`
- `crates/config-schema/tests/config_roundtrip.rs`
- `crates/config-schema/tests/validated_views.rs`
- `crates/venue-polymarket/tests/gateway_surface.rs`
- `crates/venue-polymarket/tests/negrisk_live_provider.rs`
- `crates/venue-polymarket/tests/sdk_clob_gateway.rs`
- `crates/venue-polymarket/tests/sdk_metadata_gateway.rs`
- `crates/venue-polymarket/tests/sdk_relayer_gateway.rs`
- `crates/venue-polymarket/tests/sdk_ws_gateway.rs`

---

### Task 1: Alignment / Rebaseline Onto Current `main`

**Files:**
- Modify: alignment hotspots listed above, only as required to complete the rebase and restore the already-implemented Phase B behavior
- Test: `crates/app-live/tests/config.rs`
- Test: `crates/app-live/tests/discover_command.rs`
- Test: `crates/app-live/tests/bootstrap_command.rs`
- Test: `crates/app-live/tests/doctor_command.rs`
- Test: `crates/app-live/tests/run_command.rs`
- Test: `crates/app-live/tests/real_user_shadow_smoke.rs`
- Test: `crates/app-live/tests/candidate_daemon.rs`
- Test: `crates/venue-polymarket/tests/negrisk_live_provider.rs`
- Test: `crates/venue-polymarket/tests/sdk_clob_gateway.rs`

- [ ] **Step 1: Capture the pre-alignment baseline**

Run:

```bash
git status --short
git rev-list --left-right --count main...HEAD
git cherry -v main HEAD | sed -n '1,40p'
```

Expected:
- clean worktree
- non-zero divergence from `main`
- duplicate-patch entries are acceptable and should be dropped by the rebase if Git determines they already landed on `main`

- [ ] **Step 2: Rebase the Phase B branch onto current `main`**

Run:

```bash
git rebase main
```

Expected:
- conflicts in the known hotspot files are acceptable
- do not introduce new cutover behavior while resolving them

- [ ] **Step 3: Run the post-rebase baseline suite to expose drift**

Run:

```bash
cargo test -p app-live --test config --test discover_command --test bootstrap_command --test doctor_command --test run_command --test real_user_shadow_smoke --test candidate_daemon -- --test-threads=1
cargo test -p app-live polymarket_probe --lib -- --test-threads=1
cargo test -p venue-polymarket --test negrisk_live_provider --test sdk_clob_gateway -- --test-threads=1
cargo clippy -p app-live --tests -- -D warnings
```

Expected: at least one failure if the rebase surfaced drift; green output only if the branch happened to replay cleanly.

- [ ] **Step 4: Repair only rebase-induced breakage**

Rules:

- keep this slice strictly non-semantic
- allow conflict resolution, compile fixes, fixture refreshes, and adapter rewiring needed to preserve behavior already implemented on the branch
- do not remove legacy exports
- do not add new gateway cutover behavior beyond what the branch already had before rebase

- [ ] **Step 5: Re-run the baseline suite**

Run:

```bash
cargo test -p app-live --test config --test discover_command --test bootstrap_command --test doctor_command --test run_command --test real_user_shadow_smoke --test candidate_daemon -- --test-threads=1
cargo test -p app-live polymarket_probe --lib -- --test-threads=1
cargo test -p venue-polymarket --test negrisk_live_provider --test sdk_clob_gateway -- --test-threads=1
cargo clippy -p app-live --tests -- -D warnings
```

Expected: PASS

- [ ] **Step 6: Commit any explicit post-rebase repair changes**

```bash
git add crates/app-live/Cargo.toml \
  crates/app-live/src/config.rs \
  crates/app-live/src/lib.rs \
  crates/app-live/src/commands/discover.rs \
  crates/app-live/src/commands/doctor/connectivity.rs \
  crates/app-live/src/commands/run.rs \
  crates/app-live/src/daemon.rs \
  crates/app-live/src/negrisk_live.rs \
  crates/app-live/src/polymarket_probe.rs \
  crates/app-live/src/polymarket_runtime_adapter.rs \
  crates/app-live/src/supervisor.rs \
  crates/app-live/tests/bootstrap_command.rs \
  crates/app-live/tests/candidate_daemon.rs \
  crates/app-live/tests/config.rs \
  crates/app-live/tests/discover_command.rs \
  crates/app-live/tests/doctor_command.rs \
  crates/app-live/tests/main_entrypoint.rs \
  crates/app-live/tests/support/apply_db.rs \
  crates/app-live/tests/support/discover_db.rs \
  crates/config-schema/src/validate.rs \
  crates/config-schema/tests/validated_views.rs \
  crates/execution/src/signing.rs \
  crates/venue-polymarket/src/metadata.rs \
  crates/venue-polymarket/src/negrisk_live.rs \
  crates/venue-polymarket/src/sdk_backend/clob.rs \
  crates/venue-polymarket/tests/metadata.rs \
  crates/venue-polymarket/tests/negrisk_live_provider.rs \
  crates/venue-polymarket/tests/sdk_clob_gateway.rs \
  crates/venue-polymarket/tests/support/mod.rs
git commit -m "chore: rebaseline polymarket phase b on current main"
```

If the rebase replayed cleanly and needed no extra follow-up commit, record that outcome in the execution log and continue.

### Task 2: Finish The Remaining App-Live Mainline Cutover

**Files:**
- Modify: `crates/app-live/src/source_tasks.rs`
- Modify: `crates/app-live/src/polymarket_runtime_adapter.rs`
- Modify: `crates/app-live/src/commands/run.rs`
- Modify: `crates/app-live/src/lib.rs`
- Test: `crates/app-live/tests/discover_command.rs`
- Test: `crates/app-live/tests/real_user_shadow_smoke.rs`
- Test: `crates/app-live/tests/run_command.rs`

- [ ] **Step 1: Write the failing cutover tests**

Add focused tests that prove `app-live` mainline source construction no longer owns legacy transport creation:

```rust
#[test]
fn smoke_source_builder_uses_adapter_owned_metadata_backend_selection() {
    let sources = build_real_user_shadow_smoke_sources(
        smoke_source_config(None),
        sample_signer_config(),
        "run-session-1",
    )
    .unwrap();

    assert_eq!(sources.metadata_backend_for_tests(), "sdk");
}

#[test]
fn smoke_source_builder_keeps_proxy_fallback_hidden_behind_the_adapter() {
    let sources = build_real_user_shadow_smoke_sources(
        smoke_source_config(Some("http://127.0.0.1:7897")),
        sample_signer_config(),
        "run-session-2",
    )
    .unwrap();

    assert_eq!(sources.metadata_backend_for_tests(), "legacy-rest");
}
```

Also extend one command-level test so `discover` or `run` still succeeds through the shared source bundle after the builder move.

- [ ] **Step 2: Run the cutover tests to verify they fail**

Run:

```bash
cargo test -p app-live smoke_source_builder_uses_adapter_owned_metadata_backend_selection --lib -- --test-threads=1
cargo test -p app-live smoke_source_builder_keeps_proxy_fallback_hidden_behind_the_adapter --lib -- --test-threads=1
cargo test -p app-live --test discover_command --test real_user_shadow_smoke --test run_command -- --test-threads=1
```

Expected: FAIL because `source_tasks.rs` still exports `build_polymarket_rest_client` and the source bundle does not expose adapter-owned backend selection. Do not use `ingest_task_groups` as the red signal in this slice; it currently contains stale signer-based fixture data and belongs in the alignment/final verification work.

- [ ] **Step 3: Move construction ownership into the adapter**

Target shape:

```rust
pub(crate) enum SmokeMetadataBackend {
    Sdk,
    LegacyRest,
}

pub(crate) struct RealUserShadowSmokeSources {
    pub source_config: PolymarketSourceConfig,
    pub signer_config: LocalSignerConfig,
    pub market: MarketDataTaskGroup,
    pub user: UserStateTaskGroup,
    pub heartbeat: HeartbeatTaskGroup<SmokeHeartbeatSource>,
    pub relayer: RelayerTaskGroup,
    pub metadata: MetadataTaskGroup,
    metadata_backend: SmokeMetadataBackend,
    bootstrap_snapshot: RemoteSnapshot,
}
```

Rules:

- `app-live` mainline files must stop importing or exporting `PolymarketRestClient` and `build_polymarket_rest_client`
- explicit proxy and split-endpoint fallbacks may remain, but only behind `polymarket_runtime_adapter.rs` or `polymarket_probe.rs`
- do not delete legacy transport code in this slice
- keep `doctor` on the existing facade path; do not reopen that file unless the alignment slice made it necessary

- [ ] **Step 4: Re-run the cutover tests**

Run:

```bash
cargo test -p app-live smoke_source_builder_uses_adapter_owned_metadata_backend_selection --lib -- --test-threads=1
cargo test -p app-live smoke_source_builder_keeps_proxy_fallback_hidden_behind_the_adapter --lib -- --test-threads=1
cargo test -p app-live --test discover_command --test real_user_shadow_smoke --test run_command -- --test-threads=1
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/source_tasks.rs \
  crates/app-live/src/polymarket_runtime_adapter.rs \
  crates/app-live/src/commands/run.rs \
  crates/app-live/src/lib.rs \
  crates/app-live/tests/discover_command.rs \
  crates/app-live/tests/real_user_shadow_smoke.rs \
  crates/app-live/tests/run_command.rs
git commit -m "refactor: finish app-live polymarket mainline cutover"
```

### Task 3: Remove App-Facing Legacy Surface And Update Operator Docs

**Files:**
- Modify: `crates/venue-polymarket/src/lib.rs`
- Modify: `crates/app-live/src/lib.rs`
- Modify: `crates/app-live/src/polymarket_probe.rs`
- Modify: `crates/app-live/src/polymarket_runtime_adapter.rs`
- Modify: `crates/venue-polymarket/src/metadata.rs`
- Modify: `crates/venue-polymarket/src/negrisk_live.rs`
- Modify: `crates/app-live/src/commands/init/render.rs`
- Modify: `crates/app-live/tests/init_command.rs`
- Modify: `crates/app-live/tests/config.rs`
- Modify: `config/axiom-arb.example.toml`
- Modify: `README.md`
- Modify: `docs/runbooks/real-user-shadow-smoke.md`

- [ ] **Step 1: Capture the remaining app-facing legacy footprint**

Run:

```bash
rg -n "PolymarketRestClient|PolymarketWsClient|build_polymarket_rest_client" \
  crates/app-live/src/source_tasks.rs \
  crates/app-live/src/commands \
  crates/app-live/src/lib.rs
rg -n "PolymarketRestClient|PolymarketWsClient|L2AuthHeaders|SignerContext" \
  crates/venue-polymarket/src/lib.rs
```

Expected:
- the audit still finds app-facing legacy imports, builders, or public reexports
- internal fallback matches under `polymarket_probe.rs`, `polymarket_runtime_adapter.rs`, `metadata.rs`, or `negrisk_live.rs` are acceptable until a later cleanup slice

- [ ] **Step 2: Remove the app-facing legacy surface**

Deletion guardrails:

- only do this after Task 2 lands
- `app-live` must no longer import `PolymarketRestClient`, `PolymarketWsClient`, or `build_polymarket_rest_client`
- hidden fallbacks may remain inside `polymarket_probe.rs`, `polymarket_runtime_adapter.rs`, `metadata.rs`, or `negrisk_live.rs` if still required for proxy-specific or compatibility behavior
- the Phase A websocket compatibility shell is not the final stateful subscription model, but it must remain internal rather than app-facing

- [ ] **Step 3: Re-run the footprint audit and targeted docs/config tests**

Run:

```bash
rg -n "PolymarketRestClient|PolymarketWsClient|build_polymarket_rest_client" \
  crates/app-live/src/source_tasks.rs \
  crates/app-live/src/commands \
  crates/app-live/src/lib.rs
rg -n "PolymarketRestClient|PolymarketWsClient|L2AuthHeaders|SignerContext" \
  crates/venue-polymarket/src/lib.rs
cargo test -p app-live --test init_command --test config -- --test-threads=1
```

Expected:
- no matches in the app-facing audit commands
- `init_command` and `config` stay green

- [ ] **Step 4: Commit**

```bash
git add crates/venue-polymarket/src/lib.rs \
  crates/app-live/src/lib.rs \
  crates/app-live/src/polymarket_probe.rs \
  crates/app-live/src/polymarket_runtime_adapter.rs \
  crates/venue-polymarket/src/metadata.rs \
  crates/venue-polymarket/src/negrisk_live.rs \
  crates/app-live/src/commands/init/render.rs \
  crates/app-live/tests/init_command.rs \
  crates/app-live/tests/config.rs \
  config/axiom-arb.example.toml \
  README.md \
  docs/runbooks/real-user-shadow-smoke.md
git commit -m "refactor: remove legacy polymarket app-facing surface"
```

### Task 4: Run Final Verification And Refresh Docs/Test Expectations

**Files:**
- Modify: `crates/app-live/tests/apply_command.rs`
- Modify: `crates/app-live/tests/bootstrap_command.rs`
- Modify: `crates/app-live/tests/candidate_daemon.rs`
- Modify: `crates/app-live/tests/doctor_command.rs`
- Modify: `crates/app-live/tests/discover_command.rs`
- Modify: `crates/app-live/tests/ingest_task_groups.rs`
- Modify: `crates/app-live/tests/real_user_shadow_smoke.rs`
- Modify: `crates/app-live/tests/run_command.rs`
- Modify: `crates/config-schema/tests/config_roundtrip.rs`
- Modify: `crates/config-schema/tests/validated_views.rs`
- Modify: `crates/venue-polymarket/tests/gateway_surface.rs`
- Modify: `crates/venue-polymarket/tests/negrisk_live_provider.rs`
- Modify: `crates/venue-polymarket/tests/sdk_clob_gateway.rs`
- Modify: `crates/venue-polymarket/tests/sdk_metadata_gateway.rs`
- Modify: `crates/venue-polymarket/tests/sdk_relayer_gateway.rs`
- Modify: `crates/venue-polymarket/tests/sdk_ws_gateway.rs`

- [ ] **Step 1: Add final end-to-end assertions**

```rust
#[test]
fn apply_start_uses_gateway_backed_submit_and_stream_setup() {
    let output = run_apply_start_with_scripted_gateway();
    assert!(output.contains("Starting runtime in the foreground."));
}

#[test]
fn doctor_reports_connectivity_through_the_gateway_backed_probe_path() {
    let output = run_doctor_with_scripted_gateway_success();
    assert!(output.contains("authenticated REST probe succeeded"));
    assert!(output.contains("market websocket probe succeeded"));
    assert!(output.contains("user websocket probe succeeded"));
}
```

- [ ] **Step 2: Run the final verification suite**

Run:

```bash
cargo fmt --all
cargo test -p app-live --lib -- --test-threads=1
cargo test -p app-live --test apply_command --test bootstrap_command --test candidate_daemon --test doctor_command --test discover_command --test ingest_task_groups --test real_user_shadow_smoke --test run_command -- --test-threads=1
cargo test -p config-schema --test config_roundtrip --test validated_views -- --test-threads=1
cargo test -p venue-polymarket --test gateway_surface --test negrisk_live_provider --test sdk_clob_gateway --test sdk_metadata_gateway --test sdk_relayer_gateway --test sdk_ws_gateway -- --test-threads=1
cargo clippy -p app-live -p config-schema -p venue-polymarket --tests -- -D warnings
```

Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/app-live/tests/apply_command.rs \
  crates/app-live/tests/bootstrap_command.rs \
  crates/app-live/tests/candidate_daemon.rs \
  crates/app-live/tests/doctor_command.rs \
  crates/app-live/tests/discover_command.rs \
  crates/app-live/tests/ingest_task_groups.rs \
  crates/app-live/tests/real_user_shadow_smoke.rs \
  crates/app-live/tests/run_command.rs \
  crates/config-schema/tests/config_roundtrip.rs \
  crates/config-schema/tests/validated_views.rs \
  crates/venue-polymarket/tests/gateway_surface.rs \
  crates/venue-polymarket/tests/negrisk_live_provider.rs \
  crates/venue-polymarket/tests/sdk_clob_gateway.rs \
  crates/venue-polymarket/tests/sdk_metadata_gateway.rs \
  crates/venue-polymarket/tests/sdk_relayer_gateway.rs \
  crates/venue-polymarket/tests/sdk_ws_gateway.rs
git commit -m "test: verify polymarket phase b cutover end to end"
```
