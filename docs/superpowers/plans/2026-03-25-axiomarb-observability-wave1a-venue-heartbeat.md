# AxiomArb Observability Wave 1A Venue And Heartbeat Producer Wiring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the first executable slice of the observability roadmap by giving `venue-polymarket` truthful, repo-owned observability producers for market/user websocket session lifecycle and order-heartbeat lifecycle, without pretending `app-live` already has full venue loops.

**Architecture:** Keep the work narrow and honest. Extend the repo-owned observability vocabulary with venue-session and heartbeat fields, add focused session-lifecycle and producer-instrumentation helpers inside `venue-polymarket`, and validate them through library integration tests using the existing in-process registry plus captured `tracing` output. Do not wire `app-live` to non-existent live websocket tasks in this plan.

**Tech Stack:** Rust, `tracing`, existing `observability` facade and `MetricRegistry`, `chrono`, `venue-polymarket` integration tests

---

## Scope Boundary

The roadmap spec is too large for one executable plan. It should be decomposed into follow-on plans:

- `Wave 1A` (this plan): websocket session + heartbeat producer observability
- `Wave 1B`: execution / recovery / relayer producer observability
- `Wave 1C`: neg-risk metadata discovery / refresh / halt producer observability
- later plans: multi-process contracts, collector integration, production operations

This document covers `Wave 1A` only. It must produce working, testable software on its own.

- In scope: venue-session lifecycle state, repo-owned span/field vocabulary for websocket and heartbeat producers, reconnect counter emission by channel, heartbeat freshness emission, library-level producer instrumentation tests, README alignment.
- Out of scope: real websocket networking tasks in `app-live`, execution/recovery observability, relayer pending/stale signals, neg-risk discovery/halt producers, OTel exporter work, collector config, dashboards, or alerts.

## File Structure Map

### Root Docs

- Modify: `/Users/viv/.config/superpowers/worktrees/axiom-arb/observability-roadmap-plans/README.md`
  Responsibility: explain that `venue-polymarket` now exposes repo-owned websocket/heartbeat producer observability primitives, while `app-live` still does not claim full live venue wiring.

### Observability

- Modify: `/Users/viv/.config/superpowers/worktrees/axiom-arb/observability-roadmap-plans/crates/observability/src/conventions.rs`
  Responsibility: add stable span names and field keys for venue websocket session and heartbeat producers.
- Modify: `/Users/viv/.config/superpowers/worktrees/axiom-arb/observability-roadmap-plans/crates/observability/tests/conventions.rs`
  Responsibility: lock the new venue-producer vocabulary.

### Venue Polymarket

- Create: `/Users/viv/.config/superpowers/worktrees/axiom-arb/observability-roadmap-plans/crates/venue-polymarket/src/ws_session.rs`
  Responsibility: model websocket session connect/disconnect/reconnect state separately from message parsing.
- Create: `/Users/viv/.config/superpowers/worktrees/axiom-arb/observability-roadmap-plans/crates/venue-polymarket/src/instrumentation.rs`
  Responsibility: map websocket session lifecycle and heartbeat lifecycle events into repo-owned spans and metric writes.
- Modify: `/Users/viv/.config/superpowers/worktrees/axiom-arb/observability-roadmap-plans/crates/venue-polymarket/src/lib.rs`
  Responsibility: export the new session and instrumentation surface.
- Modify: `/Users/viv/.config/superpowers/worktrees/axiom-arb/observability-roadmap-plans/crates/venue-polymarket/src/heartbeat.rs`
  Responsibility: expose the minimum heartbeat lifecycle information needed by instrumentation without forcing fake network loops.
- Create: `/Users/viv/.config/superpowers/worktrees/axiom-arb/observability-roadmap-plans/crates/venue-polymarket/tests/ws_session.rs`
  Responsibility: lock websocket session lifecycle semantics.
- Create: `/Users/viv/.config/superpowers/worktrees/axiom-arb/observability-roadmap-plans/crates/venue-polymarket/tests/support/mod.rs`
  Responsibility: shared tracing-capture helper for venue-producer integration tests.
- Create: `/Users/viv/.config/superpowers/worktrees/axiom-arb/observability-roadmap-plans/crates/venue-polymarket/tests/producer_observability.rs`
  Responsibility: verify repo-owned spans and metrics are emitted from truthful websocket session and heartbeat producers.

