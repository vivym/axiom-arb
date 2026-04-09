use std::{collections::BTreeMap, fmt};

use config_schema::AppLiveConfigView;
use persistence::{
    CandidateAdoptionRepo, CandidateArtifactRepo, PersistenceError, StrategyAdoptionRepo,
    StrategyControlArtifactRepo,
};
use sqlx::PgPool;

use crate::config::{
    neg_risk_live_targets_from_route_artifacts, ConfigError, LocalAccountRuntimeConfig,
    LocalRelayerRuntimeConfig, NegRiskFamilyLiveTarget, NegRiskLiveTargetSet,
    PolymarketSourceConfig, RouteRuntimeArtifact,
};
use crate::strategy_control::validate_live_route_scope;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupBundle {
    pub source_config: PolymarketSourceConfig,
    pub account_runtime_config: Option<LocalAccountRuntimeConfig>,
    pub relayer_runtime_config: Option<LocalRelayerRuntimeConfig>,
    pub targets: NegRiskLiveTargetSet,
    pub operator_target_revision: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTargets {
    pub targets: NegRiskLiveTargetSet,
    pub operator_target_revision: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedStrategyRevision {
    pub operator_strategy_revision: Option<String>,
    pub route_artifacts: BTreeMap<String, Vec<RouteRuntimeArtifact>>,
    pub compatibility_mode: bool,
}

#[derive(Debug)]
pub enum StartupError {
    Config(ConfigError),
    Persistence(PersistenceError),
    MigrationRequired(String),
    MissingOperatorTargetRevision,
    MissingOperatorStrategyRevision,
    MissingAdoptionProvenance {
        operator_target_revision: String,
    },
    MissingStrategyAdoptionProvenance {
        operator_strategy_revision: String,
    },
    MissingRouteArtifacts {
        operator_strategy_revision: String,
    },
    EmptyRouteArtifacts {
        operator_strategy_revision: String,
    },
    InvalidRouteArtifacts {
        operator_strategy_revision: String,
        message: String,
    },
    MissingRenderedLiveTargets {
        operator_target_revision: String,
    },
    EmptyRenderedLiveTargets {
        operator_target_revision: String,
    },
    InvalidRenderedLiveTargets {
        operator_target_revision: String,
        message: String,
    },
}

impl fmt::Display for StartupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(error) => write!(f, "{error}"),
            Self::Persistence(error) => write!(f, "{error}"),
            Self::MigrationRequired(message) => write!(f, "{message}"),
            Self::MissingOperatorTargetRevision => {
                write!(f, "missing negrisk.target_source.operator_target_revision")
            }
            Self::MissingOperatorStrategyRevision => {
                write!(f, "missing strategy_control.operator_strategy_revision")
            }
            Self::MissingAdoptionProvenance {
                operator_target_revision,
            } => write!(
                f,
                "operator_target_revision {operator_target_revision} could not be linked back to a candidate adoption provenance chain"
            ),
            Self::MissingStrategyAdoptionProvenance {
                operator_strategy_revision,
            } => write!(
                f,
                "operator_strategy_revision {operator_strategy_revision} could not be linked back to a strategy adoption provenance chain"
            ),
            Self::MissingRouteArtifacts {
                operator_strategy_revision,
            } => write!(
                f,
                "missing route_artifacts for operator_strategy_revision {operator_strategy_revision}"
            ),
            Self::EmptyRouteArtifacts {
                operator_strategy_revision,
            } => write!(
                f,
                "empty route_artifacts for operator_strategy_revision {operator_strategy_revision}"
            ),
            Self::InvalidRouteArtifacts {
                operator_strategy_revision,
                message,
            } => write!(
                f,
                "invalid route_artifacts for operator_strategy_revision {operator_strategy_revision}: {message}"
            ),
            Self::MissingRenderedLiveTargets {
                operator_target_revision,
            } => write!(
                f,
                "missing rendered_live_targets for operator_target_revision {operator_target_revision}"
            ),
            Self::EmptyRenderedLiveTargets {
                operator_target_revision,
            } => write!(
                f,
                "empty rendered_live_targets for operator_target_revision {operator_target_revision}"
            ),
            Self::InvalidRenderedLiveTargets {
                operator_target_revision,
                message,
            } => write!(
                f,
                "invalid rendered_live_targets for operator_target_revision {operator_target_revision}: {message}"
            ),
        }
    }
}

