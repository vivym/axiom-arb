use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::{
    build_relayer_auth_headers, PolymarketRestClient, RelayerAuth, RestError,
    VenueProducerInstrumentation,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum RelayerTransactionType {
    #[serde(rename = "PROXY")]
    Proxy,
    #[serde(rename = "SAFE")]
    Safe,
}

impl RelayerTransactionType {
    pub fn as_query_value(self) -> &'static str {
        match self {
            Self::Proxy => "PROXY",
            Self::Safe => "SAFE",
        }
    }
}

pub(crate) fn is_pending_relayer_state(state: Option<&str>) -> bool {
    matches!(state, Some("STATE_PENDING"))
}

pub(crate) fn summarize_recent_transactions(
    transactions: &[RelayerTransaction],
    observed_at: DateTime<Utc>,
) -> (usize, usize, f64) {
    let mut pending_tx_count = 0usize;
    let mut oldest_pending_age_seconds = 0.0f64;
    let mut has_pending_age = false;

    for transaction in transactions {
        if !is_pending_relayer_state(transaction.state.as_deref()) {
            continue;
        }

        pending_tx_count += 1;

        let Some(created_at) = transaction.created_at.as_deref().and_then(parse_created_at) else {
            continue;
        };

        let age_seconds = observed_at
            .signed_duration_since(created_at)
            .num_milliseconds() as f64
            / 1000.0;
        if age_seconds < 0.0 {
            continue;
        }

        if !has_pending_age || age_seconds > oldest_pending_age_seconds {
            oldest_pending_age_seconds = age_seconds;
            has_pending_age = true;
        }
    }

    (
        transactions.len(),
        pending_tx_count,
        if has_pending_age {
            oldest_pending_age_seconds
        } else {
            0.0
        },
    )
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RelayerTransaction {
    #[serde(alias = "transactionID", alias = "id")]
    pub transaction_id: String,
    #[serde(
        default,
        alias = "transactionHash",
        alias = "txHash",
        alias = "tx_hash"
    )]
    pub transaction_hash: Option<String>,
    #[serde(default, alias = "from", alias = "signer")]
    pub from_address: Option<String>,
    #[serde(default, alias = "to")]
    pub to_address: Option<String>,
    #[serde(
        default,
        alias = "proxyAddress",
        alias = "proxy_address",
        alias = "proxy"
    )]
    pub proxy_address: Option<String>,
    #[serde(default)]
    pub nonce: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default, rename = "type")]
    pub wallet_type: Option<RelayerTransactionType>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default, alias = "createdAt")]
    pub created_at: Option<String>,
    #[serde(default, alias = "updatedAt")]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub data: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub metadata: Option<String>,
}

impl RelayerTransaction {
    pub fn matches_pending_ref(&self, pending_ref: &str) -> bool {
        let pending_ref = pending_ref.trim();
        if pending_ref.is_empty() {
            return false;
        }

        self.transaction_id == pending_ref || self.transaction_hash.as_deref() == Some(pending_ref)
    }

    pub fn state_is_pending_or_unknown(&self) -> bool {
        matches!(
            classify_transaction_state(self.state.as_deref()),
            RelayerTransactionState::Pending | RelayerTransactionState::Unknown
        )
    }

    pub fn state_is_confirmed(&self) -> bool {
        matches!(
            classify_transaction_state(self.state.as_deref()),
            RelayerTransactionState::Confirmed
        )
    }

