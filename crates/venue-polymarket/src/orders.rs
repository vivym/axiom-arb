use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OrderPayload {
    pub token_id: String,
    pub price: String,
    pub size: String,
    pub side: String,
    pub expiration: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SignedOrderPayload {
    pub signed_order_hash: String,
    pub salt: String,
    pub nonce: String,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SignedOrderSubmission {
    pub order: OrderPayload,
    pub signed: SignedOrderPayload,
}

