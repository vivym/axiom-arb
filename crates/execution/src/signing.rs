use domain::{
    MarketId, Order, OrderId, SettlementState, SignedOrderIdentity, SubmissionState, VenueOrderState,
};

use crate::orders::SignedOrderEnvelope;
use crate::plans::ExecutionPlan;

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
pub struct SignedFamilyMemberOrder {
    pub order: Order,
    pub envelope: SignedOrderEnvelope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedFamilySubmission {
    pub plan_id: String,
    pub orders: Vec<SignedFamilyMemberOrder>,
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
                let orders = members
                    .iter()
                    .enumerate()
                    .map(|(index, member)| {
                        let identity = self.sign_identity(&plan_id, index);
                        let order_id = OrderId::from(format!(
                            "test-order:{}:{}",
                            family_id.as_str(),
                            index + 1
                        ));

                        let order = Order {
                            order_id: order_id.clone(),
                            market_id: MarketId::from(format!(
                                "test-market:{}",
                                member.token_id.as_str()
                            )),
                            condition_id: member.condition_id.clone(),
                            token_id: member.token_id.clone(),
                            quantity: member.quantity,
                            price: member.price,
                            submission_state: SubmissionState::Signed,
                            venue_state: VenueOrderState::Unknown,
                            settlement_state: SettlementState::Unknown,
                            signed_order: Some(identity.clone()),
                        };
                        let envelope = SignedOrderEnvelope::new(order_id, identity);

                        SignedFamilyMemberOrder { order, envelope }
                    })
                    .collect();

                Ok(SignedFamilySubmission { plan_id, orders })
            }
            other => Err(SigningError::UnsupportedPlan {
                plan_id: other.plan_id(),
            }),
        }
    }
}

