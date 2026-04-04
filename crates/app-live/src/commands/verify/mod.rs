use std::{error::Error, fmt::Write as _, io, path::Path};

use crate::commands::status::model::{StatusAction, StatusMode};
use crate::commands::status::render_action_template_with_mode as render_status_action_template;
use persistence::connect_pool_from_env;
use tokio::runtime::Builder;

use crate::cli::VerifyArgs;

pub mod context;
pub mod evidence;
pub mod model;
pub mod window;

pub fn execute(args: VerifyArgs) -> Result<(), Box<dyn Error>> {
    let mut verify_context = context::load(&args.config);
    if let Some(expectation) = args
        .expect
        .as_deref()
        .map(context::parse_expectation_override)
        .transpose()?
        .flatten()
    {
        verify_context.expectation = expectation;
    }
    let selection = window::VerifyWindowSelection::from_args(
        args.from_seq,
        args.to_seq,
        args.attempt_id,
        args.since,
        verify_context.scenario,
    )?;
    let anchor_comparison =
        context::compare_window_to_current_config_anchor(&verify_context, &selection);

    let evidence = match load_evidence_window(&verify_context, &anchor_comparison, &selection) {
        Ok(evidence) => evidence,
        Err(error) => {
            let reason = verify_context
                .reason
                .clone()
                .map(|context_reason| {
                    format!("{context_reason}; failed to load local verification evidence: {error}")
                })
                .unwrap_or_else(|| format!("failed to load local verification evidence: {error}"));
            let next_actions = next_actions_from_context(
                &verify_context,
                &["run app-live status --config {config}"],
            );
            render_foundation_report(
                &verify_context,
                model::VerifyVerdict::Fail,
                Some(&reason),
                &next_actions,
                &evidence::VerifyEvidenceWindow::default(),
                &args.config,
            );
            return Err(io::Error::other(reason).into());
        }
    };
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
    _anchor_comparison: &context::ConfigAnchorComparison,
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
            if verify_context.control_plane.target_source
                == Some(model::VerifyControlPlaneTargetSource::LegacyExplicitTargets) =>
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

    if let Some((reason, next_actions)) = incompatible_expectation_outcome(verify_context) {
        return (model::VerifyVerdict::Fail, Some(reason), next_actions);
    }

    if !anchor_comparison.comparable {
        return evaluate_noncomparable_historical_outcome(
            verify_context,
            anchor_comparison,
            evidence,
        );
    }

    if verify_context.control_plane.mode == Some(model::VerifyControlPlaneMode::Paper) {
        return evaluate_paper_outcome(verify_context, evidence);
    }

    if verify_context.control_plane.mode == Some(model::VerifyControlPlaneMode::RealUserShadowSmoke)
    {
        return evaluate_smoke_outcome(verify_context, evidence);
    }

    if verify_context.control_plane.mode == Some(model::VerifyControlPlaneMode::Live) {
        return evaluate_live_outcome(verify_context, evidence);
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

fn evaluate_live_outcome(
    verify_context: &context::VerifyContext,
    evidence: &evidence::VerifyEvidenceWindow,
) -> (model::VerifyVerdict, Option<String>, Vec<String>) {
    let live_attempt_count = live_attempt_count(evidence);
    let shadow_attempt_count = live_shadow_attempt_count(evidence);
    let live_artifact_count: usize = evidence.live_artifacts.values().map(Vec::len).sum();
    let live_submission_count: usize = evidence.live_submissions.values().map(Vec::len).sum();
    let live_evidence_count = live_attempt_count + live_artifact_count + live_submission_count;

    if shadow_attempt_count > 0 {
        return (
            model::VerifyVerdict::Fail,
            Some(format!(
                "contradictory local outcomes: observed {shadow_attempt_count} shadow attempt(s) in live verification"
            )),
            vec!["inspect the latest local execution attempts before rerunning verify".to_owned()],
        );
    }

    if live_evidence_count == 0 {
        return (
            model::VerifyVerdict::Fail,
            Some(
                "contradictory local outcomes: no live results were observed for a live config"
                    .to_owned(),
            ),
            next_actions_from_context(verify_context, &["run app-live status --config {config}"]),
        );
    }

    let rollout_ready = verify_context.control_plane.rollout_state
        == Some(model::VerifyControlPlaneRolloutState::Ready);
    let active_matches_config = verify_context.control_plane.configured_target.as_deref()
        == verify_context.control_plane.active_target.as_deref();

    if rollout_ready && active_matches_config {
        (
            model::VerifyVerdict::Pass,
            Some(format!(
                "live-config-consistent: {live_attempt_count} live attempt(s) with aligned control-plane state"
            )),
            vec!["continue normal live operations".to_owned()],
        )
    } else {
        (
            model::VerifyVerdict::PassWithWarnings,
            Some(format!(
                "live results are locally consistent but readiness remains incomplete; live attempts: {live_attempt_count}"
            )),
            next_actions_from_context(
                verify_context,
                &["run app-live status --config {config}"],
            ),
        )
    }
}

fn evaluate_smoke_outcome(
    verify_context: &context::VerifyContext,
    evidence: &evidence::VerifyEvidenceWindow,
) -> (model::VerifyVerdict, Option<String>, Vec<String>) {
    let selected_shadow_attempt_count = evidence
        .attempts
        .iter()
        .filter(|row| matches!(row.attempt.execution_mode, domain::ExecutionMode::Shadow))
        .count();
    let has_run_evidence =
        selected_shadow_attempt_count > 0 || !evidence.replay_shadow_attempt_artifacts.is_empty();
    let has_consistent_shadow_artifacts = shadow_attempts_have_consistent_artifacts(evidence);
    let live_side_effect_count = smoke_live_attempt_count(evidence)
        + evidence
            .live_artifacts
            .values()
            .map(Vec::len)
            .sum::<usize>()
        + evidence
            .live_submissions
            .values()
            .map(Vec::len)
            .sum::<usize>();

    if live_side_effect_count > 0 {
        return (
            model::VerifyVerdict::Fail,
            Some(format!(
                "forbidden live side effects: observed {live_side_effect_count} live side effect(s)"
            )),
            vec!["stop live activity and inspect recent local execution state".to_owned()],
        );
    }

    if !has_run_evidence {
        return (
            model::VerifyVerdict::Fail,
            Some("no credible run evidence exists".to_owned()),
            next_actions_from_context(verify_context, &["run app-live status --config {config}"]),
        );
    }

    if has_consistent_shadow_artifacts {
        return (
            model::VerifyVerdict::Pass,
            Some(format!(
                "shadow smoke evidence is complete; shadow attempts: {}",
                selected_shadow_attempt_count
            )),
            next_actions_from_context(verify_context, &["run app-live status --config {config}"]),
        );
    }

    if verify_context.control_plane.rollout_state
        == Some(model::VerifyControlPlaneRolloutState::Required)
    {
        return (
            model::VerifyVerdict::PassWithWarnings,
            Some("rollout not ready; smoke run produced no shadow work".to_owned()),
            next_actions_from_context(verify_context, &["run app-live status --config {config}"]),
        );
    }

    (
        model::VerifyVerdict::Fail,
        Some("no credible run evidence exists".to_owned()),
        next_actions_from_context(verify_context, &["run app-live status --config {config}"]),
    )
}

fn evaluate_paper_outcome(
    verify_context: &context::VerifyContext,
    evidence: &evidence::VerifyEvidenceWindow,
) -> (model::VerifyVerdict, Option<String>, Vec<String>) {
    let forbidden_live_attempt_count = paper_live_attempt_count(evidence);

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
        next_actions_from_context(
            verify_context,
            &["run app-live status --config {config}"],
        ),
    )
}

fn evaluate_noncomparable_historical_outcome(
    verify_context: &context::VerifyContext,
    anchor_comparison: &context::ConfigAnchorComparison,
    evidence: &evidence::VerifyEvidenceWindow,
) -> (model::VerifyVerdict, Option<String>, Vec<String>) {
    match verify_context.control_plane.mode {
        Some(model::VerifyControlPlaneMode::Paper) => {
            evaluate_paper_outcome(verify_context, evidence)
        }
        Some(model::VerifyControlPlaneMode::RealUserShadowSmoke) => {
            let selected_shadow_attempt_count = evidence
                .attempts
                .iter()
                .filter(|row| matches!(row.attempt.execution_mode, domain::ExecutionMode::Shadow))
                .count();
            let has_run_evidence = selected_shadow_attempt_count > 0
                || !evidence.replay_shadow_attempt_artifacts.is_empty();
            let live_side_effect_count = smoke_live_attempt_count(evidence)
                + evidence
                    .live_artifacts
                    .values()
                    .map(Vec::len)
                    .sum::<usize>()
                + evidence
                    .live_submissions
                    .values()
                    .map(Vec::len)
                    .sum::<usize>();

            if live_side_effect_count > 0 {
                return (
                    model::VerifyVerdict::Fail,
                    Some(format!(
                        "forbidden live side effects: observed {live_side_effect_count} live side effect(s)"
                    )),
                    vec!["stop live activity and inspect recent local execution state".to_owned()],
                );
            }

            if !has_run_evidence {
                return (
                    model::VerifyVerdict::Fail,
                    Some("no credible run evidence exists".to_owned()),
                    next_actions_from_context(
                        verify_context,
                        &["run app-live status --config {config}"],
                    ),
                );
            }

            (
                model::VerifyVerdict::PassWithWarnings,
                anchor_comparison.reason.clone(),
                next_actions_with_prefix(
                    verify_context,
                    "rerun verify without an explicit historical window",
                    &["run app-live status --config {config}"],
                ),
            )
        }
        Some(model::VerifyControlPlaneMode::Live) => {
            let shadow_attempt_count = live_shadow_attempt_count(evidence);
            let live_evidence_count = live_attempt_count(evidence)
                + evidence
                    .live_artifacts
                    .values()
                    .map(Vec::len)
                    .sum::<usize>()
                + evidence
                    .live_submissions
                    .values()
                    .map(Vec::len)
                    .sum::<usize>();
            let has_any_evidence =
                live_evidence_count > 0 || shadow_attempt_count > 0 || !evidence.journal.is_empty();

            if shadow_attempt_count > 0 {
                return (
                    model::VerifyVerdict::Fail,
                    Some(format!(
                        "contradictory local outcomes: observed {shadow_attempt_count} shadow attempt(s) in live verification"
                    )),
                    vec!["inspect the latest local execution attempts before rerunning verify".to_owned()],
                );
            }

            if !has_any_evidence {
                return (
                    model::VerifyVerdict::Fail,
                    Some("no credible run evidence exists".to_owned()),
                    next_actions_from_context(
                        verify_context,
                        &["run app-live status --config {config}"],
                    ),
                );
            }

            (
                model::VerifyVerdict::PassWithWarnings,
                anchor_comparison.reason.clone(),
                next_actions_with_prefix(
                    verify_context,
                    "rerun verify without an explicit historical window",
                    &["run app-live status --config {config}"],
                ),
            )
        }
        None => (
            model::VerifyVerdict::PassWithWarnings,
            anchor_comparison.reason.clone(),
            vec!["rerun verify without an explicit historical window".to_owned()],
        ),
    }
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
    let forbidden_live_attempt_count =
        if verify_context.control_plane.mode == Some(model::VerifyControlPlaneMode::Paper) {
            paper_live_attempt_count(evidence)
        } else if verify_context.control_plane.mode
            == Some(model::VerifyControlPlaneMode::RealUserShadowSmoke)
        {
            smoke_live_attempt_count(evidence)
        } else if verify_context.control_plane.mode == Some(model::VerifyControlPlaneMode::Live) {
            live_shadow_attempt_count(evidence)
        } else {
            0
        };

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
        replay_evidence_label(verify_context, evidence)
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

fn replay_evidence_label(
    verify_context: &context::VerifyContext,
    evidence: &evidence::VerifyEvidenceWindow,
) -> String {
    if verify_context.control_plane.mode == Some(model::VerifyControlPlaneMode::RealUserShadowSmoke)
    {
        return format!(
            "shadow attempts: {}",
            evidence.replay_shadow_attempt_artifacts.len()
        );
    }

    (evidence.shadow_artifacts.len() + evidence.journal.len()).to_string()
}

fn paper_live_attempt_count(evidence: &evidence::VerifyEvidenceWindow) -> usize {
    let mut attempt_ids = std::collections::BTreeSet::new();

    for row in &evidence.observed_live_attempts {
        attempt_ids.insert(row.attempt.attempt_id.clone());
    }

    for row in &evidence.attempts {
        if matches!(row.attempt.execution_mode, domain::ExecutionMode::Live) {
            attempt_ids.insert(row.attempt.attempt_id.clone());
        }
    }

    attempt_ids.len()
}

fn smoke_live_attempt_count(evidence: &evidence::VerifyEvidenceWindow) -> usize {
    let mut attempt_ids = std::collections::BTreeSet::new();

    for row in &evidence.observed_live_attempts {
        attempt_ids.insert(row.attempt.attempt_id.clone());
    }

    for row in &evidence.attempts {
        if matches!(row.attempt.execution_mode, domain::ExecutionMode::Live) {
            attempt_ids.insert(row.attempt.attempt_id.clone());
        }
    }

    attempt_ids.len()
}

fn live_attempt_count(evidence: &evidence::VerifyEvidenceWindow) -> usize {
    evidence
        .attempts
        .iter()
        .filter(|row| matches!(row.attempt.execution_mode, domain::ExecutionMode::Live))
        .map(|row| row.attempt.attempt_id.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len()
}

fn live_shadow_attempt_count(evidence: &evidence::VerifyEvidenceWindow) -> usize {
    let mut attempt_ids = std::collections::BTreeSet::new();

    for row in &evidence.observed_shadow_attempts {
        attempt_ids.insert(row.attempt.attempt_id.clone());
    }

    for row in &evidence.attempts {
        if matches!(row.attempt.execution_mode, domain::ExecutionMode::Shadow) {
            attempt_ids.insert(row.attempt.attempt_id.clone());
        }
    }

    attempt_ids.len()
}

fn shadow_attempts_have_consistent_artifacts(evidence: &evidence::VerifyEvidenceWindow) -> bool {
    let shadow_attempt_ids = evidence
        .attempts
        .iter()
        .filter(|row| matches!(row.attempt.execution_mode, domain::ExecutionMode::Shadow))
        .map(|row| row.attempt.attempt_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();

    if shadow_attempt_ids.is_empty() {
        return false;
    }

    let shadow_artifact_attempt_ids = evidence
        .shadow_artifacts
        .iter()
        .map(|row| row.attempt_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let replay_shadow_artifact_attempt_ids = evidence
        .replay_shadow_attempt_artifacts
        .iter()
        .filter(|row| !row.artifacts.is_empty())
        .map(|row| row.attempt.attempt_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();

    shadow_attempt_ids.iter().all(|attempt_id| {
        shadow_artifact_attempt_ids.contains(attempt_id)
            || replay_shadow_artifact_attempt_ids.contains(attempt_id)
    })
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

fn incompatible_expectation_outcome(
    verify_context: &context::VerifyContext,
) -> Option<(String, Vec<String>)> {
    let expected = match verify_context.control_plane.mode {
        Some(model::VerifyControlPlaneMode::Paper) => model::VerifyExpectation::PaperNoLive,
        Some(model::VerifyControlPlaneMode::RealUserShadowSmoke) => {
            model::VerifyExpectation::SmokeShadowOnly
        }
        Some(model::VerifyControlPlaneMode::Live) => model::VerifyExpectation::LiveConfigConsistent,
        None => return None,
    };

    if verify_context.expectation == expected {
        return None;
    }

    Some((
        format!(
            "expectation {} is not compatible with {} verification",
            verify_context.expectation.label(),
            verify_context.scenario.label()
        ),
        vec![format!("rerun verify with --expect {}", expected.label())],
    ))
}

fn next_actions_from_context(
    verify_context: &context::VerifyContext,
    fallback: &[&str],
) -> Vec<String> {
    let actions = verify_context
        .actions
        .iter()
        .map(|action| status_action_label(*action, verify_context.control_plane.mode))
        .collect::<Vec<_>>();

    if actions.is_empty() {
        fallback.iter().map(|value| (*value).to_owned()).collect()
    } else {
        actions
    }
}

fn next_actions_with_prefix(
    verify_context: &context::VerifyContext,
    prefix: &str,
    fallback: &[&str],
) -> Vec<String> {
    let mut actions = vec![prefix.to_owned()];
    for action in next_actions_from_context(verify_context, fallback) {
        if !actions.contains(&action) {
            actions.push(action);
        }
    }
    actions
}

fn status_action_label(
    action: StatusAction,
    mode: Option<model::VerifyControlPlaneMode>,
) -> String {
    render_status_action_template(&action, map_verify_mode_to_status(mode))
}

fn map_verify_mode_to_status(mode: Option<model::VerifyControlPlaneMode>) -> Option<StatusMode> {
    match mode {
        Some(model::VerifyControlPlaneMode::Paper) => Some(StatusMode::Paper),
        Some(model::VerifyControlPlaneMode::RealUserShadowSmoke) => {
            Some(StatusMode::RealUserShadowSmoke)
        }
        Some(model::VerifyControlPlaneMode::Live) => Some(StatusMode::Live),
        None => None,
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
        context::{ConfigAnchorComparison, VerifyContext},
        evaluate_foundation_outcome,
        evidence::VerifyEvidenceWindow,
        model::{
            VerifyControlPlaneContext, VerifyControlPlaneMode, VerifyControlPlaneRolloutState,
            VerifyControlPlaneTargetSource, VerifyExpectation, VerifyReport, VerifyResultEvidence,
            VerifyScenario, VerifyVerdict,
        },
        next_actions_from_context, render_report,
    };
    use crate::commands::status::model::{StatusAction, StatusReadiness};
    use chrono::Utc;
    use domain::ExecutionMode;
    use persistence::models::{ExecutionAttemptRow, ExecutionAttemptWithCreatedAtRow};

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

    #[test]
    fn paper_historical_windows_still_fail_forbidden_live_side_effects() {
        let context = verify_context(
            VerifyScenario::Paper,
            VerifyExpectation::PaperNoLive,
            VerifyControlPlaneMode::Paper,
            Some(StatusReadiness::PaperReady),
            vec![StatusAction::RunAppLiveRun],
        );
        let evidence = VerifyEvidenceWindow {
            attempts: vec![attempt_row("attempt-live-1", ExecutionMode::Live)],
            ..VerifyEvidenceWindow::default()
        };

        let (verdict, reason, _next_actions) =
            evaluate_foundation_outcome(&context, &noncomparable_anchor(), &evidence);

        assert_eq!(verdict, VerifyVerdict::Fail);
        assert!(reason
            .as_deref()
            .unwrap_or_default()
            .contains("forbidden live side effects"));
    }

    #[test]
    fn live_verify_fails_when_observed_shadow_attempts_exist_outside_selected_window() {
        let context = verify_context(
            VerifyScenario::Live,
            VerifyExpectation::LiveConfigConsistent,
            VerifyControlPlaneMode::Live,
            Some(StatusReadiness::LiveConfigReady),
            vec![StatusAction::RunDoctor],
        );
        let evidence = VerifyEvidenceWindow {
            attempts: vec![attempt_row("attempt-live-1", ExecutionMode::Live)],
            observed_shadow_attempts: vec![attempt_row("attempt-shadow-1", ExecutionMode::Shadow)],
            ..VerifyEvidenceWindow::default()
        };

        let (verdict, reason, _next_actions) =
            evaluate_foundation_outcome(&context, &comparable_anchor(), &evidence);

        assert_eq!(verdict, VerifyVerdict::Fail);
        assert!(reason
            .as_deref()
            .unwrap_or_default()
            .contains("contradictory local outcomes"));
    }

    #[test]
    fn live_historical_windows_still_fail_for_shadow_contradictions() {
        let context = verify_context(
            VerifyScenario::Live,
            VerifyExpectation::LiveConfigConsistent,
            VerifyControlPlaneMode::Live,
            Some(StatusReadiness::LiveConfigReady),
            vec![StatusAction::RunDoctor],
        );
        let evidence = VerifyEvidenceWindow {
            attempts: vec![attempt_row("attempt-shadow-1", ExecutionMode::Shadow)],
            ..VerifyEvidenceWindow::default()
        };

        let (verdict, reason, _next_actions) =
            evaluate_foundation_outcome(&context, &noncomparable_anchor(), &evidence);

        assert_eq!(verdict, VerifyVerdict::Fail);
        assert!(reason
            .as_deref()
            .unwrap_or_default()
            .contains("contradictory local outcomes"));
    }

    #[test]
    fn incompatible_expectations_fail_fast_for_paper_and_smoke() {
        let paper_context = verify_context(
            VerifyScenario::Paper,
            VerifyExpectation::SmokeShadowOnly,
            VerifyControlPlaneMode::Paper,
            Some(StatusReadiness::PaperReady),
            vec![StatusAction::RunAppLiveRun],
        );
        let smoke_context = verify_context(
            VerifyScenario::RealUserShadowSmoke,
            VerifyExpectation::PaperNoLive,
            VerifyControlPlaneMode::RealUserShadowSmoke,
            Some(StatusReadiness::SmokeConfigReady),
            vec![StatusAction::RunDoctor],
        );

        let (paper_verdict, paper_reason, _) = evaluate_foundation_outcome(
            &paper_context,
            &comparable_anchor(),
            &VerifyEvidenceWindow::default(),
        );
        let (smoke_verdict, smoke_reason, _) = evaluate_foundation_outcome(
            &smoke_context,
            &comparable_anchor(),
            &VerifyEvidenceWindow::default(),
        );

        assert_eq!(paper_verdict, VerifyVerdict::Fail);
        assert!(paper_reason
            .as_deref()
            .unwrap_or_default()
            .contains("not compatible"));
        assert_eq!(smoke_verdict, VerifyVerdict::Fail);
        assert!(smoke_reason
            .as_deref()
            .unwrap_or_default()
            .contains("not compatible"));
    }

    #[test]
    fn live_warning_actions_follow_readiness_context() {
        let mut context = verify_context(
            VerifyScenario::Live,
            VerifyExpectation::LiveConfigConsistent,
            VerifyControlPlaneMode::Live,
            Some(StatusReadiness::RestartRequired),
            vec![StatusAction::PerformControlledRestart],
        );
        context.control_plane.active_target = Some("targets-rev-8".to_owned());
        let evidence = VerifyEvidenceWindow {
            attempts: vec![attempt_row("attempt-live-1", ExecutionMode::Live)],
            ..VerifyEvidenceWindow::default()
        };

        let (verdict, _reason, next_actions) =
            evaluate_foundation_outcome(&context, &comparable_anchor(), &evidence);

        assert_eq!(verdict, VerifyVerdict::PassWithWarnings);
        assert!(next_actions
            .iter()
            .any(|action| action.contains("controlled restart")));
    }

    #[test]
    fn rollout_guidance_actions_remain_concrete_operator_steps() {
        let smoke_context = verify_context(
            VerifyScenario::RealUserShadowSmoke,
            VerifyExpectation::SmokeShadowOnly,
            VerifyControlPlaneMode::RealUserShadowSmoke,
            Some(StatusReadiness::SmokeRolloutRequired),
            vec![StatusAction::EnableSmokeRollout],
        );
        let live_context = verify_context(
            VerifyScenario::Live,
            VerifyExpectation::LiveConfigConsistent,
            VerifyControlPlaneMode::Live,
            Some(StatusReadiness::LiveRolloutRequired),
            vec![StatusAction::EnableLiveRollout],
        );

        let smoke_actions =
            next_actions_from_context(&smoke_context, &["run app-live status --config {config}"]);
        let live_actions =
            next_actions_from_context(&live_context, &["run app-live status --config {config}"]);

        assert!(smoke_actions
            .iter()
            .any(|action| action.contains("app-live apply --config")));
        assert!(live_actions.iter().any(|action| {
            action.contains("approved_families") && action.contains("ready_families")
        }));
    }

    #[test]
    fn smoke_target_adoption_actions_use_apply_guidance() {
        let smoke_context = verify_context(
            VerifyScenario::RealUserShadowSmoke,
            VerifyExpectation::SmokeShadowOnly,
            VerifyControlPlaneMode::RealUserShadowSmoke,
            Some(StatusReadiness::TargetAdoptionRequired),
            vec![StatusAction::RunTargetsAdopt],
        );

        let smoke_actions =
            next_actions_from_context(&smoke_context, &["run app-live status --config {config}"]);

        assert!(smoke_actions
            .iter()
            .any(|action| action.contains("app-live apply --config")));
        assert!(!smoke_actions
            .iter()
            .any(|action| action.contains("app-live targets adopt --config")));
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

    fn verify_context(
        scenario: VerifyScenario,
        expectation: VerifyExpectation,
        mode: VerifyControlPlaneMode,
        readiness: Option<StatusReadiness>,
        actions: Vec<StatusAction>,
    ) -> VerifyContext {
        VerifyContext {
            scenario,
            expectation,
            control_plane: VerifyControlPlaneContext {
                mode: Some(mode),
                target_source: Some(VerifyControlPlaneTargetSource::AdoptedTargets),
                configured_target: Some("targets-rev-9".to_owned()),
                active_target: Some("targets-rev-9".to_owned()),
                restart_needed: Some(false),
                rollout_state: Some(VerifyControlPlaneRolloutState::Ready),
            },
            readiness,
            actions,
            reason: None,
        }
    }

    fn attempt_row(
        attempt_id: &str,
        execution_mode: ExecutionMode,
    ) -> ExecutionAttemptWithCreatedAtRow {
        ExecutionAttemptWithCreatedAtRow {
            attempt: ExecutionAttemptRow {
                attempt_id: attempt_id.to_owned(),
                plan_id: "plan-1".to_owned(),
                snapshot_id: "snapshot-1".to_owned(),
                route: "neg-risk".to_owned(),
                scope: "scope-1".to_owned(),
                matched_rule_id: None,
                execution_mode,
                attempt_no: 1,
                idempotency_key: format!("{attempt_id}-idem"),
            },
            created_at: Utc::now(),
        }
    }

    fn comparable_anchor() -> ConfigAnchorComparison {
        ConfigAnchorComparison {
            comparable: true,
            reason: None,
        }
    }

    fn noncomparable_anchor() -> ConfigAnchorComparison {
        ConfigAnchorComparison {
            comparable: false,
            reason: Some(
                "historical window is not provably tied to the current config anchor".to_owned(),
            ),
        }
    }
}
