# AxiomArb Unified Runtime Phase 1/2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the unified runtime backbone from the spec so `full-set` can run on the production-shaped live path while `neg-risk` enters the same path in `Shadow`, without enabling selective `neg-risk Live` yet.

**Architecture:** Extend the current workspace instead of creating a second runtime. The implementation is split into contracts, durable apply/publish anchors, route-local snapshot projections, activation/risk, shared execution orchestration, a first-class recovery coordinator, and a real `app-live` supervisor. `Phase 3` selective `neg-risk Live` enablement remains a follow-on plan once parity, recovery, replay, and fault-injection gates are proven.

**Tech Stack:** Rust, Tokio, Serde, SQLx, PostgreSQL, `rust_decimal`, `chrono`, `tracing`, local Postgres via Docker Compose

---

## Scope Boundary

This plan intentionally covers `Phase 1` and `Phase 2` from [`2026-03-24-axiomarb-unified-runtime-v1a-v1b-design.md`](/Users/viv/projs/axiom-arb/docs/superpowers/specs/2026-03-24-axiomarb-unified-runtime-v1a-v1b-design.md).

- In scope: unified domain contracts, durable journal/apply/publish anchors, projection readiness flags, `full-set` live on the new backbone, `neg-risk` shadow on the same backbone, unified activation/risk/planning/execution, first-class recovery, `app-live` supervisor wiring, parity/recovery/replay/fault-injection tests.
- Out of scope: selective `Phase 3` `neg-risk Live` rollout, path-aware `neg-risk` pricing, real `neg-risk` live venue submission, fake fill simulation, collector-backed observability backend work.
- Rollout rule: this plan must leave `neg-risk` non-live. The capability matrix may only end in `Disabled`, `Shadow`, `ReduceOnly`, or `RecoveryOnly` for `neg-risk`.

## Execution Notes

- Execute this plan from a dedicated worktree.
- Use `@test-driven-development` on every task.
- If any red/green step behaves unexpectedly, stop and use `@systematic-debugging` before changing the plan.
- Before claiming the branch is complete, run `@verification-before-completion`.
- Keep the new backbone DRY and YAGNI: introduce only the contracts and wiring required by `Phase 1` live `full-set` and `Phase 2` shadow `neg-risk`.

## File Structure Map

### Root

- Modify: `/Users/viv/projs/axiom-arb/Cargo.toml`
  Responsibility: add the `recovery` workspace member.
- Modify: `/Users/viv/projs/axiom-arb/README.md`
  Responsibility: document the new unified runtime phases, execution modes, and the fact that `neg-risk` remains shadow-only after this plan.

### Domain Contracts

- Create: `crates/domain/src/facts.rs`
  Responsibility: `ExternalFactEvent`, source/dedupe metadata, normalizer anchors, and raw payload fingerprint contract.
- Create: `crates/domain/src/decision.rs`
  Responsibility: `DecisionInput`, `IntentCandidate`, `RecoveryIntent`, `DecisionVerdict`, `StateConfidence`, `ExecutionMode`, `ActivationDecision`.
- Create: `crates/domain/src/execution.rs`
  Responsibility: `PublishedSnapshotRef`, `ExecutionRequest`, `ExecutionPlanRef`, `ExecutionAttempt`, `ExecutionAttemptOutcome`, `ExecutionReceipt`, attempt context types.
- Modify: `crates/domain/src/lib.rs`
  Responsibility: export the new unified-runtime types.
- Test: `crates/domain/tests/runtime_backbone.rs`

### Persistence

- Create: `migrations/0005_unified_runtime_backbone.sql`
  Responsibility: persist runtime apply progress, snapshot publications, execution attempts, pending reconcile items, and physically isolated shadow artifacts.
- Modify: `crates/persistence/src/models.rs`
  Responsibility: row models for runtime progress, snapshot publication state, execution attempts, pending reconcile items, and shadow artifacts.
- Modify: `crates/persistence/src/repos.rs`
  Responsibility: repos to read/write durable `journal_seq -> state_version -> snapshot_id` anchors, attempt rows, pending reconcile items, and shadow-only artifact writes.
- Modify: `crates/persistence/src/lib.rs`
  Responsibility: export the new runtime repos.
- Test: `crates/persistence/tests/runtime_backbone.rs`

### State Apply And Snapshot Publication

- Create: `crates/state/src/facts.rs`
  Responsibility: dirty-domain sets, projection readiness metadata, pending references, and route-local projection failure reasons.
- Create: `crates/state/src/apply.rs`
  Responsibility: `StateApplier`, authoritative local ordering, idempotent duplicate handling, deferred/reconcile-required results, and restart-resume anchors.
- Create: `crates/state/src/snapshot.rs`
  Responsibility: `PublishedSnapshot`, `FullSetView`, `NegRiskView`, projection readiness flags, and publish-time route-local degradation semantics.
- Modify: `crates/state/src/store.rs`
  Responsibility: store `state_version`, confidence markers, snapshot derivation inputs, and expose state needed by the new applier/publisher.
- Modify: `crates/state/src/lib.rs`
  Responsibility: export apply and snapshot types.
- Test: `crates/state/tests/event_apply_contract.rs`
- Test: `crates/state/tests/snapshot_publish.rs`
- Modify: `crates/state/tests/bootstrap_reconcile.rs`
  Responsibility: keep bootstrap expectations aligned with the new state anchors.

### Strategy And Risk

- Modify: `crates/risk/Cargo.toml`
  Responsibility: add any `state` and `recovery` crate dependencies needed by the route-neutral activation/risk entrypoint.
- Modify: `crates/strategy-fullset/Cargo.toml`
  Responsibility: add `domain` and `state` dependencies required by the full-set intent builder.
