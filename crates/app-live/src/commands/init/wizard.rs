use std::path::Path;

use super::{
    prompt::{self, PromptIo},
    render::{self, LiveInitAnswers},
    summary::{self, InitSummary, WizardMode, WizardSummary},
    InitError,
};
use config_schema::RawAxiomConfig;

pub struct WizardResult {
    pub rendered_config: String,
    pub summary: WizardSummary,
}

pub(crate) fn paper(config_path: &Path) -> WizardResult {
    WizardResult {
        rendered_config: render_paper_config(),
        summary: summary::render(InitSummary {
            mode: WizardMode::Paper,
            config_path,
            configured_operator_target_revision: None,
            rollout_is_empty: true,
        }),
    }
}

pub fn run<P: PromptIo>(prompt: &mut P, config_path: &Path) -> Result<WizardResult, InitError> {
    match prompt::select_one(prompt, "Choose an init mode:", &["paper", "live", "smoke"])? {
        0 => {
            confirm_replace_if_needed(prompt, config_path)?;
            Ok(paper(config_path))
        }
        1 => render_live_or_smoke(prompt, config_path, false),
        2 => render_live_or_smoke(prompt, config_path, true),
        _ => unreachable!("prompt selection should stay within the provided options"),
    }
}

fn confirm_replace_if_needed<P: PromptIo>(
    prompt: &mut P,
    config_path: &Path,
) -> Result<(), InitError> {
    if config_path.exists() {
        prompt::select_one(
            prompt,
            "Config already exists. Type replace to overwrite:",
            &["replace"],
        )?;
    }
    Ok(())
}

fn render_live_or_smoke<P: PromptIo>(
    prompt: &mut P,
    config_path: &Path,
    real_user_shadow_smoke: bool,
) -> Result<WizardResult, InitError> {
    let existing_config = if config_path.exists() {
        match select_existing_config_mode(prompt)? {
            ExistingConfigMode::Preserve => Some(
                config_schema::load_raw_config_from_path(config_path)
                    .map_err(|error| InitError::new(error.to_string()))?,
            ),
            ExistingConfigMode::Replace => None,
        }
    } else {
        None
    };
    let answers = collect_live_answers(prompt)?;
    let rendered_config =
        render::render_live_config(answers, real_user_shadow_smoke, existing_config.as_ref())?;
    Ok(WizardResult {
        rendered_config,
        summary: summary::render(build_summary_view(
            config_path,
            real_user_shadow_smoke,
            existing_config.as_ref(),
        )),
    })
}

fn build_summary_view<'a>(
    config_path: &'a Path,
    real_user_shadow_smoke: bool,
    existing_config: Option<&'a RawAxiomConfig>,
) -> InitSummary<'a> {
    let configured_operator_target_revision = existing_config
        .and_then(|config| config.negrisk.as_ref())
        .and_then(|negrisk| negrisk.target_source.as_ref())
        .and_then(|target_source| target_source.operator_target_revision.as_deref());
    let rollout_is_empty = existing_config
        .and_then(|config| config.negrisk.as_ref())
        .and_then(|negrisk| negrisk.rollout.as_ref())
        .map(|rollout| rollout.approved_families.is_empty() && rollout.ready_families.is_empty())
        .unwrap_or(true);

    InitSummary {
        mode: if real_user_shadow_smoke {
            WizardMode::Smoke
        } else {
            WizardMode::Live
        },
        config_path,
        configured_operator_target_revision,
        rollout_is_empty,
    }
}

enum ExistingConfigMode {
    Preserve,
    Replace,
}

fn select_existing_config_mode<P: PromptIo>(
    prompt: &mut P,
) -> Result<ExistingConfigMode, InitError> {
    loop {
        prompt.println("Config already exists. Choose preserve or replace:")?;
        prompt.println("preserve")?;
        prompt.println("replace")?;

        match prompt.read_line()?.trim().to_lowercase().as_str() {
            "" | "preserve" => return Ok(ExistingConfigMode::Preserve),
            "replace" => return Ok(ExistingConfigMode::Replace),
            _ => prompt.println("Please choose one of the listed options.")?,
        }
    }
}

fn collect_live_answers<P: PromptIo>(prompt: &mut P) -> Result<LiveInitAnswers, InitError> {
    let account_address = prompt::ask_nonempty(prompt, "Account address:")?;
    let funder_address = ask_optional(prompt, "Funder address (optional, press Enter to skip):")?;
    let account_api_key = prompt::ask_nonempty(prompt, "Account API key:")?;
    let account_secret = prompt::ask_nonempty(prompt, "Account secret:")?;
    let account_passphrase = prompt::ask_nonempty(prompt, "Account passphrase:")?;
    let relayer_auth_kind = match prompt::select_one(
        prompt,
        "Choose a relayer auth type:",
        &["builder_api_key", "relayer_api_key"],
    )? {
        0 => config_schema::RelayerAuthKindToml::BuilderApiKey,
        1 => config_schema::RelayerAuthKindToml::RelayerApiKey,
        _ => unreachable!("prompt selection should stay within the provided options"),
    };
    let relayer_api_key = prompt::ask_nonempty(prompt, "Relayer API key:")?;

    let (relayer_secret, relayer_passphrase, relayer_address) = match relayer_auth_kind {
        config_schema::RelayerAuthKindToml::BuilderApiKey => {
            let relayer_secret = prompt::ask_nonempty(prompt, "Relayer secret:")?;
            let relayer_passphrase = prompt::ask_nonempty(prompt, "Relayer passphrase:")?;
            (Some(relayer_secret), Some(relayer_passphrase), None)
        }
        config_schema::RelayerAuthKindToml::RelayerApiKey => {
            let relayer_address = prompt::ask_nonempty(prompt, "Relayer address:")?;
            (None, None, Some(relayer_address))
        }
    };

    Ok(LiveInitAnswers {
        account_address,
        funder_address,
        account_api_key,
        account_secret,
        account_passphrase,
        relayer_auth_kind,
        relayer_api_key,
        relayer_secret,
        relayer_passphrase,
        relayer_address,
    })
}

fn ask_optional<P: PromptIo>(prompt: &mut P, label: &str) -> Result<Option<String>, InitError> {
    prompt.println(label)?;
    let answer = prompt.read_line()?.trim().to_string();
    Ok((!answer.is_empty()).then_some(answer))
}

fn render_paper_config() -> String {
    summary::paper_config_lines().join("\n") + "\n"
}
