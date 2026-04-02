# AxiomArb

`AxiomArb` is a Rust workspace for the Polymarket v1 live engine and replay tooling.

## Local Setup

1. Start local Postgres with `make db-up` or `docker compose up -d postgres`.
2. Set `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb`.
3. Prefer `cargo run -p app-live -- bootstrap` for first-run `paper` and `real-user shadow smoke` setup. It defaults to `config/axiom-arb.local.toml`, reuses the init wizard, and then guides the smoke flow through target adoption, doctor preflight, and optional startup.
4. If you use the lower-level path instead of `bootstrap`, run `cargo run -p app-live -- init --config config/axiom-arb.local.toml` to create the operator-local config and complete the interactive prompts for the long-lived account and relayer auth values.
5. Check high-level readiness with `cargo run -p app-live -- status --config config/axiom-arb.local.toml`.
6. If `status` reports `target-adoption-required`, inspect candidate/adoptable state with `cargo run -p app-live -- targets candidates --config config/axiom-arb.local.toml`, then adopt a startup-scoped operator target revision with `cargo run -p app-live -- targets adopt --config config/axiom-arb.local.toml --adoptable-revision <ADOPTABLE_REVISION>`.
7. Re-run `cargo run -p app-live -- status --config config/axiom-arb.local.toml` and follow its next action. When rollout lists are empty, adoption alone only writes `operator_target_revision`; rollout readiness remains separate work unless bootstrap explicitly enables the smoke-only rollout posture for the adopted family set.
8. Run `cargo run -p app-live -- doctor --config config/axiom-arb.local.toml` once `status` says the config is ready for preflight.
9. Start the daemon with `cargo run -p app-live -- run --config config/axiom-arb.local.toml`, or let `cargo run -p app-live -- bootstrap --start` enter `run` after the bootstrap flow reaches a ready state.
10. Run `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test --workspace` once the database is available.
11. Treat `app-replay` as a consumer of existing journal rows. It does not run migrations or seed `event_journal` for you.

## Running The Binaries

- `app-live init`: `cargo run -p app-live -- init --config config/axiom-arb.local.toml`
- `app-live bootstrap`: `cargo run -p app-live -- bootstrap [--config config/axiom-arb.local.toml]`
- `app-live bootstrap --start`: `cargo run -p app-live -- bootstrap --start [--config config/axiom-arb.local.toml]`
- `app-live status`: `cargo run -p app-live -- status --config config/axiom-arb.local.toml`
- `app-live targets status`: `cargo run -p app-live -- targets status --config config/axiom-arb.local.toml`
- `app-live targets candidates`: `cargo run -p app-live -- targets candidates --config config/axiom-arb.local.toml`
- `app-live targets show-current`: `cargo run -p app-live -- targets show-current --config config/axiom-arb.local.toml`
- `app-live targets adopt`: `cargo run -p app-live -- targets adopt --config config/axiom-arb.local.toml --adoptable-revision <ADOPTABLE_REVISION>`
- `app-live targets rollback`: `cargo run -p app-live -- targets rollback --config config/axiom-arb.local.toml [--to-operator-target-revision <operator-target-revision>]`
- `app-live doctor`: `cargo run -p app-live -- doctor --config config/axiom-arb.local.toml`
- `app-live run`: `cargo run -p app-live -- run --config config/axiom-arb.local.toml`
- `app-replay` from the beginning of an existing journal: `cargo run -p app-replay -- --config config/axiom-arb.local.toml --from-seq 0`
- Real-user shadow smoke now has a bootstrap-driven path. See [`docs/runbooks/real-user-shadow-smoke.md`](docs/runbooks/real-user-shadow-smoke.md) for the adopted-target-only smoke flow, inline rollout enablement, and shadow-only guard.

`bootstrap` is now the preferred high-level path for `paper` and `real-user shadow smoke`. `status` is the new operator homepage for config-and-control-plane readiness: it tells you whether you still need target adoption, rollout enablement, a controlled restart, `doctor`, or `run`, without doing any venue probes. For smoke, bootstrap keeps startup authority on `[negrisk.target_source].operator_target_revision`, inlines explicit adoption only when no target anchor exists yet, and then distinguishes two outcomes: `preflight-ready smoke startup` when rollout readiness is still empty, and `shadow-work-ready smoke startup` when bootstrap explicitly enables smoke-only rollout for the adopted family set. `init` remains available as the lower-level config wizard, `targets ...` remain the lower-level control-plane commands, and `targets status` / `targets show-current` remain the detailed provenance views. `doctor` is still the real preflight gate for `live` and `real-user shadow smoke`: it reports `Config`, `Credentials`, `Connectivity`, `Target Source`, and `Runtime Safety` sections, performs venue-facing probes for authenticated REST, market ws, user ws, heartbeat, and relayer reachability when those checks apply, and ends with explicit next actions. `targets adopt` and `targets rollback` rewrite `[negrisk.target_source].operator_target_revision` in the local TOML and record adoption history, but they remain startup-scoped operations. They do not hot-reload a running daemon. Use `status` first, then drop into `targets ...` or `doctor` only when the high-level next action tells you to.

