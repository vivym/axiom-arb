use std::{collections::HashMap, fmt};

use domain::{MarketRoute, NegRiskVariant};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::rest::{PolymarketRestClient, RestError};

const NEG_RISK_PAGE_LIMIT: usize = 2;
const NEG_RISK_PAGE_MAX_ATTEMPTS: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegRiskMarketMetadata {
    pub event_family_id: String,
    pub event_id: String,
    pub condition_id: String,
    pub token_id: String,
    pub outcome_label: String,
    pub route: MarketRoute,
    pub enable_neg_risk: Option<bool>,
    pub neg_risk_augmented: Option<bool>,
    pub neg_risk_variant: NegRiskVariant,
    pub is_placeholder: bool,
    pub is_other: bool,
    pub discovery_revision: i64,
    pub metadata_snapshot_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NegRiskMetadataError {
    EmptyDiscovery,
    MissingEventId,
    MissingConditionId {
        event_id: String,
    },
    MissingTokenId {
        event_id: String,
        condition_id: String,
    },
    ConflictingDuplicateRow {
        event_family_id: String,
        condition_id: String,
        token_id: String,
        existing_outcome_label: String,
        incoming_outcome_label: String,
    },
}

impl fmt::Display for NegRiskMetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyDiscovery => write!(f, "neg-risk discovery returned no rows"),
            Self::MissingEventId => write!(f, "neg-risk event is missing an id"),
            Self::MissingConditionId { event_id } => {
                write!(f, "neg-risk market is missing a condition id for event {event_id}")
            }
            Self::MissingTokenId {
                event_id,
                condition_id,
            } => write!(
                f,
                "neg-risk market is missing a token id for event {event_id} condition {condition_id}"
            ),
            Self::ConflictingDuplicateRow {
                event_family_id,
                condition_id,
                token_id,
                existing_outcome_label,
                incoming_outcome_label,
            } => write!(
                f,
                "conflicting neg-risk metadata for family {event_family_id} condition {condition_id} token {token_id}: {existing_outcome_label:?} vs {incoming_outcome_label:?}"
            ),
        }
    }
}

impl std::error::Error for NegRiskMetadataError {}

impl From<NegRiskMetadataError> for RestError {
    fn from(value: NegRiskMetadataError) -> Self {
        Self::Metadata(value)
    }
}

#[derive(Debug, Default)]
pub(crate) struct NegRiskMetadataCache {
    current: Option<NegRiskDiscoverySnapshot>,
    history: Vec<NegRiskDiscoverySnapshot>,
}

#[derive(Debug, Clone)]
struct NegRiskDiscoverySnapshot {
    discovery_revision: i64,
    #[allow(dead_code)]
    metadata_snapshot_hash: String,
    rows: Vec<NegRiskMarketMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CanonicalNegRiskRow {
    event_family_id: String,
    event_id: String,
    condition_id: String,
    token_id: String,
    outcome_label: String,
    route: MarketRoute,
    enable_neg_risk: Option<bool>,
    neg_risk_augmented: Option<bool>,
    neg_risk_variant: NegRiskVariant,
    is_placeholder: bool,
    is_other: bool,
}

impl CanonicalNegRiskRow {
    fn into_public(
        self,
        discovery_revision: i64,
        metadata_snapshot_hash: String,
    ) -> NegRiskMarketMetadata {
        NegRiskMarketMetadata {
            event_family_id: self.event_family_id,
            event_id: self.event_id,
            condition_id: self.condition_id,
            token_id: self.token_id,
            outcome_label: self.outcome_label,
            route: self.route,
            enable_neg_risk: self.enable_neg_risk,
            neg_risk_augmented: self.neg_risk_augmented,
            neg_risk_variant: self.neg_risk_variant,
            is_placeholder: self.is_placeholder,
            is_other: self.is_other,
            discovery_revision,
            metadata_snapshot_hash,
        }
    }
}

#[derive(Debug, Deserialize)]
struct GammaEvent {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default, alias = "parentEvent", alias = "familyId")]
    parent_event_id: Option<String>,
    #[serde(default, alias = "negRisk")]
    neg_risk: Option<bool>,
    #[serde(default, alias = "enableNegRisk")]
    enable_neg_risk: Option<bool>,
    #[serde(default, alias = "negRiskAugmented")]
    neg_risk_augmented: Option<bool>,
    #[serde(default)]
    markets: Vec<GammaMarket>,
}

