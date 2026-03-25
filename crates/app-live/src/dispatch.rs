use std::collections::BTreeSet;

use state::{DirtyDomain, DirtySet, FullSetView, NegRiskView, PublishedSnapshot};

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
    dirty_records: Vec<DirtyRecord>,
}

impl DispatchLoop {
    pub fn record_apply(&mut self, state_version: u64, dirty_set: DirtySet) {
        self.dirty_records.push(DirtyRecord {
            state_version,
            domains: dirty_set.domains,
        });
    }

    pub fn observe_snapshot(&mut self, snapshot: PublishedSnapshot) {
        if snapshot.fullset_ready {
            self.latest_ready_fullset = Some(snapshot.clone());
            self.clear_dirty_domains(snapshot.state_version, &fullset_domains());
        }
        if snapshot.negrisk_ready {
            self.latest_ready_negrisk = Some(snapshot.clone());
            self.clear_dirty_domains(snapshot.state_version, &negrisk_domains());
        }
    }

    pub fn flush(&mut self) -> DispatchSummary {
        self.coalesce_dirty_records();
        let coalesced_versions = self
            .dirty_records
            .iter()
            .map(|record| record.state_version)
            .collect::<Vec<_>>();
        let last_stable_state_version = last_stable_state_version(
            self.latest_ready_fullset.as_ref(),
            self.latest_ready_negrisk.as_ref(),
        );
        let last_stable_snapshot_id = last_stable_state_version.and_then(|state_version| {
            self.latest_ready_fullset
                .as_ref()
                .filter(|snapshot| snapshot.state_version == state_version)
                .or_else(|| {
                    self.latest_ready_negrisk
                        .as_ref()
                        .filter(|snapshot| snapshot.state_version == state_version)
                })
                .map(|snapshot| snapshot.snapshot_id.clone())
        });

        DispatchSummary {
            coalesced_versions,
            last_stable_snapshot_id,
            last_stable_state_version,
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
        self.record_apply(state_version, DirtySet::new(all_domains()));
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
                families: Vec::new(),
            }),
        });
    }

    pub fn pending_backlog_count(&self) -> usize {
        self.coalesced_dirty_records().len()
    }

    fn clear_dirty_domains(
        &mut self,
        up_to_state_version: u64,
        cleared_domains: &BTreeSet<DirtyDomain>,
    ) {
        for record in &mut self.dirty_records {
            if record.state_version <= up_to_state_version {
                record
                    .domains
                    .retain(|domain| !cleared_domains.contains(domain));
            }
        }
        self.dirty_records
            .retain(|record| !record.domains.is_empty());
    }

    fn coalesce_dirty_records(&mut self) {
        self.dirty_records = self.coalesced_dirty_records();
    }

    fn coalesced_dirty_records(&self) -> Vec<DirtyRecord> {
        let latest_fullset_dirty = self
            .dirty_records
            .iter()
            .filter(|record| {
                record
                    .domains
                    .iter()
                    .any(|domain| is_fullset_domain(*domain))
            })
            .map(|record| record.state_version)
            .max();
        let latest_negrisk_dirty = self
            .dirty_records
            .iter()
            .filter(|record| record.domains.contains(&DirtyDomain::NegRiskFamilies))
            .map(|record| record.state_version)
            .max();

        let mut coalesced = std::collections::BTreeMap::<u64, BTreeSet<DirtyDomain>>::new();
        if let Some(state_version) = latest_fullset_dirty {
            coalesced
                .entry(state_version)
                .or_default()
                .extend(fullset_domains());
        }
        if let Some(state_version) = latest_negrisk_dirty {
            coalesced
                .entry(state_version)
                .or_default()
                .insert(DirtyDomain::NegRiskFamilies);
        }

        coalesced
            .into_iter()
            .map(|(state_version, domains)| DirtyRecord {
                state_version,
                domains,
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirtyRecord {
    state_version: u64,
    domains: BTreeSet<DirtyDomain>,
}

fn fullset_domains() -> BTreeSet<DirtyDomain> {
    BTreeSet::from([
        DirtyDomain::Runtime,
        DirtyDomain::Orders,
        DirtyDomain::Inventory,
        DirtyDomain::Approvals,
        DirtyDomain::Resolution,
        DirtyDomain::Relayer,
    ])
}

fn negrisk_domains() -> BTreeSet<DirtyDomain> {
    BTreeSet::from([DirtyDomain::NegRiskFamilies])
}

fn all_domains() -> BTreeSet<DirtyDomain> {
    fullset_domains()
        .into_iter()
        .chain(negrisk_domains())
        .collect()
}

fn last_stable_state_version(
    fullset: Option<&PublishedSnapshot>,
    negrisk: Option<&PublishedSnapshot>,
) -> Option<u64> {
    match (fullset, negrisk) {
        (Some(fullset), Some(negrisk)) => Some(fullset.state_version.min(negrisk.state_version)),
        _ => None,
    }
}

fn is_fullset_domain(domain: DirtyDomain) -> bool {
    fullset_domains().contains(&domain)
}
