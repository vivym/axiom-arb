# AxiomArb Run Session Lifecycle UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution discipline:** Use `@superpowers:test-driven-development` for every task and `@superpowers:verification-before-completion` before claiming completion.

**Goal:** Add a durable Postgres-backed `run_session` lifecycle truth source so `run`, `status`, and `verify` can reliably explain which run produced which results under which startup intent.

**Architecture:** Introduce a dedicated `run_sessions` durability model owned only by `app-live run`, with explicit lifecycle writes for `starting`, `running`, `exited`, and `failed`, plus reader-projected `stale`. Thread `run_session_id` into runtime progress and execution attempts, expose session-aware dual-view state in `status`, and make `verify` default to the latest relevant session while degrading historical windows to evidence-only when they do not map uniquely.

**Tech Stack:** Rust, `clap`, `sqlx`/Postgres migrations, existing `app-live` runtime and command modules, `persistence` repos/models, `cargo test`, `cargo fmt`, `cargo clippy`

---

## File Map

### New files

- `migrations/0014_run_sessions.sql`
  - Create the `run_sessions` table and add session-link columns/indexes for runtime truth surfaces.
- `crates/persistence/tests/run_sessions.rs`
  - Focused persistence tests for `run_sessions`, lifecycle transitions, latest-relevant selection, and stale projection helpers.
- `crates/app-live/src/run_session.rs`
  - App-live lifecycle ownership helpers: startup snapshot building, critical session writes, retryable freshness writes, and `invoked_by` plumbing.
- `crates/app-live/tests/support/run_session_db.rs`
  - Shared Postgres helpers for seeding `run_sessions`, runtime progress, and execution attempts in `status`/`verify` tests.

### Modified files

- `crates/persistence/src/models.rs`
  - Add `RunSessionRow`, `RunSessionState`, snapshot fields, and session-linked row fields for runtime progress and execution attempts.
- `crates/persistence/src/repos.rs`
  - Add `RunSessionRepo`; extend `RuntimeProgressRepo`, `ExecutionAttemptRepo`, and session-selection queries.
- `crates/persistence/src/lib.rs`
  - Export `RunSessionRepo`, `RunSessionRow`, and lifecycle-related persistence errors.
- `crates/persistence/tests/runtime_backbone.rs`
  - Extend existing backbone tests for `active_run_session_id` and `execution_attempts.run_session_id`.
- `crates/app-live/src/lib.rs`
  - Export the new `run_session` helper module.
- `crates/app-live/src/commands/run.rs`
  - Create initial sessions, pass `invoked_by`, promote `starting -> running`, and close terminal states.
- `crates/app-live/src/daemon.rs`
  - Switch paper/live startup call sites to use session-aware foreground startup boundaries.
- `crates/app-live/src/commands/bootstrap/flow.rs`
  - Pass `invoked_by = bootstrap` when the bootstrap flow hands control to `run`.
- `crates/app-live/src/commands/apply/mod.rs`
  - Pass `invoked_by = apply` when the apply flow hands control to `run`.
- `crates/app-live/src/runtime.rs`
  - Persist `active_run_session_id`, attach `run_session_id` to execution attempts, and refresh session freshness from the runtime path.
- `crates/app-live/src/task_groups.rs`
  - Reuse `run_session_id` as `source_session_id` for runtime-originated facts.
- `crates/app-live/src/commands/status/model.rs`
  - Add the dual-view session contract fields for relevant and conflicting active sessions.
- `crates/app-live/src/commands/status/evaluate.rs`
  - Resolve latest relevant and conflicting active sessions, and project stale state from overdue freshness.
- `crates/app-live/src/commands/status/mod.rs`
  - Render session-aware key details without losing current readiness semantics.
- `crates/app-live/src/commands/verify/model.rs`
  - Add operator-visible run-session context to verify reports.
- `crates/app-live/src/commands/verify/context.rs`
  - Resolve latest relevant session for current config and degrade explicit historical windows when unmappable.
- `crates/app-live/src/commands/verify/session.rs`
  - Async session-resolution helpers for latest relevant and historical-window mapping.
- `crates/app-live/src/commands/verify/evidence.rs`
  - Load latest-run evidence via `run_session_id` and use session-aware joins before falling back to window heuristics.
- `crates/app-live/src/commands/verify/mod.rs`
  - Render session-aware verdict context and evidence-only downgrade reasons.
- `crates/app-live/tests/run_command.rs`
  - Add foreground `run` lifecycle integration coverage for paper and live/smoke starts.
