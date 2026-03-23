# AxiomArb V1a Live Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `v1a` live-capable Polymarket engine for non-sports full-set arbitrage with correct bootstrapping, reconciliation, signed-order idempotency, automatic `split / merge / redeem`, and replayable persistence.

**Architecture:** Implement a single Rust workspace with one live application and one replay application. Keep venue integration, state/reconciliation, pricing/risk, execution, and persistence in separate crates so `v1a` ships without `v1b` complexity while still preserving the identifier and settlement model needed for the later neg-risk plan.

**Tech Stack:** Rust, Tokio, Serde, Reqwest, WebSocket client library, SQLx, PostgreSQL, `rust_decimal`, Tracing, Prometheus, Docker Compose

---

## Scope Boundary

This plan intentionally covers `v1a` only.

- In scope: `non-sports`, binary YES/NO markets, single account, single venue, full-set strategy, live execution, automatic `split / merge / redeem`, journal, replay, reconciliation, risk gates.
- Out of scope: `v1b` standard neg-risk live rollout, augmented neg-risk, maker quoting, sports, multi-account support.
- Follow-on requirement: create a separate `v1b` plan after `v1a` has landed and the identifier, persistence, and replay layers are working in production-like tests.

## File Structure Map

### Root

- Create: `Cargo.toml`
  Responsibility: workspace members, shared dependencies, lint/test aliases.
- Create: `rust-toolchain.toml`
  Responsibility: pin Rust toolchain and components.
- Create: `.env.example`
  Responsibility: runtime env contract for live, paper, DB, and Polymarket credentials.
- Modify: `/Users/viv/projs/axiom-arb/README.md`
  Responsibility: repo bootstrap, local run commands, plan entrypoints.
- Create: `Makefile`
  Responsibility: repeatable `fmt`, `clippy`, `test`, `db-up`, `db-down`, `live-paper`.
- Create: `docker-compose.yml`
  Responsibility: local Postgres, optional Prometheus/Grafana.

### Database

- Create: `migrations/0001_workspace_core.sql`
  Responsibility: core metadata, identifiers, runtime-safe enums.
- Create: `migrations/0002_orders_journal.sql`
  Responsibility: orders, fills, event journal, retry linkage.
- Create: `migrations/0003_inventory_resolution.sql`
  Responsibility: approvals, inventory buckets, resolution state, relayer transactions, ctf operations.

### Applications

- Create: `crates/app-live/src/main.rs`
  Responsibility: load config, bootstrap runtime, wire tasks, choose paper/live mode.
- Create: `crates/app-live/src/bootstrap.rs`
  Responsibility: startup sequence, bootstrap `CancelOnly`, first reconcile gate.
- Create: `crates/app-live/src/runtime.rs`
  Responsibility: task orchestration and shutdown policy.
- Create: `crates/app-replay/src/main.rs`
  Responsibility: replay `event_journal` into state and strategy evaluation.

### Shared Runtime Crates

- Create: `crates/config/src/lib.rs`
  Responsibility: `Settings` parsing, environment validation, feature flags.
- Create: `crates/config/src/settings.rs`
  Responsibility: strongly typed config structs.
- Create: `crates/domain/src/lib.rs`
  Responsibility: export core domain modules.
- Create: `crates/domain/src/identifiers.rs`
  Responsibility: `Event`, `EventFamily`, `Condition`, `Token`, `IdentifierMap`, `MarketRoute`.
- Create: `crates/domain/src/runtime_mode.rs`
  Responsibility: runtime modes, overlays, venue/account status mapping.
- Create: `crates/domain/src/order.rs`
  Responsibility: submission state, venue order state, settlement state, signed-order fields.
- Create: `crates/domain/src/inventory.rs`
  Responsibility: inventory buckets, approvals, reservations.
- Create: `crates/domain/src/resolution.rs`
  Responsibility: `ResolutionState`, payout vector, redeem rules.
- Create: `crates/journal/src/lib.rs`
  Responsibility: journal interfaces and replay contract.
- Create: `crates/journal/src/writer.rs`
  Responsibility: append-only event writer with `journal_seq`.
- Create: `crates/journal/src/replay.rs`
  Responsibility: deterministic replay stream ordering.
