# AxiomArb Smoke Cold-Start Bootstrap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make fresh-database `real-user shadow smoke` cold start truthful and complete by adding a first-class pre-adoption `discover` phase, fixing adoptable/adoption semantics, and letting `bootstrap` orchestrate the full Day 0 path without inventing a second startup authority.

**Architecture:** Keep `[negrisk.target_source].operator_target_revision` as the only startup authority. Move candidate/adoptable materialization onto an explicit pre-authority discovery path that reuses the existing authoritative publication chain, then teach `targets adopt`, `status`, `apply`, and `bootstrap` to operate on the new `discovery-required -> discovery-ready-not-adoptable -> adoptable-ready -> target-adopted` lifecycle without hiding state transitions behind vague target-anchor errors.

**Tech Stack:** Rust, `clap`, `app-live` command modules, `app-live` supervisor/runtime pipeline, `venue-polymarket` metadata refresh, `config-schema`, `sqlx`/Postgres integration tests, `cargo test`, `cargo clippy`

---

## File Map

### New files

- `crates/app-live/src/commands/discover.rs`
  - Low-level `app-live discover` command entrypoint, config loading, one-shot discovery orchestration, and operator-facing summary output.
- `crates/app-live/tests/discover_command.rs`
  - CLI-facing integration tests for the new `discover` command and its authoritative materialization behavior.
- `crates/app-live/tests/support/discover_db.rs`
  - Dedicated Postgres schema helper for discover/bootstrap cold-start scenarios.
- `docs/superpowers/plans/2026-04-05-axiomarb-smoke-cold-start-bootstrap.md`
  - This execution plan.

### Existing files to modify

- `crates/app-live/src/cli.rs`
  - Add `DiscoverArgs` and wire the new top-level subcommand.
- `crates/app-live/src/main.rs`
  - Route `AppLiveCommand::Discover`.
- `crates/app-live/src/commands/mod.rs`
  - Export the new `discover` module.
- `crates/app-live/src/discovery.rs`
  - Fix `CandidateBridge` identity semantics, derive the future `operator_target_revision` from rendered live targets, and stop pre-writing provenance during discovery.
- `crates/app-live/src/commands/targets/state.rs`
  - Resolve adoption directly from adoptable artifacts, add shared recommended-revision selection, and expose richer catalog helpers for `status`, `apply`, `bootstrap`, and `targets candidates`.
- `crates/app-live/src/commands/targets/adopt.rs`
  - Write canonical provenance during adopt, preserve existing canonical linkage for repeated identical startup authority, and always append per-adoption history.
- `crates/app-live/src/commands/targets/candidates.rs`
  - Render recommended adoptable output and non-adoptable disposition summaries from persisted artifacts.
- `crates/app-live/src/commands/status/model.rs`
  - Add `discovery-required`, `discovery-ready-not-adoptable`, and `adoptable-ready` readiness vocabulary plus matching next-action enums.
- `crates/app-live/src/commands/status/evaluate.rs`
  - Derive the new readiness states from config plus persisted discovery/adoptable artifacts without forcing startup target resolution first.
- `crates/app-live/src/commands/status/mod.rs`
  - Render new readiness labels and next actions, including truthful low-level guidance.
- `crates/app-live/src/commands/apply/model.rs`
  - Stop collapsing all pre-adoption failure into `ensure-target-anchor`; keep `apply` Day 1+ only.
- `crates/app-live/src/commands/apply/mod.rs`
  - Inline adopt only for `adoptable-ready`, never trigger discovery, and stop truthfully for `discovery-required` / `discovery-ready-not-adoptable`.
- `crates/app-live/src/commands/bootstrap/error.rs`
  - Remove the current half-implemented smoke follow-through guidance and replace it with truthful Day 0 discover/adopt states.
- `crates/app-live/src/commands/bootstrap/flow.rs`
  - Orchestrate `discover -> recommend -> explicit adopt confirmation -> adopt -> doctor -> rollout`.
- `crates/app-live/src/commands/bootstrap/output.rs`
  - Render discovery summaries, recommended revision, and “waiting for explicit adoption confirmation” / “no adoptable revisions produced” outcomes.
- `crates/app-live/src/commands/bootstrap/prompt.rs`
  - Show recommendation while still requiring manual selection/confirmation.
- `crates/app-live/src/task_groups.rs`
  - Add the smallest reusable one-shot metadata-to-input conversion helpers needed by `discover`; do not embed raw HTTP in command code.
