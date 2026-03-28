use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RawAxiomConfig {
    pub runtime: RuntimeToml,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RuntimeToml {
    pub mode: RuntimeModeToml,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeModeToml {
    Paper,
    Live,
}

impl RuntimeModeToml {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::Live => "live",
        }
    }
}
