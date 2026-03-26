# Phase 3c Neg-Risk Live Submit Closure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the real `neg-risk` live submit loop on the unified runtime for `bootstrap + resume`, including real signing, real venue submission, authoritative reconcile facts, durable recovery anchors, and replay-safe restart behavior.

**Architecture:** Build directly on the merged `Phase 3b` live backbone instead of inventing another runtime. This plan keeps `operator-supplied family live targets` as the only decision input, preserves the existing `ActivationPolicy -> Risk -> Planner -> Attempt` chain, and adds signer / submit / reconcile provider contracts whose results re-enter the authoritative `ExternalFactEvent -> journal -> StateApplier -> snapshot` path. It intentionally stops short of a continuous daemon and intentionally does not add market-discovered pricing.

**Tech Stack:** Rust workspace crates (`domain`, `execution`, `app-live`, `persistence`, `app-replay`, `venue-polymarket`, `observability`), SQL migrations, Cargo tests, Clippy, unified-runtime contracts, existing `Phase 3b` neg-risk live surfaces.

---

## Scope Check

This plan covers one coherent sub-project:

1. real signer / submit / reconcile contracts for `neg-risk` live work
2. durable persistence and replay anchors for live closure
3. `bootstrap + resume` wiring in `app-live` that performs real submit closure without creating a new runtime path

This plan deliberately does **not** cover three follow-on concerns:

1. market-discovered `neg-risk` pricing or live-target generation
2. full websocket / heartbeat / relayer-poll / continuous dispatch daemonization
3. remote signer / custody / HSM integrations beyond a local concrete signer implementation behind a provider boundary

Keep the phase narrow:

- continue to use explicit operator-supplied family live targets
- continue to treat `Phase 3c` as `bootstrap + resume`, not a production daemon
- keep `Shadow` / `Live` parity up to the final provider boundary
- do not let concrete venue clients leak into `domain`, `risk`, or strategy crates

## File Map

### Existing files to modify

- `crates/domain/src/facts.rs`
  - extend `ExternalFactEvent` so live submit and live reconcile outcomes can be expressed as repo-owned, versioned facts instead of free-form metadata-only markers
- `crates/domain/src/execution.rs`
  - add live submit / reconcile reference contracts while keeping `ExecutionReceipt` source-compatible until the sink task lands
- `crates/domain/src/lib.rs`
  - re-export any new fact / execution contract types used outside `domain`
- `crates/domain/tests/runtime_backbone.rs`
  - lock down the new live-submit and live-reconcile fact contracts before touching execution or persistence
- `crates/execution/src/signing.rs`
  - replace the Phase 3b test-only narrow signer surface with a provider-shaped interface that can support a real local signer and still keep deterministic test doubles
- `crates/execution/src/sink.rs`
  - stop using the signed-family hook as the terminal live behavior; delegate real live submit to a provider result that can return accepted, rejected, or ambiguous outcomes
- `crates/execution/src/orchestrator.rs`
  - preserve unified orchestration while threading provider outcomes and new receipt semantics through request-bound attempts
- `crates/execution/src/lib.rs`
  - export the new provider contracts and any renamed signer types
- `crates/execution/tests/negrisk_signing.rs`
  - keep signer coverage green after introducing the provider abstraction
- `crates/execution/tests/retry_and_redeem.rs`
  - keep retry / redeem semantics green after live submit provider outcomes stop being a simple success-only surface
- `crates/execution/tests/orchestrator.rs`
  - keep shadow/live parity and mode-precedence guarantees green after live submit outcomes stop being a hard-coded `Succeeded`
- `crates/app-live/src/config.rs`
  - parse the minimal local signer configuration needed for `Phase 3c` live submit
- `crates/app-live/Cargo.toml`
  - add the `recovery` crate dependency if it is not already wired into `app-live`
- `crates/app-live/src/main.rs`
  - fail fast in live mode when required signer/provider configuration is missing or invalid
- `crates/app-live/src/negrisk_live.rs`
  - stop treating live promotion as an artifact-only harness path; route it through real provider-backed submit closure
- `crates/app-live/src/runtime.rs`
  - accept provider-generated fact events, append them to the input path, and keep `bootstrap + resume` semantics coherent
- `crates/app-live/src/supervisor.rs`
  - restore durable submission / reconcile anchors on resume, prioritize recovery over fresh live promotion, and stop fabricating live truth from env-only state
- `crates/app-live/src/lib.rs`
  - export any new config or summary types
- `crates/recovery/src/coordinator.rs`
  - extend the existing recovery coordinator only as needed to turn failed-ambiguous live submit outcomes into repo-owned recovery outputs rather than ad hoc local gating
- `crates/recovery/src/lib.rs`
  - re-export any new coordinator output types required by `app-live`
- `crates/app-live/tests/config.rs`
  - verify signer config parsing
