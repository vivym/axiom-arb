# Operator Target Adoption UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a first-class `app-live targets ...` control-plane workflow so operators can inspect candidate/adoptable/current target state, adopt a new `operator_target_revision`, and roll back safely without querying raw tables or inventing a second startup authority.

**Architecture:** Keep `operator_target_revision` as the sole startup/restore authority, with the configured pointer living in `[negrisk.target_source].operator_target_revision` inside the operator TOML and the active pointer continuing to live in durable runtime progress. Build a small target-control command layer under `app-live`, backed by a persistence-side adoption-history surface, config rewrite helpers, and a shared read model that compares configured vs active state while preserving startup-scoped / restart-scoped semantics.

**Tech Stack:** Rust workspace (`app-live`, `persistence`, `config-schema`), Clap nested subcommands, Serde/TOML serialization, SQLx/Postgres migrations, Cargo integration tests, existing candidate/adoption provenance tables, structured operator-facing CLI output.

---

## File Map

- Create: `migrations/0012_operator_target_adoption_history.sql`
  - Add the durable adoption-history table used for rollback traversal and latest-action summaries.
- Create: `crates/persistence/tests/operator_target_adoption.rs`
  - Pin down history append/read/previous-distinct behavior with isolated schemas.
- Create: `crates/config-schema/tests/config_roundtrip.rs`
  - Verify config round-tripping and target-source rewrite safety.
- Create: `crates/app-live/src/commands/targets/mod.rs`
  - Own nested subcommand dispatch for the `targets` namespace.
- Create: `crates/app-live/src/commands/targets/config_file.rs`
  - Load, mutate, and persist `[negrisk.target_source].operator_target_revision` in operator TOML files.
- Create: `crates/app-live/src/commands/targets/state.rs`
  - Build the shared control-plane read model from config + runtime progress + provenance + adoption history.
- Create: `crates/app-live/src/commands/targets/status.rs`
  - Render the compact configured-vs-active summary.
- Create: `crates/app-live/src/commands/targets/show_current.rs`
  - Render the detailed provenance/explainability view.
- Create: `crates/app-live/src/commands/targets/candidates.rs`
  - Render advisory candidates vs adoptable revisions vs already-adopted revisions.
- Create: `crates/app-live/src/commands/targets/adopt.rs`
  - Implement `targets adopt --operator-target-revision/--adoptable-revision`.
- Create: `crates/app-live/src/commands/targets/rollback.rs`
  - Implement default previous-distinct rollback plus explicit `--to-operator-target-revision`.
- Create: `crates/app-live/tests/targets_read_commands.rs`
  - Exercise `targets status`, `targets show-current`, and `targets candidates`.
- Create: `crates/app-live/tests/targets_write_commands.rs`
  - Exercise `targets adopt` and `targets rollback` against temp configs and isolated persistence state.

- Modify: `crates/persistence/src/models.rs`
  - Add the adoption-history row type.
- Modify: `crates/persistence/src/repos.rs`
  - Add history repo methods and any small helper lookups needed by the control-plane read model.
- Modify: `crates/persistence/src/lib.rs`
  - Export the new repo/model/error surface.
- Modify: `crates/persistence/tests/migrations.rs`
  - Assert the new table exists after migrations.
- Modify: `crates/config-schema/src/raw.rs`
  - Derive `Serialize` for raw config structs so `app-live` can safely rewrite the operator config file.
- Modify: `crates/config-schema/src/lib.rs`
  - Export `render_raw_config_to_string` / save helpers alongside existing load helpers.
- Modify: `crates/config-schema/src/validate.rs`
  - Keep target-source views ergonomic for control-plane commands.
- Modify: `crates/app-live/src/cli.rs`
  - Add the nested `targets` subcommands and explicit selector flags.
- Modify: `crates/app-live/src/main.rs`
  - Dispatch the new namespace without bloating `main`.
- Modify: `crates/app-live/src/commands/mod.rs`
  - Re-export the new targets module.
