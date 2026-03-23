use std::fmt;

use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use url::Url;

#[derive(Debug, Clone)]
pub struct PolymarketRestClient {
    http: Client,
    pub clob_host: Url,
    pub data_api_host: Url,
    pub relayer_host: Url,
}

#[derive(Debug)]
pub enum RestError {
    Http(reqwest::Error),
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

impl fmt::Display for RestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(err) => write!(f, "http error: {err}"),
            Self::Url(err) => write!(f, "url error: {err}"),
            Self::MissingField(field) => write!(f, "missing response field: {field}"),
        }
    }
}

impl std::error::Error for RestError {}

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
    pub fn new(clob_host: Url, data_api_host: Url, relayer_host: Url) -> Self {
        Self::with_http_client(Client::new(), clob_host, data_api_host, relayer_host)
    }

    pub fn with_http_client(
        http: Client,
        clob_host: Url,
        data_api_host: Url,
        relayer_host: Url,
    ) -> Self {
        Self {
            http,
            clob_host,
            data_api_host,
            relayer_host,
        }
    }

    pub async fn fetch_clob_status(&self) -> Result<VenueStatusResponse, RestError> {
        self.get_clob("status", &[]).await
    }

    pub(crate) async fn get_relayer<T>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T, RestError>
    where
        T: DeserializeOwned,
    {
        self.get_json(&self.relayer_host, path, query).await
    }

    async fn get_clob<T>(&self, path: &str, query: &[(&str, &str)]) -> Result<T, RestError>
    where
        T: DeserializeOwned,
    {
        self.get_json(&self.clob_host, path, query).await
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
        let url = join_url(base, path, query)?;
        let response = self.http.get(url).send().await?.error_for_status()?;
        Ok(response.json::<T>().await?)
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