- `crates/app-live/tests/bootstrap_modes.rs`
  - verify bootstrap mode gating when live submit closure is enabled under controlled startup
- `crates/app-live/tests/main_entrypoint.rs`
  - verify live-mode entrypoint gating around signer/provider config
- `crates/app-live/tests/negrisk_live_rollout.rs`
  - evolve Phase 3b rollout tests into true submit-closure tests
- `crates/app-live/tests/negrisk_rollout_faults.rs`
  - verify family rollout evidence and live-target mismatches still fail closed once real submit closure is present
- `crates/app-live/tests/unified_supervisor.rs`
  - keep the unified supervisor summary and resume contracts green after real submit closure lands
- `crates/app-live/tests/runtime_observability.rs`
  - keep runtime observability contracts green after live submit closure adds real submit / reconcile metrics
- `crates/app-live/tests/supervisor_observability.rs`
  - keep supervisor observability summaries green after resume starts restoring real submit / reconcile truth
- `crates/app-live/tests/fault_injection.rs`
  - verify restart / resume fail-closed behavior around missing durable live anchors
- `crates/state/src/facts.rs`
  - map repo-owned live submit / reconcile facts into state-owned fact adapters without bypassing `StateApplier`
- `crates/state/src/apply.rs`
  - apply live submit / reconcile facts into authoritative runtime state, including uncertainty markers derived from ambiguous submit/reconcile outcomes
- `crates/state/src/reconcile.rs`
  - rehydrate pending reconcile truth into reconcile-first runtime state during bootstrap / resume
- `crates/state/src/snapshot.rs`
  - surface any additional authoritative snapshot fields needed to expose live submit / reconcile state safely
- `crates/state/src/lib.rs`
  - re-export any new state helpers needed by `app-live`
- `crates/state/tests/event_apply_contract.rs`
  - verify live submit / reconcile facts enter authoritative state only through the apply path
- `crates/state/tests/restore_anchor.rs`
  - verify durable pending reconcile anchors restore the same reconcile-first state after restart
- `crates/state/tests/bootstrap_reconcile.rs`
  - verify bootstrap blocks fresh live promotion while restored reconcile work is still unresolved
- `crates/persistence/src/models.rs`
  - add rows for durable live submission records and any enriched pending-reconcile payload anchors
- `crates/persistence/src/repos.rs`
  - persist and load live submission records and enriched pending reconcile rows
- `crates/persistence/src/lib.rs`
  - re-export new repo helpers
- `crates/persistence/tests/negrisk_live.rs`
  - lock down durable live submission / reconcile round trips
- `crates/persistence/tests/migrations.rs`
  - verify schema upgrades keep live submission anchors durable and fail closed when required rows are absent
- `crates/persistence/tests/runtime_backbone.rs`
  - keep generic runtime persistence guarantees green after new anchors are introduced
- `crates/app-replay/src/lib.rs`
  - expose a replay helper that can load live attempts together with submission records and reconcile anchors
- `crates/app-replay/tests/replay_app.rs`
  - keep top-level replay entrypoints green after live submission records become part of restart truth
- `crates/app-replay/tests/negrisk_live_contract.rs`
  - verify replay visibility into live submit closure rather than artifacts alone
- `crates/venue-polymarket/src/orders.rs`
  - reuse or refine request-body builders for real submit provider usage
- `crates/venue-polymarket/src/rest.rs`
  - host the actual submit call boundary used by the concrete provider
- `crates/venue-polymarket/src/relayer.rs`
  - provide the relayer-side status/reconcile information needed for unresolved live attempts
- `crates/venue-polymarket/src/lib.rs`
  - export the new concrete provider types
- `crates/venue-polymarket/tests/order_submission.rs`
  - keep transport-body contracts green while adding provider-facing coverage
- `crates/venue-polymarket/tests/status_and_retry.rs`
  - cover the reconcile-side HTTP or relayer status shape used by the provider
- `crates/observability/src/metrics.rs`
  - add counters/gauges for live submit accepted/ambiguous/reconcile backlog if they are missing
- `crates/observability/tests/metrics.rs`
  - verify any new metric keys introduced by the submit-closure path
- `crates/recovery/tests/recovery_contract.rs`
  - verify failed-ambiguous live submit outcomes become recovery or pending-reconcile work via the existing coordinator contracts
- `README.md`
  - update the repository status so `Phase 3c` is clearly distinguished from `Phase 3b`

### New files to create

- `crates/execution/src/providers.rs`
  - define repo-owned provider contracts and outcomes for signer, live submit, and reconcile
- `crates/execution/tests/negrisk_live_submit.rs`
  - focused execution tests for accepted / rejected / ambiguous provider outcomes
- `crates/venue-polymarket/src/negrisk_live.rs`
  - concrete `Polymarket` implementations of the submit and reconcile providers used in `Phase 3c`
