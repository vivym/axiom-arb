use std::{error::Error, fmt::Write as _, io, path::Path};

use crate::cli::VerifyArgs;

pub mod model;
pub mod window;

pub fn execute(_args: VerifyArgs) -> Result<(), Box<dyn Error>> {
    let error = io::Error::new(
        io::ErrorKind::Other,
        "verify is not implemented yet; this command currently exposes only the CLI surface",
    );
    eprintln!("{error}");
    Err(error.into())
}

pub fn render_report(report: &model::VerifyReport, config_path: &Path) -> String {
    let mut rendered = String::new();
    writeln!(&mut rendered, "Scenario").expect("write scenario heading");
    writeln!(&mut rendered, "{}", report.scenario.label()).expect("write scenario label");
    writeln!(&mut rendered, "Verdict").expect("write verdict heading");
    writeln!(&mut rendered, "{}", report.verdict.label()).expect("write verdict label");
    writeln!(&mut rendered, "Result Evidence").expect("write evidence heading");
    render_evidence(&mut rendered, &report.evidence);
    writeln!(&mut rendered, "Control-Plane Context").expect("write context heading");
    render_context(&mut rendered, &report.control_plane_context);
    writeln!(&mut rendered, "Next Actions").expect("write next actions heading");
    for action in &report.next_actions {
        writeln!(
            &mut rendered,
            "Next: {}",
            action.replace("{config}", &shell_quote(config_path.display().to_string()))
        )
        .expect("write next action");
    }

    rendered
}

fn render_evidence(rendered: &mut String, evidence: &model::VerifyResultEvidence) {
    render_list(rendered, "Attempts", &evidence.attempts);
    render_list(rendered, "Artifacts", &evidence.artifacts);
    render_list(rendered, "Replay", &evidence.replay);
    render_list(rendered, "Side Effects", &evidence.side_effects);
}

fn render_context(rendered: &mut String, context: &model::VerifyControlPlaneContext) {
    render_optional(rendered, "Mode", context.mode.map(|value| value.label()));
    render_optional(
        rendered,
        "Target Source",
        context.target_source.map(|value| value.label()),
    );
    render_optional(
        rendered,
        "Configured Target",
        context.configured_target.as_deref(),
    );
    render_optional(rendered, "Active Target", context.active_target.as_deref());
    render_optional_bool(rendered, "Restart Needed", context.restart_needed);
    render_optional(
        rendered,
        "Rollout State",
        context.rollout_state.map(|value| value.label()),
    );
}

fn render_list(rendered: &mut String, label: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }

    for value in values {
        writeln!(rendered, "{label}: {value}").expect("write list item");
    }
}

fn render_optional(rendered: &mut String, label: &str, value: Option<&str>) {
    if let Some(value) = value {
        writeln!(rendered, "{label}: {value}").expect("write optional item");
    }
}

fn render_optional_bool(rendered: &mut String, label: &str, value: Option<bool>) {
    if let Some(value) = value {
        writeln!(rendered, "{label}: {value}").expect("write optional bool item");
    }
}