## Task 1: Add Venue Producer Observability Vocabulary

**Files:**
- Modify: `crates/observability/src/conventions.rs`
- Modify: `crates/observability/tests/conventions.rs`

- [ ] **Step 1: Write the failing conventions test**

```rust
use observability::{field_keys, span_names};

#[test]
fn venue_producer_conventions_define_ws_and_heartbeat_fields() {
    assert_eq!(span_names::VENUE_WS_SESSION, "axiom.venue.ws.session");
    assert_eq!(span_names::VENUE_HEARTBEAT, "axiom.venue.heartbeat");

    assert_eq!(field_keys::CHANNEL, "channel");
    assert_eq!(field_keys::CONNECTION_ID, "connection_id");
    assert_eq!(field_keys::SESSION_STATUS, "session_status");
    assert_eq!(field_keys::HEARTBEAT_ID, "heartbeat_id");
    assert_eq!(field_keys::HEARTBEAT_STATUS, "heartbeat_status");
}
```

- [ ] **Step 2: Run the test to verify failure**

Run:

```bash
cargo test -p observability venue_producer_conventions_define_ws_and_heartbeat_fields -- --exact
```

Expected: FAIL because the new span names and field keys do not exist yet.

- [ ] **Step 3: Implement the minimal vocabulary additions**

```rust
pub mod span_names {
    pub const VENUE_WS_SESSION: &str = "axiom.venue.ws.session";
    pub const VENUE_HEARTBEAT: &str = "axiom.venue.heartbeat";
}

pub mod field_keys {
    pub const CHANNEL: &str = "channel";
    pub const CONNECTION_ID: &str = "connection_id";
    pub const SESSION_STATUS: &str = "session_status";
    pub const HEARTBEAT_ID: &str = "heartbeat_id";
    pub const HEARTBEAT_STATUS: &str = "heartbeat_status";
}
```

Implementation notes:

- reuse existing `metric_dimensions::Channel`; do not invent duplicate channel vocabularies
- keep these names repo-owned so later OTel export maps from one source of truth

- [ ] **Step 4: Run the conventions suite**

Run:

```bash
cargo test -p observability conventions -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/observability/src/conventions.rs crates/observability/tests/conventions.rs
git commit -m "feat: add venue producer observability vocabulary"
```

## Task 2: Add Explicit Websocket Session Lifecycle State

**Files:**
- Create: `crates/venue-polymarket/src/ws_session.rs`
- Modify: `crates/venue-polymarket/src/lib.rs`
- Create: `crates/venue-polymarket/tests/ws_session.rs`

- [ ] **Step 1: Write the failing websocket session tests**

```rust
use chrono::{TimeZone, Utc};
use venue_polymarket::{WsChannelKind, WsSessionMonitor, WsSessionState, WsSessionStatus};

#[test]
fn reconnect_after_disconnect_increments_counter_and_updates_connection_id() {
    let monitor = WsSessionMonitor::new(WsChannelKind::Market);
    let mut state = WsSessionState::new(WsChannelKind::Market);

    let first = monitor.record_connected(&mut state, "conn-1", ts(10, 0, 0));
    assert_eq!(first.status, WsSessionStatus::Connected);
    assert_eq!(first.reconnect_total, 0);

    let disconnected = monitor.record_disconnected(&mut state, "network_gap", ts(10, 0, 5));
    assert_eq!(disconnected.unwrap().status, WsSessionStatus::Disconnected);

    let second = monitor.record_connected(&mut state, "conn-2", ts(10, 0, 8));
    assert_eq!(second.status, WsSessionStatus::Reconnected);
    assert_eq!(second.reconnect_total, 1);
    assert_eq!(state.connection_id.as_deref(), Some("conn-2"));
}

#[test]
fn duplicate_disconnect_without_active_connection_is_ignored() {
    let monitor = WsSessionMonitor::new(WsChannelKind::User);
    let mut state = WsSessionState::new(WsChannelKind::User);

    assert!(monitor.record_disconnected(&mut state, "not_connected", ts(10, 1, 0)).is_none());
}

fn ts(hour: u32, minute: u32, second: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, 25, hour, minute, second).single().unwrap()
}
```

- [ ] **Step 2: Run the test to verify failure**

Run:

```bash
cargo test -p venue-polymarket reconnect_after_disconnect_increments_counter_and_updates_connection_id -- --exact
```

