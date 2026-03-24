# AxiomArb V1b Neg-Risk Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `v1b foundation` needed for standard Polymarket `neg-risk` families: discovery, structure validation, family-level exposure reconstruction, replay visibility, and family-scoped controls, without starting live `neg-risk` execution.

**Architecture:** Extend the existing `v1a` workspace instead of creating a parallel engine. Keep `neg-risk` foundation split across venue metadata ingestion, domain/persistence, a dedicated `strategy-negrisk` crate, and replay/observability hooks so the later live rollout can reuse the same identifier, journal, and state layers.

**Tech Stack:** Rust, Tokio, Serde, Reqwest, SQLx, PostgreSQL, `rust_decimal`, Tracing, Prometheus, local listener-backed HTTP test servers

---

## Scope Boundary

This plan intentionally covers `v1b foundation` only.

- In scope: standard `neg-risk` family discovery, family graph construction, structure validation, placeholder and `Other` exclusion, family-level exposure reconstruction from existing state/persistence data, replay summaries, family metrics, family-scoped halt primitives.
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
  Responsibility: `NegRiskFamily`, family nodes, validator results, exclusion reasons, family exposure, family halt state.
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
  Responsibility: parse market and event metadata needed to discover standard `neg-risk` families and classify placeholder or `Other` outcomes.
- Modify: `crates/venue-polymarket/src/rest.rs`
  Responsibility: add request builders and fetchers for `neg-risk` discovery metadata.
- Modify: `crates/venue-polymarket/src/lib.rs`
  Responsibility: export metadata types and fetch helpers.
- Test: `crates/venue-polymarket/tests/metadata.rs`
  Responsibility: use a real local listener for mock HTTP responses.

### Persistence

- Create: `migrations/0004_neg_risk_foundation.sql`
  Responsibility: persist family validation state and operator-facing family halt settings without duplicating identifier-map truth.
- Modify: `crates/persistence/src/models.rs`
  Responsibility: row models for family validation state and family halt state.
- Modify: `crates/persistence/src/repos.rs`
  Responsibility: repositories to upsert/list family validation and halt rows.
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
  Responsibility: aggregate inventory buckets by family-level exposure using the existing state snapshot surface.
- Test: `crates/strategy-negrisk/tests/graph_and_validator.rs`
- Test: `crates/strategy-negrisk/tests/exposure.rs`
- Create: `crates/app-replay/src/negrisk_summary.rs`
- Modify: `crates/app-replay/src/lib.rs`
  Responsibility: export a persistence-backed `neg-risk` foundation summary helper while keeping existing generic `event_journal` replay behavior unchanged.
- Test: `crates/app-replay/tests/negrisk_summary.rs`

### Observability

- Modify: `crates/observability/src/metrics.rs`
  Responsibility: family discovery, valid-family count, excluded-family count, family halt count.
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
fn standard_family_rejects_placeholder_and_other_outcomes() {
    let family = sample_family(["Alice", "Bob", "Other"]);
    let verdict = family.validate_standard_scope();
    assert_eq!(verdict.status, FamilyValidationStatus::Excluded);
    assert_eq!(verdict.reason, Some(FamilyExclusionReason::OtherOutcome));
}

#[test]
fn exposure_reconstruction_only_counts_free_and_reserved_buckets() {
    let exposure = NegRiskFamilyExposure::from_inventory(sample_inventory());
    assert_eq!(exposure.net_open_positions, 2);
}

#[test]
fn state_store_exposes_inventory_snapshot_without_mutable_access() {
    let mut store = StateStore::new();
    store.record_local_inventory(TokenId::from("token-a"), InventoryBucket::Free, dec!(2));
    assert_eq!(store.inventory_snapshot().len(), 1);
}
```

- [ ] **Step 2: Run the domain test to verify failure**

Run: `cargo test -p domain standard_family_rejects_placeholder_and_other_outcomes -- --exact`

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
    MissingNamedOutcomes,
    NonNegRiskRoute,
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

- [ ] **Step 5: Run the domain and state smoke tests**

Run: `cargo test -p domain && cargo test -p state`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/domain crates/state
git commit -m "feat: add neg-risk domain and state boundaries"
```

## Task 2: Add Polymarket Neg-Risk Metadata Discovery With Local Listener Tests

**Files:**
- Create: `crates/venue-polymarket/src/metadata.rs`
- Modify: `crates/venue-polymarket/src/rest.rs`
- Modify: `crates/venue-polymarket/src/lib.rs`
- Test: `crates/venue-polymarket/tests/metadata.rs`

- [ ] **Step 1: Write the failing metadata tests**

```rust
#[tokio::test]
async fn fetch_neg_risk_metadata_filters_to_named_standard_families() {
    let server = spawn_local_listener(sample_neg_risk_payload());
    let client = test_client(server.base_url());

    let families = client.fetch_neg_risk_metadata().await.unwrap();

    assert_eq!(families.len(), 1);
    assert!(families[0].all_outcomes_named());
}
```

- [ ] **Step 2: Run the failing venue test**

