use std::path::Path;

pub struct InitSummary<'a> {
    pub mode: WizardMode,
    pub config_path: &'a Path,
    pub has_existing_polymarket_source: bool,
    pub has_existing_polymarket_source_overrides: bool,
    pub configured_operator_target_revision: Option<&'a str>,
    pub rollout_is_empty: bool,
}

#[derive(Clone, Copy)]
pub enum WizardMode {
    Paper,
    Live,
    Smoke,
}

pub struct WizardSummary {
    pub sections: Vec<WizardSummarySection>,
}

pub struct WizardSummarySection {
    pub title: &'static str,
    pub lines: Vec<String>,
}

pub fn render(summary: InitSummary<'_>) -> WizardSummary {
    match summary.mode {
        WizardMode::Paper => render_paper_summary(summary.config_path),
        WizardMode::Live | WizardMode::Smoke => render_live_summary(summary),
    }
}

fn render_paper_summary(config_path: &Path) -> WizardSummary {
    let quoted_config_path = shell_quote(config_path.display().to_string());
    WizardSummary {
        sections: vec![
            WizardSummarySection {
                title: "What Was Written",
                lines: paper_config_lines()
                    .iter()
                    .map(|line| (*line).to_string())
                    .collect(),
            },
            WizardSummarySection {
                title: "What To Run Next",
                lines: vec![
                    format!("app-live doctor --config {quoted_config_path}"),
                    format!("app-live run --config {quoted_config_path}"),
                ],
            },
        ],
    }
}

fn render_live_summary(summary: InitSummary<'_>) -> WizardSummary {
    let quoted_config_path = shell_quote(summary.config_path.display().to_string());
    let mut lines = vec!["[runtime]".to_string(), "mode = \"live\"".to_string()];
    if matches!(summary.mode, WizardMode::Smoke) {
        lines.push("real_user_shadow_smoke = true".to_string());
    }
    lines.extend([
        "[polymarket.account]".to_string(),
        "[polymarket.relayer_auth]".to_string(),
        polymarket_source_summary_line(
            summary.has_existing_polymarket_source,
            summary.has_existing_polymarket_source_overrides,
        ),
        "[negrisk.target_source]".to_string(),
    ]);
    if let Some(revision) = summary.configured_operator_target_revision {
        lines.push(format!("operator_target_revision = \"{revision}\""));
    }
    lines.push("[negrisk.rollout]".to_string());
    if summary.rollout_is_empty {
        lines.push("approved_families = []".to_string());
        lines.push("ready_families = []".to_string());
        lines.push(
            "negrisk rollout is still empty, so negrisk work remains inactive until you adopt candidates."
                .to_string(),
        );
    } else {
        lines.push("approved_families = [..existing..]".to_string());
        lines.push("ready_families = [..existing..]".to_string());
    }

    let mut next_steps = vec![format!("app-live doctor --config {quoted_config_path}")];
    if summary.configured_operator_target_revision.is_none() {
        next_steps.insert(
            0,
            format!("app-live targets adopt --config {quoted_config_path} --adoptable-revision ADOPTABLE_REVISION"),
        );
        next_steps.insert(
            0,
            format!("app-live targets candidates --config {quoted_config_path}"),
        );
    }
    next_steps.push(format!("app-live run --config {quoted_config_path}"));

    WizardSummary {
        sections: vec![
            WizardSummarySection {
                title: "What Was Written",
                lines,
            },
            WizardSummarySection {
                title: "What To Run Next",
                lines: next_steps,
            },
        ],
    }
}

pub(crate) fn paper_config_lines() -> &'static [&'static str] {
    &["[runtime]", "mode = \"paper\""]
}

fn shell_quote(value: String) -> String {
    let escaped = value.replace('\'', r"'\''");
    format!("'{escaped}'")
}

fn polymarket_source_summary_line(
    has_existing_polymarket_source: bool,
    has_existing_polymarket_source_overrides: bool,
) -> String {
    match (
        has_existing_polymarket_source,
        has_existing_polymarket_source_overrides,
    ) {
        (false, false) => "polymarket source uses built-in defaults; use [polymarket.source_overrides] only for non-default endpoints or cadence, and [polymarket.http] only for explicit outbound proxying.".to_string(),
        (true, true) => "kept existing [polymarket.source_overrides] and dropped legacy [polymarket.source].".to_string(),
        (true, false) => "migrated existing [polymarket.source] into [polymarket.source_overrides].".to_string(),
        (false, true) => "preserved existing [polymarket.source_overrides].".to_string(),
    }
}
