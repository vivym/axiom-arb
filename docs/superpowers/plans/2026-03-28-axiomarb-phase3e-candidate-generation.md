# AxiomArb Phase 3e Candidate Generation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a continuous `neg-risk` discovery pipeline that produces durable, replayable `CandidateTargetSet` and `AdoptableTargetRevision` artifacts without changing live trading authority.

**Architecture:** Add a dedicated `DiscoverySupervisor` beside the existing daemon supervisors. New discovery/backfill observations still enter `ExternalFactEvent -> journal -> StateApplier -> PublishedSnapshot`; candidate artifacts are derived products persisted in their own tables, and any adopted startup target revision keeps a durable provenance chain `operator_target_revision -> adoptable_revision -> candidate_revision`.

**Tech Stack:** Rust workspace, `app-live`, `domain`, `state`, `persistence`, `app-replay`, `observability`, `sqlx` migrations, Postgres, `tokio` tests, `serde_json`, `tracing`.

---

## File Map

### Domain / state contracts

- Create: `crates/domain/src/candidates.rs`
  - Shared candidate-generation contracts: `FamilyDiscoveryRecord`, `CandidateValidationResult`, `CandidateTarget`, `CandidateTargetSet`, `AdoptableTargetRevision`, provenance metadata.
- Modify: `crates/domain/src/lib.rs`
  - Export the new candidate contracts.
- Modify: `crates/domain/src/facts.rs`
  - Add discovery/backfill fact payloads used by the daemon ingress path.
- Create: `crates/state/src/candidate.rs`
  - Discovery-domain authoritative projection helpers and `CandidateView`.
- Modify: `crates/state/src/lib.rs`
  - Export candidate projection types.
- Modify: `crates/state/src/facts.rs`
  - Add `DirtyDomain::Candidates` and state-fact hints for discovery/backfill observations.
- Modify: `crates/state/src/store.rs`
  - Persist authoritative discovery-domain projection state without creating a second truth path.
- Modify: `crates/state/src/apply.rs`
  - Apply discovery/backfill facts into candidate-domain state and dirty tracking.
- Modify: `crates/state/src/snapshot.rs`
  - Publish a separate candidate publication/readiness object without turning it into a new fullset/negrisk publish barrier.

### Persistence / durability

- Create: `migrations/0011_phase3e_candidate_generation.sql`
  - Candidate artifact tables plus adoption-provenance table.
- Modify: `crates/persistence/src/models.rs`
  - Add row types for candidate sets, adoptable revisions, and adoption provenance.
- Modify: `crates/persistence/src/repos.rs`
  - Add repos for candidate artifact writes/reads and provenance lookup by `operator_target_revision`.
- Modify: `crates/persistence/src/lib.rs`
  - Export the new repos and persistence errors.

### Daemon discovery pipeline

- Create: `crates/app-live/src/discovery.rs`
  - `DiscoverySupervisor`, conservative validation/pricing pipeline, and `CandidateBridge`.
- Modify: `crates/app-live/src/lib.rs`
  - Export discovery pipeline types.
- Modify: `crates/app-live/src/input_tasks.rs`
  - Carry discovery/backfill facts through the ingress path.
- Modify: `crates/app-live/src/task_groups.rs`
  - Add discovery/backfill task-group helpers and candidate scheduling inputs.
- Modify: `crates/app-live/src/queues.rs`
  - Add candidate dirty-domain / notice routing without waking live dispatch.
- Modify: `crates/app-live/src/supervisor.rs`
  - Start and summarize the discovery pipeline alongside existing daemon supervisors.
- Modify: `crates/app-live/src/daemon.rs`
  - Wire discovery startup ordering and steady-state candidate ticks.
- Modify: `crates/app-live/src/runtime.rs`
  - Restore latest candidate/adoptable/provenance status during startup without blocking non-adoption live startup.

### Replay / observability / docs

- Create: `crates/app-replay/src/negrisk_candidates.rs`
  - Replay-side loaders and summaries for candidate/adoptable/provenance artifacts.
