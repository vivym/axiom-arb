# AxiomArb

`AxiomArb` is a Rust workspace for the Polymarket v1 live engine and replay tooling.

## Local Setup

1. Start local Postgres with `make db-up` or `docker compose up -d postgres`.
2. Set `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb`.
3. Run `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test --workspace` once the database is available.
4. Treat `app-replay` as a consumer of existing journal rows. It does not run migrations or seed `event_journal` for you.

## Running The Binaries

- Paper-mode bootstrap skeleton: `AXIOM_MODE=paper cargo run -p app-live`
- Live-mode bootstrap skeleton: `AXIOM_MODE=live cargo run -p app-live`
- Replay summary from the beginning of an existing journal: `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo run -p app-replay -- --from-seq 0`

`app-live` is driven by `AXIOM_MODE` today, not a `--mode` CLI flag. At current `HEAD`, the Wave 1B observability surface is scoped to repo-owned local signals only: `execution` emits execution-attempt spans plus truthful `shadow_attempt_count`, `app-live` emits recovery divergence signals for resume/rebuild mismatches, and `venue-polymarket` exposes relayer recent-transaction producer observability including local `relayer_pending_age`. The observability path remains local-only and OTel-compatible rather than OTel-enabled, and there is no OpenTelemetry exporter in the binary. This repository state still does not claim `unknown-order`, `broken-leg`, or collector-backed signals.

## V1b Neg-Risk Scope Status

- `v1b foundation` exists today as library and replay support.
- `v1b live` now includes a bootstrap-time `neg-risk` live backbone for explicit operator inputs in the static harness.
- `app-live` can now perform real `neg-risk` live submits in `bootstrap + resume` mode when explicit operator inputs provide config-backed, live-approved, and live-ready families and durable store access is available.
- Family-halt precedence is `GlobalHalt > family halt > market-local halt > strategy-local filter`.
- Foundation-phase family halt blocks new `neg-risk` activation only; it does not override bootstrap `CancelOnly` or global emergency controls.

## Unified Runtime Rollout

- Phase 1 on the unified runtime is `full-set Live`.
- Phase 2 on the unified runtime is `neg-risk Shadow`.
- Phase 3a on the unified runtime is `neg-risk` rollout gates and readiness evidence only.
- Phase 3a does not add a new `neg-risk` pricing surface or live planner.
- Phase 3b can plan `neg-risk` family submits, create request-bound attempts, build live artifact payloads, and materialize order-request bodies in `app-live` when explicit operator inputs provide config-backed, live-approved, and live-ready families.
- Phase 3c closes the `bootstrap + resume` live submit loop with real signing, real venue submission, durable live submission records, durable pending-reconcile anchors, and fail-closed restart restore.
- Fresh-boot promotion still depends on explicit operator inputs because the repository does not yet have a production `neg-risk` feed path.
- Restart and resume require durable rollout evidence plus durable live-attempt, live-submission, and pending-reconcile anchors; they will not fabricate rebuilt readiness or rebuilt live attempts from env or in-memory sets.
- `neg_risk_live_attempt_count` now counts durable bootstrap/resume live execution records.
- `observability` now defines typed counters `axiom_neg_risk_live_submit_accepted_total` and `axiom_neg_risk_live_submit_ambiguous_total` for accepted-versus-ambiguous live submit closure accounting.
- `neg_risk_live_state_source` distinguishes fresh operator-synthesized bootstrap promotion from durable restored live-attempt anchors during restart/resume.
- Families may remain in `Disabled`, `Shadow`, `ReduceOnly`, or `RecoveryOnly`.
- Continuous daemonization, market-discovered pricing, and full production feed wiring remain follow-on work beyond this repository state.
- Phase 3 `neg-risk Live` is still not fully production-enabled by this repository state.
- Shadow artifacts stay on isolated storage and stream paths; they do not share authoritative live sinks.

## Verification

- `cargo fmt --all`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test --workspace`
- `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence`

## Docs

- Runbooks: [`docs/runbooks/live-break-glass.md`](docs/runbooks/live-break-glass.md) and [`docs/runbooks/bootstrap-and-ramp.md`](docs/runbooks/bootstrap-and-ramp.md)
- Spec: [`docs/superpowers/specs/2026-03-23-axiomarb-v1-design.md`](docs/superpowers/specs/2026-03-23-axiomarb-v1-design.md)
- Plan: [`docs/superpowers/plans/2026-03-23-axiomarb-v1a-live-engine.md`](docs/superpowers/plans/2026-03-23-axiomarb-v1a-live-engine.md)
