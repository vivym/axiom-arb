# AxiomArb V1b Neg-Risk Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `v1b foundation` needed for standard Polymarket `neg-risk` families: raw discovery, structure validation, member-level exposure reconstruction, replay visibility, and family-scoped controls, without starting live `neg-risk` execution.

**Architecture:** Extend the existing `v1a` workspace instead of creating a parallel engine. Keep `neg-risk` foundation split across venue metadata ingestion, domain/persistence, a dedicated `strategy-negrisk` crate, and replay/observability hooks so the later live rollout can reuse the same identifier, journal, and state layers.

**Tech Stack:** Rust, Tokio, Serde, Reqwest, SQLx, PostgreSQL, `rust_decimal`, Tracing, Prometheus, local listener-backed HTTP test servers

---

## Scope Boundary

This plan intentionally covers `v1b foundation` only.

- In scope: standard `neg-risk` family raw discovery, family graph construction, structure validation, placeholder and `Other` classification plus exclusion verdicts, member-level exposure reconstruction from existing state/persistence data, replay summaries, family metrics, family-scoped halt primitives.
- Out of scope: live `neg-risk` order placement, `neg-risk` execution plans, broken-leg repair for `neg-risk`, augmented `neg-risk`, direct trading on `Other`, runtime venue wiring in `app-live`.
- Launch reality: current `app-live` is still a bootstrap skeleton, so this plan must not pretend to deliver a `v1b` live runner.

## File Structure Map

### Root

- Modify: `/Users/viv/projs/axiom-arb/Cargo.toml`
  Responsibility: add the `strategy-negrisk` workspace member.
- Modify: `/Users/viv/projs/axiom-arb/README.md`
  Responsibility: document the difference between `v1a` runtime reality and `v1b foundation` outputs.

### Domain And State

- Create: `crates/domain/src/negrisk.rs`
  Responsibility: `NegRiskFamily`, family nodes, `neg-risk` variant classification, validator results, exclusion reasons, member-level exposure vectors, family halt state, and halt precedence rules.
- Modify: `crates/domain/src/lib.rs`
  Responsibility: export `neg-risk` domain types.
- Modify: `crates/state/src/store.rs`
  Responsibility: expose a read-only inventory snapshot API suitable for family exposure reconstruction.
- Modify: `crates/state/src/lib.rs`
  Responsibility: export the new inventory snapshot types.
- Test: `crates/domain/tests/negrisk.rs`
- Test: `crates/state/tests/family_exposure_inputs.rs`

### Venue Metadata

- Create: `crates/venue-polymarket/src/metadata.rs`
  Responsibility: parse raw market and event metadata needed to discover all standard `neg-risk` families, classify placeholder or `Other` outcomes, classify standard vs augmented semantics, assign local `discovery_revision`, and derive canonical `metadata_snapshot_hash`.
- Modify: `crates/venue-polymarket/src/rest.rs`
  Responsibility: add paginated request builders and fetchers for `neg-risk` discovery metadata, including completeness and refresh handling.
- Modify: `crates/venue-polymarket/src/lib.rs`
  Responsibility: export metadata types and fetch helpers.
- Test: `crates/venue-polymarket/tests/metadata.rs`
  Responsibility: use a real local listener for mock HTTP responses.

### Persistence

- Create: `migrations/0004_neg_risk_foundation.sql`
  Responsibility: persist family validation current view, explainability fields, validation-history linkage, and revision-aware family halt current view without duplicating identifier-map truth.
- Modify: `crates/persistence/src/models.rs`
  Responsibility: row models for family validation state, explainability metadata, and revision-aware family halt state.
- Modify: `crates/persistence/src/repos.rs`
  Responsibility: repositories to upsert/list family validation and halt rows, persist successful discovery snapshot events, and emit explainable validation or halt events.
- Modify: `crates/persistence/src/lib.rs`
  Responsibility: export new repositories.