- `crates/app-live/tests/status_command.rs`
  - Add session-aware readiness output coverage, including conflicting active sessions and stale projection.
- `crates/app-live/tests/verify_command.rs`
  - Add latest relevant session and historical downgrade coverage.
- `crates/app-live/tests/doctor_command.rs`
  - Update runtime-progress helpers to compile with `active_run_session_id`.
- `crates/app-live/tests/targets_read_commands.rs`
  - Update runtime-progress helpers to compile with `active_run_session_id`.
- `crates/app-live/tests/targets_write_commands.rs`
  - Update runtime-progress helpers to compile with `active_run_session_id`.
- `crates/app-live/tests/candidate_daemon.rs`
  - Update seeded attempts/progress rows to carry session links.
- `crates/app-live/tests/main_entrypoint.rs`
  - Update seeded attempts/progress rows to carry session links.
- `crates/app-live/tests/negrisk_live_rollout.rs`
  - Update attempt fixtures to carry `run_session_id`.
- `crates/app-live/tests/real_user_shadow_smoke.rs`
  - Update seeded shadow attempts to carry `run_session_id`.
- `crates/app-live/tests/support/mod.rs`
  - Export the new `run_session_db` helper.
- `crates/app-live/tests/support/apply_db.rs`
  - Update seeded progress rows to carry `active_run_session_id`.
- `crates/app-live/tests/support/status_db.rs`
  - Seed relevant/conflicting sessions and compile with `active_run_session_id`.
- `crates/app-live/tests/support/verify_db.rs`
  - Seed sessions, attempts, and runtime progress consistently.
- `crates/persistence/tests/negrisk_live.rs`
  - Update attempt fixtures to carry `run_session_id`.
- `README.md`
  - Update operator docs so `run`, `status`, and `verify` mention durable run-session semantics.
- `docs/runbooks/real-user-shadow-smoke.md`
  - Document how `status` and `verify` surface `run_session_id` and stale/conflicting runs.

### Existing files to study before editing

- `crates/app-live/src/commands/run.rs`
- `crates/app-live/src/daemon.rs`
- `crates/app-live/src/runtime.rs`
- `crates/app-live/src/task_groups.rs`
- `crates/app-live/src/commands/status/model.rs`
- `crates/app-live/src/commands/status/evaluate.rs`
- `crates/app-live/src/commands/status/mod.rs`
- `crates/app-live/src/commands/verify/context.rs`
- `crates/app-live/src/commands/verify/session.rs`
- `crates/app-live/src/commands/verify/evidence.rs`
- `crates/app-live/src/commands/verify/mod.rs`
- `crates/persistence/src/models.rs`
- `crates/persistence/src/repos.rs`
- `crates/persistence/tests/runtime_backbone.rs`
- `crates/app-live/tests/support/status_db.rs`
- `crates/app-live/tests/support/verify_db.rs`
- `docs/superpowers/specs/2026-04-05-axiomarb-run-session-lifecycle-ux-design.md`

---

### Task 1: Add the Durable `run_sessions` Schema and Row Types

**Files:**
- Create: `migrations/0014_run_sessions.sql`
- Modify: `crates/persistence/src/models.rs`
- Modify: `crates/persistence/src/lib.rs`
- Create: `crates/persistence/tests/run_sessions.rs`
- Modify: `crates/persistence/tests/runtime_backbone.rs`

- [ ] **Step 1: Write the failing migration and row-shape tests**

Add tests covering:

```rust
#[tokio::test]
async fn run_sessions_migration_creates_table_and_session_link_columns() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let tables: Vec<String> = sqlx::query_scalar(
        "select table_name from information_schema.tables where table_schema = current_schema()"
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();

    assert!(tables.iter().any(|name| name == "run_sessions"));

    let progress_columns: Vec<String> = sqlx::query_scalar(
        "select column_name from information_schema.columns where table_name = 'runtime_apply_progress'"
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert!(progress_columns.iter().any(|name| name == "active_run_session_id"));

    let attempt_columns: Vec<String> = sqlx::query_scalar(
        "select column_name from information_schema.columns where table_name = 'execution_attempts'"
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert!(attempt_columns.iter().any(|name| name == "run_session_id"));
}

#[test]
fn run_session_state_labels_are_stable() {
    assert_eq!(RunSessionState::Starting.as_str(), "starting");
    assert_eq!(RunSessionState::Running.as_str(), "running");
    assert_eq!(RunSessionState::Exited.as_str(), "exited");
    assert_eq!(RunSessionState::Failed.as_str(), "failed");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test run_sessions run_sessions_migration_creates_table_and_session_link_columns -- --exact --test-threads=1
```

