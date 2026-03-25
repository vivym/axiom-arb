# Phase 3 Neg-Risk Rollout Gates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement family-scoped `neg-risk` rollout gates, readiness evidence, and launch-criteria enforcement on top of the unified runtime backbone without inventing a new `neg-risk` pricing or planner model.

**Architecture:** This plan is intentionally `Phase 3a`, not the full `Phase 3 neg-risk Live order-placement` project. It adds explicit family readiness data, replayable capability-matrix decisions, route-mode semantics, and rollout evidence surfaces so the codebase can prove when a family is eligible to move from `Shadow` to selective `Live`. It does **not** invent a new `neg-risk` strategy pricing model or a venue-specific `neg-risk` order planner; that needs a separate follow-on plan once the rollout gates are in place.

**Tech Stack:** Rust workspace crates (`state`, `risk`, `strategy-negrisk`, `execution`, `observability`, `app-live`), Cargo tests, Clippy, existing unified-runtime spec contracts.

---

## Scope Check

The user-facing phrase "`Phase 3 neg-risk Live rollout`" naturally splits into two independent sub-projects:

1. rollout gates, readiness evidence, capability-matrix policy, and launch criteria
2. a real route-specific `neg-risk` live planner / execution model

This plan covers **(1) only**. Trying to combine both would force the implementer to invent a new `neg-risk` pricing and planner surface that the current spec explicitly leaves out of scope.

## File Map

### Existing files to modify

- `crates/state/src/snapshot.rs`
  - extend `NegRiskView` so family-level rollout readiness is explicit instead of being inferred from `family_ids` alone, while preserving enough compatibility for downstream callers to keep compiling during the shape transition
- `crates/state/src/lib.rs`
  - re-export the new rollout-readiness types
- `crates/state/tests/snapshot_publish.rs`
  - lock down publication rules for explicit family readiness payloads
- `crates/strategy-negrisk/src/intent.rs`
  - adapt the intent builder to derive stable family scopes from explicit readiness records instead of the old bare `family_ids` field
- `crates/risk/src/activation.rs`
  - stop hard-coding `phase_one_defaults` semantics as the only activation shape; delegate family-scoped rule resolution to a dedicated rollout module
- `crates/risk/src/negrisk.rs`
  - first preserve compile compatibility with the new `NegRiskView` shape, then enforce the Phase 3 mode-behavior matrix using explicit family readiness instead of the current `phase_one_effective_mode` clamp
- `crates/risk/src/lib.rs`
  - export the rollout-rule types and the new `neg-risk` readiness evaluator
- `crates/risk/tests/activation_policy.rs`
  - keep the existing `Phase 1/2` defaults green while adding family-scoped rule coverage
- `crates/risk/tests/decision_engine.rs`
  - preserve the shared input-neutral decision backbone while tightening `neg-risk` route behavior by mode
- `crates/observability/src/metrics.rs`
  - add runtime metrics for rollout-gate counts and parity/drift evidence
- `crates/observability/src/lib.rs`
  - re-export any new recorder helpers
- `crates/observability/tests/metrics.rs`
  - verify the new rollout-gate metric keys and registry recording behavior
- `crates/execution/tests/orchestrator.rs`
  - extend parity tests so `Shadow` and `Live` remain identical before the final sink for the same synthetic request/plan, while keeping the execution surface route-agnostic until `Phase 3b` introduces real `neg-risk` planner/request metadata
- `crates/app-live/src/supervisor.rs`
  - store and restore durable rollout-evidence anchors in the supervisor seed/summary path so restart behavior is backed by production state, not test-only fixtures
- `crates/app-live/tests/fault_injection.rs`
  - add restart-path tests proving rollout-gate evidence must be durable and cannot silently default to a healthy/live-eligible state
- `README.md`
  - document that this plan adds rollout gates and evidence only; it does not yet add a new `neg-risk` live planner

### New files to create

