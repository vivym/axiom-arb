use state::PublishedSnapshot;

use crate::supervisor::NegRiskRolloutEvidence;

pub(crate) fn snapshot_id_for(state_version: u64) -> String {
    format!("snapshot-{state_version}")
}

pub(crate) fn rollout_evidence_from_snapshot(snapshot: &PublishedSnapshot) -> NegRiskRolloutEvidence {
    let Some(negrisk) = snapshot.negrisk.as_ref() else {
        return NegRiskRolloutEvidence {
            snapshot_id: snapshot.snapshot_id.clone(),
            ..NegRiskRolloutEvidence::default()
        };
    };

    let live_ready_family_count = negrisk
        .families
        .iter()
        .filter(|family| {
            family.shadow_parity_ready
                && family.recovery_ready
                && family.replay_drift_ready
                && family.fault_injection_ready
                && family.conversion_path_ready
                && family.halt_semantics_ready
        })
        .count();
    let parity_mismatch_count = negrisk
        .families
        .iter()
        .filter(|family| !family.shadow_parity_ready)
        .count() as u64;

    NegRiskRolloutEvidence {
        snapshot_id: snapshot.snapshot_id.clone(),
        live_ready_family_count,
        blocked_family_count: negrisk
            .families
            .len()
            .saturating_sub(live_ready_family_count),
        parity_mismatch_count,
    }
}
