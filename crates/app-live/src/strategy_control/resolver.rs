use std::fmt;

use config_schema::AppLiveConfigView;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalPersistenceState {
    pub operator_strategy_revision: String,
}

impl CanonicalPersistenceState {
    pub fn for_revision(operator_strategy_revision: impl Into<String>) -> Self {
        Self {
            operator_strategy_revision: operator_strategy_revision.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeProgressState {
    pub operator_strategy_revision: Option<String>,
}

impl RuntimeProgressState {
    pub fn with_strategy_revision(operator_strategy_revision: impl Into<String>) -> Self {
        Self {
            operator_strategy_revision: Some(operator_strategy_revision.into()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolveStrategyControlInput<'a> {
    pub config: &'a AppLiveConfigView<'a>,
    pub canonical_persistence: Option<CanonicalPersistenceState>,
    pub runtime_progress: Option<RuntimeProgressState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedStrategyControl {
    pub operator_strategy_revision: String,
    pub active_operator_strategy_revision: Option<String>,
    pub restart_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveStrategyControlError {
    MigrationRequired(String),
    InvalidConfig(String),
    MissingCanonicalPersistence(String),
}

impl fmt::Display for ResolveStrategyControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MigrationRequired(message)
            | Self::InvalidConfig(message)
            | Self::MissingCanonicalPersistence(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ResolveStrategyControlError {}

pub fn resolve_strategy_control(
    input: ResolveStrategyControlInput<'_>,
) -> Result<ResolvedStrategyControl, ResolveStrategyControlError> {
    let config = input.config;

    if config.has_canonical_strategy_control() {
        if config.has_target_source() || config.is_legacy_explicit_strategy_config() {
            return Err(ResolveStrategyControlError::InvalidConfig(
                "canonical strategy_control cannot be mixed with legacy negrisk control-plane input"
                    .to_owned(),
            ));
        }

        let operator_strategy_revision =
            config
                .canonical_operator_strategy_revision()
                .ok_or_else(|| {
                    ResolveStrategyControlError::InvalidConfig(
                        "missing strategy_control.operator_strategy_revision".to_owned(),
                    )
                })?;

        let persistence_matches = input
            .canonical_persistence
            .as_ref()
            .is_some_and(|persistence| {
                persistence.operator_strategy_revision == operator_strategy_revision
            });
        if !persistence_matches {
            return Err(ResolveStrategyControlError::MissingCanonicalPersistence(
                format!(
                    "operator_strategy_revision {operator_strategy_revision} has no matching canonical persistence"
                ),
            ));
        }

        let active_operator_strategy_revision = input
            .runtime_progress
            .and_then(|progress| progress.operator_strategy_revision);
        let restart_required = active_operator_strategy_revision
            .as_deref()
            .is_some_and(|active| active != operator_strategy_revision);

        return Ok(ResolvedStrategyControl {
            operator_strategy_revision: operator_strategy_revision.to_owned(),
            active_operator_strategy_revision,
            restart_required,
        });
    }

    if config.has_target_source() {
        if config.is_legacy_explicit_strategy_config() {
            return Err(ResolveStrategyControlError::InvalidConfig(
                "legacy negrisk.target_source cannot be combined with explicit negrisk.targets"
                    .to_owned(),
            ));
        }

        let operator_target_revision = config
            .target_source()
            .and_then(|target_source| target_source.operator_target_revision())
            .ok_or_else(|| {
                ResolveStrategyControlError::InvalidConfig(
                    "missing negrisk.target_source.operator_target_revision".to_owned(),
                )
            })?;
        return Err(ResolveStrategyControlError::MigrationRequired(format!(
            "legacy negrisk.target_source with operator_target_revision {operator_target_revision} requires migration"
        )));
    }

    if config.is_legacy_explicit_strategy_config() {
        if config.negrisk_targets().iter().next().is_none() {
            return Err(ResolveStrategyControlError::InvalidConfig(
                "empty negrisk.targets array is invalid legacy input".to_owned(),
            ));
        }

        return Err(ResolveStrategyControlError::MigrationRequired(
            "legacy explicit negrisk.targets requires migration".to_owned(),
        ));
    }

    Err(ResolveStrategyControlError::InvalidConfig(
        "missing strategy_control".to_owned(),
    ))
}
