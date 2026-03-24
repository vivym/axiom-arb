use state::{FullSetView, NegRiskView, PublishedSnapshot};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchSummary {
    pub coalesced_versions: Vec<u64>,
    pub last_stable_snapshot_id: Option<String>,
    pub last_stable_state_version: Option<u64>,
    pub fullset_last_ready_snapshot_id: Option<String>,
    pub fullset_last_ready_state_version: Option<u64>,
    pub negrisk_last_ready_snapshot_id: Option<String>,
    pub negrisk_last_ready_state_version: Option<u64>,
}

#[derive(Debug, Default)]
pub struct DispatchLoop {
    latest_ready_fullset: Option<PublishedSnapshot>,
    latest_ready_negrisk: Option<PublishedSnapshot>,
    dirty_versions: Vec<u64>,
}

impl DispatchLoop {
    pub fn record_dirty_version(&mut self, state_version: u64) {
        self.dirty_versions.push(state_version);
    }

    pub fn observe_snapshot(&mut self, snapshot: PublishedSnapshot) {
        self.record_dirty_version(snapshot.state_version);
        if snapshot.fullset_ready {
            self.latest_ready_fullset = Some(snapshot.clone());
        }
        if snapshot.negrisk_ready {
            self.latest_ready_negrisk = Some(snapshot);
        }
    }

    pub fn flush(&mut self) -> DispatchSummary {
        let coalesced_versions = std::mem::take(&mut self.dirty_versions)
            .into_iter()
            .max()
            .into_iter()
            .collect();
        let last_stable_snapshot = latest_stable_snapshot(
            self.latest_ready_fullset.as_ref(),
            self.latest_ready_negrisk.as_ref(),
        );

        DispatchSummary {
            coalesced_versions,
            last_stable_snapshot_id: last_stable_snapshot
                .map(|snapshot| snapshot.snapshot_id.clone()),
            last_stable_state_version: last_stable_snapshot.map(|snapshot| snapshot.state_version),
            fullset_last_ready_snapshot_id: self
                .latest_ready_fullset
                .as_ref()
                .map(|snapshot| snapshot.snapshot_id.clone()),
            fullset_last_ready_state_version: self
                .latest_ready_fullset
                .as_ref()
                .map(|snapshot| snapshot.state_version),
            negrisk_last_ready_snapshot_id: self
                .latest_ready_negrisk
                .as_ref()
                .map(|snapshot| snapshot.snapshot_id.clone()),
            negrisk_last_ready_state_version: self
                .latest_ready_negrisk
                .as_ref()
                .map(|snapshot| snapshot.state_version),
        }
    }

    pub fn push_test_snapshot(
        &mut self,
        state_version: u64,
        fullset_ready: bool,
        negrisk_ready: bool,
    ) {
        self.observe_snapshot(PublishedSnapshot {
            snapshot_id: format!("snapshot-{state_version}"),
            state_version,
            committed_journal_seq: state_version as i64,
            fullset_ready,
            negrisk_ready,
            fullset: fullset_ready.then(|| FullSetView {
                snapshot_id: format!("snapshot-{state_version}"),
                state_version,
                open_orders: Vec::new(),
            }),
            negrisk: negrisk_ready.then(|| NegRiskView {
                snapshot_id: format!("snapshot-{state_version}"),
                state_version,
                family_ids: Vec::new(),
            }),
        });
    }
}

fn latest_stable_snapshot<'a>(
    fullset: Option<&'a PublishedSnapshot>,
    negrisk: Option<&'a PublishedSnapshot>,
) -> Option<&'a PublishedSnapshot> {
    match (fullset, negrisk) {
        (Some(fullset), Some(negrisk)) => {
            if fullset.state_version >= negrisk.state_version {
                Some(fullset)
            } else {
                Some(negrisk)
            }
        }
        (Some(fullset), None) => Some(fullset),
        (None, Some(negrisk)) => Some(negrisk),
        (None, None) => None,
    }
}