impl std::error::Error for StartupError {}

impl From<ConfigError> for StartupError {
    fn from(value: ConfigError) -> Self {
        Self::Config(value)
    }
}

impl From<PersistenceError> for StartupError {
    fn from(value: PersistenceError) -> Self {
        Self::Persistence(value)
    }
}

pub async fn resolve_startup_targets(
    pool: &PgPool,
    config: &AppLiveConfigView<'_>,
) -> Result<ResolvedTargets, StartupError> {
    let resolved = resolve_startup_strategy_revision(pool, config).await?;
    let targets = neg_risk_live_targets_from_route_artifacts(
        &resolved.route_artifacts,
        resolved.operator_strategy_revision.as_deref(),
    )?;
    let operator_target_revision = resolved
        .operator_strategy_revision
        .clone()
        .or_else(|| (!targets.is_empty()).then(|| targets.revision().to_owned()));

    Ok(ResolvedTargets {
        targets,
        operator_target_revision,
    })
}

pub async fn resolve_startup_strategy_revision(
    pool: &PgPool,
    config: &AppLiveConfigView<'_>,
) -> Result<ResolvedStrategyRevision, StartupError> {
    if config.is_legacy_explicit_strategy_config() {
        return Err(StartupError::MigrationRequired(
            "migration required: legacy explicit negrisk.targets requires migration".to_owned(),
        ));
    }

    if let Some(target_source) = config.target_source().filter(|source| source.is_adopted()) {
        let operator_target_revision = target_source
            .operator_target_revision()
            .ok_or(StartupError::MissingOperatorTargetRevision)?
            .to_owned();
        let targets =
            resolve_adopted_targets_from_operator_target_revision(pool, &operator_target_revision)
                .await?
                .targets;
        return Ok(ResolvedStrategyRevision {
            operator_strategy_revision: Some(operator_target_revision.clone()),
            route_artifacts: compatibility_route_artifacts_from_targets(&targets),
            compatibility_mode: false,
        });
    }

    if config.has_adopted_strategy_source()
        && config.target_source().is_none()
        && !config.is_legacy_explicit_strategy_config()
    {
        let operator_strategy_revision = config
            .operator_strategy_revision()
            .ok_or(StartupError::MissingOperatorStrategyRevision)?;
        return resolve_adopted_strategy_revision(pool, operator_strategy_revision).await;
    }

    let targets = NegRiskLiveTargetSet::try_from(config)?;
    Ok(ResolvedStrategyRevision {
        operator_strategy_revision: config.operator_strategy_revision().map(str::to_owned),
        route_artifacts: compatibility_route_artifacts_from_targets(&targets),
        compatibility_mode: config.is_legacy_explicit_strategy_config(),
    })
}

pub async fn resolve_route_artifacts_for_operator_target_revision(
    pool: &PgPool,
    operator_target_revision: &str,
) -> Result<BTreeMap<String, Vec<RouteRuntimeArtifact>>, StartupError> {
    let targets =
        resolve_adopted_targets_from_operator_target_revision(pool, operator_target_revision)
            .await?
            .targets;
    Ok(compatibility_route_artifacts_from_targets(&targets))
}

pub async fn resolve_route_artifacts_for_operator_strategy_revision(
    pool: &PgPool,
    operator_strategy_revision: &str,
) -> Result<BTreeMap<String, Vec<RouteRuntimeArtifact>>, StartupError> {
    resolve_adopted_strategy_revision(pool, operator_strategy_revision)
        .await
        .map(|resolved| resolved.route_artifacts)
}