- Test: `crates/persistence/tests/negrisk.rs`

### Strategy And Replay

- Create: `crates/strategy-negrisk/Cargo.toml`
- Create: `crates/strategy-negrisk/src/lib.rs`
  Responsibility: crate exports for graph, validator, and exposure reconstruction.
- Create: `crates/strategy-negrisk/src/graph.rs`
  Responsibility: build families from identifier + venue metadata.
- Create: `crates/strategy-negrisk/src/validator.rs`
  Responsibility: enforce standard-family rules and exclusion reasons.
- Create: `crates/strategy-negrisk/src/exposure.rs`
  Responsibility: reconstruct member-level exposure vectors and family rollups using the existing state snapshot surface.
- Test: `crates/strategy-negrisk/tests/graph_and_validator.rs`
- Test: `crates/strategy-negrisk/tests/exposure.rs`
- Create: `crates/app-replay/src/negrisk_summary.rs`
- Modify: `crates/app-replay/src/lib.rs`
  Responsibility: export a persistence-backed `neg-risk` foundation summary helper while keeping existing generic `event_journal` replay behavior unchanged.
- Test: `crates/app-replay/tests/negrisk_summary.rs`
- Test: `crates/app-replay/tests/negrisk_foundation_contract.rs`

### Observability

- Modify: `crates/observability/src/metrics.rs`
  Responsibility: raw family discovery, included-family count, excluded-family count, family halt count, and metadata refresh count.
- Modify: `crates/observability/src/lib.rs`
  Responsibility: export the new metrics handles.
- Test: `crates/observability/tests/metrics.rs`

## Task 1: Add Neg-Risk Domain Types And State Read Boundaries

**Files:**
- Modify: `/Users/viv/projs/axiom-arb/Cargo.toml`
- Create: `crates/domain/src/negrisk.rs`
- Modify: `crates/domain/src/lib.rs`
- Modify: `crates/state/src/store.rs`
- Modify: `crates/state/src/lib.rs`
- Test: `crates/domain/tests/negrisk.rs`
- Test: `crates/state/tests/family_exposure_inputs.rs`

- [ ] **Step 1: Write the failing domain tests**

```rust
#[test]
fn standard_family_keeps_placeholder_and_other_members_visible_for_validation() {
    let family = sample_family(["Alice", "Bob", "Other"]);
    assert_eq!(family.members.len(), 3);
    assert!(family.members.iter().any(|member| member.is_other));
}

#[test]
fn member_level_exposure_preserves_bucket_breakdown() {
    let exposure = NegRiskExposureVector::from_inventory(sample_inventory(), sample_identifier_map());
    assert_eq!(exposure.members.len(), 2);
    assert!(
        exposure.members[0]
            .bucket_quantities
            .contains_key(&InventoryBucket::MatchedUnsettled)
    );
}

#[test]
fn state_store_exposes_inventory_snapshot_without_mutable_access() {
    let mut store = StateStore::new();
    store.record_local_inventory(TokenId::from("token-a"), InventoryBucket::Free, dec!(2));
    assert_eq!(store.inventory_snapshot().len(), 1);
}

#[test]
fn family_halt_precedence_stays_below_global_halt_and_above_strategy_filters() {
    let policy = FamilyHaltPolicy::default_block_new_risk();
    assert_eq!(policy.priority(), HaltPriority::Family);
}

#[test]
fn stale_family_halt_remains_blocking_until_reconfirmed_or_cleared() {
    let halt = FamilyHaltState::active("family-1", "sha256:snapshot-a");
    let status = halt.reconcile_against_snapshot_hash("sha256:snapshot-b");
    assert_eq!(status, FamilyHaltStatus::StaleBlocking);
}
```

- [ ] **Step 2: Run the domain test to verify failure**

Run: `cargo test -p domain standard_family_keeps_placeholder_and_other_members_visible_for_validation -- --exact`

