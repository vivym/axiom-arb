use serde::Deserialize;

use crate::{PolymarketRestClient, RestError};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RelayerQuery<'a> {
    pub owner: Option<&'a str>,
    pub signer: Option<&'a str>,
    pub proxy_address: Option<&'a str>,
    pub safe_address: Option<&'a str>,
    pub nonce: Option<&'a str>,
    pub transaction_id: Option<&'a str>,
    pub tx_hash: Option<&'a str>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RelayerTransaction {
    #[serde(alias = "id")]
    pub transaction_id: String,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub nonce: Option<String>,
    #[serde(default)]
    pub signer: Option<String>,
    #[serde(
        default,
        alias = "proxyAddress",
        alias = "proxy_address",
        alias = "proxy"
    )]
    pub proxy_address: Option<String>,
    #[serde(default, alias = "safeAddress", alias = "safe_address", alias = "safe")]
    pub safe_address: Option<String>,
    #[serde(default, alias = "txHash", alias = "tx_hash")]
    pub tx_hash: Option<String>,
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
        self.fetch_recent_transactions_with_query(&RelayerQuery {
            owner: Some(owner),
            ..RelayerQuery::default()
        })
        .await
    }

    pub async fn fetch_recent_transactions_with_query(
        &self,
        query: &RelayerQuery<'_>,
    ) -> Result<Vec<RelayerTransaction>, RestError> {
        let pairs = query.as_pairs();
        let response: RecentTransactionsResponse = self.get_relayer("transactions", &pairs).await?;

        Ok(recent_transactions_from_response(response))
    }

    pub async fn fetch_current_nonce(&self, owner: &str) -> Result<String, RestError> {
        self.fetch_current_nonce_with_query(&RelayerQuery {
            owner: Some(owner),
            ..RelayerQuery::default()
        })
        .await
    }

    pub async fn fetch_current_nonce_with_query(
        &self,
        query: &RelayerQuery<'_>,
    ) -> Result<String, RestError> {
        let pairs = query.as_pairs();
        let response: CurrentNonceResponse = self.get_relayer("nonce", &pairs).await?;

        current_nonce_from_response(response)
    }
}

impl<'a> RelayerQuery<'a> {
    fn as_pairs(&self) -> Vec<(&'static str, &'a str)> {
        let mut pairs = Vec::new();

        push_pair(&mut pairs, "owner", self.owner);
        push_pair(&mut pairs, "signer", self.signer);
        push_pair(&mut pairs, "proxyAddress", self.proxy_address);
        push_pair(&mut pairs, "safeAddress", self.safe_address);
        push_pair(&mut pairs, "nonce", self.nonce);
        push_pair(&mut pairs, "id", self.transaction_id);
        push_pair(&mut pairs, "txHash", self.tx_hash);

        pairs
    }
}

fn push_pair<'a>(
    pairs: &mut Vec<(&'static str, &'a str)>,
    key: &'static str,
    value: Option<&'a str>,
) {
    if let Some(value) = value {
        pairs.push((key, value));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relayer_query_pairs_include_correlation_fields() {
        let query = RelayerQuery {
            owner: Some("0xowner"),
            signer: Some("0xsigner"),
            proxy_address: Some("0xproxy"),
            safe_address: Some("0xsafe"),
            nonce: Some("7"),
            transaction_id: Some("tx-1"),
            tx_hash: Some("0xhash"),
        };

        let pairs = query.as_pairs();

        assert!(pairs.contains(&("owner", "0xowner")));
        assert!(pairs.contains(&("signer", "0xsigner")));
        assert!(pairs.contains(&("proxyAddress", "0xproxy")));
        assert!(pairs.contains(&("safeAddress", "0xsafe")));
        assert!(pairs.contains(&("nonce", "7")));
        assert!(pairs.contains(&("id", "tx-1")));
        assert!(pairs.contains(&("txHash", "0xhash")));
    }

    #[test]
    fn recent_transactions_response_shapes_correlation_fields() {
        let response: RecentTransactionsResponse = serde_json::from_str(
            r#"{"transactions":[{"id":"tx-1","owner":"0xowner","nonce":"7","signer":"0xsigner","proxyAddress":"0xproxy","safeAddress":"0xsafe","txHash":"0xhash","status":"pending"}]}"#,
        )
        .expect("response should deserialize");

        let transactions = recent_transactions_from_response(response);

        assert_eq!(transactions.len(), 1);
        assert_eq!(transactions[0].transaction_id, "tx-1");
        assert_eq!(transactions[0].owner.as_deref(), Some("0xowner"));
        assert_eq!(transactions[0].nonce.as_deref(), Some("7"));
        assert_eq!(transactions[0].signer.as_deref(), Some("0xsigner"));
        assert_eq!(transactions[0].proxy_address.as_deref(), Some("0xproxy"));
        assert_eq!(transactions[0].safe_address.as_deref(), Some("0xsafe"));
        assert_eq!(transactions[0].tx_hash.as_deref(), Some("0xhash"));
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