- Modify: `crates/app-replay/src/lib.rs`
  - Export candidate replay helpers.
- Modify: `crates/app-replay/src/main.rs`
  - Surface candidate-generation replay summary output.
- Modify: `crates/observability/src/conventions.rs`
  - Add field keys / span names / metric dimensions for candidate-generation status.
- Modify: `crates/observability/src/metrics.rs`
  - Add counters/gauges for candidate publication and adoption-provenance visibility.
- Modify: `README.md`
  - Update Phase 3 status to reflect `Phase 3e` candidate generation once complete.

### Tests

- Create: `crates/domain/tests/candidate_contracts.rs`
- Create: `crates/state/tests/candidate_projection.rs`
- Create: `crates/persistence/tests/phase3e_candidate_generation.rs`
- Create: `crates/app-live/tests/discovery_supervisor.rs`
- Create: `crates/app-live/tests/candidate_daemon.rs`
- Create: `crates/app-replay/tests/negrisk_candidates.rs`
- Modify: `crates/persistence/tests/migrations.rs`
- Modify: `crates/observability/tests/metrics.rs`
- Modify: `crates/observability/tests/conventions.rs`
- Modify: `crates/state/tests/snapshot_publish.rs`

## Task 1: Candidate Contracts And Discovery-Domain State Projection

**Files:**
- Create: `crates/domain/src/candidates.rs`
- Create: `crates/domain/tests/candidate_contracts.rs`
- Modify: `crates/domain/src/lib.rs`
- Modify: `crates/domain/src/facts.rs`
- Create: `crates/state/src/candidate.rs`
- Create: `crates/state/tests/candidate_projection.rs`
- Modify: `crates/state/src/lib.rs`
- Modify: `crates/state/src/facts.rs`
- Modify: `crates/state/src/store.rs`
- Modify: `crates/state/src/apply.rs`
- Modify: `crates/state/src/snapshot.rs`
- Modify: `crates/state/tests/snapshot_publish.rs`

- [ ] **Step 1: Write the failing domain and state tests**

```rust
#[test]
fn candidate_target_set_keeps_snapshot_policy_and_source_anchors() {
    let set = CandidateTargetSet {
        candidate_revision: "cand-7".to_owned(),
        based_on_snapshot_id: "snapshot-7".to_owned(),
        source_revision: "disc-7".to_owned(),
        ..CandidateTargetSet::empty_for_tests()
    };

    assert_eq!(set.candidate_revision, "cand-7");
    assert_eq!(set.based_on_snapshot_id, "snapshot-7");
    assert_eq!(set.source_revision, "disc-7");
}

#[test]
fn candidate_projection_failure_does_not_block_fullset_or_negrisk_publication() {
    let live_snapshot = PublishedSnapshot::from_store(
        &store,
        ProjectionReadiness::new("snapshot-9", true, true),
    );
    let candidate_publication = CandidatePublication::from_store(
        &store,
        CandidateProjectionReadiness::not_ready("snapshot-9"),
    );

    assert!(live_snapshot.fullset_ready);
    assert!(live_snapshot.negrisk_ready);
    assert!(!candidate_publication.ready);
}
```

- [ ] **Step 2: Run the targeted tests to verify they fail**

Run:

```bash
cargo test -p domain --test candidate_contracts
cargo test -p state --test candidate_projection --test snapshot_publish
```

Expected:
- FAIL with missing `CandidateTargetSet` / `CandidateView` / candidate readiness symbols.

- [ ] **Step 3: Add the shared candidate contracts in `domain`**

```rust
pub struct FamilyDiscoveryRecord {
    pub family_id: String,
    pub members: Vec<String>,
    pub snapshot_id: String,
    pub discovery_revision: String,
}

pub struct CandidateTargetSet {
    pub candidate_revision: String,
    pub based_on_snapshot_id: String,
    pub source_revision: String,
    pub family_candidates: Vec<CandidateTarget>,
}
```

- [ ] **Step 4: Add discovery/backfill fact payloads and state apply hooks**