- Create: `crates/observability/src/lib.rs`
  Responsibility: tracing/metrics bootstrap.
- Create: `crates/observability/src/metrics.rs`
  Responsibility: Prometheus metrics registration.
- Create: `crates/persistence/src/lib.rs`
  Responsibility: DB pool and repository exports.
- Create: `crates/persistence/src/models.rs`
  Responsibility: SQLx row models and conversion helpers.
- Create: `crates/persistence/src/repos.rs`
  Responsibility: repository methods for identifiers, orders, approvals, resolution, journal.

### Venue Integration

- Create: `crates/venue-polymarket/src/lib.rs`
  Responsibility: venue client exports.
- Create: `crates/venue-polymarket/src/auth.rs`
  Responsibility: builder/L2 auth headers and signature-type mapping.
- Create: `crates/venue-polymarket/src/rest.rs`
  Responsibility: metadata, order, balance, fee-rate, approval, status, and relayer REST calls.
- Create: `crates/venue-polymarket/src/ws_market.rs`
  Responsibility: market websocket parsing and `PING/PONG`.
- Create: `crates/venue-polymarket/src/ws_user.rs`
  Responsibility: user websocket parsing and settlement event decoding.
- Create: `crates/venue-polymarket/src/heartbeat.rs`
  Responsibility: REST order heartbeat lifecycle.
- Create: `crates/venue-polymarket/src/retry.rs`
  Responsibility: HTTP/business error matrix and bounded retry policy.
- Create: `crates/venue-polymarket/src/relayer.rs`
  Responsibility: relayer transaction lookup, nonce tracking, safe/proxy state.

### State, Strategy, Risk, Execution

- Create: `crates/state/src/lib.rs`
  Responsibility: in-memory state store exports.
- Create: `crates/state/src/store.rs`
  Responsibility: orderbooks, balances, approvals, runtime mode, resolution state.
- Create: `crates/state/src/bootstrap.rs`
  Responsibility: first-load behavior and bootstrap restrictions.
- Create: `crates/state/src/reconcile.rs`
  Responsibility: WS/REST/relayer reconciliation.
- Create: `crates/strategy-fullset/src/lib.rs`
  Responsibility: full-set strategy entrypoint.
- Create: `crates/strategy-fullset/src/pricing.rs`
  Responsibility: normalized edge computation and quantization-aware pricing.
- Create: `crates/risk/src/lib.rs`
  Responsibility: risk engine interfaces and `RiskDecision`.
- Create: `crates/risk/src/fullset.rs`
  Responsibility: v1a-specific checks, approval gating, redeem gating.
- Create: `crates/execution/src/lib.rs`
  Responsibility: execution engine exports.
- Create: `crates/execution/src/plans.rs`
  Responsibility: `ExecutionPlan` definitions including `RedeemResolved`.
- Create: `crates/execution/src/orders.rs`
  Responsibility: signed-order submission, transport retry, business retry.
- Create: `crates/execution/src/ctf.rs`
  Responsibility: `split / merge / redeem` and relayer-aware tracking.

### Tests And Docs

- Create: `crates/config/tests/load_settings.rs`
- Create: `crates/domain/tests/identifier_and_mode.rs`
- Create: `crates/persistence/tests/migrations.rs`
- Create: `crates/venue-polymarket/tests/status_and_retry.rs`
- Create: `crates/state/tests/bootstrap_reconcile.rs`
- Create: `crates/strategy-fullset/tests/net_edge.rs`
- Create: `crates/risk/tests/fullset_guards.rs`
- Create: `crates/execution/tests/retry_and_redeem.rs`
- Create: `crates/journal/tests/replay.rs`
- Create: `crates/app-live/tests/bootstrap_modes.rs`
- Create: `docs/runbooks/live-break-glass.md`
- Create: `docs/runbooks/bootstrap-and-ramp.md`

## Task 1: Bootstrap The Workspace And Runtime Contract

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `.env.example`
- Create: `Makefile`
- Create: `docker-compose.yml`
- Modify: `/Users/viv/projs/axiom-arb/README.md`
- Create: `crates/config/src/lib.rs`
- Create: `crates/config/src/settings.rs`
- Test: `crates/config/tests/load_settings.rs`

- [ ] **Step 1: Create the Rust workspace skeleton**

