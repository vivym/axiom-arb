use std::{collections::BTreeMap, fmt};

use config_schema::AppLiveConfigView;
use persistence::{
    CandidateAdoptionRepo, CandidateArtifactRepo, PersistenceError, StrategyAdoptionRepo,
    StrategyControlArtifactRepo,
};
use sqlx::PgPool;

use crate::config::{
    ConfigError, LocalSignerConfig, NegRiskFamilyLiveTarget, NegRiskLiveTargetSet,
    PolymarketSourceConfig,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupBundle {
    pub source_config: PolymarketSourceConfig,
    pub signer_config: Option<LocalSignerConfig>,
    pub targets: NegRiskLiveTargetSet,
    pub operator_target_revision: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTargets {
    pub targets: NegRiskLiveTargetSet,
    pub operator_target_revision: Option<String>,
}

#[derive(Debug)]
pub enum StartupError {
    Config(ConfigError),
    Persistence(PersistenceError),
    MissingOperatorTargetRevision,
    MissingAdoptionProvenance {
        operator_target_revision: String,
    },
    MissingStrategyAdoptionProvenance {
        operator_strategy_revision: String,
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
            Self::MissingOperatorTargetRevision => {
                write!(f, "missing negrisk.target_source.operator_target_revision")
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
    if let Some(target_source) = config.target_source().filter(|source| source.is_adopted()) {
        let operator_target_revision = target_source
            .operator_target_revision()
            .ok_or(StartupError::MissingOperatorTargetRevision)?
            .to_owned();
        return resolve_adopted_targets_from_operator_target_revision(
            pool,
            &operator_target_revision,
        )
        .await;
    }

    if config.has_adopted_strategy_source() {
        if let Some(operator_strategy_revision) = config.operator_strategy_revision() {
            return resolve_adopted_targets_from_operator_strategy_revision(
                pool,
                operator_strategy_revision,
            )
            .await;
        }
    }

    {
        let targets = NegRiskLiveTargetSet::try_from(config)?;
        let operator_target_revision = config
            .operator_strategy_revision()
            .map(str::to_owned)
            .or_else(|| (!targets.is_empty()).then(|| targets.revision().to_owned()));
        return Ok(ResolvedTargets {
            targets,
            operator_target_revision,
        });
    }
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

async fn resolve_adopted_targets_from_operator_strategy_revision(
    pool: &PgPool,
    operator_strategy_revision: &str,
) -> Result<ResolvedTargets, StartupError> {
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
    let targets = parse_rendered_live_targets(&adoptable.payload, operator_strategy_revision)?;

    Ok(ResolvedTargets {
        targets: NegRiskLiveTargetSet::from_targets_with_revision(
            operator_strategy_revision.to_owned(),
            targets,
        ),
        operator_target_revision: Some(operator_strategy_revision.to_owned()),
    })
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
    async fn resolve_startup_targets_accepts_pure_neutral_strategy_control_without_legacy_target_source(
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

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key"
address = "0x1111111111111111111111111111111111111111"
"#,
        )
        .unwrap();
        let validated = ValidatedConfig::new(raw).unwrap();
        let config = validated.for_app_live().unwrap();
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://axiom:axiom@localhost:5432/axiom_arb")
            .expect("lazy pool should initialize");

        let resolved = resolve_startup_targets(&pool, &config)
            .await
            .expect("pure neutral startup resolution should not require legacy target_source");

        assert!(resolved.targets.is_empty());
        assert_eq!(resolved.operator_target_revision, None);
    }
}