Expected: FAIL because `run_sessions`, `active_run_session_id`, and `run_session_id` do not exist yet.

- [ ] **Step 3: Add the migration and row scaffolding**

Implement the migration and row types:

```sql
CREATE TABLE run_sessions (
    run_session_id TEXT PRIMARY KEY,
    invoked_by TEXT NOT NULL,
    mode TEXT NOT NULL,
    state TEXT NOT NULL,
    started_at TIMESTAMPTZ NOT NULL,
    last_seen_at TIMESTAMPTZ NOT NULL,
    ended_at TIMESTAMPTZ,
    exit_status TEXT,
    exit_reason TEXT,
    config_path TEXT NOT NULL,
    config_fingerprint TEXT NOT NULL,
    target_source_kind TEXT NOT NULL,
    startup_target_revision_at_start TEXT NOT NULL,
    configured_operator_target_revision TEXT,
    active_operator_target_revision_at_start TEXT,
    rollout_state_at_start TEXT,
    real_user_shadow_smoke BOOLEAN NOT NULL
);

ALTER TABLE runtime_apply_progress
    ADD COLUMN active_run_session_id TEXT REFERENCES run_sessions(run_session_id);

ALTER TABLE execution_attempts
    ADD COLUMN run_session_id TEXT REFERENCES run_sessions(run_session_id);

CREATE INDEX run_sessions_state_started_idx ON run_sessions (state, started_at DESC);
CREATE INDEX execution_attempts_run_session_idx ON execution_attempts (run_session_id, created_at DESC);
```

And add the corresponding Rust shapes:

```rust
pub enum RunSessionState {
    Starting,
    Running,
    Exited,
    Failed,
}

pub struct RunSessionRow {
    pub run_session_id: String,
    pub invoked_by: String,
    pub mode: String,
    pub state: RunSessionState,
    pub started_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub exit_status: Option<String>,
    pub exit_reason: Option<String>,
    pub config_path: String,
    pub config_fingerprint: String,
    pub target_source_kind: String,
    pub startup_target_revision_at_start: String,
    pub configured_operator_target_revision: Option<String>,
    pub active_operator_target_revision_at_start: Option<String>,
    pub rollout_state_at_start: Option<String>,
    pub real_user_shadow_smoke: bool,
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test run_sessions -- --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test runtime_backbone runtime_progress_persists_journal_state_snapshot_triplet -- --exact --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add migrations/0014_run_sessions.sql crates/persistence/src/models.rs crates/persistence/src/lib.rs crates/persistence/tests/run_sessions.rs crates/persistence/tests/runtime_backbone.rs
git commit -m "feat: add run session persistence schema"
```

---

### Task 2: Implement `RunSessionRepo` and Critical Lifecycle Write Semantics

**Files:**
- Modify: `crates/persistence/src/repos.rs`
- Modify: `crates/persistence/src/lib.rs`
- Modify: `crates/persistence/tests/run_sessions.rs`

- [ ] **Step 1: Write the failing repo tests**

Add tests for critical lifecycle writes and retryable freshness:

