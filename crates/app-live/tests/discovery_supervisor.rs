use app_live::{
    CandidateArtifactRender, CandidateBridge, CandidateNotice, CandidateNoticeQueue,
    CandidateRestrictionTruth, DiscoveryReport, DiscoverySupervisor, InputTaskEvent,
    SnapshotDispatchQueue, SnapshotNotice,
};
use chrono::{TimeZone, Utc};
use domain::{
    AdoptableTargetRevision, CandidatePolicyAnchor, CandidateTargetSet, DiscoverySourceAnchor,
    EventFamilyId, FamilyDiscoveryRecord,
};
use serde_json::json;
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
        CandidateRestrictionTruth::eligible(),
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
            target_count: 1,
            adoptable_target_count: 1,
            deferred_target_count: 0,
            excluded_target_count: 0,
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
    )
    .with_adoptable_revision(AdoptableTargetRevision::new(
        "adoptable-candidate-bridge-9",
        "snapshot-9",
        "policy-v1",
    ));

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
                payload: json!({
                    "candidate_revision": "candidate-bridge-9",
                    "snapshot_id": "snapshot-9",
                    "source_anchor": {
                        "source_kind": "metadata_refresh",
                        "source_session_id": "session-9",
                        "source_event_id": "evt-9",
                        "normalizer_version": "v1-refresh",
                    },
                    "policy_name": "candidate-generation",
                    "candidate_policy_version": "policy-v1",
                    "bridge_policy_version": "bridge-policy-v1",
                    "source_revision": "evt-9",
                    "target_count": 0,
                    "advisory_pricing": {
                        "price_band_bps": 25,
                        "size_cap_contracts": 1,
                        "mode": "advisory_only",
                        "candidate_count": 0,
                    },
                    "warnings": [],
                    "execution_requests": [],
                }),
            },
            adoptable: persistence::models::AdoptableTargetRevisionRow {
                adoptable_revision: "adoptable-candidate-bridge-9".to_owned(),
                candidate_revision: "candidate-bridge-9".to_owned(),
                rendered_operator_target_revision: "targets-rev-9".to_owned(),
                payload: json!({
                    "adoptable_revision": "adoptable-candidate-bridge-9",
                    "candidate_revision": "candidate-bridge-9",
                    "rendered_operator_target_revision": "targets-rev-9",
                    "snapshot_id": "snapshot-9",
                    "source_anchor": {
                        "source_kind": "metadata_refresh",
                        "source_session_id": "session-9",
                        "source_event_id": "evt-9",
                        "normalizer_version": "v1-refresh",
                    },
                    "bridge_policy_version": "bridge-policy-v1",
                    "candidate_policy_version": "policy-v1",
                    "compatibility": {
                        "operator_target_revision_supplied": true,
                        "advisory_only": true,
                    },
                    "warnings": [],
                    "execution_requests": [],
                }),
            },
        }
    );
}

#[test]
fn candidate_bridge_rejects_non_adoptable_candidate_set_even_with_operator_target_revision() {
    let bridge = CandidateBridge::for_tests();
    let candidate_set = CandidateTargetSet::new(
        "candidate-bridge-10",
        "snapshot-10",
        FamilyDiscoveryRecord::new(
            EventFamilyId::from("family-bridge"),
            DiscoverySourceAnchor::new("metadata_refresh", "session-10", "evt-10", "v1-refresh"),
            Utc.with_ymd_and_hms(2026, 3, 28, 11, 5, 0).unwrap(),
        ),
        CandidatePolicyAnchor::new("candidate-generation", "policy-v1"),
        vec![],
    );

    let err = bridge
        .render(&candidate_set, Some("targets-rev-10"))
        .expect_err("non-adoptable candidate set should be rejected");

    assert!(err.contains("adoptable revision"));
}

#[test]
fn discovery_supervisor_reports_adoptable_candidate_without_bridge_output_when_operator_revision_missing(
) {
    let publication = ready_candidate_publication();
    let candidate_notice = CandidateNotice::from_publication(
        &publication,
        [DirtyDomain::Candidates],
        None,
        CandidateRestrictionTruth::eligible(),
    );

    let mut candidate_queue = CandidateNoticeQueue::default();
    candidate_queue.push(candidate_notice);

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
            target_count: 1,
            adoptable_target_count: 1,
            deferred_target_count: 0,
            excluded_target_count: 0,
            live_dispatch_woken: false,
            disposition: "adoptable".to_owned(),
        }
    );
}

