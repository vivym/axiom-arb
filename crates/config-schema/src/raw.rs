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
