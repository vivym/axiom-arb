# AxiomArb

`AxiomArb` is a Rust workspace for the Polymarket v1 live engine and replay tooling.

## Local Setup

1. Start local Postgres with `make db-up` or `docker compose up -d postgres`.
2. Set `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb`.
3. Prefer `cargo run -p app-live -- bootstrap` for Day 0 `paper` and `real-user shadow smoke` setup. It defaults to `config/axiom-arb.local.toml`, reuses the init wizard, and for live/smoke it now asks for the wallet kind explicitly. EOA configs write only `[polymarket.account]`; proxy/safe configs also collect `[polymarket.relayer_auth]`. In the current control plane, that EOA shape is intended for smoke and other account-L2-only flows. Non-shadow live still fail-closes unless a relayer-backed path exists. For smoke it now runs discovery first when the database has no artifacts. From there it either lists adoptable revisions and waits for explicit selection, or it truthfully stops with a no-adoptable discovery summary.
4. The built-in Polymarket endpoints use `https` and `wss`. If your network requires an outbound proxy, set `HTTPS_PROXY` or `ALL_PROXY` in the environment before running `app-live`; use `HTTP_PROXY` only if you override Polymarket endpoints to `http` or `ws`. The Rust REST and websocket clients do not read macOS system proxy settings automatically.
5. For Day 1+ `real-user shadow smoke` progression, check readiness with `cargo run -p app-live -- status --config config/axiom-arb.local.toml`, then use `cargo run -p app-live -- apply --config config/axiom-arb.local.toml` to continue from `adoptable-ready`, rollout, or restart states and stop at ready. Add `--start` only when you want the same flow to continue into foreground `run`. When readiness is `discovery-required`, `apply` stops and points you back to `bootstrap` or `discover`; it does not run discovery inline.
6. If you want the lower-level Day 0 fallback instead of `bootstrap`, start from the checked-in example config or an existing operator-local config, fill the long-lived account values there, and add `[polymarket.relayer_auth]` only for proxy/safe flows. The checked-in example config keeps the EOA default on shadow smoke and other account-L2-only truth; generic non-shadow live still needs the relayer-backed path and will fail-closed without it. Then run `cargo run -p app-live -- discover --config config/axiom-arb.local.toml`, inspect persisted artifacts with `cargo run -p app-live -- targets candidates --config config/axiom-arb.local.toml`, and adopt with `cargo run -p app-live -- targets adopt --config config/axiom-arb.local.toml --adoptable-revision <ADOPTABLE_REVISION>`. New configs should stay on built-in defaults plus optional `[polymarket.source_overrides]`; do not introduce legacy `[polymarket.source]`.
7. Re-run `cargo run -p app-live -- status --config config/axiom-arb.local.toml` and follow its next action. When rollout lists are empty, adoption alone only writes `operator_strategy_revision`; smoke `bootstrap` and `apply` can explicitly enable the smoke-only rollout posture for the adopted scopes, while live rollout readiness remains separate work until that posture already exists.
8. Discovery artifacts and adoption lineage are separate facts. `discover` persists advisory candidate and adoptable artifacts only. `targets adopt` writes or reuses the canonical adoption provenance for the chosen `operator_strategy_revision` and appends adoption history.
9. Run `cargo run -p app-live -- doctor --config config/axiom-arb.local.toml` once `status` says the config is ready for preflight, or let `apply` run `doctor` for you in the corresponding high-level flow.
10. Start the daemon with `cargo run -p app-live -- run --config config/axiom-arb.local.toml`, or let `cargo run -p app-live -- bootstrap --start` / `cargo run -p app-live -- apply --config config/axiom-arb.local.toml --start` enter `run` when those high-level flows reach a ready state.
11. Verify the latest local result with `cargo run -p app-live -- verify --config config/axiom-arb.local.toml`.
12. Run `DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb cargo test --workspace` once the database is available.
13. Treat `app-replay` as a consumer of existing journal rows. It does not run migrations or seed `event_journal` for you.

## Running The Binaries