#[test]
fn discovery_supervisor_defers_restricted_candidate_without_rendering_adoption_outputs() {
    let publication = ready_candidate_publication();
    let candidate_notice = CandidateNotice::from_publication(
        &publication,
        [DirtyDomain::Candidates],
        Some("targets-rev-operator"),
        CandidateRestrictionTruth::restricted("candidate generation halted by validation truth"),
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
            target_count: 1,
            adoptable_target_count: 0,
            deferred_target_count: 1,
            excluded_target_count: 0,
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
        CandidateRestrictionTruth::eligible(),
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
            target_count: 1,
            adoptable_target_count: 0,
            deferred_target_count: 0,
            excluded_target_count: 1,
            live_dispatch_woken: false,
            disposition: "excluded".to_owned(),
        }
    );
    assert!(live_dispatch.coalesced().is_empty());
}

#[test]
fn discovery_supervisor_keeps_all_discovery_records_in_candidate_targets() {
    let publication = ready_candidate_publication_with_family_ids(&["family-a", "family-b"]);
    let candidate_notice = CandidateNotice::from_publication(
        &publication,
        [DirtyDomain::Candidates],
        Some("targets-rev-multi"),
        CandidateRestrictionTruth::eligible(),
    );

    let mut candidate_queue = CandidateNoticeQueue::default();
    candidate_queue.push(candidate_notice);

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
            candidate_revision: Some("candidate-pub-multi".to_owned()),
            adoptable_revision: Some("adoptable-candidate-pub-multi".to_owned()),
            operator_target_revision: Some("targets-rev-multi".to_owned()),
            target_count: 2,
            adoptable_target_count: 2,
            deferred_target_count: 0,
            excluded_target_count: 0,
            live_dispatch_woken: false,
            disposition: "adoptable".to_owned(),
        }
    );
}

#[test]
fn discovery_supervisor_preserves_per_family_validation_for_mixed_publication() {
    let publication = ready_mixed_candidate_publication();
    let candidate_notice = CandidateNotice::from_publication(
        &publication,
        [DirtyDomain::Candidates],
        Some("targets-rev-mixed"),
        CandidateRestrictionTruth::eligible(),
    );

    let mut candidate_queue = CandidateNoticeQueue::default();
    candidate_queue.push(candidate_notice);

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
            candidate_revision: Some("candidate-pub-mixed".to_owned()),
            adoptable_revision: None,
            operator_target_revision: None,
            target_count: 2,
            adoptable_target_count: 1,
            deferred_target_count: 0,
            excluded_target_count: 1,
            live_dispatch_woken: false,
            disposition: "excluded".to_owned(),
        }
    );
}

#[test]
fn candidate_notice_queue_coalesced_keeps_distinct_operator_or_restriction_variants() {
    let publication = ready_candidate_publication();
    let mut queue = CandidateNoticeQueue::default();
    queue.push(CandidateNotice::from_publication(
        &publication,
        [DirtyDomain::Candidates],
        Some("targets-rev-a"),
        CandidateRestrictionTruth::eligible(),
    ));
    queue.push(CandidateNotice::from_publication(
        &publication,
        [DirtyDomain::Candidates],
        Some("targets-rev-b"),
        CandidateRestrictionTruth::eligible(),
    ));
    queue.push(CandidateNotice::from_publication(
        &publication,
        [DirtyDomain::Candidates],
        Some("targets-rev-a"),
        CandidateRestrictionTruth::restricted("validation hold"),
    ));

    let coalesced = queue.coalesced();

    assert_eq!(coalesced.len(), 3);
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
    let publication_id = if family_id.trim().is_empty() {
        "candidate-pub-weak"
    } else {
        "candidate-pub-7"
    };
    ready_candidate_publication_fixture(publication_id, &[family_id])
}

fn ready_candidate_publication_with_family_ids(family_ids: &[&str]) -> CandidatePublication {
    ready_candidate_publication_fixture("candidate-pub-multi", family_ids)
}

fn ready_mixed_candidate_publication() -> CandidatePublication {
    ready_candidate_publication_fixture("candidate-pub-mixed", &["family-a", "   "])
}

fn ready_candidate_publication_fixture(
    publication_id: &str,
    family_ids: &[&str],
) -> CandidatePublication {
    let discovered_at = Utc.with_ymd_and_hms(2026, 3, 28, 10, 0, 0).unwrap();
    let mut store = StateStore::default();
    for (index, family_id) in family_ids.iter().enumerate() {
        let discovery = domain::ExternalFactEvent::family_discovery_observed(
            "metadata-refresh-1",
            format!("evt-discovery-{index}"),
            *family_id,
            discovered_at,
        );
        let backfill = domain::ExternalFactEvent::family_backfill_observed(
            "metadata-refresh-1",
            format!("evt-backfill-{index}"),
            *family_id,
            format!("cursor-{index}"),
            true,
            discovered_at,
        );
        StateApplier::new(&mut store)
            .apply((index as i64) * 2 + 7, discovery)
            .expect("discovery fact applies");
        StateApplier::new(&mut store)
            .apply((index as i64) * 2 + 8, backfill)
            .expect("backfill fact applies");
    }

    CandidatePublication::from_store(&store, CandidateProjectionReadiness::ready(publication_id))
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
