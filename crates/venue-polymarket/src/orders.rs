use serde::Serialize;

use execution::signing::SignedFamilyMember;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostOrderContext {
    pub maker: String,
    pub signer: String,
    pub taker: String,
    pub owner: String,
    pub expiration: String,
    pub fee_rate_bps: String,
    pub order_type: OrderType,
    pub defer_exec: bool,
    pub signature_type: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostOrderMemberFields {
    pub maker_amount: String,
    pub taker_amount: String,
    pub side: OrderSide,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostOrderBuildError {
    InvalidSalt { salt: String },
}

pub fn build_post_order_request_from_signed_member(
    member: &SignedFamilyMember,
    member_fields: &PostOrderMemberFields,
    ctx: &PostOrderContext,
) -> Result<PostOrderRequest, PostOrderBuildError> {
    let salt = member
        .identity
        .salt
        .parse::<u64>()
        .map_err(|_| PostOrderBuildError::InvalidSalt {
            salt: member.identity.salt.clone(),
        })?;

    Ok(PostOrderRequest {
        order: PostOrder {
            maker: ctx.maker.clone(),
            signer: ctx.signer.clone(),
            taker: ctx.taker.clone(),
            token_id: member.token_id.as_str().to_owned(),
            maker_amount: member_fields.maker_amount.clone(),
            taker_amount: member_fields.taker_amount.clone(),
            side: member_fields.side.clone(),
            expiration: ctx.expiration.clone(),
            nonce: member.identity.nonce.clone(),
            fee_rate_bps: ctx.fee_rate_bps.clone(),
            signature: member.identity.signature.clone(),
            salt,
            signature_type: ctx.signature_type,
        },
        owner: ctx.owner.clone(),
        order_type: ctx.order_type.clone(),
        defer_exec: ctx.defer_exec,
    })
}