- Modify: `crates/app-live/src/commands/doctor.rs`
  - Reuse the new control-plane read model so `doctor` reports configured-vs-active adoption readiness.
- Modify: `config/axiom-arb.example.toml`
  - Show the configured target pointer as the only startup anchor.
- Modify: `README.md`
  - Document the new `targets` workflow.
- Modify: `docs/runbooks/bootstrap-and-ramp.md`
  - Replace manual target-revision editing guidance with `targets adopt` / `targets rollback`.
- Modify: `docs/runbooks/real-user-shadow-smoke.md`
  - Explain how smoke mode interacts with adopted targets and restart-needed status.

## Implementation Notes

- Follow @superpowers:test-driven-development for each task: write the failing test first, then the minimum implementation, then re-run the targeted suite.
- Use @superpowers:verification-before-completion before branch completion or PR creation.
- Keep `operator_target_revision` as the only startup/restore anchor. This plan does **not** add hot reload or a second configured-state store.

## Scope Guard

This plan intentionally does **not**:

1. auto-adopt the latest candidate or adoptable revision
2. add a `targets history` browser
3. hot-reload a running daemon to a new target revision
4. move configured target state into Postgres or another new durable store
5. change candidate generation, ranking, or budget-aware selection

### Task 1: Add Durable Adoption History In `persistence`

**Files:**
- Create: `migrations/0012_operator_target_adoption_history.sql`
- Create: `crates/persistence/tests/operator_target_adoption.rs`
- Modify: `crates/persistence/src/models.rs`
- Modify: `crates/persistence/src/repos.rs`
- Modify: `crates/persistence/src/lib.rs`
- Modify: `crates/persistence/tests/migrations.rs`

- [ ] **Step 1: Write the failing migration and repo tests**

```rust
#[tokio::test]
async fn migrations_create_operator_target_adoption_history_table() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    assert!(table_exists(&db.pool, "operator_target_adoption_history").await);
    db.cleanup().await;
}

#[tokio::test]
async fn adoption_history_returns_previous_distinct_operator_target_revision() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let repo = OperatorTargetAdoptionHistoryRepo;
    repo.append(&db.pool, &sample_adoption("targets-rev-1", None)).await.unwrap();
    repo.append(&db.pool, &sample_adoption("targets-rev-2", Some("targets-rev-1"))).await.unwrap();
    repo.append(&db.pool, &sample_adoption("targets-rev-2", Some("targets-rev-2"))).await.unwrap();

    let previous = repo.previous_distinct_revision(&db.pool, "targets-rev-2").await.unwrap();
    assert_eq!(previous.as_deref(), Some("targets-rev-1"));

    db.cleanup().await;
}
```

- [ ] **Step 2: Run the targeted persistence tests to verify they fail**

Run:
```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test migrations migrations_create_operator_target_adoption_history_table -- --exact
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test operator_target_adoption adoption_history_returns_previous_distinct_operator_target_revision -- --exact
```

Expected: FAIL because the table, row type, and repo do not exist yet.

- [ ] **Step 3: Implement the minimal migration and repo surface**

Add a dedicated table and repo around this shape:

```sql
CREATE TABLE operator_target_adoption_history (
    adoption_id TEXT PRIMARY KEY,
    action_kind TEXT NOT NULL,
    operator_target_revision TEXT NOT NULL,
    previous_operator_target_revision TEXT,
    adoptable_revision TEXT,
    candidate_revision TEXT,
    adopted_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

```rust
pub struct OperatorTargetAdoptionHistoryRow {
    pub adoption_id: String,
    pub action_kind: String,
    pub operator_target_revision: String,
    pub previous_operator_target_revision: Option<String>,
    pub adoptable_revision: Option<String>,
    pub candidate_revision: Option<String>,
    pub adopted_at: DateTime<Utc>,
}

pub struct OperatorTargetAdoptionHistoryRepo;

