# Phase 3d Daemon Feed Wiring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Upgrade `app-live` from the current `bootstrap + resume` runner into a long-running single-process daemon with layered supervisors, persistent ingestion loops, fail-closed degradation, and production feed wiring while keeping `operator-supplied live targets` as the only live decision input.

**Architecture:** Build directly on the merged `Phase 3c` live-submit closure rather than inventing another runtime. `Phase 3d` keeps the unified authority chain `ExternalFactEvent -> journal append -> StateApplier -> PublishedSnapshot -> ActivationPolicy -> Risk -> Planner -> Attempt -> Submit/Reconcile`, adds real venue source adapters for websocket and heartbeat inputs, persists a startup-scoped operator-target revision anchor, and splits `app-live` into top-level, ingest, state, and decision supervisor responsibilities without introducing a second execution-mode authority.

**Tech Stack:** Rust workspace crates (`app-live`, `domain`, `state`, `persistence`, `venue-polymarket`, `observability`), SQL migrations, Cargo tests, Tokio async runtime, reqwest, websocket client transport, existing Phase 3b/3c live-submit and replay contracts.

---

## Scope Check

This plan covers one coherent sub-project:

1. durable startup-scoped operator-target revision anchoring
2. production feed wiring for market websocket, user websocket, heartbeat, relayer poll, and metadata refresh
3. layered supervisor daemonization in `app-live`
4. fail-closed posture and per-scope restriction handling that still flows through `ActivationPolicy`

This plan deliberately does **not** cover:

1. market-discovered `neg-risk` pricing or autonomous live-target generation
2. hot-reloadable operator target management or dashboard tooling
3. multi-process decomposition
4. remote signer/HSM integrations beyond the provider abstraction already landed in `Phase 3c`

Keep the phase narrow:

- continue to use explicit operator-supplied live targets
- keep operator-target loading startup-scoped and restart-scoped only
- keep `app-live` single-process
- prefer fail-closed posture changes over automatic recovery that keeps expanding live risk

## File Map

### Existing files to modify

- `crates/app-live/Cargo.toml`
  - add any async/runtime dependencies needed by the daemon task groups only if they are not already present
- `crates/app-live/src/config.rs`
  - replace the env-only live-target map with a startup-scoped revisioned operator-target surface and parse the minimal Polymarket daemon source config needed by the feed task groups
- `crates/app-live/src/lib.rs`
  - export the new daemon runner, posture types, queue types, and operator-target revision helpers
- `crates/app-live/src/main.rs`
  - switch live mode from one-shot bootstrap behavior to the layered daemon entrypoint
- `crates/app-live/src/input_tasks.rs`
  - evolve the current single backlog queue into repo-owned ingress item helpers used by long-lived task groups
- `crates/app-live/src/dispatch.rs`
  - keep the existing dirty-set coalescing logic but adapt it to a `DecisionSupervisor`-style continuous scheduler
- `crates/app-live/src/runtime.rs`
  - load and persist the new operator-target revision anchor, expose state-apply helpers to the daemon loops, and keep restart ordering truth-first
- `crates/app-live/src/supervisor.rs`
  - stop being a bootstrap/resume-only shell and become the top-level lifecycle owner for layered supervisors
- `crates/app-live/src/instrumentation.rs`
  - record daemon posture, backlog, and task-group fail-closed signals without inventing non-repo-owned semantics
- `crates/app-live/tests/config.rs`
  - verify revisioned operator-target parsing
- `crates/app-live/tests/main_entrypoint.rs`
  - verify live-mode daemon startup behavior and fail-fast validation around missing startup anchors or source config
- `crates/app-live/tests/fault_injection.rs`
  - verify daemon restart sequencing and fail-closed behavior around partial progress and missing anchors
- `crates/app-live/tests/unified_supervisor.rs`
  - keep dispatch coalescing and supervisor summaries green while layering in continuous scheduling
- `crates/app-live/tests/runtime_observability.rs`
  - keep runtime observability truthful after adding daemon posture and queue backlog recording
- `crates/app-live/tests/supervisor_observability.rs`
  - keep supervisor-level summaries and spans truthful after top-level lifecycle becomes long-running