`DATABASE_URL` remains the only deployment env var. Business configuration is loaded from the TOML passed with `--config`. At current `HEAD`, the observability surface is still scoped to repo-owned local signals only: Wave 1A/1B covers execution-attempt spans plus truthful `shadow_attempt_count`, `app-live` recovery divergence signals for resume/rebuild mismatches plus daemon posture/backlog status, and `venue-polymarket` relayer recent-transaction producer observability including local `relayer_pending_age`; Wave 1C adds local `neg-risk` control-plane producer signals in `app-live` plus the `app-replay` neg-risk replay summary span. The observability path remains local-only and OTel-compatible rather than OTel-enabled: there is no OpenTelemetry exporter in the binaries, no collector-backed pipeline, and no collector/OTel deployment claimed by this repository state. This repository state still does not claim a connected production `neg-risk` feed path, dashboards, alerts, `unknown-order`, `broken-leg`, or other collector-backed signals.

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
- The real-user shadow smoke path is manual and shadow-only; it is meant for operator verification, not for claiming production-ready live trading.
- Phase 3d upgrades `app-live` into a layered single-process daemon entrypoint with repo-owned ingress/dispatch/follow-up queues, startup-scoped operator-target revisions, daemon posture/backlog observability, and fail-closed startup ordering that restores truth before resuming ingest loops.
- Phase 3e continuously generates conservative `neg-risk` candidate targets and adoptable startup-scoped target revisions from the discovery pipeline.
- Real `Polymarket` websocket subscribe/auth/ping sends and `postHeartbeat(previous_heartbeat_id)` request wiring now exist for the daemon source adapters, but live target selection still comes from explicit operator inputs.
- Candidate generation remains advisory in Phase 3e; operator adoption is still explicit and limited to startup-scoped target revisions rather than automatic live promotion.
- Operator adoption now has a first-class control-plane workflow under `app-live targets ...`, but adoption still remains restart-scoped rather than hot-reloaded.
- Restart and resume still require durable live-attempt and live-submission truth plus any pending-reconcile anchors that actually exist; when durable rollout evidence is not loaded and no `negrisk` snapshot data is present, local observability reports neutral rollout provenance instead of claiming snapshot-derived evidence.
- `neg_risk_live_attempt_count` now counts durable bootstrap/resume live execution records.
- `observability` now defines typed counters `axiom_neg_risk_live_submit_accepted_total` and `axiom_neg_risk_live_submit_ambiguous_total` for accepted-versus-ambiguous live submit closure accounting.
- `observability` now also defines typed status signals `axiom_daemon_posture`, `axiom_ingress_backlog`, and `axiom_follow_up_backlog` for truthful daemon lifecycle reporting.
- `neg_risk_live_state_source` distinguishes fresh operator-synthesized bootstrap promotion from durable restored live-attempt anchors during restart/resume.
- Families may remain in `Disabled`, `Shadow`, `ReduceOnly`, or `RecoveryOnly`.
- Market-discovered pricing, hot-reloaded operator targets, and richer continuous control-plane automation remain follow-on work beyond this repository state.
- Phase 3 `neg-risk Live` is still not fully production-enabled by this repository state.
- Shadow artifacts stay on isolated storage and stream paths; they do not share authoritative live sinks.

## Verification

- `cargo fmt --all`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test --workspace`
- `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test -p persistence`

## Docs

- Runbooks: [`docs/runbooks/live-break-glass.md`](docs/runbooks/live-break-glass.md), [`docs/runbooks/bootstrap-and-ramp.md`](docs/runbooks/bootstrap-and-ramp.md), [`docs/runbooks/real-user-shadow-smoke.md`](docs/runbooks/real-user-shadow-smoke.md), and [`docs/runbooks/operator-target-adoption.md`](docs/runbooks/operator-target-adoption.md)
- Spec: [`docs/superpowers/specs/2026-03-23-axiomarb-v1-design.md`](docs/superpowers/specs/2026-03-23-axiomarb-v1-design.md)
- Plan: [`docs/superpowers/plans/2026-03-23-axiomarb-v1a-live-engine.md`](docs/superpowers/plans/2026-03-23-axiomarb-v1a-live-engine.md)