```rust
pub enum ExternalFactPayloadData {
    NegRiskDiscoveryObserved(NegRiskDiscoveryObservedPayload),
    NegRiskLiveSubmitObserved(NegRiskLiveSubmitObservedPayload),
    // ...
}

pub enum DirtyDomain {
    Runtime,
    NegRiskFamilies,
    Candidates,
}
```

- [ ] **Step 5: Implement `CandidateView` and a separate candidate publication path**

```rust
pub struct CandidatePublication {
    pub snapshot_id: String,
    pub state_version: u64,
    pub ready: bool,
    pub candidate: Option<CandidateView>,
}
```

Implementation notes:
- `CandidatePublication` readiness must be computed independently from `PublishedSnapshot.fullset_ready` / `PublishedSnapshot.negrisk_ready`.
- Candidate projection lag/failure must never suppress an otherwise-ready fullset/negrisk `PublishedSnapshot`.
- `FamilyDiscoveryRecord` must live in authoritative discovery-domain state, not in derived candidate materialization.

- [ ] **Step 6: Re-run the targeted tests**

Run:

```bash
cargo test -p domain --test candidate_contracts
cargo test -p state --test candidate_projection --test snapshot_publish
```

Expected:
- PASS

- [ ] **Step 7: Commit**

```bash
git add crates/domain/src/candidates.rs crates/domain/src/lib.rs crates/domain/src/facts.rs \
  crates/domain/tests/candidate_contracts.rs crates/state/src/candidate.rs crates/state/src/lib.rs \
  crates/state/src/facts.rs crates/state/src/store.rs crates/state/src/apply.rs \
  crates/state/src/snapshot.rs crates/state/tests/candidate_projection.rs \
  crates/state/tests/snapshot_publish.rs
git commit -m "feat: add phase3e candidate contracts"
```

## Task 2: Candidate Artifact Persistence And Adoption Provenance

**Files:**
- Create: `migrations/0011_phase3e_candidate_generation.sql`
- Create: `crates/persistence/tests/phase3e_candidate_generation.rs`
- Modify: `crates/persistence/src/models.rs`
- Modify: `crates/persistence/src/repos.rs`
- Modify: `crates/persistence/src/lib.rs`
- Modify: `crates/persistence/tests/migrations.rs`

- [ ] **Step 1: Write the failing persistence tests**

```rust
#[tokio::test]
async fn migrations_create_candidate_and_adoption_tables() {
    assert!(table_exists(&db.pool, "candidate_target_sets").await);
    assert!(table_exists(&db.pool, "adoptable_target_revisions").await);
    assert!(table_exists(&db.pool, "candidate_adoption_provenance").await);
}

#[tokio::test]
async fn adoption_provenance_round_trips_operator_target_revision() {
    let row = CandidateAdoptionProvenanceRow {
        operator_target_revision: "targets-rev-9".to_owned(),
        adoptable_revision: "adoptable-9".to_owned(),
        candidate_revision: "candidate-9".to_owned(),
    };
    // persist + load + assert equality
}
```

- [ ] **Step 2: Run the persistence tests to verify they fail**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test migrations --test phase3e_candidate_generation
```

Expected:
- FAIL with missing tables and missing repo/model symbols.

- [ ] **Step 3: Add the migration for candidate artifacts and provenance**

```sql
CREATE TABLE candidate_target_sets (
  candidate_revision TEXT PRIMARY KEY,
  snapshot_id TEXT NOT NULL,
  source_revision TEXT NOT NULL,
  payload JSONB NOT NULL
);

CREATE TABLE adoptable_target_revisions (
  adoptable_revision TEXT PRIMARY KEY,
  candidate_revision TEXT NOT NULL REFERENCES candidate_target_sets(candidate_revision),
  rendered_operator_target_revision TEXT NOT NULL,
  payload JSONB NOT NULL
);