```toml
[workspace]
members = [
  "crates/app-live",
  "crates/app-replay",
  "crates/config",
  "crates/domain",
  "crates/journal",
  "crates/observability",
  "crates/persistence",
  "crates/venue-polymarket",
  "crates/state",
  "crates/strategy-fullset",
  "crates/risk",
  "crates/execution",
]
resolver = "2"
```

- [ ] **Step 2: Add the runtime env contract and local Postgres**

```env
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb
AXIOM_MODE=paper
POLY_CLOB_HOST=https://clob.polymarket.com
POLY_DATA_API_HOST=https://data-api.polymarket.com
POLY_RELAYER_HOST=https://relayer-v2.polymarket.com
POLY_SIGNATURE_TYPE=EOA
```

- [ ] **Step 3: Write the failing config test**

```rust
#[test]
fn load_settings_requires_database_url_and_mode() {
    let err = config::Settings::from_env_iter([
        ("DATABASE_URL", "postgres://axiom:axiom@localhost:5432/axiom_arb"),
        ("AXIOM_MODE", "paper"),
    ])
    .expect("settings should parse");

    assert_eq!(err.runtime.mode.as_str(), "paper");
}
```

- [ ] **Step 4: Run the test to verify the crate is not implemented yet**

Run: `cargo test -p config load_settings_requires_database_url_and_mode -- --exact`

Expected: FAIL because the `config` crate or `Settings::from_env_iter` does not exist yet.

- [ ] **Step 5: Implement `Settings` parsing with strict validation**

```rust
pub struct Settings {
    pub runtime: RuntimeSettings,
    pub db: DatabaseSettings,
    pub polymarket: PolymarketSettings,
}
```

- [ ] **Step 6: Run the config test and workspace smoke check**

Run: `cargo test -p config`

Expected: PASS for config tests.

Run: `cargo fmt --all && cargo check --workspace`

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml rust-toolchain.toml .env.example Makefile docker-compose.yml README.md crates/config
git commit -m "chore: bootstrap workspace and config contract"
```

## Task 2: Define The Domain Model And Runtime Rules

**Files:**
- Create: `crates/domain/src/lib.rs`
- Create: `crates/domain/src/identifiers.rs`
- Create: `crates/domain/src/runtime_mode.rs`
- Create: `crates/domain/src/order.rs`
- Create: `crates/domain/src/inventory.rs`
- Create: `crates/domain/src/resolution.rs`
- Test: `crates/domain/tests/identifier_and_mode.rs`

- [ ] **Step 1: Write the failing identifier and mode tests**

```rust
#[test]
fn identifier_map_resolves_token_condition_and_route() {
    let map = IdentifierMap::new(/* sample ids */);
    assert_eq!(map.condition_for_token("token_yes").unwrap(), "condition_a");
    assert_eq!(map.route_for_condition("condition_a"), MarketRoute::Standard);
}

#[test]
fn bootstrapping_defaults_to_cancel_only_until_first_reconcile() {
    let mode = RuntimeMode::Bootstrapping.default_overlay();
    assert_eq!(mode, Some(RuntimeOverlay::CancelOnly));
}
```

- [ ] **Step 2: Run the domain tests to verify failure**

Run: `cargo test -p domain identifier_map_resolves_token_condition_and_route -- --exact`

Expected: FAIL because `IdentifierMap` and runtime types do not exist yet.

- [ ] **Step 3: Implement identifiers, routes, order state, approval state, and resolution state**

```rust
pub struct ResolutionState {
    pub condition_id: ConditionId,
    pub resolution_status: ResolutionStatus,
    pub payout_vector: Vec<Decimal>,
    pub dispute_state: DisputeState,
    pub redeemable_at: Option<DateTime<Utc>>,
}
```

- [ ] **Step 4: Encode the mode/action rules in the domain layer**

```rust
impl RuntimeMode {
    pub fn default_overlay(&self) -> Option<RuntimeOverlay> {
        match self {
            RuntimeMode::Bootstrapping => Some(RuntimeOverlay::CancelOnly),
            _ => None,
        }
    }
}
```

- [ ] **Step 5: Run the domain test suite**

Run: `cargo test -p domain`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/domain
git commit -m "feat: define v1a domain model and runtime rules"
```

