# AxiomArb Observability Roadmap

- Date: 2026-03-25
- Status: Draft for user review
- Project: `AxiomArb`

## 1. Summary

This roadmap defines how `AxiomArb` should evolve from its current single-process, local-only observability baseline into a multi-process, production-operations-ready observability system built on an open OpenTelemetry stack.

The core decision is:

- keep business instrumentation repo-owned and `tracing`-first
- evolve observability in capability waves rather than jumping directly to the end state
- treat traces, metrics, logs, and trading-specific controls as parallel tracks in every wave
- adopt `OpenTelemetry Collector + Tempo + Prometheus + Loki + Grafana` as the long-term platform, but only after real producer signals and multi-process contracts are stable

This is a roadmap, not an implementation plan. It defines destination, sequencing, phase gates, and operating expectations across `local`, `dev`, `staging`, and `prod`.

## 2. Scope

### 2.1 In Scope

- the complete observability evolution from current monolith baseline to multi-process production operations
- traces, metrics, logs, and trading-specific observability as first-class concerns
- environment strategy across `local`, `dev`, `staging`, and `prod`
- platform architecture, signal taxonomy, rollout gates, dashboards, alerts, runbooks, and incident workflows
- the transition path from current local observability to an OSS OTel stack

### 2.2 Out Of Scope

- exact implementation tasks or code-level work breakdown
- vendor-specific commercial telemetry backends
- final dashboard panel layouts
- exact alert thresholds
- replacing the trading `event_journal` with observability signals

## 3. Current Baseline

At current `HEAD`, the repository already has an observability baseline:

- repo-owned observability facade and metric registry
- unified bootstrap surface
- local structured tracing for `app-live` and `app-replay`
- runtime/supervisor observability in `app-live`
- no OpenTelemetry exporter, collector, or external telemetry stack

This baseline is valuable, but incomplete. The highest-value producer signals are still missing or only partially wired, and the repository is not yet operating as a multi-process system.

## 4. Roadmap Structure

The roadmap is organized into five capability waves. Each wave has:

- a target architecture shape
- target environments
- deliverables
- hard gates for progressing to the next wave

### 4.1 Wave 0: Current Baseline

- shape: single process, local-first, no exporter
- environments: primarily `local`
- purpose: establish stable repo-owned observability contracts

### 4.2 Wave 1: Producer Wiring

- shape: still primarily single-process
- environments: `local + dev`
- purpose: connect real producers and stop relying on placeholder observability

### 4.3 Wave 2: Multi-Process Contracts

- shape: hybrid monolith plus background workers or service decomposition
- environments: `dev + staging`
- purpose: stabilize cross-process observability semantics before full platform rollout

### 4.4 Wave 3: Collector And OSS Stack

- shape: multi-process or multi-service deployment with centralized collection
- environments: `staging + prod`
- purpose: operationalize telemetry export with the OSS OpenTelemetry stack

### 4.5 Wave 4: Production Operations

- shape: production-ready multi-service observability with dashboards, alerts, and runbooks
- environments: primarily `prod`, validated in `staging`
- purpose: support on-call, incident response, and trading-safe operations

## 5. Architecture Principles

### 5.1 Repo-Owned Instrumentation Boundary

All business crates must continue to instrument through:

- `tracing`
- repo-owned observability types
- repo-owned metric names, dimensions, and field vocabularies

They must not depend directly on:

- OpenTelemetry SDK types
- exporter configuration
- collector topology
- vendor-specific backends

### 5.2 Layered Telemetry Architecture

The target architecture has four layers:

1. `Instrumentation Layer`
   - spans, events, and typed metrics in application and worker code
2. `Process-Local Telemetry Layer`
   - local subscribers, registries, process identity, run identity
3. `Export/Collection Layer`
   - OpenTelemetry Collector and downstream storage backends
4. `Operations Layer`
   - dashboards, alerting, runbooks, incident workflows

### 5.3 Migration Rule

Moving from one process to many must change deployment topology and resource metadata, but must not rewrite the trading-domain observability contract.

## 6. Signal Taxonomy

Observability signals are divided into two mandatory groups: `platform signals` and `trading signals`.

### 6.1 Platform Signals

