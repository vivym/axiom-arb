use app_live::{
    source_tasks::build_real_user_shadow_smoke_sources, BootstrapSource, DecisionTaskGroup,
    FollowUpQueue, FollowUpWork, HeartbeatSource, HeartbeatTaskGroup, IngressQueue, InputTaskEvent,
    LocalSignerConfig, ScopeRestriction, ScopeRestrictionKind, SnapshotDispatchQueue,
    SnapshotNotice, StaticSnapshotSource, SupervisorPosture,
};
use chrono::Utc;
use config_schema::{load_raw_config_from_str, ValidatedConfig};
use domain::ExternalFactEvent;
use state::DirtyDomain;
use venue_polymarket::HeartbeatFetchResult;

#[test]
fn global_posture_and_scope_restrictions_are_not_the_same_authority() {
    let posture = SupervisorPosture::DegradedIngress;
    let restriction = ScopeRestriction::reconciling_only("family-a");

    assert!(posture.is_global());
    assert_eq!(restriction.scope_id(), "family-a");
    assert_eq!(restriction.kind(), ScopeRestrictionKind::ReconcilingOnly);
}

#[test]
fn snapshot_dispatch_queue_keeps_latest_stable_snapshot_for_dirty_domain() {
    let mut queue = SnapshotDispatchQueue::default();
    queue.push(SnapshotNotice::new("snapshot-7", 7, [DirtyDomain::Runtime]));
    queue.push(SnapshotNotice::new(
        "snapshot-8",
        8,
        [DirtyDomain::Runtime, DirtyDomain::NegRiskFamilies],
    ));

    let drained = queue.coalesced();

    assert_eq!(
        drained
            .iter()
            .map(|notice| notice.state_version)
            .collect::<Vec<_>>(),
        vec![8]
    );
    assert_eq!(drained.last().unwrap().state_version, 8);
}

#[test]
fn ingress_queue_orders_inputs_by_journal_seq() {
    let mut queue = IngressQueue::default();
    queue.push(sample_input_task_event(9));
    queue.push(sample_input_task_event(7));

    let first = queue.next_after(None).expect("first input");
    let second = queue
        .next_after(Some(first.journal_seq))
        .expect("second input");

    assert_eq!(first.journal_seq, 7);
    assert_eq!(second.journal_seq, 9);
}

#[test]
fn follow_up_queue_preserves_fifo_work_items() {
    let mut queue = FollowUpQueue::default();
    queue.push(FollowUpWork::pending_reconcile(
        "family-a",
        "pending-1",
        "heartbeat freshness exceeded threshold",
    ));
    queue.push(FollowUpWork::recovery(
        "family-b",
        "relayer ambiguity requires recovery",
    ));

    assert_eq!(queue.len(), 2);
    assert_eq!(
        queue.pop_front(),
        Some(FollowUpWork::pending_reconcile(
            "family-a",
            "pending-1",
            "heartbeat freshness exceeded threshold",
        ))
    );
    assert_eq!(
        queue.pop_front(),
        Some(FollowUpWork::recovery(
            "family-b",
            "relayer ambiguity requires recovery",
        ))
    );
    assert!(queue.is_empty());
}

#[test]
fn heartbeat_task_group_emits_runtime_attention_fact_when_freshness_expires() {
    let emitted = run_async(async {
        let mut group = HeartbeatTaskGroup::for_tests(ScriptedHeartbeatSource::timeout());
        group.tick().await.unwrap().expect("runtime attention fact")
    });

    assert_eq!(emitted.event.source_kind, "runtime_attention");
    assert_eq!(emitted.event.payload.kind(), "runtime_attention_observed");
}

#[test]
fn decision_task_group_suppresses_live_expansion_while_follow_up_backlog_exists() {
    let result = run_async(async {
        let mut group = DecisionTaskGroup::for_tests();
        group.seed_pending_reconcile("family-a");
        group
            .tick(
                SnapshotNotice::new("snapshot-9", 9, [DirtyDomain::NegRiskFamilies])
                    .with_projection_readiness(true, true),
            )
            .await
    });

    assert!(result.suppressed);
}

#[test]
fn real_user_shadow_smoke_source_bundle_carries_source_and_signer_configs() {
    let config = live_config_view();
    let smoke = app_live::load_real_user_shadow_smoke_config(&config)
        .expect("smoke config should parse")
        .expect("smoke should be enabled");
    let signer = LocalSignerConfig::try_from(&config).expect("signer config should parse");

    let sources = build_real_user_shadow_smoke_sources(
        smoke.source_config.clone(),
        signer.clone(),
        Some("run-session-77"),
    )
    .expect("source bundle should build");

    assert_eq!(sources.source_config, smoke.source_config);
    assert_eq!(sources.signer_config, signer);
    assert_eq!(sources.snapshot(), StaticSnapshotSource::empty().snapshot());
}

#[test]
fn real_user_shadow_smoke_source_bundle_uses_caller_run_session_id_for_heartbeat() {
    let config = live_config_view();
    let smoke = app_live::load_real_user_shadow_smoke_config(&config)
        .expect("smoke config should parse")
        .expect("smoke should be enabled");
    let signer = LocalSignerConfig::try_from(&config).expect("signer config should parse");

    let sources = build_real_user_shadow_smoke_sources(
        smoke.source_config.clone(),
        signer,
        Some("run-session-77"),
    )
    .expect("source bundle should build");

    assert_eq!(sources.heartbeat.source_session_id(), "run-session-77");
    assert_ne!(sources.heartbeat.source_session_id(), "session-live");
}

fn sample_input_task_event(journal_seq: i64) -> InputTaskEvent {
    InputTaskEvent::new(
        journal_seq,
        ExternalFactEvent::new(
            "market_ws",
            "session-test",
            format!("evt-{journal_seq}"),
            "v1-test",
            Utc::now(),
        ),
    )
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

fn live_config_view() -> config_schema::AppLiveConfigView<'static> {
    let raw = load_raw_config_from_str(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = true

[polymarket.source]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://data-api.polymarket.com"
relayer_host = "https://relayer-v2.polymarket.com"
market_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
user_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60

[polymarket.signer]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key-1"
passphrase = "poly-passphrase-1"
timestamp = "1700000000"
signature = "poly-signature-1"

[polymarket.relayer_auth]
kind = "builder_api_key"
api_key = "builder-api-key-1"
timestamp = "1700000001"
passphrase = "builder-passphrase-1"
signature = "builder-signature-1"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-a"]

[[negrisk.targets]]
family_id = "family-a"

[[negrisk.targets.members]]
condition_id = "condition-1"
token_id = "token-1"
price = "0.43"
quantity = "5"
"#,
    )
    .expect("config should parse");
    let validated = Box::leak(Box::new(
        ValidatedConfig::new(raw).expect("config should validate"),
    ));

    validated
        .for_app_live()
        .expect("live config should validate")
}
