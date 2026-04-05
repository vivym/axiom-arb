use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct RawAxiomConfig {
    pub runtime: RuntimeToml,
    #[serde(default)]
    pub strategy_control: Option<StrategyControlToml>,
    #[serde(default)]
    pub strategies: Option<StrategiesToml>,
    #[serde(default)]
    pub polymarket: Option<PolymarketToml>,
    #[serde(default)]
    pub negrisk: Option<NegRiskToml>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct RuntimeToml {
    pub mode: RuntimeModeToml,
    #[serde(default)]
    pub real_user_shadow_smoke: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeModeToml {
    Paper,
    Live,
}

impl<'de> Deserialize<'de> for RuntimeModeToml {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "paper" => Ok(Self::Paper),
            "live" => Ok(Self::Live),
            _ => Err(de::Error::custom(format!(
                "runtime.mode must be one of: paper, live; got {value:?}"
            ))),
        }
    }
}

impl Serialize for RuntimeModeToml {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(match self {
            Self::Paper => "paper",
            Self::Live => "live",
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct PolymarketToml {
    #[serde(default)]
    pub account: Option<PolymarketAccountToml>,
    #[serde(default)]
    pub relayer_auth: Option<PolymarketRelayerAuthToml>,
    #[serde(default)]
    pub http: Option<PolymarketHttpToml>,
    #[serde(default)]
    pub source_overrides: Option<PolymarketSourceToml>,
    #[serde(default)]
    pub source: Option<PolymarketSourceToml>,
    #[serde(default)]
    pub signer: Option<PolymarketSignerToml>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct StrategyControlToml {
    pub source: StrategyControlSourceToml,
    #[serde(default)]
    pub operator_strategy_revision: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StrategyControlSourceToml {
    Adopted,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct StrategiesToml {
    #[serde(default)]
    pub full_set: Option<StrategyRouteToml>,
    #[serde(default)]
    pub neg_risk: Option<StrategyRouteToml>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct StrategyRouteToml {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub rollout: Option<StrategyRouteRolloutToml>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct StrategyRouteRolloutToml {
    #[serde(default)]
    pub approved_scopes: Vec<String>,
    #[serde(default)]
    pub ready_scopes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct PolymarketAccountToml {
    pub address: String,
    #[serde(default)]
    pub funder_address: Option<String>,
    pub signature_type: SignatureTypeToml,
    pub wallet_route: WalletRouteToml,
    pub api_key: String,
    pub secret: String,
    pub passphrase: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct PolymarketHttpToml {
    #[serde(default)]
    pub proxy_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct PolymarketSourceToml {
    pub clob_host: String,
    pub data_api_host: String,
    pub relayer_host: String,
    pub market_ws_url: String,
    pub user_ws_url: String,
    pub heartbeat_interval_seconds: u64,
    pub relayer_poll_interval_seconds: u64,
    pub metadata_refresh_interval_seconds: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct PolymarketSignerToml {
    pub address: String,
    pub funder_address: String,
    pub signature_type: SignatureTypeToml,
    pub wallet_route: WalletRouteToml,
    pub api_key: String,
    pub passphrase: String,
    pub timestamp: String,
    pub signature: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct PolymarketRelayerAuthToml {
    pub kind: RelayerAuthKindToml,
    pub api_key: String,
    #[serde(default)]
    pub secret: Option<String>,
    #[serde(default)]
    pub timestamp: Option<String>,
    #[serde(default)]
    pub passphrase: Option<String>,
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelayerAuthKindToml {
    BuilderApiKey,
    RelayerApiKey,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SignatureTypeToml {
    Eoa,
    Proxy,
    Safe,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WalletRouteToml {
    Eoa,
    Proxy,
    Safe,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct NegRiskToml {
    #[serde(default)]
    pub target_source: Option<NegRiskTargetSourceToml>,
    #[serde(default)]
    pub rollout: Option<NegRiskRolloutToml>,
    #[serde(default)]
    pub targets: Vec<NegRiskTargetToml>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct NegRiskTargetSourceToml {
    pub source: NegRiskTargetSourceKindToml,
    #[serde(default)]
    pub operator_target_revision: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NegRiskTargetSourceKindToml {
    Adopted,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct NegRiskRolloutToml {
    #[serde(default)]
    pub approved_families: Vec<String>,
    #[serde(default)]
    pub ready_families: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct NegRiskTargetToml {
    pub family_id: String,
    #[serde(default)]
    pub members: Vec<NegRiskTargetMemberToml>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct NegRiskTargetMemberToml {
    pub condition_id: String,
    pub token_id: String,
    pub price: String,
    pub quantity: String,
}