- process lifecycle
- runtime health
- task and queue health
- websocket and REST I/O health
- DB and relayer latency/error health
- resource usage
- exporter and collector health

### 6.2 Trading Signals

#### Market And Metadata

- metadata refresh result
- `discovery_revision`
- `metadata_snapshot_hash`
- discovered / included / excluded families
- exclusion reason
- family halt and stale halt state
- rollout gate evidence

#### Execution

- opportunity detected / approved / rejected
- execution plan lifecycle
- order submitted / acked / unknown
- transport retry / business retry
- batch mixed result
- partial fill
- broken leg
- cancel-all / reduce-only trigger

#### Inventory And CTF

- split / merge / redeem lifecycle
- pending CTF inventory movement
- redeemable inventory
- quarantined inventory
- inventory mismatch
- resolution and payout-vector state

#### Recovery And Safety

- reconcile attention by reason
- local-vs-venue divergence
- stale or unknown relayer transactions
- runtime mode transitions
- market / family / global halt activation
- operator override and break-glass entry

#### Replay And Postmortem

- replay run lifecycle
- replay input revision and snapshot
- summary counters
- decision evidence for blocks, halts, and exclusions

### 6.3 Correlation Fields

The following IDs must become stable cross-signal correlation keys:

- `run_id`
- `service.name`
- `service.instance.id`
- `runtime_mode`
- `event_family_id`
- `condition_id`
- `order_id`
- `client_order_key`
- `execution_plan_id`
- `ctf_operation_id`
- `relayer_tx_id`
- `journal_seq`
- `discovery_revision`
- `metadata_snapshot_hash`

### 6.4 Observability vs Journal

`event_journal` remains the authoritative trading fact store. Observability does not replace it.

- journal answers: what factually happened
- observability answers: how the runtime behaved, what it decided, and where it degraded

## 7. Environment Strategy

### 7.1 Local

Purpose:

- developer feedback
- contract validation
- integration and fault-injection debugging

Requirements:

- structured tracing must remain usable without any collector
- local metrics must remain queryable without external infrastructure
- trading-specific signals must be inspectable locally

### 7.2 Dev

Purpose:

- validate real producers
- validate early multi-process contracts

Requirements:

- real websocket, heartbeat, reconcile, execution, recovery, and relayer producers can emit observability
- resource metadata and run identity begin to stabilize
- optional collector validation can start here

### 7.3 Staging

Purpose:

- production-like observability validation without production risk

Requirements:

- mandatory collector-backed ingestion
- cross-process trace correlation
- sampling, retention, batching, and backpressure behavior validation
- dashboards, alert paths, and runbooks tested against realistic drills

### 7.4 Prod

Purpose:

- operational trading support
- incident response
- safe degradation and recovery

Requirements:

- controlled exporter and collector configuration
- on-call routing
- monitored observability degradation
- trading-safe runbooks and incident taxonomy

## 8. Capability Waves

### 8.1 Wave 0: Current Baseline

Deliverables:

- unified bootstrap
- repo-owned span, field, and metric contracts
- `app-live` runtime and supervisor local signals
- `app-replay` structured entrypoint and replay signals

Progression gates:

- local tests are stable
- entrypoints no longer bypass structured tracing on their success paths
- documentation matches code reality

### 8.2 Wave 1: Producer Wiring

Deliverables:

- market websocket session and reconnect signals
- user websocket session and reconnect signals
- heartbeat freshness and failure signals
- execution plan lifecycle spans
- recovery and divergence signals
- relayer pending and stale transaction signals
- neg-risk discovery, refresh, and halt producers
- real emitters for trading-specific metrics that already exist as handles

Progression gates:

- no synthetic or read-path-only emitters for core trading signals
- signal semantics are reproducible in `local + dev`
- at least one execution or recovery incident can be explained end-to-end via spans, logs, and metrics

### 8.3 Wave 2: Multi-Process Contracts

Deliverables:

- stable `service.name`, `service.instance.id`, and `run_id` conventions
- cross-process trace and log correlation rules
- worker and job naming contracts
- observability for reconcile, relayer, and metadata/discovery workers
- per-process telemetry config schema

Progression gates:

- service decomposition does not change the business-level signal vocabulary
- a single execution or recovery incident can be followed across multiple processes
- staging can run a multi-process topology with consistent observability semantics