#[derive(Debug, Deserialize)]
struct GammaMarket {
    #[serde(default, alias = "conditionId")]
    condition_id: Option<String>,
    #[serde(default, alias = "clobTokenIds")]
    clob_token_ids: FlexibleStringList,
    #[serde(default, alias = "groupItemTitle")]
    group_item_title: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default, alias = "shortOutcomes")]
    short_outcomes: FlexibleStringList,
    #[serde(default, alias = "outcomes")]
    outcomes: FlexibleStringList,
    #[serde(default)]
    question: Option<String>,
    #[serde(default, alias = "negRisk")]
    neg_risk: Option<bool>,
    #[serde(default, alias = "negRiskOther")]
    neg_risk_other: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum FlexibleStringList {
    Values(Vec<String>),
    Text(String),
}

impl Default for FlexibleStringList {
    fn default() -> Self {
        Self::Values(Vec::new())
    }
}

impl FlexibleStringList {
    fn into_vec(self) -> Vec<String> {
        match self {
            Self::Values(values) => values,
            Self::Text(text) => parse_string_list(&text),
        }
    }
}

impl PolymarketRestClient {
    /// Attempts a refresh and falls back to the last successful snapshot on failure.
    /// Call `try_fetch_neg_risk_metadata_rows` when callers need the hard error instead.
    pub async fn fetch_neg_risk_metadata_rows(
        &self,
    ) -> Result<Vec<NegRiskMarketMetadata>, RestError> {
        match self.try_fetch_neg_risk_metadata_rows().await {
            Ok(rows) => Ok(rows),
            Err(err) => {
                let cache = self
                    .metadata_state
                    .lock()
                    .expect("neg-risk metadata cache poisoned");

                if let Some(snapshot) = &cache.current {
                    Ok(snapshot.rows.clone())
                } else {
                    Err(err)
                }
            }
        }
    }

    pub async fn try_fetch_neg_risk_metadata_rows(
        &self,
    ) -> Result<Vec<NegRiskMarketMetadata>, RestError> {
        let _refresh_guard = self.metadata_refresh_lock.lock().await;
        let discovery = self.discover_neg_risk_metadata_rows().await?;
        let mut cache = self
            .metadata_state
            .lock()
            .expect("neg-risk metadata cache poisoned");

        let NegRiskDiscovery {
            rows,
            metadata_snapshot_hash,
        } = discovery;
        let discovery_revision = cache
            .current
            .as_ref()
            .map_or(1, |snapshot| snapshot.discovery_revision + 1);
        let rows = rows
            .into_iter()
            .map(|row| row.into_public(discovery_revision, metadata_snapshot_hash.clone()))
            .collect::<Vec<_>>();
        let snapshot = NegRiskDiscoverySnapshot {
            discovery_revision,
            metadata_snapshot_hash,
            rows: rows.clone(),
        };

        if let Some(previous) = cache.current.replace(snapshot.clone()) {
            cache.history.push(previous);
        }

        Ok(snapshot.rows)
    }