- `crates/venue-polymarket/tests/negrisk_live_provider.rs`
  - focused provider tests for submit normalization and reconcile normalization
- `crates/app-live/tests/negrisk_live_submit_resume.rs`
  - focused supervisor/runtime tests for real submit closure and resume-first recovery behavior
- `migrations/0009_phase3c_negrisk_live_submit_closure.sql`
  - schema for live submission records and any required reconcile-anchor enrichment

## Implementation Notes

- Reuse the existing authority chain: `durable overlays + family halt + recovery scope lock + rollout capability -> ActivationPolicy -> Risk -> Planner -> Attempt -> Submit/Reconcile`.
- Do **not** let `Risk`, the concrete venue provider, or `app-live` invent a second execution-mode authority.
- Treat provider output as fact generation, not as state mutation. The only authoritative state mutation path remains `ExternalFactEvent -> journal append -> StateApplier -> snapshot publish`.
- `Phase 3c` is allowed to use a local signer implementation, but the interface must remain provider-shaped from day one.
- Resume must be fail-closed. Missing or inconsistent live submission anchors must raise an error rather than silently rebuilding from env or in-memory state.
- Keep `Shadow` and `Live` identical through planning, attempt creation, journaling identity, and metrics as far as the codebase already allows. The meaningful fork is the real provider submit boundary.
- For durable restart truth in `Phase 3c`, treat `live_submission_records` plus enriched `pending_reconcile` rows as the only new persisted anchors. Do **not** add a second table for uncertainty or recovery locks in this phase; instead, rehydrate `StateConfidence::Uncertain` and any recovery-first overlays from those durable pending-reconcile anchors during resume.
- Keep intermediate commits buildable. In particular:
  - Task 1 must not force `sink.rs` or `app-live/src/negrisk_live.rs` changes just to compile
  - Task 2 must preserve compatibility shims for the existing Phase 3b caller surface until Task 5 rewires `app-live`
- Tests must drive every new behavior before implementation (`@superpowers:test-driven-development`).

### Task 1: Add Repo-Owned Live Submit And Reconcile Contracts

**Files:**
- Modify: `crates/domain/src/facts.rs`
- Modify: `crates/domain/src/execution.rs`
- Modify: `crates/domain/src/lib.rs`
- Create: `crates/execution/src/providers.rs`
- Modify: `crates/execution/src/lib.rs`
- Test: `crates/domain/tests/runtime_backbone.rs`
- Test: `crates/execution/tests/negrisk_live_submit.rs`

- [ ] **Step 1: Write the failing contract tests**

```rust
#[test]
fn external_fact_event_can_carry_negrisk_live_submit_fact() {
    let fact = ExternalFactEvent::negrisk_live_submit_observed(
        "session-live",
        "evt-1",
        "attempt-family-a-1",
        "family-a",
        "submission-family-a-1",
    );

    assert_eq!(fact.source_kind, "negrisk_live_submit");
    assert_eq!(fact.payload.kind(), "negrisk_live_submit_observed");
}

#[test]
fn live_submit_outcome_distinguishes_accepted_and_ambiguous() {
    let accepted = LiveSubmitOutcome::Accepted {
        submission_record: sample_submission_record("attempt-1"),
    };
    let ambiguous = LiveSubmitOutcome::Ambiguous {
        pending_ref: "pending-attempt-1".to_owned(),
        reason: "timeout".to_owned(),
    };

    assert!(accepted.is_accepted());
    assert!(ambiguous.is_ambiguous());
}
```

- [ ] **Step 2: Run the focused tests and verify they fail**

Run:

```bash
cargo test -p domain --test runtime_backbone
cargo test -p execution --test negrisk_live_submit
```

Expected:

- `domain` fails because `ExternalFactEvent` does not yet carry versioned live-submit/reconcile payloads
- `execution` fails because provider contracts do not exist yet

- [ ] **Step 3: Implement the minimal contracts**

Add repo-owned contracts, keeping them narrow:

```rust
pub enum ExternalFactPayload {
    NegRiskLiveSubmitObserved { attempt_id: String, scope: String, submission_ref: String },
    NegRiskLiveReconcileObserved { pending_ref: String, scope: String, terminal: bool },
}

pub struct LiveSubmissionRecord {
    pub submission_ref: String,
    pub attempt_id: String,
    pub route: String,
    pub scope: String,
    pub provider: String,
}

pub enum LiveSubmitOutcome {
    Accepted { submission_record: LiveSubmissionRecord },
    RejectedDefinitive { reason: String },
    AcceptedButUnconfirmed { submission_record: LiveSubmissionRecord },
    Ambiguous { pending_ref: String, reason: String },
}

pub struct PendingReconcileWork {
    pub pending_ref: String,
    pub route: String,
    pub scope: String,
}

pub enum ReconcileOutcome {
    ConfirmedAuthoritative { submission_ref: String },
    StillPending,
    NeedsRecovery { pending_ref: String, reason: String },
    FailedAmbiguous { pending_ref: String, reason: String },
    FailedDefinitive { reason: String },
}
```

