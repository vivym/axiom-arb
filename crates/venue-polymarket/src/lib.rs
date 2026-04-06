mod auth;
mod errors;
mod gateway;
mod heartbeat;
mod instrumentation;
mod metadata;
mod negrisk_live;
mod orders;
mod proxy;
mod relayer;
mod rest;
mod retry;
mod sdk_backend;
mod ws_client;
mod ws_market;
mod ws_session;
mod ws_user;

pub use auth::{
    build_l2_auth_headers, build_relayer_auth_headers, derive_builder_relayer_auth_material,
    derive_l2_auth_material, signature_type_label, signature_type_to_wallet_route,
    wallet_route_label, wallet_route_to_signature_type, AuthError,
    DerivedBuilderRelayerAuthMaterial, DerivedL2AuthMaterial, L2AuthHeaders, RelayerAuth,
    SignerContext,
};
pub use domain::{MarketRoute, NegRiskVariant};
pub use errors::{PolymarketGatewayError, PolymarketGatewayErrorKind};
pub use gateway::{
    PolymarketGateway, PolymarketHeartbeatStatus, PolymarketOpenOrderSummary, PolymarketOrderQuery,
    PolymarketSignedOrder, PolymarketSubmitResponse,
};
pub use heartbeat::{
    HeartbeatFetchResult, HeartbeatReconcileReason, OrderHeartbeatMonitor, OrderHeartbeatState,
};
pub use instrumentation::VenueProducerInstrumentation;
pub use metadata::{NegRiskMarketMetadata, NegRiskMetadataError};
pub use negrisk_live::{PolymarketNegRiskReconcileProvider, PolymarketNegRiskSubmitProvider};
pub use orders::{
    build_post_order_request_from_signed_member, OrderSide, OrderType, PostOrder,
    PostOrderBuildError, PostOrderRequest, PostOrderTransport,
};
pub use relayer::{RelayerTransaction, RelayerTransactionType};
pub use rest::{
    BalanceAllowanceResponse, OpenOrderSummary, PolymarketRestClient, RestClientBuildError,
    RestError, VenueStatusResponse,
};
pub use retry::{map_venue_status, BusinessErrorKind, HttpRetryContext, RetryClass, RetryDecision};
#[doc(hidden)]
pub use sdk_backend::{LiveClobSdkApi, PolymarketClobApi};
pub use url::Url as PolymarketUrl;
pub use ws_client::{
    PolymarketWsClient, WsClientError, WsCloseFrame, WsMessageSource, WsSubscriptionOp,
    WsTransportMessage, WsUserChannelAuth,
};
pub use ws_market::{
    parse_market_message, parse_market_messages, MarketBookUpdate, MarketLifecycleUpdate,
    MarketPriceChangeUpdate, MarketTickSizeChangeUpdate, MarketTradePriceUpdate, MarketWsEvent,
    WsChannelKind, WsChannelLivenessMonitor, WsChannelReconcileReason, WsChannelState,
    WsParseError,
};
pub use ws_session::{WsSessionEvent, WsSessionMonitor, WsSessionState, WsSessionStatus};
pub use ws_user::{parse_user_message, UserOrderUpdate, UserTradeUpdate, UserWsEvent};
