use std::{future::Future, pin::Pin};

use chrono::{DateTime, Duration, TimeZone, Utc};
use venue_polymarket::{
    HeartbeatFetchResult, HeartbeatReconcileReason, OrderHeartbeatMonitor, OrderHeartbeatState,
};

use crate::{
    input_tasks::InputTaskEvent,
    instrumentation::AppInstrumentation,
    queues::{FollowUpQueue, FollowUpWork, SnapshotNotice},
};

type HeartbeatPollFuture<'a> =
    Pin<Box<dyn Future<Output = Result<HeartbeatFetchResult, String>> + Send + 'a>>;

pub trait HeartbeatSource: Send {
    fn poll<'a>(&'a mut self, previous_heartbeat_id: Option<&'a str>) -> HeartbeatPollFuture<'a>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionTickResult {
    pub suppressed: bool,
    pub follow_up_backlog: usize,
    pub snapshot_id: Option<String>,
}

#[derive(Debug, Default)]
pub struct MarketDataTaskGroup;

#[derive(Debug, Default)]
pub struct UserStateTaskGroup;

#[derive(Debug)]
pub struct HeartbeatTaskGroup<S> {
    source: S,
    monitor: OrderHeartbeatMonitor,
    state: OrderHeartbeatState,
    next_journal_seq: i64,
    session_id: String,
    scope_id: String,
    now: chrono::DateTime<Utc>,
    instrumentation: AppInstrumentation,
}

#[derive(Debug, Default)]
pub struct RelayerTaskGroup;

#[derive(Debug, Default)]
pub struct MetadataTaskGroup;

#[derive(Debug, Default)]
pub struct DecisionTaskGroup {
    follow_up: FollowUpQueue,
}

#[derive(Debug, Default)]
pub struct RecoveryTaskGroup;

impl<S> HeartbeatTaskGroup<S>
where
    S: HeartbeatSource,
{
    pub fn for_runtime(source: S, run_session_id: impl Into<String>) -> Self {
        let now = Utc::now();

        Self {
            source,
            monitor: OrderHeartbeatMonitor::new(Duration::seconds(30)),
            state: OrderHeartbeatState {
                heartbeat_id: None,
                last_success_at: now,
                reconcile_attention_since: None,
                reconcile_reason: None,
                requires_reconcile_attention: false,
            },
            next_journal_seq: 1,
            session_id: run_session_id.into(),
            scope_id: "runtime".to_owned(),
            now,
            instrumentation: AppInstrumentation::disabled(),
        }
    }

    pub fn for_tests(source: S) -> Self {
        Self::for_tests_with_run_session_id(source, "session-live")
    }

    fn for_tests_with_run_session_id(source: S, run_session_id: impl Into<String>) -> Self {
        Self::build_test(
            source,
            run_session_id,
            Utc.with_ymd_and_hms(2026, 3, 27, 9, 0, 31).unwrap(),
        )
    }

    fn build_test(
        source: S,
        run_session_id: impl Into<String>,
        now: chrono::DateTime<Utc>,
    ) -> Self {
        Self {
            source,
            monitor: OrderHeartbeatMonitor::new(Duration::seconds(30)),
            state: OrderHeartbeatState {
                heartbeat_id: Some("hb-1".to_owned()),
                last_success_at: Utc.with_ymd_and_hms(2026, 3, 27, 9, 0, 0).unwrap(),
                reconcile_attention_since: None,
                reconcile_reason: None,
                requires_reconcile_attention: false,
            },
            next_journal_seq: 1,
            session_id: run_session_id.into(),
            scope_id: "family-a".to_owned(),
            now,
            instrumentation: AppInstrumentation::disabled(),
        }
    }

    pub async fn tick(&mut self) -> Result<Option<InputTaskEvent>, String> {
        let previous_heartbeat_id = self.state.heartbeat_id.as_deref();
        match self.source.poll(previous_heartbeat_id).await {
            Ok(result) => {
                let attention =
                    self.monitor
                        .record_fetch_result(&mut self.state, &result, self.now);
                Ok(attention.map(|reason| self.runtime_attention_input(reason, None)))
            }
            Err(error) => {
                let attention = self.monitor.reconcile_trigger(&mut self.state, self.now);
                Ok(attention.map(|reason| self.runtime_attention_input(reason, Some(error))))
            }
        }
    }

    fn runtime_attention_input(
        &mut self,
        reason: HeartbeatReconcileReason,
        detail: Option<String>,
    ) -> InputTaskEvent {
        let attention_kind = heartbeat_attention_kind(reason);
        let message = detail.unwrap_or_else(|| heartbeat_attention_reason(reason).to_owned());
        self.instrumentation
            .record_runtime_attention_fact("heartbeat", attention_kind);

        let input = InputTaskEvent::new(
            self.next_journal_seq,
            domain::ExternalFactEvent::runtime_attention_observed(
                "heartbeat",
                self.session_id.clone(),
                format!("heartbeat-attention-{}", self.next_journal_seq),
                self.scope_id.clone(),
                attention_kind,
                message,
                self.now,
            ),
        );
        self.next_journal_seq += 1;
        input
    }

    #[cfg(test)]
    pub(crate) fn test_source_session_id(&self) -> &str {
        &self.session_id
    }
}

impl DecisionTaskGroup {
    pub fn for_tests() -> Self {
        Self::default()
    }