Keep this task source-compatible:

- do **not** require changes in `crates/execution/src/sink.rs` yet
- do **not** change the `ExecutionReceipt` constructor shape in a way that breaks existing call sites
- if `ExecutionReceipt` needs new fields later, add them in Task 2 together with the sink implementation

- [ ] **Step 4: Re-run the focused contract tests**

Run:

```bash
cargo test -p domain --test runtime_backbone
cargo test -p execution --test negrisk_live_submit
```

Expected: PASS. Do **not** add concrete venue logic in this task.

- [ ] **Step 5: Commit**

```bash
git add crates/domain/src/facts.rs crates/domain/src/execution.rs crates/domain/src/lib.rs crates/domain/tests/runtime_backbone.rs crates/execution/src/providers.rs crates/execution/src/lib.rs crates/execution/tests/negrisk_live_submit.rs
git commit -m "feat: add phase3c live submit contracts"
```

### Task 2: Replace Hook-Only Live Sink Behavior With Provider-Backed Submit Semantics

**Files:**
- Modify: `crates/domain/src/execution.rs`
- Modify: `crates/execution/src/providers.rs`
- Modify: `crates/execution/src/signing.rs`
- Modify: `crates/execution/src/sink.rs`
- Modify: `crates/execution/src/orchestrator.rs`
- Modify: `crates/execution/src/lib.rs`
- Modify: `crates/domain/tests/runtime_backbone.rs`
- Test: `crates/execution/tests/negrisk_signing.rs`
- Test: `crates/execution/tests/retry_and_redeem.rs`
- Test: `crates/execution/tests/negrisk_live_submit.rs`
- Test: `crates/execution/tests/orchestrator.rs`

- [ ] **Step 1: Write the failing execution tests**

```rust
#[test]
fn live_sink_returns_accepted_but_unconfirmed_when_provider_accepts_submit() {
    let sink = LiveVenueSink::with_submit_provider(
        Arc::new(TestOrderSigner),
        Arc::new(FakeSubmitProvider::accepted("submission-a")),
    );

    let receipt = sink.execute(&sample_negrisk_plan(), &sample_attempt_context()).unwrap();

    assert_eq!(receipt.outcome, ExecutionAttemptOutcome::Succeeded);
    assert_eq!(receipt.submission_ref.as_deref(), Some("submission-a"));
}

#[test]
fn live_sink_reports_failed_ambiguous_when_submit_provider_times_out() {
    let sink = LiveVenueSink::with_submit_provider(
        Arc::new(TestOrderSigner),
        Arc::new(FakeSubmitProvider::ambiguous("pending-a")),
    );

    let receipt = sink.execute(&sample_negrisk_plan(), &sample_attempt_context()).unwrap();

    assert_eq!(receipt.outcome, ExecutionAttemptOutcome::FailedAmbiguous);
    assert_eq!(receipt.pending_ref.as_deref(), Some("pending-a"));
}
```

- [ ] **Step 2: Run the focused execution tests and verify they fail**

Run:

```bash
cargo test -p execution --test negrisk_signing --test negrisk_live_submit --test orchestrator --test retry_and_redeem
```

Expected: FAIL because the live sink still uses the Phase 3b hook model and `ExecutionReceipt` cannot carry provider-derived anchors.

- [ ] **Step 3: Implement the minimal provider-backed live sink**

Key changes:

```rust
pub trait SignerProvider: Send + Sync {
    fn sign_family(&self, plan: &ExecutionPlan) -> Result<SignedFamilySubmission, SigningError>;
}

pub trait VenueExecutionProvider: Send + Sync {
    fn submit_family(
        &self,
        signed: &SignedFamilySubmission,
        attempt: &ExecutionAttemptContext,
    ) -> Result<LiveSubmitOutcome, SubmitProviderError>;
}
```

And in `LiveVenueSink`, map provider outcomes into receipt anchors without mutating state directly.

Compatibility requirement for this task:

- keep a compatibility constructor such as `with_order_signer_and_hook(...)` or an equivalent adapter in place so [`crates/app-live/src/negrisk_live.rs`](/Users/viv/projs/axiom-arb/crates/app-live/src/negrisk_live.rs) still compiles unchanged until Task 5
- keep the deterministic test signer surface usable by existing `Phase 3b` tests until the real app wiring is updated
- because `ExecutionReceipt` gains receipt-anchor fields in this task, keep [`crates/domain/tests/runtime_backbone.rs`](/Users/viv/projs/axiom-arb/crates/domain/tests/runtime_backbone.rs) buildable by switching it to constructor-based receipt creation instead of stale struct literals

