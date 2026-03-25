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

`app-live` is driven by `AXIOM_MODE` today, not a `--mode` CLI flag. At current `HEAD`, `app-live` and `app-replay` both bootstrap observability through one repo-owned entrypoint and emit successful startup/replay summaries via local structured tracing output. `app-live` is still only a bootstrap skeleton, `app-replay` is summary-oriented, and the observability path is local-only and OTel-compatible rather than OTel-enabled. They do not yet connect to Polymarket feeds, order heartbeat, or Postgres from the binary entrypoint.

## V1b Neg-Risk Scope Status

- `v1b foundation` exists today as library and replay support.
- `v1b live` has bootstrap-summary plumbing only for explicit operator inputs on fresh boot.
- `app-live` still does not place `neg-risk` orders.
- Family-halt precedence is `GlobalHalt > family halt > market-local halt > strategy-local filter`.
- Foundation-phase family halt blocks new `neg-risk` activation only; it does not override bootstrap `CancelOnly` or global emergency controls.

## Unified Runtime Rollout

- Phase 1 on the unified runtime is `full-set Live`.
- Phase 2 on the unified runtime is `neg-risk Shadow`.
- Phase 3a on the unified runtime is `neg-risk` rollout gates and readiness evidence only.
- Phase 3a does not add a new `neg-risk` pricing surface or live planner.
- Phase 3b can surface `neg-risk Live` in `app-live` bootstrap summary and metrics only when explicit operator inputs provide config-backed, live-approved, and live-ready families.
- That fresh-boot bootstrap synthesis is not durable authority. Restart and resume still require durable rollout evidence and will not fabricate rebuilt readiness from env or in-memory sets.
- `neg_risk_live_attempt_count` reports eligible backbone surfaces only; it is not evidence of external execution.
- Families may remain in `Disabled`, `Shadow`, `ReduceOnly`, or `RecoveryOnly`.
- Actual binary-driven family promotion still requires real feed, approval, and submit wiring beyond this bootstrap skeleton.
- Phase 3 `neg-risk Live` is not fully enabled by this repository state.
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
