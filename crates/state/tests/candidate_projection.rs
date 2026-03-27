use chrono::{TimeZone, Utc};
use domain::{EventFamilyId, ExternalFactEvent};
use state::{
    ApplyResult, CandidateProjectionReadiness, CandidatePublication, DirtyDomain,
    ProjectionReadiness, PublishedSnapshot, StateApplier, StateStore,
};

#[test]
fn family_discovery_and_backfill_facts_update_authoritative_discovery_state() {
    let mut store = StateStore::new();
    apply_anchor_event(&mut store, 17);

    let discovery = StateApplier::new(&mut store)
        .apply(
            18,
            ExternalFactEvent::family_discovery_observed(
                "session-discovery",
                "evt-1",
                "family-a",
                Utc.with_ymd_and_hms(2026, 3, 27, 9, 0, 0).unwrap(),
            ),
        )
        .unwrap();

    let backfill = StateApplier::new(&mut store)
        .apply(
            19,
            ExternalFactEvent::family_backfill_observed(
                "session-discovery",
                "evt-2",
                "family-a",
                "cursor-2",
                true,
                Utc.with_ymd_and_hms(2026, 3, 27, 9, 5, 0).unwrap(),
            ),
        )
        .unwrap();

    assert!(matches!(
        discovery,
        ApplyResult::Applied {
            journal_seq: 18,
            state_version: 2,
            ref dirty_set,
        } if dirty_set.domains.contains(&DirtyDomain::Candidates)
    ));
    assert!(matches!(
        backfill,
        ApplyResult::Applied {
            journal_seq: 19,
            state_version: 3,
            ref dirty_set,
        } if dirty_set.domains.contains(&DirtyDomain::Candidates)
    ));

    let discoveries = store.family_discovery_records();
    assert_eq!(discoveries.len(), 1);
    assert_eq!(discoveries[0].family_id, EventFamilyId::from("family-a"));
    assert_eq!(discoveries[0].source.source_kind, "family_discovery");
    assert_eq!(discoveries[0].source.source_event_id, "evt-1");
    assert_eq!(discoveries[0].backfill_cursor.as_deref(), Some("cursor-2"));
    assert_eq!(
        discoveries[0].backfill_completed_at,
        Some(Utc.with_ymd_and_hms(2026, 3, 27, 9, 5, 0).unwrap())
    );
}

#[test]
fn candidate_projection_failure_does_not_block_fullset_negrisk_publication() {
    let mut store = StateStore::new();
    apply_anchor_event(&mut store, 17);
    StateApplier::new(&mut store)
        .apply(
            18,
            ExternalFactEvent::family_discovery_observed(
                "session-discovery",
                "evt-1",
                "family-a",
                Utc.with_ymd_and_hms(2026, 3, 27, 9, 0, 0).unwrap(),
            ),
        )
        .unwrap();

    let published_snapshot = PublishedSnapshot::from_store(
        &store,
        ProjectionReadiness::ready_fullset_pending_negrisk("snapshot-17"),
    );
    let candidate_publication = CandidatePublication::from_store(
        &store,
        CandidateProjectionReadiness::failed("candidate-pub-17", "candidate projection failed"),
    );

    assert_eq!(published_snapshot.snapshot_id, "snapshot-17");
    assert!(published_snapshot.fullset_ready);
    assert!(!published_snapshot.negrisk_ready);
    assert!(published_snapshot.fullset.is_some());

    assert_eq!(candidate_publication.publication_id, "candidate-pub-17");
    assert!(!candidate_publication.ready);
    assert!(candidate_publication.view.is_none());
    assert_eq!(
        candidate_publication.failure_reason.as_deref(),
        Some("candidate projection failed")
    );
    assert_eq!(store.family_discovery_records().len(), 1);
}

#[test]
fn backfill_without_prior_discovery_does_not_create_discovered_family_and_ready_publication_stays_empty(
) {
    let mut store = StateStore::new();
    apply_anchor_event(&mut store, 17);

    StateApplier::new(&mut store)
        .apply(
            18,
            ExternalFactEvent::family_backfill_observed(
                "session-discovery",
                "evt-2",
                "family-a",
                "cursor-2",
                false,
                Utc.with_ymd_and_hms(2026, 3, 27, 9, 5, 0).unwrap(),
            ),
        )
        .unwrap();

    assert!(store.family_discovery_records().is_empty());

    let publication = CandidatePublication::from_store(
        &store,
        CandidateProjectionReadiness::ready("candidate-pub-18"),
    );

    assert!(publication.ready);
    assert_eq!(publication.failure_reason, None);
    assert_eq!(publication.lag_reason, None);
    assert_eq!(
        publication
            .view
            .as_ref()
            .map(|view| view.discovery_records.len()),
        Some(0)
    );
}

