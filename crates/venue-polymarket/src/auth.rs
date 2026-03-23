use std::fmt;

use domain::{SignatureType, WalletRoute};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

const POLY_ADDRESS: HeaderName = HeaderName::from_static("poly-address");
const POLY_API_KEY: HeaderName = HeaderName::from_static("poly-api-key");
const POLY_BUILDER_API_KEY: HeaderName = HeaderName::from_static("poly-builder-api-key");
const POLY_BUILDER_PASSPHRASE: HeaderName = HeaderName::from_static("poly-builder-passphrase");
const POLY_BUILDER_SIGNATURE: HeaderName = HeaderName::from_static("poly-builder-signature");
const POLY_BUILDER_TIMESTAMP: HeaderName = HeaderName::from_static("poly-builder-timestamp");
const POLY_PASSPHRASE: HeaderName = HeaderName::from_static("poly-passphrase");
const POLY_SIGNATURE: HeaderName = HeaderName::from_static("poly-signature");
const POLY_SIGNATURE_TYPE: HeaderName = HeaderName::from_static("poly-signature-type");
const POLY_TIMESTAMP: HeaderName = HeaderName::from_static("poly-timestamp");
const RELAYER_API_KEY: HeaderName = HeaderName::from_static("relayer-api-key");
const RELAYER_API_KEY_ADDRESS: HeaderName = HeaderName::from_static("relayer-api-key-address");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignerContext<'a> {
    pub address: &'a str,
    pub funder_address: &'a str,
    pub signature_type: SignatureType,
    pub wallet_route: WalletRoute,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct L2AuthHeaders<'a> {
    pub signer: SignerContext<'a>,
    pub api_key: &'a str,
    pub passphrase: &'a str,
    pub timestamp: &'a str,
    pub signature: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayerAuth<'a> {
    BuilderApiKey {
        api_key: &'a str,
        timestamp: &'a str,
        passphrase: &'a str,
        signature: &'a str,
    },
    RelayerApiKey {
        api_key: &'a str,
        address: &'a str,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    EmptyField(&'static str),
    InvalidHeaderValue(&'static str),
    SignerMismatch,
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyField(name) => write!(f, "missing auth field: {name}"),
            Self::InvalidHeaderValue(name) => write!(f, "invalid auth header value: {name}"),
            Self::SignerMismatch => write!(f, "signature_type and wallet_route disagree"),
        }
    }
}

impl std::error::Error for AuthError {}

pub fn signature_type_label(signature_type: SignatureType) -> &'static str {
    match signature_type {
        SignatureType::Eoa => "EOA",
        SignatureType::Proxy => "POLY_PROXY",
        SignatureType::Safe => "GNOSIS_SAFE",
    }
}

pub fn signature_type_to_wallet_route(signature_type: SignatureType) -> WalletRoute {
    match signature_type {
        SignatureType::Eoa => WalletRoute::Eoa,
        SignatureType::Proxy => WalletRoute::Proxy,
        SignatureType::Safe => WalletRoute::Safe,
    }
}

pub fn wallet_route_label(wallet_route: WalletRoute) -> &'static str {
    match wallet_route {
        WalletRoute::Eoa => "eoa",
        WalletRoute::Proxy => "proxy",
        WalletRoute::Safe => "safe",
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
    if wallet_route_to_signature_type(headers.signer.wallet_route) != headers.signer.signature_type
    {
        return Err(AuthError::SignerMismatch);
    }

    ensure_field("address", headers.signer.address)?;
    ensure_field("funder_address", headers.signer.funder_address)?;

    let mut map = HeaderMap::new();
    insert_header(
        &mut map,
        POLY_ADDRESS.clone(),
        "address",
        headers.signer.address,
    )?;
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
        signature_type_label(headers.signer.signature_type),
    )?;
    Ok(map)
}

pub fn build_relayer_auth_headers(auth: &RelayerAuth<'_>) -> Result<HeaderMap, AuthError> {
    let mut map = HeaderMap::new();

    match auth {
        RelayerAuth::BuilderApiKey {
            api_key,
            timestamp,
            passphrase,
            signature,
        } => {
            insert_header(
                &mut map,
                POLY_BUILDER_API_KEY.clone(),
                "builder_api_key",
                api_key,
            )?;
            insert_header(
                &mut map,
                POLY_BUILDER_TIMESTAMP.clone(),
                "builder_timestamp",
                timestamp,
            )?;
            insert_header(
                &mut map,
                POLY_BUILDER_PASSPHRASE.clone(),
                "builder_passphrase",
                passphrase,
            )?;
            insert_header(
                &mut map,
                POLY_BUILDER_SIGNATURE.clone(),
                "builder_signature",
                signature,
            )?;
        }
        RelayerAuth::RelayerApiKey { api_key, address } => {
            insert_header(
                &mut map,
                RELAYER_API_KEY.clone(),
                "relayer_api_key",
                api_key,
            )?;
            insert_header(
                &mut map,
                RELAYER_API_KEY_ADDRESS.clone(),
                "relayer_api_key_address",
                address,
            )?;
        }
    }

    Ok(map)
}

fn insert_header(
    map: &mut HeaderMap,
    name: HeaderName,
    field: &'static str,
    value: &str,
) -> Result<(), AuthError> {
    ensure_field(field, value)?;

    let header_value =
        HeaderValue::from_str(value).map_err(|_| AuthError::InvalidHeaderValue(field))?;
    map.insert(name, header_value);
    Ok(())
}

fn ensure_field(field: &'static str, value: &str) -> Result<(), AuthError> {
    if value.trim().is_empty() {
        return Err(AuthError::EmptyField(field));
    }

    Ok(())
}
