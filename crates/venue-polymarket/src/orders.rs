use serde::Serialize;
use serde_json::value::RawValue;

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

#[derive(Debug, Clone, Serialize)]
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
    // Polymarket L1 docs describe salt as a string; we serialize as a numeric JSON literal
    // without narrowing to machine integer widths (to avoid rejecting large salts).
    pub salt: Box<RawValue>,
    #[serde(rename = "signatureType")]
    pub signature_type: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct PostOrderRequest {
    pub order: PostOrder,
    pub owner: String,
    #[serde(rename = "orderType")]
    pub order_type: OrderType,
    #[serde(rename = "deferExec")]
    pub defer_exec: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostOrderTransport {
    pub owner: String,
    pub order_type: OrderType,
    pub defer_exec: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostOrderBuildError {
    InvalidSalt { salt: String },
    InvalidSide { side: String },
}

pub fn build_post_order_request_from_signed_member(
    member: &SignedFamilyMember,
    transport: &PostOrderTransport,
) -> Result<PostOrderRequest, PostOrderBuildError> {
    let salt_str = member.identity.salt.trim();
    let salt_is_numeric = !salt_str.is_empty() && salt_str.bytes().all(|b| b.is_ascii_digit());
    if !salt_is_numeric {
        return Err(PostOrderBuildError::InvalidSalt {
            salt: member.identity.salt.clone(),
        });
    }

    // Keep the literal as-is (no u64 parsing). RawValue ensures it's valid JSON.
    let salt = RawValue::from_string(salt_str.to_owned()).map_err(|_| PostOrderBuildError::InvalidSalt {
        salt: member.identity.salt.clone(),
    })?;

    let side = match member.side.as_str() {
        "BUY" => OrderSide::Buy,
        "SELL" => OrderSide::Sell,
        other => {
            return Err(PostOrderBuildError::InvalidSide {
                side: other.to_owned(),
            })
        }
    };

    Ok(PostOrderRequest {
        order: PostOrder {
            maker: member.maker.clone(),
            signer: member.signer.clone(),
            taker: member.taker.clone(),
            token_id: member.token_id.as_str().to_owned(),
            maker_amount: member.maker_amount.clone(),
            taker_amount: member.taker_amount.clone(),
            side,
            expiration: member.expiration.clone(),
            nonce: member.identity.nonce.clone(),
            fee_rate_bps: member.fee_rate_bps.clone(),
            signature: member.identity.signature.clone(),
            salt,
            signature_type: member.signature_type,
        },
        owner: transport.owner.clone(),
        order_type: transport.order_type.clone(),
        defer_exec: transport.defer_exec,
    })
}
