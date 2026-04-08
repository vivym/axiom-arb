mod support;

use reqwest::Url;
use venue_polymarket::{PolymarketL2ProbeClient, PolymarketL2ProbeCredentials};

#[tokio::test]
async fn l2_probe_fetch_open_orders_uses_current_data_orders_path() {
    let server = support::MockServer::spawn("200 OK", r#"{"data":[]}"#);
    let probe = sample_probe(server.base_url());

    probe.fetch_open_orders().await.unwrap();

    let request = server.finish();
    assert!(request.starts_with("GET /data/orders HTTP/1.1"));
    assert!(request.contains("poly-api-key: key-1"));
    assert!(request.contains("poly-passphrase: pass-1"));
    assert!(request.contains("poly-signature: "));
}

#[tokio::test]
async fn l2_probe_post_heartbeat_uses_current_heartbeat_path_and_body() {
    let server = support::MockServer::spawn("200 OK", r#"{"ok":true}"#);
    let probe = sample_probe(server.base_url());

    probe.post_heartbeat(Some("hb-41")).await.unwrap();

    let request = server.finish();
    assert!(request.starts_with("POST /v1/heartbeats HTTP/1.1"));
    assert!(request.contains(r#""heartbeat_id":"hb-41""#));
    assert!(request.contains("poly-api-key: key-1"));
    assert!(request.contains("poly-passphrase: pass-1"));
}

#[tokio::test]
async fn l2_probe_rejects_invalid_secret_encoding() {
    let probe = PolymarketL2ProbeClient::new(
        Url::parse("http://127.0.0.1/").unwrap(),
        PolymarketL2ProbeCredentials {
            api_key: "key-1".to_owned(),
            secret: "not-base64!".to_owned(),
            passphrase: "pass-1".to_owned(),
        },
    );

    let err = probe.fetch_open_orders().await.unwrap_err();

    assert!(err.to_string().contains("secret"));
}

fn sample_probe(base_url: Url) -> PolymarketL2ProbeClient {
    std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
    PolymarketL2ProbeClient::new(base_url, sample_credentials())
}

fn sample_credentials() -> PolymarketL2ProbeCredentials {
    PolymarketL2ProbeCredentials {
        api_key: "key-1".to_owned(),
        secret: "c2VjcmV0LWJ5dGVz".to_owned(),
        passphrase: "pass-1".to_owned(),
    }
}