Expected: FAIL because `WsSessionMonitor`, `WsSessionState`, and `WsSessionStatus` do not exist yet.

- [ ] **Step 3: Implement focused session lifecycle types**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsSessionStatus {
    Connected,
    Reconnected,
    Disconnected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsSessionEvent {
    pub channel: WsChannelKind,
    pub connection_id: String,
    pub status: WsSessionStatus,
    pub reconnect_total: u64,
    pub observed_at: DateTime<Utc>,
    pub disconnect_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsSessionState {
    pub channel: WsChannelKind,
    pub connection_id: Option<String>,
    pub connected: bool,
    pub reconnect_total: u64,
    pub last_connected_at: Option<DateTime<Utc>>,
    pub last_disconnected_at: Option<DateTime<Utc>>,
    pub last_disconnect_reason: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct WsSessionMonitor {
    channel: WsChannelKind,
}
```

Implementation notes:

- keep this separate from `ws_market.rs` and `ws_user.rs`; parsing and session lifecycle are different responsibilities
- do not emit metrics here; this task only creates truthful lifecycle state and events

- [ ] **Step 4: Export the new surface from `lib.rs`**

```rust
mod ws_session;

pub use ws_session::{WsSessionEvent, WsSessionMonitor, WsSessionState, WsSessionStatus};
```

- [ ] **Step 5: Run the websocket-session tests**

Run:

```bash
cargo test -p venue-polymarket ws_session -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/venue-polymarket/src/ws_session.rs crates/venue-polymarket/src/lib.rs crates/venue-polymarket/tests/ws_session.rs
git commit -m "feat: add websocket session lifecycle state"
```

## Task 3: Add Repo-Owned Websocket Producer Instrumentation

**Files:**
- Create: `crates/venue-polymarket/src/instrumentation.rs`
- Modify: `crates/venue-polymarket/src/lib.rs`
- Create: `crates/venue-polymarket/tests/support/mod.rs`
- Create: `crates/venue-polymarket/tests/producer_observability.rs`

- [ ] **Step 1: Write the failing websocket producer observability test**

```rust
use observability::{
    bootstrap_observability,
    field_keys,
    metric_dimensions::Channel,
    span_names,
    MetricDimension,
    MetricDimensions,
};
use venue_polymarket::{VenueProducerInstrumentation, WsChannelKind, WsSessionMonitor, WsSessionState};

#[test]
fn reconnect_event_emits_repo_owned_span_and_channel_counter() {
    let observability = bootstrap_observability("venue-polymarket-test");
    let instrumentation = VenueProducerInstrumentation::enabled(observability.recorder());
    let monitor = WsSessionMonitor::new(WsChannelKind::Market);
    let mut state = WsSessionState::new(WsChannelKind::Market);

    monitor.record_connected(&mut state, "conn-1", ts(10, 0, 0));
    let reconnect = monitor.record_disconnected(&mut state, "network_gap", ts(10, 0, 5)).unwrap();
    instrumentation.record_ws_session_event(&reconnect);
    let reconnect = monitor.record_connected(&mut state, "conn-2", ts(10, 0, 8));

    let (captured_spans, ()) = capture_spans(|| instrumentation.record_ws_session_event(&reconnect));

    let dims = MetricDimensions::new([MetricDimension::Channel(Channel::Market)]);
    assert_eq!(
        observability.registry().snapshot().counter_with_dimensions(
            observability.metrics().websocket_reconnect_total.key(),
            &dims
        ),
        Some(1)
    );

    let span = captured_spans
        .iter()
        .find(|span| span.name == span_names::VENUE_WS_SESSION)
        .expect("venue websocket span missing");
    assert_eq!(span.field(field_keys::CHANNEL).map(String::as_str), Some("\"market\""));
    assert_eq!(span.field(field_keys::SESSION_STATUS).map(String::as_str), Some("\"reconnected\""));
}
```

- [ ] **Step 2: Run the test to verify failure**

Run:

```bash
cargo test -p venue-polymarket reconnect_event_emits_repo_owned_span_and_channel_counter -- --exact
```

Expected: FAIL because `VenueProducerInstrumentation` and span emission do not exist yet.

- [ ] **Step 3: Implement the instrumentation adapter**

```rust
#[derive(Debug, Clone, Default)]
pub struct VenueProducerInstrumentation {
    recorder: Option<RuntimeMetricsRecorder>,
}

impl VenueProducerInstrumentation {
    pub fn disabled() -> Self { ... }

    pub fn enabled(recorder: RuntimeMetricsRecorder) -> Self {
        Self { recorder: Some(recorder) }
    }

    pub fn record_ws_session_event(&self, event: &WsSessionEvent) {
        let channel = match event.channel {
            WsChannelKind::Market => metric_dimensions::Channel::Market,
            WsChannelKind::User => metric_dimensions::Channel::User,
        };

        tracing::info_span!(
            span_names::VENUE_WS_SESSION,
            channel = channel.as_pair().1,
            connection_id = %event.connection_id,
            session_status = ?event.status,
        )
        .in_scope(|| {
            if matches!(event.status, WsSessionStatus::Reconnected) {
                if let Some(recorder) = &self.recorder {
                    recorder.increment_websocket_reconnect_total(
                        1,
                        MetricDimensions::new([MetricDimension::Channel(channel)]),
                    );
                }
            }
        });
    }
}
```

Implementation notes:

- only reconnects increment `websocket_reconnect_total`
- initial connect and disconnect still emit structured spans, but do not inflate reconnect counters
- keep this adapter local to `venue-polymarket`; do not thread observability directly into parser types

- [ ] **Step 4: Add a shared tracing capture helper for venue integration tests**

Create `crates/venue-polymarket/tests/support/mod.rs` with the same focused span-capture pattern already used in `app-live` integration tests:

```rust
#[derive(Debug, Clone)]
pub struct CapturedSpan {
    pub name: String,
    pub fields: BTreeMap<String, String>,
}

pub fn capture_spans<T>(f: impl FnOnce() -> T) -> (Vec<CapturedSpan>, T) {
    // small test subscriber that records span names and fields
}
```

- [ ] **Step 5: Export the adapter**

```rust
mod instrumentation;

pub use instrumentation::VenueProducerInstrumentation;
```

- [ ] **Step 6: Run the venue-polymarket producer tests**

Run:

```bash
cargo test -p venue-polymarket producer_observability -- --nocapture
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/venue-polymarket/src/instrumentation.rs crates/venue-polymarket/src/lib.rs crates/venue-polymarket/tests/support/mod.rs crates/venue-polymarket/tests/producer_observability.rs
git commit -m "feat: add websocket producer observability"
```

## Task 4: Add Heartbeat Producer Observability

**Files:**
- Modify: `crates/venue-polymarket/src/heartbeat.rs`
- Modify: `crates/venue-polymarket/src/instrumentation.rs`
- Modify: `crates/venue-polymarket/tests/heartbeat.rs`
- Modify: `crates/venue-polymarket/tests/producer_observability.rs`

- [ ] **Step 1: Write the failing heartbeat producer observability test**

```rust
use chrono::{Duration, TimeZone, Utc};
use observability::{bootstrap_observability, field_keys, span_names};
use venue_polymarket::{
    HeartbeatReconcileReason, OrderHeartbeatMonitor, OrderHeartbeatState, VenueProducerInstrumentation,
};

#[test]
fn missed_heartbeat_records_freshness_and_structured_status() {
    let observability = bootstrap_observability("venue-polymarket-test");
    let instrumentation = VenueProducerInstrumentation::enabled(observability.recorder());
    let monitor = OrderHeartbeatMonitor::new(Duration::seconds(30));
    let mut state = OrderHeartbeatState {
        heartbeat_id: Some("hb-1".to_owned()),
        last_success_at: ts(10, 0, 0),
        reconcile_attention_since: None,
        reconcile_reason: None,
        requires_reconcile_attention: false,
    };

    let (captured_spans, reason) = capture_spans(|| {
        let reason = monitor.reconcile_trigger(&mut state, ts(10, 0, 31)).unwrap();
        instrumentation.record_heartbeat_attention(&state, reason, ts(10, 0, 31));
        reason
    });

    assert_eq!(reason, HeartbeatReconcileReason::MissedHeartbeat);
    assert_eq!(
        observability.registry().snapshot().gauge(
            observability.metrics().heartbeat_freshness.key()
        ),
        Some(31.0)
    );

    let span = captured_spans
        .iter()
        .find(|span| span.name == span_names::VENUE_HEARTBEAT)
        .expect("heartbeat span missing");
    assert_eq!(span.field(field_keys::HEARTBEAT_STATUS).map(String::as_str), Some("\"missed\""));
}
```

- [ ] **Step 2: Run the test to verify failure**

Run:

```bash
cargo test -p venue-polymarket missed_heartbeat_records_freshness_and_structured_status -- --exact
```

Expected: FAIL because the heartbeat instrumentation helpers do not exist yet.

- [ ] **Step 3: Expose the minimum heartbeat lifecycle data needed by instrumentation**

```rust
impl HeartbeatReconcileReason {
    pub const fn as_status(self) -> &'static str {
        match self {
            Self::MissedHeartbeat => "missed",
            Self::InvalidHeartbeat => "invalid",
        }
    }
}
```

Implementation notes:

- keep heartbeat core logic in `heartbeat.rs`
- only expose formatting and age calculation helpers needed by instrumentation; do not embed recorder calls into the monitor itself

- [ ] **Step 4: Extend the instrumentation adapter for heartbeat success and attention**

```rust
impl VenueProducerInstrumentation {
    pub fn record_heartbeat_success(
        &self,
        state: &OrderHeartbeatState,
        at: DateTime<Utc>,
    ) {
        if let Some(recorder) = &self.recorder {
            recorder.record_heartbeat_freshness(
                (at - state.last_success_at).num_seconds() as f64,
            );
        }

        tracing::info_span!(
            span_names::VENUE_HEARTBEAT,
            heartbeat_id = ?state.heartbeat_id,
            heartbeat_status = "success",
        )
        .in_scope(|| {});
    }

    pub fn record_heartbeat_attention(
        &self,
        state: &OrderHeartbeatState,
        reason: HeartbeatReconcileReason,
        at: DateTime<Utc>,
    ) {
        if let Some(recorder) = &self.recorder {
            recorder.record_heartbeat_freshness(
                (at - state.last_success_at).num_seconds() as f64,
            );
        }

        tracing::warn_span!(
            span_names::VENUE_HEARTBEAT,
            heartbeat_id = ?state.heartbeat_id,
            heartbeat_status = reason.as_status(),
        )
        .in_scope(|| {});
    }
}
```

- [ ] **Step 5: Run the venue-polymarket test slice**

Run:

```bash
cargo test -p venue-polymarket heartbeat -- --nocapture
cargo test -p venue-polymarket producer_observability -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/venue-polymarket/src/heartbeat.rs crates/venue-polymarket/src/instrumentation.rs crates/venue-polymarket/tests/heartbeat.rs crates/venue-polymarket/tests/producer_observability.rs
git commit -m "feat: add heartbeat producer observability"
```

## Task 5: Export And Document The New Producer Surface

**Files:**
- Modify: `crates/venue-polymarket/src/lib.rs`
- Modify: `README.md`

- [ ] **Step 1: Write the failing documentation expectation as a grep check**

Run:

```bash
rg -n "websocket/heartbeat producer observability" README.md
```

Expected: no matches.

- [ ] **Step 2: Update crate exports and README**

```rust
pub use instrumentation::VenueProducerInstrumentation;
pub use ws_session::{WsSessionEvent, WsSessionMonitor, WsSessionState, WsSessionStatus};
```

README note to add:

```md
- `venue-polymarket` now exposes repo-owned websocket-session and heartbeat producer observability primitives.
- `app-live` still does not claim full live websocket or heartbeat task wiring unless those loops are actually present.
```

- [ ] **Step 3: Run focused verification**

Run:

```bash
cargo test -p observability --offline
cargo test -p venue-polymarket --offline
```

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/venue-polymarket/src/lib.rs README.md
git commit -m "docs: describe venue producer observability slice"
```

## Final Verification

- [ ] Run the full targeted verification for this plan:

```bash
cargo fmt --all --check
cargo clippy -p observability -p venue-polymarket --all-targets -- -D warnings
cargo test -p observability
cargo test -p venue-polymarket
```

Expected: PASS.

- [ ] If the online registry is unavailable in the isolated worktree, rerun the same commands with `--offline` where supported and note that choice in the delivery handoff rather than mutating the scope of the implementation.

- [ ] Commit any final README/test cleanups separately:

```bash
git add README.md crates/observability crates/venue-polymarket
git commit -m "chore: finalize wave1a venue observability slice"
```
