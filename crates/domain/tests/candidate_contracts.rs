use chrono::{TimeZone, Utc};
use domain::{
    AdoptableTargetRevision, CandidatePolicyAnchor, CandidateTarget, CandidateTargetSet,
    CandidateValidationResult, DiscoverySourceAnchor, EventFamilyId, ExternalFactEvent,
    ExternalFactPayloadData, FamilyDiscoveryRecord,
};

#[test]
fn candidate_target_set_keeps_stable_snapshot_source_and_policy_anchors() {
    let discovery_record = FamilyDiscoveryRecord::new(
        EventFamilyId::from("family-a"),
        DiscoverySourceAnchor::new("discovery_ws", "session-9", "evt-44", "v2-discovery"),
        Utc.with_ymd_and_hms(2026, 3, 27, 8, 30, 0).unwrap(),
    )
    .with_backfill_cursor("cursor-1");

    let target_set = CandidateTargetSet::new(
        "target-set-7",
        "snapshot-17",
        discovery_record.clone(),
        CandidatePolicyAnchor::new("candidate-generation", "policy-v3"),
        vec![
            CandidateTarget::new(
                "target-family-a",
                EventFamilyId::from("family-a"),
                CandidateValidationResult::Adoptable,
            ),
            CandidateTarget::new(
                "target-family-b",
                EventFamilyId::from("family-b"),
                CandidateValidationResult::Rejected {
                    reason: "policy scope excluded".to_owned(),
                },
            ),
        ],
    )
    .with_adoptable_revision(AdoptableTargetRevision::new(
        "revision-3",
        "snapshot-17",
        "policy-v3",
    ));

    assert_eq!(target_set.target_set_id, "target-set-7");
    assert_eq!(target_set.source_snapshot_id, "snapshot-17");
    assert_eq!(target_set.discovery_record.family_id.as_str(), "family-a");
    assert_eq!(
        target_set.discovery_record.source.source_kind,
        "discovery_ws"
    );
    assert_eq!(target_set.discovery_record.source.source_event_id, "evt-44");
    assert_eq!(target_set.policy.policy_name, "candidate-generation");
    assert_eq!(target_set.policy.policy_version, "policy-v3");
    assert_eq!(
        target_set
            .adoptable_revision
            .as_ref()
            .map(|revision| revision.source_snapshot_id.as_str()),
        Some("snapshot-17")
    );
    assert_eq!(
        target_set
            .adoptable_revision
            .as_ref()
            .map(|revision| revision.policy_version.as_str()),
        Some("policy-v3")
    );
    assert_eq!(target_set.targets.len(), 2);
}

#[test]
fn external_fact_event_supports_family_discovery_and_backfill_payloads() {
    let discovered_at = Utc.with_ymd_and_hms(2026, 3, 27, 9, 0, 0).unwrap();
    let discovery = ExternalFactEvent::family_discovery_observed(
        "session-discovery",
        "evt-1",
        "family-a",
        discovered_at,
    );

    assert_eq!(discovery.source_kind, "family_discovery");
    assert_eq!(discovery.payload.kind(), "family_discovery_observed");

    match discovery.payload.as_ref() {
        Some(ExternalFactPayloadData::FamilyDiscoveryObserved(payload)) => {
            assert_eq!(payload.family_id, "family-a");
        }
        other => panic!("unexpected discovery payload: {other:?}"),
    }

    let backfill = ExternalFactEvent::family_backfill_observed(
        "session-discovery",
        "evt-2",
        "family-a",
        "cursor-2",
        true,
        discovered_at,
    );

    assert_eq!(backfill.source_kind, "family_backfill");
    assert_eq!(backfill.payload.kind(), "family_backfill_observed");

    match backfill.payload.as_ref() {
        Some(ExternalFactPayloadData::FamilyBackfillObserved(payload)) => {
            assert_eq!(payload.family_id, "family-a");
            assert_eq!(payload.cursor, "cursor-2");
            assert!(payload.complete);
        }
        other => panic!("unexpected backfill payload: {other:?}"),
    }
}