impl OperatorTargetAdoptionHistoryRepo {
    pub async fn append(&self, pool: &PgPool, row: &OperatorTargetAdoptionHistoryRow) -> Result<()>;
    pub async fn latest(&self, pool: &PgPool) -> Result<Option<OperatorTargetAdoptionHistoryRow>>;
    pub async fn previous_distinct_revision(
        &self,
        pool: &PgPool,
        current_revision: &str,
    ) -> Result<Option<String>>;
}
```

- [ ] **Step 4: Re-run the persistence suites**

Run:
```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test migrations --test operator_target_adoption
```

Expected: PASS with the new table and history traversal behavior pinned down.

- [ ] **Step 5: Commit**

```bash
git add migrations/0012_operator_target_adoption_history.sql crates/persistence/src/models.rs crates/persistence/src/repos.rs crates/persistence/src/lib.rs crates/persistence/tests/migrations.rs crates/persistence/tests/operator_target_adoption.rs
git commit -m "feat: add operator target adoption history"
```

### Task 2: Add Config Round-Trip And Target-Source Rewrite Helpers

**Files:**
- Create: `crates/config-schema/tests/config_roundtrip.rs`
- Create: `crates/app-live/src/commands/targets/config_file.rs`
- Create: `crates/app-live/tests/targets_config_file.rs`
- Modify: `crates/config-schema/src/raw.rs`
- Modify: `crates/config-schema/src/lib.rs`
- Modify: `crates/config-schema/src/validate.rs`

- [ ] **Step 1: Write the failing config round-trip and rewrite tests**

```rust
#[test]
fn raw_config_round_trips_target_source_operator_target_revision() {
    let raw = load_raw_config_from_str(
        r#"
[runtime]
mode = "live"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"
"#,
    )
    .unwrap();

    let text = render_raw_config_to_string(&raw).unwrap();
    assert!(text.contains("operator_target_revision = \"targets-rev-9\""));
}

#[test]
fn rewrite_operator_target_revision_updates_the_same_config_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("axiom-arb.local.toml");
    std::fs::write(&path, SAMPLE_CONFIG).unwrap();

    rewrite_operator_target_revision(&path, "targets-rev-12").unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("operator_target_revision = \"targets-rev-12\""));
}
```

- [ ] **Step 2: Run the targeted tests to verify they fail**

Run:
```bash
cargo test -p config-schema --test config_roundtrip raw_config_round_trips_target_source_operator_target_revision -- --exact
cargo test -p app-live --test targets_config_file rewrite_operator_target_revision_updates_the_same_config_file -- --exact
```

Expected: FAIL because raw config is deserialize-only and no rewrite helper exists.

- [ ] **Step 3: Implement serialize/save helpers and config mutation**

Add:

```rust
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct RawAxiomConfig { /* ... */ }

