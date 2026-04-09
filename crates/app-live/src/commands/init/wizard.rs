use std::path::Path;

use super::{
    prompt::{self, PromptIo},
    render::{self, LiveInitAnswers, LiveInitWalletKind},
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
            wallet_kind: LiveInitWalletKind::Eoa,
            config_path,
            has_existing_polymarket_source: false,
            has_existing_polymarket_source_overrides: false,
            configured_operator_strategy_revision: None,
            render_canonical_strategy_control: false,
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

pub(crate) fn smoke<P: PromptIo>(
    prompt: &mut P,
    config_path: &Path,
) -> Result<WizardResult, InitError> {
    render_live_or_smoke(prompt, config_path, true)
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
    let wallet_kind = answers.wallet_kind;
    let rendered_config =
        render::render_live_config(answers, real_user_shadow_smoke, existing_config.as_ref())?;
    Ok(WizardResult {
        rendered_config,
        summary: summary::render(build_summary_view(
            config_path,
            real_user_shadow_smoke,
            existing_config.as_ref(),
            wallet_kind,
        )),
    })
}

fn build_summary_view<'a>(
    config_path: &'a Path,
    real_user_shadow_smoke: bool,
    existing_config: Option<&'a RawAxiomConfig>,
    wallet_kind: LiveInitWalletKind,
) -> InitSummary<'a> {
    let strategy_control_shape = existing_config.map(render::existing_strategy_control_shape);
    let configured_operator_strategy_revision = strategy_control_shape
        .as_ref()
        .and_then(|shape| shape.configured_operator_strategy_revision.clone());
    let render_canonical_strategy_control = strategy_control_shape
        .as_ref()
        .is_some_and(|shape| shape.render_canonical_strategy_control);
    let rollout_is_empty = existing_config
        .map(existing_rollout_is_empty)
        .unwrap_or(true);
    let (has_existing_polymarket_source, has_existing_polymarket_source_overrides) =
        existing_config
            .and_then(|config| config.polymarket.as_ref())
            .map(|polymarket| {
                (
                    polymarket.source.is_some(),
                    polymarket.source_overrides.is_some(),
                )
            })
            .unwrap_or((false, false));

    InitSummary {
        mode: if real_user_shadow_smoke {
            WizardMode::Smoke
        } else {
            WizardMode::Live
        },
        wallet_kind,
        config_path,
        has_existing_polymarket_source,
        has_existing_polymarket_source_overrides,
        configured_operator_strategy_revision,
        render_canonical_strategy_control,
        rollout_is_empty,
    }
}

fn existing_rollout_is_empty(config: &RawAxiomConfig) -> bool {
    config
        .strategies
        .as_ref()
        .and_then(|strategies| strategies.neg_risk.as_ref())
        .and_then(|neg_risk| neg_risk.rollout.as_ref())
        .map(|rollout| rollout.approved_scopes.is_empty() && rollout.ready_scopes.is_empty())
        .or_else(|| {
            config
                .negrisk
                .as_ref()
                .and_then(|negrisk| negrisk.rollout.as_ref())
                .map(|rollout| {
                    rollout.approved_families.is_empty() && rollout.ready_families.is_empty()
                })
        })
        .unwrap_or(true)
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
    let wallet_kind =
        match prompt::select_one(prompt, "Choose a wallet kind:", &["eoa", "proxy", "safe"])? {
            0 => LiveInitWalletKind::Eoa,
            1 => LiveInitWalletKind::Proxy,
            2 => LiveInitWalletKind::Safe,
            _ => unreachable!("prompt selection should stay within the provided options"),
        };
    let account_address = prompt::ask_nonempty(prompt, "Account address:")?;
    let funder_address = ask_optional(prompt, "Funder address (optional, press Enter to skip):")?;
    let account_api_key = prompt::ask_nonempty(prompt, "Account API key:")?;
    let account_secret = prompt::ask_nonempty(prompt, "Account secret:")?;
    let account_passphrase = prompt::ask_nonempty(prompt, "Account passphrase:")?;
    let (relayer_auth_kind, relayer_api_key, relayer_secret, relayer_passphrase, relayer_address) =
        if wallet_kind.requires_relayer_auth() {
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

            (
                Some(relayer_auth_kind),
                Some(relayer_api_key),
                relayer_secret,
                relayer_passphrase,
                relayer_address,
            )
        } else {
            (None, None, None, None, None)
        };

    Ok(LiveInitAnswers {
        wallet_kind,
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
