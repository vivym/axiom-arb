I'm using the writing-plans skill to create the implementation plan.

# Runtime Instrumented Supervisor Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ensure the instrumented bootstrap helpers ride the supervisor path so runtime and supervisor gauges stay coordinated while keeping the existing helper surface stable.

**Architecture:** `run_live_instrumented`/`run_paper_instrumented` will clone the recorder embedded in `AppInstrumentation`, hand it to `AppSupervisor::new_instrumented`, and rely on the supervisor’s bootstrap/dispatch sequence instead of manually building `AppRunResult`. This keeps the runtime-level reconcile attention metrics and the supervisor-owned backlog/rollout gauges on the same recorder and ensures the helper API still accepts `AppInstrumentation`.

**Tech Stack:** Rust, `tracing`, the `observability` crate, `cargo test`.

---

### Task 1: Route the instrumented helpers through the supervisor path

**Files:**
- Modify: `/Users/viv/.config/superpowers/worktrees/axiom-arb/observability-phase2-runtime-signals-exec/crates/app-live/src/runtime.rs`
- Modify: `/Users/viv/.config/superpowers/worktrees/axiom-arb/observability-phase2-runtime-signals-exec/crates/app-live/src/instrumentation.rs`
- Modify: `/Users/viv/.config/superpowers/worktrees/axiom-arb/observability-phase2-runtime-signals-exec/crates/app-live/tests/runtime_observability.rs`

- [ ] **Step 1: Add the runtime observability regression that looks for the supervisor dispatcher backlog gauge after `run_live_instrumented`.**

```rust
#[test]
fn run_live_instrumented_records_supervisor_backlog_gauge() {
    let observability = bootstrap_observability("app-live-test");
    let instrumentation = AppInstrumentation::enabled(observability.recorder());
    let source = StaticSnapshotSource::empty();

    let _ = run_live_instrumented(&source, instrumentation);

    let snapshot = observability.registry().snapshot();
    assert_eq!(
        snapshot
            .gauge(observability.metrics().dispatcher_backlog_count.key()),
        Some(0.0)
    );
}
```

- [ ] **Step 2: Run the new test to confirm it fails today because the supervisor never recorded the gauge.**

Run: `cargo test -p app-live --test runtime_observability run_live_instrumented_records_supervisor_backlog_gauge -- --nocapture`

Expected: FAIL because the recorder is never passed to the supervisor, so `dispatcher_backlog_count` stays `None`.

- [ ] **Step 3: Switch `run_live_instrumented`/`run_paper_instrumented` to build an `AppSupervisor` with the recorder captured by `AppInstrumentation::recorder()` and let `AppSupervisor::run_bootstrap()` emit the gauges, keeping `AppInstrumentation` on the runtime for reconcile attention; add the helper on `AppInstrumentation` to clone the recorder.**

- [ ] **Step 4: Run the same regression to verify it now passes.**

Run: `cargo test -p app-live --test runtime_observability run_live_instrumented_records_supervisor_backlog_gauge -- --nocapture`

Expected: PASS because the supervisor now records `dispatcher_backlog_count`.


### Task 2: Own the committed_journal_seq field key

**Files:**
- Modify: `/Users/viv/.config/superpowers/worktrees/axiom-arb/observability-phase2-runtime-signals-exec/crates/observability/src/conventions.rs`
- Modify: `/Users/viv/.config/superpowers/worktrees/axiom-arb/observability-phase2-runtime-signals-exec/crates/app-live/src/runtime.rs`
- Modify: `/Users/viv/.config/superpowers/worktrees/axiom-arb/observability-phase2-runtime-signals-exec/crates/app-live/tests/runtime_observability.rs`

- [ ] **Step 1: Add `field_keys::COMMITTED_JOURNAL_SEQ` and update `publish_snapshot`/its regression to read that constant so the recorded commits use the repo-owned vocabulary instead of a literal string.**

- [ ] **Step 2: Re-run the runtime observability suite to ensure the new constant has no regressions.**

Run: `cargo test -p app-live --test runtime_observability -- --nocapture`

Expected: PASS.

- [ ] **Step 3: Stage and commit the changes.**

```bash
git add crates/app-live/src/runtime.rs crates/app-live/src/instrumentation.rs crates/app-live/tests/runtime_observability.rs crates/observability/src/conventions.rs
git commit -m "fix(app-live): route instrumented bootstrap through supervisor"
```
