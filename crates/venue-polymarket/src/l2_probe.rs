use chrono::Utc;
use reqwest::header::{HeaderName, HeaderValue, CONTENT_TYPE};
use reqwest::{Client, Method};
use serde::Serialize;
use sha2::{Digest, Sha256};
use url::Url;

use crate::errors::{PolymarketGatewayError, PolymarketGatewayErrorKind};

const DEFAULT_HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const POLY_API_KEY: HeaderName = HeaderName::from_static("poly_api_key");
const POLY_ADDRESS: HeaderName = HeaderName::from_static("poly_address");
const POLY_PASSPHRASE: HeaderName = HeaderName::from_static("poly_passphrase");
const POLY_SIGNATURE: HeaderName = HeaderName::from_static("poly_signature");
const POLY_TIMESTAMP: HeaderName = HeaderName::from_static("poly_timestamp");

#[derive(Clone, PartialEq, Eq)]
pub struct PolymarketL2ProbeCredentials {
    pub address: String,
    pub api_key: String,
    pub secret: String,
    pub passphrase: String,
}

impl std::fmt::Debug for PolymarketL2ProbeCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolymarketL2ProbeCredentials")
            .field("address", &"<redacted>")
            .field("api_key", &"<redacted>")
            .field("secret", &"<redacted>")
            .field("passphrase", &"<redacted>")
            .finish()
    }
}

#[derive(Clone)]
pub struct PolymarketL2ProbeClient {
    host: Url,
    http: Client,
    credentials: PolymarketL2ProbeCredentials,
}

impl std::fmt::Debug for PolymarketL2ProbeClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolymarketL2ProbeClient")
            .field("host", &self.host)
            .field("http", &"<redacted>")
            .field("credentials", &"<redacted>")
            .finish()
    }
}

impl PolymarketL2ProbeClient {
    pub fn new(
        host: Url,
        credentials: PolymarketL2ProbeCredentials,
    ) -> Result<Self, PolymarketGatewayError> {
        Ok(Self::with_http_client(
            build_default_http_client()?,
            host,
            credentials,
        ))
    }

    #[must_use]
    fn with_http_client(
        http: Client,
        host: Url,
        credentials: PolymarketL2ProbeCredentials,
    ) -> Self {
        Self {
            host,
            http,
            credentials,
        }
    }

    pub async fn fetch_open_orders(&self) -> Result<(), PolymarketGatewayError> {
        self.send_signed_request(Method::GET, "data/orders", None)
            .await
    }

    pub async fn post_heartbeat(
        &self,
        previous_heartbeat_id: Option<&str>,
    ) -> Result<(), PolymarketGatewayError> {
        let body = Some(
            serde_json::to_string(&HeartbeatRequest {
                heartbeat_id: previous_heartbeat_id,
            })
            .map_err(|err| {
                PolymarketGatewayError::protocol(format!(
                    "failed to serialize heartbeat request: {err}"
                ))
            })?,
        );

        self.send_signed_request(Method::POST, "v1/heartbeats", body)
            .await
    }

    async fn send_signed_request(
        &self,
        method: Method,
        path: &str,
        body: Option<String>,
    ) -> Result<(), PolymarketGatewayError> {
        let url = join_url(&self.host, path).map_err(|err| {
            PolymarketGatewayError::protocol(format!("invalid l2 probe url: {err}"))
        })?;
        let request_path = request_path_with_query(&url);
        let body_text = body.as_deref().unwrap_or("");
        let timestamp = Utc::now().timestamp().to_string();
        let signature = build_l2_probe_signature(
            &self.credentials,
            &timestamp,
            method.as_str(),
            &request_path,
            body_text,
        )?;

        let mut request = self
            .http
            .request(method, url)
            .header(POLY_ADDRESS, header_value(&self.credentials.address)?)
            .header(POLY_API_KEY, header_value(&self.credentials.api_key)?)
            .header(POLY_PASSPHRASE, header_value(&self.credentials.passphrase)?)
            .header(POLY_TIMESTAMP, header_value(&timestamp)?)
            .header(POLY_SIGNATURE, header_value(&signature)?);

        if let Some(body) = body {
            request = request.header(CONTENT_TYPE, "application/json").body(body);
        }

        let response = request.send().await.map_err(|err| {
            PolymarketGatewayError::connectivity(format!("l2 probe request failed: {err}"))
        })?;

        if response.status().is_success() {
            return Ok(());
        }

        Err(map_upstream_error(response).await)
    }
}