async fn resolve_adopted_targets_from_operator_target_revision(
    pool: &PgPool,
    operator_target_revision: &str,
) -> Result<ResolvedTargets, StartupError> {
    let provenance = CandidateAdoptionRepo
        .get_by_operator_target_revision(pool, operator_target_revision)
        .await?
        .ok_or_else(|| StartupError::MissingAdoptionProvenance {
            operator_target_revision: operator_target_revision.to_owned(),
        })?;
    let adoptable = CandidateArtifactRepo
        .get_adoptable_target_revision(pool, &provenance.adoptable_revision)
        .await?
        .ok_or_else(|| StartupError::MissingAdoptionProvenance {
            operator_target_revision: operator_target_revision.to_owned(),
        })?;
    let targets = parse_rendered_live_targets(&adoptable.payload, operator_target_revision)?;

    Ok(ResolvedTargets {
        targets: NegRiskLiveTargetSet::from_targets_with_revision(
            operator_target_revision.to_owned(),
            targets,
        ),
        operator_target_revision: Some(operator_target_revision.to_owned()),
    })
}

async fn resolve_adopted_strategy_revision(
    pool: &PgPool,
    operator_strategy_revision: &str,
) -> Result<ResolvedStrategyRevision, StartupError> {
    let provenance = StrategyAdoptionRepo
        .get_by_operator_strategy_revision(pool, operator_strategy_revision)
        .await?
        .ok_or_else(|| StartupError::MissingStrategyAdoptionProvenance {
            operator_strategy_revision: operator_strategy_revision.to_owned(),
        })?;
    let adoptable = StrategyControlArtifactRepo
        .get_adoptable_strategy_revision(pool, &provenance.adoptable_strategy_revision)
        .await?
        .ok_or_else(|| StartupError::MissingStrategyAdoptionProvenance {
            operator_strategy_revision: operator_strategy_revision.to_owned(),
        })?;
    let route_artifacts = parse_route_artifacts(&adoptable.payload, operator_strategy_revision)
        .or_else(|error| match error {
            StartupError::MissingRouteArtifacts { .. } => {
                let targets =
                    parse_rendered_live_targets(&adoptable.payload, operator_strategy_revision)?;
                Ok(compatibility_route_artifacts_from_targets(
                    &NegRiskLiveTargetSet::from_targets_with_revision(
                        operator_strategy_revision.to_owned(),
                        targets,
                    ),
                ))
            }
            other => Err(other),
        })?;

    Ok(ResolvedStrategyRevision {
        operator_strategy_revision: Some(operator_strategy_revision.to_owned()),
        route_artifacts,
        compatibility_mode: false,
    })
}

#[derive(serde::Deserialize)]
struct SerializedRouteArtifactKey {
    route: String,
    scope: String,
}

#[derive(serde::Deserialize)]
struct SerializedRouteArtifact {
    key: SerializedRouteArtifactKey,
    route_policy_version: String,
    semantic_digest: String,
    content: serde_json::Value,
}

fn parse_route_artifacts(
    payload: &serde_json::Value,
    operator_strategy_revision: &str,
) -> Result<BTreeMap<String, Vec<RouteRuntimeArtifact>>, StartupError> {
    let artifacts = payload
        .get("route_artifacts")
        .ok_or_else(|| StartupError::MissingRouteArtifacts {
            operator_strategy_revision: operator_strategy_revision.to_owned(),
        })?
        .clone();
    let artifacts =
        serde_json::from_value::<Vec<SerializedRouteArtifact>>(artifacts).map_err(|error| {
            StartupError::InvalidRouteArtifacts {
                operator_strategy_revision: operator_strategy_revision.to_owned(),
                message: error.to_string(),
            }
        })?;

    if artifacts.is_empty() {
        return Err(StartupError::EmptyRouteArtifacts {
            operator_strategy_revision: operator_strategy_revision.to_owned(),
        });
    }

    let mut grouped = BTreeMap::new();
    for artifact in artifacts {
        validate_live_route_scope(&artifact.key.route, &artifact.key.scope).map_err(|error| {
            StartupError::InvalidRouteArtifacts {
                operator_strategy_revision: operator_strategy_revision.to_owned(),
                message: error.to_string(),
            }
        })?;
        grouped
            .entry(artifact.key.route)
            .or_insert_with(Vec::new)
            .push(RouteRuntimeArtifact {
                scope: artifact.key.scope,
                route_policy_version: artifact.route_policy_version,
                semantic_digest: artifact.semantic_digest,
                content: artifact.content,
            });
    }
    Ok(grouped)
}

