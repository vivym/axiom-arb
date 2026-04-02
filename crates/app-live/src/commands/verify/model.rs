#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyScenario {
    Paper,
    Live,
    RealUserShadowSmoke,
}

impl VerifyScenario {
    pub fn label(self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::Live => "live",
            Self::RealUserShadowSmoke => "real-user shadow smoke",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyVerdict {
    Pass,
    PassWithWarnings,
    Fail,
}

impl VerifyVerdict {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::PassWithWarnings => "pass-with-warnings",
            Self::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyExpectation {
    Auto,
    PaperNoLive,
    SmokeShadowOnly,
    LiveConfigConsistent,
}

impl VerifyExpectation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::PaperNoLive => "paper-no-live",
            Self::SmokeShadowOnly => "smoke-shadow-only",
            Self::LiveConfigConsistent => "live-config-consistent",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyControlPlaneMode {
    Paper,
    Live,
    RealUserShadowSmoke,
}

impl VerifyControlPlaneMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::Live => "live",
            Self::RealUserShadowSmoke => "real-user shadow smoke",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyControlPlaneTargetSource {
    LegacyExplicitTargets,
    AdoptedTargets,
}

impl VerifyControlPlaneTargetSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::LegacyExplicitTargets => "legacy explicit targets",
            Self::AdoptedTargets => "adopted targets",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyControlPlaneRolloutState {
    Required,
    Ready,
}

impl VerifyControlPlaneRolloutState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Ready => "ready",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VerifyResultEvidence {
    pub attempts: Vec<String>,
    pub artifacts: Vec<String>,
    pub replay: Vec<String>,
    pub side_effects: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VerifyControlPlaneContext {
    pub mode: Option<VerifyControlPlaneMode>,
    pub target_source: Option<VerifyControlPlaneTargetSource>,
    pub configured_target: Option<String>,
    pub active_target: Option<String>,
    pub restart_needed: Option<bool>,
    pub rollout_state: Option<VerifyControlPlaneRolloutState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyReport {
    pub scenario: VerifyScenario,
    pub verdict: VerifyVerdict,
    pub evidence: VerifyResultEvidence,
    pub control_plane_context: VerifyControlPlaneContext,
    pub next_actions: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::{
        VerifyControlPlaneMode, VerifyControlPlaneRolloutState, VerifyControlPlaneTargetSource,
        VerifyExpectation,
    };

    #[test]
    fn verify_expectation_labels_use_operator_vocabulary() {
        assert_eq!(VerifyExpectation::Auto.label(), "auto");
        assert_eq!(VerifyExpectation::PaperNoLive.label(), "paper-no-live");
        assert_eq!(
            VerifyExpectation::SmokeShadowOnly.label(),
            "smoke-shadow-only"
        );
        assert_eq!(
            VerifyExpectation::LiveConfigConsistent.label(),
            "live-config-consistent"
        );
    }

    #[test]
    fn verify_control_plane_labels_use_operator_vocabulary() {
        assert_eq!(VerifyControlPlaneMode::Paper.label(), "paper");
        assert_eq!(VerifyControlPlaneMode::Live.label(), "live");
        assert_eq!(
            VerifyControlPlaneMode::RealUserShadowSmoke.label(),
            "real-user shadow smoke"
        );
        assert_eq!(
            VerifyControlPlaneTargetSource::LegacyExplicitTargets.label(),
            "legacy explicit targets"
        );
        assert_eq!(
            VerifyControlPlaneTargetSource::AdoptedTargets.label(),
            "adopted targets"
        );
        assert_eq!(VerifyControlPlaneRolloutState::Required.label(), "required");
        assert_eq!(VerifyControlPlaneRolloutState::Ready.label(), "ready");
    }
}