Expected: FAIL because `neg-risk` domain types do not exist yet.

- [ ] **Step 3: Implement the `neg-risk` domain model**

```rust
pub struct NegRiskFamily {
    pub family_id: EventFamilyId,
    pub route: MarketRoute,
    pub members: Vec<NegRiskNode>,
}

pub enum FamilyExclusionReason {
    PlaceholderOutcome,
    OtherOutcome,
    AugmentedVariant,
    MissingNamedOutcomes,
    NonNegRiskRoute,
}

pub struct NegRiskExposureVector {
    pub family_id: EventFamilyId,
    pub members: Vec<NegRiskMemberExposure>,
    pub rollup: NegRiskExposureRollup,
}

pub enum NegRiskVariant {
    Standard,
    Augmented,
    Unknown,
}
```

- [ ] **Step 4: Add a read-only state boundary for family exposure**

```rust
pub struct InventorySnapshotRow {
    pub token_id: TokenId,
    pub bucket: InventoryBucket,
    pub quantity: Decimal,
}
```

Also define the family-halt precedence contract here:
- `GlobalHalt` overrides every family-level control.
- family halt outranks market-local or strategy-local `neg-risk` activation filters.
- foundation-phase family halt blocks new `neg-risk` risk only; it does not rewrite bootstrap or global cancel behavior.
- family halt is scoped to `family_id`, but records the metadata snapshot it was evaluated against.
- if an active halt's `metadata_snapshot_hash` does not match the latest discovered snapshot, it becomes `StaleBlocking`: still blocks new `neg-risk` risk until revalidated or manually cleared.

- [ ] **Step 5: Run the domain and state smoke tests**

Run: `cargo test -p domain && cargo test -p state`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/domain crates/state
git commit -m "feat: add neg-risk domain and state boundaries"
```

## Task 2: Add Polymarket Neg-Risk Raw Discovery, Pagination, And Classification

**Files:**
- Create: `crates/venue-polymarket/src/metadata.rs`
- Modify: `crates/venue-polymarket/src/rest.rs`
- Modify: `crates/venue-polymarket/src/lib.rs`
- Test: `crates/venue-polymarket/tests/metadata.rs`

- [ ] **Step 1: Write the failing metadata tests**

```rust
#[tokio::test]
async fn fetch_neg_risk_metadata_discovers_all_pages_and_classifies_members() {
    let server = spawn_local_listener(sample_paginated_neg_risk_payloads());
    let client = test_client(server.base_url());

    let families = client.fetch_neg_risk_metadata().await.unwrap();

    assert_eq!(families.len(), 2);
    assert!(families
        .iter()
        .any(|family| family.members.iter().any(|member| member.is_other)));
}

#[tokio::test]
async fn successful_refresh_publishes_a_new_discovery_revision() {
    let server = spawn_local_listener(sample_refreshing_neg_risk_payloads());
    let client = test_client(server.base_url());

    let initial = client.fetch_neg_risk_metadata().await.unwrap();
    let refreshed = client.fetch_neg_risk_metadata().await.unwrap();

    assert!(initial[0].discovery_revision < refreshed[0].discovery_revision);
}

#[tokio::test]
async fn failed_refresh_does_not_publish_a_new_revision_or_replace_current_view() {
    let server = spawn_local_listener(sample_partial_failure_neg_risk_payloads());
    let client = test_client(server.base_url());

    let initial = client.fetch_neg_risk_metadata().await.unwrap();
    let failed = client.try_fetch_neg_risk_metadata().await;
    let after_failure = client.fetch_neg_risk_metadata().await.unwrap();

    assert!(failed.is_err());
    assert_eq!(initial[0].discovery_revision, after_failure[0].discovery_revision);
    assert_eq!(initial[0].metadata_snapshot_hash, after_failure[0].metadata_snapshot_hash);
}

