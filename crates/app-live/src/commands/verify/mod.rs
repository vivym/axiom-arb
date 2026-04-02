use std::{error::Error, fmt::Write as _, io, path::Path};

use persistence::connect_pool_from_env;
use tokio::runtime::Builder;

use crate::cli::VerifyArgs;

pub mod context;
pub mod evidence;
pub mod model;
pub mod window;

pub fn execute(args: VerifyArgs) -> Result<(), Box<dyn Error>> {
    let verify_context = context::load(&args.config);
    let selection = window::VerifyWindowSelection::from_args(
        args.from_seq,
        args.to_seq,
        args.attempt_id,
        args.since,
        verify_context.scenario,
    )?;
    let anchor_comparison =
        context::compare_window_to_current_config_anchor(&verify_context, &selection);

    let evidence = load_evidence_window(&verify_context, &anchor_comparison, &selection)?;
    let (verdict, reason, next_actions) =
        evaluate_foundation_outcome(&verify_context, &anchor_comparison, &evidence);
    render_foundation_report(
        &verify_context,
        verdict,
        reason.as_deref(),
        &next_actions,
        &evidence,
        &args.config,
    );

    if matches!(verdict, model::VerifyVerdict::Fail) {
        return Err(io::Error::other(reason.unwrap_or_else(|| "verify failed".to_owned())).into());
    }

    Ok(())
}

fn load_evidence_window(
    verify_context: &context::VerifyContext,
    anchor_comparison: &context::ConfigAnchorComparison,
    selection: &window::VerifyWindowSelection,
) -> Result<evidence::VerifyEvidenceWindow, Box<dyn Error>> {
    let runtime = Builder::new_current_thread().enable_all().build()?;
    let result = runtime.block_on(async {
        let pool = connect_pool_from_env().await?;
        evidence::load(&pool, selection).await
    });

    match result {
        Ok(evidence) => Ok(evidence),
        Err(error)
            if verify_context.reason.is_some()
                || verify_context.control_plane.target_source
                    == Some(model::VerifyControlPlaneTargetSource::LegacyExplicitTargets)
                || !anchor_comparison.comparable =>
        {
            Ok(evidence::VerifyEvidenceWindow::default())
        }
        Err(error) => Err(error.into()),
    }
}

fn evaluate_foundation_outcome(
    verify_context: &context::VerifyContext,
    anchor_comparison: &context::ConfigAnchorComparison,
    evidence: &evidence::VerifyEvidenceWindow,
) -> (model::VerifyVerdict, Option<String>, Vec<String>) {
    if verify_context.control_plane.target_source
        == Some(model::VerifyControlPlaneTargetSource::LegacyExplicitTargets)
    {
        return (
            model::VerifyVerdict::Fail,
            Some(verify_context.reason.clone().unwrap_or_else(|| {
                "legacy explicit targets are not supported in verify".to_owned()
            })),
            vec!["migrate to adopted target source or use lower-level commands".to_owned()],
        );
    }

    if !anchor_comparison.comparable {
        return (
            model::VerifyVerdict::PassWithWarnings,
            anchor_comparison.reason.clone(),
            vec!["rerun verify without an explicit historical window".to_owned()],
        );
    }

    if verify_context.control_plane.mode == Some(model::VerifyControlPlaneMode::Paper) {
        return evaluate_paper_outcome(evidence);
    }

    (
        model::VerifyVerdict::Fail,
        verify_context.reason.clone().or_else(|| {
            Some(
                "verify verdict rules beyond control-plane foundation are not implemented yet"
                    .to_owned(),
            )
        }),
        vec!["run app-live status --config {config}".to_owned()],
    )
}

fn evaluate_paper_outcome(
    evidence: &evidence::VerifyEvidenceWindow,
) -> (model::VerifyVerdict, Option<String>, Vec<String>) {
    let selected_live_attempt_count = evidence
        .attempts
        .iter()
        .filter(|row| matches!(row.attempt.execution_mode, domain::ExecutionMode::Live))
        .count();
    let forbidden_live_attempt_count =
        evidence.observed_live_attempts.len() + selected_live_attempt_count;

    if forbidden_live_attempt_count > 0 {
        return (
            model::VerifyVerdict::Fail,
            Some(format!(
                "forbidden live side effects: observed {} live attempt(s)",
                forbidden_live_attempt_count
            )),
            vec!["stop live activity and inspect recent local execution state".to_owned()],
        );
    }

    (
        model::VerifyVerdict::PassWithWarnings,
        Some(
            "paper verification is conservative: no forbidden live side effects were observed, but basic run evidence is incomplete".to_owned(),
        ),
        vec!["run app-live status --config {config}".to_owned()],
    )
}

fn render_foundation_report(
    verify_context: &context::VerifyContext,
    verdict: model::VerifyVerdict,
    reason: Option<&str>,
    next_actions: &[String],
    evidence: &evidence::VerifyEvidenceWindow,
    config_path: &Path,
) {
    let live_artifact_count: usize = evidence.live_artifacts.values().map(Vec::len).sum();
    let live_submission_count: usize = evidence.live_submissions.values().map(Vec::len).sum();
    let forbidden_live_attempt_count = evidence.observed_live_attempts.len();

    println!("Scenario: {}", verify_context.scenario.label());
    println!("Verdict: {}", verdict_label_upper(verdict));
    println!("Result Evidence");
    println!("Attempts: {}", evidence.attempts.len());
    println!(
        "Artifacts: {}",
        evidence.shadow_artifacts.len() + live_artifact_count
    );
    println!(
        "Replay: {}",
        evidence.shadow_artifacts.len() + evidence.journal.len()
    );
    println!(
        "Side Effects: {}",
        forbidden_live_attempt_count + live_artifact_count + live_submission_count
    );
    println!("Control-Plane Context");
    println!("Expectation: {}", verify_context.expectation.label());
    render_context_line(
        "Mode",
        verify_context.control_plane.mode.map(|value| value.label()),
    );
    render_context_line(
        "Target Source",
        verify_context
            .control_plane
            .target_source
            .map(|value| value.label()),
    );
    render_context_line(
        "Configured Target",
        verify_context.control_plane.configured_target.as_deref(),
    );
    render_context_line(
        "Active Target",
        verify_context.control_plane.active_target.as_deref(),
    );
    if let Some(restart_needed) = verify_context.control_plane.restart_needed {
        println!("Restart Needed: {restart_needed}");
    }
    render_context_line(
        "Rollout State",
        verify_context
            .control_plane
            .rollout_state
            .map(|value| value.label()),
    );
    if let Some(reason) = reason {
        println!("Reason: {reason}");
    }
    println!("Next Actions");
    for action in next_actions {
        println!(
            "Next: {}",
            action.replace("{config}", &shell_quote(config_path.display().to_string()))
        );
    }
}

fn render_context_line(label: &str, value: Option<&str>) {
    if let Some(value) = value {
        println!("{label}: {value}");
    }
}

fn verdict_label_upper(verdict: model::VerifyVerdict) -> &'static str {
    match verdict {
        model::VerifyVerdict::Pass => "PASS",
        model::VerifyVerdict::PassWithWarnings => "PASS WITH WARNINGS",
        model::VerifyVerdict::Fail => "FAIL",
    }
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