#[test]
fn discovery_after_backfill_preserves_backfill_metadata() {
    let mut store = StateStore::new();
    apply_anchor_event(&mut store, 17);

    StateApplier::new(&mut store)
        .apply(
            18,
            ExternalFactEvent::family_backfill_observed(
                "session-discovery",
                "evt-2",
                "family-a",
                "cursor-2",
                true,
                Utc.with_ymd_and_hms(2026, 3, 27, 9, 5, 0).unwrap(),
            ),
        )
        .unwrap();

    StateApplier::new(&mut store)
        .apply(
            19,
            ExternalFactEvent::family_discovery_observed(
                "session-discovery",
                "evt-3",
                "family-a",
                Utc.with_ymd_and_hms(2026, 3, 27, 9, 6, 0).unwrap(),
            ),
        )
        .unwrap();

    let discoveries = store.family_discovery_records();
    assert_eq!(discoveries.len(), 1);
    assert_eq!(discoveries[0].source.source_kind, "family_discovery");
    assert_eq!(discoveries[0].source.source_event_id, "evt-3");
    assert_eq!(
        discoveries[0].discovered_at,
        Utc.with_ymd_and_hms(2026, 3, 27, 9, 6, 0).unwrap()
    );
    assert_eq!(discoveries[0].backfill_cursor.as_deref(), Some("cursor-2"));
    assert_eq!(
        discoveries[0].backfill_completed_at,
        Some(Utc.with_ymd_and_hms(2026, 3, 27, 9, 5, 0).unwrap())
    );
}

#[test]
fn backfill_completion_does_not_regress_from_complete_to_incomplete() {
    let mut store = StateStore::new();
    apply_anchor_event(&mut store, 17);

    StateApplier::new(&mut store)
        .apply(
            18,
            ExternalFactEvent::family_backfill_observed(
                "session-discovery",
                "evt-2",
                "family-a",
                "cursor-2",
                true,
                Utc.with_ymd_and_hms(2026, 3, 27, 9, 5, 0).unwrap(),
            ),
        )
        .unwrap();
    StateApplier::new(&mut store)
        .apply(
            19,
            ExternalFactEvent::family_backfill_observed(
                "session-discovery",
                "evt-3",
                "family-a",
                "cursor-3",
                false,
                Utc.with_ymd_and_hms(2026, 3, 27, 9, 6, 0).unwrap(),
            ),
        )
        .unwrap();
    StateApplier::new(&mut store)
        .apply(
            20,
            ExternalFactEvent::family_discovery_observed(
                "session-discovery",
                "evt-4",
                "family-a",
                Utc.with_ymd_and_hms(2026, 3, 27, 9, 7, 0).unwrap(),
            ),
        )
        .unwrap();
    StateApplier::new(&mut store)
        .apply(
            21,
            ExternalFactEvent::family_backfill_observed(
                "session-discovery",
                "evt-5",
                "family-a",
                "cursor-4",
                false,
                Utc.with_ymd_and_hms(2026, 3, 27, 9, 8, 0).unwrap(),
            ),
        )
        .unwrap();

    let discoveries = store.family_discovery_records();
    assert_eq!(discoveries.len(), 1);
    assert_eq!(discoveries[0].backfill_cursor.as_deref(), Some("cursor-4"));
    assert_eq!(
        discoveries[0].backfill_completed_at,
        Some(Utc.with_ymd_and_hms(2026, 3, 27, 9, 5, 0).unwrap())
    );
}

#[test]
fn backfill_completion_keeps_newest_terminal_timestamp() {
    let mut store = StateStore::new();
    apply_anchor_event(&mut store, 17);

    StateApplier::new(&mut store)
        .apply(
            18,
            ExternalFactEvent::family_backfill_observed(
                "session-discovery",
                "evt-2",
                "family-a",
                "cursor-2",
                true,
                Utc.with_ymd_and_hms(2026, 3, 27, 9, 6, 0).unwrap(),
            ),
        )
        .unwrap();
    StateApplier::new(&mut store)
        .apply(
            19,
            ExternalFactEvent::family_backfill_observed(
                "session-discovery",
                "evt-3",
                "family-a",
                "cursor-3",
                true,
                Utc.with_ymd_and_hms(2026, 3, 27, 9, 5, 0).unwrap(),
            ),
        )
        .unwrap();
    StateApplier::new(&mut store)
        .apply(
            20,
            ExternalFactEvent::family_discovery_observed(
                "session-discovery",
                "evt-4",
                "family-a",
                Utc.with_ymd_and_hms(2026, 3, 27, 9, 7, 0).unwrap(),
            ),
        )
        .unwrap();
    StateApplier::new(&mut store)
        .apply(
            21,
            ExternalFactEvent::family_backfill_observed(
                "session-discovery",
                "evt-5",
                "family-a",
                "cursor-4",
                true,
                Utc.with_ymd_and_hms(2026, 3, 27, 9, 4, 0).unwrap(),
            ),
        )
        .unwrap();

    let discoveries = store.family_discovery_records();
    assert_eq!(discoveries.len(), 1);
    assert_eq!(discoveries[0].backfill_cursor.as_deref(), Some("cursor-4"));
    assert_eq!(
        discoveries[0].backfill_completed_at,
        Some(Utc.with_ymd_and_hms(2026, 3, 27, 9, 6, 0).unwrap())
    );
}

fn apply_anchor_event(store: &mut StateStore, journal_seq: i64) {
    StateApplier::new(store)
        .apply(
            journal_seq,
            ExternalFactEvent::new(
                "market_ws",
                "session-1",
                format!("evt-{journal_seq}"),
                "v1",
                Utc.with_ymd_and_hms(2026, 3, 24, 10, 0, 0).unwrap(),
            ),
        )
        .unwrap();
}