#[tokio::test]
async fn augmented_family_is_classified_from_family_level_flags() {
    let server = spawn_local_listener(sample_augmented_neg_risk_payloads());
    let client = test_client(server.base_url());

    let families = client.fetch_neg_risk_metadata().await.unwrap();

    assert!(families
        .iter()
        .any(|family| family.neg_risk_variant == NegRiskVariant::Augmented));
}
```

- [ ] **Step 2: Run the failing venue test**

Run: `cargo test -p venue-polymarket fetch_neg_risk_metadata_discovers_all_pages_and_classifies_members -- --exact`

Expected: FAIL because the metadata client and parser do not exist yet.

- [ ] **Step 3: Implement metadata parsing and fetchers**

```rust
pub struct NegRiskMarketMetadata {
    pub event_family_id: String,
    pub event_id: String,
    pub condition_id: String,
    pub token_id: String,
    pub outcome_label: String,
    pub route: MarketRoute,
    pub enable_neg_risk: Option<bool>,
    pub neg_risk_augmented: Option<bool>,
    pub neg_risk_variant: NegRiskVariant,
    pub is_placeholder: bool,
    pub is_other: bool,
    pub discovery_revision: i64,
    pub metadata_snapshot_hash: String,
}
```

Discovery rules for this task:
- fetch raw `neg-risk` metadata without applying validator exclusions
- traverse pagination until exhaustion
- dedupe canonical member rows by `(event_family_id, condition_id, token_id)`
- if duplicate rows disagree within the same `discovery_revision`, treat that as a data-quality error and surface it explicitly
- `discovery_revision` is a local monotonic snapshot identifier assigned per completed discovery refresh
- `metadata_snapshot_hash` is the content hash of the canonicalized family/member set produced by one successful refresh
- if duplicate rows disagree across different revisions, prefer the highest local `discovery_revision` while preserving the older revision in journaled history
- a refresh is published atomically: only after all pages are fetched, deduped, canonicalized, and classified
- failed refreshes must not replace the current view and must not mint a new `discovery_revision`
- retry page fetches using bounded transport retry rules
- preserve enough classification metadata for later validator decisions
- canonical member-vector ordering must be deterministic:
  - use stable venue order if the upstream payload provides one
  - otherwise sort by `(event_id, condition_id, token_id, outcome_label)`

- [ ] **Step 4: Use a real local listener for HTTP mocking**

```rust
let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
let handle = thread::spawn(move || {
    let (mut stream, _) = listener.accept().expect("accept request");
    // write raw HTTP response body here
});
```

Run: `cargo test -p venue-polymarket metadata`

Expected: PASS with no external network dependency and no new web-framework test dependency.

- [ ] **Step 5: Commit**

```bash
git add crates/venue-polymarket
git commit -m "feat: add neg-risk metadata discovery client"
```

## Task 3: Persist Family Validation State, Explainability, And Halt State

**Files:**
- Create: `migrations/0004_neg_risk_foundation.sql`
- Modify: `crates/persistence/src/models.rs`
- Modify: `crates/persistence/src/repos.rs`
- Modify: `crates/persistence/src/lib.rs`
- Test: `crates/persistence/tests/negrisk.rs`

- [ ] **Step 1: Write the failing persistence test**

```rust
#[tokio::test]
async fn stores_family_validation_revision_and_explainability_fields() {
    let pool = test_db_pool().await;
    run_migrations(&pool).await.unwrap();

    NegRiskFamilyRepo
        .upsert_validation(&pool, &sample_validation("family-1"))
        .await
        .unwrap();

    let row = NegRiskFamilyRepo
        .list_validations(&pool)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(row.member_count, 3);
    assert!(row.metadata_snapshot_hash.starts_with("sha256:"));
}