    async fn discover_neg_risk_metadata_rows(&self) -> Result<NegRiskDiscovery, RestError> {
        let mut rows = Vec::<CanonicalNegRiskRow>::new();
        let mut seen = HashMap::<NegRiskMemberKey, usize>::new();
        let mut offset = 0usize;

        loop {
            let page = self.fetch_neg_risk_metadata_page(offset).await?;
            let page_count = page.len();

            for event in page {
                let event_id = event
                    .id
                    .clone()
                    .ok_or(NegRiskMetadataError::MissingEventId)?;
                let family_id = event
                    .parent_event_id
                    .clone()
                    .unwrap_or_else(|| event_id.clone());

                for market in event.markets {
                    let is_neg_risk =
                        event.neg_risk.unwrap_or(false) || market.neg_risk.unwrap_or(false);
                    if !is_neg_risk {
                        continue;
                    }

                    let condition_id = market.condition_id.clone().ok_or_else(|| {
                        NegRiskMetadataError::MissingConditionId {
                            event_id: event_id.clone(),
                        }
                    })?;
                    let token_id = market.yes_token_id().ok_or_else(|| {
                        NegRiskMetadataError::MissingTokenId {
                            event_id: event_id.clone(),
                            condition_id: condition_id.clone(),
                        }
                    })?;
                    let outcome_label = market.outcome_label(event.title.as_deref());
                    let is_other = market.neg_risk_other.unwrap_or(false)
                        || outcome_label.eq_ignore_ascii_case("other");
                    let is_placeholder = outcome_label.is_empty()
                        || outcome_label.eq_ignore_ascii_case("placeholder");
                    let neg_risk_variant = classify_variant(
                        is_neg_risk,
                        event.enable_neg_risk,
                        event.neg_risk_augmented,
                    );
                    let row = CanonicalNegRiskRow {
                        event_family_id: family_id.clone(),
                        event_id: event_id.clone(),
                        condition_id: condition_id.clone(),
                        token_id: token_id.clone(),
                        outcome_label,
                        route: MarketRoute::NegRisk,
                        enable_neg_risk: event.enable_neg_risk,
                        neg_risk_augmented: event.neg_risk_augmented,
                        neg_risk_variant,
                        is_placeholder,
                        is_other,
                    };
                    let key = NegRiskMemberKey {
                        event_family_id: row.event_family_id.clone(),
                        condition_id: row.condition_id.clone(),
                        token_id: row.token_id.clone(),
                    };

                    if let Some(existing_index) = seen.get(&key).copied() {
                        let existing = &rows[existing_index];
                        if existing != &row {
                            return Err(NegRiskMetadataError::ConflictingDuplicateRow {
                                event_family_id: row.event_family_id,
                                condition_id: row.condition_id,
                                token_id: row.token_id,
                                existing_outcome_label: existing.outcome_label.clone(),
                                incoming_outcome_label: row.outcome_label,
                            }
                            .into());
                        }
                        continue;
                    }

                    seen.insert(key, rows.len());
                    rows.push(row);
                }
            }

            if page_count < NEG_RISK_PAGE_LIMIT {
                break;
            }

            offset += NEG_RISK_PAGE_LIMIT;
        }

        canonicalize_rows(&mut rows);
        let metadata_snapshot_hash = snapshot_hash(&rows);
        Ok(NegRiskDiscovery {
            rows,
            metadata_snapshot_hash,
        })
    }

    async fn fetch_neg_risk_metadata_page(
        &self,
        offset: usize,
    ) -> Result<Vec<GammaEvent>, RestError> {
        let mut last_error = None;

        for attempt in 0..NEG_RISK_PAGE_MAX_ATTEMPTS {
            let limit = NEG_RISK_PAGE_LIMIT.to_string();
            let offset = offset.to_string();
            let query = [
                ("active", "true"),
                ("closed", "false"),
                ("limit", limit.as_str()),
                ("offset", offset.as_str()),
            ];
            let request = self.build_get_request(&self.data_api_host, "events", &query, None)?;

            match self.execute_json(request).await {
                Ok(page) => return Ok(page),
                Err(err)
                    if attempt + 1 < NEG_RISK_PAGE_MAX_ATTEMPTS
                        && is_retryable_metadata_error(&err) =>
                {
                    last_error = Some(err);
                    continue;
                }
                Err(err) => return Err(err),
            }
        }

        Err(last_error.expect("retry loop should always capture an error"))
    }
}

