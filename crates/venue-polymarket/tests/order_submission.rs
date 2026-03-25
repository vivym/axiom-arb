use domain::{SignatureType, WalletRoute};
use serde_json::json;
use url::Url;
use venue_polymarket::{
    L2AuthHeaders, OrderSide, OrderType, PolymarketRestClient, PostOrder, PostOrderRequest,
    SignerContext,
};

#[test]
fn submit_order_request_uses_documented_post_path_and_signed_payload() {
    let client = sample_rest_client();
    let submission = sample_post_order_request();
    let request = client
        .build_submit_order_request(&sample_l2_auth(), &submission)
        .unwrap();

    assert_eq!(request.method().as_str(), "POST");
    assert!(request.url().as_str().ends_with("/order"));

    assert_eq!(
        serde_json::to_value(submission).unwrap(),
        json!({
          "order": {
            "maker": "0xmaker",
            "signer": "0xsigner",
            "taker": "0x0000000000000000000000000000000000000000",
            "tokenId": "token-1",
            "makerAmount": "100",
            "takerAmount": "45",
            "side": "BUY",
            "expiration": "0",
            "nonce": "0",
            "feeRateBps": "30",
            "signature": "0xsig",
            "salt": 123,
            "signatureType": 0
          },
          "owner": "owner-uuid",
          "orderType": "GTC",
          "deferExec": false
        })
    );
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

fn sample_post_order_request() -> PostOrderRequest {
    PostOrderRequest {
        order: PostOrder {
            maker: "0xmaker".to_owned(),
            signer: "0xsigner".to_owned(),
            taker: "0x0000000000000000000000000000000000000000".to_owned(),
            token_id: "token-1".to_owned(),
            maker_amount: "100".to_owned(),
            taker_amount: "45".to_owned(),
            side: OrderSide::Buy,
            expiration: "0".to_owned(),
            nonce: "0".to_owned(),
            fee_rate_bps: "30".to_owned(),
            signature: "0xsig".to_owned(),
            salt: 123,
            signature_type: 0,
        },
        owner: "owner-uuid".to_owned(),
        order_type: OrderType::Gtc,
        defer_exec: false,
    }
}