- `crates/risk/src/rollout.rs`
  - hold the family-scoped capability-matrix rule types and activation matching logic
- `crates/risk/tests/rollout_matrix.rs`
  - focused tests for matched-rule precedence, scope fallback, and replay anchors
- `crates/risk/tests/negrisk_rollout.rs`
  - focused tests for `Shadow` / `Live` / `ReduceOnly` / `RecoveryOnly` family behavior
- `crates/app-live/tests/negrisk_rollout_faults.rs`
  - focused restart and missing-evidence tests for rollout-gate durability

## Implementation Notes

- Reuse the existing domain `ActivationDecision { mode, scope, reason, policy_version, matched_rule_id }`; do **not** invent a second activation result type.
- Keep the existing `Phase 1/2` defaults available via `ActivationPolicy::phase_one_defaults()` so the current branch stays backwards-compatible.
- Do **not** create a new `neg-risk` order planner enum variant in this plan. The rollout gates must be implementable and testable without inventing new pricing semantics.
- Family readiness should be explicit and machine-readable. Do not collapse it into one opaque `bool`.
- `Phase 3a` does **not** add a new runtime decision loop or a route-specific execution request shape. `ActivationPolicy::activation_for(...)` is a contract surface verified in `risk`, while execution parity stays route-agnostic until `Phase 3b`.
- Do not land a transitional commit that removes `NegRiskView.family_ids` but leaves downstream production callers broken. Task 1 must either adapt those callers in the same commit or provide a compatibility helper/accessor that preserves compilation until Task 3.
- Tests must drive every new behavior before implementation (`@superpowers:test-driven-development`).

### Task 1: Add Explicit Family Rollout Readiness To `NegRiskView`

**Files:**
- Modify: `crates/state/src/snapshot.rs`
- Modify: `crates/state/src/lib.rs`
- Modify: `crates/strategy-negrisk/src/intent.rs`
- Modify: `crates/risk/src/negrisk.rs`
- Test: `crates/state/tests/snapshot_publish.rs`
- Test: `crates/strategy-negrisk/tests/intent.rs`

- [ ] **Step 1: Write the failing snapshot test**

```rust
#[test]
fn published_snapshot_exposes_family_level_rollout_readiness() {
    let snapshot = PublishedSnapshot {
        snapshot_id: "snapshot-12".to_owned(),
        state_version: 12,
        committed_journal_seq: 44,
        fullset_ready: true,
        negrisk_ready: true,
        fullset: None,
        negrisk: Some(NegRiskView {
            snapshot_id: "snapshot-12".to_owned(),
            state_version: 12,
            families: vec![NegRiskFamilyRolloutReadiness {
                family_id: "family-a".to_owned(),
                shadow_parity_ready: true,
                recovery_ready: true,
                replay_drift_ready: false,
                fault_injection_ready: true,
                conversion_path_ready: true,
                halt_semantics_ready: true,
            }],
        }),
    };

    assert!(!snapshot.negrisk.as_ref().unwrap().families[0].replay_drift_ready);
}
```

- [ ] **Step 2: Run the state test to verify it fails**

Run: `cargo test -p state published_snapshot_exposes_family_level_rollout_readiness -- --exact`

Expected: FAIL because `NegRiskView` does not yet expose family readiness records.

