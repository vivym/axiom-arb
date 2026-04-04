use std::{collections::BTreeSet, error::Error, fmt, path::Path};

use config_schema::{load_raw_config_from_path, ValidatedConfig};
use persistence::connect_pool_from_env;

use crate::cli::ApplyArgs;
use crate::commands::{
    status::{self, evaluate::StatusOutcome, model::StatusRolloutState},
    targets::{
        adopt, config_file::rewrite_smoke_rollout_families, state::load_target_candidates_catalog,
    },
};
use crate::startup;

pub mod model;
mod prompt;

use self::model::{ApplyFailureKind, ApplyScenario, ApplyUnsupportedScenario};
use self::prompt::{ApplyPrompt, InlineSmokeRolloutSelection, InlineTargetAdoptionSelection};

enum InlineTargetAdoptionOutcome {
    Adopted,
    Cancelled,
    Unavailable,
}

enum InlineSmokeRolloutOutcome {
    Enabled,
    Declined,
}

#[derive(Debug)]
struct ApplyError {
    kind: ApplyFailureKind,
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind.guidance())
    }
}

impl Error for ApplyError {}

pub fn execute(args: ApplyArgs) -> Result<(), Box<dyn Error>> {
    let raw = match load_raw_config_from_path(&args.config) {
        Ok(raw) => raw,
        Err(error) => {
            return Err(apply_failure(ApplyFailureKind::ReadinessError(
                error.to_string(),
            )))
        }
    };
    let validated = match ValidatedConfig::new(raw) {
        Ok(validated) => validated,
        Err(error) => {
            return Err(apply_failure(ApplyFailureKind::ReadinessError(
                error.to_string(),
            )))
        }
    };
    let config = match validated.for_app_live() {
        Ok(config) => config,
        Err(error) => {
            return Err(apply_failure(ApplyFailureKind::ReadinessError(
                error.to_string(),
            )))
        }
    };
    let scenario = ApplyScenario::from_config(&config);

    match scenario {
        ApplyScenario::Paper => Err(apply_failure(ApplyFailureKind::UnsupportedScenario(
            ApplyUnsupportedScenario::Paper,
        ))),
        ApplyScenario::Live => Err(apply_failure(ApplyFailureKind::UnsupportedScenario(
            ApplyUnsupportedScenario::Live,
        ))),
        ApplyScenario::Smoke => execute_smoke_apply(&args.config),
    }
}

fn execute_smoke_apply(config_path: &Path) -> Result<(), Box<dyn Error>> {
    let mut prompt = ApplyPrompt::new();

    loop {
        match status::evaluate::evaluate(config_path) {
            StatusOutcome::Summary(summary) => {
                let reason = summary
                    .details
                    .reason
                    .unwrap_or_else(|| "blocked".to_owned());
                let failure = if summary.readiness
                    == status::model::StatusReadiness::RestartRequired
                    && summary.details.rollout_state == Some(StatusRolloutState::Required)
                {
                    ApplyFailureKind::Transition(model::ApplyStage::EnsureSmokeRollout)
                } else {
                    ApplyFailureKind::from_status_readiness(summary.readiness, reason)
                };

                if failure == ApplyFailureKind::Transition(model::ApplyStage::EnsureTargetAnchor) {
                    if !prompt::stdin_is_interactive() {
                        return Err(apply_failure(ApplyFailureKind::Transition(
                            model::ApplyStage::EnsureTargetAnchor,
                        )));
                    }

                    match inline_target_adoption(config_path, &mut prompt) {
                        Ok(InlineTargetAdoptionOutcome::Adopted) => continue,
                        Ok(InlineTargetAdoptionOutcome::Cancelled) => {
                            return Err(apply_failure(ApplyFailureKind::ReadinessError(
                                "inline target adoption cancelled".to_owned(),
                            )))
                        }
                        Ok(InlineTargetAdoptionOutcome::Unavailable) => {
                            return Err(apply_failure(ApplyFailureKind::Transition(
                                model::ApplyStage::EnsureTargetAnchor,
                            )))
                        }
                        Err(error) => {
                            return Err(apply_failure(ApplyFailureKind::ReadinessError(
                                error.to_string(),
                            )))
                        }
                    }
                }

                if failure == ApplyFailureKind::Transition(model::ApplyStage::EnsureSmokeRollout) {
                    if !prompt::stdin_is_interactive() {
                        return Err(apply_failure(ApplyFailureKind::Transition(
                            model::ApplyStage::EnsureSmokeRollout,
                        )));
                    }

                    match inline_smoke_rollout_enablement(config_path, &mut prompt) {
                        Ok(InlineSmokeRolloutOutcome::Enabled) => continue,
                        Ok(InlineSmokeRolloutOutcome::Declined) => {
                            return Err(apply_failure(ApplyFailureKind::ReadinessError(
                                "inline smoke rollout enablement declined".to_owned(),
                            )))
                        }
                        Err(error) => {
                            return Err(apply_failure(ApplyFailureKind::ReadinessError(
                                error.to_string(),
                            )))
                        }
                    }
                }

                return Err(apply_failure(failure));
            }
            StatusOutcome::Deferred(deferred) => {
                return Err(apply_failure(ApplyFailureKind::ReadinessError(
                    deferred.reason,
                )))
            }
        }
    }
}