pub fn render_raw_config_to_string(raw: &RawAxiomConfig) -> Result<String, ConfigSchemaError> {
    Ok(toml::to_string_pretty(raw)?)
}
```

and a small app-live helper:

```rust
pub fn rewrite_operator_target_revision(
    path: &Path,
    operator_target_revision: &str,
) -> Result<(), Box<dyn Error>> {
    let mut raw = load_raw_config_from_path(path)?;
    raw.negrisk
        .as_mut()
        .and_then(|n| n.target_source.as_mut())
        .expect("validated live config should contain target_source")
        .operator_target_revision = Some(operator_target_revision.to_owned());
    std::fs::write(path, render_raw_config_to_string(&raw)?)?;
    Ok(())
}
```

- [ ] **Step 4: Re-run the targeted suites**

Run:
```bash
cargo test -p config-schema --test config_roundtrip
cargo test -p app-live --test targets_config_file
```

Expected: PASS with stable config-file rewrite behavior.

- [ ] **Step 5: Commit**

```bash
git add crates/config-schema/src/raw.rs crates/config-schema/src/lib.rs crates/config-schema/src/validate.rs crates/config-schema/tests/config_roundtrip.rs crates/app-live/src/commands/targets/config_file.rs crates/app-live/tests/targets_config_file.rs
git commit -m "feat: add target config rewrite helpers"
```

### Task 3: Add The `app-live targets` Namespace And Read-Only Control-Plane Views

**Files:**
- Create: `crates/app-live/src/commands/targets/mod.rs`
- Create: `crates/app-live/src/commands/targets/state.rs`
- Create: `crates/app-live/src/commands/targets/status.rs`
- Create: `crates/app-live/src/commands/targets/show_current.rs`
- Create: `crates/app-live/src/commands/targets/candidates.rs`
- Create: `crates/app-live/tests/targets_read_commands.rs`
- Modify: `crates/app-live/src/cli.rs`
- Modify: `crates/app-live/src/main.rs`
- Modify: `crates/app-live/src/commands/mod.rs`
- Modify: `crates/app-live/src/commands/doctor.rs`

- [ ] **Step 1: Write the failing read-only CLI tests**

```rust
#[test]
fn targets_status_reports_configured_revision_and_unavailable_active_state() {
    let output = Command::new(app_live_binary())
        .arg("targets")
        .arg("status")
        .arg("--config")
        .arg(sample_live_config_path())
        .env("DATABASE_URL", default_test_database_url())
        .output()
        .unwrap();

    let text = combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("configured_operator_target_revision = targets-rev-9"), "{text}");
    assert!(text.contains("active_operator_target_revision = unavailable"), "{text}");
}

#[test]
fn targets_candidates_labels_advisory_adoptable_and_adopted_rows() {
    let output = Command::new(app_live_binary())
        .arg("targets")
        .arg("candidates")
        .arg("--config")
        .arg(sample_live_config_path())
        .env("DATABASE_URL", default_test_database_url())
        .output()
        .unwrap();

    let text = combined(&output);
    assert!(text.contains("candidate"));
    assert!(text.contains("adoptable"));
    assert!(text.contains("adopted"));
}
```

- [ ] **Step 2: Run the targeted CLI tests to verify they fail**

Run:
```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test targets_read_commands targets_status_reports_configured_revision_and_unavailable_active_state -- --exact
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test targets_read_commands targets_candidates_labels_advisory_adoptable_and_adopted_rows -- --exact
```

Expected: FAIL because the `targets` namespace and shared state loader do not exist.

- [ ] **Step 3: Implement nested CLI args and the shared state loader**

Shape the CLI like this:

```rust
#[derive(clap::Subcommand, Debug)]
pub enum TargetCommand {
    Status(TargetStatusArgs),
    Candidates(TargetCandidatesArgs),
    ShowCurrent(TargetShowCurrentArgs),
    Adopt(TargetAdoptArgs),
    Rollback(TargetRollbackArgs),
}

pub struct TargetControlPlaneState {
    pub configured_operator_target_revision: Option<String>,
    pub active_operator_target_revision: Option<String>,
    pub restart_needed: Option<bool>,
    pub provenance: Option<CandidateAdoptionProvenanceRow>,
    pub latest_action: Option<OperatorTargetAdoptionHistoryRow>,
}
```

`state.rs` should:
- read `configured_operator_target_revision` from the operator config file
- read `active_operator_target_revision` from `RuntimeProgressRepo::current`
- load provenance through `CandidateAdoptionRepo`
- load latest action through the new history repo
- keep runtime-unavailable as explicit `unavailable/unknown`, never guessed from history

- [ ] **Step 4: Re-run the read-only CLI suite**

Run:
```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test targets_read_commands
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test doctor_command doctor_paper_mode_marks_live_checks_as_skip -- --exact
```

Expected: PASS with `targets status`, `targets show-current`, and `targets candidates` working, and `doctor` still honoring mode-scoped `OK/FAIL/SKIP`.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/cli.rs crates/app-live/src/main.rs crates/app-live/src/commands/mod.rs crates/app-live/src/commands/doctor.rs crates/app-live/src/commands/targets/mod.rs crates/app-live/src/commands/targets/state.rs crates/app-live/src/commands/targets/status.rs crates/app-live/src/commands/targets/show_current.rs crates/app-live/src/commands/targets/candidates.rs crates/app-live/tests/targets_read_commands.rs
git commit -m "feat: add target control-plane read commands"
```

