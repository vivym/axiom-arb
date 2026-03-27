use domain::{
    AdoptableTargetRevision, CandidatePolicyAnchor, CandidateTarget, CandidateTargetSet,
    CandidateValidationResult, EventFamilyId, FamilyDiscoveryRecord,
};
use persistence::models::{AdoptableTargetRevisionRow, CandidateTargetSetRow};
use serde_json::json;
use state::{CandidatePublication, DirtyDomain};

use crate::queues::{CandidateNoticeQueue, CandidateRestrictionTruth};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryReport {
    pub candidate_revision: Option<String>,
    pub adoptable_revision: Option<String>,
    pub operator_target_revision: Option<String>,
    pub target_count: usize,
    pub adoptable_target_count: usize,
    pub deferred_target_count: usize,
    pub excluded_target_count: usize,
    pub live_dispatch_woken: bool,
    pub disposition: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CandidateArtifactRender {
    pub candidate: CandidateTargetSetRow,
    pub adoptable: AdoptableTargetRevisionRow,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CandidateValidationEngine;

#[derive(Debug, Clone, Copy, Default)]
pub struct CandidatePricingEngine;

#[derive(Debug, Clone, Copy, Default)]
pub struct CandidateBridge;

impl CandidateBridge {
    pub fn for_tests() -> Self {
        Self
    }

    pub fn render(
        &self,
        candidate_set: &CandidateTargetSet,
        operator_target_revision: Option<&str>,
    ) -> Result<CandidateArtifactRender, String> {
        let Some(operator_target_revision) = operator_target_revision.map(str::to_owned) else {
            return Err(
                "candidate bridge requires explicit rendered operator target revision".to_owned(),
            );
        };
        let candidate_revision = candidate_set.target_set_id.clone();
        let Some(adoptable_revision) = candidate_set
            .adoptable_revision
            .as_ref()
            .map(|revision| revision.revision_id.clone())
        else {
            return Err("candidate bridge requires an adoptable revision".to_owned());
        };
        let advisory = CandidatePricingEngine::default().advisory_terms(candidate_set);

        Ok(CandidateArtifactRender {
            candidate: CandidateTargetSetRow {
                candidate_revision: candidate_revision.clone(),
                snapshot_id: candidate_set.source_snapshot_id.clone(),
                source_revision: candidate_set
                    .discovery_record
                    .source
                    .source_event_id
                    .clone(),
                payload: json!({
                    "candidate_revision": candidate_revision,
                    "snapshot_id": candidate_set.source_snapshot_id.clone(),
                    "source_revision": candidate_set.discovery_record.source.source_event_id.clone(),
                    "source_anchor": {
                        "source_kind": candidate_set.discovery_record.source.source_kind.clone(),
                        "source_session_id": candidate_set.discovery_record.source.source_session_id.clone(),
                        "source_event_id": candidate_set.discovery_record.source.source_event_id.clone(),
                        "normalizer_version": candidate_set.discovery_record.source.normalizer_version.clone(),
                    },
                    "policy_name": candidate_set.policy.policy_name.clone(),
                    "candidate_policy_version": candidate_set.policy.policy_version.clone(),
                    "bridge_policy_version": "bridge-policy-v1",
                    "target_count": candidate_set.targets.len(),
                    "advisory_pricing": advisory,
                    "warnings": [],
                    "execution_requests": [],
                }),
            },
            adoptable: AdoptableTargetRevisionRow {
                adoptable_revision: adoptable_revision.clone(),
                candidate_revision: candidate_revision.clone(),
                rendered_operator_target_revision: operator_target_revision.clone(),
                payload: json!({
                    "adoptable_revision": adoptable_revision,
                    "candidate_revision": candidate_revision,
                    "rendered_operator_target_revision": operator_target_revision,
                    "snapshot_id": candidate_set.source_snapshot_id.clone(),
                    "source_anchor": {
                        "source_kind": candidate_set.discovery_record.source.source_kind.clone(),
                        "source_session_id": candidate_set.discovery_record.source.source_session_id.clone(),
                        "source_event_id": candidate_set.discovery_record.source.source_event_id.clone(),
                        "normalizer_version": candidate_set.discovery_record.source.normalizer_version.clone(),
                    },
                    "bridge_policy_version": "bridge-policy-v1",
                    "candidate_policy_version": candidate_set.policy.policy_version.clone(),
                    "compatibility": {
                        "operator_target_revision_supplied": true,
                        "advisory_only": true,
                    },
                    "warnings": [],
                    "execution_requests": [],
                }),
            },
        })
    }
}

impl CandidatePricingEngine {
    fn advisory_terms(&self, candidate_set: &CandidateTargetSet) -> serde_json::Value {
        json!({
            "price_band_bps": 25,
            "size_cap_contracts": 1,
            "mode": "advisory_only",
            "candidate_count": candidate_set.targets.len(),
        })
    }
}

impl CandidateValidationEngine {
    fn candidate_set_from_publication(
        &self,
        publication: &CandidatePublication,
        restriction: &CandidateRestrictionTruth,
        _operator_target_revision: Option<&str>,
    ) -> Result<(CandidateTargetSet, String, ValidationSummary), String> {
        let Some(view) = publication.view.as_ref() else {
            return Err("candidate publication is not ready".to_owned());
        };

        let Some(discovery_record) = view.discovery_records.first().cloned() else {
            return Err("candidate publication has no discovery records".to_owned());
        };

        let targets: Vec<_> = view
            .discovery_records
            .iter()
            .map(|record| {
                CandidateTarget::new(
                    format!("candidate-target-{}", record.family_id.as_str()),
                    EventFamilyId::from(record.family_id.as_str()),
                    validation_for_record(record, restriction),
                )
            })
            .collect();
        let summary = ValidationSummary::from_targets(&targets);
        let disposition = summary.aggregate_disposition().to_owned();

        let mut candidate_set = CandidateTargetSet::new(
            publication.publication_id.clone(),
            publication.publication_id.clone(),
            discovery_record.clone(),
            CandidatePolicyAnchor::new("candidate-generation", "policy-v1"),
            targets,
        );

        if disposition == "adoptable" {
            candidate_set = candidate_set.with_adoptable_revision(AdoptableTargetRevision::new(
                format!("adoptable-{}", publication.publication_id),
                publication.publication_id.clone(),
                "policy-v1",
            ));
        }

        Ok((candidate_set, disposition, summary))
    }
}

#[derive(Debug, Default)]
pub struct DiscoverySupervisor {
    notices: CandidateNoticeQueue,
    validation: CandidateValidationEngine,
    pricing: CandidatePricingEngine,
    bridge: CandidateBridge,
}

impl DiscoverySupervisor {
    pub fn for_tests(notices: CandidateNoticeQueue) -> Self {
        Self {
            notices,
            validation: CandidateValidationEngine,
            pricing: CandidatePricingEngine,
            bridge: CandidateBridge,
        }
    }

    pub async fn tick_candidate_generation_for_tests(&mut self) -> Result<DiscoveryReport, String> {
        let notice = self
            .notices
            .pop_front()
            .ok_or_else(|| "candidate notice queue is empty".to_owned())?;

        if !notice.dirty_domains.contains(&DirtyDomain::Candidates) {
            return Ok(DiscoveryReport {
                candidate_revision: None,
                adoptable_revision: None,
                operator_target_revision: notice.operator_target_revision,
                target_count: 0,
                adoptable_target_count: 0,
                deferred_target_count: 0,
                excluded_target_count: 0,
                live_dispatch_woken: false,
                disposition: "ignored".to_owned(),
            });
        }

        let _ = self.pricing;
        let (candidate_set, disposition, summary) =
            self.validation.candidate_set_from_publication(
                &notice.publication,
                &notice.restriction,
                notice.operator_target_revision.as_deref(),
            )?;

        if disposition != "adoptable" || notice.operator_target_revision.is_none() {
            return Ok(DiscoveryReport {
                candidate_revision: Some(candidate_set.target_set_id),
                adoptable_revision: None,
                operator_target_revision: None,
                target_count: candidate_set.targets.len(),
                adoptable_target_count: summary.adoptable_count,
                deferred_target_count: summary.deferred_count,
                excluded_target_count: summary.excluded_count,
                live_dispatch_woken: false,
                disposition,
            });
        }

        let rendered = self
            .bridge
            .render(&candidate_set, notice.operator_target_revision.as_deref())?;

        Ok(DiscoveryReport {
            candidate_revision: Some(rendered.candidate.candidate_revision),
            adoptable_revision: Some(rendered.adoptable.adoptable_revision),
            operator_target_revision: Some(rendered.adoptable.rendered_operator_target_revision),
            target_count: candidate_set.targets.len(),
            adoptable_target_count: summary.adoptable_count,
            deferred_target_count: summary.deferred_count,
            excluded_target_count: summary.excluded_count,
            live_dispatch_woken: false,
            disposition,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ValidationSummary {
    adoptable_count: usize,
    deferred_count: usize,
    excluded_count: usize,
}

impl ValidationSummary {
    fn from_targets(targets: &[CandidateTarget]) -> Self {
        let mut summary = Self {
            adoptable_count: 0,
            deferred_count: 0,
            excluded_count: 0,
        };

        for target in targets {
            match target.validation {
                CandidateValidationResult::Adoptable => summary.adoptable_count += 1,
                CandidateValidationResult::Deferred { .. } => summary.deferred_count += 1,
                CandidateValidationResult::Rejected { .. } => summary.excluded_count += 1,
            }
        }

        summary
    }

    fn aggregate_disposition(&self) -> &'static str {
        if self.adoptable_count > 0 && self.deferred_count == 0 && self.excluded_count == 0 {
            "adoptable"
        } else if self.excluded_count > 0 {
            "excluded"
        } else {
            "deferred"
        }
    }
}

fn validation_for_record(
    discovery_record: &FamilyDiscoveryRecord,
    restriction: &CandidateRestrictionTruth,
) -> CandidateValidationResult {
    if matches!(restriction, CandidateRestrictionTruth::Restricted { .. })
        || discovery_record.backfill_completed_at.is_none()
    {
        CandidateValidationResult::Deferred {
            reason: deferred_reason(discovery_record, restriction),
        }
    } else if discovery_record.family_id.as_str().trim().is_empty() {
        CandidateValidationResult::Rejected {
            reason: "candidate excluded by conservative discovery policy".to_owned(),
        }
    } else {
        CandidateValidationResult::Adoptable
    }
}

fn deferred_reason(
    discovery_record: &FamilyDiscoveryRecord,
    restriction: &CandidateRestrictionTruth,
) -> String {
    if let Some(reason) = restriction.restriction_reason() {
        reason.to_owned()
    } else if discovery_record.backfill_completed_at.is_none() {
        "candidate generation deferred until discovery backfill completes".to_owned()
    } else {
        "candidate generation deferred by conservative discovery policy".to_owned()
    }
}