Run: `cargo test -p venue-polymarket fetch_neg_risk_metadata_filters_to_named_standard_families -- --exact`

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
    pub is_placeholder: bool,
    pub is_other: bool,
}
```

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

## Task 3: Persist Family Validation And Halt State

**Files:**
- Create: `migrations/0004_neg_risk_foundation.sql`
- Modify: `crates/persistence/src/models.rs`
- Modify: `crates/persistence/src/repos.rs`
- Modify: `crates/persistence/src/lib.rs`
- Test: `crates/persistence/tests/negrisk.rs`

- [ ] **Step 1: Write the failing persistence test**

```rust
#[tokio::test]
async fn stores_family_validation_without_duplicating_identifier_rows() {
    let pool = test_db_pool().await;
    run_migrations(&pool).await.unwrap();

    NegRiskFamilyRepo
        .upsert_validation(&pool, &sample_validation("family-1"))
        .await
        .unwrap();

    let rows = NegRiskFamilyRepo.list_validations(&pool).await.unwrap();
    assert_eq!(rows.len(), 1);
}
```

- [ ] **Step 2: Run the failing persistence test**

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence stores_family_validation_without_duplicating_identifier_rows -- --exact`

Expected: FAIL because the migration and repositories do not exist yet.

- [ ] **Step 3: Add the new migration**

```sql
CREATE TABLE neg_risk_family_validations (
  event_family_id TEXT PRIMARY KEY,
  validation_status TEXT NOT NULL,
  exclusion_reason TEXT,
  validated_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE family_halt_settings (
  event_family_id TEXT PRIMARY KEY,
  halted BOOLEAN NOT NULL,
  reason TEXT
);
```

- [ ] **Step 4: Implement repository and row conversions**

```rust
pub struct NegRiskFamilyRepo;

impl NegRiskFamilyRepo {
    pub async fn upsert_validation(&self, pool: &PgPool, row: &NegRiskFamilyValidationRow) -> Result<()>;
    pub async fn list_validations(&self, pool: &PgPool) -> Result<Vec<NegRiskFamilyValidationRow>>;
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
    assert_eq!(graph.families().len(), 1);
}

#[test]
fn validator_excludes_augmented_or_other_families_from_initial_scope() {
    let verdict = validate_family(sample_augmented_family());
    assert_eq!(verdict.status, FamilyValidationStatus::Excluded);
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
pub fn validate_family(family: &NegRiskFamily) -> FamilyValidation {
    // reject non-neg-risk routes, placeholder outcomes, direct Other trading
}
```

- [ ] **Step 5: Run the strategy test suite**

Run: `cargo test -p strategy-negrisk`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/strategy-negrisk Cargo.toml
git commit -m "feat: add neg-risk graph and validator foundation"
```

## Task 5: Reconstruct Bucket-Based Family Exposure And Add Foundation Summaries

**Files:**
- Create: `crates/strategy-negrisk/src/exposure.rs`
- Create: `crates/app-replay/src/negrisk_summary.rs`
- Modify: `crates/app-replay/src/lib.rs`
- Test: `crates/strategy-negrisk/tests/exposure.rs`
- Test: `crates/app-replay/tests/negrisk_summary.rs`

- [ ] **Step 1: Write the failing exposure and foundation-summary tests**

```rust
#[test]
fn family_exposure_groups_inventory_by_family_and_bucket_role() {
    let exposure = reconstruct_family_exposure(sample_family(), sample_inventory_rows());
    assert_eq!(exposure.family_id.as_str(), "family-1");
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

- [ ] **Step 3: Implement bucket-based family exposure reconstruction**

```rust
pub struct FamilyExposure {
    pub family_id: EventFamilyId,
    pub free_inventory: Decimal,
    pub reserved_inventory: Decimal,
    pub quarantined_inventory: Decimal,
}
```

- [ ] **Step 4: Add a persistence-backed foundation summary without changing the current replay CLI**

```rust
pub struct NegRiskFoundationSummary {
    pub validated_family_count: u64,
    pub excluded_family_count: u64,
    pub halted_family_count: u64,
}
```

- [ ] **Step 5: Run the strategy and app-replay tests**

Run: `cargo test -p strategy-negrisk exposure && DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-replay`

Expected: PASS.

- [ ] **Step 6: Commit**

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
        metrics.neg_risk_family_count.key(),
        MetricKey::new("axiom_neg_risk_family_count")
    );
}
```

- [ ] **Step 2: Run the failing observability test**

Run: `cargo test -p observability runtime_metrics_expose_neg_risk_family_counts -- --exact`

Expected: FAIL because the metrics do not exist yet.

- [ ] **Step 3: Implement family-level metrics and docs**

```rust
pub struct RuntimeMetrics {
    pub neg_risk_family_count: GaugeHandle,
    pub neg_risk_family_excluded_count: GaugeHandle,
    pub neg_risk_family_halt_count: GaugeHandle,
}
```

- [ ] **Step 4: Update the README to keep scope honest**

Document:
- `v1b foundation` is present as library/replay support
- `v1b live` is not yet implemented
- `app-live` still does not place `neg-risk` orders

- [ ] **Step 5: Run final targeted verification**

Run: `cargo fmt --all`

Expected: PASS.

Run: `cargo clippy --workspace --all-targets -- -D warnings`

Expected: PASS.

Run: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p domain -p venue-polymarket -p persistence -p strategy-negrisk -p app-replay -p observability`

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
- Keep family exposure bucket-based in this phase. If reservation-aware family exposure needs additional state surfaces, defer that to the live `v1b` rollout plan instead of widening this foundation slice.
- Treat augmented `neg-risk`, placeholder outcomes, and direct `Other` trading as explicit exclusions until a later spec changes that rule.
