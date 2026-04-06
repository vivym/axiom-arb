# AxiomArb Polymarket SDK Rewrite Phase A Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution discipline:** Use `@superpowers:test-driven-development` for every task and `@superpowers:verification-before-completion` before claiming completion.

**Goal:** Build a reusable SDK-backed Polymarket protocol core inside `venue-polymarket` without cutting over `app-live` mainline call sites yet.

**Architecture:** Keep `venue-polymarket` as the only venue integration crate, but add an experimental capability-oriented gateway backed by `polymarket-client-sdk`. In Phase A, keep legacy exports available as a controlled compatibility shell while moving new protocol behavior and tests onto the new gateway internals.

**Tech Stack:** Rust, `polymarket-client-sdk`, existing `venue-polymarket` crate, `reqwest`/websocket test harnesses already in repo, `cargo test`, `cargo fmt`, `cargo clippy`

---

## File Map

### New files

- `crates/venue-polymarket/src/gateway.rs`
  - Experimental capability-oriented gateway and stable venue-facing DTOs such as signed order/query/cancel inputs.
- `crates/venue-polymarket/src/errors.rs`
  - Consolidated venue-level error categories for SDK-backed transport/auth/protocol/policy/relayer failures.
- `crates/venue-polymarket/src/sdk_backend/mod.rs`
  - Shared SDK backend assembly and internal traits/adapters.
- `crates/venue-polymarket/src/sdk_backend/clob.rs`
  - Official SDK-backed authenticated CLOB REST integration.
- `crates/venue-polymarket/src/sdk_backend/ws.rs`
  - Official SDK-backed market/user websocket integration and event projection helpers.
- `crates/venue-polymarket/src/sdk_backend/metadata.rs`
  - Official SDK-backed Gamma/Data fetch integration plus repo-owned metadata policy hooks.
- `crates/venue-polymarket/tests/gateway_surface.rs`
  - Coverage for the new experimental gateway surface, DTOs, and error model.
- `crates/venue-polymarket/tests/sdk_clob_gateway.rs`
  - Integration-style tests for SDK-backed authenticated REST behavior using repo test doubles/adapters.
- `crates/venue-polymarket/tests/sdk_ws_gateway.rs`
  - Integration-style tests for SDK-backed market/user websocket projection.
- `crates/venue-polymarket/tests/sdk_metadata_gateway.rs`
  - Integration-style tests for SDK-backed metadata fetches and malformed-row policy.

### Modified files

- `crates/venue-polymarket/Cargo.toml`
  - Add `polymarket-client-sdk` and any small supporting dependencies required by the new SDK backend.
- `crates/venue-polymarket/src/lib.rs`
  - Export the new experimental gateway surface while keeping legacy exports available during Phase A.
- `crates/venue-polymarket/src/rest.rs`
  - Either become a thin compatibility shell over the new gateway or stop owning the primary REST logic.
- `crates/venue-polymarket/src/ws_client.rs`
  - Either become a thin compatibility shell over the new gateway or stop owning the primary websocket logic.
- `crates/venue-polymarket/src/metadata.rs`
  - Keep repo-owned metadata policy and cache logic, but stop owning primary transport/session behavior.
- `crates/venue-polymarket/src/negrisk_live.rs`
  - Translate `SignedFamilySubmission` into venue-facing DTOs above the gateway without passing route-specific payloads into the gateway.
- `crates/venue-polymarket/src/relayer.rs`
  - Fit relayer access into the same gateway/error model without changing relayer protocol ownership.
- `crates/venue-polymarket/tests/support/mod.rs`
  - Add shared test doubles/builders for the new gateway/backend surface.
- `crates/venue-polymarket/tests/heartbeat.rs`
- `crates/venue-polymarket/tests/metadata.rs`
- `crates/venue-polymarket/tests/negrisk_live_provider.rs`
- `crates/venue-polymarket/tests/status_and_retry.rs`
- `crates/venue-polymarket/tests/ws_client.rs`
- `crates/venue-polymarket/tests/ws_feeds.rs`
  - Keep legacy-shell tests honest while migrating primary assertions to the new gateway tests.