    pub fn state_is_terminal(&self) -> bool {
        matches!(
            classify_transaction_state(self.state.as_deref()),
            RelayerTransactionState::Terminal
        )
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RecentTransactionsResponse {
    List(Vec<RelayerTransaction>),
    Envelope {
        #[serde(default)]
        transactions: Vec<RelayerTransaction>,
    },
}

#[derive(Debug, Deserialize)]
struct CurrentNonceResponse {
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    current_nonce: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelayerTransactionState {
    Pending,
    Confirmed,
    Terminal,
    Unknown,
}

impl PolymarketRestClient {
    pub async fn fetch_recent_transactions(
        &self,
        auth: &RelayerAuth<'_>,
    ) -> Result<Vec<RelayerTransaction>, RestError> {
        if let Some(relayer_api) = &self.relayer_api {
            return relayer_api
                .recent_transactions(auth)
                .await
                .map_err(RestError::from);
        }
        let headers = build_relayer_auth_headers(auth)?;
        let request =
            self.build_get_request(&self.relayer_host, "transactions", &[], Some(headers))?;
        let response: RecentTransactionsResponse = self.execute_json(request).await?;
        Ok(recent_transactions_from_response(response))
    }

    pub async fn fetch_recent_transactions_instrumented(
        &self,
        auth: &RelayerAuth<'_>,
        instrumentation: &VenueProducerInstrumentation,
        observed_at: DateTime<Utc>,
    ) -> Result<Vec<RelayerTransaction>, RestError> {
        let transactions = self.fetch_recent_transactions(auth).await?;
        instrumentation.record_relayer_transactions(&transactions, observed_at);
        Ok(transactions)
    }

    pub async fn fetch_current_nonce(
        &self,
        auth: &RelayerAuth<'_>,
        address: &str,
        wallet_type: RelayerTransactionType,
    ) -> Result<String, RestError> {
        if let Some(relayer_api) = &self.relayer_api {
            return relayer_api
                .current_nonce(auth, address, wallet_type)
                .await
                .map_err(RestError::from);
        }
        let headers = build_relayer_auth_headers(auth)?;
        let query = [("address", address), ("type", wallet_type.as_query_value())];
        let request = self.build_get_request(&self.relayer_host, "nonce", &query, Some(headers))?;
        let response: CurrentNonceResponse = self.execute_json(request).await?;
        current_nonce_from_response(response)
    }
}

fn recent_transactions_from_response(
    response: RecentTransactionsResponse,
) -> Vec<RelayerTransaction> {
    match response {
        RecentTransactionsResponse::List(items) => items,
        RecentTransactionsResponse::Envelope { transactions } => transactions,
    }
}

fn current_nonce_from_response(response: CurrentNonceResponse) -> Result<String, RestError> {
    response
        .current_nonce
        .or(response.nonce)
        .ok_or(RestError::MissingField("nonce"))
}

fn classify_transaction_state(state: Option<&str>) -> RelayerTransactionState {
    match state.map(|value| value.trim().to_ascii_uppercase()) {
        Some(ref value)
            if matches!(
                value.as_str(),
                "STATE_NEW" | "STATE_EXECUTED" | "STATE_MINED"
            ) =>
        {
            RelayerTransactionState::Pending
        }
        Some(ref value) if value == "STATE_CONFIRMED" => RelayerTransactionState::Confirmed,
        Some(ref value) if matches!(value.as_str(), "STATE_INVALID" | "STATE_FAILED") => {
            RelayerTransactionState::Terminal
        }
        Some(_) | None => RelayerTransactionState::Unknown,
    }
}

fn parse_created_at(created_at: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(created_at)
        .ok()
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_transactions_response_shapes_documented_camel_case_fields() {
        let response: RecentTransactionsResponse = serde_json::from_str(
            r#"[{"transactionID":"tx-1","transactionHash":"0xhash","from":"0x1111111111111111111111111111111111111111","to":"0x2222222222222222222222222222222222222222","proxyAddress":"0x3333333333333333333333333333333333333333","nonce":"60","state":"STATE_CONFIRMED","type":"SAFE","owner":"0x4444444444444444444444444444444444444444","createdAt":"2024-07-14T21:13:08.819782Z","updatedAt":"2024-07-14T21:13:46.576639Z"}]"#,
        )
        .expect("response should deserialize");

        let transactions = recent_transactions_from_response(response);

        assert_eq!(transactions.len(), 1);
        assert_eq!(transactions[0].transaction_id, "tx-1");
        assert_eq!(transactions[0].transaction_hash.as_deref(), Some("0xhash"));
        assert_eq!(
            transactions[0].from_address.as_deref(),
            Some("0x1111111111111111111111111111111111111111")
        );
        assert_eq!(
            transactions[0].to_address.as_deref(),
            Some("0x2222222222222222222222222222222222222222")
        );
        assert_eq!(
            transactions[0].proxy_address.as_deref(),
            Some("0x3333333333333333333333333333333333333333")
        );
        assert_eq!(transactions[0].nonce.as_deref(), Some("60"));
        assert_eq!(transactions[0].state.as_deref(), Some("STATE_CONFIRMED"));
        assert_eq!(
            transactions[0].wallet_type,
            Some(RelayerTransactionType::Safe)
        );
        assert_eq!(
            transactions[0].created_at.as_deref(),
            Some("2024-07-14T21:13:08.819782Z")
        );
        assert_eq!(
            transactions[0].updated_at.as_deref(),
            Some("2024-07-14T21:13:46.576639Z")
        );
    }

    #[test]
    fn current_nonce_missing_field_is_explicit() {
        let response = CurrentNonceResponse {
            nonce: None,
            current_nonce: None,
        };

        let err = current_nonce_from_response(response).expect_err("missing nonce should fail");

        match err {
            RestError::MissingField("nonce") => {}
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