#[tokio::test]
async fn validation_and_halt_updates_are_journaled_for_explainability() {
    let pool = test_db_pool().await;
    run_migrations(&pool).await.unwrap();

    NegRiskFamilyRepo
        .upsert_validation(&pool, &sample_validation("family-1"))
        .await
        .unwrap();

    let rows = JournalRepo.list_after(&pool, 0, 100).await.unwrap();
    assert!(rows.iter().any(|row| row.event_type == "family_validation"));
}

#[tokio::test]
async fn validation_journal_payload_preserves_the_exact_member_vector() {
    let pool = test_db_pool().await;
    run_migrations(&pool).await.unwrap();

    NegRiskFamilyRepo
        .upsert_validation(&pool, &sample_validation("family-1"))
        .await
        .unwrap();

    let row = JournalRepo
        .list_after(&pool, 0, 100)
        .await
        .unwrap()
        .into_iter()
        .find(|row| row.event_type == "family_validation")
        .unwrap();
    assert!(row.payload.to_string().contains("\"member_vector\""));
    assert!(row.payload.to_string().contains("condition-1"));
    assert!(row.payload.to_string().contains("token-1"));
}

#[tokio::test]
async fn halt_state_records_the_snapshot_hash_it_applies_to() {
    let pool = test_db_pool().await;
    run_migrations(&pool).await.unwrap();

    NegRiskFamilyRepo
        .upsert_halt(&pool, &sample_halt("family-1", "sha256:snapshot-a"))
        .await
        .unwrap();

    let row = NegRiskFamilyRepo.list_halts(&pool).await.unwrap().pop().unwrap();
    assert_eq!(row.metadata_snapshot_hash.as_deref(), Some("sha256:snapshot-a"));
}

#[tokio::test]
async fn repeated_halt_updates_append_multiple_halt_journal_events() {
    let pool = test_db_pool().await;
    run_migrations(&pool).await.unwrap();

    NegRiskFamilyRepo
        .upsert_halt(&pool, &sample_halt("family-1", "sha256:snapshot-a"))
        .await
        .unwrap();
    NegRiskFamilyRepo
        .upsert_halt(&pool, &sample_halt("family-1", "sha256:snapshot-b"))
        .await
        .unwrap();

    let rows = JournalRepo.list_after(&pool, 0, 100).await.unwrap();
    let halt_rows = rows
        .into_iter()
        .filter(|row| row.event_type == "family_halt")
        .count();
    assert_eq!(halt_rows, 2);
}
```

- [ ] **Step 2: Run the failing persistence test**

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence stores_family_validation_revision_and_explainability_fields -- --exact`

Expected: FAIL because the migration and repositories do not exist yet.

- [ ] **Step 3: Add the new migration**

```sql
CREATE TABLE neg_risk_family_validations (
  event_family_id TEXT PRIMARY KEY,
  validation_status TEXT NOT NULL,
  exclusion_reason TEXT,
  metadata_snapshot_hash TEXT NOT NULL,
  member_count INTEGER NOT NULL,
  first_seen_at TIMESTAMPTZ NOT NULL,
  last_seen_at TIMESTAMPTZ NOT NULL,
  validated_at TIMESTAMPTZ NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE family_halt_settings (
  event_family_id TEXT PRIMARY KEY,
  halted BOOLEAN NOT NULL,
  reason TEXT,
  blocks_new_risk BOOLEAN NOT NULL,
  metadata_snapshot_hash TEXT,
  set_at TIMESTAMPTZ NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL
);
```

`family_halt_settings` is the current-view table only. Halt history is preserved in append-only `event_journal`.

This task must also append `family_validation` and `family_halt` entries to the existing `event_journal` whenever the persisted verdict or halt state changes, including:
- the metadata snapshot hash that the decision applied to
- the exact ordered family member vector at decision time: `condition_id`, `token_id`, `outcome_label`, placeholder or `Other` classification, and `neg_risk_variant`
- the `discovery_revision` that produced that canonical member vector

- [ ] **Step 4: Implement repository and row conversions**

