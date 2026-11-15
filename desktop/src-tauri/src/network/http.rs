use crate::market_domain::dates::epoch_millis;
use crate::network::powershell::powershell_http_get_bytes_with_headers;
use crate::network::proxy::apply_proxy_url;
use crate::network::proxy::proxy_from_payload;
use crate::network::proxy::validate_network_proxy_settings;
use crate::network::tls::apply_android_tls_backend;
use serde_json::json;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;

pub(crate) const TENCENT_CONNECT_TIMEOUT_SECS: u64 = 3;

pub(crate) const TENCENT_NETWORK_PROBE_TIMEOUT_SECS: u64 = 4;

pub(crate) const MAX_CACHED_HTTP_CLIENTS: usize = 16;

pub(crate) const LLM_MODEL_LIST_MAX_BYTES: usize = 2 * 1024 * 1024;

pub(crate) static HTTP_CLIENT_CACHE: OnceLock<Mutex<HashMap<HttpClientCacheKey, reqwest::Client>>> =
    OnceLock::new();

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct HttpClientCacheKey {
    pub(crate) user_agent: String,
    pub(crate) timeout_ms: u128,
    pub(crate) proxy: Option<String>,
    pub(crate) bypass_environment_proxy: bool,
}

pub(crate) async fn probe_mobile_url(
    client: &reqwest::Client,
    label: &'static str,
    url: &'static str,
    timeout: Duration,
) -> Value {
    let started_at = epoch_millis();
    let result = tokio::time::timeout(timeout, client.get(url).send()).await;
    match result {
        Err(_) => json!({
            "ok": false,
            "label": label,
            "stage": "timeout",
            "url": url,
            "timeout_seconds": timeout.as_secs(),
            "elapsed_ms": epoch_millis().saturating_sub(started_at),
            "error": format!("network probe timed out after {} seconds", timeout.as_secs())
        }),
        Ok(Err(error)) => json!({
            "ok": false,
            "label": label,
            "stage": "request",
            "url": url,
            "timeout_seconds": timeout.as_secs(),
            "elapsed_ms": epoch_millis().saturating_sub(started_at),
            "error": error.to_string()
        }),
        Ok(Ok(response)) => {
            let status = response.status();
            json!({
                "ok": status.is_success(),
                "label": label,
                "stage": "http",
                "url": url,
                "status": status.as_u16(),
                "timeout_seconds": timeout.as_secs(),
                "elapsed_ms": epoch_millis().saturating_sub(started_at),
                "error": if status.is_success() { String::new() } else { format!("HTTP {}", status.as_u16()) }
            })
        }
    }
}

pub(crate) fn build_tencent_http_client(
    user_agent: &str,
    timeout: Duration,
) -> Result<reqwest::Client, String> {
    build_http_client_with_proxy(user_agent, timeout, None)
}

pub(crate) fn build_direct_http_client(
    user_agent: &str,
    timeout: Duration,
) -> Result<reqwest::Client, String> {
    let builder = reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(TENCENT_CONNECT_TIMEOUT_SECS))
        .user_agent(user_agent)
        .no_proxy();
    apply_android_tls_backend(builder)?
        .build()
        .map_err(|error| format!("create direct HTTP client failed: {error}"))
}

pub(crate) fn build_http_client_with_proxy(
    user_agent: &str,
    timeout: Duration,
    payload: Option<&Value>,
) -> Result<reqwest::Client, String> {
    let has_network_proxy_settings = payload
        .and_then(|value| value.get("network_proxy_mode"))
        .is_some();
    if has_network_proxy_settings {
        validate_network_proxy_settings(payload)?;
    }
    let proxy = proxy_from_payload(payload);
    let key = HttpClientCacheKey {
        user_agent: user_agent.to_string(),
        timeout_ms: timeout.as_millis(),
        proxy: proxy.clone(),
        bypass_environment_proxy: has_network_proxy_settings,
    };
    if let Some(client) = http_client_cache()
        .lock()
        .map_err(|_| "HTTP client cache lock poisoned".to_string())?
        .get(&key)
        .cloned()
    {
        return Ok(client);
    }

    let client = configured_http_client_builder(
        user_agent,
        timeout,
        proxy.as_deref(),
        has_network_proxy_settings,
    )?
    .build()
    .map_err(|error| format!("create HTTP client failed: {error}"))?;

    let mut cache = http_client_cache()
        .lock()
        .map_err(|_| "HTTP client cache lock poisoned".to_string())?;
    if cache.len() >= MAX_CACHED_HTTP_CLIENTS {
        cache.clear();
    }
    Ok(cache.entry(key).or_insert_with(|| client.clone()).clone())
}

