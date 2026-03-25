use domain::{SignatureType, WalletRoute};
use serde_json::json;
use url::Url;
use venue_polymarket::{
    build_post_order_request_from_signed_member, L2AuthHeaders, OrderSide, OrderType,
    PolymarketRestClient, PostOrderContext, PostOrderMemberFields,
    SignerContext,
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
    let submission = build_post_order_request_from_signed_member(
        member,
        &PostOrderMemberFields {
            maker_amount: "100".to_owned(),
            taker_amount: "45".to_owned(),
            side: OrderSide::Buy,
        },
        &sample_post_order_context(),
    )
    .unwrap();
    let request = client
        .build_submit_order_request(&sample_l2_auth(), &submission)
        .unwrap();

    assert_eq!(request.method().as_str(), "POST");
    assert!(request.url().as_str().ends_with("/order"));

    let salt = member.identity.salt.parse::<u64>().unwrap();
    assert_eq!(serde_json::to_value(submission).unwrap(), json!({
          "order": {
            "maker": "0xmaker",
            "signer": "0xsigner",
            "taker": "0x0000000000000000000000000000000000000000",
            "tokenId": "token-1",
            "makerAmount": "100",
            "takerAmount": "45",
            "side": "BUY",
            "expiration": "0",
            "nonce": member.identity.nonce,
            "feeRateBps": "30",
            "signature": member.identity.signature,
            "salt": salt,
            "signatureType": 0
          },
          "owner": "owner-uuid",
          "orderType": "GTC",
          "deferExec": false
        }));
}

fn sample_rest_client() -> PolymarketRestClient {
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test client");
    let base =
        Url::parse("https://clob.polymarket.com/").expect("clob url should parse for tests");

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

fn sample_post_order_context() -> PostOrderContext {
    PostOrderContext {
        maker: "0xmaker".to_owned(),
        signer: "0xsigner".to_owned(),
        taker: "0x0000000000000000000000000000000000000000".to_owned(),
        owner: "owner-uuid".to_owned(),
        expiration: "0".to_owned(),
        fee_rate_bps: "30".to_owned(),
        order_type: OrderType::Gtc,
        defer_exec: false,
        signature_type: 0,
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
    TestOrderSigner::default().sign_family(&plan).unwrap()
}
