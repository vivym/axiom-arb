use state::{FullSetView, PublishedSnapshot};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchSummary {
    pub coalesced_versions: Vec<u64>,
    pub dispatched_snapshot_id: Option<String>,
    pub last_dispatched_state_version: Option<u64>,
}

#[derive(Debug, Default)]
pub struct DispatchLoop {
    latest_ready_snapshot: Option<PublishedSnapshot>,
    dirty_versions: Vec<u64>,
}

impl DispatchLoop {
    pub fn record_dirty_version(&mut self, state_version: u64) {
        self.dirty_versions.push(state_version);
    }

    pub fn observe_snapshot(&mut self, snapshot: PublishedSnapshot) {
        self.record_dirty_version(snapshot.state_version);
        if snapshot.fullset_ready || snapshot.negrisk_ready {
            self.latest_ready_snapshot = Some(snapshot);
        }
    }

    pub fn flush(&mut self) -> DispatchSummary {
        let coalesced_versions = std::mem::take(&mut self.dirty_versions);
        let dispatched_snapshot_id = self
            .latest_ready_snapshot
            .as_ref()
            .map(|snapshot| snapshot.snapshot_id.clone());
        let last_dispatched_state_version = self
            .latest_ready_snapshot
            .as_ref()
            .map(|snapshot| snapshot.state_version);

        DispatchSummary {
            coalesced_versions,
            dispatched_snapshot_id,
            last_dispatched_state_version,
        }
    }

    pub fn push_test_snapshot(&mut self, state_version: u64) {
        self.observe_snapshot(PublishedSnapshot {
            snapshot_id: format!("snapshot-{state_version}"),
            state_version,
            committed_journal_seq: state_version as i64,
            fullset_ready: true,
            negrisk_ready: false,
            fullset: Some(FullSetView {
                snapshot_id: format!("snapshot-{state_version}"),
                state_version,
                open_orders: Vec::new(),
            }),
            negrisk: None,
        });
    }
}
