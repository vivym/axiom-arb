use std::collections::BTreeMap;

use app_live::{
    CandidateBridge, CandidateNotice, CandidateNoticeQueue, CandidateRestrictionTruth,
    DiscoveryReport, DiscoverySupervisor, InputTaskEvent, NegRiskFamilyLiveTarget,
    NegRiskMemberLiveTarget, SnapshotDispatchQueue, SnapshotNotice,
};
use chrono::{TimeZone, Utc};
use domain::{
    AdoptableTargetRevision, CandidatePolicyAnchor, CandidateTarget, CandidateTargetSet,
    CandidateValidationResult, DiscoverySourceAnchor, EventFamilyId, FamilyDiscoveryRecord,
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
        sample_rendered_live_targets(),
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

    assert!(report.candidate_revision.is_some());
    assert!(report.adoptable_revision.is_some());
    assert!(report.operator_target_revision.is_some());
    assert_eq!(report.target_count, 1);
    assert_eq!(report.adoptable_target_count, 1);
    assert_eq!(report.deferred_target_count, 0);
    assert_eq!(report.excluded_target_count, 0);
    assert!(!report.live_dispatch_woken);
    assert_eq!(report.disposition, "adoptable");
    assert!(live_dispatch.coalesced().is_empty());
}

#[test]
fn candidate_bridge_derives_future_operator_target_revision_from_rendered_live_targets() {
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
        .render(&candidate_set, None, &sample_rendered_live_targets())
        .expect("candidate render");

    assert!(render
        .candidate
        .strategy_candidate_revision
        .starts_with("strategy-candidate-"));
    assert!(render
        .adoptable
        .adoptable_strategy_revision
        .starts_with("adoptable-strategy-"));
    assert!(render
        .adoptable
        .rendered_operator_strategy_revision
        .starts_with("operator-strategy-"));
    assert_eq!(render.candidate.payload["route_artifact_count"], json!(1));
    assert_eq!(render.candidate.payload["targets"], json!([]));
    assert_eq!(
        route_digest(&render.candidate.payload, "full-set", "default"),
        route_digest(&render.adoptable.payload, "full-set", "default"),
    );
    assert_eq!(
        render.adoptable.payload["rendered_live_targets"],
        json!(sample_rendered_live_targets())
    );
}