- `crates/app-live/tests/negrisk_live_submit_resume.rs`
  - preserve `Phase 3c` resume-first guarantees while the daemon loops are introduced
- `crates/domain/src/facts.rs`
  - add repo-owned daemon attention payloads for websocket gap, heartbeat attention, relayer ambiguity, and metadata staleness follow-up work
- `crates/domain/src/lib.rs`
  - export any new daemon fact payload types used by `state` or `app-live`
- `crates/domain/tests/runtime_backbone.rs`
  - lock the daemon fact contracts before `state` or `app-live` starts depending on them
- `crates/state/src/facts.rs`
  - translate daemon attention facts into deterministic `PendingReconcileAnchor`s or no-op authoritative facts as appropriate
- `crates/state/src/apply.rs`
  - keep `StateApplier` as the only mutation path while mapping daemon attention facts into reconcile-required outcomes
- `crates/state/src/store.rs`
  - expose the minimum authoritative queries needed to derive per-scope restriction inputs from pending reconcile truth
- `crates/state/tests/event_apply_contract.rs`
  - verify daemon attention facts become reconcile-required outcomes through `StateApplier`
- `crates/state/tests/bootstrap_reconcile.rs`
  - verify restored daemon follow-up work keeps the runtime in reconcile-first posture
- `crates/persistence/src/models.rs`
  - extend `RuntimeProgressRow` with a durable operator-target revision anchor
- `crates/persistence/src/repos.rs`
  - persist and load the operator-target revision alongside existing runtime progress anchors
- `crates/persistence/src/lib.rs`
  - re-export any new runtime-progress helpers
- `crates/persistence/tests/runtime_backbone.rs`
  - verify runtime progress persists and restores the operator-target revision fail-closed
- `crates/persistence/tests/migrations.rs`
  - verify the schema upgrade adds the revision anchor without weakening existing live closure guarantees
- `crates/venue-polymarket/Cargo.toml`
  - add websocket transport dependencies needed for real market/user stream adapters
- `crates/venue-polymarket/src/lib.rs`
  - export the new websocket source adapter and heartbeat fetch helper
- `crates/venue-polymarket/src/rest.rs`
  - add the documented heartbeat request boundary and any shared HTTP helpers needed by the daemon task groups
- `crates/venue-polymarket/src/heartbeat.rs`
  - keep the heartbeat monitor focused on truth/freshness while adding the minimal fetch-result shape needed by `app-live`
- `crates/venue-polymarket/tests/heartbeat.rs`
  - verify fetched heartbeat results map into existing monitor semantics
- `crates/venue-polymarket/tests/ws_feeds.rs`
  - keep parser and liveness logic green once the source adapter starts using it
- `crates/observability/src/metrics.rs`
  - add queue backlog, daemon posture, and follow-up-work gauges/counters only if they do not already exist
- `crates/observability/src/conventions.rs`
  - add span names / fields for daemon supervisors only if the current surface is missing them
- `crates/observability/tests/metrics.rs`
  - lock any new daemon metric keys
- `crates/observability/tests/conventions.rs`
  - lock any new daemon span/field conventions
- `README.md`
  - update repository status so `Phase 3d` is clearly distinguished from `Phase 3c`

### New files to create

- `crates/app-live/src/posture.rs`
  - define top-level supervisor posture and per-scope restriction-input types without creating a second execution authority
- `crates/app-live/src/queues.rs`
  - define `IngressQueue`, `SnapshotDispatchQueue`, and `FollowUpQueue`
- `crates/app-live/src/task_groups.rs`
  - hold `MarketDataTaskGroup`, `UserStateTaskGroup`, `HeartbeatTaskGroup`, `RelayerTaskGroup`, `MetadataTaskGroup`, and scheduler-facing helpers
- `crates/app-live/src/daemon.rs`
  - host the long-running daemon runner and deterministic test hooks for bounded loop execution
- `crates/app-live/tests/daemon_lifecycle.rs`
  - focused tests for startup ordering, shutdown ordering, and no-duplicate-live-submit restart behavior
- `crates/app-live/tests/ingest_task_groups.rs`
  - focused tests for task-group fact emission, backlog, and fail-closed triggers
- `crates/venue-polymarket/src/ws_client.rs`
  - websocket source adapter for market/user channels that uses the existing parser and liveness monitors
