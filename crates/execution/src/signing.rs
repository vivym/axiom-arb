use crate::plans::ExecutionPlan;
use domain::{ConditionId, SignedOrderIdentity, TokenId};
use rust_decimal::Decimal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SigningError {
    UnsupportedPlan { plan_id: String },
}

/// Compatibility signer surface used by existing Phase 3b wiring and tests.
/// Phase 3c execution consumes the provider-facing `SignerProvider` trait, which
/// is implemented for every `OrderSigner`.
pub trait OrderSigner: Send + Sync {
    fn sign_family(&self, plan: &ExecutionPlan) -> Result<SignedFamilySubmission, SigningError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedFamilyMember {
    pub condition_id: ConditionId,
    pub token_id: TokenId,
    pub price: Decimal,
    pub quantity: Decimal,
    // Signature-covered venue fields (per Polymarket L1 docs). The venue layer should not
    // reconstruct these, only transport them.
    pub maker: String,
    pub signer: String,
    pub taker: String,
    pub maker_amount: String,
    pub taker_amount: String,
    pub side: String,
    pub expiration: String,
    pub fee_rate_bps: String,
    pub signature_type: u8,
    pub identity: SignedOrderIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedFamilySubmission {
    pub plan_id: String,
    pub members: Vec<SignedFamilyMember>,
}

#[derive(Debug, Default)]
pub struct TestOrderSigner;

impl TestOrderSigner {
    fn sign_identity(&self, plan_id: &str, index: usize) -> SignedOrderIdentity {
        // Deterministic fake identity for tests and plumbing. Not cryptographic signing.
        SignedOrderIdentity {
            signed_order_hash: format!("test-hash:{plan_id}:{index}"),
            // Polymarket docs show numeric salt; keep the test artifact parseable.
            salt: format!("{}", 123_u64 + index as u64),
            // Polymarket docs show numeric nonce string.
            nonce: format!("{index}"),
            signature: format!("test-sig:{plan_id}:{index}"),
        }
    }
}

impl OrderSigner for TestOrderSigner {
    fn sign_family(&self, plan: &ExecutionPlan) -> Result<SignedFamilySubmission, SigningError> {
        match plan {
            ExecutionPlan::NegRiskSubmitFamily { family_id, members } => {
                let plan_id = plan.plan_id();
                let _ = family_id; // family is already encoded into plan_id; keep signer output narrow.

                // Canonicalize member ordering so logically-equivalent plans sign deterministically.
                // Keep the effective ordering consistent with the canonical plan identity.
                let mut canonical_members: Vec<_> = members.iter().collect();
                canonical_members.sort_by(|left, right| {
                    left.condition_id
                        .as_str()
                        .cmp(right.condition_id.as_str())
                        .then_with(|| left.token_id.as_str().cmp(right.token_id.as_str()))
                        .then_with(|| {
                            left.price
                                .normalize()
                                .to_string()
                                .cmp(&right.price.normalize().to_string())
                        })
                        .then_with(|| {
                            left.quantity
                                .normalize()
                                .to_string()
                                .cmp(&right.quantity.normalize().to_string())
                        })
                });

                let signed_members = canonical_members
                    .into_iter()
                    .enumerate()
                    .map(|(index, member)| {
                        let identity = self.sign_identity(&plan_id, index);
                        let maker_amount = member.quantity.normalize().to_string();
                        let taker_amount = (member.price * member.quantity).normalize().to_string();

                        SignedFamilyMember {
                            condition_id: member.condition_id.clone(),
                            token_id: member.token_id.clone(),
                            price: member.price,
                            quantity: member.quantity,
                            maker: "0xmaker".to_owned(),
                            signer: "0xsigner".to_owned(),
                            taker: "0x0000000000000000000000000000000000000000".to_owned(),
                            maker_amount,
                            taker_amount,
                            side: "BUY".to_owned(),
                            expiration: "0".to_owned(),
                            fee_rate_bps: "30".to_owned(),
                            signature_type: 0,
                            identity,
                        }
                    })
                    .collect();

                Ok(SignedFamilySubmission {
                    plan_id,
                    members: signed_members,
                })
            }
            other => Err(SigningError::UnsupportedPlan {
                plan_id: other.plan_id(),
            }),
        }
    }
}