fn build_l2_probe_signature(
    credentials: &PolymarketL2ProbeCredentials,
    timestamp: &str,
    method: &str,
    request_path: &str,
    body: &str,
) -> Result<String, PolymarketGatewayError> {
    ensure_field("api_key", &credentials.api_key)?;
    ensure_field("address", &credentials.address)?;
    ensure_field("secret", &credentials.secret)?;
    ensure_field("passphrase", &credentials.passphrase)?;
    ensure_field("timestamp", timestamp)?;
    ensure_field("method", method)?;
    ensure_field("request_path", request_path)?;

    let secret = decode_base64(credentials.secret.as_bytes()).map_err(|err| {
        PolymarketGatewayError::protocol(format!("invalid l2 probe secret encoding: {err}"))
    })?;
    let payload = format!(
        "{}{}{}{}",
        timestamp,
        method.to_ascii_uppercase(),
        request_path,
        body
    );
    let digest = hmac_sha256(&secret, payload.as_bytes());
    Ok(base64_urlsafe(&digest))
}

#[derive(Debug, Serialize)]
struct HeartbeatRequest<'a> {
    heartbeat_id: Option<&'a str>,
}

async fn map_upstream_error(response: reqwest::Response) -> PolymarketGatewayError {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    PolymarketGatewayError::new(
        PolymarketGatewayErrorKind::UpstreamResponse,
        format!("l2 probe returned {status}: {body}"),
    )
}

fn join_url(base: &Url, path: &str) -> Result<Url, url::ParseError> {
    let trimmed = path.trim_start_matches('/');
    base.join(trimmed)
}

fn request_path_with_query(url: &Url) -> String {
    match url.query() {
        Some(query) => format!("{}?{}", url.path(), query),
        None => url.path().to_owned(),
    }
}

fn header_value(value: &str) -> Result<HeaderValue, PolymarketGatewayError> {
    HeaderValue::from_str(value).map_err(|err| {
        PolymarketGatewayError::protocol(format!("invalid l2 probe header value: {err}"))
    })
}

fn ensure_field(field: &'static str, value: &str) -> Result<(), PolymarketGatewayError> {
    if value.trim().is_empty() {
        return Err(PolymarketGatewayError::auth(format!(
            "missing l2 probe field: {field}"
        )));
    }

    Ok(())
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK_SIZE: usize = 64;
    let mut key_block = [0_u8; BLOCK_SIZE];
    if key.len() > BLOCK_SIZE {
        let digest = Sha256::digest(key);
        key_block[..digest.len()].copy_from_slice(&digest);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut inner_pad = [0x36_u8; BLOCK_SIZE];
    let mut outer_pad = [0x5c_u8; BLOCK_SIZE];
    for index in 0..BLOCK_SIZE {
        inner_pad[index] ^= key_block[index];
        outer_pad[index] ^= key_block[index];
    }

    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(message);
    let inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner_digest);

    let mut digest = [0_u8; 32];
    digest.copy_from_slice(&outer.finalize());
    digest
}

fn base64_urlsafe(bytes: &[u8]) -> String {
    let standard = base64_standard(bytes);
    standard.replace('+', "-").replace('/', "_")
}

fn base64_standard(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);

    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);

        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        output.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        output.push(TABLE[((n >> 12) & 0x3f) as usize] as char);

        if chunk.len() > 1 {
            output.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
        } else {
            output.push('=');
        }

        if chunk.len() > 2 {
            output.push(TABLE[(n & 0x3f) as usize] as char);
        } else {
            output.push('=');
        }
    }

    output
}

