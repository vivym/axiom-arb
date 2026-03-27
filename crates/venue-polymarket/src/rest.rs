use std::{
    fmt,
    sync::{Arc, Mutex},
};

use reqwest::header::HeaderMap;
use reqwest::{Client, Request, Response, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex as AsyncMutex;
use url::Url;

use crate::heartbeat::HeartbeatFetchResult;
use crate::instrumentation::VenueProducerInstrumentation;
use crate::metadata::{NegRiskMetadataCache, NegRiskMetadataError};
use crate::orders::PostOrderRequest;
use crate::{
    build_l2_auth_headers, signature_type_label, wallet_route_label, AuthError, L2AuthHeaders,
};

#[derive(Debug, Clone)]
pub struct PolymarketRestClient {
    http: Client,
    pub clob_host: Url,
    pub data_api_host: Url,
    pub relayer_host: Url,
    pub(crate) metadata_state: Arc<Mutex<NegRiskMetadataCache>>,
    pub(crate) metadata_refresh_lock: Arc<AsyncMutex<()>>,
    pub(crate) instrumentation: VenueProducerInstrumentation,
}

#[derive(Debug)]
pub enum RestError {
    Auth(AuthError),
    Http(reqwest::Error),
    HttpResponse { status: StatusCode, body: String },
    Metadata(NegRiskMetadataError),
    Url(url::ParseError),
    MissingField(&'static str),
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct VenueStatusResponse {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub trading_status: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct OpenOrderSummary {
    #[serde(alias = "id")]
    pub order_id: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub market: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct BalanceAllowanceResponse {
    #[serde(default, alias = "asset")]
    pub asset_id: Option<String>,
    #[serde(default)]
    pub balance: Option<String>,
    #[serde(default)]
    pub allowance: Option<String>,
    #[serde(default)]
    pub spender: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct HeartbeatResponse {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    heartbeat_id: Option<String>,
}

impl fmt::Display for RestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auth(err) => write!(f, "auth error: {err}"),
            Self::Http(err) => write!(f, "http error: {err}"),
            Self::HttpResponse { status, body } => {
                write!(f, "http response error {status}: {body}")
            }
            Self::Metadata(err) => write!(f, "metadata error: {err}"),
            Self::Url(err) => write!(f, "url error: {err}"),
            Self::MissingField(field) => write!(f, "missing response field: {field}"),
        }
    }
}

impl std::error::Error for RestError {}

impl From<AuthError> for RestError {
    fn from(value: AuthError) -> Self {
        Self::Auth(value)
    }
}

impl From<reqwest::Error> for RestError {
    fn from(value: reqwest::Error) -> Self {
        Self::Http(value)
    }
}

impl From<url::ParseError> for RestError {
    fn from(value: url::ParseError) -> Self {
        Self::Url(value)
    }
}

impl PolymarketRestClient {
    pub fn new(
        clob_host: Url,
        data_api_host: Url,
        relayer_host: Url,
        instrumentation: Option<VenueProducerInstrumentation>,
    ) -> Self {
        Self::with_http_client(
            Client::new(),
            clob_host,
            data_api_host,
            relayer_host,
            instrumentation,
        )
    }

    pub fn with_http_client(
        http: Client,
        clob_host: Url,
        data_api_host: Url,
        relayer_host: Url,
        instrumentation: Option<VenueProducerInstrumentation>,
    ) -> Self {
        Self {
            http,
            clob_host,
            data_api_host,
            relayer_host,
            metadata_state: Arc::new(Mutex::new(NegRiskMetadataCache::default())),
            metadata_refresh_lock: Arc::new(AsyncMutex::new(())),
            instrumentation: instrumentation.unwrap_or_else(VenueProducerInstrumentation::disabled),
        }
    }

    pub async fn fetch_clob_status(&self) -> Result<VenueStatusResponse, RestError> {
        self.get_clob("status", &[]).await
    }

    pub fn build_post_heartbeat_request(
        &self,
        auth: &L2AuthHeaders<'_>,
        previous_heartbeat_id: &str,
    ) -> Result<reqwest::Request, RestError> {
        let headers = build_l2_auth_headers(auth)?;
        let url = join_url(&self.clob_host, "heartbeat", &[])?;

        Ok(self
            .http
            .post(url)
            .headers(headers)
            .json(&HeartbeatPostRequest {
                heartbeat_id: previous_heartbeat_id,
            })
            .build()?)
    }

    pub async fn post_order_heartbeat(
        &self,
        auth: &L2AuthHeaders<'_>,
        previous_heartbeat_id: &str,
    ) -> Result<HeartbeatFetchResult, RestError> {
        let request = self.build_post_heartbeat_request(auth, previous_heartbeat_id)?;
        let response = self.http.execute(request).await?;
        let status = response.status();

        if status.is_success() {
            let heartbeat: HeartbeatResponse = response.json().await?;
            return Ok(map_heartbeat_response(heartbeat));
        }

        let body = response.text().await?;
        if let Ok(heartbeat) = serde_json::from_str::<HeartbeatResponse>(&body) {
            if heartbeat.heartbeat_id.is_some() {
                return Ok(map_heartbeat_response(heartbeat));
            }
        }

        Err(RestError::HttpResponse { status, body })
    }

    pub fn build_open_orders_request(
        &self,
        auth: &L2AuthHeaders<'_>,
    ) -> Result<reqwest::Request, RestError> {
        self.build_authenticated_get_request(&self.clob_host, "orders", &[], auth)
    }

    pub async fn fetch_open_orders(
        &self,
        auth: &L2AuthHeaders<'_>,
    ) -> Result<Vec<OpenOrderSummary>, RestError> {
        let request = self.build_open_orders_request(auth)?;
        self.execute_json(request).await
    }

    pub fn build_balance_allowance_request(
        &self,
        auth: &L2AuthHeaders<'_>,
        asset_id: &str,
    ) -> Result<reqwest::Request, RestError> {
        self.build_authenticated_get_request(
            &self.clob_host,
            "balance-allowance",
            &[("asset", asset_id)],
            auth,
        )
    }

    pub async fn fetch_balance_allowance(
        &self,
        auth: &L2AuthHeaders<'_>,
        asset_id: &str,
    ) -> Result<BalanceAllowanceResponse, RestError> {
        let request = self.build_balance_allowance_request(auth, asset_id)?;
        self.execute_json(request).await
    }

    pub fn build_submit_order_request(
        &self,
        auth: &L2AuthHeaders<'_>,
        submission: &PostOrderRequest,
    ) -> Result<reqwest::Request, RestError> {
        let headers = build_l2_auth_headers(auth)?;
        let url = join_url(&self.clob_host, "order", &[])?;

        Ok(self
            .http
            .post(url)
            .headers(headers)
            .json(submission)
            .build()?)
    }

    async fn get_clob<T>(&self, path: &str, query: &[(&str, &str)]) -> Result<T, RestError>
    where
        T: DeserializeOwned,
    {
        self.get_json(&self.clob_host, path, query).await
    }

    fn build_authenticated_get_request(
        &self,
        base: &Url,
        path: &str,
        extra_query: &[(&str, &str)],
        auth: &L2AuthHeaders<'_>,
    ) -> Result<reqwest::Request, RestError> {
        let headers = build_l2_auth_headers(auth)?;
        let mut query = signer_query(auth);
        query.extend_from_slice(extra_query);
        self.build_get_request(base, path, &query, Some(headers))
    }

    async fn get_json<T>(
        &self,
        base: &Url,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T, RestError>
    where
        T: DeserializeOwned,
    {
        let request = self.build_get_request(base, path, query, None)?;
        self.execute_json(request).await
    }

    pub(crate) fn build_get_request(
        &self,
        base: &Url,
        path: &str,
        query: &[(&str, &str)],
        headers: Option<HeaderMap>,
    ) -> Result<Request, RestError> {
        let url = join_url(base, path, query)?;
        let builder = self.http.get(url);
        let builder = match headers {
            Some(headers) => builder.headers(headers),
            None => builder,
        };

        Ok(builder.build()?)
    }

    pub(crate) async fn execute_json<T>(&self, request: Request) -> Result<T, RestError>
    where
        T: DeserializeOwned,
    {
        let response = self.execute(request).await?;
        Ok(response.json::<T>().await?)
    }

    async fn execute(&self, request: Request) -> Result<Response, RestError> {
        let response = self.http.execute(request).await?;
        let status = response.status();

        if status.is_success() {
            return Ok(response);
        }

        let body = response.text().await?;
        Err(RestError::HttpResponse { status, body })
    }
}

#[derive(Debug, Serialize)]
struct HeartbeatPostRequest<'a> {
    heartbeat_id: &'a str,
}

fn map_heartbeat_response(heartbeat: HeartbeatResponse) -> HeartbeatFetchResult {
    let valid = heartbeat.success && heartbeat.heartbeat_id.is_some();
    HeartbeatFetchResult {
        heartbeat_id: heartbeat.heartbeat_id.unwrap_or_default(),
        valid,
    }
}

fn join_url(base: &Url, path: &str, query: &[(&str, &str)]) -> Result<Url, RestError> {
    let trimmed = path.trim_start_matches('/');
    let mut url = base.join(trimmed)?;

    if !query.is_empty() {
        let mut pairs = url.query_pairs_mut();
        for (key, value) in query {
            pairs.append_pair(key, value);
        }
    }

    Ok(url)
}

fn signer_query<'a>(auth: &'a L2AuthHeaders<'a>) -> Vec<(&'a str, &'a str)> {
    vec![
        ("owner", auth.signer.address),
        ("funder", auth.signer.funder_address),
        (
            "signature_type",
            signature_type_label(auth.signer.signature_type),
        ),
        ("wallet_route", wallet_route_label(auth.signer.wallet_route)),
    ]
}