```rust
pub struct NegRiskFamilyRepo;

impl NegRiskFamilyRepo {
    pub async fn upsert_validation(&self, pool: &PgPool, row: &NegRiskFamilyValidationRow) -> Result<()>;
    pub async fn list_validations(&self, pool: &PgPool) -> Result<Vec<NegRiskFamilyValidationRow>>;
    pub async fn upsert_halt(&self, pool: &PgPool, row: &FamilyHaltRow) -> Result<()>;
    pub async fn list_halts(&self, pool: &PgPool) -> Result<Vec<FamilyHaltRow>>;
}
```

- [ ] **Step 5: Run persistence tests**

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence negrisk`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add migrations crates/persistence
git commit -m "feat: persist neg-risk family validation state"
```

## Task 4: Build The Neg-Risk Graph And Standard-Scope Validator

**Files:**
- Create: `crates/strategy-negrisk/Cargo.toml`
- Create: `crates/strategy-negrisk/src/lib.rs`
- Create: `crates/strategy-negrisk/src/graph.rs`
- Create: `crates/strategy-negrisk/src/validator.rs`
- Test: `crates/strategy-negrisk/tests/graph_and_validator.rs`

- [ ] **Step 1: Write the failing graph and validator tests**

```rust
#[test]
fn graph_builder_groups_conditions_by_event_family() {
    let graph = build_family_graph(sample_identifier_records(), sample_metadata());
    assert_eq!(graph.families().len(), 2);
    assert!(graph
        .families()
        .iter()
        .any(|family| family.members.iter().any(|member| member.is_placeholder)));
}

#[test]
fn validator_excludes_augmented_or_other_families_from_initial_scope_without_hiding_them() {
    let verdict = validate_family(sample_augmented_family(), sample_discovery_revision(), "sha256:snapshot-a");
    assert_eq!(verdict.status, FamilyValidationStatus::Excluded);
    assert_eq!(verdict.reason, Some(FamilyExclusionReason::AugmentedVariant));
}

#[test]
fn validator_recomputes_verdict_when_snapshot_hash_changes() {
    let first = validate_family(sample_placeholder_family(), 7, "sha256:snapshot-a");
    let second = validate_family(sample_named_family(), 8, "sha256:snapshot-b");
    assert_ne!(first.metadata_snapshot_hash, second.metadata_snapshot_hash);
}
```

- [ ] **Step 2: Run the failing strategy test**

Run: `cargo test -p strategy-negrisk graph_builder_groups_conditions_by_event_family -- --exact`

Expected: FAIL because the crate does not exist yet.

- [ ] **Step 3: Implement graph building from identifier and venue metadata**

```rust
pub fn build_family_graph(
    records: Vec<IdentifierRecord>,
    metadata: Vec<NegRiskMarketMetadata>,
) -> Result<NegRiskGraph, GraphBuildError> { ... }
```

- [ ] **Step 4: Implement the initial-scope validator**

```rust
pub fn validate_family(
    family: &NegRiskFamily,
    discovery_revision: i64,
    metadata_snapshot_hash: &str,
) -> FamilyValidation {
    // keep every discovered family visible, then emit Included or Excluded with reason
}
```

Validator rules for this task:
- raw discovery never drops a family merely because it is excluded from initial live scope
- validator produces `Included` or `Excluded` verdicts plus explainability metadata
- placeholder, direct `Other`, and augmented semantics remain explicit exclusion reasons
- augmented exclusion must be driven by family-level variant evidence, not inferred only from individual member labels

- [ ] **Step 5: Run the strategy test suite**