- Create: `crates/strategy-fullset/src/intent.rs`
  Responsibility: build `IntentCandidate` values from `FullSetView`.
- Modify: `crates/strategy-fullset/src/lib.rs`
  Responsibility: export the new full-set intent builder.
- Create: `crates/strategy-fullset/tests/intent.rs`
- Modify: `crates/strategy-negrisk/Cargo.toml`
  Responsibility: add the `state` dependency required by the neg-risk intent builder.
- Create: `crates/strategy-negrisk/src/intent.rs`
  Responsibility: build `IntentCandidate` values from `NegRiskView` without live-order semantics.
- Modify: `crates/strategy-negrisk/src/lib.rs`
  Responsibility: export the new neg-risk intent builder.
- Create: `crates/strategy-negrisk/tests/intent.rs`
- Create: `crates/risk/src/activation.rs`
  Responsibility: `ActivationPolicy`, capability matrix lookup, mode precedence, and replayable policy anchors.
- Create: `crates/risk/src/engine.rs`
  Responsibility: route-neutral risk entrypoint that consumes `DecisionInput` + `ActivationDecision`.
- Create: `crates/risk/src/negrisk.rs`
  Responsibility: `neg-risk` shadow/risk admissibility checks that do not place live orders.
- Modify: `crates/risk/src/fullset.rs`
  Responsibility: adapt current full-set checks to the new route-neutral engine.
- Modify: `crates/risk/src/lib.rs`
  Responsibility: export activation and unified risk engine modules.
- Test: `crates/risk/tests/activation_policy.rs`
- Create: `crates/risk/tests/decision_engine.rs`
- Modify: `crates/risk/tests/fullset_guards.rs`

### Execution And Recovery

- Modify: `crates/execution/Cargo.toml`
  Responsibility: add local crate dependencies needed by the attempt-aware orchestrator and sink boundary.
- Create: `crates/execution/src/attempt.rs`
  Responsibility: attempt factories, numbering, and idempotency helpers that construct the domain-owned `ExecutionAttempt` and attempt context.
- Create: `crates/execution/src/orchestrator.rs`
  Responsibility: shared `ActivationDecision -> RiskVerdict -> ExecutionRequest -> ExecutionPlan -> ExecutionAttempt -> ExecutionReceipt` orchestration.
- Create: `crates/execution/src/sink.rs`
  Responsibility: `VenueSink`, `LiveVenueSink`, and `ShadowVenueSink`.
- Modify: `crates/execution/src/plans.rs`
  Responsibility: move current plan enum toward the unified business-level plan surface.
- Modify: `crates/execution/src/orders.rs`
  Responsibility: adapt order submission helpers to consume attempt context rather than raw plan-only input.
- Modify: `crates/execution/src/ctf.rs`
  Responsibility: adapt CTF helpers to the new plan/attempt flow.
- Modify: `crates/execution/src/lib.rs`
  Responsibility: export orchestrator, attempts, and sink modules.
- Create: `crates/execution/tests/orchestrator.rs`
- Modify: `crates/execution/tests/retry_and_redeem.rs`
  Responsibility: keep existing full-set semantics working through the new attempt layer.
- Create: `crates/recovery/Cargo.toml`
- Create: `crates/recovery/src/lib.rs`
- Create: `crates/recovery/src/locks.rs`
  Responsibility: `RecoveryScopeLock`, scope inheritance, and route-local expansion blocking.
- Create: `crates/recovery/src/coordinator.rs`
  Responsibility: detect unresolved attempts, pending reconcile items, and convergence invariants; emit `RecoveryIntent`.
- Create: `crates/recovery/tests/recovery_contract.rs`

### App Runtime And Observability

- Modify: `crates/app-live/Cargo.toml`
  Responsibility: add local crate dependencies for execution, persistence, recovery, risk, strategies, and venue wiring.
- Create: `crates/app-live/src/supervisor.rs`
  Responsibility: runtime composition root, task lifecycle, and bootstrap/restart supervision.
- Create: `crates/app-live/src/dispatch.rs`
  Responsibility: dirty-set coalescing, route-local dispatch, backlog handling, and projection readiness-aware scheduling.
- Create: `crates/app-live/src/input_tasks.rs`
  Responsibility: input-task adapters that normalize venue observations into `ExternalFactEvent`.
- Modify: `crates/app-live/src/runtime.rs`
  Responsibility: host the new supervisor-oriented runtime API instead of the static bootstrap-only wrapper.
- Modify: `crates/app-live/src/bootstrap.rs`
  Responsibility: bridge old bootstrap semantics into the new supervisor boot path.
- Modify: `crates/app-live/src/main.rs`
  Responsibility: load settings, build the supervisor, and print/record runtime summary without bypassing the structured runtime path.
- Modify: `crates/app-live/src/lib.rs`
  Responsibility: export supervisor/runtime entrypoints.
- Create: `crates/app-live/tests/unified_supervisor.rs`
- Create: `crates/app-live/tests/fault_injection.rs`
- Modify: `crates/app-live/tests/bootstrap_modes.rs`
- Modify: `crates/app-live/tests/main_entrypoint.rs`
- Modify: `crates/observability/src/metrics.rs`
  Responsibility: add dispatcher backlog, projection lag, recovery backlog, and shadow-attempt metrics while keeping typed handles repo-owned.
- Modify: `crates/observability/src/lib.rs`
  Responsibility: export the new typed metrics.
- Modify: `crates/observability/tests/metrics.rs`

## Task 1: Add Unified Runtime Domain Contracts

