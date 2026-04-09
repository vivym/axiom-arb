use config_schema::{
    render_raw_config_to_string, NegRiskRolloutToml, NegRiskTargetSourceKindToml,
    NegRiskTargetSourceToml, NegRiskToml, PolymarketAccountToml, PolymarketRelayerAuthToml,
    PolymarketToml, RawAxiomConfig, RelayerAuthKindToml, RuntimeModeToml, RuntimeToml,
    SignatureTypeToml, StrategiesToml, StrategyControlSourceToml, StrategyControlToml,
    StrategyRouteRolloutToml, StrategyRouteToml, WalletRouteToml,
};

use super::InitError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveInitWalletKind {
    Eoa,
    Proxy,
    Safe,
}

impl LiveInitWalletKind {
    pub fn option_label(self) -> &'static str {
        match self {
            Self::Eoa => "eoa",
            Self::Proxy => "proxy",
            Self::Safe => "safe",
        }
    }

    pub fn signature_type(self) -> SignatureTypeToml {
        match self {
            Self::Eoa => SignatureTypeToml::Eoa,
            Self::Proxy => SignatureTypeToml::Proxy,
            Self::Safe => SignatureTypeToml::Safe,
        }
    }

    pub fn wallet_route(self) -> WalletRouteToml {
        match self {
            Self::Eoa => WalletRouteToml::Eoa,
            Self::Proxy => WalletRouteToml::Proxy,
            Self::Safe => WalletRouteToml::Safe,
        }
    }

    pub fn requires_relayer_auth(self) -> bool {
        !matches!(self, Self::Eoa)
    }
}