fn decode_base64(input: &[u8]) -> Result<Vec<u8>, String> {
    let mut filtered: Vec<u8> = input
        .iter()
        .copied()
        .filter(|byte| !byte.is_ascii_whitespace())
        .map(|byte| match byte {
            b'-' => b'+',
            b'_' => b'/',
            other => other,
        })
        .collect();

    if filtered.is_empty() {
        return Err("invalid base64 length".to_owned());
    }

    match filtered.len() % 4 {
        0 => {}
        2 => filtered.extend([b'=', b'=']),
        3 => filtered.push(b'='),
        _ => return Err("invalid base64 length".to_owned()),
    }

    let mut output = Vec::with_capacity(filtered.len() / 4 * 3);
    let total_chunks = filtered.len() / 4;

    for (chunk_index, chunk) in filtered.chunks(4).enumerate() {
        let mut values = [0_u8; 4];
        let mut seen_padding = false;

        for (index, byte) in chunk.iter().enumerate() {
            match byte {
                b'=' => {
                    if index < 2 {
                        return Err("invalid base64 padding".to_owned());
                    }
                    seen_padding = true;
                }
                other => {
                    if seen_padding {
                        return Err("invalid base64 padding".to_owned());
                    }
                    values[index] = decode_base64_char(*other)
                        .ok_or_else(|| format!("invalid base64 character: {}", *other as char))?;
                }
            }
        }

        if seen_padding && chunk_index + 1 != total_chunks {
            return Err("invalid base64 padding".to_owned());
        }

        let word = ((values[0] as u32) << 18)
            | ((values[1] as u32) << 12)
            | ((values[2] as u32) << 6)
            | (values[3] as u32);

        output.push((word >> 16) as u8);
        if chunk[2] != b'=' {
            output.push((word >> 8) as u8);
        }
        if chunk[3] != b'=' {
            output.push(word as u8);
        }
    }

    Ok(output)
}

