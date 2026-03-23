use serde::Deserialize;

use crate::{PolymarketRestClient, RestError};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RelayerTransaction {
    #[serde(alias = "id")]
    pub transaction_id: String,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub nonce: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
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

impl PolymarketRestClient {
    pub async fn fetch_recent_transactions(
        &self,
        owner: &str,
    ) -> Result<Vec<RelayerTransaction>, RestError> {
        let response: RecentTransactionsResponse = self
            .get_relayer("transactions", &[("owner", owner)])
            .await?;

        Ok(match response {
            RecentTransactionsResponse::List(items) => items,
            RecentTransactionsResponse::Envelope { transactions } => transactions,
        })
    }

    pub async fn fetch_current_nonce(&self, owner: &str) -> Result<String, RestError> {
        let response: CurrentNonceResponse = self.get_relayer("nonce", &[("owner", owner)]).await?;

        response
            .current_nonce
            .or(response.nonce)
            .ok_or(RestError::MissingField("nonce"))
    }
}
