use crate::plans::ExecutionPlan;
use domain::{ConditionId, SignedOrderIdentity, TokenId};
use rust_decimal::Decimal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SigningError {
    UnsupportedPlan {
        plan_id: String,
    },
}

pub trait OrderSigner: Send + Sync {
    fn sign_family(&self, plan: &ExecutionPlan) -> Result<SignedFamilySubmission, SigningError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedFamilyMember {
    pub condition_id: ConditionId,
    pub token_id: TokenId,
    pub price: Decimal,
    pub quantity: Decimal,
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
            salt: format!("test-salt:{plan_id}:{index}"),
            nonce: format!("test-nonce:{plan_id}:{index}"),
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
                let signed_members = members
                    .iter()
                    .enumerate()
                    .map(|(index, member)| {
                        let identity = self.sign_identity(&plan_id, index);
                        SignedFamilyMember {
                            condition_id: member.condition_id.clone(),
                            token_id: member.token_id.clone(),
                            price: member.price,
                            quantity: member.quantity,
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