- `crates/app-live/src/source_tasks.rs`
  - Expose or extend source bundle construction so `discover` can reuse the same Polymarket source configuration as `run`, without startup target resolution.
- `crates/app-live/src/supervisor.rs`
  - Extract a reusable pre-authority publication/materialization path from the current runtime/discovery flow.
- `crates/app-live/src/daemon.rs`
  - If needed, add a thin reusable runner for one-shot discovery materialization so `discover` and `bootstrap` do not fork logic.
- `crates/venue-polymarket/src/metadata.rs`
  - Reuse existing metadata refresh instead of inventing a new raw API path; expose only the smallest additional helper needed to drive one-shot authoritative discovery inputs.
- `crates/app-live/tests/discovery_supervisor.rs`
  - Lock new bridge semantics and no-provenance-on-discovery behavior.
- `crates/app-live/tests/candidate_daemon.rs`
  - Lock runtime/discovery materialization behavior on persisted candidate/adoptable artifacts without adopted authority.
- `crates/app-live/tests/targets_write_commands.rs`
  - Cover adopt-from-adoptable without preexisting provenance and repeated-adopt canonical-linkage semantics.
- `crates/app-live/tests/targets_read_commands.rs`
  - Cover richer `targets candidates` output and shared recommendation behavior.
- `crates/app-live/tests/status_command.rs`
  - Cover the new discovery-oriented readiness states and next actions.
- `crates/app-live/tests/apply_command.rs`
  - Cover truthful pre-adoption failure mapping and inline adopt only at `adoptable-ready`.
- `crates/app-live/tests/bootstrap_command.rs`
  - Add true Day 0 smoke cold-start integration coverage; this file is currently effectively empty.
- `crates/app-live/tests/support/mod.rs`
  - Export the new `discover_db` helper.
- `crates/venue-polymarket/tests/metadata.rs`
  - Add or reuse one-shot metadata-refresh test coverage if `discover` needs a small new metadata helper.
- `README.md`
  - Update smoke cold-start guidance to `bootstrap` Day 0 and `discover` low-level fallback.
- `docs/runbooks/real-user-shadow-smoke.md`
  - Replace the broken Day 0 target-anchor flow with `bootstrap -> discover -> adopt`.
- `docs/runbooks/operator-target-adoption.md`
  - Update readiness guidance and explain canonical provenance vs adoption history semantics.
- `docs/runbooks/bootstrap-and-ramp.md`
  - Update the adopted-target/operator workflow so fresh databases no longer assume preexisting adoptable revisions.

### Existing files to study before editing

- `docs/superpowers/specs/2026-04-05-axiomarb-smoke-cold-start-bootstrap-design.md`
- `docs/superpowers/specs/2026-03-28-axiomarb-phase3e-candidate-generation-design.md`
- `docs/superpowers/specs/2026-04-02-axiomarb-bootstrap-ux-design.md`
- `docs/superpowers/specs/2026-04-02-axiomarb-status-readiness-ux-design.md`
- `docs/superpowers/specs/2026-04-03-axiomarb-apply-flow-ux-design.md`
- `crates/app-live/src/commands/run.rs`
- `crates/app-live/src/startup.rs`
- `crates/app-live/src/runtime.rs`
- `crates/app-live/src/queues.rs`
- `crates/app-live/tests/support/cli.rs`

---

### Task 1: Fix Bridge Identity And Discovery Persistence Semantics

**Files:**
- Modify: `crates/app-live/src/discovery.rs`
- Modify: `crates/app-live/tests/discovery_supervisor.rs`
- Modify: `crates/app-live/tests/candidate_daemon.rs`
- Modify: `crates/app-live/src/config.rs` if a small visibility tweak is needed to reuse `neg_risk_live_target_revision_from_targets(...)`

- [ ] **Step 1: Write the failing bridge/discovery tests**

Add focused failing coverage for these contracts:

```rust
#[test]
fn candidate_bridge_derives_future_operator_target_revision_from_rendered_live_targets() {
    let bridge = CandidateBridge::for_tests();
    let targets = sample_rendered_live_targets();

    let render = bridge.render(&candidate_set, &targets).expect("candidate render");

    assert_eq!(
        render.adoptable.rendered_operator_target_revision,
        app_live::config::neg_risk_live_target_revision_from_targets(&targets)
    );
    assert_eq!(
        render.adoptable.payload["compatibility"]["operator_target_revision_supplied"],
        json!(false)
    );
}

#[test]
fn discovery_supervisor_materializes_adoptable_output_without_prior_operator_revision() {
    let notice = CandidateNotice::from_publication(
        &ready_candidate_publication(),
        [DirtyDomain::Candidates],
        None,
        sample_rendered_live_targets(),
        CandidateRestrictionTruth::eligible(),
    );

    let report = run_async(async {
        DiscoverySupervisor::for_tests(queue_with(notice))
            .tick_candidate_generation_for_tests()
            .await
            .expect("candidate generation report")
    });

    assert_eq!(report.adoptable_revision.as_deref(), Some("adoptable-candidate-pub-7"));
    assert!(report.operator_target_revision.is_some());
}
```

Also flip the current candidate-daemon expectation so candidate generation persists candidate/adoptable artifacts but not discovery-time provenance.

- [ ] **Step 2: Run the targeted tests to verify they fail**

Run:

```bash
cargo test -p app-live --test discovery_supervisor candidate_bridge_derives_future_operator_target_revision_from_rendered_live_targets -- --exact
cargo test -p app-live --test discovery_supervisor discovery_supervisor_materializes_adoptable_output_without_prior_operator_revision -- --exact
cargo test -p app-live --test candidate_daemon daemon_run_persists_candidate_artifacts_from_candidate_dirty_inputs -- --exact --test-threads=1
```

Expected:
- FAIL because `CandidateBridge::render` still requires a supplied operator revision.
- FAIL because `DiscoverySupervisor` still suppresses adoptable output when `operator_target_revision` is missing.
- FAIL because runtime candidate persistence still writes provenance during discovery.

- [ ] **Step 3: Implement the minimal bridge contract fix**

Change `CandidateBridge::render(...)` so it derives the future revision from canonical rendered live targets:

```rust
pub fn render(
    &self,
    candidate_set: &CandidateTargetSet,
    rendered_live_targets: &BTreeMap<String, NegRiskFamilyLiveTarget>,
) -> Result<CandidateArtifactRender, String> {
    let rendered_operator_target_revision =
        neg_risk_live_target_revision_from_targets(rendered_live_targets);
    // keep candidate/adoptable rendering otherwise unchanged
}
```

Implementation notes:
- Keep `rendered_operator_target_revision` byte-compatible with the existing target-content hash helper.
- Do not overload `candidate_revision` or `adoptable_revision` into the startup authority hash.
- Keep `AdoptableTargetRevision.payload["rendered_live_targets"]` intact so startup/adopt can validate it later.

- [ ] **Step 4: Stop discovery from writing provenance**

Update `DiscoverySupervisor::process_notice(...)` / `persist_notice_for_runtime(...)` so:

```rust
DiscoveryOutcome {
    report,
    candidate: Some(rendered.candidate),
    adoptable: Some(rendered.adoptable),
    provenance: None,
}
```

and remove the current “requires explicit rendered operator target revision” / discovery-time provenance assumptions.

- [ ] **Step 5: Run the targeted tests to verify they pass**

Run:

```bash
cargo test -p app-live --test discovery_supervisor -- --test-threads=1
cargo test -p app-live --test candidate_daemon daemon_run_persists_candidate_artifacts_from_candidate_dirty_inputs -- --exact --test-threads=1
```

Expected:
- PASS
- Candidate/adoptable artifacts persist
- No discovery-time provenance row is required or written

- [ ] **Step 6: Commit**

```bash
git add crates/app-live/src/discovery.rs crates/app-live/tests/discovery_supervisor.rs crates/app-live/tests/candidate_daemon.rs crates/app-live/src/config.rs
git commit -m "fix: decouple discovery from adopted target authority"
```

---

### Task 2: Make Adopt-By-Adoptable The Canonical Provenance Write Path

**Files:**
- Modify: `crates/app-live/src/commands/targets/state.rs`
- Modify: `crates/app-live/src/commands/targets/adopt.rs`
- Modify: `crates/app-live/tests/targets_write_commands.rs`
- Modify: `crates/app-live/tests/support/apply_db.rs` if new seed shapes are needed

- [ ] **Step 1: Write the failing adoption tests**

Add three focused tests:

```rust
#[test]
fn targets_adopt_from_fresh_adoptable_revision_writes_canonical_provenance_and_history() {
    let database = TestDatabase::new();
    database.seed_adoptable_revision_without_provenance(
        "adoptable-9",
        "candidate-9",
        "targets-rev-9",
    );

    let output = Command::new(app_live_binary())
        .arg("targets").arg("adopt")
        .arg("--config").arg(&config)
        .arg("--adoptable-revision").arg("adoptable-9")
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("targets adopt should run");

    assert!(output.status.success(), "{}", combined(&output));
    assert_eq!(
        database.provenance_for("targets-rev-9").unwrap().adoptable_revision,
        "adoptable-9"
    );
}

#[test]
fn targets_adopt_same_operator_target_revision_appends_history_without_rewriting_canonical_linkage() {
    // existing canonical provenance points at adoptable-7/candidate-7
    // newly adopted artifact renders the same targets-rev-9 but has adoptable-9/candidate-9
    // expectation: success, same canonical provenance, new history row appended
}

#[test]
fn targets_adopt_rejects_invalid_adoptable_payload_before_writing_history_or_config() {
    // preserve the current fail-closed behavior for malformed rendered_live_targets
}
```

- [ ] **Step 2: Run the targeted tests to verify they fail**

Run:

```bash
cargo test -p app-live --test targets_write_commands targets_adopt_from_fresh_adoptable_revision_writes_canonical_provenance_and_history -- --exact --test-threads=1
cargo test -p app-live --test targets_write_commands targets_adopt_same_operator_target_revision_appends_history_without_rewriting_canonical_linkage -- --exact --test-threads=1
```

Expected:
- FAIL because `resolve_adoption_selection` still requires preexisting provenance for `--adoptable-revision`.
- FAIL because repeated-adopt same-authority semantics are not implemented and `adopt_selected_revision` only records history when config changes.

- [ ] **Step 3: Resolve adoption directly from adoptable artifacts**

Change `resolve_adoption_selection(...)` so the adoptable-revision branch:

```rust
let adoptable = CandidateArtifactRepo
    .get_adoptable_target_revision(pool, adoptable_revision)
    .await?
    .ok_or_else(...)?;
validate_rendered_live_targets(&adoptable.payload, &adoptable.rendered_operator_target_revision)?;

ResolvedAdoptionSelection {
    operator_target_revision: adoptable.rendered_operator_target_revision.clone(),
    adoptable_revision: Some(adoptable.adoptable_revision.clone()),
    candidate_revision: Some(adoptable.candidate_revision.clone()),
}
```

Do not require existing provenance for this path.

- [ ] **Step 4: Write canonical provenance during adopt**

Update `adopt_selected_revision(...)` so the successful adopt transaction:

```rust
let canonical = CandidateAdoptionProvenanceRow {
    operator_target_revision: selection.operator_target_revision.clone(),
    adoptable_revision: selection.adoptable_revision.clone().expect("adopt path"),
    candidate_revision: selection.candidate_revision.clone().expect("adopt path"),
};

match CandidateAdoptionRepo.get_by_operator_target_revision(pool, &canonical.operator_target_revision).await? {
    None => CandidateAdoptionRepo.upsert_provenance(pool, &canonical).await?,
    Some(existing) => {
        // preserve existing canonical linkage when startup authority matches
    }
}

OperatorTargetAdoptionHistoryRepo.append(pool, &history_row).await?;
rewrite_operator_target_revision(config_path, &selection.operator_target_revision)?;
```

Required behavior:
- If no canonical provenance exists, create it now.
- If canonical provenance already exists for the same `operator_target_revision`, preserve it even if the new adoptable/candidate lineage differs.
- Still append a new history row for each explicit adopt action, including same-authority re-adopt.
- Keep malformed-payload failure closed.

- [ ] **Step 5: Run the targeted tests to verify they pass**

Run:

```bash
cargo test -p app-live --test targets_write_commands -- --test-threads=1
cargo test -p app-live --test apply_command apply_can_inline_smoke_target_adoption -- --exact --test-threads=1
```

Expected:
- PASS
- Inline `apply` adoption still works because it reuses the same helper

- [ ] **Step 6: Commit**

```bash
git add crates/app-live/src/commands/targets/state.rs crates/app-live/src/commands/targets/adopt.rs crates/app-live/tests/targets_write_commands.rs crates/app-live/tests/support/apply_db.rs
git commit -m "fix: make adopt write canonical target provenance"
```

---

### Task 3: Add A First-Class `discover` Command On The Authoritative Publication Path