```rust
#[tokio::test]
async fn run_session_repo_round_trips_starting_running_and_terminal_states() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    RunSessionRepo
        .create_starting(&db.pool, &sample_starting_session("rs-1"))
        .await
        .unwrap();
    RunSessionRepo.mark_running(&db.pool, "rs-1", Utc::now()).await.unwrap();
    RunSessionRepo
        .mark_exited(&db.pool, "rs-1", Utc::now(), "success", None)
        .await
        .unwrap();

    let row = RunSessionRepo.get(&db.pool, "rs-1").await.unwrap().unwrap();
    assert_eq!(row.state, RunSessionState::Exited);
    assert_eq!(row.exit_status.as_deref(), Some("success"));
}

#[tokio::test]
async fn run_session_repo_projects_stale_from_freshness_without_writing_stale_state() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    RunSessionRepo
        .create_starting(&db.pool, &sample_running_session("rs-old", Utc::now() - chrono::Duration::minutes(10)))
        .await
        .unwrap();

    let projected = RunSessionRepo
        .load_with_projected_state(&db.pool, "rs-old", chrono::Duration::minutes(5))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(projected.state_label, "stale");
    let raw = RunSessionRepo.get(&db.pool, "rs-old").await.unwrap().unwrap();
    assert_eq!(raw.state, RunSessionState::Running);
}

#[tokio::test]
async fn run_session_repo_selects_latest_relevant_and_conflicting_active_sessions() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    RunSessionRepo
        .create_starting(
            &db.pool,
            &sample_running_session_for_target("rs-old", "config/axiom-arb.local.toml", "fp-old", "startup-target-1", "targets-rev-1"),
        )
        .await
        .unwrap();
    RunSessionRepo
        .create_starting(
            &db.pool,
            &sample_running_session_for_target("rs-new", "config/axiom-arb.local.toml", "fp-new", "startup-target-2", "targets-rev-2"),
        )
        .await
        .unwrap();

    let relevant = RunSessionRepo
        .latest_relevant(
            &db.pool,
            "live",
            "config/axiom-arb.local.toml",
            "fp-new",
            "targets-rev-2",
            "startup-target-2",
            Some("ready"),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(relevant.run_session_id, "rs-new");

    RuntimeProgressRepo
        .record_progress(
            &db.pool,
            41,
            7,
            Some("snapshot-7"),
            Some("targets-rev-1"),
            Some("rs-old"),
        )
        .await
        .unwrap();

    let conflicting = RunSessionRepo
        .conflicting_active_for_run_session(&db.pool, "rs-old")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(conflicting.run_session_id, "rs-old");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test run_sessions run_session_repo_round_trips_starting_running_and_terminal_states -- --exact --test-threads=1
```

Expected: FAIL because `RunSessionRepo` does not exist yet.

- [ ] **Step 3: Implement `RunSessionRepo`**

Add lifecycle methods and explicit semantics:

```rust
pub struct RunSessionRepo;

impl RunSessionRepo {
    pub async fn create_starting(&self, pool: &PgPool, row: &RunSessionRow) -> Result<()>;
    pub async fn mark_running(
        &self,
        pool: &PgPool,
        run_session_id: &str,
        seen_at: DateTime<Utc>,
    ) -> Result<()>;
    pub async fn mark_exited(
        &self,
        pool: &PgPool,
        run_session_id: &str,
        ended_at: DateTime<Utc>,
        exit_status: &str,
        exit_reason: Option<&str>,
    ) -> Result<()>;
    pub async fn mark_failed(
        &self,
        pool: &PgPool,
        run_session_id: &str,
        ended_at: DateTime<Utc>,
        exit_reason: &str,
    ) -> Result<()>;
    pub async fn refresh_last_seen(
        &self,
        pool: &PgPool,
        run_session_id: &str,
        seen_at: DateTime<Utc>,
    ) -> Result<()>;
}
```

Implementation rules:

- fail closed on `create_starting`, `mark_running`, `mark_exited`, and `mark_failed`
- make `refresh_last_seen` a small, separate update path that can be retried by callers
- add `load_with_projected_state(..., stale_after)` or equivalent read helper so readers project `stale` without mutating the stored enum
- add read helpers used later by `status` and `verify`, for example:

```rust
pub async fn latest_relevant(
    &self,
    pool: &PgPool,
    mode: &str,
    config_path: &str,
    config_fingerprint: &str,
    configured_target: &str,
    startup_target_revision_at_start: &str,
    rollout_state: Option<&str>,
) -> Result<Option<RunSessionRow>>;

pub async fn conflicting_active_for_run_session(
    &self,
    pool: &PgPool,
    active_run_session_id: &str,
) -> Result<Option<RunSessionRow>>;

pub async fn resolve_unique_for_attempt_id(
    &self,
    pool: &PgPool,
    attempt_id: &str,
) -> Result<Option<RunSessionRow>>;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test run_sessions -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/persistence/src/repos.rs crates/persistence/src/lib.rs crates/persistence/tests/run_sessions.rs
git commit -m "feat: add run session lifecycle repo"
```

---

### Task 3: Link Runtime Progress and Execution Attempts to `run_session_id`

**Files:**
- Modify: `crates/persistence/src/models.rs`
- Modify: `crates/persistence/src/repos.rs`
- Modify: `crates/persistence/tests/runtime_backbone.rs`
- Modify: `crates/app-live/src/runtime.rs`
- Modify: `crates/app-live/src/task_groups.rs`
- Modify: `crates/app-live/src/negrisk_shadow.rs`
- Modify: `crates/persistence/tests/negrisk_live.rs`
- Modify: `crates/app-live/tests/doctor_command.rs`
- Modify: `crates/app-live/tests/targets_read_commands.rs`
- Modify: `crates/app-live/tests/targets_write_commands.rs`
- Modify: `crates/app-live/tests/candidate_daemon.rs`
- Modify: `crates/app-live/tests/main_entrypoint.rs`
- Modify: `crates/app-live/tests/negrisk_live_rollout.rs`
- Modify: `crates/app-live/tests/real_user_shadow_smoke.rs`
- Modify: `crates/app-live/tests/support/apply_db.rs`
- Modify: `crates/app-live/tests/support/status_db.rs`
- Modify: `crates/app-live/tests/support/verify_db.rs`

