# Phase 3b Neg-Risk Live Backbone Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the remaining `Phase 3b` runtime surface so `neg-risk` can move from family-scoped `Shadow` gates to family-scoped `Live` planning, live-attempt persistence, and replayable execution artifacts on the unified backbone.

**Architecture:** Build directly on the merged `Phase 3a` rollout-gate work instead of inventing another runtime. This plan adds route-aware request/attempt contracts, a minimal operator-supplied family live-target config, a real `neg-risk` execution-plan shape, signer/request-builder plumbing, live artifact persistence/replay, and `app-live` wiring that promotes eligible families through `ActivationPolicy` instead of hard-coding `Shadow`. It intentionally does **not** turn `app-live` into a full external-feed daemon and does **not** attempt a sophisticated pricing engine.

**Tech Stack:** Rust workspace crates (`domain`, `execution`, `risk`, `state`, `app-live`, `persistence`, `app-replay`, `venue-polymarket`, `observability`), SQL migrations, Cargo tests, Clippy, unified-runtime spec contracts, existing `Phase 3a` rollout-gate surfaces.

---

## Scope Check

The remaining phrase "`Phase 3 neg-risk Live`" is still too wide to treat as one unbounded blob.

This plan covers one coherent sub-project:

1. route-aware `neg-risk` live planning and execution contracts
2. family-scoped live-target configuration
3. live/shadow orchestration, persistence, replay, and supervisor wiring

This plan deliberately does **not** cover two separate follow-on concerns:

1. dynamic or market-discovered `neg-risk` pricing research
2. full production collector / websocket / relayer daemonization in `app-live`