**Files:**
- Modify: `crates/app-live/src/cli.rs`
- Modify: `crates/app-live/src/main.rs`
- Modify: `crates/app-live/src/commands/mod.rs`
- Create: `crates/app-live/src/commands/discover.rs`
- Modify: `crates/app-live/src/task_groups.rs`
- Modify: `crates/app-live/src/source_tasks.rs`
- Modify: `crates/app-live/src/supervisor.rs`
- Modify: `crates/app-live/src/daemon.rs` if a thin reusable one-shot runner is the smallest clean extraction
- Modify: `crates/venue-polymarket/src/metadata.rs` only if a tiny helper extraction is needed
- Create: `crates/app-live/tests/discover_command.rs`
- Create: `crates/app-live/tests/support/discover_db.rs`
- Modify: `crates/app-live/tests/support/mod.rs`
- Modify: `crates/venue-polymarket/tests/metadata.rs` if a small helper is extracted

- [ ] **Step 1: Write the failing CLI exposure test**

Create `crates/app-live/tests/discover_command.rs` with:

```rust
mod support;

use std::process::Command;
use support::cli;

#[test]
fn discover_subcommand_is_exposed() {
    let output = Command::new(cli::app_live_binary())
        .arg("discover")
        .arg("--help")
        .output()
        .expect("app-live discover --help should execute");

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("--config"), "{text}");
}
```

- [ ] **Step 2: Run the CLI test to verify it fails**

Run:

```bash
cargo test -p app-live --test discover_command discover_subcommand_is_exposed -- --exact --test-threads=1
```

Expected: FAIL because `discover` is not wired into the CLI yet.

- [ ] **Step 3: Add the minimal CLI plumbing**

Implement:

```rust
#[derive(clap::Args, Debug)]
pub struct DiscoverArgs {
    #[arg(long)]
    pub config: PathBuf,
}
```

and wire:
- `AppLiveCommand::Discover(DiscoverArgs)` in `crates/app-live/src/cli.rs`
- `pub mod discover;` in `crates/app-live/src/commands/mod.rs`
- dispatch in `crates/app-live/src/main.rs`
- temporary `execute(args: DiscoverArgs)` in `crates/app-live/src/commands/discover.rs`

- [ ] **Step 4: Write the failing end-to-end discover materialization test**

Add a real integration test that uses:
- a fresh schema from `support::discover_db::TestDatabase`
- a temp smoke config with local `polymarket.source_overrides`
- a local HTTP listener copied in the smallest possible form from `crates/venue-polymarket/tests/metadata.rs`

Lock this behavior:

```rust
#[test]
fn discover_materializes_candidate_and_adoptable_artifacts_from_smoke_config() {
    let output = Command::new(cli::app_live_binary())
        .arg("discover")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live discover should execute");

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("candidate_count = 1"), "{text}");
    assert!(text.contains("adoptable_count = 1"), "{text}");
    assert!(text.contains("recommended_adoptable_revision = "), "{text}");

    assert!(database.has_candidate_rows());
    assert!(database.has_adoptable_rows());
    assert!(!database.has_candidate_provenance_rows());
}
```

- [ ] **Step 5: Run the integration test to verify it fails**

Run:

```bash
cargo test -p app-live --test discover_command discover_materializes_candidate_and_adoptable_artifacts_from_smoke_config -- --exact --test-threads=1
```

Expected: FAIL because there is no first-class discover path yet.

- [ ] **Step 6: Extract the one-shot authoritative discovery path**

Implement the smallest clean reusable path that:
- uses existing Polymarket source config and signer loading from the smoke config
- reuses `venue-polymarket` metadata refresh instead of inventing raw HTTP
- converts metadata refresh output into authoritative `InputTaskEvent::family_discovery_observed(...)` inputs
- applies those inputs through the existing runtime/supervisor publication path
- materializes `CandidatePublication -> CandidateNotice -> CandidateTargetSet / AdoptableTargetRevision`
- never calls `resolve_startup_targets`
- never requires an adopted `operator_target_revision`

Target shape:

```rust
pub fn execute(args: DiscoverArgs) -> Result<(), Box<dyn Error>> {
    let summary = run_discover_from_config(&args.config)?;
    render_discover_summary(&summary);
    Ok(())
}
```