### Task 4: Implement `targets adopt`

**Files:**
- Create: `crates/app-live/src/commands/targets/adopt.rs`
- Create: `crates/app-live/tests/targets_write_commands.rs`
- Modify: `crates/app-live/src/cli.rs`
- Modify: `crates/app-live/src/commands/targets/mod.rs`
- Modify: `crates/app-live/src/commands/targets/state.rs`

- [ ] **Step 1: Write the failing adopt-command tests**

```rust
#[test]
fn targets_adopt_requires_exactly_one_selector_flag() {
    let output = Command::new(app_live_binary())
        .arg("targets")
        .arg("adopt")
        .arg("--config")
        .arg(temp_config_path())
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(combined(&output).contains("exactly one of --operator-target-revision or --adoptable-revision"));
}

#[test]
fn targets_adopt_from_adoptable_revision_rewrites_config_and_records_history() {
    let output = Command::new(app_live_binary())
        .arg("targets")
        .arg("adopt")
        .arg("--config")
        .arg(temp_config_path())
        .arg("--adoptable-revision")
        .arg("adoptable-9")
        .env("DATABASE_URL", default_test_database_url())
        .output()
        .unwrap();

    let text = combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("operator_target_revision = targets-rev-9"), "{text}");
    assert!(text.contains("restart_required = true"), "{text}");
}
```

- [ ] **Step 2: Run the targeted adopt tests to verify they fail**

Run:
```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test targets_write_commands targets_adopt_requires_exactly_one_selector_flag -- --exact
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test targets_write_commands targets_adopt_from_adoptable_revision_rewrites_config_and_records_history -- --exact
```

Expected: FAIL because the adopt command does not exist yet.

- [ ] **Step 3: Implement the adopt write path**

Behavior:

```rust
pub fn execute(args: TargetAdoptArgs) -> Result<(), Box<dyn Error>> {
    let resolved = resolve_adoption_selector(&pool, &args)?;
    rewrite_operator_target_revision(&args.config, &resolved.operator_target_revision)?;
    OperatorTargetAdoptionHistoryRepo.append(&pool, &resolved.history_row).await?;
    print_adoption_result(&resolved);
    Ok(())
}
```

Required semantics:
- exactly one selector flag
- `--adoptable-revision` resolves through durable provenance to one `operator_target_revision`
- direct `--operator-target-revision` only succeeds if the revision already has durable lineage
- no-op adopt of the already-configured revision reports success without appending misleading history

- [ ] **Step 4: Re-run the adopt suite**

Run:
```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test targets_write_commands targets_adopt_requires_exactly_one_selector_flag -- --exact
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test targets_write_commands targets_adopt_from_adoptable_revision_rewrites_config_and_records_history -- --exact
```

Expected: PASS with config rewrite + history append + restart-needed reporting.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/cli.rs crates/app-live/src/commands/targets/mod.rs crates/app-live/src/commands/targets/state.rs crates/app-live/src/commands/targets/adopt.rs crates/app-live/tests/targets_write_commands.rs
git commit -m "feat: add target adoption command"
```

### Task 5: Implement `targets rollback` And Adoption-Aware `doctor`

**Files:**
- Create: `crates/app-live/src/commands/targets/rollback.rs`
- Modify: `crates/app-live/src/cli.rs`
- Modify: `crates/app-live/src/commands/targets/mod.rs`
- Modify: `crates/app-live/src/commands/targets/state.rs`
- Modify: `crates/app-live/src/commands/doctor.rs`
- Modify: `crates/app-live/tests/targets_write_commands.rs`
- Modify: `crates/app-live/tests/doctor_command.rs`

- [ ] **Step 1: Write the failing rollback and doctor tests**

```rust
#[test]
fn targets_rollback_defaults_to_previous_distinct_revision() {
    let output = Command::new(app_live_binary())
        .arg("targets")
        .arg("rollback")
        .arg("--config")
        .arg(temp_config_path())
        .env("DATABASE_URL", default_test_database_url())
        .output()
        .unwrap();

    let text = combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("operator_target_revision = targets-rev-8"), "{text}");
}