- `crates/venue-polymarket/tests/ws_client.rs`
  - focused websocket adapter tests using scripted messages rather than a real upstream dependency
- `migrations/0010_phase3d_daemon_feed_wiring.sql`
  - schema for the operator-target revision anchor on runtime progress

## Implementation Notes

- Preserve the unified authority chain. No websocket loop, heartbeat poller, relayer poller, or recovery path may mutate authoritative state directly.
- Keep restriction ownership clear:
  - top-level `AppSupervisor` owns only global posture
  - per-scope restriction inputs are derived from durable truth and feed `ActivationPolicy`
  - `ActivationPolicy` remains the only execution-mode authority
- `Phase 3d` operator targets are startup-scoped and restart-scoped only. Do **not** add in-process hot reload.
- Favor repo-owned facts over ad hoc hints whenever a condition must survive replay, restart, or postmortem analysis.
- Keep intermediate commits buildable:
  - land the operator-target revision anchor before the daemon entrypoint needs it
  - land venue websocket/heartbeat adapters before `app-live` task groups try to consume them
  - land daemon attention facts before the task groups emit them
- Drive every behavior from tests first (`@superpowers:test-driven-development`).

### Task 1: Persist A Startup-Scoped Operator-Target Revision Anchor

**Files:**
- Create: `migrations/0010_phase3d_daemon_feed_wiring.sql`
- Modify: `crates/persistence/src/models.rs`
- Modify: `crates/persistence/src/repos.rs`
- Modify: `crates/persistence/src/lib.rs`
- Modify: `crates/app-live/src/config.rs`
- Modify: `crates/app-live/src/runtime.rs`
- Modify: `crates/app-live/src/main.rs`
- Test: `crates/persistence/tests/runtime_backbone.rs`
- Test: `crates/persistence/tests/migrations.rs`
- Test: `crates/app-live/tests/config.rs`
- Test: `crates/app-live/tests/main_entrypoint.rs`

- [ ] **Step 1: Write the failing operator-target revision tests**

```rust
#[test]
fn live_target_config_reports_stable_revision_for_startup_set() {
    let config = load_neg_risk_live_target_set(Some(sample_targets_json())).unwrap();

    assert_eq!(config.revision, "sha256:6e0d8c...");
    assert_eq!(config.targets["family-a"].members.len(), 2);
}

#[tokio::test]
async fn runtime_progress_round_trips_operator_target_revision() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    RuntimeProgressRepo
        .record_progress(&db.pool, 41, 7, Some("snapshot-7"), Some("targets-rev-3"))
        .await
        .unwrap();

    let row = RuntimeProgressRepo.current(&db.pool).await.unwrap().unwrap();
    assert_eq!(row.operator_target_revision.as_deref(), Some("targets-rev-3"));
}
```

- [ ] **Step 2: Run the focused tests and verify they fail**

Run:

```bash
cargo test -p app-live --test config --test main_entrypoint
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test runtime_backbone --test migrations
```

Expected:

- `app-live` fails because live targets still deserialize as an env-only map with no revision surface
- `persistence` fails because `runtime_apply_progress` has no operator-target revision anchor yet

- [ ] **Step 3: Implement the minimal revisioned startup surface**

Keep this task narrow:

```rust
pub struct NegRiskLiveTargetSet {
    pub revision: String,
    pub targets: BTreeMap<String, NegRiskFamilyLiveTarget>,
}

pub struct RuntimeProgressRow {
    pub last_journal_seq: i64,
    pub last_state_version: i64,
    pub last_snapshot_id: Option<String>,
    pub operator_target_revision: Option<String>,
}
```

And update runtime progress persistence with a single new nullable column:

```sql
ALTER TABLE runtime_apply_progress
ADD COLUMN operator_target_revision TEXT;
```

Rules for this task:

- compute the revision deterministically from the loaded startup target set
- persist that revision with runtime progress
- require the revision anchor during live daemon startup and resume whenever operator-supplied live targets are present
- do **not** add hot reload, file watching, or a second config table

- [ ] **Step 4: Re-run the focused tests**

Run:

```bash
cargo test -p app-live --test config --test main_entrypoint
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test runtime_backbone --test migrations
```