### Existing files to study before editing

- `docs/superpowers/specs/2026-04-06-axiomarb-polymarket-sdk-rewrite-design.md`
- `crates/venue-polymarket/src/lib.rs`
- `crates/venue-polymarket/src/auth.rs`
- `crates/venue-polymarket/src/rest.rs`
- `crates/venue-polymarket/src/ws_client.rs`
- `crates/venue-polymarket/src/metadata.rs`
- `crates/venue-polymarket/src/negrisk_live.rs`
- `crates/venue-polymarket/src/relayer.rs`
- `crates/venue-polymarket/tests/support/mod.rs`

---

### Task 1: Add Experimental Gateway Surface And Error Model

**Files:**
- Modify: `crates/venue-polymarket/Cargo.toml`
- Modify: `crates/venue-polymarket/src/lib.rs`
- Create: `crates/venue-polymarket/src/gateway.rs`
- Create: `crates/venue-polymarket/src/errors.rs`
- Create: `crates/venue-polymarket/src/sdk_backend/mod.rs`
- Test: `crates/venue-polymarket/tests/gateway_surface.rs`

- [ ] **Step 1: Write the failing gateway-surface tests**

```rust
use venue_polymarket::{
    PolymarketGatewayError, PolymarketOrderQuery, PolymarketSignedOrder,
};

#[test]
fn gateway_surface_exposes_route_agnostic_signed_order_dto() {
    let order = PolymarketSignedOrder {
        order: serde_json::json!({"tokenId": "token-1"}),
        owner: "0x1111111111111111111111111111111111111111".to_owned(),
        order_type: "GTC".to_owned(),
        defer_exec: false,
    };

    assert_eq!(order.owner, "0x1111111111111111111111111111111111111111");
}

#[test]
fn gateway_error_categories_are_stable() {
    let error = PolymarketGatewayError::auth("invalid api key");
    assert!(error.to_string().contains("invalid api key"));
}

#[test]
fn order_query_does_not_encode_route_specific_fields() {
    let query = PolymarketOrderQuery::open_orders();
    let debug = format!("{query:?}");
    assert!(!debug.contains("family"));
    assert!(!debug.contains("neg-risk"));
}
```

- [ ] **Step 2: Run the new tests to verify they fail**

Run:

```bash
cargo test -p venue-polymarket --test gateway_surface -- --test-threads=1
```

Expected: FAIL because the new gateway/error/DTO types do not exist yet.

- [ ] **Step 3: Add the minimal gateway surface and error model**

