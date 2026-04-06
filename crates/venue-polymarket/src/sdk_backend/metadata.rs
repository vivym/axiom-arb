use async_trait::async_trait;
use polymarket_client_sdk::error::{Error as SdkError, Kind as SdkErrorKind, Status as SdkStatus};
use polymarket_client_sdk::gamma::types::request::EventsRequest;
use polymarket_client_sdk::gamma::Client as SdkGammaClient;

use crate::errors::{PolymarketGatewayError, PolymarketGatewayErrorKind};
use crate::metadata::{FlexibleStringList, GammaEvent, GammaMarket};

const NEG_RISK_PAGE_MAX_ATTEMPTS: usize = 2;

#[async_trait]
pub trait PolymarketMetadataApi: Send + Sync {
    async fn fetch_neg_risk_metadata_page(
        &self,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<GammaEvent>, PolymarketGatewayError>;
}

pub struct LiveMetadataSdkApi {
    client: SdkGammaClient,
}

impl LiveMetadataSdkApi {
    #[must_use]
    pub fn new(client: SdkGammaClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl PolymarketMetadataApi for LiveMetadataSdkApi {
    async fn fetch_neg_risk_metadata_page(
        &self,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<GammaEvent>, PolymarketGatewayError> {
        for attempt in 0..NEG_RISK_PAGE_MAX_ATTEMPTS {
            let request = EventsRequest::builder()
                .active(true)
                .closed(false)
                .limit(limit as i32)
                .offset(offset as i32)
                .build();

            match self.client.events(&request).await {
                Ok(events) => {
                    return Ok(events.into_iter().map(map_sdk_event).collect());
                }
                Err(error)
                    if attempt + 1 < NEG_RISK_PAGE_MAX_ATTEMPTS
                        && is_retryable_metadata_error(&error) =>
                {
                    continue;
                }
                Err(error) => return Err(map_sdk_error(error)),
            }
        }

        Err(PolymarketGatewayError::connectivity(
            "metadata retry loop exhausted without returning an error",
        ))
    }
}

fn map_sdk_event(event: polymarket_client_sdk::gamma::types::response::Event) -> GammaEvent {
    GammaEvent {
        id: Some(event.id),
        title: event.title,
        parent_event_id: event.parent_event_id.or(event.parent_event),
        neg_risk: event.neg_risk,
        enable_neg_risk: event.enable_neg_risk,
        neg_risk_augmented: event.neg_risk_augmented,
        markets: event
            .markets
            .unwrap_or_default()
            .into_iter()
            .map(map_sdk_market)
            .collect(),
    }
}

fn map_sdk_market(market: polymarket_client_sdk::gamma::types::response::Market) -> GammaMarket {
    GammaMarket {
        condition_id: market.condition_id.map(|value| value.to_string()),
        clob_token_ids: FlexibleStringList::Values(
            market
                .clob_token_ids
                .unwrap_or_default()
                .into_iter()
                .map(|value| value.to_string())
                .collect(),
        ),
        group_item_title: market.group_item_title,
        title: None,
        short_outcomes: market
            .short_outcomes
            .map(FlexibleStringList::Text)
            .unwrap_or_default(),
        outcomes: market
            .outcomes
            .map(FlexibleStringList::Values)
            .unwrap_or_default(),
        question: market.question,
        neg_risk: market.neg_risk,
        neg_risk_other: market.neg_risk_other,
    }
}

fn is_retryable_metadata_error(error: &SdkError) -> bool {
    match error.kind() {
        SdkErrorKind::Status => error.downcast_ref::<SdkStatus>().is_some_and(|status| {
            status.status_code.is_server_error() || status.status_code.as_u16() == 429
        }),
        _ => false,
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
        SdkErrorKind::Synchronization | SdkErrorKind::Internal => {
            PolymarketGatewayError::connectivity(error.to_string())
        }
        SdkErrorKind::Geoblock => PolymarketGatewayError::policy(error.to_string()),
        _ => PolymarketGatewayError::protocol(error.to_string()),
    }
}