fn inline_target_adoption(
    config_path: &Path,
    prompt: &mut ApplyPrompt,
) -> Result<InlineTargetAdoptionOutcome, Box<dyn Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let (pool, catalog) = runtime.block_on(async {
        let pool = connect_pool_from_env().await?;
        let catalog = load_target_candidates_catalog(&pool).await?;
        Ok::<_, Box<dyn Error>>((pool, catalog))
    })?;

    let adoptable_revisions = catalog
        .adoptable_revisions
        .iter()
        .map(|adoptable| adoptable.adoptable_revision.clone())
        .collect::<Vec<_>>();
    if adoptable_revisions.is_empty() {
        return Ok(InlineTargetAdoptionOutcome::Unavailable);
    }

    let selection = prompt::choose_adoptable_revision(prompt, &adoptable_revisions)?;
    let adoptable_revision = match selection {
        InlineTargetAdoptionSelection::AdoptableRevision(revision) => revision,
        InlineTargetAdoptionSelection::Cancel => return Ok(InlineTargetAdoptionOutcome::Cancelled),
    };

    let summary = runtime.block_on(adopt::adopt_selected_revision(
        &pool,
        config_path,
        None,
        Some(adoptable_revision.as_str()),
    ))?;

    println!(
        "adopted adoptable revision {} as operator_target_revision {}",
        summary
            .selection
            .adoptable_revision
            .as_deref()
            .unwrap_or(adoptable_revision.as_str()),
        summary.selection.operator_target_revision
    );

    Ok(InlineTargetAdoptionOutcome::Adopted)
}

fn inline_smoke_rollout_enablement(
    config_path: &Path,
    prompt: &mut ApplyPrompt,
) -> Result<InlineSmokeRolloutOutcome, Box<dyn Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let family_ids = runtime.block_on(adopted_smoke_family_ids(config_path))?;
    let selection = prompt::choose_smoke_rollout_confirmation(prompt, &family_ids)?;

    match selection {
        InlineSmokeRolloutSelection::Confirm => {
            rewrite_smoke_rollout_families(config_path, &family_ids)?;
            Ok(InlineSmokeRolloutOutcome::Enabled)
        }
        InlineSmokeRolloutSelection::Decline => Ok(InlineSmokeRolloutOutcome::Declined),
    }
}

async fn adopted_smoke_family_ids(config_path: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let raw = load_raw_config_from_path(config_path)?;
    let validated = ValidatedConfig::new(raw)?;
    let config = validated.for_app_live()?;
    let pool = connect_pool_from_env().await?;
    let resolved = startup::resolve_startup_targets(&pool, &config).await?;
    let family_ids = resolved
        .targets
        .targets()
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if family_ids.is_empty() {
        return Err("apply could not derive any adopted smoke families".into());
    }
    Ok(family_ids)
}

fn apply_failure(kind: ApplyFailureKind) -> Box<dyn Error> {
    let error = ApplyError { kind };
    eprintln!("{error}");
    Box::new(error)
}
