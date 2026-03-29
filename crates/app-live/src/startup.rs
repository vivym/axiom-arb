use std::{collections::BTreeMap, fmt};

use config_schema::AppLiveConfigView;
use persistence::{CandidateAdoptionRepo, CandidateArtifactRepo, PersistenceError};
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
    MissingRenderedLiveTargets {
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
            Self::MissingRenderedLiveTargets {
                operator_target_revision,
            } => write!(
                f,
                "missing rendered_live_targets for operator_target_revision {operator_target_revision}"
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
    let Some(target_source) = config.target_source() else {
        let targets = NegRiskLiveTargetSet::try_from(config)?;
        let operator_target_revision = (!targets.is_empty()).then(|| targets.revision().to_owned());
        return Ok(ResolvedTargets {
            targets,
            operator_target_revision,
        });
    };

    if !target_source.is_adopted() {
        let targets = NegRiskLiveTargetSet::try_from(config)?;
        let operator_target_revision = (!targets.is_empty()).then(|| targets.revision().to_owned());
        return Ok(ResolvedTargets {
            targets,
            operator_target_revision,
        });
    }

    let operator_target_revision = target_source
        .operator_target_revision()
        .ok_or(StartupError::MissingOperatorTargetRevision)?
        .to_owned();
    let provenance = CandidateAdoptionRepo
        .get_by_operator_target_revision(pool, &operator_target_revision)
        .await?
        .ok_or_else(|| StartupError::MissingAdoptionProvenance {
            operator_target_revision: operator_target_revision.clone(),
        })?;
    let adoptable = CandidateArtifactRepo
        .get_adoptable_target_revision(pool, &provenance.adoptable_revision)
        .await?
        .ok_or_else(|| StartupError::MissingAdoptionProvenance {
            operator_target_revision: operator_target_revision.clone(),
        })?;
    let rendered_live_targets = adoptable
        .payload
        .get("rendered_live_targets")
        .ok_or_else(|| StartupError::MissingRenderedLiveTargets {
            operator_target_revision: operator_target_revision.clone(),
        })?
        .clone();
    let targets = serde_json::from_value::<BTreeMap<String, NegRiskFamilyLiveTarget>>(
        rendered_live_targets,
    )
    .map_err(|error| StartupError::InvalidRenderedLiveTargets {
        operator_target_revision: operator_target_revision.clone(),
        message: error.to_string(),
    })?;

    Ok(ResolvedTargets {
        targets: NegRiskLiveTargetSet::from_targets_with_revision(operator_target_revision.clone(), targets),
        operator_target_revision: Some(operator_target_revision),
    })
}
