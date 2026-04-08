use config_schema::{
    render_raw_config_to_string, NegRiskRolloutToml, NegRiskTargetSourceKindToml,
    NegRiskTargetSourceToml, NegRiskToml, PolymarketAccountToml, PolymarketRelayerAuthToml,
    PolymarketToml, RawAxiomConfig, RelayerAuthKindToml, RuntimeModeToml, RuntimeToml,
    SignatureTypeToml, WalletRouteToml,
};

use super::InitError;

pub struct LiveInitAnswers {
    pub account_address: String,
    pub funder_address: Option<String>,
    pub account_api_key: String,
    pub account_secret: String,
    pub account_passphrase: String,
    pub relayer_auth_kind: RelayerAuthKindToml,
    pub relayer_api_key: String,
    pub relayer_secret: Option<String>,
    pub relayer_passphrase: Option<String>,
    pub relayer_address: Option<String>,
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
                signature_type: SignatureTypeToml::Eoa,
                wallet_route: WalletRouteToml::Eoa,
                api_key: answers.account_api_key,
                secret: answers.account_secret,
                passphrase: answers.account_passphrase,
            }),
            relayer_auth: Some(PolymarketRelayerAuthToml {
                kind: answers.relayer_auth_kind,
                api_key: answers.relayer_api_key,
                secret: answers.relayer_secret,
                timestamp: None,
                passphrase: answers.relayer_passphrase,
                signature: None,
                address: answers.relayer_address,
            }),
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

        if let Some(existing_negrisk) = &existing_config.negrisk {
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
            account.signature_type = existing_account.signature_type;
            account.wallet_route = existing_account.wallet_route;
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
