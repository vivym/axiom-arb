use domain::{ConditionId, EventFamilyId, TokenId};
use execution::plans::{ExecutionPlan, NegRiskMemberOrderPlan};
use execution::signing::{OrderSigner, TestOrderSigner};
use rust_decimal::Decimal;

#[test]
fn deterministic_test_signer_attaches_signed_identity_to_each_planned_member_order() {
    let signed = TestOrderSigner::default()
        .sign_family(&sample_family_plan())
        .unwrap();
    assert_eq!(signed.orders.len(), 2);
    assert!(signed
        .orders
        .iter()
        .all(|order| order.order.signed_order.is_some()));
}

fn sample_family_plan() -> ExecutionPlan {
    ExecutionPlan::NegRiskSubmitFamily {
        family_id: EventFamilyId::from("family-a"),
        members: vec![
            NegRiskMemberOrderPlan {
                condition_id: ConditionId::from("condition-1"),
                token_id: TokenId::from("token-1"),
                price: Decimal::new(45, 2),
                quantity: Decimal::new(10, 0),
            },
            NegRiskMemberOrderPlan {
                condition_id: ConditionId::from("condition-2"),
                token_id: TokenId::from("token-2"),
                price: Decimal::new(55, 2),
                quantity: Decimal::new(8, 0),
            },
        ],
    }
}