Expected: PASS. The daemon still does not exist yet; only the revisioned startup anchor should land here.

- [ ] **Step 5: Commit**

```bash
git add migrations/0010_phase3d_daemon_feed_wiring.sql crates/persistence/src/models.rs crates/persistence/src/repos.rs crates/persistence/src/lib.rs crates/persistence/tests/runtime_backbone.rs crates/persistence/tests/migrations.rs crates/app-live/src/config.rs crates/app-live/src/runtime.rs crates/app-live/src/main.rs crates/app-live/tests/config.rs crates/app-live/tests/main_entrypoint.rs
git commit -m "feat: persist phase3d operator target revisions"
```

### Task 2: Add Real `Polymarket` Websocket And Heartbeat Source Adapters

**Files:**
- Modify: `crates/venue-polymarket/Cargo.toml`
- Create: `crates/venue-polymarket/src/ws_client.rs`
- Modify: `crates/venue-polymarket/src/rest.rs`
- Modify: `crates/venue-polymarket/src/heartbeat.rs`
- Modify: `crates/venue-polymarket/src/lib.rs`
- Modify: `crates/venue-polymarket/src/ws_market.rs`
- Modify: `crates/venue-polymarket/src/ws_user.rs`
- Modify: `crates/app-live/src/config.rs`
- Test: `crates/venue-polymarket/tests/ws_client.rs`
- Modify: `crates/venue-polymarket/tests/ws_feeds.rs`
- Modify: `crates/venue-polymarket/tests/heartbeat.rs`
- Modify: `crates/venue-polymarket/tests/support/mod.rs`
- Modify: `crates/app-live/tests/config.rs`

- [ ] **Step 1: Write the failing websocket and heartbeat source tests**

```rust
#[tokio::test]
async fn market_ws_client_yields_parsed_market_events_from_scripted_messages() {
    let mut source = ScriptedWsSource::market(vec![
        r#"{"event":"book","asset_id":"token-1","best_bid":"0.40","best_ask":"0.41"}"#,
        r#"{"event":"PONG"}"#,
    ]);

    let events = collect_market_events(&mut source).await;
    assert!(matches!(events[0], MarketWsEvent::Book(_)));
    assert_eq!(events[1], MarketWsEvent::Pong);
}

#[tokio::test]
async fn heartbeat_fetch_maps_success_payload_into_monitor_input() {
    let server = MockServer::spawn("200 OK", r#"{"success":true,"heartbeat_id":"hb-42"}"#);
    let client = sample_client(server.base_url());

    let heartbeat = client.fetch_order_heartbeat().await.unwrap();
    assert_eq!(heartbeat.heartbeat_id, "hb-42");
    assert!(heartbeat.valid);
}

#[test]
fn daemon_source_config_parses_hosts_and_cadence_from_json() {
    let config = load_polymarket_source_config(Some(sample_source_config_json())).unwrap();

    assert_eq!(config.market_ws_url.as_str(), "wss://ws-subscriptions.polymarket.com/market");
    assert_eq!(config.user_ws_url.as_str(), "wss://ws-subscriptions.polymarket.com/user");
    assert_eq!(config.heartbeat_interval_seconds, 15);
}
```

- [ ] **Step 2: Run the focused source-adapter tests and verify they fail**

Run:

```bash
cargo test -p venue-polymarket --test ws_client --test ws_feeds --test heartbeat
cargo test -p app-live --test config
```

Expected:

- websocket tests fail because there is no source adapter yet
- heartbeat tests fail because there is no real fetch boundary returning a repo-owned heartbeat result
- `app-live` config tests fail because daemon source configuration does not exist yet

- [ ] **Step 3: Implement the minimal source adapters**

Add only the adapter surface Phase 3d needs:

```rust
pub struct PolymarketWsClient { /* connection config + transport */ }

impl PolymarketWsClient {
    pub async fn next_market_event(&mut self) -> Result<MarketWsEvent, WsClientError> { /* ... */ }
    pub async fn next_user_event(&mut self) -> Result<UserWsEvent, WsClientError> { /* ... */ }
}

pub struct HeartbeatFetchResult {
    pub heartbeat_id: String,
    pub valid: bool,
}

pub struct PolymarketSourceConfig {
    pub clob_host: Url,
    pub data_api_host: Url,
    pub relayer_host: Url,
    pub market_ws_url: Url,
    pub user_ws_url: Url,
    pub heartbeat_interval_seconds: u64,
    pub relayer_poll_interval_seconds: u64,
    pub metadata_refresh_interval_seconds: u64,
}
```