#[derive(Debug)]
struct NegRiskDiscovery {
    rows: Vec<CanonicalNegRiskRow>,
    metadata_snapshot_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct NegRiskMemberKey {
    event_family_id: String,
    condition_id: String,
    token_id: String,
}

fn classify_variant(
    is_neg_risk: bool,
    enable_neg_risk: Option<bool>,
    neg_risk_augmented: Option<bool>,
) -> NegRiskVariant {
    if !is_neg_risk {
        return NegRiskVariant::Unknown;
    }

    if enable_neg_risk == Some(true) && neg_risk_augmented == Some(true) {
        NegRiskVariant::Augmented
    } else {
        NegRiskVariant::Standard
    }
}

fn snapshot_hash(rows: &[CanonicalNegRiskRow]) -> String {
    let mut hasher = Sha256::new();

    for row in rows {
        update_hash_field(&mut hasher, &row.event_family_id);
        update_hash_field(&mut hasher, &row.event_id);
        update_hash_field(&mut hasher, &row.condition_id);
        update_hash_field(&mut hasher, &row.token_id);
        update_hash_field(&mut hasher, &row.outcome_label);
        update_hash_field(&mut hasher, market_route_label(row.route));
        update_hash_field(&mut hasher, optional_bool_label(row.enable_neg_risk));
        update_hash_field(&mut hasher, optional_bool_label(row.neg_risk_augmented));
        update_hash_field(&mut hasher, neg_risk_variant_label(row.neg_risk_variant));
        update_hash_field(&mut hasher, bool_label(row.is_placeholder));
        update_hash_field(&mut hasher, bool_label(row.is_other));
        hasher.update([b'\n']);
    }

    format!("sha256:{}", hex_digest(&hasher.finalize()))
}

fn canonicalize_rows(rows: &mut [CanonicalNegRiskRow]) {
    rows.sort_by(|left, right| {
        (
            left.event_id.as_str(),
            left.condition_id.as_str(),
            left.token_id.as_str(),
            left.outcome_label.as_str(),
        )
            .cmp(&(
                right.event_id.as_str(),
                right.condition_id.as_str(),
                right.token_id.as_str(),
                right.outcome_label.as_str(),
            ))
    });
}

fn update_hash_field(hasher: &mut Sha256, value: &str) {
    hasher.update(value.as_bytes());
    hasher.update([0x1f]);
}

fn market_route_label(route: MarketRoute) -> &'static str {
    match route {
        MarketRoute::Standard => "standard",
        MarketRoute::NegRisk => "negrisk",
    }
}

fn neg_risk_variant_label(variant: NegRiskVariant) -> &'static str {
    match variant {
        NegRiskVariant::Standard => "standard",
        NegRiskVariant::Augmented => "augmented",
        NegRiskVariant::Unknown => "unknown",
    }
}

fn optional_bool_label(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "null",
    }
}

fn bool_label(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);

    for byte in bytes {
        output.push(nibble_to_hex(byte >> 4));
        output.push(nibble_to_hex(byte & 0x0f));
    }

    output
}

fn nibble_to_hex(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'a' + (value - 10)) as char,
        _ => unreachable!("nibble must be 0..=15"),
    }
}

fn parse_string_list(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    if trimmed.starts_with('[') {
        if let Ok(values) = serde_json::from_str::<Vec<String>>(trimmed) {
            return values
                .into_iter()
                .map(|entry| entry.trim().to_owned())
                .filter(|entry| !entry.is_empty())
                .collect();
        }
    }

    trimmed
        .split(',')
        .map(|entry| entry.trim().trim_matches('"').to_owned())
        .filter(|entry| !entry.is_empty())
        .collect()
}

fn is_retryable_metadata_error(error: &RestError) -> bool {
    match error {
        RestError::Http(_) => true,
        RestError::HttpResponse { status, .. } => {
            matches!(status.as_u16(), 425 | 429 | 500 | 502 | 503 | 504)
        }
        RestError::Metadata(_)
        | RestError::Auth(_)
        | RestError::Url(_)
        | RestError::MissingField(_) => false,
    }
}

impl GammaMarket {
    // Gamma returns `clobTokenIds` as `[Yes token, No token]`. Neg-risk member
    // discovery anchors the member row to the named outcome's Yes token.
    fn yes_token_id(&self) -> Option<String> {
        self.clob_token_ids
            .clone()
            .into_vec()
            .into_iter()
            .next()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    }

    fn outcome_label(&self, event_title: Option<&str>) -> String {
        let mut candidates = Vec::new();
        candidates.extend(self.group_item_title.clone());
        candidates.extend(self.title.clone());
        if let Some(question) = self.question.clone() {
            candidates.push(question);
        }
        candidates.extend(
            self.short_outcomes
                .clone()
                .into_vec()
                .into_iter()
                .filter(|value| !is_binary_outcome_label(value)),
        );
        candidates.extend(
            self.outcomes
                .clone()
                .into_vec()
                .into_iter()
                .filter(|value| !is_binary_outcome_label(value)),
        );

        candidates
            .into_iter()
            .map(|entry| entry.trim().to_owned())
            .find(|entry| {
                !entry.is_empty()
                    && !is_binary_outcome_label(entry)
                    && event_title.is_none_or(|title| !entry.eq_ignore_ascii_case(title.trim()))
            })
            .unwrap_or_default()
    }
}

fn is_binary_outcome_label(value: &str) -> bool {
    matches!(value.trim().to_ascii_lowercase().as_str(), "yes" | "no")
}