## Task 3: Create Migrations And Persistence Foundations

**Files:**
- Create: `migrations/0001_workspace_core.sql`
- Create: `migrations/0002_orders_journal.sql`
- Create: `migrations/0003_inventory_resolution.sql`
- Create: `crates/persistence/src/lib.rs`
- Create: `crates/persistence/src/models.rs`
- Create: `crates/persistence/src/repos.rs`
- Test: `crates/persistence/tests/migrations.rs`

- [ ] **Step 1: Write the failing migration test**

```rust
#[tokio::test]
async fn migrations_create_signed_order_and_resolution_tables() {
    let pool = test_db_pool().await;
    run_migrations(&pool).await.unwrap();

    assert!(table_exists(&pool, "orders").await);
    assert!(column_exists(&pool, "orders", "signed_order_hash").await);
    assert!(table_exists(&pool, "resolution_states").await);
}
```

- [ ] **Step 2: Start Postgres and run the failing test**

Run: `docker compose up -d postgres`

Expected: Postgres container is healthy.

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence migrations_create_signed_order_and_resolution_tables -- --exact`

Expected: FAIL because migrations and repositories do not exist yet.

- [ ] **Step 3: Add the three migrations**

```sql
CREATE TABLE orders (
  internal_order_id UUID PRIMARY KEY,
  signed_order_hash TEXT,
  salt TEXT,
  nonce TEXT,
  signature TEXT,
  retry_of_order_id UUID,
  ...
);
```

- [ ] **Step 4: Implement repositories for identifiers, approvals, resolution state, journal, and orders**

```rust
pub struct OrderRepo;

impl OrderRepo {
    pub async fn insert_signed_order(&self, pool: &PgPool, row: NewOrderRow) -> Result<()> {
        // sqlx insert with retry_of_order_id support
    }
}
```

- [ ] **Step 5: Run migration and repository tests**

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add migrations crates/persistence docker-compose.yml
git commit -m "feat: add persistence schema for v1a runtime"
```

## Task 4: Implement Polymarket REST, Auth, And Retry Semantics

**Files:**
- Create: `crates/venue-polymarket/src/lib.rs`
- Create: `crates/venue-polymarket/src/auth.rs`
- Create: `crates/venue-polymarket/src/rest.rs`
- Create: `crates/venue-polymarket/src/retry.rs`
- Create: `crates/venue-polymarket/src/relayer.rs`
- Test: `crates/venue-polymarket/tests/status_and_retry.rs`

- [ ] **Step 1: Write the failing retry/status tests**

```rust
#[test]
fn http_503_cancel_only_maps_to_no_new_risk_cancel_only() {
    let mapped = map_venue_status(503, Some("cancel-only"));
    assert_eq!(mapped.mode, RuntimeMode::NoNewRisk);
    assert_eq!(mapped.overlay, Some(RuntimeOverlay::CancelOnly));
}

#[test]
fn transport_retry_preserves_signed_order_identity() {
    let retry = RetryDecision::for_transport_timeout(&sample_signed_order());
    assert!(retry.reuse_payload);
}
```

- [ ] **Step 2: Run the failing venue test**

Run: `cargo test -p venue-polymarket http_503_cancel_only_maps_to_no_new_risk_cancel_only -- --exact`

Expected: FAIL because mapping and retry code do not exist yet.

- [ ] **Step 3: Implement auth and approval-aware REST clients**

```rust
pub struct PolymarketRestClient {
    pub clob_host: Url,
    pub data_api_host: Url,
    pub relayer_host: Url,
}
```

- [ ] **Step 4: Implement the retry matrix as code, not comments**

```rust
pub enum RetryClass {
    Transport,
    Business,
    None,
}
```

- [ ] **Step 5: Add relayer transaction and nonce fetch support**

```rust
pub async fn fetch_recent_transactions(&self, owner: &str) -> Result<Vec<RelayerTx>> { ... }
pub async fn fetch_current_nonce(&self, owner: &str) -> Result<String> { ... }
```

- [ ] **Step 6: Run the venue test suite**