The current repository still boots `app-live` from [`StaticSnapshotSource::empty()`](/Users/viv/projs/axiom-arb/crates/app-live/src/main.rs#L19). Do **not** try to solve production feed ingestion in this plan. Keep this plan focused on the route-specific live backbone that the current harness can prove with tests.

## File Map

### Existing files to modify

- `crates/domain/src/decision.rs`
  - add explicit route metadata to `IntentCandidate` and keep `DecisionInput` input-neutral while making downstream planning route-aware
- `crates/domain/src/execution.rs`
  - expand `ExecutionRequest` and `ExecutionAttemptContext` so route, scope, and activation anchors survive into planning, attempts, and receipts
- `crates/domain/src/lib.rs`
  - re-export any newly added execution or live-target contract types
- `crates/domain/tests/runtime_backbone.rs`
  - lock down the enriched request / attempt / receipt contracts before touching planner code
- `crates/execution/src/plans.rs`
  - add a real `neg-risk` family submission plan shape alongside existing `full-set` plans
- `crates/execution/src/orchestrator.rs`
  - keep shared orchestration but allow route-aware planning inputs and mode checks for `neg-risk`
- `crates/execution/src/attempt.rs`
  - preserve request-bound plan identity after route/scope metadata expands
- `crates/execution/src/sink.rs`
  - keep `Shadow` and `Live` sharing the same pre-sink path while allowing `neg-risk` live submit plumbing
- `crates/execution/src/lib.rs`
  - export the new planner / signer / live-submit types
- `crates/execution/tests/orchestrator.rs`
  - keep the parity guarantees green after adding route-aware plans and attempts
- `crates/risk/src/engine.rs`
  - keep the activation/risk boundary intact while allowing `Live` `neg-risk` requests through only when the input is eligible
- `crates/app-live/src/main.rs`
  - load minimal planner config from environment and fail fast on invalid live-target config
- `crates/app-live/src/runtime.rs`
  - store route-aware plan execution inputs and keep published-snapshot / restart semantics intact
- `crates/app-live/src/supervisor.rs`
  - stop hard-coding `negrisk_mode=Shadow`; summarize live-capable families and durable live-attempt anchors instead
- `crates/app-live/src/lib.rs`
  - export the new config types and supervisor summary surfaces
- `crates/app-live/src/dispatch.rs`
  - let the dispatcher expose ready `neg-risk` snapshots to the new planner path without regressing route-local readiness
- `crates/app-live/tests/main_entrypoint.rs`
  - verify config parsing and live-mode startup behavior at the binary boundary
- `crates/persistence/src/models.rs`
  - add live `neg-risk` execution artifact rows without breaking existing shadow-artifact or attempt rows
- `crates/persistence/src/repos.rs`
  - persist new live execution artifacts and expose replay queries for them
- `crates/persistence/src/lib.rs`
  - re-export any new repo helpers
- `crates/persistence/tests/runtime_backbone.rs`
  - lock down the new live artifact persistence contract
- `crates/app-replay/src/lib.rs`
  - surface a replay consumer or helper for route-aware live attempts/artifacts
- `crates/app-replay/tests/replay_app.rs`
  - keep generic event-journal replay green after new event types or streams are added
- `crates/venue-polymarket/src/rest.rs`
  - add request builders for live order submission without changing existing status/balance APIs
- `crates/venue-polymarket/src/lib.rs`
  - export the new live-submit request types
- `crates/venue-polymarket/tests/status_and_retry.rs`
  - keep existing retry/status semantics green while adding submit-request coverage
- `crates/observability/src/metrics.rs`
  - record `neg-risk` live-attempt and live-submit artifact counters
- `crates/observability/tests/metrics.rs`
  - verify the new runtime metric keys
- `README.md`
  - update unified-runtime rollout status so `Phase 3a` and `Phase 3b` are clearly separated

### New files to create

- `crates/app-live/src/config.rs`
  - parse minimal family-scoped live-target config from environment in a way tests can control
- `crates/app-live/tests/config.rs`
  - focused config parser tests
- `crates/execution/src/negrisk.rs`
  - hold the minimal route-specific planner that converts a family scope plus config into a `neg-risk` execution plan
- `crates/execution/src/signing.rs`
  - define a small signer abstraction and deterministic test signer so live-submit plumbing is testable without inventing a production signer implementation in this plan
- `crates/execution/tests/negrisk_live_planner.rs`
  - focused tests for family-plan construction and mode behavior
- `crates/execution/tests/negrisk_signing.rs`
  - focused tests for signer plumbing and signed-envelope attachment
- `crates/persistence/tests/negrisk_live.rs`
  - focused persistence tests for live artifacts and replay anchors
- `crates/app-live/tests/negrisk_live_rollout.rs`
  - focused supervisor/runtime tests for family-scoped live promotion and durable attempt state
- `crates/app-replay/tests/negrisk_live_contract.rs`
  - focused replay tests for live `neg-risk` attempt/artifact streams
- `crates/venue-polymarket/src/orders.rs`
  - build live order-submit request bodies from signed order envelopes
- `crates/venue-polymarket/tests/order_submission.rs`
  - focused submit-request tests
- `migrations/0006_phase3b_negrisk_live.sql`
  - add schema needed for live `neg-risk` artifacts

## Implementation Notes

- Reuse the existing `Phase 3a` authority chain: `runtime overlays + family halt + recovery scope lock + rollout capability -> ActivationPolicy -> ActivationDecision -> Risk -> Planner`. Do **not** let planner or executor grow a private route-enablement switch.
- Keep `Shadow` and `Live` identical through planning, attempt creation, journaling, and metrics. The only intentional fork remains the final live/shadow sink path.
- Because the repository does not currently expose `neg-risk` price discovery or order-book state in snapshots, this plan uses explicit operator-supplied family live-target config. Do **not** invent dynamic pricing heuristics here.
- Because the repository does not currently include a production signer, add a narrow signer abstraction and deterministic test implementation. Do **not** attempt cryptographic signing infrastructure in this plan.
- Do **not** change `Phase 3a` rollout readiness semantics. This plan should consume the readiness and capability-matrix results already merged.
- Tests must drive every new behavior before implementation (`@superpowers:test-driven-development`).

### Task 1: Expand Route-Aware Decision And Attempt Contracts

**Files:**
- Modify: `crates/domain/src/decision.rs`
- Modify: `crates/domain/src/execution.rs`
- Modify: `crates/domain/src/lib.rs`
- Test: `crates/domain/tests/runtime_backbone.rs`

- [ ] **Step 1: Write the failing domain contract tests**

```rust
#[test]
fn execution_request_preserves_route_scope_and_activation_anchor() {
    let intent = IntentCandidate::new("intent-1", "snapshot-7", "neg-risk", "family-a");
    assert_eq!(intent.route, "neg-risk");
    assert_eq!(intent.scope, "family-a");

    let request = ExecutionRequest {
        request_id: "request-1".to_owned(),
        decision_input_id: intent.intent_id.clone(),
        snapshot_id: "snapshot-7".to_owned(),
        route: "neg-risk".to_owned(),
        scope: "family-a".to_owned(),
        activation_mode: ExecutionMode::Live,
        matched_rule_id: Some("family-a-live".to_owned()),
    };

    assert_eq!(request.route, "neg-risk");
    assert_eq!(request.scope, "family-a");
    assert_eq!(request.activation_mode, ExecutionMode::Live);
    assert_eq!(request.matched_rule_id.as_deref(), Some("family-a-live"));
}
```

- [ ] **Step 2: Run the domain test to verify it fails**

Run: `cargo test -p domain --test runtime_backbone`

Expected: FAIL because `IntentCandidate` and `ExecutionRequest` do not yet carry route-aware live-planning fields.

- [ ] **Step 3: Implement the minimal contract expansion**

```rust
pub struct IntentCandidate {
    pub intent_id: String,
    pub source_snapshot_id: String,
    pub route: String,
    pub scope: String,
}

pub struct ExecutionRequest {
    pub request_id: String,
    pub decision_input_id: String,
    pub snapshot_id: String,
    pub route: String,
    pub scope: String,
    pub activation_mode: ExecutionMode,
    pub matched_rule_id: Option<String>,
}

pub struct ExecutionAttemptContext {
    pub attempt_id: String,
    pub snapshot_id: String,
    pub execution_mode: ExecutionMode,
    pub route: String,
    pub scope: String,
    pub matched_rule_id: Option<String>,
}
```

- [ ] **Step 4: Re-run the scoped domain test**

Run: `cargo test -p domain --test runtime_backbone`

Expected: PASS. Keep these types minimal; do **not** add planner config or signed-order details in this task.

- [ ] **Step 5: Commit**

```bash
git add crates/domain/src/decision.rs crates/domain/src/execution.rs crates/domain/src/lib.rs crates/domain/tests/runtime_backbone.rs
git commit -m "feat: add route-aware neg-risk execution contracts"
```

### Task 2: Add Family-Scoped Live Target Config To `app-live`

**Files:**
- Create: `crates/app-live/src/config.rs`
- Modify: `crates/app-live/src/lib.rs`
- Modify: `crates/app-live/src/main.rs`
- Test: `crates/app-live/tests/config.rs`
- Modify: `crates/app-live/tests/main_entrypoint.rs`

- [ ] **Step 1: Write the failing config parser tests**

```rust
#[test]
fn parses_neg_risk_live_target_config_from_env_json() {
    let json = r#"
    [
      {
        "family_id": "family-a",
        "members": [
          { "condition_id": "condition-1", "token_id": "token-1", "price": "0.43", "quantity": "5" },
          { "condition_id": "condition-2", "token_id": "token-2", "price": "0.41", "quantity": "5" }
        ]
      }
    ]
    "#;

    let config = load_neg_risk_live_targets(Some(json)).unwrap();
    assert_eq!(config["family-a"].members.len(), 2);
    assert_eq!(config["family-a"].members[0].token_id, "token-1");
}
```

- [ ] **Step 2: Run the config tests to verify they fail**

Run:

```bash
cargo test -p app-live --test config
cargo test -p app-live --test main_entrypoint -- --exact live_entrypoint_rejects_invalid_neg_risk_target_config
```

Expected: FAIL because `app-live` does not yet have a config module or a live-target env contract.

- [ ] **Step 3: Implement the minimal config module**

```rust
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NegRiskMemberLiveTarget {
    pub condition_id: String,
    pub token_id: String,
    pub price: Decimal,
    pub quantity: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NegRiskFamilyLiveTarget {
    pub family_id: String,
    pub members: Vec<NegRiskMemberLiveTarget>,
}

pub fn load_neg_risk_live_targets(
    json: Option<&str>,
) -> Result<BTreeMap<String, NegRiskFamilyLiveTarget>, ConfigError> { /* ... */ }
```

- [ ] **Step 4: Re-run the scoped `app-live` config tests**

Run:

```bash
cargo test -p app-live --test config
cargo test -p app-live --test main_entrypoint
```

Expected: PASS. Keep missing env = empty config. Only invalid JSON or duplicate family ids should fail.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/config.rs crates/app-live/src/lib.rs crates/app-live/src/main.rs crates/app-live/tests/config.rs crates/app-live/tests/main_entrypoint.rs
git commit -m "feat: add neg-risk family live target config"
```

### Task 3: Add The Minimal `neg-risk` Live Planner

**Files:**
- Create: `crates/execution/src/negrisk.rs`
- Modify: `crates/execution/src/plans.rs`
- Modify: `crates/execution/src/orchestrator.rs`
- Modify: `crates/execution/src/attempt.rs`
- Modify: `crates/execution/src/lib.rs`
- Modify: `crates/risk/src/engine.rs`
- Test: `crates/execution/tests/negrisk_live_planner.rs`
- Modify: `crates/execution/tests/orchestrator.rs`

- [ ] **Step 1: Write the failing planner tests**

```rust
#[test]
fn planner_builds_family_submission_plan_from_live_target_config() {
    let request = sample_negrisk_request(ExecutionMode::Live, "family-a");
    let config = sample_family_target("family-a");

    let plan = execution::negrisk::plan_family_submission(&request, &config).unwrap();

    match plan {
        ExecutionPlan::NegRiskSubmitFamily { family_id, members } => {
            assert_eq!(family_id.as_str(), "family-a");
            assert_eq!(members.len(), 2);
            assert_eq!(members[0].token_id.as_str(), "token-1");
        }
        other => panic!("unexpected plan: {other:?}"),
    }
}
```

- [ ] **Step 2: Run the execution tests to verify they fail**

Run:

```bash
cargo test -p execution --test negrisk_live_planner
cargo test -p execution --test orchestrator
```

Expected: FAIL because there is no `NegRiskSubmitFamily` plan and no family planner.

- [ ] **Step 3: Implement the minimal route-specific plan**

```rust
pub struct NegRiskMemberOrderPlan {
    pub condition_id: ConditionId,
    pub token_id: TokenId,
    pub price: Decimal,
    pub quantity: Decimal,
}

pub enum ExecutionPlan {
    FullSetBuyThenMerge { condition_id: ConditionId },
    FullSetSplitThenSell { condition_id: ConditionId },
    CancelStale { order_id: OrderId },
    RedeemResolved { condition_id: ConditionId },
    NegRiskSubmitFamily {
        family_id: EventFamilyId,
        members: Vec<NegRiskMemberOrderPlan>,
    },
}
```

- [ ] **Step 4: Keep execution-mode behavior explicit**

Run:

```bash
cargo test -p execution --test negrisk_live_planner
cargo test -p execution --test orchestrator
```

Expected: PASS with these semantics:
- `Live` and `Shadow` build the same `NegRiskSubmitFamily` plan before the final sink
- `ReduceOnly` rejects the plan as risk-expanding
- `RecoveryOnly` rejects strategy-originated family-submit plans

Do **not** add signing or HTTP submission in this task. This task ends at a business-level family submission plan.

- [ ] **Step 5: Commit**

```bash
git add crates/execution/src/negrisk.rs crates/execution/src/plans.rs crates/execution/src/orchestrator.rs crates/execution/src/attempt.rs crates/execution/src/lib.rs crates/risk/src/engine.rs crates/execution/tests/negrisk_live_planner.rs crates/execution/tests/orchestrator.rs
git commit -m "feat: add neg-risk live family planner"
```

### Task 4: Add Signer Plumbing And Venue Submit Request Builders

**Files:**
- Create: `crates/execution/src/signing.rs`
- Modify: `crates/execution/src/sink.rs`
- Modify: `crates/execution/src/lib.rs`
- Test: `crates/execution/tests/negrisk_signing.rs`
- Create: `crates/venue-polymarket/src/orders.rs`
- Modify: `crates/venue-polymarket/src/rest.rs`
- Modify: `crates/venue-polymarket/src/lib.rs`
- Create: `crates/venue-polymarket/tests/order_submission.rs`

- [ ] **Step 1: Write the failing signing and request-builder tests**

```rust
#[test]
fn deterministic_test_signer_attaches_signed_identity_to_each_planned_member_order() {
    let signed = TestOrderSigner::default().sign_family(&sample_family_plan()).unwrap();
    assert_eq!(signed.orders.len(), 2);
    assert!(signed.orders.iter().all(|order| order.order.signed_order.is_some()));
}

#[test]
fn submit_order_request_uses_documented_post_path_and_signed_payload() {
    let client = sample_rest_client();
    let request = client
        .build_submit_order_request(&sample_l2_auth(), &sample_signed_order_submission())
        .unwrap();

    assert_eq!(request.method().as_str(), "POST");
    assert!(request.url().as_str().ends_with("/order"));
}
```

- [ ] **Step 2: Run the scoped tests to verify they fail**

Run:

```bash
cargo test -p execution --test negrisk_signing
cargo test -p venue-polymarket --test order_submission
```

Expected: FAIL because there is no signer abstraction and no order-submit request builder.

- [ ] **Step 3: Implement the narrow signer abstraction**

```rust
pub trait OrderSigner {
    fn sign_family(
        &self,
        plan: &ExecutionPlan,
    ) -> Result<SignedFamilySubmission, SigningError>;
}

#[derive(Debug, Default)]
pub struct TestOrderSigner;
```

- [ ] **Step 4: Implement the submit-request builder and re-run the scoped tests**

Run:

```bash
cargo test -p execution --test negrisk_signing
cargo test -p venue-polymarket --test order_submission
```

Expected: PASS. Keep this abstraction narrow:
- the signer must produce signed payloads for tests and runtime plumbing
- this plan does **not** implement real cryptographic signing
- the venue layer only builds documented HTTP requests from already signed orders

- [ ] **Step 5: Commit**

```bash
git add crates/execution/src/signing.rs crates/execution/src/sink.rs crates/execution/src/lib.rs crates/execution/tests/negrisk_signing.rs crates/venue-polymarket/src/orders.rs crates/venue-polymarket/src/rest.rs crates/venue-polymarket/src/lib.rs crates/venue-polymarket/tests/order_submission.rs
git commit -m "feat: add neg-risk live submit plumbing"
```

### Task 5: Persist Live Attempts And Replayable `neg-risk` Artifacts

**Files:**
- Create: `migrations/0006_phase3b_negrisk_live.sql`
- Modify: `crates/persistence/src/models.rs`
- Modify: `crates/persistence/src/repos.rs`
- Modify: `crates/persistence/src/lib.rs`
- Test: `crates/persistence/tests/runtime_backbone.rs`
- Create: `crates/persistence/tests/negrisk_live.rs`
- Modify: `crates/app-replay/src/lib.rs`
- Create: `crates/app-replay/tests/negrisk_live_contract.rs`
- Modify: `crates/app-replay/tests/replay_app.rs`

- [ ] **Step 1: Write the failing persistence and replay tests**

```rust
#[tokio::test]
async fn live_neg_risk_artifacts_round_trip_with_attempt_anchor() {
    let row = LiveExecutionArtifactRow {
        attempt_id: "request-bound:family-a:attempt-1".to_owned(),
        stream: "neg-risk-live-orders".to_owned(),
        payload: serde_json::json!({ "family_id": "family-a", "member_count": 2 }),
    };

    LiveArtifactRepo.append(&pool, &row).await.unwrap();
    let loaded = LiveArtifactRepo.list_for_attempt(&pool, &row.attempt_id).await.unwrap();
    assert_eq!(loaded, vec![row]);
}
```

- [ ] **Step 2: Run the scoped persistence and replay tests to verify they fail**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test negrisk_live
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-replay --test negrisk_live_contract
```

Expected: FAIL because there is no live-artifact schema or replay helper yet.

- [ ] **Step 3: Add the new schema and repo surface**

```sql
CREATE TABLE live_execution_artifacts (
    attempt_id TEXT NOT NULL,
    stream TEXT NOT NULL,
    payload JSONB NOT NULL,
    PRIMARY KEY (attempt_id, stream),
    FOREIGN KEY (attempt_id) REFERENCES execution_attempts(attempt_id)
);
```

- [ ] **Step 4: Re-run the scoped DB-backed tests**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test runtime_backbone --test negrisk_live
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-replay --test replay_app --test negrisk_live_contract
```

Expected: PASS. Keep live artifacts physically separate from `shadow_execution_artifacts`; do **not** collapse them into one table with implicit mode filtering.

- [ ] **Step 5: Commit**

```bash
git add migrations/0006_phase3b_negrisk_live.sql crates/persistence/src/models.rs crates/persistence/src/repos.rs crates/persistence/src/lib.rs crates/persistence/tests/runtime_backbone.rs crates/persistence/tests/negrisk_live.rs crates/app-replay/src/lib.rs crates/app-replay/tests/replay_app.rs crates/app-replay/tests/negrisk_live_contract.rs
git commit -m "feat: persist neg-risk live execution artifacts"
```

### Task 6: Wire `Phase 3b` Through `app-live`, Metrics, And Docs

**Files:**
- Modify: `crates/app-live/src/runtime.rs`
- Modify: `crates/app-live/src/supervisor.rs`
- Modify: `crates/app-live/src/dispatch.rs`
- Modify: `crates/app-live/src/main.rs`
- Modify: `crates/app-live/src/lib.rs`
- Create: `crates/app-live/tests/negrisk_live_rollout.rs`
- Modify: `crates/observability/src/metrics.rs`
- Modify: `crates/observability/tests/metrics.rs`
- Modify: `README.md`

- [ ] **Step 1: Write the failing supervisor/runtime tests**

```rust
#[test]
fn live_ready_family_with_live_rule_produces_live_neg_risk_attempt_artifacts() {
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.seed_neg_risk_live_targets(sample_live_target_config("family-a"));
    supervisor.seed_activation_rule_live("family-a");
    supervisor.seed_runtime_progress(41, 7, Some("snapshot-7"));

    let summary = supervisor.resume_once().unwrap();

    assert_eq!(summary.negrisk_mode, ExecutionMode::Live);
    assert_eq!(summary.neg_risk_rollout_evidence.as_ref().unwrap().live_ready_family_count, 1);
    assert!(summary.neg_risk_live_attempt_count >= 1);
}
```

- [ ] **Step 2: Run the scoped `app-live` and observability tests to verify they fail**

Run:

```bash
cargo test -p app-live --test negrisk_live_rollout
cargo test -p observability --test metrics
```

Expected: FAIL because `app-live` still hard-codes `negrisk_mode=Shadow` and does not emit live-attempt surfaces.

- [ ] **Step 3: Implement the minimal runtime wiring**

```rust
SupervisorSummary {
    fullset_mode: ExecutionMode::Live,
    negrisk_mode: resolved_negrisk_mode,
    neg_risk_rollout_evidence: self.neg_risk_rollout_evidence.clone(),
    neg_risk_live_attempt_count: self.neg_risk_live_attempt_count,
    /* existing fields unchanged */
}
```

- [ ] **Step 4: Re-run the end-to-end scoped suite**

Run:

```bash
cargo test -p app-live --test main_entrypoint --test fault_injection --test negrisk_rollout_faults --test negrisk_live_rollout
cargo test -p observability --test metrics
```

Expected: PASS. Keep the runtime truthful:
- families without `Live` capability remain `Shadow`, `ReduceOnly`, or `RecoveryOnly`
- `neg-risk` live artifacts must not appear unless the family is both config-backed and activation-approved
- existing restart/resume and rollout-evidence anchors must stay intact

- [ ] **Step 5: Update docs and run the final verification set**

Run:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p domain --test runtime_backbone
cargo test -p app-live --test config --test main_entrypoint --test fault_injection --test negrisk_rollout_faults --test negrisk_live_rollout
cargo test -p execution --test orchestrator --test negrisk_live_planner --test negrisk_signing
cargo test -p venue-polymarket --test status_and_retry --test order_submission
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test runtime_backbone --test negrisk_live
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-replay --test replay_app --test negrisk_live_contract
cargo test -p observability --test metrics
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/app-live/src/runtime.rs crates/app-live/src/supervisor.rs crates/app-live/src/dispatch.rs crates/app-live/src/main.rs crates/app-live/src/lib.rs crates/app-live/tests/negrisk_live_rollout.rs crates/observability/src/metrics.rs crates/observability/tests/metrics.rs README.md
git commit -m "feat: wire phase3b neg-risk live backbone"
```

## Follow-On Plan

After this plan is complete, write a separate follow-on plan only if you still need:

- a real cryptographic signer implementation instead of the test/runtime abstraction added here
- market-discovered `neg-risk` pricing instead of operator-supplied family live targets
- full external-feed / relayer / heartbeat daemon wiring in `app-live`
- operator tooling or dashboard surfaces for family promotion management
