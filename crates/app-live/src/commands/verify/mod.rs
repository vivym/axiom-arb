use std::{error::Error, fmt::Write as _, io, path::Path};

use crate::cli::VerifyArgs;

pub mod model;

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
    render_optional(rendered, "Mode", context.mode.as_deref());
    render_optional(rendered, "Target Source", context.target_source.as_deref());
    render_optional(
        rendered,
        "Configured Target",
        context.configured_target.as_deref(),
    );
    render_optional(rendered, "Active Target", context.active_target.as_deref());
    render_optional_bool(rendered, "Restart Needed", context.restart_needed);
    render_optional(rendered, "Rollout State", context.rollout_state.as_deref());
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

    use super::{model::VerifyReport, render_report};

    #[test]
    fn render_report_uses_expected_section_order() {
        let report = VerifyReport::fixture_for_render_test();
        let rendered = render_report(&report, Path::new("config/axiom-arb.local.toml"));
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
    }
}