Rules for this task:

- reuse the existing liveness monitors and extend parser compatibility only as needed to accept documented `Polymarket` websocket payload shapes
- keep tests transport-scripted; do not depend on the real upstream service
- do not let the websocket client emit `ExternalFactEvent`s yet; that belongs in `app-live`
- keep source configuration startup-scoped; do not add file watching or dynamic reload
- keep `app-live` behavior limited to source-config support; websocket handshake payloads, heartbeat request bodies, and parser compatibility live inside `venue-polymarket`

- [ ] **Step 4: Re-run the focused source-adapter tests**

Run:

```bash
cargo test -p venue-polymarket --test ws_client --test ws_feeds --test heartbeat
cargo test -p app-live --test config
```

Expected: PASS. `app-live` should still be unchanged.

- [ ] **Step 5: Commit**

```bash
git add crates/venue-polymarket/Cargo.toml crates/venue-polymarket/src/ws_client.rs crates/venue-polymarket/src/rest.rs crates/venue-polymarket/src/heartbeat.rs crates/venue-polymarket/src/lib.rs crates/venue-polymarket/tests/ws_client.rs crates/venue-polymarket/tests/ws_feeds.rs crates/venue-polymarket/tests/heartbeat.rs crates/app-live/src/config.rs crates/app-live/tests/config.rs
git commit -m "feat: add phase3d websocket and heartbeat sources"
```

### Task 3: Add Repo-Owned Daemon Attention Facts And Apply Contracts

**Files:**
- Modify: `crates/domain/src/facts.rs`
- Modify: `crates/domain/src/lib.rs`
- Test: `crates/domain/tests/runtime_backbone.rs`
- Modify: `crates/state/src/facts.rs`
- Modify: `crates/state/src/apply.rs`
- Modify: `crates/state/src/store.rs`
- Test: `crates/state/tests/event_apply_contract.rs`
- Test: `crates/state/tests/bootstrap_reconcile.rs`

- [ ] **Step 1: Write the failing daemon-attention contract tests**

```rust
#[test]
fn external_fact_event_can_carry_runtime_attention_payload() {
    let fact = ExternalFactEvent::runtime_attention_observed(
        "heartbeat",
        "session-live",
        "hb-gap-1",
        "family-a",
        "missed_heartbeat",
        "heartbeat freshness exceeded threshold",
        Utc::now(),
    );

    assert_eq!(fact.source_kind, "runtime_attention");
    assert_eq!(fact.payload.kind(), "runtime_attention_observed");
}

#[test]
fn heartbeat_attention_becomes_reconcile_required_via_state_applier() {
    let mut store = StateStore::new();
    let result = StateApplier::new(&mut store)
        .apply(51, ExternalFactEvent::runtime_attention_observed(
            "heartbeat",
            "session-live",
            "hb-gap-1",
            "family-a",
            "missed_heartbeat",
            "heartbeat freshness exceeded threshold",
            Utc::now(),
        ))
        .unwrap();

    assert!(matches!(result, ApplyResult::ReconcileRequired { .. }));
    assert_eq!(store.pending_reconcile_count(), 1);
}
```

- [ ] **Step 2: Run the focused daemon-attention tests and verify they fail**

Run:

```bash
cargo test -p domain --test runtime_backbone
cargo test -p state --test event_apply_contract --test bootstrap_reconcile
```

Expected:

- `domain` fails because daemon attention payloads do not exist yet
- `state` fails because `StateApplier` does not know how to convert daemon attention facts into reconcile-required outcomes

- [ ] **Step 3: Implement the minimal daemon-attention contract**

Keep the new fact shape narrow:

```rust
pub struct RuntimeAttentionObservedPayload {
    pub source: String,
    pub scope_id: String,
    pub attention_kind: String,
    pub reason: String,
}
```

And map it in `state` only as far as Phase 3d needs:

- websocket gap, heartbeat attention, and relayer ambiguity become deterministic `PendingReconcileAnchor`s
- metadata staleness becomes a dirty runtime fact that blocks new live expansion without inventing a new execution authority
- generic successful market/user events remain generic facts unless a later task needs more structure

- [ ] **Step 4: Re-run the focused daemon-attention tests**

Run:

```bash
cargo test -p domain --test runtime_backbone
cargo test -p state --test event_apply_contract --test bootstrap_reconcile
```

Expected: PASS. No daemon loops yet; only the replay-safe fact/apply surface lands here.

- [ ] **Step 5: Commit**

```bash
git add crates/domain/src/facts.rs crates/domain/src/lib.rs crates/domain/tests/runtime_backbone.rs crates/state/src/facts.rs crates/state/src/apply.rs crates/state/src/store.rs crates/state/tests/event_apply_contract.rs crates/state/tests/bootstrap_reconcile.rs
git commit -m "feat: add phase3d daemon attention facts"
```

### Task 4: Introduce App-Live Posture And Queue Primitives

**Files:**
- Create: `crates/app-live/src/posture.rs`
- Create: `crates/app-live/src/queues.rs`
- Modify: `crates/app-live/src/input_tasks.rs`
- Modify: `crates/app-live/src/dispatch.rs`
- Modify: `crates/app-live/src/lib.rs`
- Test: `crates/app-live/tests/unified_supervisor.rs`
- Create: `crates/app-live/tests/ingest_task_groups.rs`

- [ ] **Step 1: Write the failing posture and queue tests**

```rust
#[test]
fn global_posture_and_scope_restrictions_are_not_the_same_authority() {
    let posture = SupervisorPosture::DegradedIngress;
    let restriction = ScopeRestriction::reconciling_only("family-a");

    assert!(posture.is_global());
    assert_eq!(restriction.scope_id(), "family-a");
}

#[test]
fn snapshot_dispatch_queue_keeps_latest_stable_snapshot_for_dirty_domain() {
    let mut queue = SnapshotDispatchQueue::default();
    queue.push(sample_snapshot_notice(7, ["runtime"]));
    queue.push(sample_snapshot_notice(8, ["runtime", "neg-risk"]));

    let drained = queue.coalesced();
    assert_eq!(drained.last().unwrap().state_version, 8);
}
```

- [ ] **Step 2: Run the focused posture/queue tests and verify they fail**

Run:

```bash
cargo test -p app-live --test unified_supervisor --test ingest_task_groups
```

Expected:

- new test file fails because posture and queue primitives do not exist yet
- existing unified supervisor tests fail once they start depending on the new queue boundary behavior

- [ ] **Step 3: Implement the minimal posture and queue layer**

Add focused files instead of overloading `supervisor.rs`:

```rust
pub enum SupervisorPosture {
    Healthy,
    DegradedIngress,
    DegradedDispatch,
    GlobalHalt,
}

pub enum ScopeRestrictionKind {
    ReconcilingOnly,
    RecoveryOnly,
}
```

And queue structs:

```rust
pub struct IngressQueue { /* ExternalFactEvent work */ }
pub struct SnapshotDispatchQueue { /* published snapshot notices */ }
pub struct FollowUpQueue { /* pending reconcile + recovery work */ }
```

Rules for this task:

- do not add execution-mode logic here
- do not let queues own business rules
- keep `DispatchLoop` as the coalescing primitive rather than rewriting it

- [ ] **Step 4: Re-run the focused posture/queue tests**

Run:

```bash
cargo test -p app-live --test unified_supervisor --test ingest_task_groups
```