pub(crate) fn build_http_client_with_redirect_policy(
    user_agent: &str,
    timeout: Duration,
    payload: Option<&Value>,
    redirect_policy: reqwest::redirect::Policy,
) -> Result<reqwest::Client, String> {
    let has_network_proxy_settings = payload
        .and_then(|value| value.get("network_proxy_mode"))
        .is_some();
    if has_network_proxy_settings {
        validate_network_proxy_settings(payload)?;
    }
    let proxy = proxy_from_payload(payload);
    // Keep source-specific policies out of the shared, default-policy client cache.
    configured_http_client_builder(
        user_agent,
        timeout,
        proxy.as_deref(),
        has_network_proxy_settings,
    )?
    .redirect(redirect_policy)
    .build()
    .map_err(|error| format!("create HTTP client failed: {error}"))
}

fn configured_http_client_builder(
    user_agent: &str,
    timeout: Duration,
    proxy: Option<&str>,
    bypass_environment_proxy: bool,
) -> Result<reqwest::ClientBuilder, String> {
    let builder = reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(TENCENT_CONNECT_TIMEOUT_SECS))
        .user_agent(user_agent);
    let builder = if bypass_environment_proxy {
        // Scoped settings are authoritative, including "direct" and resolved "system" proxies.
        builder.no_proxy()
    } else {
        builder
    };
    apply_proxy_url(apply_android_tls_backend(builder)?, proxy)
}

pub(crate) fn http_client_cache() -> &'static Mutex<HashMap<HttpClientCacheKey, reqwest::Client>> {
    HTTP_CLIENT_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn decode_utf8_lossy(bytes: Vec<u8>) -> String {
    match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => String::from_utf8_lossy(&error.into_bytes()).to_string(),
    }
}

pub(crate) async fn http_get_text_with_headers_first(
    client: &reqwest::Client,
    url: &str,
    timeout_secs: u64,
    user_agent: &str,
    referer: &str,
) -> Result<String, String> {
    let fetch = async {
        let powershell_url = url.to_string();
        let powershell_user_agent = user_agent.to_string();
        let powershell_referer = referer.to_string();
        let powershell_result =
            crate::runtime::run_transport_bound("market blocking transport", move || {
                powershell_http_get_bytes_with_headers(
                    &powershell_url,
                    timeout_secs,
                    &powershell_user_agent,
                    &powershell_referer,
                )
            })
            .await;
        let powershell_error = match powershell_result {
            Ok(Ok(bytes)) => return Ok(decode_utf8_lossy(bytes)),
            Ok(Err(error)) => error,
            Err(error) => format!("PowerShell HTTP task failed: {error}"),
        };
        match client
            .get(url)
            .header("User-Agent", user_agent)
            .header("Referer", referer)
            .header("Accept", "text/html,application/json,text/plain,*/*")
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                response.text().await.map_err(|error| error.to_string())
            }
            Ok(response) => Err(format!(
                "PowerShell: {powershell_error}; reqwest HTTP {}",
                response.status().as_u16()
            )),
            Err(error) => Err(format!("PowerShell: {powershell_error}; reqwest: {error}")),
        }
    };
    match tokio::time::timeout(Duration::from_secs(timeout_secs), fetch).await {
        Ok(result) => result,
        Err(_) => Err(format!(
            "PowerShell/reqwest HTTP request timed out after {timeout_secs} seconds"
        )),
    }
}
