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
        StatusAction, StatusDetails, StatusMode, StatusReadiness, StatusRolloutState,
        StatusSummary, StatusTargetSource,
    },
};

#[derive(Debug, Clone)]
pub struct VerifyContext {
    pub scenario: VerifyScenario,
    pub expectation: VerifyExpectation,
    pub control_plane: VerifyControlPlaneContext,
    pub readiness: Option<StatusReadiness>,
    pub actions: Vec<StatusAction>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigAnchorComparison {
    pub comparable: bool,
    pub reason: Option<String>,
}

pub fn load(config_path: &Path) -> VerifyContext {
    match evaluate::evaluate(config_path) {
        StatusOutcome::Summary(summary) => from_summary(*summary),
        StatusOutcome::Deferred(deferred) => from_deferred(deferred),
    }
}

pub fn parse_expectation_override(value: &str) -> Result<Option<VerifyExpectation>, String> {
    match value {
        "auto" => Ok(None),
        "paper-no-live" => Ok(Some(VerifyExpectation::PaperNoLive)),
        "smoke-shadow-only" => Ok(Some(VerifyExpectation::SmokeShadowOnly)),
        "live-config-consistent" => Ok(Some(VerifyExpectation::LiveConfigConsistent)),
        other => Err(format!(
            "unsupported verify expectation: {other}; expected auto, paper-no-live, smoke-shadow-only, or live-config-consistent"
        )),
    }
}

pub fn compare_window_to_current_config_anchor(
    _context: &VerifyContext,
    window: &VerifyWindowSelection,
) -> ConfigAnchorComparison {
    if !window.is_historical_explicit() {
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
    let readiness = summary.readiness;
    let actions = summary.actions;
    let StatusDetails {
        configured_target,
        active_target,
        target_source,
        rollout_state,
        restart_needed,
        reason,
        ..
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
        readiness: Some(readiness),
        actions,
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
        readiness: Some(StatusReadiness::Blocked),
        actions: vec![StatusAction::FixBlockingIssueAndRerunStatus],
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

#[cfg(test)]
mod tests {
    use super::{
        compare_window_to_current_config_anchor, parse_expectation_override, VerifyContext,
    };
    use crate::commands::verify::{
        model::{
            VerifyControlPlaneContext, VerifyControlPlaneMode, VerifyControlPlaneRolloutState,
            VerifyControlPlaneTargetSource, VerifyExpectation,
        },
        window::VerifyWindowSelection,
    };

    #[test]
    fn parse_expectation_override_accepts_fixed_profiles() {
        assert_eq!(parse_expectation_override("auto").unwrap(), None);
        assert_eq!(
            parse_expectation_override("paper-no-live").unwrap(),
            Some(VerifyExpectation::PaperNoLive)
        );
        assert_eq!(
            parse_expectation_override("smoke-shadow-only").unwrap(),
            Some(VerifyExpectation::SmokeShadowOnly)
        );
        assert_eq!(
            parse_expectation_override("live-config-consistent").unwrap(),
            Some(VerifyExpectation::LiveConfigConsistent)
        );
    }

    #[test]
    fn explicit_historical_windows_are_not_comparable_to_current_config() {
        let context = VerifyContext {
            scenario: crate::commands::verify::model::VerifyScenario::Live,
            expectation: VerifyExpectation::LiveConfigConsistent,
            control_plane: VerifyControlPlaneContext {
                mode: Some(VerifyControlPlaneMode::Live),
                target_source: Some(VerifyControlPlaneTargetSource::AdoptedTargets),
                configured_target: Some("targets-rev-9".to_owned()),
                active_target: Some("targets-rev-9".to_owned()),
                restart_needed: Some(false),
                rollout_state: Some(VerifyControlPlaneRolloutState::Ready),
            },
            readiness: Some(crate::commands::status::model::StatusReadiness::LiveConfigReady),
            actions: vec![crate::commands::status::model::StatusAction::RunDoctor],
            reason: None,
        };
        let window = VerifyWindowSelection::ExplicitAttemptId("attempt-old".to_owned());

        let comparison = compare_window_to_current_config_anchor(&context, &window);
        assert!(!comparison.comparable);
        assert_eq!(
            comparison.reason.as_deref(),
            Some("historical window is not provably tied to the current config anchor")
        );
    }
}
