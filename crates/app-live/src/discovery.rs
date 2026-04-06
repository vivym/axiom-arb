use domain::{
    canonical_strategy_artifact_semantic_digest, AdoptableTargetRevision, CandidatePolicyAnchor,
    CandidateTarget, CandidateTargetSet, CandidateValidationResult, EventFamilyId,
    FamilyDiscoveryRecord, StrategyArtifactSemanticDigestInput, StrategyKey,
};
use persistence::{
    connect_pool_from_env,
    models::{
        AdoptableStrategyRevisionRow, StrategyAdoptionProvenanceRow, StrategyCandidateSetRow,
    },
    StrategyAdoptionRepo, StrategyControlArtifactRepo,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use state::{CandidatePublication, DirtyDomain};

use crate::config::NegRiskFamilyLiveTarget;
use crate::queues::{
    default_full_set_basis_digest, CandidateNotice, CandidateNoticeQueue, CandidateRestrictionTruth,
};

const BUNDLE_POLICY_VERSION: &str = "strategy-bundle-v1";
const FULL_SET_ROUTE_POLICY_VERSION: &str = "full-set-route-policy-v1";
const NEG_RISK_ROUTE_POLICY_VERSION: &str = "neg-risk-route-policy-v1";

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
    pub candidate: StrategyCandidateSetRow,
    pub adoptable: AdoptableStrategyRevisionRow,
}

#[derive(Debug, Clone, PartialEq)]
struct DiscoveryOutcome {
    report: DiscoveryReport,
    candidate: Option<StrategyCandidateSetRow>,
    adoptable: Option<AdoptableStrategyRevisionRow>,
    provenance: Option<StrategyAdoptionProvenanceRow>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CandidateValidationEngine;

#[derive(Debug, Clone, Copy, Default)]
pub struct CandidatePricingEngine;

#[derive(Debug, Clone, Copy, Default)]
pub struct CandidateBridge;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RouteArtifact {
    key: StrategyKey,
    route_policy_version: String,
    semantic_digest: String,
    content: serde_json::Value,
}

impl CandidateBridge {
    pub fn for_tests() -> Self {
        Self
    }

    pub fn render(
        &self,
        candidate_set: &CandidateTargetSet,
        _operator_target_revision: Option<&str>,
        rendered_live_targets: &std::collections::BTreeMap<String, NegRiskFamilyLiveTarget>,
    ) -> Result<CandidateArtifactRender, String> {
        self.render_with_full_set_basis(
            candidate_set,
            _operator_target_revision,
            rendered_live_targets,
            &default_full_set_basis_digest(),
        )
    }

    pub fn render_with_full_set_basis(
        &self,
        candidate_set: &CandidateTargetSet,
        _operator_target_revision: Option<&str>,
        rendered_live_targets: &std::collections::BTreeMap<String, NegRiskFamilyLiveTarget>,
        full_set_basis_digest: &str,
    ) -> Result<CandidateArtifactRender, String> {
        let rendered_operator_strategy_revision =
            operator_strategy_revision(candidate_set, rendered_live_targets, full_set_basis_digest);
        let candidate = self.render_candidate_with_full_set_basis(
            candidate_set,
            rendered_live_targets,
            full_set_basis_digest,
        );
        let route_artifacts =
            self.route_artifacts(candidate_set, rendered_live_targets, full_set_basis_digest);
        let Some(adoptable_strategy_revision) = candidate_set
            .adoptable_revision
            .as_ref()
            .map(|_| adoptable_strategy_revision(&route_artifacts))
        else {
            return Err("candidate bridge requires an adoptable revision".to_owned());
        };

        Ok(CandidateArtifactRender {
            candidate: candidate.clone(),
            adoptable: AdoptableStrategyRevisionRow {
                adoptable_strategy_revision: adoptable_strategy_revision.clone(),
                strategy_candidate_revision: candidate.strategy_candidate_revision.clone(),
                rendered_operator_strategy_revision: rendered_operator_strategy_revision.clone(),
                payload: json!({
                    "adoptable_strategy_revision": adoptable_strategy_revision,
                    "strategy_candidate_revision": candidate.strategy_candidate_revision.clone(),
                    "rendered_operator_strategy_revision": rendered_operator_strategy_revision,
                    "bundle_policy_version": BUNDLE_POLICY_VERSION,
                    "route_artifact_count": route_artifacts.len(),
                    "route_artifacts": route_artifacts
                        .iter()
                        .map(serialized_route_artifact)
                        .collect::<Vec<_>>(),
                    "rendered_live_targets": rendered_live_targets,
                    "warnings": [],
                    "execution_requests": [],
                }),
            },
        })
    }