Recommended implementation seam:
- add a small one-shot helper in `task_groups.rs` that groups refreshed metadata rows into distinct family discovery inputs
- add a small runner in `supervisor.rs` or `daemon.rs` that applies those inputs and persists candidate/adoptable artifacts through the existing `DiscoverySupervisor`
- keep `commands/discover.rs` as orchestration only

Do not:
- materialize candidate/adoptable rows directly from metadata rows
- shell out to `app-live run`
- duplicate REST logic in `app-live`

- [ ] **Step 7: Run the discover tests to verify they pass**

Run:

```bash
cargo test -p app-live --test discover_command -- --test-threads=1
cargo test -p venue-polymarket --test metadata -- --test-threads=1
```

Expected:
- PASS
- The only new `venue-polymarket` surface is a small helper extraction, if any

- [ ] **Step 8: Commit**

```bash
git add crates/app-live/src/cli.rs crates/app-live/src/main.rs crates/app-live/src/commands/mod.rs crates/app-live/src/commands/discover.rs crates/app-live/src/task_groups.rs crates/app-live/src/source_tasks.rs crates/app-live/src/supervisor.rs crates/app-live/src/daemon.rs crates/venue-polymarket/src/metadata.rs crates/app-live/tests/discover_command.rs crates/app-live/tests/support/discover_db.rs crates/app-live/tests/support/mod.rs crates/venue-polymarket/tests/metadata.rs
git commit -m "feat: add authoritative smoke discovery command"
```

---

### Task 4: Rework Readiness, Recommendation, And `targets candidates` Output

**Files:**
- Modify: `crates/app-live/src/commands/targets/state.rs`
- Modify: `crates/app-live/src/commands/targets/candidates.rs`
- Modify: `crates/app-live/src/commands/status/model.rs`
- Modify: `crates/app-live/src/commands/status/evaluate.rs`
- Modify: `crates/app-live/src/commands/status/mod.rs`
- Modify: `crates/app-live/src/commands/apply/model.rs`
- Modify: `crates/app-live/src/commands/apply/mod.rs`
- Modify: `crates/app-live/tests/targets_read_commands.rs`
- Modify: `crates/app-live/tests/status_command.rs`
- Modify: `crates/app-live/tests/apply_command.rs`

- [ ] **Step 1: Write the failing readiness and recommendation tests**

Add/extend tests for:

```rust
#[test]
fn status_smoke_without_discovery_artifacts_is_discovery_required() { /* ... */ }

#[test]
fn status_smoke_with_advisory_only_candidate_is_discovery_ready_not_adoptable() { /* ... */ }

#[test]
fn status_smoke_with_adoptable_artifacts_and_no_anchor_is_adoptable_ready() { /* ... */ }

#[test]
fn apply_maps_discovery_required_to_truthful_next_action() { /* ... */ }

#[test]
fn apply_does_not_inline_discovery_when_only_advisory_candidates_exist() { /* ... */ }

#[test]
fn targets_candidates_prints_recommended_adoptable_and_non_adoptable_summary() { /* ... */ }
```

- [ ] **Step 2: Run the targeted tests to verify they fail**

Run:

```bash
cargo test -p app-live --test status_command -- --test-threads=1
cargo test -p app-live --test apply_command apply_maps_discovery_required_to_truthful_next_action -- --exact --test-threads=1
cargo test -p app-live --test targets_read_commands -- --test-threads=1
```

Expected:
- FAIL because readiness still collapses into `target-adoption-required`
- FAIL because `apply` still returns `ensure-target-anchor`
- FAIL because `targets candidates` has no recommendation/disposition summary

- [ ] **Step 3: Add the shared catalog/recommendation helper**

In `crates/app-live/src/commands/targets/state.rs`, add a deterministic helper over persisted artifacts:

```rust
pub fn recommended_adoptable_revision(
    catalog: &TargetCandidatesCatalog,
) -> Option<&AdoptableTargetRevisionRow> {
    catalog.adoptable_revisions.first()
}
```

and, if needed, a small summary struct that exposes:
- advisory count
- adoptable count
- deferred/excluded counts derived from candidate/adoptable payloads

Keep the selector stable and shared. Do not let `discover`, `bootstrap`, and `targets candidates` invent separate “recommended” logic.

- [ ] **Step 4: Implement the new status readiness model**

Replace the pre-adoption catch-all with:

```rust
pub enum StatusReadiness {
    PaperReady,
    DiscoveryRequired,
    DiscoveryReadyNotAdoptable,
    AdoptableReady,
    RestartRequired,
    SmokeRolloutRequired,
    SmokeConfigReady,
    LiveRolloutRequired,
    LiveConfigReady,
    Blocked,
}
```

Implementation notes:
- For adopted-target smoke/live configs with no configured operator target revision:
  - no candidate/adoptable artifacts -> `discovery-required`
  - advisory-only candidate artifacts -> `discovery-ready-not-adoptable`
  - adoptable artifacts present -> `adoptable-ready`
- Do not call `resolve_startup_targets` until there is an adopted authority to resolve.
- Keep paper behavior unchanged.

- [ ] **Step 5: Make `apply` truthful and bounded**

Update `ApplyFailureKind::from_status_readiness(...)` and `execute_smoke_apply(...)` so:
- `discovery-required` stops with concrete next action pointing to `bootstrap` or `discover`
- `discovery-ready-not-adoptable` stops with reasons and does not try to prompt
- `adoptable-ready` is the only pre-adoption state that may enter inline adoption
- `apply` never runs discovery itself

- [ ] **Step 6: Update `targets candidates` output**

Render something like:

```text
advisory_count = 1
adoptable_count = 2
recommended_adoptable_revision = adoptable-9
non_adoptable_summary = deferred:0 excluded:1
```

Keep existing per-row output, but add the shared recommendation and summary lines so operators are not stuck with `adoptable = none` as the only signal.

- [ ] **Step 7: Run the targeted tests to verify they pass**

Run:

```bash
cargo test -p app-live --test status_command -- --test-threads=1
cargo test -p app-live --test apply_command -- --test-threads=1
cargo test -p app-live --test targets_read_commands -- --test-threads=1
```

Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add crates/app-live/src/commands/targets/state.rs crates/app-live/src/commands/targets/candidates.rs crates/app-live/src/commands/status/model.rs crates/app-live/src/commands/status/evaluate.rs crates/app-live/src/commands/status/mod.rs crates/app-live/src/commands/apply/model.rs crates/app-live/src/commands/apply/mod.rs crates/app-live/tests/targets_read_commands.rs crates/app-live/tests/status_command.rs crates/app-live/tests/apply_command.rs
git commit -m "feat: add discovery-aware smoke readiness states"
```

---

### Task 5: Rework `bootstrap` Into The Real Day 0 Orchestrator

**Files:**
- Modify: `crates/app-live/src/commands/bootstrap/error.rs`
- Modify: `crates/app-live/src/commands/bootstrap/flow.rs`
- Modify: `crates/app-live/src/commands/bootstrap/output.rs`
- Modify: `crates/app-live/src/commands/bootstrap/prompt.rs`
- Modify: `crates/app-live/src/commands/bootstrap.rs`
- Modify: `crates/app-live/src/commands/discover.rs` if shared internal helpers should be reused
- Modify: `crates/app-live/tests/bootstrap_command.rs`
- Modify: `crates/app-live/tests/discover_command.rs` only if a shared cold-start harness is extracted

- [ ] **Step 1: Write the failing Day 0 bootstrap tests**

Populate `crates/app-live/tests/bootstrap_command.rs` with real CLI integration tests:

```rust
#[test]
fn bootstrap_smoke_empty_database_runs_discover_then_waits_for_explicit_adoption_confirmation() {
    let output = run_bootstrap_with_stdin(
        &config_path,
        database.database_url(),
        "smoke\n...\nadoptable-9\npreflight-only\n",
    );

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("Discovery completed"), "{text}");
    assert!(text.contains("Recommended:"), "{text}");
    assert!(text.contains("Waiting for explicit adoption confirmation"), "{text}");
}

#[test]
fn bootstrap_smoke_without_adoptables_stops_at_discovery_ready_not_adoptable() {
    // same flow, but mocked metadata produces advisory-only candidate output
    // assert no follow-up text tells the operator to run targets adopt yet
}