- [ ] **Step 3: Implement the minimal snapshot shape**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegRiskFamilyRolloutReadiness {
    pub family_id: String,
    pub shadow_parity_ready: bool,
    pub recovery_ready: bool,
    pub replay_drift_ready: bool,
    pub fault_injection_ready: bool,
    pub conversion_path_ready: bool,
    pub halt_semantics_ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegRiskView {
    pub snapshot_id: String,
    pub state_version: u64,
    pub families: Vec<NegRiskFamilyRolloutReadiness>,
}
```

- [ ] **Step 4: Update downstream callers, then rerun the scoped tests**

Run:

```bash
cargo test -p state --test snapshot_publish
cargo test -p strategy-negrisk --test intent
cargo test -p risk --test decision_engine --no-run
```

Expected: PASS after the `NegRiskView` helpers, `strategy-negrisk` intent builder, and `risk` compile path derive family scopes from the new `families` vector rather than the old bare list. Do **not** implement `Live` readiness logic in this task; Task 3 owns the mode-behavior semantics.

- [ ] **Step 5: Commit**

```bash
git add crates/state/src/snapshot.rs crates/state/src/lib.rs crates/strategy-negrisk/src/intent.rs crates/risk/src/negrisk.rs crates/state/tests/snapshot_publish.rs crates/strategy-negrisk/tests/intent.rs
git commit -m "feat: add neg-risk family rollout readiness snapshot shape"
```

### Task 2: Add Family-Scoped Capability Matrix Rules

**Files:**
- Create: `crates/risk/src/rollout.rs`
- Modify: `crates/risk/src/activation.rs`
- Modify: `crates/risk/src/lib.rs`
- Test: `crates/risk/tests/activation_policy.rs`
- Test: `crates/risk/tests/rollout_matrix.rs`

- [ ] **Step 1: Write the failing activation tests**

```rust
#[test]
fn family_specific_live_rule_overrides_default_shadow_rule() {
    let policy = ActivationPolicy::from_rules(
        "phase-three-rules",
        vec![
            RolloutRule::new("neg-risk", "default", ExecutionMode::Shadow, "default-shadow"),
            RolloutRule::new("neg-risk", "family-a", ExecutionMode::Live, "family-a-live"),
        ],
    );

    let activation = policy.activation_for("neg-risk", "family-a", "snapshot-12");
    assert_eq!(activation.mode, ExecutionMode::Live);
    assert_eq!(activation.matched_rule_id.as_deref(), Some("family-a-live"));
}
```

- [ ] **Step 2: Run the activation tests to verify they fail**

Run:

```bash
cargo test -p risk family_specific_live_rule_overrides_default_shadow_rule -- --exact
cargo test -p risk --test activation_policy
```

Expected: FAIL because `ActivationPolicy` only supports the phase-one builder and `mode_for_route`.

- [ ] **Step 3: Implement the rollout-rule module and activation helper**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolloutRule {
    pub route: String,
    pub scope: String,
    pub mode: ExecutionMode,
    pub rule_id: String,
}

impl ActivationPolicy {
    pub fn from_rules(policy_version: impl Into<String>, rules: Vec<RolloutRule>) -> Self { /* ... */ }

    pub fn activation_for(
        &self,
        route: &str,
        scope: &str,
        snapshot_id: &str,
    ) -> ActivationDecision { /* ... */ }
}
```

- [ ] **Step 4: Re-run the rollout tests and keep the old defaults green**

Run:

```bash
cargo test -p risk --test activation_policy
cargo test -p risk --test rollout_matrix
```

Expected: PASS, including the existing `phase_one_defaults()` tests.

This task is intentionally library-layer only. Do **not** add a new `app-live` caller here; the runtime wiring in this plan starts with durable evidence surfaces in Task 4.

- [ ] **Step 5: Commit**

```bash
git add crates/risk/src/rollout.rs crates/risk/src/activation.rs crates/risk/src/lib.rs crates/risk/tests/activation_policy.rs crates/risk/tests/rollout_matrix.rs
git commit -m "feat: add family-scoped neg-risk rollout rules"
```

### Task 3: Enforce The Phase 3 `neg-risk` Mode Behavior Matrix

**Files:**
- Modify: `crates/risk/src/negrisk.rs`
- Modify: `crates/risk/src/engine.rs`
- Test: `crates/risk/tests/decision_engine.rs`
- Test: `crates/risk/tests/negrisk_rollout.rs`

- [ ] **Step 1: Write the failing `neg-risk` rollout-gate tests**

```rust
#[test]
fn live_mode_requires_all_family_readiness_gates() {
    let view = sample_negrisk_view_with_family(
        "family-a",
        /* shadow_parity_ready = */ true,
        /* recovery_ready = */ true,
        /* replay_drift_ready = */ false,
        /* fault_injection_ready = */ true,
        /* conversion_path_ready = */ true,
        /* halt_semantics_ready = */ true,
    );

    assert_eq!(
        risk::negrisk::evaluate_negrisk_family(&view, "family-a", ExecutionMode::Live),
        DecisionVerdict::Rejected
    );
}
```

- [ ] **Step 2: Run the scoped tests to verify they fail**

Run:

```bash
cargo test -p risk live_mode_requires_all_family_readiness_gates -- --exact
cargo test -p risk --test decision_engine
```

Expected: FAIL because current `neg-risk` evaluation only distinguishes `Shadow` vs everything else.

- [ ] **Step 3: Implement the explicit family readiness evaluator**

```rust
fn live_ready(family: &NegRiskFamilyRolloutReadiness) -> bool {
    family.shadow_parity_ready
        && family.recovery_ready
        && family.replay_drift_ready
        && family.fault_injection_ready
        && family.conversion_path_ready
        && family.halt_semantics_ready
}

pub fn evaluate_negrisk_family(
    view: &NegRiskView,
    family_id: &str,
    mode: ExecutionMode,
) -> DecisionVerdict { /* ... */ }
```

- [ ] **Step 4: Re-run the risk tests**

Run:

```bash
cargo test -p risk --test decision_engine
cargo test -p risk --test negrisk_rollout
```

Expected: PASS, with these explicit contracts locked in:
- `Shadow` may proceed with a published family record
- `Live` requires all readiness gates
- `ReduceOnly` rejects strategy expansion but keeps recovery inputs available
- `RecoveryOnly` suppresses strategy inputs and allows recovery inputs only

- [ ] **Step 5: Commit**

```bash
git add crates/risk/src/negrisk.rs crates/risk/src/engine.rs crates/risk/tests/decision_engine.rs crates/risk/tests/negrisk_rollout.rs
git commit -m "feat: enforce phase3 neg-risk mode behavior gates"
```

### Task 4: Add Rollout Evidence Surfaces And Parity Contracts

**Files:**
- Modify: `crates/observability/src/metrics.rs`
- Modify: `crates/observability/src/lib.rs`
- Modify: `crates/observability/tests/metrics.rs`
- Modify: `crates/execution/tests/orchestrator.rs`
- Modify: `crates/app-live/src/supervisor.rs`
- Modify: `crates/app-live/tests/fault_injection.rs`
- Create: `crates/app-live/tests/negrisk_rollout_faults.rs`

- [ ] **Step 1: Write the failing evidence tests**

```rust
#[test]
fn runtime_metrics_expose_neg_risk_rollout_gate_counts() {
    let metrics = RuntimeMetrics::default();
    assert_eq!(
        metrics.neg_risk_live_ready_family_count.key(),
        MetricKey::new("axiom_neg_risk_live_ready_family_count")
    );
    assert_eq!(
        metrics.neg_risk_live_gate_block_count.key(),
        MetricKey::new("axiom_neg_risk_live_gate_block_count")
    );
}
```

```rust
#[test]
fn restart_requires_durable_rollout_gate_evidence_before_live_promotion() {
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.seed_runtime_progress(42, 6, Some("snapshot-6"));
    supervisor.seed_committed_state_version(6);
    // no durable rollout evidence seed
    let err = supervisor.resume_once().unwrap_err();
    assert!(err.to_string().contains("rollout gate evidence"));
}
```

- [ ] **Step 2: Run the evidence tests to verify they fail**

Run:

```bash
cargo test -p observability runtime_metrics_expose_neg_risk_rollout_gate_counts -- --exact
cargo test -p app-live restart_requires_durable_rollout_gate_evidence_before_live_promotion -- --exact
```

Expected: FAIL because neither the metrics nor the rollout-fault contract exists yet.

- [ ] **Step 3: Implement the minimal evidence surface**

```rust
pub struct RuntimeMetrics {
    pub neg_risk_live_ready_family_count: GaugeHandle,
    pub neg_risk_live_gate_block_count: GaugeHandle,
    pub neg_risk_rollout_parity_mismatch_count: CounterHandle,
    // existing fields...
}
```

```rust
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NegRiskRolloutEvidence {
    pub snapshot_id: String,
    pub live_ready_family_count: usize,
    pub blocked_family_count: usize,
    pub parity_mismatch_count: u64,
}
```

Add a durable rollout-evidence anchor to `AppSupervisor` seed/summary plumbing so `resume_once()` can reject `Live`-eligible restart paths when the evidence is missing or mismatched. This production state belongs in `crates/app-live/src/supervisor.rs`, not only in tests.

Update the execution parity test so the same synthetic request and plan are exercised through `LiveVenueSink::noop()` and `ShadowVenueSink::noop()` with identical pre-sink artifacts. Keep the test route-agnostic in `Phase 3a`; do **not** add fake family metadata, fake fills, or fake settlement effects before `Phase 3b` introduces a real `neg-risk` planner/request surface.

Rollout evidence added in this task must remain stable across restart and replay. Missing evidence must block `Live` promotion rather than silently defaulting to a healthy/live-eligible state.

- [ ] **Step 4: Re-run the scoped tests**

Run:

```bash
cargo test -p observability --test metrics
cargo test -p execution --test orchestrator
cargo test -p app-live --test fault_injection
cargo test -p app-live --test negrisk_rollout_faults
```

Expected: PASS, with `app-live` proving that rollout evidence survives restart as a durable supervisor anchor rather than as an in-memory default.

- [ ] **Step 5: Commit**

```bash
git add crates/observability/src/metrics.rs crates/observability/src/lib.rs crates/observability/tests/metrics.rs crates/execution/tests/orchestrator.rs crates/app-live/src/supervisor.rs crates/app-live/tests/fault_injection.rs crates/app-live/tests/negrisk_rollout_faults.rs
git commit -m "feat: add neg-risk rollout evidence surfaces"
```

### Task 5: Update Docs And Run The Verification Suite

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Update the README first**

Document these exact points:

- `Phase 3a` in this repository means rollout gates and readiness evidence only
- it does **not** add a new `neg-risk` pricing or live planner surface
- families may remain in `Disabled`, `Shadow`, `ReduceOnly`, or `RecoveryOnly`
- actual family promotion to `Live` needs this plan plus a later route-planner plan

- [ ] **Step 2: Run formatting**

Run: `cargo fmt --all`

Expected: PASS

- [ ] **Step 3: Run lints**

Run: `cargo clippy --workspace --all-targets -- -D warnings`

Expected: PASS

- [ ] **Step 4: Run the targeted rollout-gate suite**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p state --test snapshot_publish
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p strategy-negrisk --test intent
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p risk --test activation_policy
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p risk --test rollout_matrix
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p risk --test decision_engine
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p risk --test negrisk_rollout
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p execution --test orchestrator
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test fault_injection
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test negrisk_rollout_faults
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p observability --test metrics
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add README.md
git commit -m "docs: close out phase3 neg-risk rollout gates"
```

## Follow-On Plan

After this plan is complete, write a separate plan for the missing `Phase 3b` work:

- a real route-specific `neg-risk` planner
- venue-facing `neg-risk` live execution semantics
- family-level `Live` submission using the rollout gates added here
- replayable decision and attempt journaling for real `neg-risk` live orders

## Plan Review Notes

Review this plan against:

- `docs/superpowers/specs/2026-03-24-axiomarb-unified-runtime-v1a-v1b-design.md`
