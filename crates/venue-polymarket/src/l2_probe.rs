use chrono::Utc;
use reqwest::header::{HeaderName, HeaderValue, CONTENT_TYPE};
use reqwest::{Client, Method};
use serde::Serialize;
use sha2::{Digest, Sha256};
use url::Url;

use crate::errors::{PolymarketGatewayError, PolymarketGatewayErrorKind};

const POLY_API_KEY: HeaderName = HeaderName::from_static("poly-api-key");
const POLY_PASSPHRASE: HeaderName = HeaderName::from_static("poly-passphrase");
const POLY_SIGNATURE: HeaderName = HeaderName::from_static("poly-signature");
const POLY_TIMESTAMP: HeaderName = HeaderName::from_static("poly-timestamp");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolymarketL2ProbeCredentials {
    pub api_key: String,
    pub secret: String,
    pub passphrase: String,
}

#[derive(Debug, Clone)]
pub struct PolymarketL2ProbeClient {
    host: Url,
    http: Client,
    credentials: PolymarketL2ProbeCredentials,
}

impl PolymarketL2ProbeClient {
    #[must_use]
    pub fn new(host: Url, credentials: PolymarketL2ProbeCredentials) -> Self {
        Self::with_http_client(Client::new(), host, credentials)
    }

    #[must_use]
    pub fn with_http_client(
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
        let body = match previous_heartbeat_id {
            Some(heartbeat_id) => Some(
                serde_json::to_string(&HeartbeatRequest { heartbeat_id }).map_err(|err| {
                    PolymarketGatewayError::protocol(format!(
                        "failed to serialize heartbeat request: {err}"
                    ))
                })?,
            ),
            None => None,
        };

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

pub fn build_l2_probe_signature(
    credentials: &PolymarketL2ProbeCredentials,
    timestamp: &str,
    method: &str,
    request_path: &str,
    body: &str,
) -> Result<String, PolymarketGatewayError> {
    ensure_field("api_key", &credentials.api_key)?;
    ensure_field("secret", &credentials.secret)?;
    ensure_field("passphrase", &credentials.passphrase)?;
    ensure_field("timestamp", timestamp)?;
    ensure_field("method", method)?;
    ensure_field("request_path", request_path)?;

    let secret = decode_base64(credentials.secret.as_bytes()).map_err(|err| {
        PolymarketGatewayError::auth(format!("invalid l2 probe secret encoding: {err}"))
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
    heartbeat_id: &'a str,
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
    let mut output = String::with_capacity(((bytes.len() + 2) / 3) * 4);

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

    for chunk in filtered.chunks(4) {
        let mut values = [0_u8; 4];
        let mut padding = 0;

        for (index, byte) in chunk.iter().enumerate() {
            match byte {
                b'=' => {
                    if index < 2 {
                        return Err("invalid base64 padding".to_owned());
                    }
                    padding += 1;
                }
                other => {
                    values[index] = decode_base64_char(*other)
                        .ok_or_else(|| format!("invalid base64 character: {}", *other as char))?;
                }
            }
        }

        let word = ((values[0] as u32) << 18)
            | ((values[1] as u32) << 12)
            | ((values[2] as u32) << 6)
            | (values[3] as u32);

        output.push((word >> 16) as u8);
        if padding < 2 {
            output.push((word >> 8) as u8);
        }
        if padding < 1 {
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