    fn render_candidate_with_full_set_basis(
        &self,
        candidate_set: &CandidateTargetSet,
        rendered_live_targets: &std::collections::BTreeMap<String, NegRiskFamilyLiveTarget>,
        full_set_basis_digest: &str,
    ) -> StrategyCandidateSetRow {
        let advisory = CandidatePricingEngine.advisory_terms(candidate_set);
        let route_artifacts =
            self.route_artifacts(candidate_set, rendered_live_targets, full_set_basis_digest);
        let strategy_candidate_revision = strategy_candidate_revision(&route_artifacts);

        StrategyCandidateSetRow {
            strategy_candidate_revision: strategy_candidate_revision.clone(),
            snapshot_id: strategy_candidate_revision.clone(),
            source_revision: strategy_candidate_revision.clone(),
            payload: json!({
                "strategy_candidate_revision": strategy_candidate_revision.clone(),
                "snapshot_id": strategy_candidate_revision.clone(),
                "source_revision": strategy_candidate_revision.clone(),
                "bundle_policy_version": BUNDLE_POLICY_VERSION,
                "policy_name": candidate_set.policy.policy_name.clone(),
                "candidate_policy_version": candidate_set.policy.policy_version.clone(),
                "route_artifact_count": route_artifacts.len(),
                "route_artifacts": route_artifacts
                    .iter()
                    .map(serialized_route_artifact)
                    .collect::<Vec<_>>(),
                "target_count": candidate_set.targets.len(),
                "targets": candidate_set
                    .targets
                    .iter()
                    .map(serialized_candidate_target)
                    .collect::<Vec<_>>(),
                "advisory_pricing": advisory,
                "warnings": [],
                "execution_requests": [],
            }),
        }
    }