fn decode_base64_char(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn build_default_http_client() -> Result<Client, PolymarketGatewayError> {
    Client::builder()
        .timeout(DEFAULT_HTTP_TIMEOUT)
        .build()
        .map_err(|err| {
            PolymarketGatewayError::connectivity(format!("failed to build l2 probe client: {err}"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_credentials() -> PolymarketL2ProbeCredentials {
        PolymarketL2ProbeCredentials {
            address: "0x1111111111111111111111111111111111111111".to_owned(),
            api_key: "key-1".to_owned(),
            secret: "c2VjcmV0LWJ5dGVz".to_owned(),
            passphrase: "pass-1".to_owned(),
        }
    }

    #[test]
    fn signature_normalizes_method_and_keeps_query_and_empty_body() {
        let lower = build_l2_probe_signature(
            &sample_credentials(),
            "1700000000",
            "post",
            "/v1/heartbeats",
            "",
        )
        .unwrap();
        let upper = build_l2_probe_signature(
            &sample_credentials(),
            "1700000000",
            "POST",
            "/v1/heartbeats",
            "",
        )
        .unwrap();
        let query = build_l2_probe_signature(
            &sample_credentials(),
            "1700000000",
            "GET",
            "/data/orders?cursor=abc",
            "",
        )
        .unwrap();
        let no_query = build_l2_probe_signature(
            &sample_credentials(),
            "1700000000",
            "GET",
            "/data/orders",
            "",
        )
        .unwrap();
        let nonempty_body = build_l2_probe_signature(
            &sample_credentials(),
            "1700000000",
            "GET",
            "/data/orders?cursor=abc",
            r#"{"heartbeat_id":"abc"}"#,
        )
        .unwrap();

        assert_eq!(lower, upper);
        assert_ne!(query, no_query);
        assert_ne!(query, nonempty_body);
    }

    #[test]
    fn signature_matches_official_fixture() {
        let signature = build_l2_probe_signature(
            &PolymarketL2ProbeCredentials {
                address: "0x6e0c80c90ea6c15917308F820Eac91Ce2724B5b5".to_owned(),
                api_key: "019894b9-cb40-79c4-b2bd-6aecb6f8c6c5".to_owned(),
                secret: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_owned(),
                passphrase: "1816e5ed89518467ffa78c65a2d6a62d240f6fd6d159cba7b2c4dc510800f75a"
                    .to_owned(),
            },
            "1758744060",
            "POST",
            "/order",
            r#"{"deferExec":false,"order":{"salt":718139292476,"maker":"0x6e0c80c90ea6c15917308F820Eac91Ce2724B5b5","signer":"0x6e0c80c90ea6c15917308F820Eac91Ce2724B5b5","taker":"0x0000000000000000000000000000000000000000","tokenId":"15871154585880608648532107628464183779895785213830018178010423617714102767076","makerAmount":"5000000","takerAmount":"10000000","side":"BUY","expiration":"0","nonce":"0","feeRateBps":"1000","signatureType":0,"signature":"0x64a2b097cf14f9a24403748b4060bedf8f33f3dbe2a38e5f85bc2a5f2b841af633a2afcc9c4d57e60e4ff1d58df2756b2ca469f984ecfd46cb0c8baba8a0d6411b"},"owner":"5d1c266a-ed39-b9bd-c1f5-f24ae3e14a7b","orderType":"GTC"}"#,
        )
        .unwrap();

        assert_eq!(signature, "8xh8d0qZHhBcLLYbsKNeiOW3Z0W2N5yNEq1kCVMe5QE=");
    }

    #[test]
    fn debug_output_redacts_sensitive_credentials() {
        let credentials = sample_credentials();
        let client = PolymarketL2ProbeClient::with_http_client(
            Client::builder().build().unwrap(),
            Url::parse("http://127.0.0.1/").unwrap(),
            credentials.clone(),
        );

        let credentials_debug = format!("{:?}", credentials);
        let client_debug = format!("{:?}", client);

        assert!(!credentials_debug.contains("0x1111111111111111111111111111111111111111"));
        assert!(!credentials_debug.contains("key-1"));
        assert!(!credentials_debug.contains("c2VjcmV0LWJ5dGVz"));
        assert!(!credentials_debug.contains("pass-1"));
        assert!(credentials_debug.contains("redacted"));

        assert!(!client_debug.contains("0x1111111111111111111111111111111111111111"));
        assert!(!client_debug.contains("key-1"));
        assert!(!client_debug.contains("c2VjcmV0LWJ5dGVz"));
        assert!(!client_debug.contains("pass-1"));
        assert!(client_debug.contains("redacted"));
    }

    #[test]
    fn signature_rejects_invalid_secret_encoding() {
        let err = build_l2_probe_signature(
            &PolymarketL2ProbeCredentials {
                address: "0x1111111111111111111111111111111111111111".to_owned(),
                api_key: "key-1".to_owned(),
                secret: "not-base64!".to_owned(),
                passphrase: "pass-1".to_owned(),
            },
            "1700000000",
            "GET",
            "/data/orders",
            "",
        )
        .unwrap_err();

        assert_eq!(err.kind, PolymarketGatewayErrorKind::Protocol);
    }

    #[test]
    fn signature_rejects_malformed_base64_padding() {
        let err = build_l2_probe_signature(
            &PolymarketL2ProbeCredentials {
                address: "0x1111111111111111111111111111111111111111".to_owned(),
                api_key: "key-1".to_owned(),
                secret: "AA=A".to_owned(),
                passphrase: "pass-1".to_owned(),
            },
            "1700000000",
            "GET",
            "/data/orders",
            "",
        )
        .unwrap_err();

        assert_eq!(err.kind, PolymarketGatewayErrorKind::Protocol);
    }
}