Run: `cargo test -p strategy-negrisk`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/strategy-negrisk Cargo.toml
git commit -m "feat: add neg-risk graph and validator foundation"
```

## Task 5: Reconstruct Member-Level Family Exposure And Add Foundation Summaries

**Files:**
- Create: `crates/strategy-negrisk/src/exposure.rs`
- Create: `crates/app-replay/src/negrisk_summary.rs`
- Modify: `crates/app-replay/src/lib.rs`
- Test: `crates/strategy-negrisk/tests/exposure.rs`
- Test: `crates/app-replay/tests/negrisk_summary.rs`
- Test: `crates/app-replay/tests/negrisk_foundation_contract.rs`

- [ ] **Step 1: Write the failing exposure and foundation-summary tests**

```rust
#[test]
fn family_exposure_reconstructs_member_vectors_and_rollups() {
    let exposure = reconstruct_family_exposure(
        sample_family(),
        sample_inventory_rows(),
        sample_identifier_map(),
    );
    assert_eq!(exposure.family_id.as_str(), "family-1");
    assert!(exposure
        .members
        .iter()
        .any(|member| member.bucket_quantities.contains_key(&InventoryBucket::Redeemable)));
}

#[tokio::test]
async fn foundation_summary_reports_family_validation_and_halts() {
    let pool = test_db_pool().await;
    seed_neg_risk_validation_rows(&pool).await;

    let summary = load_neg_risk_foundation_summary(&pool).await.unwrap();

    assert_eq!(summary.validated_family_count, 1);
    assert_eq!(summary.halted_family_count, 1);
}
```

- [ ] **Step 2: Run the failing summary test**

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-replay foundation_summary_reports_family_validation_and_halts -- --exact`

Expected: FAIL because no persistence-backed `neg-risk` foundation summary exists yet.

- [ ] **Step 3: Implement member-level family exposure reconstruction**

```rust
pub struct FamilyExposure {
    pub family_id: EventFamilyId,
    pub members: Vec<FamilyMemberExposure>,
    pub rollup: FamilyExposureRollup,
}

pub struct FamilyMemberExposure {
    pub condition_id: ConditionId,
    pub token_id: TokenId,
    pub outcome_label: String,
    pub bucket_quantities: BTreeMap<InventoryBucket, Decimal>,
}
```

- [ ] **Step 4: Add a persistence-backed foundation summary without changing the current replay CLI**

```rust
pub struct NegRiskFoundationSummary {
    pub discovered_family_count: u64,
    pub validated_family_count: u64,
    pub excluded_family_count: u64,
    pub halted_family_count: u64,
    pub recent_validation_event_count: u64,
    pub latest_discovery_revision: i64,
}
```

The summary helper should combine:
- current validation and halt rows from persistence
- the latest successful `neg_risk_discovery_snapshot` event from `event_journal` as the authoritative source for `discovered_family_count`
- recent `family_validation` and `family_halt` event counts from `event_journal`
- enough metadata to explain why a family is excluded right now
- the metadata snapshot hash that the current halt state applies to, if any
- a path to recover the exact ordered member vector from the corresponding `family_validation` or `family_halt` journal entry

- [ ] **Step 5: Run the strategy and app-replay tests**

Run: `cargo test -p strategy-negrisk exposure && DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-replay`

Expected: PASS.

- [ ] **Step 6: Add a cross-crate foundation contract test**