Expected: PASS. `app-live` still does not run continuously, but the primitives are in place.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/posture.rs crates/app-live/src/queues.rs crates/app-live/src/input_tasks.rs crates/app-live/src/dispatch.rs crates/app-live/src/lib.rs crates/app-live/tests/unified_supervisor.rs crates/app-live/tests/ingest_task_groups.rs
git commit -m "feat: add phase3d daemon posture and queues"
```

### Task 5: Implement Continuous Ingest And Decision Task Groups

**Files:**
- Create: `crates/app-live/src/task_groups.rs`
- Modify: `crates/app-live/src/runtime.rs`
- Modify: `crates/app-live/src/supervisor.rs`
- Modify: `crates/app-live/src/instrumentation.rs`
- Test: `crates/app-live/tests/ingest_task_groups.rs`
- Test: `crates/app-live/tests/negrisk_live_submit_resume.rs`
- Test: `crates/app-live/tests/fault_injection.rs`

- [ ] **Step 1: Write the failing task-group tests**

```rust
#[tokio::test]
async fn heartbeat_task_group_emits_runtime_attention_fact_when_freshness_expires() {
    let mut group = HeartbeatTaskGroup::for_tests(sample_heartbeat_source_timeout());

    let emitted = group.tick().await.unwrap().unwrap();
    assert_eq!(emitted.event.source_kind, "runtime_attention");
    assert_eq!(emitted.event.payload.kind(), "runtime_attention_observed");
}

#[tokio::test]
async fn decision_task_group_suppresses_live_expansion_while_follow_up_backlog_exists() {
    let mut group = DecisionTaskGroup::for_tests();
    group.seed_pending_reconcile("family-a");

    let result = group.tick(sample_live_snapshot_notice("family-a")).await;
    assert!(result.suppressed);
}
```

- [ ] **Step 2: Run the focused task-group tests and verify they fail**

Run:

```bash
cargo test -p app-live --test ingest_task_groups --test negrisk_live_submit_resume --test fault_injection
```

Expected:

- task-group tests fail because there are no continuous task groups yet
- resume/fault-injection tests fail once they start asserting truth-first recovery before loop resumption

- [ ] **Step 3: Implement the minimal task groups**

Create task groups that only do one job each:

```rust
pub struct MarketDataTaskGroup { /* ws source + liveness monitor */ }
pub struct UserStateTaskGroup { /* ws source + liveness monitor */ }
pub struct HeartbeatTaskGroup { /* heartbeat fetch + monitor */ }
pub struct RelayerTaskGroup { /* recent transaction poll cadence */ }
pub struct MetadataTaskGroup { /* refresh cadence */ }
pub struct DecisionTaskGroup { /* dispatch/reconcile/recovery scheduler */ }
pub struct RecoveryTaskGroup { /* follow-up priority + lock lifecycle */ }
```

Task rules:

- ingestion groups emit `InputTaskEvent`s or follow-up work only
- `DecisionTaskGroup` consumes published snapshots and follow-up queues only
- recovery stays a child of decision scheduling, not a parallel authority path
- fail-closed conditions produce repo-owned facts or queue items rather than ad hoc booleans

- [ ] **Step 4: Re-run the focused task-group tests**

Run:

```bash
cargo test -p app-live --test ingest_task_groups --test negrisk_live_submit_resume --test fault_injection
```

Expected: PASS. `supervisor.rs` should now be able to schedule continuous work deterministically in tests.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/task_groups.rs crates/app-live/src/runtime.rs crates/app-live/src/supervisor.rs crates/app-live/src/instrumentation.rs crates/app-live/tests/ingest_task_groups.rs crates/app-live/tests/negrisk_live_submit_resume.rs crates/app-live/tests/fault_injection.rs
git commit -m "feat: add phase3d ingest and decision task groups"
```

### Task 6: Wire The Long-Running Daemon Entry Point, Observability, And Status Docs

**Files:**
- Create: `crates/app-live/src/daemon.rs`
- Modify: `crates/app-live/src/lib.rs`
- Modify: `crates/app-live/src/main.rs`
- Modify: `crates/app-live/src/supervisor.rs`
- Modify: `crates/app-live/src/instrumentation.rs`
- Modify: `crates/observability/src/metrics.rs`
- Modify: `crates/observability/src/conventions.rs`
- Modify: `crates/observability/tests/metrics.rs`
- Modify: `crates/observability/tests/conventions.rs`
- Create: `crates/app-live/tests/daemon_lifecycle.rs`
- Modify: `crates/app-live/tests/runtime_observability.rs`
- Modify: `crates/app-live/tests/supervisor_observability.rs`
- Modify: `crates/app-live/tests/main_entrypoint.rs`
- Modify: `README.md`

- [ ] **Step 1: Write the failing daemon lifecycle and observability tests**