- [ ] **Step 4: Re-run the focused execution tests**

Run:

```bash
cargo test -p execution --test negrisk_signing --test negrisk_live_submit --test orchestrator --test retry_and_redeem
```

Expected: PASS. Keep `ShadowVenueSink` behavior unchanged except for any necessary receipt-shape updates.

- [ ] **Step 5: Commit**

```bash
git add crates/domain/src/execution.rs crates/domain/tests/runtime_backbone.rs crates/execution/src/providers.rs crates/execution/src/signing.rs crates/execution/src/sink.rs crates/execution/src/orchestrator.rs crates/execution/src/lib.rs crates/execution/tests/negrisk_signing.rs crates/execution/tests/retry_and_redeem.rs crates/execution/tests/negrisk_live_submit.rs crates/execution/tests/orchestrator.rs
git commit -m "feat: route live sink through phase3c providers"
```

### Task 3: Persist And Replay Live Submission Records And Reconcile Anchors

**Files:**
- Create: `migrations/0009_phase3c_negrisk_live_submit_closure.sql`
- Modify: `crates/persistence/src/models.rs`
- Modify: `crates/persistence/src/repos.rs`
- Modify: `crates/persistence/src/lib.rs`
- Test: `crates/persistence/tests/negrisk_live.rs`
- Test: `crates/persistence/tests/migrations.rs`
- Test: `crates/persistence/tests/runtime_backbone.rs`
- Modify: `crates/app-replay/src/lib.rs`
- Test: `crates/app-replay/tests/negrisk_live_contract.rs`

- [ ] **Step 1: Write the failing persistence and replay tests**

```rust
#[tokio::test]
async fn live_submission_record_round_trips_with_attempt_anchor() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    ExecutionAttemptRepo.append(&db.pool, &sample_attempt("attempt-live-1")).await.unwrap();
    LiveSubmissionRepo
        .append(&db.pool, &sample_submission_row("attempt-live-1", "submission-live-1"))
        .await
        .unwrap();

    let rows = LiveSubmissionRepo.list_for_attempt(&db.pool, "attempt-live-1").await.unwrap();
    assert_eq!(rows.len(), 1);
}
```

Add two more failing cases in this step:

- a migration regression that upgrades a pre-`0009` schema, preserves any existing live execution artifacts, and verifies the new `live_submission_records` plus enriched `pending_reconcile` anchors can coexist without weakening fail-closed resume behavior
- a persistence round-trip that proves a pending-reconcile row can carry the durable `submission_ref` / `family_id` / `route` / `reason` payload needed for Task 5 to rehydrate `Uncertain` state and recovery-first overlays without inventing a second persistence table

