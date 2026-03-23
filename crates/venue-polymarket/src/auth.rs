use std::fmt;

use domain::{SignatureType, WalletRoute};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

const POLY_ADDRESS: HeaderName = HeaderName::from_static("poly-address");
const POLY_API_KEY: HeaderName = HeaderName::from_static("poly-api-key");
const POLY_PASSPHRASE: HeaderName = HeaderName::from_static("poly-passphrase");
const POLY_SIGNATURE: HeaderName = HeaderName::from_static("poly-signature");
const POLY_SIGNATURE_TYPE: HeaderName = HeaderName::from_static("poly-signature-type");
const POLY_TIMESTAMP: HeaderName = HeaderName::from_static("poly-timestamp");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct L2AuthHeaders<'a> {
    pub address: &'a str,
    pub api_key: &'a str,
    pub passphrase: &'a str,
    pub timestamp: &'a str,
    pub signature: &'a str,
    pub signature_type: SignatureType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    EmptyField(&'static str),
    InvalidHeaderValue(&'static str),
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyField(name) => write!(f, "missing auth field: {name}"),
            Self::InvalidHeaderValue(name) => write!(f, "invalid auth header value: {name}"),
        }
    }
}

impl std::error::Error for AuthError {}

pub fn signature_type_label(signature_type: SignatureType) -> &'static str {
    match signature_type {
        SignatureType::Eoa => "EOA",
        SignatureType::Proxy => "PROXY",
        SignatureType::Safe => "SAFE",
    }
}

pub fn signature_type_to_wallet_route(signature_type: SignatureType) -> WalletRoute {
    match signature_type {
        SignatureType::Eoa => WalletRoute::Eoa,
        SignatureType::Proxy => WalletRoute::Proxy,
        SignatureType::Safe => WalletRoute::Safe,
    }
}

pub fn wallet_route_to_signature_type(wallet_route: WalletRoute) -> SignatureType {
    match wallet_route {
        WalletRoute::Eoa => SignatureType::Eoa,
        WalletRoute::Proxy => SignatureType::Proxy,
        WalletRoute::Safe => SignatureType::Safe,
    }
}

pub fn build_l2_auth_headers(headers: &L2AuthHeaders<'_>) -> Result<HeaderMap, AuthError> {
    let mut map = HeaderMap::new();
    insert_header(&mut map, POLY_ADDRESS.clone(), "address", headers.address)?;
    insert_header(&mut map, POLY_API_KEY.clone(), "api_key", headers.api_key)?;
    insert_header(
        &mut map,
        POLY_PASSPHRASE.clone(),
        "passphrase",
        headers.passphrase,
    )?;
    insert_header(
        &mut map,
        POLY_TIMESTAMP.clone(),
        "timestamp",
        headers.timestamp,
    )?;
    insert_header(
        &mut map,
        POLY_SIGNATURE.clone(),
        "signature",
        headers.signature,
    )?;
    insert_header(
        &mut map,
        POLY_SIGNATURE_TYPE.clone(),
        "signature_type",
        signature_type_label(headers.signature_type),
    )?;
    Ok(map)
}

fn insert_header(
    map: &mut HeaderMap,
    name: HeaderName,
    field: &'static str,
    value: &str,
) -> Result<(), AuthError> {
    if value.trim().is_empty() {
        return Err(AuthError::EmptyField(field));
    }

    let header_value =
        HeaderValue::from_str(value).map_err(|_| AuthError::InvalidHeaderValue(field))?;
    map.insert(name, header_value);
    Ok(())
}
