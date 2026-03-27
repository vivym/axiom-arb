use app_live::{
    CandidateArtifactRender, CandidateBridge, CandidateNotice, CandidateNoticeQueue,
    DiscoveryReport, DiscoverySupervisor, InputTaskEvent, SnapshotDispatchQueue, SnapshotNotice,
};
use chrono::{TimeZone, Utc};
use domain::{
    CandidatePolicyAnchor, CandidateTargetSet, DiscoverySourceAnchor, EventFamilyId,
    FamilyDiscoveryRecord,
};
use state::{
    CandidateProjectionReadiness, CandidatePublication, DirtyDomain, StateApplier, StateStore,
};

#[test]
fn discovery_supervisor_publishes_candidate_target_set_without_waking_live_dispatch() {
    let publication = ready_candidate_publication();
    let candidate_notice = CandidateNotice::from_publication(
        &publication,
        [DirtyDomain::Candidates],
        Some("targets-rev-operator"),
        false,
    );

    let mut candidate_queue = CandidateNoticeQueue::default();
    candidate_queue.push(candidate_notice);

    let mut live_dispatch = SnapshotDispatchQueue::default();
    live_dispatch.push(SnapshotNotice::new(
        publication.publication_id.clone(),
        publication.state_version,
        [DirtyDomain::Candidates],
    ));

    let mut supervisor = DiscoverySupervisor::for_tests(candidate_queue);
    let report = run_async(async {
        supervisor
            .tick_candidate_generation_for_tests()
            .await
            .expect("candidate generation report")
    });

    assert_eq!(
        report,
        DiscoveryReport {
            candidate_revision: Some("candidate-pub-7".to_owned()),
            adoptable_revision: Some("adoptable-candidate-pub-7".to_owned()),
            operator_target_revision: Some("targets-rev-operator".to_owned()),
            live_dispatch_woken: false,
            disposition: "adoptable".to_owned(),
        }
    );
    assert!(live_dispatch.coalesced().is_empty());
}

#[test]
fn candidate_bridge_renders_adoptable_revision_with_operator_target_revision() {
    let bridge = CandidateBridge::for_tests();
    let candidate_set = CandidateTargetSet::new(
        "candidate-bridge-9",
        "snapshot-9",
        FamilyDiscoveryRecord::new(
            EventFamilyId::from("family-bridge"),
            DiscoverySourceAnchor::new("metadata_refresh", "session-9", "evt-9", "v1-refresh"),
            Utc.with_ymd_and_hms(2026, 3, 28, 11, 0, 0).unwrap(),
        )
        .with_backfill_cursor("cursor-9"),
        CandidatePolicyAnchor::new("candidate-generation", "policy-v1"),
        vec![],
    );

    let render = bridge
        .render(&candidate_set, Some("targets-rev-9"))
        .expect("candidate render");

    assert_eq!(
        render,
        CandidateArtifactRender {
            candidate: persistence::models::CandidateTargetSetRow {
                candidate_revision: "candidate-bridge-9".to_owned(),
                snapshot_id: "snapshot-9".to_owned(),
                source_revision: "evt-9".to_owned(),
                payload: render.candidate.payload.clone(),
            },
            adoptable: persistence::models::AdoptableTargetRevisionRow {
                adoptable_revision: "adoptable-candidate-bridge-9".to_owned(),
                candidate_revision: "candidate-bridge-9".to_owned(),
                rendered_operator_target_revision: "targets-rev-9".to_owned(),
                payload: render.adoptable.payload.clone(),
            },
        }
    );
}

#[test]
fn discovery_supervisor_defers_halted_candidate_without_rendering_adoption_outputs() {
    let publication = ready_candidate_publication();
    let candidate_notice = CandidateNotice::from_publication(
        &publication,
        [DirtyDomain::Candidates],
        Some("targets-rev-operator"),
        true,
    );

    let mut candidate_queue = CandidateNoticeQueue::default();
    candidate_queue.push(candidate_notice);

    let mut live_dispatch = SnapshotDispatchQueue::default();
    live_dispatch.push(SnapshotNotice::new(
        publication.publication_id.clone(),
        publication.state_version,
        [DirtyDomain::Candidates],
    ));

    let mut supervisor = DiscoverySupervisor::for_tests(candidate_queue);
    let report = run_async(async {
        supervisor
            .tick_candidate_generation_for_tests()
            .await
            .expect("candidate generation report")
    });

    assert_eq!(
        report,
        DiscoveryReport {
            candidate_revision: Some("candidate-pub-7".to_owned()),
            adoptable_revision: None,
            operator_target_revision: None,
            live_dispatch_woken: false,
            disposition: "deferred".to_owned(),
        }
    );
    assert!(live_dispatch.coalesced().is_empty());
}