- `app-live init`: `cargo run -p app-live -- init --config config/axiom-arb.local.toml`
- `app-live bootstrap`: `cargo run -p app-live -- bootstrap [--config config/axiom-arb.local.toml]`
- `app-live bootstrap --start`: `cargo run -p app-live -- bootstrap --start [--config config/axiom-arb.local.toml]`
- `app-live discover`: `cargo run -p app-live -- discover --config config/axiom-arb.local.toml`
- `app-live status`: `cargo run -p app-live -- status --config config/axiom-arb.local.toml`
- `app-live apply`: `cargo run -p app-live -- apply --config config/axiom-arb.local.toml`
- `app-live apply --start`: `cargo run -p app-live -- apply --config config/axiom-arb.local.toml --start`
- `app-live targets status`: `cargo run -p app-live -- targets status --config config/axiom-arb.local.toml`
- `app-live targets candidates`: `cargo run -p app-live -- targets candidates --config config/axiom-arb.local.toml`
- `app-live targets show-current`: `cargo run -p app-live -- targets show-current --config config/axiom-arb.local.toml`
- `app-live targets adopt`: `cargo run -p app-live -- targets adopt --config config/axiom-arb.local.toml --adoptable-revision <ADOPTABLE_REVISION>`
- `app-live targets rollback`: `cargo run -p app-live -- targets rollback --config config/axiom-arb.local.toml [--to-operator-strategy-revision <operator-strategy-revision>]`
- `app-live doctor`: `cargo run -p app-live -- doctor --config config/axiom-arb.local.toml`
- `app-live run`: `cargo run -p app-live -- run --config config/axiom-arb.local.toml`
- `app-live verify`: `cargo run -p app-live -- verify --config config/axiom-arb.local.toml`
- `app-live verify` for paper: `cargo run -p app-live -- verify --config config/axiom-arb.local.toml --expect paper-no-live`
- `app-live verify` for real-user shadow smoke: `cargo run -p app-live -- verify --config config/axiom-arb.local.toml --expect smoke-shadow-only`
- `app-live verify` for live consistency: `cargo run -p app-live -- verify --config config/axiom-arb.local.toml --expect live-config-consistent`
- `app-replay` from the beginning of an existing journal: `cargo run -p app-replay -- --config config/axiom-arb.local.toml --from-seq 0`
- Real-user shadow smoke now has a bootstrap-driven path. See [`docs/runbooks/real-user-shadow-smoke.md`](docs/runbooks/real-user-shadow-smoke.md) for the adopted-revision smoke flow and shadow-only guard.

`bootstrap` remains the preferred Day 0 path for `paper` and `real-user shadow smoke`. The low-level Day 0 fallback is `discover -> targets candidates -> targets adopt` on top of the example/local config shape, not legacy `[polymarket.source]` wiring. For `real-user shadow smoke`, `apply` is the Day 1+ orchestration path: it reuses `status`, can inline smoke-only strategy adoption only after discovery has already produced adoptable artifacts, can explicitly enable smoke-only rollout for the adopted scopes, runs `doctor`, and with `--start` can enter foreground `run` after the explicit manual restart-boundary confirmation when needed. For live configs, `apply` is the conservative Day 1+ path after discovery, adoption, and rollout posture already exist: it reuses `status`, does not inline discovery, strategy adoption, or live rollout mutation, runs `doctor`, and still keeps `verify` separate. `status` is the operator homepage for config-and-control-plane readiness: it tells you whether you still need discovery, adoption, rollout enablement, a controlled restart, `doctor`, or `run`, without doing any venue probes. For smoke, `bootstrap` keeps startup authority on `[strategy_control].operator_strategy_revision`, and the smoke clamp keeps every risk-expanding route on the shadow path. `apply` continues from the same authority without introducing any new one. If the config is still using legacy explicit `[[negrisk.targets]]`, that is compatibility mode: `status`, `doctor`, `verify`, and `run` can read it, but mutation flows do not auto-migrate it; use `targets adopt` to enter the neutral adopted-revision model. `run` creates a durable `run_session`; `status` now surfaces both the latest relevant `run_session` for the current config and any conflicting active run, and it projects `stale` from freshness instead of storing it as a separate truth value. `verify` is the post-run result check: it uses only local evidence, does not do venue probes, and answers whether the latest local result matches `paper-no-live`, `smoke-shadow-only`, or conservative `live-config-consistent` expectations. When `verify` cannot uniquely map a historical window to a session, it downgrades to evidence-only rather than pretending to know the historical control-plane truth. In the operator output, `Run session`, `Relevant run session`, and `Conflicting active run session` are the stable handles to look for. `targets adopt` and `targets rollback` rewrite `[strategy_control].operator_strategy_revision` in the local TOML, write or reuse canonical adoption provenance, and append adoption history. In compatibility mode, the first `targets adopt` is also the explicit migration step into the neutral control plane. These remain startup-scoped operations; they do not hot-reload a running daemon. Use `status` first, then drop into `apply`, `targets ...`, `doctor`, or `verify` only when the high-level next action tells you to.

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
- Phase 3d upgrades `app-live` into a layered single-process daemon entrypoint with repo-owned ingress/dispatch/follow-up queues, startup-scoped operator strategy revisions, daemon posture/backlog observability, and fail-closed startup ordering that restores truth before resuming ingest loops.
- Phase 3e continuously generates conservative `neg-risk` candidate artifacts and adoptable startup-scoped strategy revisions from the discovery pipeline.
- Real `Polymarket` websocket subscribe/auth/ping sends and `postHeartbeat(previous_heartbeat_id)` request wiring now exist for the daemon source adapters, but live strategy selection still comes from explicit operator inputs.
- Candidate generation remains advisory in Phase 3e; operator adoption is still explicit and limited to startup-scoped strategy revisions rather than automatic live promotion.
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