    fn route_artifacts(
        &self,
        candidate_set: &CandidateTargetSet,
        rendered_live_targets: &std::collections::BTreeMap<String, NegRiskFamilyLiveTarget>,
        full_set_basis_digest: &str,
    ) -> Vec<RouteArtifact> {
        let mut route_artifacts = vec![full_set_route_artifact(full_set_basis_digest)];
        route_artifacts.extend(candidate_set.targets.iter().map(|target| {
            neg_risk_route_artifact(target, rendered_live_targets.get(target.family_id.as_str()))
        }));
        route_artifacts.sort_by(|left, right| left.key.cmp(&right.key));
        route_artifacts
    }
}

fn serialized_candidate_target(target: &CandidateTarget) -> serde_json::Value {
    let validation = serialized_validation(&target.validation);

    json!({
        "target_id": target.target_id,
        "family_id": target.family_id.as_str(),
        "validation": validation,
    })
}

fn serialized_validation(validation: &CandidateValidationResult) -> serde_json::Value {
    match validation {
        CandidateValidationResult::Adoptable => json!({
            "status": "adoptable",
        }),
        CandidateValidationResult::Deferred { reason } => json!({
            "status": "deferred",
            "reason": reason,
        }),
        CandidateValidationResult::Rejected { reason } => json!({
            "status": "excluded",
            "reason": reason,
        }),
    }
}

fn serialized_route_artifact(artifact: &RouteArtifact) -> serde_json::Value {
    json!({
        "key": {
            "route": artifact.key.route,
            "scope": artifact.key.scope,
        },
        "route_policy_version": artifact.route_policy_version,
        "semantic_digest": artifact.semantic_digest,
        "content": artifact.content,
    })
}

fn full_set_route_artifact(full_set_basis_digest: &str) -> RouteArtifact {
    let key = StrategyKey::new("full-set", "default");
    let content = json!({
        "config_basis_digest": full_set_basis_digest,
        "mode": "static-default",
    });
    let semantic_digest = route_semantic_digest(&key, FULL_SET_ROUTE_POLICY_VERSION, &content);

    RouteArtifact {
        key,
        route_policy_version: FULL_SET_ROUTE_POLICY_VERSION.to_owned(),
        semantic_digest,
        content,
    }
}

fn neg_risk_route_artifact(
    target: &CandidateTarget,
    rendered_live_target: Option<&NegRiskFamilyLiveTarget>,
) -> RouteArtifact {
    let scope = if target.family_id.as_str().trim().is_empty() {
        format!("target:{}", target.target_id)
    } else {
        target.family_id.as_str().to_owned()
    };
    let key = StrategyKey::new("neg-risk", scope);
    let content = json!({
        "family_id": target.family_id.as_str(),
        "rendered_live_target": rendered_live_target,
        "target_id": target.target_id,
        "validation": serialized_validation(&target.validation),
    });
    let semantic_digest = route_semantic_digest(&key, NEG_RISK_ROUTE_POLICY_VERSION, &content);

    RouteArtifact {
        key,
        route_policy_version: NEG_RISK_ROUTE_POLICY_VERSION.to_owned(),
        semantic_digest,
        content,
    }
}

fn route_semantic_digest(
    key: &StrategyKey,
    route_policy_version: &str,
    content: &serde_json::Value,
) -> String {
    canonical_strategy_artifact_semantic_digest(&StrategyArtifactSemanticDigestInput {
        key: key.clone(),
        route_policy_version: route_policy_version.to_owned(),
        canonical_semantic_payload: serde_json::to_string(content)
            .expect("route artifact content should serialize"),
        source_snapshot_id: None,
        source_session_id: None,
        observed_at: None,
        strategy_candidate_revision: None,
        adoptable_strategy_revision: None,
        provenance_explanation: None,
    })
}

fn bundle_revision(prefix: &str, route_artifacts: &[RouteArtifact]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(BUNDLE_POLICY_VERSION.as_bytes());
    hasher.update(prefix.as_bytes());
    for artifact in route_artifacts {
        hasher.update(artifact.key.route.as_bytes());
        hasher.update([0]);
        hasher.update(artifact.key.scope.as_bytes());
        hasher.update([0]);
        hasher.update(artifact.semantic_digest.as_bytes());
        hasher.update([0]);
    }

    format!("{prefix}-{:x}", hasher.finalize())
}

fn strategy_candidate_revision(route_artifacts: &[RouteArtifact]) -> String {
    bundle_revision("strategy-candidate", route_artifacts)
}

fn adoptable_strategy_revision(route_artifacts: &[RouteArtifact]) -> String {
    bundle_revision("adoptable-strategy", route_artifacts)
}

fn operator_strategy_revision(
    candidate_set: &CandidateTargetSet,
    rendered_live_targets: &std::collections::BTreeMap<String, NegRiskFamilyLiveTarget>,
    full_set_basis_digest: &str,
) -> String {
    let bridge = CandidateBridge;
    let route_artifacts =
        bridge.route_artifacts(candidate_set, rendered_live_targets, full_set_basis_digest);
    bundle_revision("operator-strategy", &route_artifacts)
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
        authoritative: bool,
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
                    validation_for_record(record, restriction, authoritative),
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

        self.process_notice(notice).map(|outcome| outcome.report)
    }

