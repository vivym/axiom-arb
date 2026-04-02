use std::path::Path;

use super::{
    model::{
        VerifyControlPlaneContext, VerifyControlPlaneMode, VerifyControlPlaneRolloutState,
        VerifyControlPlaneTargetSource, VerifyExpectation, VerifyScenario,
    },
    window::VerifyWindowSelection,
};
use crate::commands::status::{
    evaluate::{self, StatusDeferred, StatusOutcome},
    model::{
        StatusDetails, StatusMode, StatusReadiness, StatusRolloutState, StatusSummary,
        StatusTargetSource,
    },
};

#[derive(Debug, Clone)]
pub struct VerifyContext {
    pub scenario: VerifyScenario,
    pub expectation: VerifyExpectation,
    pub control_plane: VerifyControlPlaneContext,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigAnchorComparison {
    pub comparable: bool,
    pub reason: Option<String>,
}

pub fn load(config_path: &Path) -> VerifyContext {
    match evaluate::evaluate(config_path) {
        StatusOutcome::Summary(summary) => from_summary(summary),
        StatusOutcome::Deferred(deferred) => from_deferred(deferred),
    }
}

pub fn compare_window_to_current_config_anchor(
    context: &VerifyContext,
    window: &VerifyWindowSelection,
) -> ConfigAnchorComparison {
    if !window.is_historical_explicit() {
        return ConfigAnchorComparison {
            comparable: true,
            reason: None,
        };
    }

    if context.control_plane.target_source != Some(VerifyControlPlaneTargetSource::AdoptedTargets)
        || context.control_plane.configured_target.is_none()
    {
        return ConfigAnchorComparison {
            comparable: true,
            reason: None,
        };
    }

    ConfigAnchorComparison {
        comparable: false,
        reason: Some(
            "historical window is not provably tied to the current config anchor".to_owned(),
        ),
    }
}

fn from_summary(summary: StatusSummary) -> VerifyContext {
    let scenario = map_scenario(summary.mode);
    let expectation = derive_auto_expectation(summary.mode, summary.readiness);
    let StatusDetails {
        configured_target,
        active_target,
        target_source,
        rollout_state,
        restart_needed,
        reason,
    } = summary.details;

    VerifyContext {
        scenario,
        expectation,
        control_plane: VerifyControlPlaneContext {
            mode: summary.mode.map(map_mode),
            target_source: target_source.map(map_target_source),
            configured_target,
            active_target,
            restart_needed,
            rollout_state: rollout_state.map(map_rollout_state),
        },
        reason,
    }
}

fn from_deferred(deferred: StatusDeferred) -> VerifyContext {
    VerifyContext {
        scenario: map_scenario(Some(deferred.mode)),
        expectation: derive_auto_expectation(Some(deferred.mode), StatusReadiness::Blocked),
        control_plane: VerifyControlPlaneContext {
            mode: Some(map_mode(deferred.mode)),
            ..VerifyControlPlaneContext::default()
        },
        reason: Some(deferred.reason),
    }
}

fn derive_auto_expectation(
    mode: Option<StatusMode>,
    _readiness: StatusReadiness,
) -> VerifyExpectation {
    match mode {
        Some(StatusMode::Paper) | None => VerifyExpectation::PaperNoLive,
        Some(StatusMode::RealUserShadowSmoke) => VerifyExpectation::SmokeShadowOnly,
        Some(StatusMode::Live) => VerifyExpectation::LiveConfigConsistent,
    }
}

fn map_scenario(mode: Option<StatusMode>) -> VerifyScenario {
    match mode {
        Some(StatusMode::Paper) | None => VerifyScenario::Paper,
        Some(StatusMode::RealUserShadowSmoke) => VerifyScenario::RealUserShadowSmoke,
        Some(StatusMode::Live) => VerifyScenario::Live,
    }
}

fn map_mode(mode: StatusMode) -> VerifyControlPlaneMode {
    match mode {
        StatusMode::Paper => VerifyControlPlaneMode::Paper,
        StatusMode::RealUserShadowSmoke => VerifyControlPlaneMode::RealUserShadowSmoke,
        StatusMode::Live => VerifyControlPlaneMode::Live,
    }
}

fn map_target_source(source: StatusTargetSource) -> VerifyControlPlaneTargetSource {
    match source {
        StatusTargetSource::LegacyExplicitTargets => {
            VerifyControlPlaneTargetSource::LegacyExplicitTargets
        }
        StatusTargetSource::AdoptedTargets => VerifyControlPlaneTargetSource::AdoptedTargets,
    }
}

fn map_rollout_state(state: StatusRolloutState) -> VerifyControlPlaneRolloutState {
    match state {
        StatusRolloutState::Required => VerifyControlPlaneRolloutState::Required,
        StatusRolloutState::Ready => VerifyControlPlaneRolloutState::Ready,
    }
}