CREATE TABLE candidate_adoption_provenance (
  operator_target_revision TEXT PRIMARY KEY,
  adoptable_revision TEXT NOT NULL REFERENCES adoptable_target_revisions(adoptable_revision),
  candidate_revision TEXT NOT NULL REFERENCES candidate_target_sets(candidate_revision)
);
```

- [ ] **Step 4: Add row types and repos**

```rust
pub struct CandidateTargetSetRow {
    pub candidate_revision: String,
    pub snapshot_id: String,
    pub payload: Value,
}

pub struct CandidateArtifactRepo;
pub struct CandidateAdoptionRepo;
```

Implementation notes:
- The provenance lookup must be keyed by `operator_target_revision`.
- Missing candidate artifacts must not block ordinary live startup.
- Adoption-aware restore must fail closed if a candidate-derived `operator_target_revision` cannot be linked back to `adoptable_revision` and `candidate_revision`.

- [ ] **Step 5: Re-run the persistence tests**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test migrations --test phase3e_candidate_generation
```

Expected:
- PASS

- [ ] **Step 6: Commit**

```bash
git add migrations/0011_phase3e_candidate_generation.sql crates/persistence/src/models.rs \
  crates/persistence/src/repos.rs crates/persistence/src/lib.rs \
  crates/persistence/tests/migrations.rs crates/persistence/tests/phase3e_candidate_generation.rs
git commit -m "feat: persist phase3e candidate artifacts"
```

## Task 3: DiscoverySupervisor And Conservative Candidate Pipeline

**Files:**
- Create: `crates/app-live/src/discovery.rs`
- Create: `crates/app-live/tests/discovery_supervisor.rs`
- Modify: `crates/app-live/src/lib.rs`
- Modify: `crates/app-live/src/input_tasks.rs`
- Modify: `crates/app-live/src/task_groups.rs`
- Modify: `crates/app-live/src/queues.rs`

- [ ] **Step 1: Write the failing `app-live` discovery tests**

```rust
#[tokio::test]
async fn discovery_supervisor_publishes_candidate_target_set_without_waking_live_dispatch() {
    let report = supervisor.tick_candidate_generation_for_tests().await.unwrap();

    assert_eq!(report.candidate_revision.as_deref(), Some("candidate-7"));
    assert_eq!(report.live_dispatch_woken, false);
}

#[tokio::test]
async fn candidate_bridge_renders_adoptable_revision_with_operator_target_revision() {
    let bridge = CandidateBridge::for_tests();
    let adoptable = bridge.render(&candidate_set).unwrap();

    assert_eq!(adoptable.candidate_revision, candidate_set.candidate_revision);
    assert!(!adoptable.rendered_operator_target_revision.is_empty());
}
```

- [ ] **Step 2: Run the failing tests**

Run:

```bash
cargo test -p app-live --test discovery_supervisor
```

Expected:
- FAIL with missing `DiscoverySupervisor` / `CandidateBridge` / candidate report types.

- [ ] **Step 3: Implement `DiscoverySupervisor` and conservative engines**

```rust
pub struct DiscoverySupervisor {
    validation: CandidateValidationEngine,
    pricing: CandidatePricingEngine,
    bridge: CandidateBridge,
}
```

Implementation notes:
- Start from existing discovery snapshot + validation/halt truth.
- Reuse the existing `venue-polymarket` metadata refresh/backfill surface as the first discovery source; do not invent a second transport unless a concrete gap blocks candidate generation.
- Keep pricing conservative: advisory price band, advisory size cap, no execution requests.
- Return `Deferred` / `Excluded` rather than producing weak candidates.

- [ ] **Step 4: Route candidate dirty-domain notices through a dedicated queue**

```rust
pub struct CandidateNoticeQueue {
    backlog: VecDeque<CandidateNotice>,
}
```

Implementation notes:
- `DecisionSupervisor` must keep ignoring pure candidate-domain updates.
- `DiscoverySupervisor` should consume `CandidateNoticeQueue`, not `SnapshotDispatchQueue`.
- Discovery/backfill input helpers should still emit `InputTaskEvent` that flows through the same ingress path.

- [ ] **Step 5: Re-run the discovery tests**

Run:

```bash
cargo test -p app-live --test discovery_supervisor
```

Expected:
- PASS

- [ ] **Step 6: Commit**