**Files:**
- Create: `crates/domain/src/facts.rs`
- Create: `crates/domain/src/decision.rs`
- Create: `crates/domain/src/execution.rs`
- Modify: `crates/domain/src/lib.rs`
- Test: `crates/domain/tests/runtime_backbone.rs`

- [ ] **Step 1: Write the failing domain contract test**

```rust
use chrono::Utc;
use domain::{
    ActivationDecision, DecisionInput, ExecutionAttempt, ExecutionMode, ExternalFactEvent,
    IntentCandidate, RecoveryIntent,
};

#[test]
fn decision_contracts_stay_input_neutral_and_attempt_scoped() {
    let strategy = DecisionInput::Strategy(IntentCandidate::new(
        "intent-1",
        "snapshot-7",
        "full-set",
    ));
    let recovery = DecisionInput::Recovery(RecoveryIntent::new(
        "recovery-1",
        "snapshot-7",
        "family-lock",
    ));

    assert_eq!(strategy.decision_input_id(), "intent-1");
    assert_eq!(recovery.decision_input_id(), "recovery-1");
    assert!(matches!(ExecutionMode::Shadow, ExecutionMode::Shadow));

    let attempt = ExecutionAttempt::new("attempt-1", "plan-1", "snapshot-7", 1);
    assert_eq!(attempt.attempt_id.as_str(), "attempt-1");
    assert_eq!(attempt.plan_id.as_str(), "plan-1");
}

#[test]
fn activation_decision_keeps_policy_anchors_for_replay() {
    let decision = ActivationDecision::shadow("family-a", "policy-v1", Some("rule-3"));
    assert_eq!(decision.policy_version, "policy-v1");
    assert_eq!(decision.matched_rule_id.as_deref(), Some("rule-3"));
}

#[test]
fn external_fact_event_carries_normalizer_anchor() {
    let event = ExternalFactEvent::new(
        "market_ws",
        "session-1",
        "evt-1",
        "v1-market-normalizer",
        Utc::now(),
    );
    assert_eq!(event.normalizer_version, "v1-market-normalizer");
}
```

- [ ] **Step 2: Run the domain test to verify it fails**

Run: `cargo test -p domain --test runtime_backbone`

Expected: FAIL because `facts`, `decision`, and `execution` modules do not exist yet.

- [ ] **Step 3: Implement the external fact and decision contracts**

```rust
pub struct ExternalFactEvent {
    pub source_kind: String,
    pub source_session_id: String,
    pub source_event_id: String,
    pub normalizer_version: String,
    pub observed_at: DateTime<Utc>,
    pub raw_payload_hash: Option<String>,
}

pub enum DecisionInput {
    Strategy(IntentCandidate),
    Recovery(RecoveryIntent),
}

pub enum ExecutionMode {
    Disabled,
    Shadow,
    Live,
    ReduceOnly,
    RecoveryOnly,
}
```

- [ ] **Step 4: Implement the published-snapshot and execution-attempt contracts**

```rust
pub struct PublishedSnapshotRef {
    pub snapshot_id: String,
    pub state_version: u64,
    pub committed_journal_seq: i64,
}

pub struct ExecutionAttempt {
    pub attempt_id: String,
    pub plan_id: String,
    pub snapshot_id: String,
    pub attempt_no: u32,
}

pub struct ActivationDecision {
    pub mode: ExecutionMode,
    pub scope: String,
    pub reason: String,
    pub policy_version: String,
    pub matched_rule_id: Option<String>,
}
```

- [ ] **Step 5: Export the new modules from `crates/domain/src/lib.rs`**

```rust
mod decision;
mod execution;
mod facts;

pub use decision::{ActivationDecision, DecisionInput, DecisionVerdict, ExecutionMode};
pub use execution::{ExecutionAttempt, ExecutionReceipt, ExecutionRequest};
pub use facts::ExternalFactEvent;
```

- [ ] **Step 6: Run the domain contract test to verify it passes**

Run: `cargo test -p domain --test runtime_backbone`

Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/domain
git commit -m "feat: add unified runtime domain contracts"
```

## Task 2: Persist Runtime Anchors, Attempts, And Shadow Isolation

**Files:**
- Create: `migrations/0005_unified_runtime_backbone.sql`
- Modify: `crates/persistence/src/models.rs`
- Modify: `crates/persistence/src/repos.rs`
- Modify: `crates/persistence/src/lib.rs`
- Test: `crates/persistence/tests/runtime_backbone.rs`

- [ ] **Step 1: Write the failing persistence test**

```rust
use persistence::{
    run_migrations, RuntimeProgressRepo, ShadowArtifactRepo, ExecutionAttemptRepo,
};

#[tokio::test]
async fn runtime_progress_persists_journal_state_snapshot_triplet() {
    let pool = test_db_pool().await;
    run_migrations(&pool).await.unwrap();

    RuntimeProgressRepo
        .record_progress(&pool, 41, 7, Some("snapshot-7"))
        .await
        .unwrap();

    let progress = RuntimeProgressRepo.current(&pool).await.unwrap().unwrap();
    assert_eq!(progress.last_journal_seq, 41);
    assert_eq!(progress.last_state_version, 7);
    assert_eq!(progress.last_snapshot_id.as_deref(), Some("snapshot-7"));
}

#[tokio::test]
async fn shadow_artifacts_are_isolated_from_live_attempt_rows() {
    let pool = test_db_pool().await;
    run_migrations(&pool).await.unwrap();

    ShadowArtifactRepo
        .append(&pool, sample_shadow_artifact("attempt-shadow-1"))
        .await
        .unwrap();

    assert!(ExecutionAttemptRepo.list_live_attempts(&pool).await.unwrap().is_empty());
}
```

- [ ] **Step 2: Run the persistence test to verify it fails**

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test runtime_backbone -- --test-threads=1`