Implement an experimental surface like:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolymarketSignedOrder {
    pub order: serde_json::Value,
    pub owner: String,
    pub order_type: String,
    pub defer_exec: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolymarketGatewayErrorKind {
    Auth,
    Connectivity,
    UpstreamResponse,
    Protocol,
    Policy,
    Relayer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolymarketGatewayError {
    pub kind: PolymarketGatewayErrorKind,
    pub message: String,
}
```

Keep the new exports available from `lib.rs`, but do not remove any existing legacy exports yet.

- [ ] **Step 4: Re-run the gateway-surface tests**

Run:

```bash
cargo test -p venue-polymarket --test gateway_surface -- --test-threads=1
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/venue-polymarket/Cargo.toml \
  crates/venue-polymarket/src/lib.rs \
  crates/venue-polymarket/src/gateway.rs \
  crates/venue-polymarket/src/errors.rs \
  crates/venue-polymarket/src/sdk_backend/mod.rs \
  crates/venue-polymarket/tests/gateway_surface.rs
git commit -m "feat: add experimental polymarket gateway surface"
```

### Task 2: Implement SDK-Backed Authenticated CLOB REST Core

**Files:**
- Create: `crates/venue-polymarket/src/sdk_backend/clob.rs`
- Modify: `crates/venue-polymarket/src/sdk_backend/mod.rs`
- Modify: `crates/venue-polymarket/src/gateway.rs`
- Modify: `crates/venue-polymarket/tests/support/mod.rs`
- Test: `crates/venue-polymarket/tests/sdk_clob_gateway.rs`

- [ ] **Step 1: Write the failing REST-core tests**

```rust
#[tokio::test]
async fn gateway_open_orders_maps_sdk_rows() {
    let gateway = scripted_gateway()
        .with_open_orders_response(vec![scripted_open_order("order-1")]);

    let orders = gateway.open_orders(scripted_order_query()).await.unwrap();

    assert_eq!(orders[0].order_id, "order-1");
}

#[tokio::test]
async fn gateway_heartbeat_maps_success_response() {
    let gateway = scripted_gateway().with_heartbeat_id("hb-1");

    let heartbeat = gateway.post_heartbeat("hb-0").await.unwrap();

    assert_eq!(heartbeat.heartbeat_id, "hb-1");
    assert!(heartbeat.valid);
}

#[tokio::test]
async fn gateway_submit_maps_upstream_rejection_to_upstream_response_error() {
    let gateway = scripted_gateway().with_submit_rejection(401, "{\"error\":\"bad auth\"}");

    let error = gateway.submit_order(sample_signed_order()).await.unwrap_err();

    assert_eq!(error.kind, PolymarketGatewayErrorKind::UpstreamResponse);
}
```

- [ ] **Step 2: Run the REST-core tests to verify they fail**

Run:

```bash
cargo test -p venue-polymarket --test sdk_clob_gateway -- --test-threads=1
```

Expected: FAIL because the SDK-backed CLOB backend does not exist yet.

- [ ] **Step 3: Implement the minimal SDK-backed CLOB adapter**

Create a small internal adapter seam so tests do not depend on live network:

```rust
#[async_trait::async_trait]
pub(crate) trait ClobSdkApi: Send + Sync {
    async fn open_orders(&self, query: &PolymarketOrderQuery)
        -> Result<Vec<OpenOrderSummary>, PolymarketGatewayError>;
    async fn submit(&self, order: &PolymarketSignedOrder)
        -> Result<GatewaySubmitResponse, PolymarketGatewayError>;
    async fn heartbeat(&self, previous: &str)
        -> Result<HeartbeatFetchResult, PolymarketGatewayError>;
}
```

Back the production implementation with `polymarket-client-sdk`, and back tests with scripted doubles in `tests/support/mod.rs`.

- [ ] **Step 4: Re-run the REST-core tests**

Run:

```bash
cargo test -p venue-polymarket --test sdk_clob_gateway -- --test-threads=1
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/venue-polymarket/src/sdk_backend/clob.rs \
  crates/venue-polymarket/src/sdk_backend/mod.rs \
  crates/venue-polymarket/src/gateway.rs \
  crates/venue-polymarket/tests/support/mod.rs \
  crates/venue-polymarket/tests/sdk_clob_gateway.rs
git commit -m "feat: add sdk-backed polymarket clob core"
```

### Task 3: Implement SDK-Backed Websocket Core

**Files:**
- Create: `crates/venue-polymarket/src/sdk_backend/ws.rs`
- Modify: `crates/venue-polymarket/src/sdk_backend/mod.rs`
- Modify: `crates/venue-polymarket/src/gateway.rs`
- Modify: `crates/venue-polymarket/tests/support/mod.rs`
- Test: `crates/venue-polymarket/tests/sdk_ws_gateway.rs`

- [ ] **Step 1: Write the failing websocket-core tests**

```rust
#[tokio::test]
async fn market_stream_projects_repo_owned_market_events() {
    let gateway = scripted_gateway().with_market_messages(vec![scripted_trade_message()]);

    let events = gateway.collect_market_events(vec!["token-1".to_owned()]).await.unwrap();

    assert!(matches!(events[0], venue_polymarket::MarketWsEvent::TradePrice(_)));
}

#[tokio::test]
async fn user_stream_projects_repo_owned_user_events() {
    let gateway = scripted_gateway().with_user_messages(vec![scripted_user_trade_message()]);

    let events = gateway
        .collect_user_events(sample_user_auth(), vec!["condition-1".to_owned()])
        .await
        .unwrap();

    assert!(matches!(events[0], venue_polymarket::UserWsEvent::Trade(_)));
}
```

- [ ] **Step 2: Run the websocket-core tests to verify they fail**

Run:

```bash
cargo test -p venue-polymarket --test sdk_ws_gateway -- --test-threads=1
```

Expected: FAIL because the SDK-backed websocket layer does not exist yet.

- [ ] **Step 3: Implement the minimal SDK-backed websocket adapter**

Use an internal seam like:

```rust
#[async_trait::async_trait]
pub(crate) trait StreamSdkApi: Send + Sync {
    async fn market_events(
        &self,
        token_ids: &[String],
    ) -> Result<Vec<MarketWsEvent>, PolymarketGatewayError>;

    async fn user_events(
        &self,
        auth: &GatewayUserAuth,
        condition_ids: &[String],
    ) -> Result<Vec<UserWsEvent>, PolymarketGatewayError>;
}
```

Keep projection into repo-owned `MarketWsEvent` / `UserWsEvent` types inside `venue-polymarket`.

- [ ] **Step 4: Re-run the websocket-core tests**

Run:

```bash
cargo test -p venue-polymarket --test sdk_ws_gateway -- --test-threads=1
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/venue-polymarket/src/sdk_backend/ws.rs \
  crates/venue-polymarket/src/sdk_backend/mod.rs \
  crates/venue-polymarket/src/gateway.rs \
  crates/venue-polymarket/tests/support/mod.rs \
  crates/venue-polymarket/tests/sdk_ws_gateway.rs
git commit -m "feat: add sdk-backed polymarket websocket core"
```

### Task 4: Implement SDK-Backed Metadata Core And Relayer Facade

**Files:**
- Create: `crates/venue-polymarket/src/sdk_backend/metadata.rs`
- Modify: `crates/venue-polymarket/src/gateway.rs`
- Modify: `crates/venue-polymarket/src/metadata.rs`
- Modify: `crates/venue-polymarket/src/relayer.rs`
- Modify: `crates/venue-polymarket/tests/support/mod.rs`
- Test: `crates/venue-polymarket/tests/sdk_metadata_gateway.rs`

- [ ] **Step 1: Write the failing metadata/policy tests**

```rust
#[tokio::test]
async fn metadata_gateway_skips_malformed_neg_risk_rows_but_keeps_valid_rows() {
    let gateway = scripted_gateway().with_metadata_rows(vec![
        scripted_valid_neg_risk_row("316000"),
        scripted_malformed_neg_risk_row("316248"),
    ]);

    let rows = gateway.refresh_neg_risk_metadata().await.unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].event_id, "316000");
}