#[test]
fn discovery_supervisor_excludes_weak_candidate_without_rendering_adoption_outputs() {
    let publication = ready_candidate_publication_with_family_id("   ");
    let candidate_notice = CandidateNotice::from_publication(
        &publication,
        [DirtyDomain::Candidates],
        Some("targets-rev-operator"),
        false,
    );

    let mut candidate_queue = CandidateNoticeQueue::default();
    candidate_queue.push(candidate_notice);

    let mut live_dispatch = SnapshotDispatchQueue::default();
    live_dispatch.push(SnapshotNotice::new(
        publication.publication_id.clone(),
        publication.state_version,
        [DirtyDomain::Candidates],
    ));

    let mut supervisor = DiscoverySupervisor::for_tests(candidate_queue);
    let report = run_async(async {
        supervisor
            .tick_candidate_generation_for_tests()
            .await
            .expect("candidate generation report")
    });

    assert_eq!(
        report,
        DiscoveryReport {
            candidate_revision: Some("candidate-pub-weak".to_owned()),
            adoptable_revision: None,
            operator_target_revision: None,
            live_dispatch_woken: false,
            disposition: "excluded".to_owned(),
        }
    );
    assert!(live_dispatch.coalesced().is_empty());
}

#[test]
fn discovery_and_backfill_input_helpers_emit_through_ingress_path() {
    let discovered_at = Utc.with_ymd_and_hms(2026, 3, 28, 12, 0, 0).unwrap();

    let discovery = InputTaskEvent::family_discovery_observed(
        7,
        "metadata-refresh-1",
        "evt-1",
        "family-a",
        discovered_at,
    );
    let backfill = InputTaskEvent::family_backfill_observed(
        8,
        "metadata-refresh-1",
        "evt-2",
        "family-a",
        "cursor-2",
        true,
        discovered_at,
    );

    let mut store = StateStore::default();
    StateApplier::new(&mut store)
        .apply(discovery.journal_seq, discovery.into_state_fact_input())
        .expect("discovery fact applies");
    StateApplier::new(&mut store)
        .apply(backfill.journal_seq, backfill.into_state_fact_input())
        .expect("backfill fact applies");

    let records = store.family_discovery_records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].family_id.as_str(), "family-a");
    assert_eq!(records[0].backfill_cursor.as_deref(), Some("cursor-2"));
    assert!(records[0].backfill_completed_at.is_some());
}

fn ready_candidate_publication() -> CandidatePublication {
    ready_candidate_publication_with_family_id("family-7")
}

fn ready_candidate_publication_with_family_id(family_id: &str) -> CandidatePublication {
    let discovered_at = Utc.with_ymd_and_hms(2026, 3, 28, 10, 0, 0).unwrap();
    let discovery = domain::ExternalFactEvent::family_discovery_observed(
        "metadata-refresh-1",
        "evt-7",
        family_id,
        discovered_at,
    );
    let backfill = domain::ExternalFactEvent::family_backfill_observed(
        "metadata-refresh-1",
        "evt-8",
        family_id,
        "cursor-7",
        true,
        discovered_at,
    );

    let mut store = StateStore::default();
    StateApplier::new(&mut store)
        .apply(7, discovery)
        .expect("discovery fact applies");
    StateApplier::new(&mut store)
        .apply(8, backfill)
        .expect("backfill fact applies");

    CandidatePublication::from_store(
        &store,
        CandidateProjectionReadiness::ready(if family_id.trim().is_empty() {
            "candidate-pub-weak"
        } else {
            "candidate-pub-7"
        }),
    )
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