    pub fn seed_pending_reconcile(&mut self, scope_id: &str) {
        self.follow_up.push(FollowUpWork::pending_reconcile(
            scope_id,
            format!("pending-{scope_id}"),
            "seeded follow-up backlog",
        ));
    }

    pub async fn tick(&mut self, notice: SnapshotNotice) -> DecisionTickResult {
        DecisionTickResult {
            suppressed: !self.follow_up.is_empty(),
            follow_up_backlog: self.follow_up.len(),
            snapshot_id: Some(notice.snapshot_id),
        }
    }
}

impl MetadataTaskGroup {
    pub fn discovery_input(
        journal_seq: i64,
        source_session_id: impl Into<String>,
        source_event_id: impl Into<String>,
        family_id: impl Into<String>,
        observed_at: DateTime<Utc>,
    ) -> InputTaskEvent {
        InputTaskEvent::family_discovery_observed(
            journal_seq,
            source_session_id,
            source_event_id,
            family_id,
            observed_at,
        )
    }

    pub fn backfill_input(
        journal_seq: i64,
        source_session_id: impl Into<String>,
        source_event_id: impl Into<String>,
        family_id: impl Into<String>,
        cursor: impl Into<String>,
        complete: bool,
        observed_at: DateTime<Utc>,
    ) -> InputTaskEvent {
        InputTaskEvent::family_backfill_observed(
            journal_seq,
            source_session_id,
            source_event_id,
            family_id,
            cursor,
            complete,
            observed_at,
        )
    }
}

fn heartbeat_attention_kind(reason: HeartbeatReconcileReason) -> &'static str {
    match reason {
        HeartbeatReconcileReason::MissedHeartbeat => "missed_heartbeat",
        HeartbeatReconcileReason::InvalidHeartbeat => "invalid_heartbeat",
    }
}

fn heartbeat_attention_reason(reason: HeartbeatReconcileReason) -> &'static str {
    match reason {
        HeartbeatReconcileReason::MissedHeartbeat => "heartbeat freshness exceeded threshold",
        HeartbeatReconcileReason::InvalidHeartbeat => "heartbeat response was invalid",
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use venue_polymarket::HeartbeatFetchResult;

    use super::{HeartbeatSource, HeartbeatTaskGroup};

    #[test]
    fn runtime_originated_heartbeat_fact_uses_run_session_id_as_source_session_id() {
        let emitted = run_async(async {
            let mut group = HeartbeatTaskGroup::for_tests_with_run_session_id(
                ScriptedHeartbeatSource::timeout(),
                "run-session-42",
            );
            group.tick().await.unwrap().expect("runtime attention fact")
        });

        assert_eq!(emitted.event.source_kind, "runtime_attention");
        assert_eq!(emitted.event.source_session_id, "run-session-42");
        assert_eq!(emitted.event.payload.kind(), "runtime_attention_observed");
    }

    #[test]
    fn runtime_heartbeat_group_uses_runtime_defaults_instead_of_test_seeds() {
        let before = Utc::now();
        let group = HeartbeatTaskGroup::for_runtime(
            ScriptedHeartbeatSource::timeout(),
            Some("run-session-9"),
        );
        let after = Utc::now();

        assert_eq!(group.session_id, "run-session-9");
        assert_eq!(group.scope_id, "runtime");
        assert_eq!(group.state.heartbeat_id, None);
        assert!(group.now >= before);
        assert!(group.now <= after);
        assert!(group.state.last_success_at >= before - Duration::seconds(1));
        assert!(group.state.last_success_at <= after + Duration::seconds(1));
    }

    #[derive(Debug)]
    struct ScriptedHeartbeatSource {
        result: Result<HeartbeatFetchResult, String>,
    }

    impl ScriptedHeartbeatSource {
        fn timeout() -> Self {
            Self {
                result: Err("heartbeat timeout".to_owned()),
            }
        }
    }

    impl HeartbeatSource for ScriptedHeartbeatSource {
        fn poll<'a>(
            &'a mut self,
            _previous_heartbeat_id: Option<&'a str>,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<HeartbeatFetchResult, String>> + Send + 'a>,
        > {
            let result = self.result.clone();
            Box::pin(async move { result })
        }
    }

    fn run_async<F>(future: F) -> F::Output
    where
        F: std::future::Future,
    {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("test runtime")
            .block_on(future)
    }
}
