use async_trait::async_trait;

use crate::auth::RelayerAuth;
use crate::errors::PolymarketGatewayError;
use crate::relayer::{RelayerTransaction, RelayerTransactionType};
use crate::rest::{PolymarketRestClient, RestError};

#[async_trait]
pub trait PolymarketRelayerApi: Send + Sync {
    async fn recent_transactions(
        &self,
        auth: &RelayerAuth<'_>,
    ) -> Result<Vec<RelayerTransaction>, PolymarketGatewayError>;

    async fn current_nonce(
        &self,
        auth: &RelayerAuth<'_>,
        address: &str,
        wallet_type: RelayerTransactionType,
    ) -> Result<String, PolymarketGatewayError>;
}

#[derive(Debug, Clone)]
pub struct LiveRelayerApi {
    client: PolymarketRestClient,
}

impl LiveRelayerApi {
    #[must_use]
    pub fn new(client: PolymarketRestClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl PolymarketRelayerApi for LiveRelayerApi {
    async fn recent_transactions(
        &self,
        auth: &RelayerAuth<'_>,
    ) -> Result<Vec<RelayerTransaction>, PolymarketGatewayError> {
        self.client
            .fetch_recent_transactions(auth)
            .await
            .map_err(map_rest_error)
    }

    async fn current_nonce(
        &self,
        auth: &RelayerAuth<'_>,
        address: &str,
        wallet_type: RelayerTransactionType,
    ) -> Result<String, PolymarketGatewayError> {
        self.client
            .fetch_current_nonce(auth, address, wallet_type)
            .await
            .map_err(map_rest_error)
    }
}

fn map_rest_error(error: RestError) -> PolymarketGatewayError {
    match error {
        RestError::Auth(error) => PolymarketGatewayError::auth(error.to_string()),
        RestError::Http(error) => PolymarketGatewayError::connectivity(error.to_string()),
        RestError::HttpResponse { status, body } => {
            PolymarketGatewayError::upstream_response(format!("{status}: {body}"))
        }
        RestError::Gateway(error) => error,
        RestError::Metadata(error) => PolymarketGatewayError::protocol(error.to_string()),
        RestError::Url(error) => PolymarketGatewayError::protocol(error.to_string()),
        RestError::MissingField(field) => {
            PolymarketGatewayError::relayer(format!("missing response field: {field}"))
        }
    }
}