#[test]
fn bootstrap_smoke_multiple_adoptables_marks_recommended_but_requires_manual_choice() {
    // assert all revisions are shown and one is marked recommended
}
```

- [ ] **Step 2: Run the bootstrap tests to verify they fail**

Run:

```bash
cargo test -p app-live --test bootstrap_command -- --test-threads=1
```

Expected: FAIL because bootstrap still emits the half-implemented smoke follow-through error.

- [ ] **Step 3: Replace the half-implemented smoke follow-up**

Update `bootstrap/flow.rs` so the smoke path becomes:

```rust
config creation/reuse
-> discover if artifacts are missing
-> inspect persisted catalog
-> show recommended adoptable revision
-> explicit operator selection/confirmation
-> adopt
-> doctor
-> rollout confirmation
-> optional --start
```

Required behavior:
- Empty DB + no artifacts -> run discover inline
- Advisory-only result -> stop with truthful discovery summary and next actions
- Adoptable result -> prompt with recommendation but require manual confirmation
- Already-adopted path -> skip straight to doctor/rollout as today

- [ ] **Step 4: Update bootstrap output/error text**

Replace the current `SmokeConfigCompletionOnly` guidance with truthful outcomes such as:

```text
Discovery completed
Adoptable revisions: adoptable-9, adoptable-8
Recommended: adoptable-9
Waiting for explicit adoption confirmation
```

or:

```text
Discovery completed but no adoptable revisions were produced
Reasons: deferred=1 excluded=0
Next: rerun app-live discover --config ...
```

Do not keep the old `targets candidates ; targets adopt` fallback text for empty DB cold start.

- [ ] **Step 5: Run the targeted tests to verify they pass**

Run:

```bash
cargo test -p app-live --test bootstrap_command -- --test-threads=1
cargo test -p app-live --test discover_command -- --test-threads=1
```

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/app-live/src/commands/bootstrap/error.rs crates/app-live/src/commands/bootstrap/flow.rs crates/app-live/src/commands/bootstrap/output.rs crates/app-live/src/commands/bootstrap/prompt.rs crates/app-live/src/commands/bootstrap.rs crates/app-live/tests/bootstrap_command.rs crates/app-live/src/commands/discover.rs
git commit -m "feat: make bootstrap the smoke cold-start happy path"
```

---

### Task 6: Update Runbooks And Verify The Whole Slice

**Files:**
- Modify: `README.md`
- Modify: `docs/runbooks/real-user-shadow-smoke.md`
- Modify: `docs/runbooks/operator-target-adoption.md`
- Modify: `docs/runbooks/bootstrap-and-ramp.md`

- [ ] **Step 1: Write the docs changes after code behavior is stable**

Update operator-facing docs so they no longer claim:
- fresh DB bootstrap can continue directly into `targets candidates` / `targets adopt`
- `apply` can inline adopt before any discovery artifacts exist
- discovery/adoption provenance are the same lifecycle fact

Required doc shape:
- Day 0 happy path: `bootstrap`
- Low-level fallback: `discover -> targets candidates -> targets adopt`
- Day 1+: `status` / `apply`

- [ ] **Step 2: Run focused verification**

Run:

```bash
cargo test -p app-live --test discovery_supervisor --test candidate_daemon --test targets_write_commands --test targets_read_commands --test discover_command --test status_command --test apply_command --test bootstrap_command -- --test-threads=1
cargo test -p venue-polymarket --test metadata -- --test-threads=1
cargo clippy -p app-live -p venue-polymarket --tests -- -D warnings
```

Expected:
- PASS across the new cold-start slice
- no new Clippy warnings

- [ ] **Step 3: Do one manual smoke-path sanity check**

On a clean local schema:

```bash
export DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb
cargo run -p app-live -- bootstrap --config config/axiom-arb.local.toml
```

Expected:
- empty-DB smoke bootstrap no longer exits with “follow-through is not implemented yet”
- bootstrap either shows a truthful discovery-not-adoptable summary or reaches adoptable selection

- [ ] **Step 4: Commit**

```bash
git add README.md docs/runbooks/real-user-shadow-smoke.md docs/runbooks/operator-target-adoption.md docs/runbooks/bootstrap-and-ramp.md
git commit -m "docs: update smoke cold-start operator guidance"
```

---

## Notes For The Implementer

- Execute this plan in a dedicated git worktree, not in a mixed branch.
- Keep `discover` orchestration-only. If command code starts building HTTP requests directly, stop and move that logic back behind `venue-polymarket` / `task_groups`.
- Keep startup authority semantics stable. If a change would make identical rendered targets produce different `operator_target_revision` values, stop and fix the design drift before proceeding.
- Do not let `apply` become a second Day 0 entrypoint. It may consume `adoptable-ready`, but it must not synthesize or refresh discovery state on its own.
- Prefer extending existing DB test helpers over building a giant all-purpose fixture. Small, scenario-specific helpers are easier to trust.