pub struct LiveInitAnswers {
    pub wallet_kind: LiveInitWalletKind,
    pub account_address: String,
    pub funder_address: Option<String>,
    pub account_api_key: String,
    pub account_secret: String,
    pub account_passphrase: String,
    pub relayer_auth_kind: Option<RelayerAuthKindToml>,
    pub relayer_api_key: Option<String>,
    pub relayer_secret: Option<String>,
    pub relayer_passphrase: Option<String>,
    pub relayer_address: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct ExistingStrategyControlShape {
    pub render_canonical_strategy_control: bool,
    pub configured_operator_strategy_revision: Option<String>,
}

pub(crate) fn existing_strategy_control_shape(
    existing_config: &RawAxiomConfig,
) -> ExistingStrategyControlShape {
    let configured_operator_strategy_revision = existing_strategy_revision(existing_config);
    ExistingStrategyControlShape {
        render_canonical_strategy_control: existing_config.strategy_control.is_some()
            || configured_operator_strategy_revision.is_some(),
        configured_operator_strategy_revision,
    }
}

pub fn render_live_config(
    answers: LiveInitAnswers,
    real_user_shadow_smoke: bool,
    existing_config: Option<&RawAxiomConfig>,
) -> Result<String, InitError> {
    let mut raw = RawAxiomConfig {
        runtime: RuntimeToml {
            mode: RuntimeModeToml::Live,
            real_user_shadow_smoke,
        },
        strategy_control: None,
        strategies: None,
        polymarket: Some(PolymarketToml {
            account: Some(PolymarketAccountToml {
                address: answers.account_address,
                funder_address: answers.funder_address,
                signature_type: answers.wallet_kind.signature_type(),
                wallet_route: answers.wallet_kind.wallet_route(),
                api_key: answers.account_api_key,
                secret: answers.account_secret,
                passphrase: answers.account_passphrase,
            }),
            relayer_auth: if answers.wallet_kind.requires_relayer_auth() {
                Some(PolymarketRelayerAuthToml {
                    kind: answers
                        .relayer_auth_kind
                        .expect("non-EOA init flow should collect relayer auth kind"),
                    api_key: answers
                        .relayer_api_key
                        .expect("non-EOA init flow should collect relayer API key"),
                    secret: answers.relayer_secret,
                    timestamp: None,
                    passphrase: answers.relayer_passphrase,
                    signature: None,
                    address: answers.relayer_address,
                })
            } else {
                None
            },
            source_overrides: None,
            source: None,
            signer: None,
        }),
        negrisk: Some(NegRiskToml {
            target_source: Some(NegRiskTargetSourceToml {
                source: NegRiskTargetSourceKindToml::Adopted,
                operator_target_revision: None,
            }),
            rollout: Some(NegRiskRolloutToml {
                approved_families: vec![],
                ready_families: vec![],
            }),
            targets: Default::default(),
        }),
    };

    if let Some(existing_config) = existing_config {
        merge_existing_polymarket(&mut raw, existing_config);

        merge_existing_control_plane(&mut raw, existing_config);
    }

    render_raw_config_to_string(&raw).map_err(|error| InitError::new(error.to_string()))
}

fn merge_existing_polymarket(raw: &mut RawAxiomConfig, existing_config: &RawAxiomConfig) {
    let Some(existing_polymarket) = &existing_config.polymarket else {
        return;
    };

    let Some(polymarket) = raw.polymarket.as_mut() else {
        return;
    };

    if let Some(existing_source_overrides) = &existing_polymarket.source_overrides {
        polymarket.source_overrides = Some(existing_source_overrides.clone());
    } else if let Some(existing_source) = &existing_polymarket.source {
        polymarket.source_overrides = Some(existing_source.clone());
    }

    if let Some(existing_account) = &existing_polymarket.account {
        if let Some(account) = polymarket.account.as_mut() {
            if account.funder_address.is_none() {
                account.funder_address = existing_account.funder_address.clone();
            }
        }
    }

    let Some(existing_relayer_auth) = &existing_polymarket.relayer_auth else {
        return;
    };

    let Some(relayer_auth) = polymarket.relayer_auth.as_mut() else {
        return;
    };

    if relayer_auth.kind != existing_relayer_auth.kind {
        return;
    }

    if relayer_auth.secret.is_none() {
        relayer_auth.secret = existing_relayer_auth.secret.clone();
    }
    if relayer_auth.timestamp.is_none() {
        relayer_auth.timestamp = existing_relayer_auth.timestamp.clone();
    }
    if relayer_auth.passphrase.is_none() {
        relayer_auth.passphrase = existing_relayer_auth.passphrase.clone();
    }
    if relayer_auth.signature.is_none() {
        relayer_auth.signature = existing_relayer_auth.signature.clone();
    }
    if relayer_auth.address.is_none() {
        relayer_auth.address = existing_relayer_auth.address.clone();
    }
}

fn merge_existing_control_plane(raw: &mut RawAxiomConfig, existing_config: &RawAxiomConfig) {
    let existing_full_set = existing_config
        .strategies
        .as_ref()
        .and_then(|strategies| strategies.full_set.clone());
    let Some(existing_negrisk) = &existing_config.negrisk else {
        if let Some(existing_strategy_control) = existing_config.strategy_control.as_ref() {
            raw.strategy_control = Some(existing_strategy_control.clone());
        }
        if let Some(existing_neg_risk) = existing_config
            .strategies
            .as_ref()
            .and_then(|strategies| strategies.neg_risk.as_ref())
        {
            raw.strategies = Some(StrategiesToml {
                full_set: existing_full_set,
                neg_risk: Some(existing_neg_risk.clone()),
            });
        }
        return;
    };

    let existing_strategy_shape = existing_strategy_control_shape(existing_config);

    if existing_strategy_shape.render_canonical_strategy_control {
        raw.strategy_control = Some(StrategyControlToml {
            source: StrategyControlSourceToml::Adopted,
            operator_strategy_revision: existing_strategy_shape
                .configured_operator_strategy_revision
                .clone(),
        });
        if let Some(negrisk) = raw.negrisk.as_mut() {
            negrisk.target_source = None;
            negrisk.rollout = None;
        }

        if let Some(existing_neg_risk) = existing_config
            .strategies
            .as_ref()
            .and_then(|strategies| strategies.neg_risk.as_ref())
        {
            raw.strategies = Some(StrategiesToml {
                full_set: existing_full_set,
                neg_risk: Some(existing_neg_risk.clone()),
            });
            return;
        }

        if let Some(existing_rollout) = &existing_negrisk.rollout {
            raw.strategies = Some(StrategiesToml {
                full_set: existing_full_set,
                neg_risk: Some(StrategyRouteToml {
                    enabled: true,
                    rollout: Some(StrategyRouteRolloutToml {
                        approved_scopes: existing_rollout.approved_families.clone(),
                        ready_scopes: existing_rollout.ready_families.clone(),
                    }),
                }),
            });
        }
        return;
    }

    if let (Some(existing_target_source), Some(target_source)) = (
        &existing_negrisk.target_source,
        raw.negrisk
            .as_mut()
            .and_then(|negrisk| negrisk.target_source.as_mut()),
    ) {
        target_source.operator_target_revision =
            existing_target_source.operator_target_revision.clone();
    }

    if let Some(existing_rollout) = &existing_negrisk.rollout {
        if let Some(negrisk) = raw.negrisk.as_mut() {
            negrisk.rollout = Some(existing_rollout.clone());
        }
    }
}

fn existing_strategy_revision(existing_config: &RawAxiomConfig) -> Option<String> {
    existing_config
        .strategy_control
        .as_ref()
        .and_then(|strategy_control| strategy_control.operator_strategy_revision.clone())
        .or_else(|| {
            existing_config
                .negrisk
                .as_ref()
                .and_then(|negrisk| negrisk.target_source.as_ref())
                .and_then(|target_source| {
                    target_source
                        .operator_target_revision
                        .as_deref()
                        .and_then(canonical_strategy_revision_from_legacy_target_revision)
                })
        })
}

fn canonical_strategy_revision_from_legacy_target_revision(revision: &str) -> Option<String> {
    if let Some(suffix) = revision.strip_prefix("targets-rev-") {
        return Some(format!("strategy-rev-{suffix}"));
    }

    revision
        .strip_prefix("sha256:")
        .map(|suffix| format!("strategy-rev-{suffix}"))
}
