use url::Url;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProxyEnvironment {
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
    pub all_proxy: Option<String>,
    pub no_proxy: Option<String>,
}

impl ProxyEnvironment {
    pub(crate) fn from_env() -> Self {
        Self {
            http_proxy: read_env_any(&["HTTP_PROXY", "http_proxy"]),
            https_proxy: read_env_any(&["HTTPS_PROXY", "https_proxy"]),
            all_proxy: read_env_any(&["ALL_PROXY", "all_proxy"]),
            no_proxy: read_env_any(&["NO_PROXY", "no_proxy"]),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyConfigError {
    message: String,
}

impl ProxyConfigError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ProxyConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ProxyConfigError {}

pub(crate) fn resolve_proxy_url(
    target: &Url,
    explicit_proxy_url: Option<&Url>,
    env: &ProxyEnvironment,
) -> Result<Option<Url>, ProxyConfigError> {
    if let Some(proxy_url) = explicit_proxy_url {
        validate_proxy_url(proxy_url)?;
        return Ok(Some(proxy_url.clone()));
    }

    if env
        .no_proxy
        .as_deref()
        .is_some_and(|value| host_matches_no_proxy(target, value))
    {
        return Ok(None);
    }

    let raw_proxy = match target.scheme() {
        "https" | "wss" => env.https_proxy.as_deref().or(env.all_proxy.as_deref()),
        "http" | "ws" => env.http_proxy.as_deref().or(env.all_proxy.as_deref()),
        _ => None,
    };

    match raw_proxy {
        Some(raw_proxy) => {
            let proxy_url = Url::parse(raw_proxy).map_err(|error| {
                ProxyConfigError::new(format!("invalid proxy URL {raw_proxy:?}: {error}"))
            })?;
            validate_proxy_url(&proxy_url)?;
            Ok(Some(proxy_url))
        }
        None => Ok(None),
    }
}

fn read_env_any(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        std::env::var(key)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    })
}

fn validate_proxy_url(proxy_url: &Url) -> Result<(), ProxyConfigError> {
    if proxy_url.scheme() != "http" {
        return Err(ProxyConfigError::new(format!(
            "unsupported proxy URL scheme '{}'; expected http",
            proxy_url.scheme()
        )));
    }
    if proxy_url.host_str().is_none() {
        return Err(ProxyConfigError::new(
            "proxy URL must include a host".to_owned(),
        ));
    }
    if proxy_url.query().is_some() || proxy_url.fragment().is_some() {
        return Err(ProxyConfigError::new(
            "proxy URL must not include a query or fragment".to_owned(),
        ));
    }
    Ok(())
}

fn host_matches_no_proxy(target: &Url, no_proxy: &str) -> bool {
    let Some(host) = target.host_str() else {
        return false;
    };

    let host = host.trim_matches('.').to_ascii_lowercase();
    no_proxy
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .any(|entry| {
            if entry == "*" {
                return true;
            }

            let candidate = entry
                .trim_matches('.')
                .split(':')
                .next()
                .unwrap_or(entry)
                .trim_matches('.')
                .to_ascii_lowercase();

            host == candidate || host.ends_with(&format!(".{candidate}"))
        })
}
