use domain::{SignatureType, WalletRoute};
use serde_json::json;
use url::Url;
use venue_polymarket::{
    build_post_order_request_from_signed_member, L2AuthHeaders, OrderType, PolymarketRestClient,
    PostOrderTransport, SignerContext,
};

use execution::plans::{ExecutionPlan, NegRiskMemberOrderPlan};
use execution::signing::TestOrderSigner;
use execution::OrderSigner;
use rust_decimal::Decimal;

#[test]
fn submit_order_request_uses_documented_post_path_and_signed_payload() {
    let client = sample_rest_client();
    let signed = sample_signed_negrisk_family_submission();
    let member = signed
        .members
        .first()
        .expect("test plan should include at least one member");
    let submission =
        build_post_order_request_from_signed_member(member, &sample_post_order_transport())
            .unwrap();
    let request = client
        .build_submit_order_request(&sample_l2_auth(), &submission)
        .unwrap();

    assert_eq!(request.method().as_str(), "POST");
    assert!(request.url().as_str().ends_with("/order"));

    let salt = member.identity.salt.parse::<u64>().unwrap();
    assert_eq!(
        serde_json::to_value(submission).unwrap(),
        json!({
          "order": {
            "maker": "0xmaker",
            "signer": "0xsigner",
            "taker": "0x0000000000000000000000000000000000000000",
            "tokenId": "token-1",
            "makerAmount": member.maker_amount,
            "takerAmount": member.taker_amount,
            "side": "BUY",
            "expiration": member.expiration,
            "nonce": member.identity.nonce,
            "feeRateBps": member.fee_rate_bps,
            "signature": member.identity.signature,
            "salt": salt,
            "signatureType": 0
          },
          "owner": "owner-uuid",
          "orderType": "GTC",
          "deferExec": false
        })
    );
}

#[test]
fn submit_order_builder_rejects_non_numeric_salt_values() {
    let signed = sample_signed_negrisk_family_submission();
    let mut member = signed.members[0].clone();
    member.identity.salt = "not-a-number".to_owned();

    let err = build_post_order_request_from_signed_member(&member, &sample_post_order_transport())
        .unwrap_err();

    assert!(matches!(
        err,
        venue_polymarket::PostOrderBuildError::InvalidSalt { .. }
    ));
}

#[test]
fn submit_order_builder_rejects_whitespace_padded_salt_values() {
    let signed = sample_signed_negrisk_family_submission();
    let mut member = signed.members[0].clone();
    member.identity.salt = " 123".to_owned();

    let err = build_post_order_request_from_signed_member(&member, &sample_post_order_transport())
        .unwrap_err();

    assert!(matches!(
        err,
        venue_polymarket::PostOrderBuildError::InvalidSalt { .. }
    ));
}

#[test]
fn submit_order_builder_accepts_large_numeric_string_salt_without_loss() {
    let signed = sample_signed_negrisk_family_submission();
    let mut member = signed.members[0].clone();
    let big_salt = "1844674407370955161600"; // > u64::MAX
    member.identity.salt = big_salt.to_owned();

    let submission =
        build_post_order_request_from_signed_member(&member, &sample_post_order_transport())
            .unwrap();

    let body = serde_json::to_string(&submission).unwrap();
    assert!(body.contains(&format!("\"salt\":{big_salt}")));
    assert!(!body.contains(&format!("\"salt\":\"{big_salt}\"")));
}

fn sample_rest_client() -> PolymarketRestClient {
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test client");
    let base = Url::parse("https://clob.polymarket.com/").expect("clob url should parse for tests");

    PolymarketRestClient::with_http_client(client, base.clone(), base.clone(), base)
}

fn sample_l2_auth() -> L2AuthHeaders<'static> {
    L2AuthHeaders {
        signer: SignerContext {
            address: "0xowner",
            funder_address: "0xfunder",
            signature_type: SignatureType::Eoa,
            wallet_route: WalletRoute::Eoa,
        },
        api_key: "key-1",
        passphrase: "pass-1",
        timestamp: "1700000000",
        signature: "0xsig",
    }
}

fn sample_post_order_transport() -> PostOrderTransport {
    PostOrderTransport {
        owner: "owner-uuid".to_owned(),
        order_type: OrderType::Gtc,
        defer_exec: false,
    }
}

fn sample_signed_negrisk_family_submission() -> execution::signing::SignedFamilySubmission {
    let plan = ExecutionPlan::NegRiskSubmitFamily {
        family_id: domain::EventFamilyId::from("family-a"),
        members: vec![NegRiskMemberOrderPlan {
            condition_id: domain::ConditionId::from("condition-1"),
            token_id: domain::TokenId::from("token-1"),
            price: Decimal::new(45, 2),
            quantity: Decimal::new(10, 0),
        }],
    };
    TestOrderSigner.sign_family(&plan).unwrap()
}