#[test]
fn doctor_live_mode_reports_restart_needed_when_configured_and_active_revisions_diverge() {
    let output = Command::new(app_live_binary())
        .arg("doctor")
        .arg("--config")
        .arg(temp_config_path())
        .env("DATABASE_URL", default_test_database_url())
        .output()
        .unwrap();

    let text = combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("restart needed"), "{text}");
}
```

- [ ] **Step 2: Run the targeted rollback and doctor tests to verify they fail**

Run:
```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test targets_write_commands targets_rollback_defaults_to_previous_distinct_revision -- --exact
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test doctor_command doctor_live_mode_reports_restart_needed_when_configured_and_active_revisions_diverge -- --exact
```

Expected: FAIL because rollback does not exist and doctor does not yet compare configured vs active adoption state.

- [ ] **Step 3: Implement rollback and doctor linkage**

Required behavior:

```rust
let destination = match args.to_operator_target_revision.as_deref() {
    Some(revision) => revision.to_owned(),
    None => history.previous_distinct_revision(&pool, &configured_revision).await?
        .ok_or(TargetCommandError::NoPreviousOperatorTargetRevision)?,
};

rewrite_operator_target_revision(&args.config, &destination)?;
history.append(&pool, &rollback_row(destination.clone(), configured_revision.clone())).await?;
```

`doctor` should reuse the same control-plane loader and emit:
- configured target resolution
- active runtime state available/unavailable
- restart-needed true/false/unknown

- [ ] **Step 4: Re-run the rollback and doctor suites**

Run:
```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test targets_write_commands --test doctor_command
```

Expected: PASS with default previous-distinct rollback, explicit `--to-operator-target-revision`, and adoption-aware doctor output.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/cli.rs crates/app-live/src/commands/targets/mod.rs crates/app-live/src/commands/targets/state.rs crates/app-live/src/commands/targets/rollback.rs crates/app-live/src/commands/doctor.rs crates/app-live/tests/targets_write_commands.rs crates/app-live/tests/doctor_command.rs
git commit -m "feat: add target rollback workflow"
```

### Task 6: Update Operator Docs, Example Config, And Run Full Verification

**Files:**
- Modify: `README.md`
- Modify: `config/axiom-arb.example.toml`
- Modify: `docs/runbooks/bootstrap-and-ramp.md`
- Modify: `docs/runbooks/real-user-shadow-smoke.md`

- [ ] **Step 1: Update docs and examples to the new operator workflow**

Document:
- `app-live targets status`
- `app-live targets candidates`
- `app-live targets show-current`
- `app-live targets adopt --adoptable-revision ...`
- `app-live targets rollback [--to-operator-target-revision ...]`
- the fact that adopt/rollback rewrite the config-file `operator_target_revision`
- the fact that changes remain restart-scoped, not hot-reloaded

- [ ] **Step 2: Run a docs drift grep**

Run:
```bash
rg -n "manual.*operator_target_revision|edit .*operator_target_revision|AXIOM_NEG_RISK_LIVE_TARGETS" README.md config/axiom-arb.example.toml docs/runbooks
```

Expected: No stale operator guidance that still tells users to hand-edit low-level target payloads or legacy env surfaces for adoption.

- [ ] **Step 3: Run full verification**

Run:
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test --workspace
```

Expected: PASS across the workspace, including the new `targets` command suites and persistence history coverage.

- [ ] **Step 4: Commit**

```bash
git add README.md config/axiom-arb.example.toml docs/runbooks/bootstrap-and-ramp.md docs/runbooks/real-user-shadow-smoke.md
git commit -m "docs: add operator target adoption workflow"
```
