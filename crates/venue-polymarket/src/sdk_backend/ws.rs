use async_trait::async_trait;
use futures_util::StreamExt;
use polymarket_client_sdk::auth::{Credentials, Uuid};
use polymarket_client_sdk::clob::ws::types::response::{
    LastTradePrice as SdkLastTradePrice, OrderMessage as SdkOrderMessage,
    TradeMessage as SdkTradeMessage, WsMessage as SdkWsMessage,
};
use polymarket_client_sdk::clob::ws::Client as SdkWsClient;
use polymarket_client_sdk::types::{Address, B256, U256};
use polymarket_client_sdk::ws::config::Config as SdkWsConfig;

use crate::gateway::PolymarketUserStreamAuth;
use crate::{
    MarketTradePriceUpdate, MarketWsEvent, PolymarketGatewayError, UserOrderUpdate,
    UserTradeUpdate, UserWsEvent,
};

#[async_trait]
pub trait PolymarketStreamApi: Send + Sync {
    async fn market_events(
        &self,
        token_ids: &[String],
    ) -> Result<Vec<MarketWsEvent>, PolymarketGatewayError>;

    async fn user_events(
        &self,
        auth: &PolymarketUserStreamAuth,
        condition_ids: &[String],
    ) -> Result<Vec<UserWsEvent>, PolymarketGatewayError>;
}

#[derive(Debug, Clone)]
pub struct LiveWsSdkApi {
    base_endpoint: String,
    config: SdkWsConfig,
}

impl LiveWsSdkApi {
    #[must_use]
    pub fn new(base_endpoint: impl Into<String>, config: SdkWsConfig) -> Self {
        Self {
            base_endpoint: base_endpoint.into(),
            config,
        }
    }
}

#[async_trait]
impl PolymarketStreamApi for LiveWsSdkApi {
    async fn market_events(
        &self,
        token_ids: &[String],
    ) -> Result<Vec<MarketWsEvent>, PolymarketGatewayError> {
        let asset_ids = token_ids
            .iter()
            .map(|token_id| {
                token_id.parse::<U256>().map_err(|err| {
                    PolymarketGatewayError::protocol(format!("invalid market token id: {err}"))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let client =
            SdkWsClient::new(&self.base_endpoint, self.config.clone()).map_err(map_ws_error)?;
        let stream = client
            .subscribe_last_trade_price(asset_ids)
            .map_err(map_ws_error)?;
        tokio::pin!(stream);
        let first = stream
            .next()
            .await
            .ok_or_else(|| {
                PolymarketGatewayError::protocol("market websocket ended before yielding an event")
            })?
            .map_err(map_ws_error)?;

        Ok(vec![MarketWsEvent::LastTradePrice(map_market_trade_price(
            first,
        ))])
    }

    async fn user_events(
        &self,
        auth: &PolymarketUserStreamAuth,
        condition_ids: &[String],
    ) -> Result<Vec<UserWsEvent>, PolymarketGatewayError> {
        let markets = condition_ids
            .iter()
            .map(|condition_id| {
                condition_id.parse::<B256>().map_err(|err| {
                    PolymarketGatewayError::protocol(format!(
                        "invalid user market condition id: {err}"
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let credentials = Credentials::new(
            Uuid::parse_str(&auth.api_key).map_err(|err| {
                PolymarketGatewayError::protocol(format!("invalid websocket api key uuid: {err}"))
            })?,
            auth.secret.clone(),
            auth.passphrase.clone(),
        );
        let address = auth.address.parse::<Address>().map_err(|err| {
            PolymarketGatewayError::protocol(format!("invalid websocket address: {err}"))
        })?;

        let client = SdkWsClient::new(&self.base_endpoint, self.config.clone())
            .map_err(map_ws_error)?
            .authenticate(credentials, address)
            .map_err(map_ws_error)?;
        let stream = client
            .subscribe_user_events(markets)
            .map_err(map_ws_error)?;
        tokio::pin!(stream);
        let first = stream
            .next()
            .await
            .ok_or_else(|| {
                PolymarketGatewayError::protocol("user websocket ended before yielding an event")
            })?
            .map_err(map_ws_error)?;

        Ok(vec![match first {
            SdkWsMessage::Trade(trade) => UserWsEvent::Trade(map_user_trade(trade)),
            SdkWsMessage::Order(order) => UserWsEvent::Order(map_user_order(order)),
            other => {
                return Err(PolymarketGatewayError::protocol(format!(
                    "unexpected user websocket message: {other:?}"
                )))
            }
        }])
    }
}

fn map_ws_error(error: polymarket_client_sdk::error::Error) -> PolymarketGatewayError {
    PolymarketGatewayError::connectivity(error.to_string())
}

fn map_market_trade_price(event: SdkLastTradePrice) -> MarketTradePriceUpdate {
    MarketTradePriceUpdate {
        asset_id: event.asset_id.to_string(),
        price: event.price.to_string(),
        size: event.size.map(|size| size.to_string()),
        event_ts: None,
    }
}

fn map_user_trade(event: SdkTradeMessage) -> UserTradeUpdate {
    UserTradeUpdate {
        trade_id: event.id,
        order_id: event
            .taker_order_id
            .or_else(|| {
                event
                    .maker_orders
                    .first()
                    .map(|maker_order| maker_order.order_id.clone())
            })
            .unwrap_or_default(),
        status: format!("{:?}", event.status),
        condition_id: event.market.to_string(),
        price: Some(event.price.to_string()),
        size: Some(event.size.to_string()),
        fee_rate_bps: event.fee_rate_bps.map(|value| value.to_string()),
        transaction_hash: event.transaction_hash.map(|hash| format!("{hash:#x}")),
        event_ts: None,
    }
}

fn map_user_order(event: SdkOrderMessage) -> UserOrderUpdate {
    UserOrderUpdate {
        order_id: event.id,
        status: event
            .status
            .map(|status| status.to_string())
            .unwrap_or_else(|| "UNKNOWN".to_owned()),
        condition_id: event.market.to_string(),
        price: Some(event.price.to_string()),
        size: event.original_size.map(|size| size.to_string()),
        fee_rate_bps: None,
        transaction_hash: None,
        event_ts: None,
    }
}