Expected: FAIL because the migration and repos do not exist yet.

- [ ] **Step 3: Add the new migration**

```sql
CREATE TABLE runtime_apply_progress (
  progress_key TEXT PRIMARY KEY,
  last_journal_seq BIGINT NOT NULL,
  last_state_version BIGINT NOT NULL,
  last_snapshot_id TEXT,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE snapshot_publications (
  snapshot_id TEXT PRIMARY KEY,
  state_version BIGINT NOT NULL,
  committed_journal_seq BIGINT NOT NULL,
  fullset_ready BOOLEAN NOT NULL,
  negrisk_ready BOOLEAN NOT NULL,
  metadata JSONB NOT NULL DEFAULT '{}'::JSONB,
  published_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE execution_attempts (
  attempt_id TEXT PRIMARY KEY,
  plan_id TEXT NOT NULL,
  snapshot_id TEXT NOT NULL,
  execution_mode TEXT NOT NULL,
  attempt_no INTEGER NOT NULL,
  idempotency_key TEXT NOT NULL,
  outcome TEXT,
  payload JSONB NOT NULL DEFAULT '{}'::JSONB,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE pending_reconcile_items (
  pending_ref TEXT PRIMARY KEY,
  scope_kind TEXT NOT NULL,
  scope_id TEXT NOT NULL,
  reason TEXT NOT NULL,
  payload JSONB NOT NULL DEFAULT '{}'::JSONB,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE shadow_execution_artifacts (
  artifact_id BIGSERIAL PRIMARY KEY,
  attempt_id TEXT NOT NULL,
  stream TEXT NOT NULL,
  payload JSONB NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

- [ ] **Step 4: Implement row models and repos for progress, attempts, pending reconcile, and shadow artifacts**

```rust
pub struct RuntimeProgressRow {
    pub last_journal_seq: i64,
    pub last_state_version: i64,
    pub last_snapshot_id: Option<String>,
}

pub struct ExecutionAttemptRow {
    pub attempt_id: String,
    pub plan_id: String,
    pub snapshot_id: String,
    pub execution_mode: String,
    pub attempt_no: i32,
    pub idempotency_key: String,
}

pub struct PendingReconcileRow {
    pub pending_ref: String,
    pub scope_kind: String,
    pub scope_id: String,
    pub reason: String,
}
```

- [ ] **Step 5: Run the persistence test to verify it passes**

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test runtime_backbone -- --test-threads=1`

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add migrations/0005_unified_runtime_backbone.sql crates/persistence
git commit -m "feat: persist unified runtime anchors and attempts"
```

## Task 3: Add Authoritative State Apply Contracts

**Files:**
- Create: `crates/state/src/facts.rs`
- Create: `crates/state/src/apply.rs`
- Modify: `crates/state/src/store.rs`
- Modify: `crates/state/src/lib.rs`
- Test: `crates/state/tests/event_apply_contract.rs`

- [ ] **Step 1: Write the failing state-applier test**

```rust
use chrono::Utc;
use domain::ExternalFactEvent;
use state::{ApplyResult, StateApplier, StateStore};

#[test]
fn duplicate_fact_returns_duplicate_anchor_without_mutating_state_version() {
    let mut store = StateStore::new();
    let mut applier = StateApplier::new(&mut store);
    let event = ExternalFactEvent::new("market_ws", "session-1", "evt-1", "v1", Utc::now());

    let first = applier.apply(17, event.clone()).unwrap();
    let duplicate = applier.apply(18, event).unwrap();

    assert!(matches!(first, ApplyResult::Applied { .. }));
    assert!(matches!(duplicate, ApplyResult::Duplicate { duplicate_of_journal_seq: 17, .. }));
}

#[test]
fn out_of_order_fact_creates_reconcile_required_pending_ref() {
    let mut store = StateStore::new();
    let mut applier = StateApplier::new(&mut store);
    let event = sample_out_of_order_user_trade();

    let result = applier.apply(19, event).unwrap();
    assert!(matches!(result, ApplyResult::ReconcileRequired { pending_ref: Some(_), .. }));
}
```

- [ ] **Step 2: Run the state-applier test to verify it fails**

Run: `cargo test -p state --test event_apply_contract`

Expected: FAIL because `StateApplier` and `ApplyResult` do not exist yet.

- [ ] **Step 3: Implement dirty-set and pending-reference support**

```rust
pub enum DirtyDomain {
    Runtime,
    Orders,
    Inventory,
    Approvals,
    Resolution,
    Relayer,
    NegRiskFamilies,
}

pub struct PendingRef(pub String);

pub struct DirtySet {
    pub domains: BTreeSet<DirtyDomain>,
}
```

- [ ] **Step 4: Implement `StateApplier` and `ApplyResult` with authoritative ordering semantics**

```rust
pub enum ApplyResult {
    Applied {
        journal_seq: i64,
        state_version: u64,
        dirty_set: DirtySet,
    },
    Duplicate {
        journal_seq: i64,
        duplicate_of_journal_seq: i64,
        state_version: u64,
    },
    Deferred {
        journal_seq: i64,
        pending_ref: PendingRef,
        reason: String,
    },
    ReconcileRequired {
        journal_seq: i64,
        pending_ref: Option<PendingRef>,
        reason: String,
    },
}

pub struct StateApplier<'a> {
    store: &'a mut StateStore,
}
```

- [ ] **Step 5: Extend `StateStore` with `state_version` and apply-time confidence markers**

```rust
#[derive(Debug, Clone)]
pub struct StateStore {
    state_version: u64,
    last_applied_journal_seq: Option<i64>,
    // existing fields...
}
```

- [ ] **Step 6: Run the state-applier test to verify it passes**

Run: `cargo test -p state --test event_apply_contract`

Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/state
git commit -m "feat: add authoritative state apply contracts"
```