- [ ] **Step 2: Run the persistence and replay tests and verify they fail**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test negrisk_live --test runtime_backbone
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test migrations
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-replay --test negrisk_live_contract
```

Expected: FAIL because the schema and repos do not yet persist live submission records or replay them.

- [ ] **Step 3: Implement schema and repo support**

Add only the durable anchors required for `Phase 3c`:

```sql
CREATE TABLE live_submission_records (
  submission_ref TEXT PRIMARY KEY,
  attempt_id TEXT NOT NULL REFERENCES execution_attempts(attempt_id),
  route TEXT NOT NULL,
  scope TEXT NOT NULL,
  provider TEXT NOT NULL,
  state TEXT NOT NULL,
  payload JSONB NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

Add repo helpers for:

- append and list live submission records
- append and list enriched pending reconcile rows carrying the `submission_ref`, family scope, and reconcile reason required for restart truth
- replay loading of attempts + submission records + live artifacts

Explicit persistence boundary for this task:

- `Phase 3c` does **not** add a separate durable table for uncertainty markers or recovery locks
- the durable resume truth is `execution_attempts + live_submission_records + enriched pending_reconcile rows`
- Task 5 must reconstruct `StateConfidence::Uncertain` and any recovery-first overlays from those durable pending-reconcile anchors, so Task 3 has to store enough information to do that deterministically

Carry forward the same fail-closed pattern already used for `live_execution_artifacts` in `0006_phase3b_negrisk_live.sql`:

- add a trigger that rejects `live_submission_records` rows unless the referenced attempt is `execution_mode = 'live'`
- add a trigger that prevents an `execution_attempts` row with attached live submission records from drifting away from `live`
- keep the migration idempotent with the repository’s existing trigger style instead of relying on the foreign key alone

- [ ] **Step 4: Re-run the persistence and replay tests**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test negrisk_live --test migrations --test runtime_backbone
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-replay --test negrisk_live_contract
```

Expected: PASS. Do **not** persist venue-specific SDK objects; store repo-owned rows and JSON payloads only.

- [ ] **Step 5: Commit**

```bash
git add migrations/0009_phase3c_negrisk_live_submit_closure.sql crates/persistence/src/models.rs crates/persistence/src/repos.rs crates/persistence/src/lib.rs crates/persistence/tests/negrisk_live.rs crates/persistence/tests/migrations.rs crates/persistence/tests/runtime_backbone.rs crates/app-replay/src/lib.rs crates/app-replay/tests/negrisk_live_contract.rs
git commit -m "feat: persist phase3c live submit anchors"
```

### Task 4: Implement Concrete `Polymarket` Submit And Reconcile Providers

**Files:**
- Create: `crates/venue-polymarket/src/negrisk_live.rs`
- Modify: `crates/venue-polymarket/src/orders.rs`
- Modify: `crates/venue-polymarket/src/rest.rs`
- Modify: `crates/venue-polymarket/src/relayer.rs`
- Modify: `crates/venue-polymarket/src/lib.rs`
- Test: `crates/venue-polymarket/tests/order_submission.rs`
- Create: `crates/venue-polymarket/tests/negrisk_live_provider.rs`
- Modify: `crates/venue-polymarket/tests/status_and_retry.rs`

- [ ] **Step 1: Write the failing provider tests**

```rust
#[test]
fn polymarket_submit_provider_maps_success_into_submission_record() {
    let provider = sample_submit_provider_with_accepted_response();
    let outcome = provider.submit_family(&sample_signed_submission(), &sample_attempt()).unwrap();

    match outcome {
        LiveSubmitOutcome::Accepted { submission_record } => {
            assert_eq!(submission_record.provider, "polymarket");
        }
        other => panic!("unexpected outcome: {other:?}"),
    }
}

#[test]
fn polymarket_reconcile_provider_maps_unknown_status_into_still_pending() {
    let provider = sample_reconcile_provider_with_pending_status();
    let outcome = provider.reconcile(&sample_pending_reconcile()).unwrap();
    assert!(matches!(outcome, ReconcileOutcome::StillPending));
}
```

- [ ] **Step 2: Run the venue tests and verify they fail**

Run:

```bash
cargo test -p venue-polymarket --test order_submission --test negrisk_live_provider --test status_and_retry
```

Expected: FAIL because the concrete `Phase 3c` providers do not exist yet.

- [ ] **Step 3: Implement the minimal concrete providers**

Guidelines:

- keep request-body construction in `orders.rs` / `rest.rs`
- keep relayer/status normalization in `relayer.rs`
- put the concrete submit/reconcile provider glue in `negrisk_live.rs`
- normalize every provider response into repo-owned `LiveSubmitOutcome` / `ReconcileOutcome`

Example skeleton:

```rust
pub struct PolymarketNegRiskSubmitProvider {
    rest: PolymarketRestClient,
}

impl VenueExecutionProvider for PolymarketNegRiskSubmitProvider {
    fn submit_family(...) -> Result<LiveSubmitOutcome, SubmitProviderError> {
        // build request, execute call, map accepted/rejected/ambiguous
    }
}
```

- [ ] **Step 4: Re-run the venue tests**

Run:

```bash
cargo test -p venue-polymarket --test order_submission --test negrisk_live_provider --test status_and_retry
```

Expected: PASS. Do **not** wire these providers into `app-live` yet.

- [ ] **Step 5: Commit**

```bash
git add crates/venue-polymarket/src/negrisk_live.rs crates/venue-polymarket/src/orders.rs crates/venue-polymarket/src/rest.rs crates/venue-polymarket/src/relayer.rs crates/venue-polymarket/src/lib.rs crates/venue-polymarket/tests/order_submission.rs crates/venue-polymarket/tests/negrisk_live_provider.rs crates/venue-polymarket/tests/status_and_retry.rs
git commit -m "feat: add polymarket phase3c live providers"
```

### Task 5: Wire Real Submit Closure Through `app-live` Bootstrap And Resume

**Files:**
- Modify: `crates/app-live/Cargo.toml`
- Modify: `crates/app-live/src/config.rs`
- Modify: `crates/app-live/src/main.rs`
- Modify: `crates/app-live/src/negrisk_live.rs`
- Modify: `crates/app-live/src/runtime.rs`
- Modify: `crates/app-live/src/supervisor.rs`
- Modify: `crates/app-live/src/lib.rs`
- Modify: `crates/state/src/facts.rs`
- Modify: `crates/state/src/apply.rs`
- Modify: `crates/state/src/reconcile.rs`
- Modify: `crates/state/src/snapshot.rs`
- Modify: `crates/state/src/lib.rs`
- Modify: `crates/recovery/src/coordinator.rs`
- Modify: `crates/recovery/src/lib.rs`
- Modify: `crates/recovery/tests/recovery_contract.rs`
- Modify: `crates/state/tests/event_apply_contract.rs`
- Modify: `crates/state/tests/restore_anchor.rs`
- Modify: `crates/state/tests/bootstrap_reconcile.rs`
- Modify: `crates/app-live/tests/bootstrap_modes.rs`
- Modify: `crates/app-live/tests/config.rs`
- Modify: `crates/app-live/tests/main_entrypoint.rs`
- Modify: `crates/app-live/tests/negrisk_live_rollout.rs`
- Modify: `crates/app-live/tests/negrisk_rollout_faults.rs`
- Modify: `crates/app-live/tests/unified_supervisor.rs`
- Modify: `crates/app-live/tests/runtime_observability.rs`
- Modify: `crates/app-live/tests/supervisor_observability.rs`
- Create: `crates/app-live/tests/negrisk_live_submit_resume.rs`
- Modify: `crates/app-live/tests/fault_injection.rs`

- [ ] **Step 1: Write the failing app-live tests**

```rust
#[test]
fn live_bootstrap_with_real_submit_provider_persists_submission_record_and_pending_reconcile() {
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.seed_neg_risk_live_targets(BTreeMap::from([(
        "family-a".to_owned(),
        sample_live_target("family-a"),
    )]));
    supervisor.seed_neg_risk_live_approval("family-a");
    supervisor.seed_neg_risk_live_ready_family("family-a");
    supervisor.seed_local_signer_config(sample_local_signer_config());
    supervisor.seed_submit_provider(sample_accepted_but_unconfirmed_provider());

    let summary = supervisor.run_once().unwrap();

    assert_eq!(summary.negrisk_mode, ExecutionMode::Live);
    assert_eq!(summary.neg_risk_live_attempt_count, 1);
    assert_eq!(summary.pending_reconcile_count, 1);
}

#[test]
fn resume_recovers_pending_reconcile_before_emitting_new_live_submit() {
    // seed durable live submission + pending reconcile anchors
    // assert resume reconciles first and does not create a second live attempt
}

#[test]
fn recovery_coordinator_turns_failed_live_submit_into_recovery_or_pending_reconcile() {
    let coordinator = RecoveryCoordinator;
    let outputs = coordinator.on_failed_ambiguous(sample_live_submit_attempt());

    assert!(
        outputs.recovery_intent.is_some() || outputs.pending_reconcile.is_some(),
        "live submit failures must create durable follow-up work"
    );
}
```

Add two more failing cases in this step:

- a state restore test that loads durable `pending_reconcile` plus `submission_ref` anchors and proves restart rehydrates `StateConfidence::Uncertain` before any fresh live submit path runs
- a bootstrap reconcile test that proves a family with restored reconcile work stays recovery-first / no-new-live-submit until that reconcile work reaches a terminal state

- [ ] **Step 2: Run the focused app-live tests and verify they fail**

Run:

```bash
cargo test -p app-live --test bootstrap_modes --test config --test main_entrypoint --test negrisk_live_rollout --test negrisk_rollout_faults --test negrisk_live_submit_resume --test unified_supervisor --test runtime_observability --test supervisor_observability --test fault_injection
cargo test -p recovery --test recovery_contract
cargo test -p state --test event_apply_contract --test restore_anchor --test bootstrap_reconcile
```

Expected: FAIL because `app-live` still treats live promotion as a Phase 3b artifact path and has no real signer/provider wiring.

- [ ] **Step 3: Implement `bootstrap + resume` submit closure**

Required behavior:

- parse minimal live signer config in `config.rs`
- add the `persistence` dependency to `app-live` and load durable live submission / pending-reconcile anchors through the repos created in Task 3 rather than through ad hoc in-memory reconstruction
- wire the existing `recovery::RecoveryCoordinator` into `app-live` instead of open-coding failed-ambiguous handling inside the supervisor
- fail fast in `main.rs` when live mode is requested without valid signer/provider inputs
- build real submission records in `negrisk_live.rs`
- append provider-generated fact events through the same input/apply path in `runtime.rs`
- teach `state::facts` / `state::apply` to interpret the new live submit and live reconcile fact variants, and teach `state::reconcile` / `state::snapshot` to restore reconcile-first truth from durable pending-reconcile anchors instead of app-live-local flags
- in `supervisor.rs`, restore durable live submission and pending reconcile anchors before any fresh live promotion
- keep recovery-first precedence: unresolved scopes must not emit a new live submit
- preserve the `Phase 3c` durability boundary: uncertainty markers and recovery-first overlays are rehydrated from durable pending-reconcile anchors plus coordinator outputs, not from a second ad hoc persistence table

- [ ] **Step 4: Re-run the focused app-live tests**

Run:

```bash
cargo test -p app-live --test bootstrap_modes --test config --test main_entrypoint --test negrisk_live_rollout --test negrisk_rollout_faults --test negrisk_live_submit_resume --test unified_supervisor --test runtime_observability --test supervisor_observability --test fault_injection
cargo test -p recovery --test recovery_contract
cargo test -p state --test event_apply_contract --test restore_anchor --test bootstrap_reconcile
```

Expected: PASS. Resume should fail closed when anchors are missing, should never fabricate live attempts from env-only state, and should rehydrate reconcile-first state before any new live submit is considered.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/Cargo.toml crates/app-live/src/config.rs crates/app-live/src/main.rs crates/app-live/src/negrisk_live.rs crates/app-live/src/runtime.rs crates/app-live/src/supervisor.rs crates/app-live/src/lib.rs crates/state/src/facts.rs crates/state/src/apply.rs crates/state/src/reconcile.rs crates/state/src/snapshot.rs crates/state/src/lib.rs crates/recovery/src/coordinator.rs crates/recovery/src/lib.rs crates/recovery/tests/recovery_contract.rs crates/state/tests/event_apply_contract.rs crates/state/tests/restore_anchor.rs crates/state/tests/bootstrap_reconcile.rs crates/app-live/tests/bootstrap_modes.rs crates/app-live/tests/config.rs crates/app-live/tests/main_entrypoint.rs crates/app-live/tests/negrisk_live_rollout.rs crates/app-live/tests/negrisk_rollout_faults.rs crates/app-live/tests/negrisk_live_submit_resume.rs crates/app-live/tests/unified_supervisor.rs crates/app-live/tests/runtime_observability.rs crates/app-live/tests/supervisor_observability.rs crates/app-live/tests/fault_injection.rs
git commit -m "feat: wire phase3c live submit closure through app-live"
```

### Task 6: Add Replay/Observability Coverage, Update Docs, And Run Full Verification

**Files:**
- Modify: `crates/observability/src/metrics.rs`
- Modify: `crates/observability/tests/metrics.rs`
- Modify: `crates/app-replay/tests/replay_app.rs`
- Modify: `README.md`

- [ ] **Step 1: Write the failing regression tests**

```rust
#[test]
fn runtime_metrics_expose_negrisk_live_submit_closure_signals() {
    let metrics = RuntimeMetrics::default();
    assert_eq!(
        metrics.neg_risk_live_submit_accepted_total.key(),
        MetricKey::new("axiom_neg_risk_live_submit_accepted_total")
    );
    assert_eq!(
        metrics.neg_risk_live_submit_ambiguous_total.key(),
        MetricKey::new("axiom_neg_risk_live_submit_ambiguous_total")
    );
}
```

- [ ] **Step 2: Run the regression tests and verify they fail**

Run:

```bash
cargo test -p observability --test metrics
cargo test -p app-replay --test replay_app
```

Expected: FAIL until the new runtime metrics and replay coverage exist.

- [ ] **Step 3: Implement the minimal metrics/docs/replay updates**

Update:

- typed runtime metrics for live submit accepted / ambiguous / reconcile backlog if they are missing
- replay smoke coverage so new fact/event shapes do not break generic journal replay
- `README.md` so repository status explicitly replaces the current `app-live still does not place neg-risk orders` wording with `Phase 3c bootstrap + resume live submit closure`, while still preserving that continuous daemonization / market-discovered pricing / full production enablement remain follow-on work

- [ ] **Step 4: Run the full verification suite**

Run:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p domain --test runtime_backbone
cargo test -p execution --test negrisk_signing --test negrisk_live_submit --test orchestrator --test retry_and_redeem
cargo test -p state --test event_apply_contract --test restore_anchor --test bootstrap_reconcile
cargo test -p venue-polymarket --test order_submission --test negrisk_live_provider --test status_and_retry
cargo test -p app-live --test bootstrap_modes --test config --test main_entrypoint --test negrisk_live_rollout --test negrisk_rollout_faults --test negrisk_live_submit_resume --test unified_supervisor --test fault_injection --test runtime_observability --test supervisor_observability
cargo test -p observability --test metrics
cargo test -p recovery --test recovery_contract
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test migrations --test runtime_backbone --test negrisk_live
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-replay --test replay_app --test negrisk_live_contract
```

Expected: PASS across the full `Phase 3c` surface.

- [ ] **Step 5: Commit**

```bash
git add crates/observability/src/metrics.rs crates/observability/tests/metrics.rs crates/app-replay/tests/replay_app.rs README.md
git commit -m "feat: finalize phase3c live submit closure"
```

## Final Acceptance Checklist

Before calling this plan complete, verify all of the following are true:

- `app-live` can perform a real `neg-risk` live submit in `bootstrap + resume` mode for config-backed, approved, ready families
- signer / submit / reconcile are all provider-shaped contracts, not one-off bootstrap glue
- every provider result re-enters the authoritative fact/apply path
- live submission records and pending reconcile anchors are durable and replayable
- resume restores durable live truth before considering fresh promotion
- ambiguous outcomes create durable follow-up work instead of warning-only behavior
- `README.md` no longer describes the repository as Phase 3b-only