- [ ] **Step 1: Write the failing linkage tests**

Extend persistence/runtime tests with:

```rust
#[tokio::test]
async fn runtime_progress_round_trips_active_run_session_id() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    RunSessionRepo
        .create_starting(&db.pool, &sample_starting_session("rs-1"))
        .await
        .unwrap();
    RuntimeProgressRepo
        .record_progress(&db.pool, 41, 7, Some("snapshot-7"), Some("targets-rev-3"), Some("rs-1"))
        .await
        .unwrap();

    let progress = RuntimeProgressRepo.current(&db.pool).await.unwrap().unwrap();
    assert_eq!(progress.active_run_session_id.as_deref(), Some("rs-1"));
}

#[tokio::test]
async fn execution_attempts_round_trip_run_session_id() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    RunSessionRepo
        .create_starting(&db.pool, &sample_starting_session("rs-2"))
        .await
        .unwrap();
    ExecutionAttemptRepo
        .append(&db.pool, &sample_attempt("attempt-1", ExecutionMode::Shadow, "rs-2"))
        .await
        .unwrap();

    let row = ExecutionAttemptRepo.get_by_attempt_id(&db.pool, "attempt-1").await.unwrap().unwrap();
    assert_eq!(row.attempt.run_session_id.as_deref(), Some("rs-2"));
}

#[test]
fn runtime_attention_uses_run_session_id_as_source_session_id() {
    let mut group = HeartbeatTaskGroup::for_tests(TestHeartbeatSource::success());
    let event = futures::executor::block_on(group.tick())
        .unwrap()
        .expect("runtime attention event should exist");

    assert_eq!(event.event.source_session_id, "session-live");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test runtime_backbone runtime_progress_round_trips_active_run_session_id -- --exact --test-threads=1
```

Expected: FAIL because repo/model signatures do not carry session IDs yet.

- [ ] **Step 3: Implement the linkage**

Update the row and repo contracts:

```rust
pub struct RuntimeProgressRow {
    pub last_journal_seq: i64,
    pub last_state_version: i64,
    pub last_snapshot_id: Option<String>,
    pub operator_target_revision: Option<String>,
    pub active_run_session_id: Option<String>,
}

pub struct ExecutionAttemptRow {
    pub attempt_id: String,
    pub run_session_id: Option<String>,
    // existing fields...
}
```

Then thread them through:

```rust
RuntimeProgressRepo.record_progress(
    &pool,
    summary.last_journal_seq,
    state_version,
    summary.published_snapshot_id.as_deref(),
    Some(operator_target_revision),
    Some(run_session_id),
)

let attempt = ExecutionAttemptRow {
    run_session_id: Some(run_session_id.to_owned()),
    // existing fields...
};
```

Also update `HeartbeatTaskGroup` so runtime-originated facts reuse the owning `run_session_id` as emitted `source_session_id`.

- [ ] **Step 4: Run the tests to verify they pass**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test runtime_backbone -- --test-threads=1
cargo test -p app-live --lib task_groups:: -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/persistence/src/models.rs crates/persistence/src/repos.rs crates/persistence/tests/runtime_backbone.rs crates/app-live/src/runtime.rs crates/app-live/src/task_groups.rs
git commit -m "feat: thread run session ids through runtime truth"
```

---

### Task 4: Add the `app-live` Session Ownership Layer and Wire `run`

**Files:**
- Create: `crates/app-live/src/run_session.rs`
- Modify: `crates/app-live/src/lib.rs`
- Modify: `crates/app-live/src/commands/run.rs`
- Modify: `crates/app-live/src/daemon.rs`
- Modify: `crates/app-live/src/commands/bootstrap/flow.rs`
- Modify: `crates/app-live/src/commands/apply/mod.rs`
- Modify: `crates/app-live/tests/run_command.rs`
- Create: `crates/app-live/tests/support/run_session_db.rs`
- Modify: `crates/app-live/tests/support/mod.rs`

- [ ] **Step 1: Write the failing `run` lifecycle tests**

Add tests covering paper and startup failure semantics:

```rust
#[test]
fn run_paper_creates_running_then_exited_session() {
    let output = Command::new(cli::app_live_binary())
        .arg("run")
        .arg("--config")
        .arg(cli::config_fixture("app-live-paper.toml"))
        .env("DATABASE_URL", cli::default_test_database_url())
        .output()
        .expect("paper run should execute");

    assert!(output.status.success(), "{}", cli::combined(&output));

    let session = run_session_db::latest_session();
    assert_eq!(session.mode, "paper");
    assert_eq!(session.invoked_by, "run");
    assert_eq!(session.state, RunSessionState::Exited);
}