## Task 4: Publish Route-Local Snapshots With Projection Readiness

**Files:**
- Create: `crates/state/src/snapshot.rs`
- Modify: `crates/state/src/store.rs`
- Modify: `crates/state/src/lib.rs`
- Test: `crates/state/tests/snapshot_publish.rs`
- Modify: `crates/state/tests/bootstrap_reconcile.rs`

- [ ] **Step 1: Write the failing snapshot publication test**

```rust
use state::{ProjectionReadiness, PublishedSnapshot, StateStore};

#[test]
fn fullset_snapshot_publish_does_not_wait_for_negrisk_projection() {
    let store = sample_store_with_fullset_only();
    let snapshot = PublishedSnapshot::from_store(
        &store,
        ProjectionReadiness::ready_fullset_pending_negrisk("snapshot-7"),
    );

    assert_eq!(snapshot.snapshot_id, "snapshot-7");
    assert!(snapshot.fullset.is_some());
    assert!(snapshot.negrisk.is_none());
    assert!(!snapshot.negrisk_ready);
}

#[test]
fn published_snapshot_keeps_projection_readiness_flags_explicit() {
    let snapshot = sample_snapshot("snapshot-9");
    assert_eq!(snapshot.state_version, 9);
    assert!(snapshot.fullset_ready);
    assert!(!snapshot.negrisk_ready);
}
```

- [ ] **Step 2: Run the snapshot publication test to verify it fails**

Run: `cargo test -p state --test snapshot_publish`

Expected: FAIL because `PublishedSnapshot` and projection readiness types do not exist yet.

- [ ] **Step 3: Implement published snapshot and readiness types**

```rust
pub struct PublishedSnapshot {
    pub snapshot_id: String,
    pub state_version: u64,
    pub committed_journal_seq: i64,
    pub fullset_ready: bool,
    pub negrisk_ready: bool,
    pub fullset: Option<FullSetView>,
    pub negrisk: Option<NegRiskView>,
}

pub struct FullSetView {
    pub snapshot_id: String,
    pub state_version: u64,
    pub open_orders: Vec<String>,
}

pub struct NegRiskView {
    pub snapshot_id: String,
    pub state_version: u64,
    pub family_ids: Vec<String>,
}
```

- [ ] **Step 4: Derive route-local projections from one committed store version**

```rust
impl PublishedSnapshot {
    pub fn from_store(store: &StateStore, readiness: ProjectionReadiness) -> Self {
        // derive both projections from the same state_version, leaving route-local Option<> holes
        // when a projection is not ready.
    }
}
```

- [ ] **Step 5: Update bootstrap/reconcile tests to assert `state_version` and publish-friendly anchors**

Run: `cargo test -p state --test bootstrap_reconcile -- --test-threads=1`

Expected: PASS after updating existing assertions for the new state fields.

- [ ] **Step 6: Run the snapshot publication test to verify it passes**

Run: `cargo test -p state --test snapshot_publish`

Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/state
git commit -m "feat: publish route-local snapshots"
```

## Task 5: Add Strategy Intents, Activation Policy, And Unified Risk Entry

**Files:**
- Modify: `crates/risk/Cargo.toml`
- Modify: `crates/strategy-fullset/Cargo.toml`
- Create: `crates/strategy-fullset/src/intent.rs`
- Modify: `crates/strategy-fullset/src/lib.rs`
- Create: `crates/strategy-fullset/tests/intent.rs`
- Modify: `crates/strategy-negrisk/Cargo.toml`
- Create: `crates/strategy-negrisk/src/intent.rs`
- Modify: `crates/strategy-negrisk/src/lib.rs`
- Create: `crates/strategy-negrisk/tests/intent.rs`
- Create: `crates/risk/src/activation.rs`
- Create: `crates/risk/src/engine.rs`
- Create: `crates/risk/src/negrisk.rs`
- Modify: `crates/risk/src/fullset.rs`
- Modify: `crates/risk/src/lib.rs`
- Test: `crates/risk/tests/activation_policy.rs`
- Test: `crates/risk/tests/decision_engine.rs`
- Modify: `crates/risk/tests/fullset_guards.rs`

- [ ] **Step 1: Write the failing strategy and activation tests**

```rust
use domain::{ActivationDecision, DecisionInput, DecisionVerdict, ExecutionMode};
use risk::ActivationPolicy;

#[test]
fn fullset_strategy_emits_intent_from_fullset_view() {
    let intents = strategy_fullset::build_intents(&sample_fullset_view());
    assert_eq!(intents.len(), 1);
    assert!(matches!(intents[0], DecisionInput::Strategy(_)));
}

#[test]
fn negrisk_strategy_is_silent_when_projection_is_not_ready() {
    let intents = strategy_negrisk::build_intents(&sample_unready_negrisk_view());
    assert!(intents.is_empty());
}

#[test]
fn activation_policy_returns_live_for_fullset_and_shadow_for_negrisk() {
    let policy = ActivationPolicy::phase_one_defaults();
    assert_eq!(policy.mode_for_route("full-set", "market-a"), ExecutionMode::Live);
    assert_eq!(policy.mode_for_route("neg-risk", "family-a"), ExecutionMode::Shadow);
}

#[test]
fn recovery_overlay_takes_precedence_over_rollout_mode() {
    let policy = ActivationPolicy::phase_one_defaults()
        .with_overlay("family-a", ExecutionMode::RecoveryOnly);
    assert_eq!(
        policy.mode_for_route("neg-risk", "family-a"),
        ExecutionMode::RecoveryOnly
    );
}