Run: `cargo test -p venue-polymarket`

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/venue-polymarket
git commit -m "feat: add polymarket rest client and retry rules"
```

## Task 5: Add WebSocket Feeds, Order Heartbeat, And Journal Ingestion

**Files:**
- Create: `crates/venue-polymarket/src/ws_market.rs`
- Create: `crates/venue-polymarket/src/ws_user.rs`
- Create: `crates/venue-polymarket/src/heartbeat.rs`
- Create: `crates/journal/src/lib.rs`
- Create: `crates/journal/src/writer.rs`
- Test: `crates/journal/tests/replay.rs`

- [ ] **Step 1: Write the failing journal ordering test**

```rust
#[tokio::test]
async fn journal_assigns_monotonic_seq_for_mixed_sources() {
    let writer = JournalWriter::in_memory();
    let a = writer.append(sample_market_event()).await.unwrap();
    let b = writer.append(sample_user_event()).await.unwrap();
    assert!(a.journal_seq < b.journal_seq);
}
```

- [ ] **Step 2: Run the failing journal test**

Run: `cargo test -p journal journal_assigns_monotonic_seq_for_mixed_sources -- --exact`

Expected: FAIL because the journal writer does not exist yet.

- [ ] **Step 3: Implement websocket event parsing and heartbeat handling**

```rust
pub struct OrderHeartbeatState {
    pub heartbeat_id: Option<String>,
    pub last_success_at: DateTime<Utc>,
}
```

- [ ] **Step 4: Implement append-only journal entries with deterministic replay fields**

```rust
pub struct JournalEntry {
    pub journal_seq: i64,
    pub source_kind: SourceKind,
    pub source_session_id: String,
    pub dedupe_key: String,
    pub causal_parent_id: Option<String>,
}
```

- [ ] **Step 5: Add tests for missing heartbeat causing reconcile trigger**

Run: `cargo test -p venue-polymarket heartbeat`

Expected: PASS for heartbeat freshness and failure handling tests.

- [ ] **Step 6: Run journal and venue websocket tests**

Run: `cargo test -p journal && cargo test -p venue-polymarket ws_`

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/journal crates/venue-polymarket
git commit -m "feat: add ws feeds heartbeat and journal ingestion"
```

## Task 6: Build State Store, Bootstrap, And Reconciliation

**Files:**
- Create: `crates/state/src/lib.rs`
- Create: `crates/state/src/store.rs`
- Create: `crates/state/src/bootstrap.rs`
- Create: `crates/state/src/reconcile.rs`
- Test: `crates/state/tests/bootstrap_reconcile.rs`
- Test: `crates/app-live/tests/bootstrap_modes.rs`

- [ ] **Step 1: Write the failing bootstrap test**

```rust
#[tokio::test]
async fn startup_remains_cancel_only_until_first_reconcile_succeeds() {
    let mut store = StateStore::new();
    store.mark_bootstrapping();
    assert_eq!(store.mode_overlay(), Some(RuntimeOverlay::CancelOnly));
}
```

- [ ] **Step 2: Run the failing state test**

Run: `cargo test -p state startup_remains_cancel_only_until_first_reconcile_succeeds -- --exact`

Expected: FAIL because `StateStore` and bootstrap policy are missing.

- [ ] **Step 3: Implement in-memory state with approvals, inventory buckets, and resolution state**

```rust
pub struct StateStore {
    pub runtime_mode: RuntimeMode,
    pub overlay: Option<RuntimeOverlay>,
    pub approvals: HashMap<ApprovalKey, ApprovalState>,
    pub resolution: HashMap<ConditionId, ResolutionState>,
}
```

- [ ] **Step 4: Implement reconcile from REST + relayer + local journal**

```rust
pub async fn reconcile(&mut self, snapshot: RemoteSnapshot) -> Result<ReconcileReport> {
    // align open orders, balances, approvals, relayer txs, and resolution state
}
```

- [ ] **Step 5: Add tests for duplicate signed order detection forcing `Reconciling`**

