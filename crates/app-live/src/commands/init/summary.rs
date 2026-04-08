use std::path::Path;

use super::render::LiveInitWalletKind;

pub struct InitSummary<'a> {
    pub mode: WizardMode,
    pub wallet_kind: LiveInitWalletKind,
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
    lines.push("[polymarket.account]".to_string());
    lines.push(format!(
        "wallet kind = {}",
        summary.wallet_kind.option_label()
    ));
    if summary.wallet_kind.requires_relayer_auth() {
        lines.push("[polymarket.relayer_auth]".to_string());
    }
    lines.push(polymarket_source_summary_line(
        summary.has_existing_polymarket_source,
        summary.has_existing_polymarket_source_overrides,
    ));
    lines.push("[negrisk.target_source]".to_string());
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
        (false, false) => "polymarket source uses built-in defaults; use [polymarket.source_overrides] only for non-default endpoints or cadence, and set HTTPS_PROXY or ALL_PROXY in the environment if Polymarket traffic must traverse an outbound proxy.".to_string(),
        (true, true) => "kept existing [polymarket.source_overrides] and dropped legacy [polymarket.source].".to_string(),
        (true, false) => "migrated existing [polymarket.source] into [polymarket.source_overrides].".to_string(),
        (false, true) => "preserved existing [polymarket.source_overrides].".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::polymarket_source_summary_line;

    #[test]
    fn default_polymarket_source_summary_line_uses_env_proxy_guidance() {
        let line = polymarket_source_summary_line(false, false);

        assert!(line.contains("HTTPS_PROXY") || line.contains("ALL_PROXY"));
        assert!(!line.contains("[polymarket.http]"));
        assert!(!line.contains("proxy_url"));
    }
}