#[test]
fn recovery_only_rejects_strategy_inputs_but_allows_recovery_inputs() {
    let activation = ActivationDecision::recovery_only("family-a", "policy-v1", Some("rule-7"));
    let strategy = sample_strategy_input("family-a");
    let recovery = sample_recovery_input("family-a");

    assert!(matches!(
        risk::evaluate_decision(&strategy, &activation),
        DecisionVerdict::Rejected
    ));
    assert!(matches!(
        risk::evaluate_decision(&recovery, &activation),
        DecisionVerdict::Approved
    ));
}
```

- [ ] **Step 2: Run the strategy and risk tests to verify they fail**

Run:

```bash
cargo test -p strategy-fullset --test intent
cargo test -p strategy-negrisk --test intent
cargo test -p risk --test activation_policy
cargo test -p risk --test decision_engine
```

Expected: FAIL because the intent builders and activation policy do not exist yet.

- [ ] **Step 3: Implement `IntentCandidate` builders in the strategy crates**

```rust
pub fn build_intents(view: &FullSetView) -> Vec<DecisionInput> {
    vec![DecisionInput::Strategy(IntentCandidate::new(
        "fullset-intent-1",
        &view.snapshot_id,
        "full-set",
    ))]
}

pub fn build_intents(view: &NegRiskView) -> Vec<DecisionInput> {
    if view.family_ids.is_empty() {
        return Vec::new();
    }

    vec![DecisionInput::Strategy(IntentCandidate::new(
        "negrisk-intent-1",
        &view.snapshot_id,
        "neg-risk",
    ))]
}
```

- [ ] **Step 4: Implement `ActivationPolicy` and the route-neutral risk engine**

```rust
pub struct ActivationPolicy {
    capability_matrix: BTreeMap<(String, String), ExecutionMode>,
    overlays: BTreeMap<String, ExecutionMode>,
    policy_version: String,
}

pub fn evaluate_decision(
    input: &DecisionInput,
    activation: &ActivationDecision,
) -> DecisionVerdict {
    if matches!(activation.mode, ExecutionMode::Disabled) {
        return DecisionVerdict::Rejected;
    }
    if matches!(activation.mode, ExecutionMode::RecoveryOnly)
        && matches!(input, DecisionInput::Strategy(_))
    {
        return DecisionVerdict::Rejected;
    }
    DecisionVerdict::Approved
}
```

- [ ] **Step 5: Adapt the existing full-set checks to the new risk entrypoint**

```rust
pub fn evaluate_fullset_trade_in_mode(
    mode: ExecutionMode,
    context: &FullSetRiskContext,
) -> RiskDecision {
    if !matches!(mode, ExecutionMode::Live | ExecutionMode::Shadow) {
        return RiskDecision::Reject(RejectReason::ModeNotHealthy);
    }
    evaluate_fullset_trade(context)
}
```

- [ ] **Step 6: Run the strategy and risk tests to verify they pass**

Run:

```bash
cargo test -p strategy-fullset --test intent
cargo test -p strategy-negrisk --test intent
cargo test -p risk --test activation_policy
cargo test -p risk --test decision_engine
cargo test -p risk --test fullset_guards
```

Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/strategy-fullset crates/strategy-negrisk crates/risk
git commit -m "feat: add route intents and activation risk engine"
```

## Task 6: Build Execution Orchestration, Attempts, And Live/Shadow Sinks

**Files:**
- Modify: `crates/execution/Cargo.toml`
- Create: `crates/execution/src/attempt.rs`
- Create: `crates/execution/src/orchestrator.rs`
- Create: `crates/execution/src/sink.rs`
- Modify: `crates/execution/src/plans.rs`
- Modify: `crates/execution/src/orders.rs`
- Modify: `crates/execution/src/ctf.rs`
- Modify: `crates/execution/src/lib.rs`
- Test: `crates/execution/tests/orchestrator.rs`
- Modify: `crates/execution/tests/retry_and_redeem.rs`

- [ ] **Step 1: Write the failing execution orchestration test**

```rust
use execution::{ExecutionOrchestrator, LiveVenueSink, ShadowVenueSink};

#[test]
fn shadow_and_live_share_the_same_plan_before_the_final_sink() {
    let request = sample_execution_request();
    let live = ExecutionOrchestrator::new(LiveVenueSink::noop());
    let shadow = ExecutionOrchestrator::new(ShadowVenueSink::noop());

    let live_plan = live.plan(&request).unwrap();
    let shadow_plan = shadow.plan(&request).unwrap();

    assert_eq!(live_plan, shadow_plan);
}

#[test]
fn shadow_sink_records_attempt_without_authoritative_fill_effect() {
    let orchestrator = ExecutionOrchestrator::new(ShadowVenueSink::noop());
    let receipt = orchestrator.execute(sample_execution_request()).unwrap();

    assert!(receipt.is_shadow_recorded());
}

#[test]
fn reduce_only_mode_refuses_plans_that_expand_risk() {
    let orchestrator = ExecutionOrchestrator::new(LiveVenueSink::noop());
    let err = orchestrator.plan(sample_reduce_only_expanding_request()).unwrap_err();

    assert!(err.is_mode_violation());
}
```

- [ ] **Step 2: Run the execution orchestration test to verify it fails**

Run: `cargo test -p execution --test orchestrator`

Expected: FAIL because the orchestrator, attempt layer, and sink trait do not exist yet.

- [ ] **Step 3: Implement attempt identity and execution context**