### 8.4 Wave 3: Collector And OSS Stack

Deliverables:

- OpenTelemetry Collector configuration
- trace export to Tempo
- metric export to Prometheus
- log export to Loki
- Grafana integration
- environment-specific sampling and retention policy
- exporter retry, batching, and backpressure policy

Progression gates:

- collector failure degrades observability only, not trading correctness
- traces, metrics, and logs are stable in staging
- failure drills cover websocket churn, relayer lag, broken-leg recovery, and collector outage

### 8.5 Wave 4: Production Operations

Deliverables:

- production dashboards
- alerts
- SLOs
- incident taxonomy
- operator views
- break-glass and halt runbooks
- postmortem templates bound to observability evidence

Progression gates:

- signals are actionable by operators, not just developers
- alerts have owners and defined responses
- degrade / halt / resume actions have standard operating procedures
- observability degradation itself is monitored and has a runbook

## 9. Dashboard Families

At minimum, the roadmap should produce four dashboard families:

### 9.1 Platform Health

- process health
- queue and backlog health
- websocket / REST / DB / relayer health
- collector and exporter health
- host and process resource usage

### 9.2 Trading Safety

- runtime mode transitions
- halt activations by scope
- reconcile attention by reason
- divergence counts
- unknown-order counts
- broken-leg inventory counts
- stale relayer transaction counts

### 9.3 Neg-Risk And Metadata Control Plane

- metadata refresh cadence
- family discovered / included / excluded counts
- exclusion reasons
- stale halt counts
- rollout gate evidence
- metadata churn

### 9.4 Execution And Inventory

- opportunity and risk outcomes
- execution plan outcomes
- partial fills and mixed batch results
- CTF operation lifecycle
- pending and quarantined inventory
- redeemable inventory

## 10. Alert Policy

Alerts must be designed around operator action, not raw metric existence.

### 10.1 Page-Level Alerts

- unexpected global halt activation
- repeated broken-leg incidents above hard threshold
- relayer pending age above hard limit
- sustained reconcile divergence above hard limit
- sustained market or user feed unavailability
- critical production observability loss when it blocks safe operation

### 10.2 Ticket Or Chat Alerts

- stale family halt present
- metadata refresh failures
- excluded-family spike
- rising reconnect rate
- rising quarantine inventory
- rollout gate drift or parity anomalies

## 11. Runbook Set

The roadmap should eventually produce at least these runbooks:

- websocket churn
- relayer stale or unknown
- broken-leg recovery
- unknown-order and reconcile divergence
- metadata refresh failure
- family halt and rollout gate block
- collector or pipeline degradation
- global halt / reduce-only / resume

Each runbook must define:

- trigger signals
- likely failure domains
- immediate safe action
- evidence to inspect
- escalation rule
- resume criteria

## 12. Horizontal Quality Gates

Two gates apply across all waves:

### 12.1 Signal Correctness Gate

Signals are not considered complete unless:

- names and dimensions are stable
- emitters are attached to real producers
- counters are not incremented from read paths
- metrics do not depend on synthetic state without an explicit contract
- spans and logs carry the required correlation fields

### 12.2 Operator Usability Gate

Observability is not considered complete unless:

- a human can use it to understand an incident
- the correct next action is identifiable from dashboards, traces, and logs
- alerts link to runbooks with unambiguous actions

## 13. Recommended Sequencing

The next recommended sequence is:

1. Finish `Wave 1: Producer Wiring`
2. Then define and land `Wave 2: Multi-Process Contracts`
3. Then roll out `Wave 3` in `staging`
4. Then adopt `Wave 4` in `prod`

The roadmap explicitly does **not** recommend introducing OpenTelemetry exporter work before producer wiring and cross-process observability contracts are stable.

## 14. Acceptance Criteria

This roadmap is acceptable when:

- it provides a complete path from current baseline to production operations
- it covers `local`, `dev`, `staging`, and `prod`
- it covers monolith-to-multi-process migration rather than only the end state
- it treats traces, metrics, logs, and trading-specific controls as parallel first-class tracks
- it keeps business instrumentation repo-owned and OTel-compatible rather than OTel-first