```rust
#[tokio::test]
async fn paginated_discovery_to_validation_to_summary_stays_explainable() {
    let metadata = fetch_sample_paginated_metadata().await;
    let graph = build_family_graph(sample_identifier_records(), metadata).unwrap();
    let verdict = validate_family(&graph.families()[0], 7, "sha256:snapshot-a");
    persist_validation_and_summary_inputs(&pool, &verdict).await.unwrap();

    let summary = load_neg_risk_foundation_summary(&pool).await.unwrap();
    assert_eq!(summary.excluded_family_count, 1);
    assert_eq!(summary.latest_discovery_revision, 7);
}
```

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-replay negrisk_foundation_contract -- --exact`

Expected: PASS. This test must cover:
- paginated raw metadata fetch
- graph build
- excluded-family validator verdict
- persistence upsert
- replay or summary explainability
- exact ordered family member vector round-trip from decision-time journal payload
- failed refresh not replacing the last successful current view

- [ ] **Step 7: Commit**

```bash
git add crates/strategy-negrisk crates/app-replay
git commit -m "feat: add neg-risk exposure reconstruction and replay summaries"
```

## Task 6: Add Family Metrics And Finish Docs

**Files:**
- Modify: `crates/observability/src/metrics.rs`
- Modify: `crates/observability/src/lib.rs`
- Modify: `/Users/viv/projs/axiom-arb/README.md`
- Test: `crates/observability/tests/metrics.rs`

- [ ] **Step 1: Write the failing metrics test**

```rust
#[test]
fn runtime_metrics_expose_neg_risk_family_counts() {
    let metrics = RuntimeMetrics::default();
    assert_eq!(
        metrics.neg_risk_family_included_count.key(),
        MetricKey::new("axiom_neg_risk_family_included_count")
    );
}
```

- [ ] **Step 2: Run the failing observability test**

Run: `cargo test -p observability runtime_metrics_expose_neg_risk_family_counts -- --exact`

Expected: FAIL because the metrics do not exist yet.

- [ ] **Step 3: Implement family-level metrics and docs**

```rust
pub struct RuntimeMetrics {
    pub neg_risk_family_discovered_count: GaugeHandle,
    pub neg_risk_family_included_count: GaugeHandle,
    pub neg_risk_family_excluded_count: GaugeHandle,
    pub neg_risk_family_halt_count: GaugeHandle,
    pub neg_risk_metadata_refresh_count: CounterHandle,
}
```

Metric source rules:
- `neg_risk_family_discovered_count` comes from the latest successful `neg_risk_discovery_snapshot` event
- `neg_risk_family_included_count` comes from current validation rows with `Included`
- `neg_risk_family_excluded_count` comes from current validation rows with `Excluded`
- `neg_risk_family_halt_count` comes from current active halt rows
- `neg_risk_metadata_refresh_count` increments once per completed successful refresh attempt

- [ ] **Step 4: Update the README to keep scope honest**

Document:
- `v1b foundation` is present as library/replay support
- `v1b live` is not yet implemented
- `app-live` still does not place `neg-risk` orders
- family halt precedence: `GlobalHalt > family halt > market-local halt > strategy-local filter`
- foundation-phase family halt blocks new `neg-risk` activation but does not override bootstrap `CancelOnly` or global emergency controls

- [ ] **Step 5: Run final targeted verification**

Run: `cargo fmt --all`

Expected: PASS.

Run: `cargo clippy --workspace --all-targets -- -D warnings`

Expected: PASS.

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p domain -p state -p venue-polymarket -p persistence -p strategy-negrisk -p app-replay -p observability`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add README.md crates/observability
git commit -m "docs: close out v1b foundation plan surface"
```

## Execution Notes

- Keep `v1b foundation` reviewable in isolation; do not mix in `app-live` venue-loop wiring.
- Reuse the current identifier map as the authoritative bridge. Do not create a parallel family graph store that can drift from `identifier_map`.
- When testing HTTP metadata calls, use a local listener-backed mock server. Do not rely on fixture-only tests or external endpoints.
- Keep raw discovery separate from validator exclusions. The fetch layer discovers and classifies; the validator decides inclusion or exclusion.
- Use member-level exposure vectors as the canonical foundation shape. Rollups are derived views, not the only stored or returned shape.
- Include pagination, completeness, dedupe, retry, and metadata-refresh tests in the venue discovery slice.
- Treat `discovery_revision` and `metadata_snapshot_hash` as different concepts:
  - `discovery_revision` orders successful refreshes
  - `metadata_snapshot_hash` identifies canonical content equality and stale-halt checks
- Treat augmented `neg-risk`, placeholder outcomes, and direct `Other` trading as explicit exclusions until a later spec changes that rule.