    pub async fn persist_notice_for_runtime(
        notice: CandidateNotice,
    ) -> Result<DiscoveryReport, String> {
        let outcome = Self::default().process_notice(notice)?;

        if outcome.candidate.is_none()
            && outcome.adoptable.is_none()
            && outcome.provenance.is_none()
        {
            return Ok(outcome.report);
        }

        let pool = connect_pool_from_env()
            .await
            .map_err(|error| format!("candidate persistence pool error: {error}"))?;
        let artifacts = StrategyControlArtifactRepo;
        if let Some(candidate) = outcome.candidate.as_ref() {
            artifacts
                .upsert_strategy_candidate_set(&pool, candidate)
                .await
                .map_err(|error| format!("candidate persistence error: {error}"))?;
        }
        if let Some(adoptable) = outcome.adoptable.as_ref() {
            artifacts
                .upsert_adoptable_strategy_revision(&pool, adoptable)
                .await
                .map_err(|error| format!("adoptable persistence error: {error}"))?;
        }
        if let Some(provenance) = outcome.provenance.as_ref() {
            StrategyAdoptionRepo
                .upsert_provenance(&pool, provenance)
                .await
                .map_err(|error| format!("candidate provenance persistence error: {error}"))?;
        }

        Ok(outcome.report)
    }

    pub fn persist_notice_blocking(notice: CandidateNotice) -> Result<DiscoveryReport, String> {
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| format!("candidate persistence runtime error: {error}"))?;
            runtime.block_on(Self::persist_notice_for_runtime(notice))
        })
        .join()
        .map_err(|_| "candidate persistence thread panicked".to_owned())?
    }

    fn process_notice(&self, notice: CandidateNotice) -> Result<DiscoveryOutcome, String> {
        let CandidateNotice {
            publication,
            dirty_domains,
            operator_target_revision,
            rendered_live_targets,
            restriction,
            authoritative,
            full_set_basis_digest,
        } = notice;

        if !dirty_domains.contains(&DirtyDomain::Candidates) {
            return Ok(DiscoveryOutcome {
                report: DiscoveryReport {
                    candidate_revision: None,
                    adoptable_revision: None,
                    operator_target_revision,
                    target_count: 0,
                    adoptable_target_count: 0,
                    deferred_target_count: 0,
                    excluded_target_count: 0,
                    live_dispatch_woken: false,
                    disposition: "ignored".to_owned(),
                },
                candidate: None,
                adoptable: None,
                provenance: None,
            });
        }

        let _ = self.pricing;
        let (candidate_set, disposition, summary) =
            self.validation.candidate_set_from_publication(
                &publication,
                &restriction,
                operator_target_revision.as_deref(),
                authoritative,
            )?;
        let candidate = self.bridge.render_candidate_with_full_set_basis(
            &candidate_set,
            &rendered_live_targets,
            &full_set_basis_digest,
        );

        if disposition != "adoptable" {
            return Ok(DiscoveryOutcome {
                report: DiscoveryReport {
                    candidate_revision: Some(candidate.strategy_candidate_revision.clone()),
                    adoptable_revision: None,
                    operator_target_revision: None,
                    target_count: candidate_set.targets.len(),
                    adoptable_target_count: summary.adoptable_count,
                    deferred_target_count: summary.deferred_count,
                    excluded_target_count: summary.excluded_count,
                    live_dispatch_woken: false,
                    disposition,
                },
                candidate: Some(candidate),
                adoptable: None,
                provenance: None,
            });
        }

        let rendered = self.bridge.render_with_full_set_basis(
            &candidate_set,
            operator_target_revision.as_deref(),
            &rendered_live_targets,
            &full_set_basis_digest,
        )?;

        Ok(DiscoveryOutcome {
            report: DiscoveryReport {
                candidate_revision: Some(rendered.candidate.strategy_candidate_revision.clone()),
                adoptable_revision: Some(rendered.adoptable.adoptable_strategy_revision.clone()),
                operator_target_revision: Some(
                    rendered
                        .adoptable
                        .rendered_operator_strategy_revision
                        .clone(),
                ),
                target_count: candidate_set.targets.len(),
                adoptable_target_count: summary.adoptable_count,
                deferred_target_count: summary.deferred_count,
                excluded_target_count: summary.excluded_count,
                live_dispatch_woken: false,
                disposition,
            },
            provenance: None,
            candidate: Some(rendered.candidate),
            adoptable: Some(rendered.adoptable),
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
    authoritative: bool,
) -> CandidateValidationResult {
    if matches!(restriction, CandidateRestrictionTruth::Restricted { .. })
        || (!authoritative && discovery_record.backfill_completed_at.is_none())
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