```bash
git add crates/app-live/src/discovery.rs crates/app-live/src/lib.rs \
  crates/app-live/src/input_tasks.rs crates/app-live/src/task_groups.rs \
  crates/app-live/src/queues.rs crates/app-live/tests/discovery_supervisor.rs
git commit -m "feat: add phase3e discovery supervisor"
```

## Task 4: Daemon Wiring, Startup Restore, And Provenance-Aware Status

**Files:**
- Create: `crates/app-live/tests/candidate_daemon.rs`
- Modify: `crates/app-live/src/supervisor.rs`
- Modify: `crates/app-live/src/daemon.rs`
- Modify: `crates/app-live/src/runtime.rs`
- Modify: `crates/app-live/tests/daemon_lifecycle.rs`
- Modify: `crates/app-live/tests/unified_supervisor.rs`

- [ ] **Step 1: Write the failing daemon/startup tests**

```rust
#[tokio::test]
async fn daemon_restores_candidate_status_without_blocking_non_adoption_startup() {
    let report = daemon.run_until_idle_for_tests(3).await.unwrap();
    assert_eq!(report.summary.latest_candidate_revision.as_deref(), Some("candidate-9"));
}

#[tokio::test]
async fn candidate_derived_operator_target_revision_requires_provenance_on_restore() {
    let err = run_live_daemon_from_durable_store_with_neg_risk_live_targets_instrumented(...).unwrap_err();
    assert!(err.to_string().contains("candidate adoption provenance"));
}
```

- [ ] **Step 2: Run the daemon tests to verify they fail**

Run:

```bash
cargo test -p app-live --test candidate_daemon --test daemon_lifecycle --test unified_supervisor
```

Expected:
- FAIL with missing summary fields and missing provenance-aware restore logic.

- [ ] **Step 3: Extend supervisor summaries with latest candidate/adoptable status**

```rust
pub struct SupervisorSummary {
    pub latest_candidate_revision: Option<String>,
    pub latest_adoptable_revision: Option<String>,
    pub adopted_candidate_provenance: Option<String>,
    // existing fields...
}
```

- [ ] **Step 4: Restore candidate artifacts and fail closed only for adoption-aware startup**

```rust
if let Some(operator_target_revision) = operator_target_revision {
    load_candidate_adoption_provenance(operator_target_revision)?;
}
```

Implementation notes:
- Ordinary startup with no candidate-derived adoption must still boot if candidate artifacts are absent.
- If the configured `operator_target_revision` is candidate-derived, the provenance chain must resolve fully or startup must fail closed.

- [ ] **Step 5: Re-run the daemon tests**

Run:

```bash
cargo test -p app-live --test candidate_daemon --test daemon_lifecycle --test unified_supervisor
```

Expected:
- PASS

- [ ] **Step 6: Commit**

```bash
git add crates/app-live/src/supervisor.rs crates/app-live/src/daemon.rs \
  crates/app-live/src/runtime.rs crates/app-live/tests/candidate_daemon.rs \
  crates/app-live/tests/daemon_lifecycle.rs crates/app-live/tests/unified_supervisor.rs
git commit -m "feat: wire phase3e candidate daemon status"
```

## Task 5: Replay And Observability For Candidate Generation

**Files:**
- Create: `crates/app-replay/src/negrisk_candidates.rs`
- Create: `crates/app-replay/tests/negrisk_candidates.rs`
- Modify: `crates/app-replay/src/lib.rs`
- Modify: `crates/app-replay/src/main.rs`
- Modify: `crates/observability/src/conventions.rs`
- Modify: `crates/observability/src/metrics.rs`
- Modify: `crates/observability/tests/metrics.rs`
- Modify: `crates/observability/tests/conventions.rs`

- [ ] **Step 1: Write the failing replay and metrics tests**