```rust
pub struct ExecutionAttemptFactory {
    next_attempt_no: u32,
}

impl ExecutionAttemptFactory {
    pub fn next_for_plan(
        &mut self,
        plan: &ExecutionPlan,
        snapshot_id: &str,
        execution_mode: ExecutionMode,
    ) -> (ExecutionAttempt, ExecutionAttemptContext) {
        // construct the domain-owned attempt record and execution context together
    }
}
```

- [ ] **Step 4: Implement the shared orchestrator and sink boundary**

```rust
pub trait VenueSink {
    fn execute(
        &self,
        plan: &ExecutionPlan,
        attempt: &ExecutionAttemptContext,
    ) -> Result<ExecutionReceipt, ExecutionError>;
}

pub struct ExecutionOrchestrator<S> {
    sink: S,
}
```

- [ ] **Step 5: Adapt plan/order/CTF helpers to work through the new attempt-aware path**

```rust
pub enum ExecutionPlan {
    SubmitOrder { route: String, scope: String },
    CancelOrder { order_id: String },
    Split { condition_id: String },
    Merge { condition_id: String },
    Redeem { condition_id: String },
}
```

- [ ] **Step 6: Run the execution tests to verify they pass**

Run:

```bash
cargo test -p execution --test orchestrator
cargo test -p execution --test retry_and_redeem
```

Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/execution
git commit -m "feat: add execution orchestrator and shadow sink"
```

## Task 7: Add A First-Class Recovery Crate

**Files:**
- Modify: `/Users/viv/projs/axiom-arb/Cargo.toml`
- Create: `crates/recovery/Cargo.toml`
- Create: `crates/recovery/src/lib.rs`
- Create: `crates/recovery/src/locks.rs`
- Create: `crates/recovery/src/coordinator.rs`
- Test: `crates/recovery/tests/recovery_contract.rs`

- [ ] **Step 1: Write the failing recovery contract test**

```rust
use recovery::{RecoveryCoordinator, RecoveryScopeLock};

#[test]
fn recovery_scope_lock_blocks_strategy_expansion_for_same_family() {
    let lock = RecoveryScopeLock::family("family-a");
    assert!(lock.blocks_expansion("family-a"));
    assert!(!lock.blocks_expansion("family-b"));
}

#[test]
fn ambiguous_attempt_emits_recovery_intent_or_pending_reconcile() {
    let coordinator = RecoveryCoordinator::default();
    let outputs = coordinator.on_failed_ambiguous(sample_ambiguous_attempt());

    assert!(outputs.recovery_intent.is_some() || outputs.pending_reconcile.is_some());
}
```

- [ ] **Step 2: Run the recovery test to verify it fails**

Run: `cargo test -p recovery --test recovery_contract`

Expected: FAIL because the `recovery` crate does not exist yet.

- [ ] **Step 3: Add the new workspace crate and lock types**

```toml
# Cargo.toml
members = [
  # ...
  "crates/recovery",
]
```

```rust
pub enum RecoveryScopeLock {
    Market(String),
    Condition(String),
    Family(String),
    InventorySet(String),
    ExecutionPath(String),
}
```

- [ ] **Step 4: Implement the recovery coordinator and convergence outputs**

```rust
pub struct RecoveryOutputs {
    pub recovery_intent: Option<RecoveryIntent>,
    pub pending_reconcile: Option<String>,
}

pub struct RecoveryCoordinator;

impl RecoveryCoordinator {
    pub fn on_failed_ambiguous(&self, attempt: ExecutionAttempt) -> RecoveryOutputs {
        // emit structured follow-up instead of leaving only a warning log
    }
}
```

- [ ] **Step 5: Run the recovery test to verify it passes**

Run: `cargo test -p recovery --test recovery_contract`

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/recovery
git commit -m "feat: add recovery coordinator contracts"
```

## Task 8: Wire The Unified App Supervisor And Dispatcher

**Files:**
- Modify: `crates/app-live/Cargo.toml`
- Create: `crates/app-live/src/supervisor.rs`
- Create: `crates/app-live/src/dispatch.rs`
- Create: `crates/app-live/src/input_tasks.rs`
- Modify: `crates/app-live/src/runtime.rs`
- Modify: `crates/app-live/src/bootstrap.rs`
- Modify: `crates/app-live/src/main.rs`
- Modify: `crates/app-live/src/lib.rs`
- Test: `crates/app-live/tests/unified_supervisor.rs`
- Test: `crates/app-live/tests/fault_injection.rs`
- Modify: `crates/app-live/tests/bootstrap_modes.rs`
- Modify: `crates/app-live/tests/main_entrypoint.rs`

- [ ] **Step 1: Write the failing supervisor and fault-injection tests**

```rust
use app_live::AppSupervisor;

#[test]
fn supervisor_bootstraps_fullset_live_and_negrisk_shadow_modes() {
    let result = AppSupervisor::for_tests().run_once().unwrap();
    assert_eq!(result.fullset_mode.as_str(), "live");
    assert_eq!(result.negrisk_mode.as_str(), "shadow");
}

#[test]
fn dispatcher_coalesces_dirty_snapshots_without_dropping_latest_version() {
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.push_dirty_snapshot(7);
    supervisor.push_dirty_snapshot(8);
    supervisor.push_dirty_snapshot(9);

    let dispatched = supervisor.flush_dispatch();
    assert_eq!(dispatched.last_dispatched_state_version, 9);
}

#[test]
fn restart_resumes_from_durable_journal_state_snapshot_anchors() {
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.seed_runtime_progress(41, 7, None);
    supervisor.seed_committed_state_version(7);

    let resumed = supervisor.resume_once().unwrap();
    assert_eq!(resumed.last_journal_seq, 41);
    assert_eq!(resumed.last_state_version, 7);
    assert_eq!(resumed.published_snapshot_id.as_deref(), Some("snapshot-7"));
}

#[test]
fn restart_replays_unapplied_journal_entries_before_dispatch_resumes() {
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.seed_runtime_progress(41, 6, Some("snapshot-6"));
    supervisor.seed_unapplied_journal_entry(42, sample_input_task_event());

    let resumed = supervisor.resume_once().unwrap();
    assert_eq!(resumed.last_journal_seq, 42);
    assert_eq!(resumed.last_state_version, 7);
    assert_eq!(resumed.published_snapshot_id.as_deref(), Some("snapshot-7"));
}
```

