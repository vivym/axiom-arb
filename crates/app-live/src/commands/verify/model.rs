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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VerifyResultEvidence {
    pub attempts: Vec<String>,
    pub artifacts: Vec<String>,
    pub replay: Vec<String>,
    pub side_effects: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VerifyControlPlaneContext {
    pub mode: Option<String>,
    pub target_source: Option<String>,
    pub configured_target: Option<String>,
    pub active_target: Option<String>,
    pub restart_needed: Option<bool>,
    pub rollout_state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyReport {
    pub scenario: VerifyScenario,
    pub verdict: VerifyVerdict,
    pub evidence: VerifyResultEvidence,
    pub control_plane_context: VerifyControlPlaneContext,
    pub next_actions: Vec<String>,
}

impl VerifyReport {
    pub fn fixture_for_render_test() -> Self {
        Self {
            scenario: VerifyScenario::Paper,
            verdict: VerifyVerdict::PassWithWarnings,
            evidence: VerifyResultEvidence {
                attempts: vec!["1 attempt".to_owned()],
                artifacts: vec!["replay transcript".to_owned()],
                replay: vec!["shadow replay completed".to_owned()],
                side_effects: vec!["no live side effects".to_owned()],
            },
            control_plane_context: VerifyControlPlaneContext {
                mode: Some("paper".to_owned()),
                target_source: Some("adopted targets".to_owned()),
                configured_target: Some("targets-rev-9".to_owned()),
                active_target: Some("targets-rev-9".to_owned()),
                restart_needed: Some(false),
                rollout_state: Some("ready".to_owned()),
            },
            next_actions: vec!["app-live status --config {config}".to_owned()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::VerifyExpectation;

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
}
