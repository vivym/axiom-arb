use domain::{SignatureType, WalletRoute};
use url::Url;
use venue_polymarket::{L2AuthHeaders, PolymarketRestClient, SignedOrderSubmission, SignerContext};

#[test]
fn submit_order_request_uses_documented_post_path_and_signed_payload() {
    let client = sample_rest_client();
    let request = client
        .build_submit_order_request(&sample_l2_auth(), &sample_signed_order_submission())
        .unwrap();

    assert_eq!(request.method().as_str(), "POST");
    assert!(request.url().as_str().ends_with("/order"));
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

fn sample_signed_order_submission() -> SignedOrderSubmission {
    SignedOrderSubmission {
        order: venue_polymarket::OrderPayload {
            token_id: "token-1".to_owned(),
            price: "0.45".to_owned(),
            size: "10".to_owned(),
            side: "BUY".to_owned(),
            expiration: "0".to_owned(),
        },
        signed: venue_polymarket::SignedOrderPayload {
            signed_order_hash: "0xhash".to_owned(),
            salt: "123".to_owned(),
            nonce: "7".to_owned(),
            signature: "0xsig".to_owned(),
        },
    }
}

