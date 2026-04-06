use async_trait::async_trait;
use polymarket_client_sdk::auth::state::Authenticated;
use polymarket_client_sdk::auth::{Normal, Uuid};
use polymarket_client_sdk::clob::types::request::OrdersRequest;
use polymarket_client_sdk::clob::Client as SdkClobClient;
use polymarket_client_sdk::error::{Error as SdkError, Kind as SdkErrorKind, Status as SdkStatus};

use crate::errors::{PolymarketGatewayError, PolymarketGatewayErrorKind};
use crate::gateway::{
    PolymarketHeartbeatStatus, PolymarketOpenOrderSummary, PolymarketOrderQuery,
    PolymarketSignedOrder, PolymarketSubmitResponse,
};

#[async_trait]
pub trait PolymarketClobApi: Send + Sync {
    async fn open_orders(
        &self,
        query: &PolymarketOrderQuery,
    ) -> Result<Vec<PolymarketOpenOrderSummary>, PolymarketGatewayError>;

    async fn submit_order(
        &self,
        order: &PolymarketSignedOrder,
    ) -> Result<PolymarketSubmitResponse, PolymarketGatewayError>;

    async fn post_heartbeat(
        &self,
        previous_heartbeat_id: Option<&str>,
    ) -> Result<PolymarketHeartbeatStatus, PolymarketGatewayError>;
}

pub struct LiveClobSdkApi {
    client: SdkClobClient<Authenticated<Normal>>,
}

impl LiveClobSdkApi {
    #[must_use]
    pub fn new(client: SdkClobClient<Authenticated<Normal>>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl PolymarketClobApi for LiveClobSdkApi {
    async fn open_orders(
        &self,
        query: &PolymarketOrderQuery,
    ) -> Result<Vec<PolymarketOpenOrderSummary>, PolymarketGatewayError> {
        let request = match query {
            PolymarketOrderQuery::OpenOrders => OrdersRequest::default(),
        };
        let page = self
            .client
            .orders(&request, None)
            .await
            .map_err(map_sdk_error)?;

        Ok(page
            .data
            .into_iter()
            .map(|row| PolymarketOpenOrderSummary { order_id: row.id })
            .collect())
    }

    async fn submit_order(
        &self,
        _order: &PolymarketSignedOrder,
    ) -> Result<PolymarketSubmitResponse, PolymarketGatewayError> {
        Err(PolymarketGatewayError::protocol(
            "live SDK submit translation is not available from the current public polymarket-client-sdk surface",
        ))
    }

    async fn post_heartbeat(
        &self,
        previous_heartbeat_id: Option<&str>,
    ) -> Result<PolymarketHeartbeatStatus, PolymarketGatewayError> {
        let heartbeat_id = previous_heartbeat_id
            .map(Uuid::parse_str)
            .transpose()
            .map_err(|err| {
                PolymarketGatewayError::protocol(format!("invalid heartbeat id: {err}"))
            })?;
        let response = self
            .client
            .post_heartbeat(heartbeat_id)
            .await
            .map_err(map_sdk_error)?;

        Ok(PolymarketHeartbeatStatus {
            heartbeat_id: response.heartbeat_id.to_string(),
            valid: response.error.is_none(),
        })
    }
}

fn map_sdk_error(error: SdkError) -> PolymarketGatewayError {
    match error.kind() {
        SdkErrorKind::Status => {
            if let Some(status) = error.downcast_ref::<SdkStatus>() {
                PolymarketGatewayError::new(
                    PolymarketGatewayErrorKind::UpstreamResponse,
                    format!(
                        "{} {} returned {}: {}",
                        status.method, status.path, status.status_code, status.message
                    ),
                )
            } else {
                PolymarketGatewayError::upstream_response(error.to_string())
            }
        }
        SdkErrorKind::Validation => PolymarketGatewayError::protocol(error.to_string()),
        SdkErrorKind::Synchronization | SdkErrorKind::Internal => {
            PolymarketGatewayError::connectivity(error.to_string())
        }
        SdkErrorKind::WebSocket => PolymarketGatewayError::protocol(error.to_string()),
        SdkErrorKind::Geoblock => PolymarketGatewayError::policy(error.to_string()),
        _ => PolymarketGatewayError::protocol(error.to_string()),
    }
}