#[test]
fn run_startup_failure_records_failed_session() {
    let output = Command::new(cli::app_live_binary())
        .arg("run")
        .arg("--config")
        .arg(cli::config_fixture("app-live-ux-live.toml"))
        .env("DATABASE_URL", cli::default_test_database_url())
        .output()
        .expect("broken live run should execute");

    assert!(!output.status.success(), "{}", cli::combined(&output));

    let session = run_session_db::latest_session();
    assert_eq!(session.state, RunSessionState::Failed);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test run_command -- --test-threads=1
```

Expected: FAIL because `run` does not create or close sessions yet.

- [ ] **Step 3: Implement the session owner**

Create a focused helper:

```rust
pub struct RunSessionHandle {
    pub run_session_id: String,
    pub invoked_by: &'static str,
}

impl RunSessionHandle {
    pub fn create_starting(config_path: &Path, config: &AppLiveConfigView<'_>, invoked_by: &'static str) -> Result<Self, Box<dyn Error>>;
    pub fn mark_running(&self) -> Result<(), Box<dyn Error>>;
    pub fn refresh_last_seen(&self) -> Result<(), Box<dyn Error>>;
    pub fn mark_exited(&self) -> Result<(), Box<dyn Error>>;
    pub fn mark_failed(&self, reason: &str) -> Result<(), Box<dyn Error>>;
}
```

Wire it so:

- `run_from_config_path` creates `starting`
- paper uses the foreground daemon path and marks `running` only after paper runtime loop establishment
- live/smoke mark `running` only after startup target resolution and daemon loop establishment
- normal exit writes `exited`
- startup/runtime failure writes `failed`
- wire `refresh_last_seen` from the foreground runtime path so successful `running` sessions stay fresh while the daemon loop is alive

- [ ] **Step 4: Run the tests to verify they pass**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test run_command -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/run_session.rs crates/app-live/src/lib.rs crates/app-live/src/commands/run.rs crates/app-live/src/daemon.rs crates/app-live/src/commands/bootstrap/flow.rs crates/app-live/src/commands/apply/mod.rs crates/app-live/tests/run_command.rs crates/app-live/tests/support/run_session_db.rs crates/app-live/tests/support/mod.rs
git commit -m "feat: add run-owned session lifecycle writes"
```

---

### Task 5: Add Reader-Projected Stale and Dual-View Session State to `status`

**Files:**
- Modify: `crates/app-live/src/commands/status/model.rs`
- Modify: `crates/app-live/src/commands/status/evaluate.rs`
- Modify: `crates/app-live/src/commands/status/mod.rs`
- Modify: `crates/app-live/tests/support/status_db.rs`
- Modify: `crates/app-live/tests/status_command.rs`

- [ ] **Step 1: Write the failing `status` session-view tests**

Add coverage for relevant and conflicting sessions:

```rust
#[test]
fn status_restart_required_shows_relevant_and_conflicting_active_sessions() {
    let output = status_db::run_status_fixture("smoke_restart_with_old_active_session");
    let text = cli::combined(&output);

    assert!(text.contains("Relevant run session: rs-new"));
    assert!(text.contains("Conflicting active run session: rs-old"));
    assert!(text.contains("Conflicting active state: running"));
}

#[test]
fn status_projects_overdue_running_session_as_stale() {
    let output = status_db::run_status_fixture("smoke_stale_session");
    let text = cli::combined(&output);

    assert!(text.contains("Relevant run state: stale"));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test status_command status_restart_required_shows_relevant_and_conflicting_active_sessions -- --exact --test-threads=1
```

Expected: FAIL because `status` has no run-session fields yet.

- [ ] **Step 3: Implement dual-view status details**

Extend `StatusDetails` with the minimum session contract:

```rust
pub struct StatusDetails {
    pub configured_target: Option<String>,
    pub active_target: Option<String>,
    pub target_source: Option<StatusTargetSource>,
    pub rollout_state: Option<StatusRolloutState>,
    pub restart_needed: Option<bool>,
    pub relevant_run_session_id: Option<String>,
    pub relevant_run_state: Option<String>,
    pub relevant_run_started_at: Option<DateTime<Utc>>,
    pub relevant_startup_target_revision: Option<String>,
    pub conflicting_active_run_session_id: Option<String>,
    pub conflicting_active_run_state: Option<String>,
    pub conflicting_active_started_at: Option<DateTime<Utc>>,
    pub conflicting_active_startup_target_revision: Option<String>,
    pub reason: Option<String>,
}
```

Then update evaluation rules:

- load latest relevant session by current config/mode
- if `configured != active`, also load the conflicting active session by `active_run_session_id`
- project `stale` from overdue `last_seen_at` without mutating the stored enum
- keep current readiness calculation intact

- [ ] **Step 4: Run the tests to verify they pass**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test status_command -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app-live/src/commands/status/model.rs crates/app-live/src/commands/status/evaluate.rs crates/app-live/src/commands/status/mod.rs crates/app-live/tests/support/status_db.rs crates/app-live/tests/status_command.rs
git commit -m "feat: add session-aware status views"
```

---

### Task 6: Make `verify` Session-Aware by Default and Safe on Historical Windows

**Files:**
- Modify: `crates/persistence/src/repos.rs`
- Modify: `crates/app-live/src/commands/verify/model.rs`
- Modify: `crates/app-live/src/commands/verify/context.rs`
- Create: `crates/app-live/src/commands/verify/session.rs`
- Modify: `crates/app-live/src/commands/verify/evidence.rs`
- Modify: `crates/app-live/src/commands/verify/mod.rs`
- Modify: `crates/app-live/tests/support/verify_db.rs`
- Modify: `crates/app-live/tests/verify_command.rs`

- [ ] **Step 1: Write the failing `verify` session-resolution tests**

Add tests for default latest-session behavior and historical downgrade:

```rust
#[test]
fn verify_latest_run_uses_latest_relevant_session() {
    let output = verify_db::run_verify_fixture("latest_relevant_session");
    let text = cli::combined(&output);

    assert!(text.contains("Run session: rs-live-2"));
    assert!(text.contains("Verdict: PASS"));
}

#[test]
fn verify_attempt_id_without_unique_session_mapping_degrades_to_evidence_only() {
    let output = verify_db::run_verify_with_args(
        "ambiguous_historical_attempt",
        &["--attempt-id", "attempt-legacy-7"],
    );
    let text = cli::combined(&output);

    assert!(text.contains("historical window is not uniquely mapped to a run session"));
    assert!(text.contains("config/lifecycle consistency was not evaluated"));
}

#[test]
fn verify_since_with_unique_session_mapping_keeps_strong_interpretation() {
    let output = verify_db::run_verify_with_args(
        "unique_since_window",
        &["--since", "10m"],
    );
    let text = cli::combined(&output);

    assert!(text.contains("Run session: rs-live-2"));
    assert!(!text.contains("config/lifecycle consistency was not evaluated"));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test verify_command verify_latest_run_uses_latest_relevant_session -- --exact --test-threads=1
```

Expected: FAIL because `verify` still defaults to scenario heuristics instead of session truth.

- [ ] **Step 3: Implement session-aware `verify`**

Update the verify context and evidence loaders:

```rust
pub struct VerifyControlPlaneContext {
    pub mode: Option<VerifyControlPlaneMode>,
    pub target_source: Option<VerifyControlPlaneTargetSource>,
    pub configured_target: Option<String>,
    pub active_target: Option<String>,
    pub restart_needed: Option<bool>,
    pub rollout_state: Option<VerifyControlPlaneRolloutState>,
    pub run_session_id: Option<String>,
    pub run_session_state: Option<String>,
}

pub struct ResolvedVerifySession {
    pub relevant_session: Option<RunSessionRow>,
    pub historical_window_session: Option<RunSessionRow>,
    pub historical_window_unique: bool,
}
```

Behavior rules:

- latest/default verify resolves the latest relevant `run_session`
- attempts join directly by `execution_attempts.run_session_id`
- attempt-scoped artifacts stay attached indirectly via `attempt_id`
- projected stale session state must be surfaced into the verify control-plane context and treated as degraded, not as a healthy current run
- explicit historical windows:
  - if uniquely resolvable to one session, keep strong config/lifecycle interpretation
  - otherwise downgrade to evidence-only and say so in the report

Implement async resolution in `verify/session.rs`, and make `verify/mod.rs` call it before `compare_window_to_current_config_anchor`:

```rust
pub async fn resolve_session_window(
    pool: &PgPool,
    context: &VerifyContext,
    selection: &VerifyWindowSelection,
) -> Result<ResolvedVerifySession>;
```

Then keep `compare_window_to_current_config_anchor` pure by passing in the resolved result instead of giving it hidden DB responsibilities.

Add explicit repo-backed resolution paths for:

```rust
pub async fn resolve_unique_for_attempt_id(... ) -> Result<Option<RunSessionRow>>;
pub async fn resolve_unique_for_since(... ) -> Result<Option<RunSessionRow>>;
pub async fn resolve_unique_for_seq_range(... ) -> Result<Option<RunSessionRow>>;
```

and update `compare_window_to_current_config_anchor` so it only hard-degrades when the historical window does not uniquely map to a single session.

- [ ] **Step 4: Run the tests to verify they pass**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test verify_command -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/persistence/src/repos.rs crates/app-live/src/commands/verify/model.rs crates/app-live/src/commands/verify/context.rs crates/app-live/src/commands/verify/session.rs crates/app-live/src/commands/verify/evidence.rs crates/app-live/src/commands/verify/mod.rs crates/app-live/tests/support/verify_db.rs crates/app-live/tests/verify_command.rs
git commit -m "feat: resolve verify against run sessions"
```

---

### Task 7: Document Session Truth and Run the Full Regression Sweep

**Files:**
- Modify: `README.md`
- Modify: `docs/runbooks/real-user-shadow-smoke.md`
- Modify: `crates/app-live/tests/run_command.rs`
- Modify: `crates/app-live/tests/status_command.rs`
- Modify: `crates/app-live/tests/verify_command.rs`
- Modify: `crates/persistence/tests/run_sessions.rs`
- Modify: `crates/persistence/tests/runtime_backbone.rs`

- [ ] **Step 1: Write the failing documentation and output assertions**

Add assertions to existing command tests for operator-visible session handles:

```rust
#[test]
fn status_output_shows_relevant_run_session_handle() {
    let output = status_db::run_status_fixture("smoke_config_ready_with_session");
    assert!(cli::combined(&output).contains("Relevant run session: rs-"));
}

#[test]
fn verify_output_shows_run_session_handle_before_next_actions() {
    let output = verify_db::run_verify_fixture("latest_relevant_session");
    let text = cli::combined(&output);
    assert!(text.contains("Run session: rs-"));
    assert!(text.contains("Next Actions"));
}
```

- [ ] **Step 2: Run the focused tests to verify they fail**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test status_command status_output_shows_relevant_run_session_handle -- --exact --test-threads=1
```

Expected: FAIL until the operator-visible output is fully wired.

- [ ] **Step 3: Update docs and finish output polish**

Document the new lifecycle truth in `README.md` and `docs/runbooks/real-user-shadow-smoke.md`:

```md
- `app-live run` now creates a durable `run_session`
- `app-live status` shows both the latest relevant run session and any conflicting active run
- `app-live verify` defaults to the latest relevant run session and degrades historical windows when they cannot be uniquely mapped
```

Make sure command output uses the same vocabulary:

- `Run session`
- `Relevant run session`
- `Conflicting active run session`
- `stale`

- [ ] **Step 4: Run the final regression sweep**

Run:

```bash
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test run_sessions -- --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence --test runtime_backbone -- --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test run_command -- --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test status_command -- --test-threads=1
DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p app-live --test verify_command -- --test-threads=1
cargo fmt --all --check
cargo clippy -p app-live -p persistence --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add README.md docs/runbooks/real-user-shadow-smoke.md crates/app-live/tests/run_command.rs crates/app-live/tests/status_command.rs crates/app-live/tests/verify_command.rs crates/persistence/tests/run_sessions.rs crates/persistence/tests/runtime_backbone.rs
git commit -m "docs: document run session lifecycle UX"
```

---

## Notes for Execution

- Do not introduce a second lifecycle backend; first phase is Postgres-only for all modes, including paper.
- Do not persist raw config blobs or secrets inside `run_sessions`.
- Do not add a durable `stale` enum value; project it from freshness on reads.
- Do not attach `run_session_id` directly to every artifact table; use attempt -> session joins unless the table is truly session-level.
- Keep `run` as the only lifecycle writer; `bootstrap` and `apply` may only pass `invoked_by`.