- [ ] **Step 2: Run the app-live tests to verify they fail**

Run:

```bash
cargo test -p app-live --test unified_supervisor
cargo test -p app-live --test fault_injection
```

Expected: FAIL because the supervisor and dispatcher do not exist yet.

- [ ] **Step 3: Implement the supervisor and input-task adapters**

```rust
pub struct AppSupervisor {
    dispatcher: DispatchLoop,
    runtime: AppRuntime,
}

pub struct InputTaskEvent {
    pub journal_seq: i64,
    pub event: ExternalFactEvent,
}
```

- [ ] **Step 4: Implement dirty-set coalescing and projection-readiness-aware dispatch**

```rust
pub struct DispatchLoop {
    latest_ready_snapshot: Option<PublishedSnapshot>,
    dirty_versions: Vec<u64>,
}

impl DispatchLoop {
    pub fn flush(&mut self) -> DispatchSummary {
        // coalesce intermediate versions but keep the latest stable one
    }
}
```

- [ ] **Step 5: Replace the static bootstrap entrypoint with the supervisor-backed path**

Run:

```bash
cargo test -p app-live --test bootstrap_modes
cargo test -p app-live --test main_entrypoint
```

Expected: PASS after existing entrypoint tests are updated to exercise the supervisor path.

- [ ] **Step 6: Run the new supervisor tests to verify they pass**

Run:

```bash
cargo test -p app-live --test unified_supervisor
cargo test -p app-live --test fault_injection
```

Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/app-live
git commit -m "feat: wire unified app supervisor"
```

## Task 9: Finish Observability, Docs, And Verification

**Files:**
- Modify: `crates/observability/src/metrics.rs`
- Modify: `crates/observability/src/lib.rs`
- Modify: `crates/observability/tests/metrics.rs`
- Modify: `/Users/viv/projs/axiom-arb/README.md`

- [ ] **Step 1: Write the failing observability test**

```rust
use observability::{MetricKey, RuntimeMetrics};

#[test]
fn runtime_metrics_expose_dispatch_and_recovery_backlog_signals() {
    let metrics = RuntimeMetrics::default();
    assert_eq!(
        metrics.dispatcher_backlog_count.key(),
        MetricKey::new("axiom_dispatcher_backlog_count")
    );
    assert_eq!(
        metrics.recovery_backlog_count.key(),
        MetricKey::new("axiom_recovery_backlog_count")
    );
}
```

- [ ] **Step 2: Run the observability test to verify it fails**

Run: `cargo test -p observability runtime_metrics_expose_dispatch_and_recovery_backlog_signals -- --exact`

Expected: FAIL because the new typed metrics do not exist yet.

- [ ] **Step 3: Implement the final metrics and README updates**

```rust
pub struct RuntimeMetrics {
    pub dispatcher_backlog_count: GaugeHandle,
    pub projection_publish_lag_count: GaugeHandle,
    pub recovery_backlog_count: GaugeHandle,
    pub shadow_attempt_count: CounterHandle,
    // existing fields...
}
```

Document in `README.md`:

- unified runtime `Phase 1` = `full-set Live`
- unified runtime `Phase 2` = `neg-risk Shadow`
- `Phase 3 neg-risk Live` is not enabled by this plan
- shadow artifacts live in isolated storage/stream paths

- [ ] **Step 4: Run formatting**

Run: `cargo fmt --all`

Expected: PASS

- [ ] **Step 5: Run lints**

Run: `cargo clippy --workspace --all-targets -- -D warnings`

Expected: PASS

- [ ] **Step 6: Run the targeted verification suite**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p domain --test runtime_backbone
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test runtime_backbone -- --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p state --test event_apply_contract
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p state --test snapshot_publish
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p strategy-fullset --test intent
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p strategy-negrisk --test intent
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p risk --test activation_policy
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p risk --test decision_engine
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p execution --test orchestrator
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p recovery --test recovery_contract
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test unified_supervisor
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test fault_injection
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p observability runtime_metrics_expose_dispatch_and_recovery_backlog_signals -- --exact
```

Expected: PASS

- [ ] **Step 7: Run the existing regression targets that must keep passing**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-replay
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p venue-polymarket
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p strategy-negrisk
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence
```

Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add README.md crates/observability
git commit -m "docs: close out unified runtime phase 1 and 2 surface"
```

## Follow-On Plan

After this plan is complete and the new tests are green, write a separate `Phase 3` plan to:

- promote specific `neg-risk` families from `Shadow` to `Live`
- add capability-matrix-driven rollout rules per family
- prove `shadow/live parity`, `recovery contract`, `replay drift detection`, and `fault-injection resilience`
- keep `neg-risk` families that are not ready in `Disabled`, `Shadow`, `ReduceOnly`, or `RecoveryOnly`

## Plan Review Notes

Use the local reviewer prompt at:

- `/Users/viv/.codex/superpowers/skills/writing-plans/plan-document-reviewer-prompt.md`

Review this plan against:

- `/Users/viv/projs/axiom-arb/docs/superpowers/specs/2026-03-24-axiomarb-unified-runtime-v1a-v1b-design.md`
