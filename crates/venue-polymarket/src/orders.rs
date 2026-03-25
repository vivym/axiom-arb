use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OrderType {
    Gtc,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OrderSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PostOrder {
    pub maker: String,
    pub signer: String,
    pub taker: String,
    #[serde(rename = "tokenId")]
    pub token_id: String,
    #[serde(rename = "makerAmount")]
    pub maker_amount: String,
    #[serde(rename = "takerAmount")]
    pub taker_amount: String,
    pub side: OrderSide,
    pub expiration: String,
    pub nonce: String,
    #[serde(rename = "feeRateBps")]
    pub fee_rate_bps: String,
    pub signature: String,
    pub salt: u64,
    #[serde(rename = "signatureType")]
    pub signature_type: u8,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PostOrderRequest {
    pub order: PostOrder,
    pub owner: String,
    #[serde(rename = "orderType")]
    pub order_type: OrderType,
    #[serde(rename = "deferExec")]
    pub defer_exec: bool,
}
