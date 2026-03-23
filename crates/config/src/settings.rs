use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub runtime: RuntimeSettings,
    pub db: DatabaseSettings,
    pub polymarket: PolymarketSettings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSettings {
    pub mode: RuntimeMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseSettings {
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolymarketSettings {
    pub clob_host: String,
    pub data_api_host: String,
    pub relayer_host: String,
    pub signature_type: SignatureType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeMode {
    Paper,
    Live,
}

impl RuntimeMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::Live => "live",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureType {
    Eoa,
    Proxy,
    Safe,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    MissingVar(&'static str),
    InvalidVar { key: &'static str, value: String },
}

const DEFAULT_POLY_CLOB_HOST: &str = "https://clob.polymarket.com";
const DEFAULT_POLY_DATA_API_HOST: &str = "https://data-api.polymarket.com";
const DEFAULT_POLY_RELAYER_HOST: &str = "https://relayer-v2.polymarket.com";
const DEFAULT_POLY_SIGNATURE_TYPE: SignatureType = SignatureType::Eoa;

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingVar(key) => write!(f, "missing required env var: {key}"),
            Self::InvalidVar { key, value } => {
                write!(f, "invalid value for {key}: {value}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

impl Settings {
    pub fn from_env_iter<I, K, V>(iter: I) -> Result<Self, ConfigError>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        let mut db_url = None;
        let mut mode = None;
        let mut clob_host = None;
        let mut data_api_host = None;
        let mut relayer_host = None;
        let mut signature_type = None;

        for (key, value) in iter {
            let key = key.as_ref();
            let value = value.as_ref().trim();
            match key {
                "DATABASE_URL" if !value.is_empty() => db_url = Some(value.to_owned()),
                "AXIOM_MODE" if !value.is_empty() => mode = Some(parse_runtime_mode(value)?),
                "POLY_CLOB_HOST" if !value.is_empty() => {
                    clob_host = Some(value.to_owned());
                }
                "POLY_DATA_API_HOST" if !value.is_empty() => {
                    data_api_host = Some(value.to_owned());
                }
                "POLY_RELAYER_HOST" if !value.is_empty() => {
                    relayer_host = Some(value.to_owned());
                }
                "POLY_SIGNATURE_TYPE" if !value.is_empty() => {
                    signature_type = Some(parse_signature_type(value)?);
                }
                _ => {}
            }
        }

        Ok(Self {
            runtime: RuntimeSettings {
                mode: mode.ok_or(ConfigError::MissingVar("AXIOM_MODE"))?,
            },
            db: DatabaseSettings {
                url: db_url.ok_or(ConfigError::MissingVar("DATABASE_URL"))?,
            },
            polymarket: PolymarketSettings {
                clob_host: clob_host.unwrap_or_else(|| DEFAULT_POLY_CLOB_HOST.to_owned()),
                data_api_host: data_api_host
                    .unwrap_or_else(|| DEFAULT_POLY_DATA_API_HOST.to_owned()),
                relayer_host: relayer_host.unwrap_or_else(|| DEFAULT_POLY_RELAYER_HOST.to_owned()),
                signature_type: signature_type.unwrap_or(DEFAULT_POLY_SIGNATURE_TYPE),
            },
        })
    }
}

fn parse_runtime_mode(value: &str) -> Result<RuntimeMode, ConfigError> {
    match value {
        "paper" => Ok(RuntimeMode::Paper),
        "live" => Ok(RuntimeMode::Live),
        _ => Err(ConfigError::InvalidVar {
            key: "AXIOM_MODE",
            value: value.to_owned(),
        }),
    }
}

fn parse_signature_type(value: &str) -> Result<SignatureType, ConfigError> {
    match value {
        "EOA" => Ok(SignatureType::Eoa),
        "PROXY" => Ok(SignatureType::Proxy),
        "SAFE" => Ok(SignatureType::Safe),
        _ => Err(ConfigError::InvalidVar {
            key: "POLY_SIGNATURE_TYPE",
            value: value.to_owned(),
        }),
    }
}