#[tokio::test]
async fn metadata_gateway_fails_closed_when_all_rows_are_malformed() {
    let gateway = scripted_gateway().with_metadata_rows(vec![scripted_malformed_neg_risk_row("316248")]);

    let error = gateway.refresh_neg_risk_metadata().await.unwrap_err();

    assert_eq!(error.kind, PolymarketGatewayErrorKind::Policy);
}
```

- [ ] **Step 2: Run the metadata/policy tests to verify they fail**

Run:

```bash
cargo test -p venue-polymarket --test sdk_metadata_gateway -- --test-threads=1
```

Expected: FAIL because the SDK-backed metadata backend does not exist yet.

- [ ] **Step 3: Implement the metadata backend and relayer facade**

Keep the repo-owned policy layer explicit:

```rust
pub(crate) struct MetadataPolicyDecision {
    pub accepted: Vec<NegRiskMarketMetadata>,
    pub skipped: Vec<String>,
}

fn apply_metadata_policy(rows: Vec<SdkMetadataRow>)
    -> Result<MetadataPolicyDecision, PolymarketGatewayError> { /* ... */ }
```

Fit relayer access into the same gateway error surface, but do not change relayer protocol ownership.

- [ ] **Step 4: Re-run the metadata/policy tests**

Run:

```bash
cargo test -p venue-polymarket --test sdk_metadata_gateway -- --test-threads=1
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/venue-polymarket/src/sdk_backend/metadata.rs \
  crates/venue-polymarket/src/gateway.rs \
  crates/venue-polymarket/src/metadata.rs \
  crates/venue-polymarket/src/relayer.rs \
  crates/venue-polymarket/tests/support/mod.rs \
  crates/venue-polymarket/tests/sdk_metadata_gateway.rs