fn compatibility_route_artifacts_from_targets(
    targets: &NegRiskLiveTargetSet,
) -> BTreeMap<String, Vec<RouteRuntimeArtifact>> {
    let mut route_artifacts = BTreeMap::new();
    route_artifacts.insert(
        "full-set".to_owned(),
        vec![RouteRuntimeArtifact {
            scope: "default".to_owned(),
            route_policy_version: "full-set-route-policy-v1".to_owned(),
            semantic_digest: "full-set-basis-default".to_owned(),
            content: serde_json::json!({
                "config_basis_digest": "full-set-basis-default",
                "mode": "static-default",
            }),
        }],
    );
    route_artifacts.insert(
        "neg-risk".to_owned(),
        targets
            .targets()
            .values()
            .map(|target| RouteRuntimeArtifact {
                scope: target.family_id.clone(),
                route_policy_version: "neg-risk-route-policy-v1".to_owned(),
                semantic_digest: target.family_id.clone(),
                content: serde_json::json!({
                    "family_id": target.family_id,
                    "rendered_live_target": target,
                    "target_id": format!("candidate-target-{}", target.family_id),
                    "validation": {
                        "status": "adoptable",
                    },
                }),
            })
            .collect(),
    );
    route_artifacts
}

fn parse_rendered_live_targets(
    payload: &serde_json::Value,
    operator_revision: &str,
) -> Result<BTreeMap<String, NegRiskFamilyLiveTarget>, StartupError> {
    let rendered_live_targets = payload
        .get("rendered_live_targets")
        .ok_or_else(|| StartupError::MissingRenderedLiveTargets {
            operator_target_revision: operator_revision.to_owned(),
        })?
        .clone();
    let targets =
        serde_json::from_value::<BTreeMap<String, NegRiskFamilyLiveTarget>>(rendered_live_targets)
            .map_err(|error| StartupError::InvalidRenderedLiveTargets {
                operator_target_revision: operator_revision.to_owned(),
                message: error.to_string(),
            })?;

    if targets.is_empty() {
        return Err(StartupError::EmptyRenderedLiveTargets {
            operator_target_revision: operator_revision.to_owned(),
        });
    }

    Ok(targets)
}

#[cfg(test)]
mod tests {
    use config_schema::{load_raw_config_from_str, ValidatedConfig};
    use sqlx::postgres::PgPoolOptions;

    use super::resolve_startup_targets;

    #[tokio::test]
    async fn resolve_startup_targets_rejects_pure_neutral_strategy_control_without_operator_strategy_revision(
    ) {
        let raw = load_raw_config_from_str(
            r#"
[runtime]
mode = "live"
real_user_shadow_smoke = true

[strategy_control]
source = "adopted"

[strategies.neg_risk]
enabled = true

[strategies.neg_risk.rollout]
approved_scopes = []
ready_scopes = []

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key"
secret = "poly-secret"
passphrase = "poly-passphrase"
"#,
        )
        .unwrap();
        let validated = ValidatedConfig::new(raw).unwrap();
        let config = validated.for_app_live().unwrap();
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://axiom:axiom@localhost:5432/axiom_arb")
            .expect("lazy pool should initialize");

        let error = resolve_startup_targets(&pool, &config)
            .await
            .expect_err("missing neutral adopted revision should fail closed");

        assert_eq!(
            error.to_string(),
            "missing strategy_control.operator_strategy_revision"
        );
    }
}