Run: `cargo test -p state`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/state crates/app-live/tests
git commit -m "feat: add bootstrap and reconciliation state machine"
```

## Task 7: Implement Full-Set Pricing And Risk Gates

**Files:**
- Create: `crates/strategy-fullset/src/lib.rs`
- Create: `crates/strategy-fullset/src/pricing.rs`
- Create: `crates/risk/src/lib.rs`
- Create: `crates/risk/src/fullset.rs`
- Test: `crates/strategy-fullset/tests/net_edge.rs`
- Test: `crates/risk/tests/fullset_guards.rs`

- [ ] **Step 1: Write the failing pricing and risk tests**

```rust
#[test]
fn net_edge_uses_usdc_normalization_and_fee_rounding() {
    let edge = pricing::fullset_buy_merge(sample_inputs());
    assert_eq!(edge.net_edge_usdc.round_dp(4).to_string(), "0.0123");
}

#[test]
fn redeem_is_rejected_while_condition_is_disputed() {
    let decision = risk.evaluate(sample_redeem_opp_in_dispute());
    assert!(matches!(decision, RiskDecision::Reject { .. }));
}
```

- [ ] **Step 2: Run the failing pricing test**

Run: `cargo test -p strategy-fullset net_edge_uses_usdc_normalization_and_fee_rounding -- --exact`

Expected: FAIL because pricing code is missing.

- [ ] **Step 3: Implement normalized edge calculation with quantization-aware inputs**

```rust
pub struct EdgeBreakdown {
    pub gross_in_usdc: Decimal,
    pub gross_out_usdc: Decimal,
    pub fee_usdc_equiv: Decimal,
    pub rounding_loss: Decimal,
}
```

- [ ] **Step 4: Implement risk checks for approvals, resolution state, heartbeat freshness, and mode restrictions**

```rust
if state.runtime_mode != RuntimeMode::Healthy {
    return RiskDecision::Reject { reason: RejectReason::ModeNotHealthy };
}
```

- [ ] **Step 5: Run strategy and risk tests**

Run: `cargo test -p strategy-fullset && cargo test -p risk`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/strategy-fullset crates/risk
git commit -m "feat: add full-set pricing and v1a risk checks"
```

## Task 8: Implement Execution Plans, Signed-Order Semantics, And CTF Operations

**Files:**
- Create: `crates/execution/src/lib.rs`
- Create: `crates/execution/src/plans.rs`
- Create: `crates/execution/src/orders.rs`
- Create: `crates/execution/src/ctf.rs`
- Test: `crates/execution/tests/retry_and_redeem.rs`

- [ ] **Step 1: Write the failing execution tests**

```rust
#[test]
fn transport_retry_reuses_the_same_signed_order_identity() {
    let result = execution::retry_transport(sample_signed_order());
    assert_eq!(result.signed_order_hash, sample_signed_order().signed_order_hash);
}

#[test]
fn redeem_plan_is_condition_scoped_and_amountless() {
    let plan = ExecutionPlan::RedeemResolved { condition_id: sample_condition() };
    assert_eq!(plan.condition_id(), sample_condition());
}
```

- [ ] **Step 2: Run the failing execution test**

Run: `cargo test -p execution transport_retry_reuses_the_same_signed_order_identity -- --exact`

Expected: FAIL because execution code is missing.

- [ ] **Step 3: Implement execution plan types and signed-order storage**

```rust
pub enum ExecutionPlan {
    FullSetBuyThenMerge { ... },
    FullSetSplitThenSell { ... },
    CancelStale { ... },
    RedeemResolved { condition_id: ConditionId },
}
```

- [ ] **Step 4: Implement transport retry vs business retry rules**

```rust
pub enum SubmissionAttempt {
    TransportRetry { original_order_id: Uuid },
    BusinessRetry { retry_of_order_id: Uuid, new_nonce: String },
}
```

- [ ] **Step 5: Implement relayer-aware `split / merge / redeem` tracking**

```rust
pub struct CtfOperationTracker {
    pub relayer_transaction_id: Option<String>,
    pub nonce: Option<String>,
}
```

- [ ] **Step 6: Run the execution test suite**