```rust
#[tokio::test]
async fn daemon_startup_restores_truth_before_resuming_ingest_loops() {
    let mut daemon = test_daemon_with_restored_progress();

    let report = daemon.run_until_idle_for_tests(3).await.unwrap();
    assert_eq!(report.startup_order, vec!["restore", "state", "decision", "ingest"]);
}

#[test]
fn observability_exposes_daemon_posture_and_backlog_metrics() {
    let metrics = Observability::new("app-live").metrics();

    assert_eq!(metrics.ingress_backlog.key(), MetricKey::new("axiom_ingress_backlog"));
    assert_eq!(metrics.follow_up_backlog.key(), MetricKey::new("axiom_follow_up_backlog"));
}
```

- [ ] **Step 2: Run the focused daemon lifecycle and observability tests and verify they fail**

Run:

```bash
cargo test -p app-live --test daemon_lifecycle --test main_entrypoint --test runtime_observability --test supervisor_observability
cargo test -p observability --test metrics --test conventions
```

Expected:

- `app-live` fails because the binary still runs a one-shot bootstrap path
- `observability` fails because daemon posture/backlog metrics and span names are not defined yet

- [ ] **Step 3: Implement the minimal daemon runner and status surface**

Add a bounded runner for tests and an open-ended runner for the binary:

```rust
pub struct AppDaemon { /* top-level lifecycle + child supervisors */ }

impl AppDaemon {
    pub async fn run_until_shutdown(self) -> Result<(), SupervisorError> { /* ... */ }
    pub async fn run_until_idle_for_tests(&mut self, max_ticks: usize) -> Result<DaemonReport, SupervisorError> { /* ... */ }
}
```

Rules for this task:

- preserve `Phase 3c` truth-first startup ordering
- do not add a second `main` entrypoint; route live mode through the daemon runner
- expose runtime summary fields for operator-target revision, global posture, and queue backlog
- update `README.md` only after the tests prove the daemon path exists

- [ ] **Step 4: Re-run the focused daemon lifecycle and observability tests**

Run:

```bash
cargo test -p app-live --test daemon_lifecycle --test main_entrypoint --test runtime_observability --test supervisor_observability
cargo test -p observability --test metrics --test conventions
```

Expected: PASS. The repository status can now claim `Phase 3d` daemonization rather than only `Phase 3c` closure.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/daemon.rs crates/app-live/src/lib.rs crates/app-live/src/main.rs crates/app-live/src/supervisor.rs crates/app-live/src/instrumentation.rs crates/observability/src/metrics.rs crates/observability/src/conventions.rs crates/observability/tests/metrics.rs crates/observability/tests/conventions.rs crates/app-live/tests/daemon_lifecycle.rs crates/app-live/tests/runtime_observability.rs crates/app-live/tests/supervisor_observability.rs crates/app-live/tests/main_entrypoint.rs README.md
git commit -m "feat: wire phase3d daemon feed runtime"
```

## Final Verification

- [ ] Run formatting:

```bash
cargo fmt --all --check
```

Expected: PASS

- [ ] Run linting:

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS

- [ ] Run focused Phase 3d suites:

```bash
cargo test -p venue-polymarket --test ws_client --test ws_feeds --test heartbeat
cargo test -p domain --test runtime_backbone
cargo test -p state --test event_apply_contract --test bootstrap_reconcile
cargo test -p app-live --test config --test ingest_task_groups --test daemon_lifecycle --test fault_injection --test negrisk_live_submit_resume --test runtime_observability --test supervisor_observability --test main_entrypoint --test unified_supervisor
cargo test -p observability --test metrics --test conventions
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test runtime_backbone --test migrations
```

Expected: PASS

- [ ] Run broad regression checks that protect existing live closure behavior:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test negrisk_live
cargo test -p venue-polymarket --test negrisk_live_provider --test status_and_retry
cargo test -p app-live --test negrisk_live_rollout
```

Expected: PASS

- [ ] Commit any final formatting or doc-only follow-ups:

```bash
git add README.md crates/app-live crates/domain crates/state crates/persistence crates/venue-polymarket crates/observability migrations
git commit -m "chore: finalize phase3d verification follow-ups"
```

Only create this commit if the verification step changed tracked files.