fn shell_quote(value: String) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '-' | '_'))
    {
        value
    } else {
        format!("'{}'", value.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        model::{
            VerifyControlPlaneContext, VerifyControlPlaneMode, VerifyControlPlaneRolloutState,
            VerifyControlPlaneTargetSource, VerifyReport, VerifyResultEvidence, VerifyScenario,
            VerifyVerdict,
        },
        render_report,
    };

    #[test]
    fn render_report_renders_populated_content_and_quotes_config_paths() {
        let report = populated_report_fixture();
        let rendered = render_report(&report, Path::new("config/axiom arb's local.toml"));

        assert!(rendered.contains("Scenario\npaper\n"), "{rendered}");
        assert!(
            rendered.contains("Verdict\npass-with-warnings\n"),
            "{rendered}"
        );
        assert!(rendered.contains("Result Evidence\n"), "{rendered}");
        assert!(rendered.contains("Attempts: 1 attempt\n"), "{rendered}");
        assert!(
            rendered.contains("Artifacts: replay transcript\n"),
            "{rendered}"
        );
        assert!(
            rendered.contains("Replay: shadow replay completed\n"),
            "{rendered}"
        );
        assert!(
            rendered.contains("Side Effects: no live side effects\n"),
            "{rendered}"
        );
        assert!(rendered.contains("Control-Plane Context\n"), "{rendered}");
        assert!(rendered.contains("Mode: paper\n"), "{rendered}");
        assert!(
            rendered.contains("Target Source: adopted targets\n"),
            "{rendered}"
        );
        assert!(
            rendered.contains("Configured Target: targets-rev-9\n"),
            "{rendered}"
        );
        assert!(
            rendered.contains("Active Target: targets-rev-9\n"),
            "{rendered}"
        );
        assert!(rendered.contains("Restart Needed: false\n"), "{rendered}");
        assert!(rendered.contains("Rollout State: ready\n"), "{rendered}");
        assert!(
            rendered
                .contains("Next: app-live status --config 'config/axiom arb'\\''s local.toml'\n"),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "Next: app-live targets adopt --config 'config/axiom arb'\\''s local.toml'\n"
            ),
            "{rendered}"
        );
    }

    #[test]
    fn render_report_handles_sparse_content_without_reordering_sections() {
        let report = sparse_report_fixture();
        let rendered = render_report(&report, Path::new("config/axiom-arb.local.toml"));

        assert!(rendered.contains("Scenario\nlive\n"), "{rendered}");
        assert!(rendered.contains("Verdict\nfail\n"), "{rendered}");
        assert!(rendered.contains("Result Evidence\n"), "{rendered}");
        assert!(rendered.contains("Control-Plane Context\n"), "{rendered}");
        assert!(rendered.contains("Next Actions\n"), "{rendered}");
        assert!(rendered.find("Scenario").unwrap() < rendered.find("Verdict").unwrap());
        assert!(rendered.find("Verdict").unwrap() < rendered.find("Result Evidence").unwrap());
        assert!(
            rendered.find("Result Evidence").unwrap()
                < rendered.find("Control-Plane Context").unwrap()
        );
        assert!(
            rendered.find("Control-Plane Context").unwrap()
                < rendered.find("Next Actions").unwrap()
        );
        assert!(!rendered.contains("Attempts:"), "{rendered}");
        assert!(!rendered.contains("Mode:"), "{rendered}");
        assert!(!rendered.contains("Target Source:"), "{rendered}");
        assert!(!rendered.contains("Restart Needed:"), "{rendered}");
    }

    fn populated_report_fixture() -> VerifyReport {
        VerifyReport {
            scenario: VerifyScenario::Paper,
            verdict: VerifyVerdict::PassWithWarnings,
            evidence: VerifyResultEvidence {
                attempts: vec!["1 attempt".to_owned()],
                artifacts: vec!["replay transcript".to_owned()],
                replay: vec!["shadow replay completed".to_owned()],
                side_effects: vec!["no live side effects".to_owned()],
            },
            control_plane_context: VerifyControlPlaneContext {
                mode: Some(VerifyControlPlaneMode::Paper),
                target_source: Some(VerifyControlPlaneTargetSource::AdoptedTargets),
                configured_target: Some("targets-rev-9".to_owned()),
                active_target: Some("targets-rev-9".to_owned()),
                restart_needed: Some(false),
                rollout_state: Some(VerifyControlPlaneRolloutState::Ready),
            },
            next_actions: vec![
                "app-live status --config {config}".to_owned(),
                "app-live targets adopt --config {config}".to_owned(),
            ],
        }
    }

    fn sparse_report_fixture() -> VerifyReport {
        VerifyReport {
            scenario: VerifyScenario::Live,
            verdict: VerifyVerdict::Fail,
            evidence: VerifyResultEvidence::default(),
            control_plane_context: VerifyControlPlaneContext::default(),
            next_actions: Vec::new(),
        }
    }
}