```rust
#[tokio::test]
async fn replay_loads_candidate_and_adoption_provenance_chain() {
    let summary = load_negrisk_candidate_summary(&pool).await.unwrap();
    assert_eq!(summary.latest_candidate_revision.as_deref(), Some("candidate-9"));
    assert_eq!(summary.operator_target_revision.as_deref(), Some("targets-rev-9"));
}

#[test]
fn runtime_metrics_include_candidate_generation_keys() {
    assert_eq!(field_keys::CANDIDATE_REVISION, "candidate_revision");
    assert!(RuntimeMetrics::default().neg_risk_candidate_publish_total().key().as_str().contains("candidate"));
}
```

- [ ] **Step 2: Run the replay and observability tests to verify they fail**

Run:

```bash
cargo test -p app-replay --test negrisk_candidates
cargo test -p observability --test metrics --test conventions
```

Expected:
- FAIL with missing replay loader/summary symbols and missing metric/convention keys.

- [ ] **Step 3: Add replay loaders for candidate/adoptable/provenance artifacts**

```rust
pub async fn load_negrisk_candidate_summary(
    pool: &PgPool,
) -> Result<NegRiskCandidateSummary, PersistenceError> {
    // load latest candidate, adoptable revision, and provenance chain
}
```

- [ ] **Step 4: Add observability keys and counters**

```rust
pub const CANDIDATE_REVISION: &str = "candidate_revision";
pub const ADOPTABLE_REVISION: &str = "adoptable_revision";
pub const OPERATOR_TARGET_REVISION: &str = "operator_target_revision";
```

- [ ] **Step 5: Re-run the replay and observability tests**

Run:

```bash
cargo test -p app-replay --test negrisk_candidates
cargo test -p observability --test metrics --test conventions
```

Expected:
- PASS

- [ ] **Step 6: Commit**

```bash
git add crates/app-replay/src/negrisk_candidates.rs crates/app-replay/src/lib.rs \
  crates/app-replay/src/main.rs crates/app-replay/tests/negrisk_candidates.rs \
  crates/observability/src/conventions.rs crates/observability/src/metrics.rs \
  crates/observability/tests/metrics.rs crates/observability/tests/conventions.rs
git commit -m "feat: add phase3e replay and metrics surfaces"
```

## Task 6: README, End-To-End Verification, And Final Cleanup

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Update the README status text**

```md
- Phase 3e continuously generates conservative neg-risk candidate targets and adoptable startup-scoped target revisions.
- Candidate generation remains advisory; operator adoption is still explicit and startup-scoped.
```

- [ ] **Step 2: Run formatting**

Run:

```bash
cargo fmt --all
```

Expected:
- PASS with no diff after a second `cargo fmt --all --check`.

- [ ] **Step 3: Run targeted Phase 3e verification**

Run:

```bash
cargo test -p domain --test candidate_contracts
cargo test -p state --test candidate_projection --test snapshot_publish
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test migrations --test phase3e_candidate_generation --test negrisk
cargo test -p app-live --test discovery_supervisor --test candidate_daemon --test daemon_lifecycle --test unified_supervisor
cargo test -p app-replay --test negrisk_candidates
cargo test -p observability --test metrics --test conventions
```

Expected:
- PASS

- [ ] **Step 4: Run workspace-level safety checks**

Run:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test --workspace
```

Expected:
- PASS

- [ ] **Step 5: Commit**

```bash
git add README.md
git commit -m "docs: update phase3e candidate generation status"
```

## Implementation Notes

- Use `@superpowers:test-driven-development` for every task. Do not write implementation before the task's failing test exists and has been run.
- Use `@superpowers:verification-before-completion` before claiming the branch is ready.
- Keep candidate generation advisory in this phase. Do not create any code path that directly promotes `CandidateTargetSet` into live execution.
- Do not hot-reload active operator targets. Any bridge/adoption work must preserve the existing startup-scoped or controlled-restart-scoped boundary from `Phase 3d`.
- Treat `FamilyDiscoveryRecord` as authoritative discovery-domain projection state. Treat `CandidateTargetSet` and `AdoptableTargetRevision` as durable derived artifacts.
- Preserve the single provenance chain `operator_target_revision -> adoptable_revision -> candidate_revision` anywhere candidate-derived startup targets are made active.