Run: `cargo test -p execution`

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/execution
git commit -m "feat: add execution engine and ctf operation tracking"
```

## Task 9: Wire The Live App, Paper Mode, And Replay App

**Files:**
- Create: `crates/app-live/src/main.rs`
- Create: `crates/app-live/src/bootstrap.rs`
- Create: `crates/app-live/src/runtime.rs`
- Create: `crates/app-replay/src/main.rs`
- Create: `crates/observability/src/lib.rs`
- Create: `crates/observability/src/metrics.rs`
- Test: `crates/app-live/tests/bootstrap_modes.rs`

- [ ] **Step 1: Write the failing app bootstrap test**

```rust
#[tokio::test]
async fn app_stays_in_bootstrap_cancel_only_without_successful_reconcile() {
    let runtime = build_test_runtime().await;
    assert_eq!(runtime.mode(), RuntimeMode::Bootstrapping);
    assert_eq!(runtime.overlay(), Some(RuntimeOverlay::CancelOnly));
}
```

- [ ] **Step 2: Run the failing live app test**

Run: `cargo test -p app-live app_stays_in_bootstrap_cancel_only_without_successful_reconcile -- --exact`

Expected: FAIL because the app runtime does not exist yet.

- [ ] **Step 3: Implement app wiring for live and paper modes**

```rust
match settings.runtime.mode.as_str() {
    "paper" => run_paper(runtime).await,
    "live" => run_live(runtime).await,
    _ => bail!("unsupported mode"),
}
```

- [ ] **Step 4: Implement replay app over `event_journal`**

```rust
pub async fn replay_from_journal(pool: &PgPool, range: ReplayRange) -> Result<()> {
    // load journal_seq order and rebuild state
}
```

- [ ] **Step 5: Expose metrics for heartbeat freshness, runtime mode, relayer pending age, and divergence count**

Run: `cargo test -p app-live && cargo test -p journal && cargo check --workspace`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/app-live crates/app-replay crates/observability
git commit -m "feat: wire live runtime and replay app"
```

## Task 10: Finalize Runbooks, Ramp Policy, And Verification Gates

**Files:**
- Create: `docs/runbooks/live-break-glass.md`
- Create: `docs/runbooks/bootstrap-and-ramp.md`
- Modify: `/Users/viv/projs/axiom-arb/README.md`

- [ ] **Step 1: Write the runbook drafts**

```md
# Live Break-Glass

1. Enter `GlobalHalt`
2. Run cancel-all
3. Verify no open orders remain
4. Inspect relayer pending transactions
5. Move quarantined inventory into manual review
```

- [ ] **Step 2: Document capital ramp and launch gates**

```md
# Bootstrap And Ramp

- Start with paper mode
- Run one-market minimal notional live session
- Require multiple clean sessions with no unreconciled state before increasing size
```

- [ ] **Step 3: Run the full verification sequence**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings`

Expected: PASS.

Run: `cargo test --workspace`

Expected: PASS.

Run: `docker compose up -d postgres && DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence`

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add README.md docs/runbooks
git commit -m "docs: add live runbooks and launch policy"
```

## Task 11: Stabilize The First Live Candidate

**Files:**
- Modify: `crates/app-live/src/bootstrap.rs`
- Modify: `crates/state/src/reconcile.rs`
- Modify: `crates/execution/src/orders.rs`
- Modify: `crates/journal/src/replay.rs`
- Test: `crates/app-live/tests/bootstrap_modes.rs`
- Test: `crates/execution/tests/retry_and_redeem.rs`

- [ ] **Step 1: Run the end-to-end paper bootstrap sequence**

Run: `cargo run -p app-live -- --mode paper`

Expected: service starts, remains `CancelOnly` during bootstrap, enters `Healthy` only after reconcile succeeds.

- [ ] **Step 2: Run the replay binary against journaled paper data**

Run: `cargo run -p app-replay -- --from-seq 1`

Expected: deterministic replay finishes without divergence.

- [ ] **Step 3: Fix any failing bootstrap, retry, or replay invariants before live**

```rust
assert_eq!(replayed_state.order_count(), live_state.order_count());
```

- [ ] **Step 4: Re-run the focused suites**

Run: `cargo test -p state && cargo test -p execution && cargo test -p app-live`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live crates/state crates/execution crates/journal
git commit -m "fix: stabilize v1a live candidate invariants"
```

## Deferred Follow-On

Do not start `v1b` in this plan.

Create a separate plan after Task 11 lands and the following are true:

- identifier persistence is stable in real journal/replay runs
- relayer and redeem flows have at least one clean resolved-condition validation
- bootstrap and reconcile transitions are stable under repeated restarts
- live runbooks have been exercised in paper or minimal-notional sessions