git commit -m "feat: add sdk-backed polymarket metadata core"
```

### Task 5: Back The Legacy Shell With The New Core

**Files:**
- Modify: `crates/venue-polymarket/src/rest.rs`
- Modify: `crates/venue-polymarket/src/ws_client.rs`
- Modify: `crates/venue-polymarket/src/negrisk_live.rs`
- Modify: `crates/venue-polymarket/tests/heartbeat.rs`
- Modify: `crates/venue-polymarket/tests/negrisk_live_provider.rs`
- Modify: `crates/venue-polymarket/tests/status_and_retry.rs`
- Modify: `crates/venue-polymarket/tests/ws_client.rs`
- Modify: `crates/venue-polymarket/tests/ws_feeds.rs`

- [ ] **Step 1: Write failing legacy-shell tests that assert delegation boundaries**

```rust
#[tokio::test]
async fn legacy_rest_shell_can_be_backed_by_gateway_without_owning_auth_math() {
    let (client, recorder) = legacy_client_backed_by_scripted_gateway();

    let _ = client.fetch_open_orders(&sample_auth()).await.unwrap();

    assert_eq!(recorder.gateway_open_orders_calls(), 1);
    assert_eq!(recorder.legacy_header_sign_calls(), 0);
}
```

- [ ] **Step 2: Run the legacy-shell tests to verify they fail**

Run:

```bash
cargo test -p venue-polymarket --test status_and_retry --test heartbeat --test negrisk_live_provider --test ws_client --test ws_feeds -- --test-threads=1
```

Expected: FAIL because the legacy shell still owns the primary transport path.

- [ ] **Step 3: Make the legacy shell delegate to the new core**

Keep the old surface callable, but route primary behavior through the new core:

```rust
pub struct PolymarketRestClient {
    gateway: Arc<PolymarketGateway>,
}

impl PolymarketRestClient {
    pub async fn fetch_open_orders(&self, auth: &L2AuthHeaders<'_>)
        -> Result<Vec<OpenOrderSummary>, RestError> {
        let query = legacy_auth_to_query(auth)?;
        self.gateway.open_orders(query).await.map_err(RestError::from_gateway)
    }
}
```

Do not remove any legacy exports in this task.

- [ ] **Step 4: Run the Phase A verification suite**

Run:

```bash
cargo fmt --all
cargo test -p venue-polymarket --test gateway_surface --test sdk_clob_gateway --test sdk_ws_gateway --test sdk_metadata_gateway -- --test-threads=1
cargo test -p venue-polymarket --test heartbeat --test negrisk_live_provider --test status_and_retry --test ws_client --test ws_feeds -- --test-threads=1
cargo clippy -p venue-polymarket --tests -- -D warnings
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/venue-polymarket/src/rest.rs \
  crates/venue-polymarket/src/ws_client.rs \
  crates/venue-polymarket/src/negrisk_live.rs \
  crates/venue-polymarket/tests/heartbeat.rs \
  crates/venue-polymarket/tests/negrisk_live_provider.rs \
  crates/venue-polymarket/tests/status_and_retry.rs \
  crates/venue-polymarket/tests/ws_client.rs \
  crates/venue-polymarket/tests/ws_feeds.rs
git commit -m "refactor: back legacy polymarket shell with sdk core"
```
