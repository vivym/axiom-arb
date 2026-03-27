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
        let adoptable_revision = candidate_set
            .adoptable_revision
            .as_ref()
            .map(|revision| revision.revision_id.clone())
            .unwrap_or_else(|| format!("adoptable-{candidate_revision}"));
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
                    "policy": {
                        "name": candidate_set.policy.policy_name.clone(),
                        "version": candidate_set.policy.policy_version.clone(),
                    },
                    "target_count": candidate_set.targets.len(),
                    "advisory_pricing": advisory,
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
    ) -> Result<(CandidateTargetSet, String), String> {
        let Some(view) = publication.view.as_ref() else {
            return Err("candidate publication is not ready".to_owned());
        };

        let Some(discovery_record) = view.discovery_records.first().cloned() else {
            return Err("candidate publication has no discovery records".to_owned());
        };

        let disposition = candidate_disposition(&discovery_record, restriction);
        let validation = match disposition {
            "adoptable" => CandidateValidationResult::Adoptable,
            "deferred" => CandidateValidationResult::Deferred {
                reason: deferred_reason(&discovery_record, restriction),
            },
            "excluded" => CandidateValidationResult::Rejected {
                reason: "candidate excluded by conservative discovery policy".to_owned(),
            },
            other => return Err(format!("unsupported candidate disposition {other}")),
        };

        let target_id = format!("candidate-target-{}", discovery_record.family_id.as_str());
        let mut candidate_set = CandidateTargetSet::new(
            publication.publication_id.clone(),
            publication.publication_id.clone(),
            discovery_record.clone(),
            CandidatePolicyAnchor::new("candidate-generation", "policy-v1"),
            vec![CandidateTarget::new(
                target_id,
                EventFamilyId::from(discovery_record.family_id.as_str()),
                validation,
            )],
        );

        if disposition == "adoptable" {
            candidate_set = candidate_set.with_adoptable_revision(AdoptableTargetRevision::new(
                format!("adoptable-{}", publication.publication_id),
                publication.publication_id.clone(),
                "policy-v1",
            ));
        }

        Ok((candidate_set, disposition.to_owned()))
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
                live_dispatch_woken: false,
                disposition: "ignored".to_owned(),
            });
        }

        let _ = self.pricing;
        let (candidate_set, disposition) = self.validation.candidate_set_from_publication(
            &notice.publication,
            &notice.restriction,
            notice.operator_target_revision.as_deref(),
        )?;

        if disposition != "adoptable" || notice.operator_target_revision.is_none() {
            return Ok(DiscoveryReport {
                candidate_revision: Some(candidate_set.target_set_id),
                adoptable_revision: None,
                operator_target_revision: None,
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
            live_dispatch_woken: false,
            disposition,
        })
    }
}

fn candidate_disposition(
    discovery_record: &FamilyDiscoveryRecord,
    restriction: &CandidateRestrictionTruth,
) -> &'static str {
    if matches!(restriction, CandidateRestrictionTruth::Restricted { .. })
        || discovery_record.backfill_completed_at.is_none()
    {
        "deferred"
    } else if discovery_record.family_id.as_str().trim().is_empty() {
        "excluded"
    } else {
        "adoptable"
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
