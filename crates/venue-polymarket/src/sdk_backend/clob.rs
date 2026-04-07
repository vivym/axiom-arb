use async_trait::async_trait;
use polymarket_client_sdk::auth::state::Authenticated;
use polymarket_client_sdk::auth::Uuid as ApiKey;
use polymarket_client_sdk::auth::{Normal, Uuid};
use polymarket_client_sdk::clob::types::request::OrdersRequest;
use polymarket_client_sdk::clob::types::{
    Order as SdkOrder, OrderType as SdkOrderType, SignedOrder as SdkSignedOrder,
};
use polymarket_client_sdk::clob::Client as SdkClobClient;
use polymarket_client_sdk::error::{Error as SdkError, Kind as SdkErrorKind, Status as SdkStatus};
use polymarket_client_sdk::types::Signature;
use std::str::FromStr;

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
        order: &PolymarketSignedOrder,
    ) -> Result<PolymarketSubmitResponse, PolymarketGatewayError> {
        let signed_order = sdk_signed_order_from_dto(order)?;
        let response = self
            .client
            .post_order(signed_order)
            .await
            .map_err(map_sdk_error)?;

        Ok(PolymarketSubmitResponse {
            order_id: response.order_id,
            status: response.status.to_string(),
            success: response.success,
            error_message: response.error_msg,
            transaction_hashes: response
                .transaction_hashes
                .into_iter()
                .map(|hash| hash.to_string())
                .collect(),
        })
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

fn sdk_signed_order_from_dto(
    order: &PolymarketSignedOrder,
) -> Result<SdkSignedOrder, PolymarketGatewayError> {
    let payload = order.order.as_object().ok_or_else(|| {
        PolymarketGatewayError::protocol("signed order payload must be a JSON object")
    })?;

    let signature = Signature::from_str(
        payload
            .get("signature")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                PolymarketGatewayError::protocol(
                    "signed order payload is missing its embedded signature",
                )
            })?,
    )
    .map_err(|err| {
        PolymarketGatewayError::protocol(format!("invalid signed order signature: {err}"))
    })?;
    let owner = ApiKey::parse_str(&order.owner).map_err(|err| {
        PolymarketGatewayError::protocol(format!("invalid signed order owner: {err}"))
    })?;
    let order_type = match order.order_type.as_str() {
        "GTC" | "gtc" => SdkOrderType::GTC,
        "FOK" | "fok" => SdkOrderType::FOK,
        "GTD" | "gtd" => SdkOrderType::GTD,
        "FAK" | "fak" => SdkOrderType::FAK,
        other => SdkOrderType::Unknown(other.to_owned()),
    };

    let mut sdk_order = SdkOrder::default();
    sdk_order.salt =
        serde_json::from_value(payload.get("salt").cloned().ok_or_else(|| {
            PolymarketGatewayError::protocol("signed order payload is missing salt")
        })?)
        .map_err(|err| {
            PolymarketGatewayError::protocol(format!("invalid signed order salt: {err}"))
        })?;
    sdk_order.maker = serde_json::from_value(payload.get("maker").cloned().ok_or_else(|| {
        PolymarketGatewayError::protocol("signed order payload is missing maker")
    })?)
    .map_err(|err| {
        PolymarketGatewayError::protocol(format!("invalid signed order maker: {err}"))
    })?;
    sdk_order.signer = serde_json::from_value(payload.get("signer").cloned().ok_or_else(|| {
        PolymarketGatewayError::protocol("signed order payload is missing signer")
    })?)
    .map_err(|err| {
        PolymarketGatewayError::protocol(format!("invalid signed order signer: {err}"))
    })?;
    sdk_order.taker = serde_json::from_value(payload.get("taker").cloned().ok_or_else(|| {
        PolymarketGatewayError::protocol("signed order payload is missing taker")
    })?)
    .map_err(|err| {
        PolymarketGatewayError::protocol(format!("invalid signed order taker: {err}"))
    })?;
    sdk_order.tokenId =
        serde_json::from_value(payload.get("tokenId").cloned().ok_or_else(|| {
            PolymarketGatewayError::protocol("signed order payload is missing tokenId")
        })?)
        .map_err(|err| {
            PolymarketGatewayError::protocol(format!("invalid signed order tokenId: {err}"))
        })?;
    sdk_order.makerAmount =
        serde_json::from_value(payload.get("makerAmount").cloned().ok_or_else(|| {
            PolymarketGatewayError::protocol("signed order payload is missing makerAmount")
        })?)
        .map_err(|err| {
            PolymarketGatewayError::protocol(format!("invalid signed order makerAmount: {err}"))
        })?;
    sdk_order.takerAmount =
        serde_json::from_value(payload.get("takerAmount").cloned().ok_or_else(|| {
            PolymarketGatewayError::protocol("signed order payload is missing takerAmount")
        })?)
        .map_err(|err| {
            PolymarketGatewayError::protocol(format!("invalid signed order takerAmount: {err}"))
        })?;
    sdk_order.expiration =
        serde_json::from_value(payload.get("expiration").cloned().ok_or_else(|| {
            PolymarketGatewayError::protocol("signed order payload is missing expiration")
        })?)
        .map_err(|err| {
            PolymarketGatewayError::protocol(format!("invalid signed order expiration: {err}"))
        })?;
    sdk_order.nonce = serde_json::from_value(payload.get("nonce").cloned().ok_or_else(|| {
        PolymarketGatewayError::protocol("signed order payload is missing nonce")
    })?)
    .map_err(|err| {
        PolymarketGatewayError::protocol(format!("invalid signed order nonce: {err}"))
    })?;
    sdk_order.feeRateBps =
        serde_json::from_value(payload.get("feeRateBps").cloned().ok_or_else(|| {
            PolymarketGatewayError::protocol("signed order payload is missing feeRateBps")
        })?)
        .map_err(|err| {
            PolymarketGatewayError::protocol(format!("invalid signed order feeRateBps: {err}"))
        })?;
    let side =
        serde_json::from_value::<String>(payload.get("side").cloned().ok_or_else(|| {
            PolymarketGatewayError::protocol("signed order payload is missing side")
        })?)
        .map_err(|err| {
            PolymarketGatewayError::protocol(format!("invalid signed order side: {err}"))
        })?;
    sdk_order.side = match side.as_str() {
        "BUY" | "buy" => 0,
        "SELL" | "sell" => 1,
        other => {
            return Err(PolymarketGatewayError::protocol(format!(
                "invalid signed order side: {other}"
            )))
        }
    };
    sdk_order.signatureType =
        serde_json::from_value(payload.get("signatureType").cloned().ok_or_else(|| {
            PolymarketGatewayError::protocol("signed order payload is missing signatureType")
        })?)
        .map_err(|err| {
            PolymarketGatewayError::protocol(format!("invalid signed order signatureType: {err}"))
        })?;

    Ok(SdkSignedOrder::builder()
        .order(sdk_order)
        .signature(signature)
        .order_type(order_type)
        .owner(owner)
        .post_only(order.defer_exec)
        .build())
}
