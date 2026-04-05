use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StrategyKey {
    pub route: String,
    pub scope: String,
}

impl StrategyKey {
    pub fn new(route: impl Into<String>, scope: impl Into<String>) -> Self {
        Self {
            route: route.into(),
            scope: scope.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrategyCandidateSet {
    pub strategy_candidate_revision: String,
    pub strategy_keys: Vec<StrategyKey>,
    pub semantic_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdoptableStrategyRevision {
    pub adoptable_strategy_revision: String,
    pub strategy_candidate_revision: String,
    pub rendered_operator_strategy_revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrategyAdoptionProvenance {
    pub operator_strategy_revision: String,
    pub adoptable_strategy_revision: String,
    pub strategy_candidate_revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorStrategyAdoptionRecord {
    pub adoption_id: String,
    pub action_kind: String,
    pub operator_strategy_revision: String,
    pub previous_operator_strategy_revision: Option<String>,
    pub adoptable_strategy_revision: Option<String>,
    pub strategy_candidate_revision: Option<String>,
    pub adopted_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrategyArtifactSemanticDigestInput {
    pub key: StrategyKey,
    pub route_policy_version: String,
    pub canonical_semantic_payload: String,
    pub source_snapshot_id: Option<String>,
    pub source_session_id: Option<String>,
    pub observed_at: Option<DateTime<Utc>>,
    pub strategy_candidate_revision: Option<String>,
    pub adoptable_strategy_revision: Option<String>,
    pub provenance_explanation: Option<String>,
}

pub fn canonical_strategy_artifact_semantic_digest(
    input: &StrategyArtifactSemanticDigestInput,
) -> String {
    [
        "strategy-artifact-semantic-v1".to_owned(),
        encode_component(&input.key.route),
        encode_component(&input.key.scope),
        encode_component(&input.route_policy_version),
        encode_component(&input.canonical_semantic_payload),
    ]
    .join("|")
}

fn encode_component(value: &str) -> String {
    format!("{}:{value}", value.len())
}