#[test]
fn candidate_bridge_serializes_per_family_target_validations_for_mixed_candidate_set() {
    let bridge = CandidateBridge::for_tests();
    let candidate_set = CandidateTargetSet::new(
        "candidate-bridge-mixed",
        "snapshot-mixed",
        FamilyDiscoveryRecord::new(
            EventFamilyId::from("family-a"),
            DiscoverySourceAnchor::new(
                "metadata_refresh",
                "session-mixed",
                "evt-mixed",
                "v1-refresh",
            ),
            Utc.with_ymd_and_hms(2026, 3, 28, 11, 10, 0).unwrap(),
        ),
        CandidatePolicyAnchor::new("candidate-generation", "policy-v1"),
        vec![
            CandidateTarget::new(
                "candidate-target-family-a",
                EventFamilyId::from("family-a"),
                CandidateValidationResult::Adoptable,
            ),
            CandidateTarget::new(
                "candidate-target-family-b",
                EventFamilyId::from("family-b"),
                CandidateValidationResult::Rejected {
                    reason: "candidate excluded by conservative discovery policy".to_owned(),
                },
            ),
        ],
    )
    .with_adoptable_revision(AdoptableTargetRevision::new(
        "adoptable-candidate-bridge-mixed",
        "snapshot-mixed",
        "policy-v1",
    ));

    let render = bridge
        .render(
            &candidate_set,
            Some("targets-rev-mixed"),
            &sample_rendered_live_targets(),
        )
        .expect("candidate render");

    assert_eq!(
        render.candidate.payload["targets"],
        json!([
            {
                "target_id": "candidate-target-family-a",
                "family_id": "family-a",
                "validation": {
                    "status": "adoptable"
                }
            },
            {
                "target_id": "candidate-target-family-b",
                "family_id": "family-b",
                "validation": {
                    "status": "excluded",
                    "reason": "candidate excluded by conservative discovery policy"
                }
            }
        ])
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
        .render(
            &candidate_set,
            Some("targets-rev-10"),
            &sample_rendered_live_targets(),
        )
        .expect_err("non-adoptable candidate set should be rejected");

    assert!(err.contains("adoptable revision"));
}

#[test]
fn discovery_supervisor_materializes_adoptable_output_without_prior_operator_revision() {
    let publication = ready_candidate_publication();
    let candidate_notice = CandidateNotice::from_publication(
        &publication,
        [DirtyDomain::Candidates],
        None,
        sample_rendered_live_targets(),
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

    assert!(report.candidate_revision.is_some());
    assert!(report.adoptable_revision.is_some());
    assert!(report.operator_target_revision.is_some());
    assert_eq!(report.target_count, 1);
    assert_eq!(report.adoptable_target_count, 1);
    assert_eq!(report.deferred_target_count, 0);
    assert_eq!(report.excluded_target_count, 0);
    assert!(!report.live_dispatch_woken);
    assert_eq!(report.disposition, "adoptable");
}

#[test]
fn discovery_supervisor_defers_non_authoritative_notice_when_backfill_is_missing() {
    let publication = discovery_only_candidate_publication();
    assert!(publication
        .view
        .as_ref()
        .expect("candidate publication view")
        .discovery_records[0]
        .backfill_completed_at
        .is_none());
    let candidate_notice = CandidateNotice::from_publication(
        &publication,
        [DirtyDomain::Candidates],
        None,
        sample_rendered_live_targets(),
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

    assert!(report.candidate_revision.is_some());
    assert_eq!(report.adoptable_revision, None);
    assert_eq!(report.operator_target_revision, None);
    assert_eq!(report.target_count, 1);
    assert_eq!(report.adoptable_target_count, 0);
    assert_eq!(report.deferred_target_count, 1);
    assert_eq!(report.excluded_target_count, 0);
    assert!(!report.live_dispatch_woken);
    assert_eq!(report.disposition, "deferred");
}

#[test]
fn discovery_supervisor_authoritative_notice_materializes_adoptable_without_backfill_facts() {
    let publication = discovery_only_candidate_publication();
    assert!(publication
        .view
        .as_ref()
        .expect("candidate publication view")
        .discovery_records[0]
        .backfill_completed_at
        .is_none());
    let candidate_notice = CandidateNotice::authoritative_from_publication(
        &publication,
        [DirtyDomain::Candidates],
        None,
        sample_rendered_live_targets(),
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

    assert!(report.candidate_revision.is_some());
    assert!(report.adoptable_revision.is_some());
    assert!(report.operator_target_revision.is_some());
    assert_eq!(report.target_count, 1);
    assert_eq!(report.adoptable_target_count, 1);
    assert_eq!(report.deferred_target_count, 0);
    assert_eq!(report.excluded_target_count, 0);
    assert!(!report.live_dispatch_woken);
    assert_eq!(report.disposition, "adoptable");
}

#[test]
fn discovery_supervisor_reports_restricted_candidate_as_hard_gate() {
    let publication = ready_candidate_publication();
    let candidate_notice = CandidateNotice::from_publication(
        &publication,
        [DirtyDomain::Candidates],
        Some("targets-rev-operator"),
        sample_rendered_live_targets(),
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

    assert!(report.candidate_revision.is_some());
    assert_eq!(report.adoptable_revision, None);
    assert_eq!(report.operator_target_revision, None);
    assert_eq!(report.target_count, 1);
    assert_eq!(report.adoptable_target_count, 0);
    assert_eq!(report.deferred_target_count, 1);
    assert_eq!(report.excluded_target_count, 0);
    assert!(!report.live_dispatch_woken);
    assert_eq!(report.disposition, "deferred");
    assert_eq!(
        report.warnings,
        vec!["candidate generation halted by validation truth".to_owned()]
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
        sample_rendered_live_targets(),
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

    assert!(report.candidate_revision.is_some());
    assert_eq!(report.adoptable_revision, None);
    assert_eq!(report.operator_target_revision, None);
    assert_eq!(report.target_count, 1);
    assert_eq!(report.adoptable_target_count, 0);
    assert_eq!(report.deferred_target_count, 0);
    assert_eq!(report.excluded_target_count, 1);
    assert!(!report.live_dispatch_woken);
    assert_eq!(report.disposition, "excluded");
    assert!(live_dispatch.coalesced().is_empty());
}

#[test]
fn discovery_supervisor_keeps_all_discovery_records_in_candidate_targets() {
    let publication = ready_candidate_publication_with_family_ids(&["family-a", "family-b"]);
    let candidate_notice = CandidateNotice::from_publication(
        &publication,
        [DirtyDomain::Candidates],
        Some("targets-rev-multi"),
        sample_rendered_live_targets(),
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

    assert!(report.candidate_revision.is_some());
    assert!(report.adoptable_revision.is_some());
    assert!(report.operator_target_revision.is_some());
    assert_eq!(report.target_count, 2);
    assert_eq!(report.adoptable_target_count, 2);
    assert_eq!(report.deferred_target_count, 0);
    assert_eq!(report.excluded_target_count, 0);
    assert!(!report.live_dispatch_woken);
    assert_eq!(report.disposition, "adoptable");
}

#[test]
fn discovery_supervisor_preserves_per_family_validation_for_mixed_publication() {
    let publication = ready_mixed_candidate_publication();
    let candidate_notice = CandidateNotice::from_publication(
        &publication,
        [DirtyDomain::Candidates],
        Some("targets-rev-mixed"),
        sample_rendered_live_targets(),
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

    assert!(report.candidate_revision.is_some());
    assert_eq!(report.adoptable_revision, None);
    assert_eq!(report.operator_target_revision, None);
    assert_eq!(report.target_count, 2);
    assert_eq!(report.adoptable_target_count, 1);
    assert_eq!(report.deferred_target_count, 0);
    assert_eq!(report.excluded_target_count, 1);
    assert!(!report.live_dispatch_woken);
    assert_eq!(report.disposition, "excluded");
}

#[test]
fn discovery_supervisor_reuses_bundle_identity_when_only_publication_provenance_changes() {
    let first_publication = ready_candidate_publication_fixture("candidate-pub-7", &["family-a"]);
    let second_publication =
        ready_candidate_publication_fixture("candidate-pub-rediscovered", &["family-a"]);

    let first_report = discovery_report_for(first_publication);
    let second_report = discovery_report_for(second_publication);

    assert_eq!(
        first_report.candidate_revision, second_report.candidate_revision,
        "publication-only churn should not alter the strategy candidate identity"
    );
    assert_eq!(
        first_report.adoptable_revision, second_report.adoptable_revision,
        "publication-only churn should not alter the adoptable strategy identity"
    );
    assert_eq!(
        first_report.operator_target_revision, second_report.operator_target_revision,
        "publication-only churn should not alter the rendered operator strategy identity"
    );
}

#[test]
fn discovery_supervisor_ignores_readiness_only_advisories_for_bundle_identity() {
    let publication = ready_candidate_publication_fixture("candidate-pub-readiness", &["family-a"]);
    let eligible_report =
        discovery_report_for_notice(CandidateNotice::authoritative_from_publication(
            &publication,
            [DirtyDomain::Candidates],
            None,
            sample_rendered_live_targets(),
            CandidateRestrictionTruth::eligible(),
        ));
    let restricted_report =
        discovery_report_for_notice(CandidateNotice::authoritative_from_publication(
            &publication,
            [DirtyDomain::Candidates],
            None,
            sample_rendered_live_targets(),
            CandidateRestrictionTruth::advisory("connectivity degraded"),
        ));

    assert_eq!(
        eligible_report.candidate_revision, restricted_report.candidate_revision,
        "readiness-only restriction should not alter the strategy candidate identity"
    );
    assert_eq!(
        eligible_report.adoptable_revision, restricted_report.adoptable_revision,
        "readiness-only restriction should not alter the adoptable strategy identity"
    );
    assert_eq!(
        eligible_report.operator_target_revision, restricted_report.operator_target_revision,
        "readiness-only restriction should not alter the rendered operator strategy identity"
    );
    assert_eq!(restricted_report.disposition, "adoptable");
    assert_eq!(restricted_report.deferred_target_count, 0);
    assert_eq!(eligible_report.warnings, Vec::<String>::new());
    assert_eq!(
        restricted_report.warnings,
        vec!["connectivity degraded".to_owned()]
    );
}

#[test]
fn candidate_bridge_carries_forward_full_set_route_digest_when_neg_risk_changes() {
    let bridge = CandidateBridge::for_tests();
    let first_candidate =
        ready_candidate_publication_fixture("candidate-pub-route-a", &["family-a"])
            .view
            .expect("first candidate view")
            .discovery_records;
    let second_candidate =
        ready_candidate_publication_fixture("candidate-pub-route-b", &["family-a", "family-b"])
            .view
            .expect("second candidate view")
            .discovery_records;
    let second_rendered_live_targets = BTreeMap::from([
        (
            "family-a".to_owned(),
            sample_rendered_live_targets()
                .get("family-a")
                .expect("family-a target")
                .clone(),
        ),
        (
            "family-b".to_owned(),
            NegRiskFamilyLiveTarget {
                family_id: "family-b".to_owned(),
                members: vec![NegRiskMemberLiveTarget {
                    condition_id: "condition-2".to_owned(),
                    token_id: "token-2".to_owned(),
                    price: rust_decimal::Decimal::new(44, 2),
                    quantity: rust_decimal::Decimal::new(6, 0),
                }],
            },
        ),
    ]);

    let first_render = bridge
        .render(
            &CandidateTargetSet::new(
                "candidate-pub-route-a",
                "candidate-pub-route-a",
                first_candidate[0].clone(),
                CandidatePolicyAnchor::new("candidate-generation", "policy-v1"),
                vec![CandidateTarget::new(
                    "candidate-target-family-a",
                    EventFamilyId::from("family-a"),
                    CandidateValidationResult::Adoptable,
                )],
            )
            .with_adoptable_revision(AdoptableTargetRevision::new(
                "adoptable-candidate-pub-route-a",
                "candidate-pub-route-a",
                "policy-v1",
            )),
            None,
            &sample_rendered_live_targets(),
        )
        .expect("first candidate render");
    let second_render = bridge
        .render(
            &CandidateTargetSet::new(
                "candidate-pub-route-b",
                "candidate-pub-route-b",
                second_candidate[0].clone(),
                CandidatePolicyAnchor::new("candidate-generation", "policy-v1"),
                vec![
                    CandidateTarget::new(
                        "candidate-target-family-a",
                        EventFamilyId::from("family-a"),
                        CandidateValidationResult::Adoptable,
                    ),
                    CandidateTarget::new(
                        "candidate-target-family-b",
                        EventFamilyId::from("family-b"),
                        CandidateValidationResult::Adoptable,
                    ),
                ],
            )
            .with_adoptable_revision(AdoptableTargetRevision::new(
                "adoptable-candidate-pub-route-b",
                "candidate-pub-route-b",
                "policy-v1",
            )),
            None,
            &second_rendered_live_targets,
        )
        .expect("second candidate render");

    assert_eq!(
        route_digest(&first_render.candidate.payload, "full-set", "default"),
        route_digest(&second_render.candidate.payload, "full-set", "default"),
        "full-set route digest should remain reusable when neg-risk routes change"
    );
    assert_ne!(
        first_render.candidate.strategy_candidate_revision,
        second_render.candidate.strategy_candidate_revision,
        "bundle identity should change when neg-risk route content changes"
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
        sample_rendered_live_targets(),
        CandidateRestrictionTruth::eligible(),
    ));
    queue.push(CandidateNotice::from_publication(
        &publication,
        [DirtyDomain::Candidates],
        Some("targets-rev-b"),
        sample_rendered_live_targets(),
        CandidateRestrictionTruth::eligible(),
    ));
    queue.push(CandidateNotice::from_publication(
        &publication,
        [DirtyDomain::Candidates],
        Some("targets-rev-a"),
        sample_rendered_live_targets(),
        CandidateRestrictionTruth::restricted("validation hold"),
    ));

    let coalesced = queue.coalesced();

    assert_eq!(coalesced.len(), 3);
}

#[test]
fn candidate_notice_queue_coalesced_keeps_authoritative_notices_distinct() {
    let publication = ready_candidate_publication();
    let mut queue = CandidateNoticeQueue::default();
    queue.push(CandidateNotice::from_publication(
        &publication,
        [DirtyDomain::Candidates],
        Some("targets-rev-a"),
        sample_rendered_live_targets(),
        CandidateRestrictionTruth::eligible(),
    ));
    queue.push(CandidateNotice::authoritative_from_publication(
        &publication,
        [DirtyDomain::Candidates],
        Some("targets-rev-a"),
        sample_rendered_live_targets(),
        CandidateRestrictionTruth::eligible(),
    ));

    let coalesced = queue.coalesced();

    assert_eq!(coalesced.len(), 2);
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

fn discovery_only_candidate_publication() -> CandidatePublication {
    let discovered_at = Utc.with_ymd_and_hms(2026, 3, 28, 10, 0, 0).unwrap();
    let mut store = StateStore::default();
    let discovery = domain::ExternalFactEvent::family_discovery_observed(
        "metadata-refresh-1",
        "evt-discovery-only-0",
        "family-a",
        discovered_at,
    );
    StateApplier::new(&mut store)
        .apply(7, discovery)
        .expect("discovery fact applies");

    CandidatePublication::from_store(
        &store,
        CandidateProjectionReadiness::ready("candidate-pub-discovery-only"),
    )
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

fn discovery_report_for(publication: CandidatePublication) -> DiscoveryReport {
    discovery_report_for_notice(CandidateNotice::authoritative_from_publication(
        &publication,
        [DirtyDomain::Candidates],
        None,
        sample_rendered_live_targets(),
        CandidateRestrictionTruth::eligible(),
    ))
}

fn discovery_report_for_notice(candidate_notice: CandidateNotice) -> DiscoveryReport {
    let mut queue = CandidateNoticeQueue::default();
    queue.push(candidate_notice);

    let mut supervisor = DiscoverySupervisor::for_tests(queue);
    run_async(async {
        supervisor
            .tick_candidate_generation_for_tests()
            .await
            .expect("candidate generation report")
    })
}

fn route_digest<'a>(payload: &'a serde_json::Value, route: &str, scope: &str) -> &'a str {
    payload["route_artifacts"]
        .as_array()
        .expect("route_artifacts should be present")
        .iter()
        .find(|artifact| artifact["key"]["route"] == route && artifact["key"]["scope"] == scope)
        .and_then(|artifact| artifact["semantic_digest"].as_str())
        .expect("route artifact digest should be present")
}

fn sample_rendered_live_targets() -> BTreeMap<String, NegRiskFamilyLiveTarget> {
    BTreeMap::from([(
        "family-a".to_owned(),
        NegRiskFamilyLiveTarget {
            family_id: "family-a".to_owned(),
            members: vec![NegRiskMemberLiveTarget {
                condition_id: "condition-1".to_owned(),
                token_id: "token-1".to_owned(),
                price: rust_decimal::Decimal::new(43, 2),
                quantity: rust_decimal::Decimal::new(5, 0),
            }],
        },
    )])
}
