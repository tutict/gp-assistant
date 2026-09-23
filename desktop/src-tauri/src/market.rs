use base64::{engine::general_purpose, Engine as _};
use futures::stream::{self, StreamExt};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
#[cfg(not(mobile))]
use std::process::Command;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use stock_optimizer_core as gp_core;
use tauri::{Emitter, Manager};


pub(crate) const MOBILE_MARKET_DATA_FILE: &str = "mobile-market-data.json";

pub(crate) const MOBILE_MARKET_PATCH_DIR: &str = "mobile-market-data-patches";

pub(crate) const MOBILE_MARKET_WRITE_RETRY_ATTEMPTS: usize = 3;

pub(crate) const MOBILE_MARKET_WRITE_RETRY_DELAY_MS: u64 = 50;

pub(crate) const TENCENT_QUOTE_ENDPOINT: &str = "https://qt.gtimg.cn/q=";

pub(crate) const EASTMONEY_KLINE_ENDPOINT: &str = "https://push2his.eastmoney.com/api/qt/stock/kline/get";

pub(crate) const EASTMONEY_DATACENTER_ENDPOINT: &str = "https://datacenter-web.eastmoney.com/api/data/v1/get";

pub(crate) const EASTMONEY_SECURITIES_ENDPOINT: &str =
    "https://datacenter.eastmoney.com/securities/api/data/v1/get";

pub(crate) const TENCENT_DAILY_KLINE_ENDPOINT: &str = "https://web.ifzq.gtimg.cn/appstock/app/fqkline/get";

pub(crate) const TENCENT_BATCH_SIZE: usize = 120;

pub(crate) const TENCENT_FETCH_CONCURRENCY: usize = 12;

pub(crate) const TENCENT_DEFAULT_MAX_CANDIDATES: usize = 8_000;

pub(crate) const TENCENT_DEFAULT_MAX_FAILED_BATCHES: usize = 4;

pub(crate) const TENCENT_CONNECT_TIMEOUT_SECS: u64 = 3;

pub(crate) const TENCENT_REQUEST_TIMEOUT_SECS: u64 = 6;

pub(crate) const TENCENT_BATCH_TIMEOUT_SECS: u64 = 8;

pub(crate) const TENCENT_NETWORK_PROBE_TIMEOUT_SECS: u64 = 4;

pub(crate) const OBSERVE_FUNDAMENTAL_PREFERRED_TIMEOUT_SECS: u64 = 3;

pub(crate) const OBSERVE_HISTORY_TIMEOUT_SECS: u64 = 8;

// Tencent fqkline rejects very large count values with "param error".
pub(crate) const OBSERVE_DAILY_HISTORY_LIMIT: usize = 2_000;

pub(crate) const FINANCIAL_REQUEST_TIMEOUT_SECS: u64 = 6;

pub(crate) const MAX_TENCENT_WEBVIEW_QUOTE_BYTES: usize = 1_048_576;

pub(crate) const COMPLETE_QUARTERLY_EPS_POINTS: usize = 8;

pub(crate) const MAX_CACHED_HTTP_CLIENTS: usize = 16;

pub(crate) const LLM_MODEL_LIST_MAX_BYTES: usize = 2 * 1024 * 1024;

pub(crate) const THS_FINANCIAL_ENDPOINT: &str =
    "https://basic.10jqka.com.cn/basicapi/finance/index/v1/app_data/";

pub(crate) const SINA_FINANCIAL_GUIDELINE_ENDPOINT: &str =
    "https://money.finance.sina.com.cn/corp/go.php/vFD_FinancialGuideLine";

pub(crate) const SCREEN_STOCK_FINANCIAL_FIELDS: [&str; 5] = [
    "deducted_net_profit_billion",
    "deducted_net_profit_margin",
    "deducted_net_profit_growth_rate",
    "latest_eps",
    "latest_bps",
];

pub(crate) static REFRESH_SEED_CACHE: OnceLock<Mutex<HashMap<PathBuf, Value>>> = OnceLock::new();

pub(crate) static REFRESH_FINANCIAL_SNAPSHOT_CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<Value>>>> =
    OnceLock::new();

pub(crate) static MOBILE_MARKET_DATA_CACHE: OnceLock<Mutex<HashMap<PathBuf, MobileMarketDataCacheEntry>>> =
    OnceLock::new();

pub(crate) static SCREEN_STOCK_OVERLAY_CACHE: OnceLock<Mutex<HashMap<PathBuf, ScreenStockOverlayCacheEntry>>> =
    OnceLock::new();

pub(crate) static MOBILE_MARKET_UPDATE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub(crate) static HTTP_CLIENT_CACHE: OnceLock<Mutex<HashMap<HttpClientCacheKey, reqwest::Client>>> =
    OnceLock::new();

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct HttpClientCacheKey {
    pub(crate) user_agent: String,
    pub(crate) timeout_ms: u128,
    pub(crate) proxy: Option<String>,
}

#[derive(Clone)]
pub(crate) struct MobileMarketDataCacheEntry {
    pub(crate) bytes: u64,
    pub(crate) modified_at_epoch_ms: Option<u128>,
    pub(crate) data: Arc<Value>,
    pub(crate) typed: Arc<gp_core::CoreDataSet>,
    pub(crate) summary: Value,
}

#[derive(Clone)]
pub(crate) struct ScreenStockOverlayCacheEntry {
    pub(crate) data: Arc<gp_core::CoreDataSet>,
    pub(crate) financial_snapshot: Arc<Value>,
    pub(crate) stocks: Arc<Vec<gp_core::StockItem>>,
}

pub(crate) async fn fetch_eastmoney_public_json(
    client: &reqwest::Client,
    url: &str,
    label: &str,
    empty_is_valid: bool,
) -> Result<Value, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("{label} request failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("{label} returned HTTP {}", status.as_u16()));
    }
    let text = response
        .text()
        .await
        .map_err(|error| format!("{label} response read failed: {error}"))?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| format!("{label} JSON parse failed: {error}"))?;
    normalize_eastmoney_public_json(value, label, empty_is_valid)
}

pub(crate) async fn fetch_eastmoney_public_json_with_direct_retry(
    preferred_client: &reqwest::Client,
    direct_client: &reqwest::Client,
    url: &str,
    label: &str,
    empty_is_valid: bool,
) -> Result<Value, String> {
    let preferred_error = match tokio::time::timeout(
        Duration::from_secs(OBSERVE_FUNDAMENTAL_PREFERRED_TIMEOUT_SECS),
        fetch_eastmoney_public_json(preferred_client, url, label, empty_is_valid),
    )
    .await
    {
        Ok(Ok(value)) => return Ok(value),
        Ok(Err(error)) => error,
        Err(_) => format!(
            "{label} preferred route timed out after {OBSERVE_FUNDAMENTAL_PREFERRED_TIMEOUT_SECS}s"
        ),
    };
    fetch_eastmoney_public_json(direct_client, url, label, empty_is_valid)
        .await
        .map_err(|direct_error| format!("{preferred_error}; direct retry failed: {direct_error}"))
}

pub(crate) fn normalize_eastmoney_public_json(
    mut value: Value,
    label: &str,
    empty_is_valid: bool,
) -> Result<Value, String> {
    if value.get("success").and_then(Value::as_bool) == Some(false) {
        let empty_response = value.get("code").and_then(Value::as_i64) == Some(9201);
        if empty_is_valid && empty_response {
            value["result"] = json!({"data": []});
            value["success"] = json!(true);
            return Ok(value);
        }
        return Err(format!(
            "{label} rejected the request: {}",
            value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
        ));
    }
    if value
        .get("result")
        .and_then(|result| result.get("data"))
        .and_then(Value::as_array)
        .is_none()
    {
        return Err(format!("{label} response did not contain result.data"));
    }
    Ok(value)
}

pub(crate) fn eastmoney_result_rows(value: &Value) -> &[Value] {
    value
        .get("result")
        .and_then(|result| result.get("data"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

pub(crate) fn eastmoney_metric_period(
    row: Option<&serde_json::Map<String, Value>>,
    fields: &[&str],
) -> Option<String> {
    fields.iter().find_map(|field| {
        row.and_then(|row| row.get(*field))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.chars().take(10).collect())
    })
}

#[tauri::command]
pub(crate) fn api_market_status(app: tauri::AppHandle) -> Result<Value, String> {
    market_data_status(&app)
}

#[tauri::command]
pub(crate) async fn api_market_refresh(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    core_mobile_market_data_refresh_tencent(app, payload).await
}

#[tauri::command]
pub(crate) fn api_market_ingest_tencent_quotes(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    core_mobile_market_data_ingest_tencent_quotes(app, payload)
}

#[tauri::command]
pub(crate) fn api_market_clear_cache(app: tauri::AppHandle) -> Result<Value, String> {
    let cleared = core_mobile_market_data_clear(app.clone())?;
    Ok(json!({
        "removed_files": if cleared.get("removed").and_then(Value::as_bool).unwrap_or(false) { 1 } else { 0 },
        "removed_bytes": cleared.get("removed_bytes").and_then(Value::as_u64).unwrap_or(0),
        "status": market_data_status(&app)?,
        "notes": cleared.get("notes").cloned().unwrap_or_else(|| json!([]))
    }))
}

pub(crate) fn adaptive_benchmark_codes() -> [&'static str; 3] {
    ["000001.SH", "399001.SZ", "399006.SZ"]
}

#[tauri::command]
pub(crate) fn api_stock_search(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let query = payload
        .get("q")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_lowercase();
    let limit = payload_usize_field(&payload, "limit", 8, 1, 20);
    if query.is_empty() {
        return Ok(json!([]));
    }
    let data = cached_market_data(&app)?;
    let items = data
        .get("stocks")
        .and_then(Value::as_array)
        .map(|stocks| {
            stocks
                .iter()
                .filter(|stock| {
                    ["code", "name", "industry"]
                        .iter()
                        .filter_map(|key| stock.get(*key).and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join(" ")
                        .to_lowercase()
                        .contains(&query)
                })
                .take(limit)
                .cloned()
                .collect::<Vec<Value>>()
        })
        .unwrap_or_default();
    Ok(Value::Array(items))
}

#[tauri::command]
pub(crate) fn api_data_sources(app: tauri::AppHandle) -> Result<Value, String> {
    Ok(json!({
        "current": "tauri",
        "available": [{"id": "tauri", "name": "Tauri/Rust", "description": "Tauri/Rust native market cache and Tencent refresh path."}],
        "status": market_data_status(&app)?
    }))
}

#[tauri::command]
pub(crate) fn api_stock_get(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let code = payload
        .get("code")
        .and_then(Value::as_str)
        .and_then(normalize_stock_code)
        .ok_or_else(|| "code is required".to_string())?;
    let data = cached_market_data(&app)?;
    data.get("stocks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|stock| {
            stock
                .get("code")
                .and_then(Value::as_str)
                .and_then(normalize_stock_code)
                .as_deref()
                == Some(code.as_str())
        })
        .cloned()
        .ok_or_else(|| "stock not found in Tauri/Rust cache".to_string())
}

#[tauri::command]
pub(crate) fn api_minutes(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let code = payload
        .get("code")
        .and_then(Value::as_str)
        .and_then(normalize_stock_code)
        .ok_or_else(|| "code is required".to_string())?;
    let limit = payload_usize_field(&payload, "limit", 500, 1, 500);
    let data = cached_market_data(&app)?;
    let history = data
        .get("histories")
        .and_then(Value::as_object)
        .and_then(|items| items.get(&code))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let rows = history.into_iter().rev().take(limit).map(|bar| json!({
        "datetime": bar.get("date").or_else(|| bar.get("datetime")).and_then(Value::as_str).unwrap_or(""),
        "open": bar.get("open").and_then(Value::as_f64).unwrap_or(0.0),
        "high": bar.get("high").and_then(Value::as_f64).unwrap_or(0.0),
        "low": bar.get("low").and_then(Value::as_f64).unwrap_or(0.0),
        "close": bar.get("close").and_then(Value::as_f64).unwrap_or(0.0),
        "volume": bar.get("volume").cloned().unwrap_or(Value::Null),
        "amount": bar.get("amount").cloned().unwrap_or(Value::Null)
    })).collect::<Vec<_>>();
    Ok(Value::Array(rows.into_iter().rev().collect()))
}

#[tauri::command]
pub(crate) fn api_order_book(_app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let code = payload
        .get("code")
        .and_then(Value::as_str)
        .and_then(normalize_stock_code)
        .ok_or_else(|| "code is required".to_string())?;
    Ok(
        json!({"code": code, "timestamp": Value::Null, "bids": [], "asks": [], "metrics": {}, "notes": ["Standalone order book is replaced by Tauri/Rust observe payload when available; no level-2 book is cached locally."]}),
    )
}

#[tauri::command]
pub(crate) async fn core_validate_data_source(payload: Value) -> Result<Value, String> {
    crate::runtime::run_cpu_bound("core_validate_data_source", move || {
        gp_core::validate_data_source_value(payload).map_err(|error| error.to_string())
    })
    .await?
}

#[tauri::command]
pub(crate) fn core_mobile_market_data_read(app: tauri::AppHandle) -> Result<Value, String> {
    read_mobile_market_data(&app)
}

#[tauri::command]
pub(crate) fn core_mobile_market_data_write(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    write_mobile_market_data(&app, payload)
}

#[tauri::command]
pub(crate) fn core_mobile_market_data_clear(app: tauri::AppHandle) -> Result<Value, String> {
    let path = mobile_market_data_path(&app)?;
    if !path.exists() {
        clear_refresh_seed(&app);
        return Ok(json!({
            "removed": false,
            "removed_bytes": 0,
            "notes": ["mobile market cache is already empty"]
        }));
    }
    let removed_bytes = fs::metadata(&path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    fs::remove_file(&path).map_err(|error| {
        format!(
            "remove mobile market cache failed: {}: {error}",
            path.display()
        )
    })?;
    clear_refresh_seed(&app);
    Ok(json!({
        "removed": true,
        "removed_bytes": removed_bytes,
        "notes": ["mobile market cache removed"]
    }))
}

#[tauri::command]
pub(crate) async fn core_mobile_network_probe(payload: Option<Value>) -> Result<Value, String> {
    let http_timeout = Duration::from_secs(TENCENT_NETWORK_PROBE_TIMEOUT_SECS);
    let payload_ref = payload.as_ref();
    let proxy_mode = proxy_mode_from_payload(payload_ref);
    let proxy_url = proxy_from_payload(payload_ref);
    let client = build_http_client_with_proxy(
        "Mozilla/5.0 GuXuanYou/0.3 mobile probe",
        http_timeout,
        payload_ref,
    )?;
    let (baidu_probe, tencent_probe, eastmoney_probe, sina_probe, ths_probe) = futures::join!(
        probe_mobile_url(&client, "baidu_https", "https://www.baidu.com", http_timeout),
        probe_mobile_url(&client, "tencent_quote", "https://qt.gtimg.cn/q=sz000001", http_timeout),
        probe_mobile_url(
            &client,
            "eastmoney_guba",
            "https://guba.eastmoney.com/list,000100.html",
            http_timeout,
        ),
        probe_mobile_url(
            &client,
            "sina_stock_news",
            "https://vip.stock.finance.sina.com.cn/corp/go.php/vCB_AllNewsStock/symbol/sz000100.phtml",
            http_timeout,
        ),
        probe_mobile_url(
            &client,
            "ths_stock_news",
            "https://basic.10jqka.com.cn/000100/news.html",
            http_timeout,
        ),
    );
    let probes = vec![
        baidu_probe,
        tencent_probe,
        eastmoney_probe,
        sina_probe,
        ths_probe,
    ];

    let any_ok = probes
        .iter()
        .any(|probe| probe.get("ok").and_then(Value::as_bool).unwrap_or(false));
    let quote_ok = probes.iter().any(|probe| {
        probe.get("ok").and_then(Value::as_bool).unwrap_or(false)
            && probe.get("label").and_then(Value::as_str) == Some("tencent_quote")
    });
    Ok(json!({
        "ok": any_ok,
        "quote_ok": quote_ok,
        "timeout_seconds": http_timeout.as_secs(),
        "resolver": "system_dns",
        "proxy_mode": proxy_mode,
        "proxy_configured": proxy_url.is_some(),
        "probes": probes,
    }))
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
    let proxy = proxy_from_payload(payload);
    let key = HttpClientCacheKey {
        user_agent: user_agent.to_string(),
        timeout_ms: timeout.as_millis(),
        proxy: proxy.clone(),
    };
    if let Some(client) = http_client_cache()
        .lock()
        .map_err(|_| "HTTP client cache lock poisoned".to_string())?
        .get(&key)
        .cloned()
    {
        return Ok(client);
    }

    let builder = reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(TENCENT_CONNECT_TIMEOUT_SECS))
        .user_agent(user_agent);
    let builder = apply_android_tls_backend(builder)?;
    let client = apply_proxy_url(builder, proxy.as_deref())?
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

pub(crate) fn http_client_cache() -> &'static Mutex<HashMap<HttpClientCacheKey, reqwest::Client>> {
    HTTP_CLIENT_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(target_os = "android")]
pub(crate) fn apply_android_tls_backend(
    builder: reqwest::ClientBuilder,
) -> Result<reqwest::ClientBuilder, String> {
    let root_store = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let tls_config = rustls::ClientConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .map_err(|error| format!("create Android TLS config failed: {error}"))?
        .with_root_certificates(root_store)
        .with_no_client_auth();
    Ok(builder.tls_backend_preconfigured(tls_config))
}

#[cfg(not(target_os = "android"))]
pub(crate) fn apply_android_tls_backend(
    builder: reqwest::ClientBuilder,
) -> Result<reqwest::ClientBuilder, String> {
    Ok(builder)
}

pub(crate) fn apply_proxy_url(
    builder: reqwest::ClientBuilder,
    proxy: Option<&str>,
) -> Result<reqwest::ClientBuilder, String> {
    let Some(proxy) = proxy else {
        return Ok(builder);
    };
    let reqwest_proxy = reqwest::Proxy::all(proxy)
        .map_err(|error| format!("invalid proxy URL {proxy}: {error}"))?;
    Ok(builder.proxy(reqwest_proxy))
}

pub(crate) fn proxy_from_payload(payload: Option<&Value>) -> Option<String> {
    let payload = payload?;
    let mode = payload
        .get("proxy_mode")
        .and_then(Value::as_str)
        .unwrap_or("none")
        .trim()
        .to_ascii_lowercase();
    match mode.as_str() {
        "manual" => payload
            .get("proxy_url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(normalize_manual_proxy_url),
        "system" => std::env::var("HTTPS_PROXY")
            .or_else(|_| std::env::var("HTTP_PROXY"))
            .or_else(|_| std::env::var("ALL_PROXY"))
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        _ => None,
    }
}

pub(crate) fn normalize_manual_proxy_url(value: &str) -> String {
    #[cfg(target_os = "android")]
    {
        if let Ok(mut url) = reqwest::Url::parse(value) {
            let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
            if host == "127.0.0.1" || host == "localhost" || host == "::1" {
                if url.set_host(Some("10.0.2.2")).is_ok() {
                    return url.to_string();
                }
            }
        }
    }
    value.to_string()
}

pub(crate) fn proxy_mode_from_payload(payload: Option<&Value>) -> String {
    payload
        .and_then(|value| value.get("proxy_mode"))
        .and_then(Value::as_str)
        .unwrap_or("none")
        .trim()
        .to_ascii_lowercase()
}

#[cfg(windows)]
pub(crate) fn powershell_http_get_bytes(url: &str, timeout_secs: u64) -> Result<Vec<u8>, String> {
    let timeout_ms = timeout_secs.saturating_mul(1000).max(1000).to_string();
    let script = r#"& {
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$u = $env:GP_HTTP_URL
$timeoutMs = [int]$env:GP_HTTP_TIMEOUT_MS
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$request = [System.Net.WebRequest]::Create($u)
$request.Method = 'GET'
$request.Timeout = $timeoutMs
$request.ReadWriteTimeout = $timeoutMs
if ($request -is [System.Net.HttpWebRequest]) {
  $request.UserAgent = 'Mozilla/5.0 GuXuanYou/0.3 financial'
  $request.Accept = '*/*'
  $request.AutomaticDecompression = [Net.DecompressionMethods]::GZip -bor [Net.DecompressionMethods]::Deflate
}
$response = $request.GetResponse()
try {
  $stream = $response.GetResponseStream()
  $memory = New-Object System.IO.MemoryStream
  $stream.CopyTo($memory)
  [Convert]::ToBase64String($memory.ToArray())
} finally {
  if ($stream) { $stream.Dispose() }
  if ($response) { $response.Dispose() }
}
}"#;
    let mut command = Command::new("powershell.exe");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command
        .env("GP_HTTP_URL", url)
        .env("GP_HTTP_TIMEOUT_MS", timeout_ms.as_str())
        .args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output()
        .map_err(|error| format!("PowerShell HTTP 启动失败：{error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if !stderr.trim().is_empty() {
            stderr.trim()
        } else {
            stdout.trim()
        };
        return Err(format!(
            "PowerShell HTTP 失败：{}",
            truncate_for_note(detail, 240)
        ));
    }
    let encoded = String::from_utf8_lossy(&output.stdout);
    let encoded = encoded.trim();
    if encoded.is_empty() {
        return Err("PowerShell HTTP 返回空响应".to_string());
    }
    general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| format!("PowerShell HTTP 响应解码失败：{error}"))
}

#[cfg(not(windows))]
pub(crate) fn powershell_http_get_bytes(_url: &str, _timeout_secs: u64) -> Result<Vec<u8>, String> {
    Err("PowerShell HTTP fallback only runs on Windows".to_string())
}

#[cfg(windows)]
pub(crate) fn powershell_http_get_bytes_with_headers(
    url: &str,
    timeout_secs: u64,
    user_agent: &str,
    referer: &str,
) -> Result<Vec<u8>, String> {
    let timeout_ms = timeout_secs.saturating_mul(1000).max(1000).to_string();
    let script = r#"& {
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$u = $env:GP_HTTP_URL
$timeoutMs = [int]$env:GP_HTTP_TIMEOUT_MS
$ua = $env:GP_HTTP_UA
$referer = $env:GP_HTTP_REFERER
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$request = [System.Net.WebRequest]::Create($u)
$request.Method = 'GET'
$request.Timeout = $timeoutMs
$request.ReadWriteTimeout = $timeoutMs
if ($request -is [System.Net.HttpWebRequest]) {
  $request.UserAgent = $ua
  $request.Accept = 'text/html,application/json,text/plain,*/*'
  if ($referer) { $request.Referer = $referer }
  $request.AutomaticDecompression = [Net.DecompressionMethods]::GZip -bor [Net.DecompressionMethods]::Deflate
}
$response = $request.GetResponse()
try {
  $stream = $response.GetResponseStream()
  $memory = New-Object System.IO.MemoryStream
  $stream.CopyTo($memory)
  [Convert]::ToBase64String($memory.ToArray())
} finally {
  if ($stream) { $stream.Dispose() }
  if ($response) { $response.Dispose() }
}
}"#;
    let mut command = Command::new("powershell.exe");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command
        .env("GP_HTTP_URL", url)
        .env("GP_HTTP_TIMEOUT_MS", timeout_ms.as_str())
        .env("GP_HTTP_UA", user_agent)
        .env("GP_HTTP_REFERER", referer)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output()
        .map_err(|error| format!("PowerShell HTTP 启动失败：{error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if !stderr.trim().is_empty() {
            stderr.trim()
        } else {
            stdout.trim()
        };
        return Err(format!(
            "PowerShell HTTP 失败：{}",
            truncate_for_note(detail, 240)
        ));
    }
    let encoded = String::from_utf8_lossy(&output.stdout);
    let encoded = encoded.trim();
    if encoded.is_empty() {
        return Err("PowerShell HTTP 返回空响应".to_string());
    }
    general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| format!("PowerShell HTTP 响应解码失败：{error}"))
}

#[cfg(not(windows))]
pub(crate) fn powershell_http_get_bytes_with_headers(
    _url: &str,
    _timeout_secs: u64,
    _user_agent: &str,
    _referer: &str,
) -> Result<Vec<u8>, String> {
    Err("PowerShell HTTP fallback only runs on Windows".to_string())
}

pub(crate) fn decode_utf8_lossy(bytes: Vec<u8>) -> String {
    match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => String::from_utf8_lossy(&error.into_bytes()).to_string(),
    }
}

pub(crate) fn payload_usize_field(value: &Value, key: &str, default: usize, min: usize, max: usize) -> usize {
    let parsed = value.get(key).and_then(|field| {
        field
            .as_u64()
            .and_then(|number| usize::try_from(number).ok())
            .or_else(|| {
                field
                    .as_str()
                    .and_then(|text| text.trim().parse::<usize>().ok())
            })
    });
    parsed.unwrap_or(default).clamp(min, max)
}

pub(crate) fn truncate_for_note(value: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for ch in value.chars().take(max_chars) {
        output.push(ch);
    }
    if value.chars().count() > max_chars {
        output.push_str("...");
    }
    output
}

#[tauri::command]
pub(crate) async fn core_mobile_market_data_refresh_tencent(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    crate::runtime::with_market_refresh_permit(
        "core_mobile_market_data_refresh_tencent",
        core_mobile_market_data_refresh_tencent_inner(app, payload),
    )
    .await
}

pub(crate) async fn core_mobile_market_data_refresh_tencent_inner(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let seed = refresh_seed_payload(&app, &payload);
    let financial_snapshot = refresh_financial_snapshot_payload(&app, &payload);
    let scan_candidates = payload
        .get("scan_candidates")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let use_previous_close = payload
        .get("use_previous_close")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let max_candidates = payload
        .get("max_candidates")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or(TENCENT_DEFAULT_MAX_CANDIDATES);
    let max_failed_batches = payload
        .get("max_failed_batches")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or(TENCENT_DEFAULT_MAX_FAILED_BATCHES);
    let batch_start = payload
        .get("batch_start")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0);
    let batch_count = payload
        .get("batch_count")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0);
    emit_market_refresh_log(
        &app,
        "command_received",
        "info",
        json!({
            "seed_stock_count": seed.get("stocks").and_then(Value::as_array).map(|stocks| stocks.len()).unwrap_or(0),
            "scan_candidates": scan_candidates,
            "max_candidates": max_candidates,
            "batch_start": batch_start,
            "batch_count": batch_count,
        }),
    );
    let refresh = refresh_tencent_market_data(
        &app,
        seed,
        scan_candidates,
        max_candidates,
        use_previous_close,
        max_failed_batches,
        batch_start,
        batch_count,
        financial_snapshot,
        Some(&payload),
    )
    .await?;
    emit_market_refresh_log(
        &app,
        "command_complete",
        "ok",
        json!({
            "fetched": refresh.fetched,
            "preserved": refresh.preserved,
            "failed_batches": refresh.failed_batches,
            "empty_batches": refresh.empty_batches,
            "next_batch_start": refresh.next_batch_start,
            "total_batches": refresh.total_batches,
            "done": refresh.done,
        }),
    );
    let cache = write_mobile_market_data_record(&app, refresh.dataset, false)?;
    let mut notes = vec![format!(
        "Tencent quote refresh finished: fetched {} of {} candidates, preserved {} local rows",
        refresh.fetched, refresh.requested, refresh.preserved
    )];
    match refresh.stop_reason.as_deref() {
        Some("failed_batches") => {
            notes.push(format!(
                "Tencent quote refresh stopped early after {} failed batches",
                refresh.failed_batches
            ));
        }
        _ => {}
    }
    if !refresh.error_samples.is_empty() {
        notes.push(format!(
            "Recent Tencent network errors: {}",
            refresh.error_samples.join(" | ")
        ));
    }
    Ok(json!({
        "refreshed": true,
        "source": "tencent",
        "requested": refresh.requested,
        "fetched": refresh.fetched,
        "preserved": refresh.preserved,
        "failed_batches": refresh.failed_batches,
        "empty_batches": refresh.empty_batches,
        "error_samples": refresh.error_samples.clone(),
        "stopped_early": refresh.stopped_early,
        "stop_reason": refresh.stop_reason,
        "batch_start": refresh.batch_start,
        "batch_count": refresh.batch_count,
        "next_batch_start": refresh.next_batch_start,
        "total_batches": refresh.total_batches,
        "done": refresh.done,
        "processed_codes": refresh.processed_codes,
        "total_candidates": refresh.total_candidates,
        "status": cache,
        "notes": notes
    }))
}

pub(crate) fn core_mobile_market_data_ingest_tencent_quotes(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let seed = refresh_seed_payload(&app, &payload);
    let financial_snapshot = refresh_financial_snapshot_payload(&app, &payload);
    let scan_candidates = payload
        .get("scan_candidates")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let use_previous_close = payload
        .get("use_previous_close")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let max_candidates = payload
        .get("max_candidates")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or(TENCENT_DEFAULT_MAX_CANDIDATES);
    let batch_start = payload
        .get("batch_start")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0);
    let batch_count = payload
        .get("batch_count")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0);
    let quote_bytes = payload
        .get("quote_bytes_base64")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(|encoded| {
            if encoded.len() > MAX_TENCENT_WEBVIEW_QUOTE_BYTES * 2 {
                return Err("Tencent WebView quote payload is too large".to_string());
            }
            let bytes = general_purpose::STANDARD
                .decode(encoded)
                .map_err(|error| format!("decode Tencent WebView quote bytes failed: {error}"))?;
            if bytes.len() > MAX_TENCENT_WEBVIEW_QUOTE_BYTES {
                return Err("Tencent WebView quote payload is too large".to_string());
            }
            Ok(bytes)
        })
        .transpose()?;
    let quote_text = if let Some(bytes) = quote_bytes.as_ref() {
        let (text, _, _) = encoding_rs::GBK.decode(bytes);
        text.into_owned()
    } else {
        let text = payload
            .get("quote_text")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing Tencent quote text from WebView".to_string())?
            .to_string();
        if text.len() > MAX_TENCENT_WEBVIEW_QUOTE_BYTES {
            return Err("Tencent WebView quote payload is too large".to_string());
        }
        text
    };
    if quote_text.trim().is_empty() {
        return Err("Tencent quote text from WebView is empty".to_string());
    }
    let webview_status = payload
        .get("webview_status")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .unwrap_or(200);
    let webview_byte_len = payload
        .get("webview_byte_len")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .or_else(|| quote_bytes.as_ref().map(Vec::len))
        .unwrap_or_else(|| quote_text.len());
    let webview_elapsed_ms = payload
        .get("webview_elapsed_ms")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    emit_market_refresh_log(
        &app,
        "webview_ingest_received",
        "info",
        json!({
            "seed_stock_count": seed.get("stocks").and_then(Value::as_array).map(|stocks| stocks.len()).unwrap_or(0),
            "scan_candidates": scan_candidates,
            "max_candidates": max_candidates,
            "batch_start": batch_start,
            "batch_count": batch_count,
            "webview_status": webview_status,
            "webview_byte_len": webview_byte_len,
            "webview_elapsed_ms": webview_elapsed_ms,
        }),
    );

    let refresh = ingest_tencent_market_data(
        &app,
        seed,
        scan_candidates,
        max_candidates,
        use_previous_close,
        batch_start,
        batch_count,
        &quote_text,
        webview_status,
        webview_byte_len,
        webview_elapsed_ms,
        financial_snapshot,
    )?;
    emit_market_refresh_log(
        &app,
        "command_complete",
        "ok",
        json!({
            "fetched": refresh.fetched,
            "preserved": refresh.preserved,
            "failed_batches": refresh.failed_batches,
            "empty_batches": refresh.empty_batches,
            "next_batch_start": refresh.next_batch_start,
            "total_batches": refresh.total_batches,
            "done": refresh.done,
        }),
    );
    let cache = write_mobile_market_data_record(&app, refresh.dataset, false)?;
    Ok(json!({
        "refreshed": true,
        "source": "tencent-webview",
        "requested": refresh.requested,
        "fetched": refresh.fetched,
        "preserved": refresh.preserved,
        "failed_batches": refresh.failed_batches,
        "empty_batches": refresh.empty_batches,
        "error_samples": refresh.error_samples.clone(),
        "stopped_early": refresh.stopped_early,
        "stop_reason": refresh.stop_reason,
        "batch_start": refresh.batch_start,
        "batch_count": refresh.batch_count,
        "next_batch_start": refresh.next_batch_start,
        "total_batches": refresh.total_batches,
        "done": refresh.done,
        "processed_codes": refresh.processed_codes,
        "total_candidates": refresh.total_candidates,
        "status": cache,
        "notes": [format!(
            "Tencent WebView quote ingest finished: fetched {} of {} candidates, preserved {} local rows",
            refresh.fetched, refresh.requested, refresh.preserved
        )]
    }))
}

pub(crate) fn refresh_seed_cache() -> &'static Mutex<HashMap<PathBuf, Value>> {
    REFRESH_SEED_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn refresh_financial_snapshot_cache() -> &'static Mutex<HashMap<PathBuf, Arc<Value>>> {
    REFRESH_FINANCIAL_SNAPSHOT_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn mobile_market_data_cache() -> &'static Mutex<HashMap<PathBuf, MobileMarketDataCacheEntry>> {
    MOBILE_MARKET_DATA_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn screen_stock_overlay_cache() -> &'static Mutex<HashMap<PathBuf, ScreenStockOverlayCacheEntry>> {
    SCREEN_STOCK_OVERLAY_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn mobile_market_update_lock() -> &'static Mutex<()> {
    MOBILE_MARKET_UPDATE_LOCK.get_or_init(|| Mutex::new(()))
}

pub(crate) fn cache_context_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    mobile_market_data_path(app).ok()
}

pub(crate) fn remember_refresh_seed(app: &tauri::AppHandle, seed: &Value) {
    if !market_data_payload_present(seed) {
        return;
    }
    let Some(path) = cache_context_path(app) else {
        return;
    };
    if let Ok(mut slot) = refresh_seed_cache().lock() {
        slot.insert(path, seed.clone());
    }
}

pub(crate) fn clear_refresh_seed(app: &tauri::AppHandle) {
    if let Some(path) = cache_context_path(app) {
        if let Ok(mut slot) = refresh_seed_cache().lock() {
            slot.remove(&path);
        }
        if let Ok(mut slot) = refresh_financial_snapshot_cache().lock() {
            slot.remove(&path);
        }
        if let Ok(mut slot) = screen_stock_overlay_cache().lock() {
            slot.remove(&path);
        }
    } else {
        if let Ok(mut slot) = refresh_seed_cache().lock() {
            slot.clear();
        }
        if let Ok(mut slot) = refresh_financial_snapshot_cache().lock() {
            slot.clear();
        }
        if let Ok(mut slot) = screen_stock_overlay_cache().lock() {
            slot.clear();
        }
    }
}

pub(crate) fn remember_refresh_financial_snapshot(app: &tauri::AppHandle, snapshot: &Value) {
    if !financial_snapshot_payload_present(snapshot) {
        return;
    }
    let Some(path) = cache_context_path(app) else {
        return;
    };
    if let Ok(mut slot) = refresh_financial_snapshot_cache().lock() {
        slot.insert(path, Arc::new(snapshot.clone()));
    }
}

pub(crate) fn refresh_financial_snapshot_payload(app: &tauri::AppHandle, payload: &Value) -> Value {
    let snapshot = payload
        .get("financial_snapshot")
        .cloned()
        .unwrap_or(Value::Null);
    if financial_snapshot_payload_present(&snapshot) {
        remember_refresh_financial_snapshot(app, &snapshot);
        return snapshot;
    }
    let Some(path) = cache_context_path(app) else {
        return Value::Null;
    };
    if let Ok(slot) = refresh_financial_snapshot_cache().lock() {
        if let Some(cached) = slot.get(&path) {
            if financial_snapshot_payload_present(cached) {
                return cached.as_ref().clone();
            }
        }
    }
    Value::Null
}

pub(crate) fn financial_snapshot_payload_present(value: &Value) -> bool {
    value
        .get("stocks")
        .and_then(Value::as_array)
        .map(|stocks| !stocks.is_empty())
        .unwrap_or(false)
        || value
            .get("financials")
            .and_then(Value::as_object)
            .map(|financials| !financials.is_empty())
            .unwrap_or(false)
        || value
            .get("industries")
            .and_then(Value::as_object)
            .map(|industries| !industries.is_empty())
            .unwrap_or(false)
}

pub(crate) fn refresh_seed_payload(app: &tauri::AppHandle, payload: &Value) -> Value {
    let seed = payload.get("seed").cloned().unwrap_or(Value::Null);
    if market_data_payload_present(&seed) {
        remember_refresh_seed(app, &seed);
        return seed;
    }
    if let Some(path) = cache_context_path(app) {
        if let Ok(slot) = refresh_seed_cache().lock() {
            if let Some(cached) = slot.get(&path) {
                if market_data_payload_present(cached) {
                    return cached.clone();
                }
            }
        }
    }
    let cached = read_mobile_market_data_record(app, true)
        .ok()
        .and_then(|record| record.get("data").cloned())
        .unwrap_or_else(|| json!({}));
    remember_refresh_seed(app, &cached);
    cached
}

pub(crate) fn market_data_payload_present(value: &Value) -> bool {
    value
        .get("stocks")
        .and_then(Value::as_array)
        .map(|stocks| !stocks.is_empty())
        .unwrap_or(false)
        || value
            .get("relations")
            .and_then(Value::as_array)
            .map(|relations| !relations.is_empty())
            .unwrap_or(false)
        || value
            .get("histories")
            .and_then(Value::as_object)
            .map(|histories| !histories.is_empty())
            .unwrap_or(false)
}

pub(crate) struct TencentRefreshResult {
    pub(crate) dataset: Value,
    pub(crate) requested: usize,
    pub(crate) fetched: usize,
    pub(crate) preserved: usize,
    pub(crate) failed_batches: usize,
    pub(crate) empty_batches: usize,
    pub(crate) error_samples: Vec<String>,
    pub(crate) stopped_early: bool,
    pub(crate) stop_reason: Option<String>,
    pub(crate) batch_start: usize,
    pub(crate) batch_count: usize,
    pub(crate) next_batch_start: usize,
    pub(crate) total_batches: usize,
    pub(crate) done: bool,
    pub(crate) processed_codes: usize,
    pub(crate) total_candidates: usize,
}

pub(crate) struct TencentQuotePayload {
    pub(crate) text: String,
    pub(crate) byte_len: usize,
    pub(crate) status: u16,
    pub(crate) transport: &'static str,
}

pub(crate) fn emit_market_refresh_log(app: &tauri::AppHandle, stage: &str, tone: &str, payload: Value) {
    let _ = app.emit(
        "market-refresh-log",
        json!({
            "stage": stage,
            "tone": tone,
            "payload": payload,
            "timestamp_ms": epoch_millis(),
        }),
    );
}

pub(crate) async fn refresh_tencent_market_data(
    app: &tauri::AppHandle,
    seed: Value,
    scan_candidates: bool,
    max_candidates: usize,
    use_previous_close: bool,
    max_failed_batches: usize,
    batch_start: usize,
    batch_count: Option<usize>,
    financial_snapshot: Value,
    network_payload: Option<&Value>,
) -> Result<TencentRefreshResult, String> {
    let (seed_stocks, seed_codes) = seed_stock_maps(&seed);
    let enriched_stocks = enriched_stock_maps(&seed_stocks, &financial_snapshot);
    let candidate_codes = build_candidate_codes(&seed_codes, scan_candidates, max_candidates);
    if candidate_codes.is_empty() {
        return Err("mobile market refresh has no candidate stock codes".to_string());
    }

    let client = build_http_client_with_proxy(
        "Mozilla/5.0 GuXuanYou/0.3 mobile",
        Duration::from_secs(TENCENT_REQUEST_TIMEOUT_SECS),
        network_payload,
    )?;

    emit_market_refresh_log(
        app,
        "candidate_ready",
        "info",
        json!({
            "candidate_count": candidate_codes.len(),
            "seed_candidate_count": seed_stocks.len(),
            "scan_candidates": scan_candidates,
        }),
    );

    let (normalized_batch_start, batch_end, total_batches) =
        candidate_batch_window(candidate_codes.len(), batch_start, batch_count);
    let effective_batch_count = batch_end.saturating_sub(normalized_batch_start);
    let requested_codes: Vec<String> = candidate_codes
        .chunks(TENCENT_BATCH_SIZE)
        .skip(normalized_batch_start)
        .take(effective_batch_count)
        .flat_map(|chunk| chunk.iter().cloned())
        .collect();

    let mut stocks = Vec::new();
    let mut seen = HashSet::new();
    let mut failed_batches = 0usize;
    let mut empty_batches = 0usize;
    let mut error_samples = Vec::new();
    let mut stopped_early = false;
    let mut stop_reason = None;
    let request_timeout = Duration::from_secs(TENCENT_REQUEST_TIMEOUT_SECS);
    let batch_timeout = Duration::from_secs(TENCENT_BATCH_TIMEOUT_SECS);
    emit_market_refresh_log(
        app,
        "batch_window",
        "info",
        json!({
            "batch_start": normalized_batch_start,
            "batch_end": batch_end,
            "batch_count": effective_batch_count,
            "total_batches": total_batches,
            "request_timeout_seconds": request_timeout.as_secs(),
            "batch_timeout_seconds": batch_timeout.as_secs(),
        }),
    );
    // Fetch every batch in the window concurrently (bounded by TENCENT_FETCH_CONCURRENCY)
    // instead of awaiting them one at a time. qt.gtimg.cn tolerates parallel requests, so
    // wall-clock collapses from sum-of-batches to roughly slowest-batch * ceil(n / concurrency).
    // Codes are cloned into owned Vecs so the per-batch future captures no borrowed slice
    // (a borrowed `&[String]` trips higher-ranked lifetime inference inside the stream).
    let window: Vec<(usize, Vec<String>)> = candidate_codes
        .chunks(TENCENT_BATCH_SIZE)
        .enumerate()
        .skip(normalized_batch_start)
        .take(effective_batch_count)
        .map(|(offset, batch)| (offset, batch.to_vec()))
        .collect();
    let mut fetch_results: Vec<(usize, Result<(TencentQuotePayload, u128), (String, u128)>)> =
        stream::iter(window)
            .map(|(offset, batch)| {
                let client = &client;
                async move {
                    let batch_started_at = epoch_millis();
                    emit_market_refresh_log(
                        app,
                        "batch_request",
                        "info",
                        json!({
                            "batch_index": offset + 1,
                            "total_batches": total_batches,
                            "code_count": batch.len(),
                            "first_code": batch.first(),
                            "last_code": batch.last(),
                        }),
                    );
                    let fetch_result = match tokio::time::timeout(
                        batch_timeout,
                        fetch_tencent_quotes(client, &batch, request_timeout),
                    )
                    .await
                    {
                        Ok(result) => result,
                        Err(_) => Err(format!(
                            "Tencent quote batch timed out after {} seconds",
                            batch_timeout.as_secs()
                        )),
                    };
                    let elapsed_ms = epoch_millis().saturating_sub(batch_started_at);
                    let timed_result = fetch_result
                        .map(|payload| (payload, elapsed_ms))
                        .map_err(|error| (error, elapsed_ms));
                    (offset, timed_result)
                }
            })
            .buffer_unordered(TENCENT_FETCH_CONCURRENCY)
            .collect()
            .await;

    // The whole window is attempted, so advance the cursor to its end up front.
    let next_batch_start = batch_end;
    // Process responses in deterministic batch order so dedup and logs stay stable.
    fetch_results.sort_by_key(|(offset, _)| *offset);
    for (offset, fetch_result) in fetch_results {
        match fetch_result {
            Ok((payload, elapsed_ms)) => {
                let parsed_stocks =
                    parse_tencent_quotes(&payload.text, &enriched_stocks, use_previous_close);
                if parsed_stocks.is_empty() {
                    empty_batches += 1;
                }
                emit_market_refresh_log(
                    app,
                    "batch_response",
                    if parsed_stocks.is_empty() {
                        "warn"
                    } else {
                        "ok"
                    },
                    json!({
                        "batch_index": offset + 1,
                        "status": payload.status,
                        "transport": payload.transport,
                        "byte_len": payload.byte_len,
                        "parsed_count": parsed_stocks.len(),
                        "elapsed_ms": elapsed_ms,
                        "sample": payload.text.chars().take(120).collect::<String>(),
                    }),
                );
                for mut stock in parsed_stocks {
                    let Some(code) = stock
                        .get("code")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                    else {
                        continue;
                    };
                    if seen.insert(code) {
                        stocks.push(Value::Object(std::mem::take(&mut stock)));
                    }
                }
            }
            Err((error, elapsed_ms)) => {
                emit_market_refresh_log(
                    app,
                    "batch_error",
                    "error",
                    json!({
                        "batch_index": offset + 1,
                        "elapsed_ms": elapsed_ms,
                        "error": error,
                    }),
                );
                if error_samples.len() < 3 {
                    error_samples.push(error);
                }
                failed_batches += 1;
            }
        }
    }

    // With concurrent fetching the entire window is attempted before we decide anything, so
    // "early stop" becomes a post-hoc check: only bail when the window saw enough failures and
    // produced no fresh quotes at all (a network-down signal). The front-end then stops paging.
    if failed_batches >= max_failed_batches && seen.is_empty() {
        stopped_early = true;
        stop_reason = Some("failed_batches".to_string());
    }
    let fetched = seen.len();
    let preserved =
        append_all_preserved_seed_stocks(&seed_stocks, &enriched_stocks, &mut stocks, &mut seen);
    if stocks.is_empty() {
        let suffix = if error_samples.is_empty() {
            String::new()
        } else {
            format!("; recent errors: {}", error_samples.join(" | "))
        };
        return Err(format!(
            "Tencent quote refresh returned no valid stocks{suffix}"
        ));
    }
    stocks.sort_by(|left, right| {
        left.get("code")
            .and_then(Value::as_str)
            .unwrap_or("")
            .cmp(right.get("code").and_then(Value::as_str).unwrap_or(""))
    });
    let valid_codes: HashSet<String> = stocks
        .iter()
        .filter_map(|stock| {
            stock
                .get("code")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect();

    let dataset = json!({
        "source": "tencent",
        "generated_at": epoch_millis().to_string(),
        "generated_at_epoch_ms": epoch_millis(),
        "notes": [
            "mobile online refresh via Tencent quote",
            "industry and slow-changing metrics are merged from the previous local dataset",
            if use_previous_close {
                "price policy: previous close before market close"
            } else {
                "price policy: latest Tencent quote"
            }
        ],
        "stocks": stocks,
        "relations": filter_seed_relations(&seed, &valid_codes),
        "histories": filter_seed_histories(&seed, &valid_codes),
        "financials": filtered_financial_snapshot_map(&seed, &financial_snapshot, &valid_codes)
    });

    let done = stopped_early || next_batch_start >= total_batches;
    Ok(TencentRefreshResult {
        requested: candidate_codes.len(),
        fetched,
        preserved,
        failed_batches,
        empty_batches,
        error_samples,
        stopped_early,
        stop_reason,
        batch_start: normalized_batch_start,
        batch_count: effective_batch_count,
        next_batch_start,
        total_batches,
        done,
        processed_codes: requested_codes.len(),
        total_candidates: candidate_codes.len(),
        dataset,
    })
}

pub(crate) fn ingest_tencent_market_data(
    app: &tauri::AppHandle,
    seed: Value,
    scan_candidates: bool,
    max_candidates: usize,
    use_previous_close: bool,
    batch_start: usize,
    batch_count: Option<usize>,
    quote_text: &str,
    webview_status: u16,
    webview_byte_len: usize,
    webview_elapsed_ms: u64,
    financial_snapshot: Value,
) -> Result<TencentRefreshResult, String> {
    let (seed_stocks, seed_codes) = seed_stock_maps(&seed);
    let enriched_stocks = enriched_stock_maps(&seed_stocks, &financial_snapshot);
    let candidate_codes = build_candidate_codes(&seed_codes, scan_candidates, max_candidates);
    if candidate_codes.is_empty() {
        return Err("mobile market refresh has no candidate stock codes".to_string());
    }

    emit_market_refresh_log(
        app,
        "candidate_ready",
        "info",
        json!({
            "candidate_count": candidate_codes.len(),
            "seed_candidate_count": seed_stocks.len(),
            "scan_candidates": scan_candidates,
        }),
    );

    let (normalized_batch_start, batch_end, total_batches) =
        candidate_batch_window(candidate_codes.len(), batch_start, batch_count);
    let effective_batch_count = batch_end.saturating_sub(normalized_batch_start);
    let requested_codes: Vec<String> = candidate_codes
        .chunks(TENCENT_BATCH_SIZE)
        .skip(normalized_batch_start)
        .take(effective_batch_count)
        .flat_map(|chunk| chunk.iter().cloned())
        .collect();

    emit_market_refresh_log(
        app,
        "batch_window",
        "info",
        json!({
            "batch_start": normalized_batch_start,
            "batch_end": batch_end,
            "batch_count": effective_batch_count,
            "total_batches": total_batches,
            "request_timeout_seconds": 0,
            "batch_timeout_seconds": 0,
            "transport": "webview",
        }),
    );

    let parsed_stocks = parse_tencent_quotes(quote_text, &enriched_stocks, use_previous_close);
    let empty_batches = if parsed_stocks.is_empty() && effective_batch_count > 0 {
        effective_batch_count
    } else {
        0
    };
    emit_market_refresh_log(
        app,
        "batch_response",
        if parsed_stocks.is_empty() {
            "warn"
        } else {
            "ok"
        },
        json!({
            "batch_index": normalized_batch_start + 1,
            "status": webview_status,
            "byte_len": webview_byte_len,
            "parsed_count": parsed_stocks.len(),
            "elapsed_ms": webview_elapsed_ms,
            "transport": "webview",
            "sample": quote_text.chars().take(120).collect::<String>(),
        }),
    );

    let requested_set: HashSet<String> = requested_codes.iter().cloned().collect();
    let mut stocks = Vec::new();
    let mut seen = HashSet::new();
    let mut ignored_quote_codes = Vec::new();
    for mut stock in parsed_stocks {
        let Some(code) = stock
            .get("code")
            .and_then(Value::as_str)
            .and_then(normalize_stock_code)
        else {
            continue;
        };
        if !requested_set.contains(&code) {
            if ignored_quote_codes.len() < 8 {
                ignored_quote_codes.push(code);
            }
            continue;
        }
        stock.insert("code".to_string(), json!(code));
        let code = stock
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if seen.insert(code) {
            stocks.push(Value::Object(std::mem::take(&mut stock)));
        }
    }
    let fetched = seen.len();
    let missing_requested_count = requested_set.len().saturating_sub(fetched);
    if !ignored_quote_codes.is_empty() || missing_requested_count > 0 {
        emit_market_refresh_log(
            app,
            "webview_batch_mismatch",
            "warn",
            json!({
                "batch_index": normalized_batch_start + 1,
                "requested_count": requested_set.len(),
                "fetched_count": fetched,
                "missing_requested_count": missing_requested_count,
                "ignored_quote_codes": ignored_quote_codes,
                "transport": "webview",
            }),
        );
    }
    let preserved =
        append_all_preserved_seed_stocks(&seed_stocks, &enriched_stocks, &mut stocks, &mut seen);
    if stocks.is_empty() {
        return Err(format!(
            "Tencent WebView quote returned no valid stocks; sample: {}",
            quote_text.chars().take(160).collect::<String>()
        ));
    }
    stocks.sort_by(|left, right| {
        left.get("code")
            .and_then(Value::as_str)
            .unwrap_or("")
            .cmp(right.get("code").and_then(Value::as_str).unwrap_or(""))
    });
    let valid_codes: HashSet<String> = stocks
        .iter()
        .filter_map(|stock| {
            stock
                .get("code")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect();

    let dataset = json!({
        "source": "tencent",
        "generated_at": epoch_millis().to_string(),
        "generated_at_epoch_ms": epoch_millis(),
        "notes": [
            "mobile online refresh via WebView Tencent quote",
            "industry and slow-changing metrics are merged from the previous local dataset",
            if use_previous_close {
                "price policy: previous close before market close"
            } else {
                "price policy: latest Tencent quote"
            }
        ],
        "stocks": stocks,
        "relations": filter_seed_relations(&seed, &valid_codes),
        "histories": filter_seed_histories(&seed, &valid_codes),
        "financials": filtered_financial_snapshot_map(&seed, &financial_snapshot, &valid_codes)
    });

    Ok(TencentRefreshResult {
        requested: candidate_codes.len(),
        fetched,
        preserved,
        failed_batches: 0,
        empty_batches,
        error_samples: Vec::new(),
        stopped_early: false,
        stop_reason: None,
        batch_start: normalized_batch_start,
        batch_count: effective_batch_count,
        next_batch_start: batch_end,
        total_batches,
        done: batch_end >= total_batches,
        processed_codes: requested_codes.len(),
        total_candidates: candidate_codes.len(),
        dataset,
    })
}

pub(crate) fn append_preserved_seed_stocks(
    candidate_codes: &[String],
    seed_stocks: &HashMap<String, serde_json::Map<String, Value>>,
    stocks: &mut Vec<Value>,
    seen: &mut HashSet<String>,
) -> usize {
    let mut preserved = 0usize;
    for code in candidate_codes {
        if seen.contains(code) {
            continue;
        }
        let Some(existing) = seed_stocks.get(code) else {
            continue;
        };
        let mut stock = existing.clone();
        stock.insert("code".to_string(), json!(code));
        if stock
            .get("industry")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
        {
            stock.insert("industry".to_string(), json!(board_label(code)));
        }
        if seen.insert(code.clone()) {
            stocks.push(Value::Object(stock));
            preserved += 1;
        }
    }
    preserved
}

pub(crate) fn append_all_preserved_seed_stocks(
    seed_stocks: &HashMap<String, serde_json::Map<String, Value>>,
    enriched_stocks: &HashMap<String, serde_json::Map<String, Value>>,
    stocks: &mut Vec<Value>,
    seen: &mut HashSet<String>,
) -> usize {
    let mut codes: Vec<String> = seed_stocks.keys().cloned().collect();
    codes.sort();
    append_preserved_seed_stocks(&codes, enriched_stocks, stocks, seen)
}

pub(crate) fn enriched_stock_maps(
    seed_stocks: &HashMap<String, serde_json::Map<String, Value>>,
    financial_snapshot: &Value,
) -> HashMap<String, serde_json::Map<String, Value>> {
    let mut enriched = seed_stocks.clone();

    if let Some(financials) = financial_snapshot
        .get("financials")
        .and_then(Value::as_object)
    {
        for (raw_code, item) in financials {
            let Some(code) = normalize_stock_code(raw_code) else {
                continue;
            };
            let Some(object) = item.as_object() else {
                continue;
            };
            merge_stock_financial_fields(&mut enriched, &code, object);
        }
    }

    if let Some(snapshot_stocks) = financial_snapshot.get("stocks").and_then(Value::as_array) {
        for item in snapshot_stocks {
            let Some(object) = item.as_object() else {
                continue;
            };
            let Some(code) = object
                .get("code")
                .and_then(Value::as_str)
                .and_then(normalize_stock_code)
            else {
                continue;
            };
            merge_stock_financial_fields(&mut enriched, &code, object);
        }
    }

    if let Some(industries) = financial_snapshot
        .get("industries")
        .and_then(Value::as_object)
    {
        for (raw_code, value) in industries {
            let Some(code) = normalize_stock_code(raw_code) else {
                continue;
            };
            let Some(industry) = value.as_str().map(str::trim) else {
                continue;
            };
            if industry.is_empty() || industry == "-" {
                continue;
            }
            let target = enriched.entry(code.clone()).or_insert_with(|| {
                let mut row = serde_json::Map::new();
                row.insert("code".to_string(), json!(code));
                row
            });
            target.insert("industry".to_string(), json!(industry));
        }
    }

    enriched
}

pub(crate) fn merge_stock_financial_fields(
    enriched: &mut HashMap<String, serde_json::Map<String, Value>>,
    code: &str,
    source: &serde_json::Map<String, Value>,
) {
    let target = enriched.entry(code.to_string()).or_insert_with(|| {
        let mut row = serde_json::Map::new();
        row.insert("code".to_string(), json!(code));
        row
    });
    for field in SCREEN_STOCK_FINANCIAL_FIELDS {
        if finite_object_number(target, field).is_some() {
            continue;
        }
        if let Some(value) = finite_object_number(source, field) {
            target.insert(field.to_string(), json!(value));
        }
    }
}

pub(crate) fn filtered_financial_snapshot_map(
    seed: &Value,
    financial_snapshot: &Value,
    valid_codes: &HashSet<String>,
) -> Value {
    let mut entries = serde_json::Map::new();
    merge_financials_object(&mut entries, seed.get("financials"));
    merge_financials_object(&mut entries, financial_snapshot.get("financials"));
    merge_financials_array(&mut entries, seed.get("stocks"));
    merge_financials_array(&mut entries, financial_snapshot.get("stocks"));

    let mut filtered = serde_json::Map::new();
    for (code, value) in entries {
        if valid_codes.contains(&code) {
            filtered.insert(code, value);
        }
    }
    Value::Object(filtered)
}

pub(crate) fn merge_financials_object(target: &mut serde_json::Map<String, Value>, value: Option<&Value>) {
    let Some(object) = value.and_then(Value::as_object) else {
        return;
    };
    for (raw_code, item) in object {
        merge_financial_entry(target, raw_code, item);
    }
}

pub(crate) fn merge_financials_array(target: &mut serde_json::Map<String, Value>, value: Option<&Value>) {
    let Some(items) = value.and_then(Value::as_array) else {
        return;
    };
    for item in items {
        let Some(object) = item.as_object() else {
            continue;
        };
        let Some(raw_code) = object.get("code").and_then(Value::as_str) else {
            continue;
        };
        merge_financial_entry(target, raw_code, item);
    }
}

pub(crate) fn merge_financial_entry(
    target: &mut serde_json::Map<String, Value>,
    raw_code: &str,
    item: &Value,
) {
    let Some(code) = normalize_stock_code(raw_code) else {
        return;
    };
    let Some(object) = item.as_object() else {
        return;
    };
    let mut entry = target
        .get(&code)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    if let Some(value) = finite_object_number_any(object, &["latest_eps", "eps", "EPSJB"]) {
        entry.insert("latest_eps".to_string(), json!(value));
    }
    if let Some(value) = finite_object_number_any(object, &["latest_bps", "bps", "BPS"]) {
        entry.insert("latest_bps".to_string(), json!(value));
    }
    for field in [
        "operating_revenue_billion",
        "operating_revenue_yoy",
        "parent_net_profit_billion",
        "parent_net_profit_yoy",
        "gross_margin",
        "net_margin",
        "roe",
        "asset_liability_ratio",
        "goodwill_to_net_assets",
        "pledged_share_ratio",
        "dividend_yield",
        "dividend_payout_ratio",
    ] {
        if let Some(value) = finite_object_number(object, field) {
            entry.insert(field.to_string(), json!(value));
        }
    }
    for field in ["goodwill_period", "pledged_share_period", "dividend_period"] {
        if let Some(value) = object_string(object, field).filter(|value| !value.trim().is_empty()) {
            entry.insert(field.to_string(), json!(value));
        }
    }
    if let Some(period) = object_string_any(object, &["period", "latest_period", "report_period"])
        .filter(|value| !value.trim().is_empty())
    {
        entry.insert("period".to_string(), json!(period));
    }
    if let Some(source) =
        object_string_any(object, &["source"]).filter(|value| !value.trim().is_empty())
    {
        entry.insert("source".to_string(), json!(source));
    }
    if let Some(note_values) = object.get("notes").and_then(Value::as_array) {
        let mut notes = entry
            .get("notes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut seen_notes: HashSet<String> = notes
            .iter()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect();
        for note in note_values.iter().filter_map(Value::as_str) {
            let trimmed = note.trim();
            if !trimmed.is_empty() && seen_notes.insert(trimmed.to_string()) {
                notes.push(Value::String(trimmed.to_string()));
            }
        }
        if !notes.is_empty() {
            entry.insert("notes".to_string(), Value::Array(notes));
        }
    }
    let quarterly_eps = normalize_quarterly_eps(object.get("quarterly_eps"));
    if !quarterly_eps.is_empty() {
        if !entry.contains_key("period") {
            if let Some(period) = quarterly_eps
                .first()
                .and_then(|item| item.get("period"))
                .and_then(Value::as_str)
            {
                entry.insert("period".to_string(), json!(period));
            }
        }
        entry.insert("quarterly_eps".to_string(), Value::Array(quarterly_eps));
    }
    if !entry.is_empty() {
        target.insert(code, Value::Object(entry));
    }
}

pub(crate) fn normalize_quarterly_eps(value: Option<&Value>) -> Vec<Value> {
    let Some(items) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for item in items {
        let Some(object) = item.as_object() else {
            continue;
        };
        let Some(period) = object
            .get("period")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|period| valid_financial_period_key(period))
        else {
            continue;
        };
        let Some(value) = object
            .get("value")
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite())
        else {
            continue;
        };
        if !seen.insert(period.to_string()) {
            continue;
        }
        let mut row = serde_json::Map::new();
        row.insert("period".to_string(), json!(period));
        row.insert("value".to_string(), json!(value));
        if let Some(source) =
            object_string(object, "source").filter(|value| !value.trim().is_empty())
        {
            row.insert("source".to_string(), json!(source));
        }
        if object
            .get("inferred")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            row.insert("inferred".to_string(), json!(true));
        }
        if let Some(note) = object_string(object, "note").filter(|value| !value.trim().is_empty()) {
            row.insert("note".to_string(), json!(note));
        }
        normalized.push(Value::Object(row));
        if normalized.len() >= 12 {
            break;
        }
    }
    normalized
}

pub(crate) fn valid_financial_period_key(period: &str) -> bool {
    let bytes = period.as_bytes();
    bytes.len() == 6
        && bytes[0..4].iter().all(|byte| byte.is_ascii_digit())
        && bytes[4].eq_ignore_ascii_case(&b'Q')
        && matches!(bytes[5], b'1'..=b'4')
}

pub(crate) fn finite_object_number_any(
    object: &serde_json::Map<String, Value>,
    fields: &[&str],
) -> Option<f64> {
    fields
        .iter()
        .find_map(|field| finite_object_number(object, field))
}

pub(crate) fn object_string_any(object: &serde_json::Map<String, Value>, fields: &[&str]) -> Option<String> {
    fields.iter().find_map(|field| object_string(object, field))
}

pub(crate) fn finite_object_number(object: &serde_json::Map<String, Value>, field: &str) -> Option<f64> {
    object
        .get(field)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

pub(crate) fn candidate_batch_window(
    total_codes: usize,
    batch_start: usize,
    batch_count: Option<usize>,
) -> (usize, usize, usize) {
    if total_codes == 0 {
        return (0, 0, 0);
    }
    let total_batches = total_codes.div_ceil(TENCENT_BATCH_SIZE);
    let start = batch_start.min(total_batches);
    let count = batch_count.unwrap_or_else(|| total_batches.saturating_sub(start));
    let end = start.saturating_add(count).min(total_batches);
    (start, end, total_batches)
}

pub(crate) async fn fetch_tencent_quotes(
    client: &reqwest::Client,
    codes: &[String],
    request_timeout: Duration,
) -> Result<TencentQuotePayload, String> {
    #[cfg(windows)]
    async fn fetch_tencent_quotes_windows_fallback(
        url: &str,
        timeout_secs: u64,
        status: Option<u16>,
        primary_error: Option<String>,
    ) -> Result<TencentQuotePayload, String> {
        let url = url.to_string();
        let fallback = tokio::task::spawn_blocking(move || powershell_http_get_bytes(&url, timeout_secs))
        .await
        .map_err(|error| format!("Tencent quote PowerShell fallback task failed: {error}"))?
        .map_err(|powershell_error| match (status, primary_error.as_deref()) {
            (Some(status), _) => format!("Tencent quote HTTP {status}; PowerShell fallback failed: {powershell_error}"),
            (None, Some(error)) => format!("Tencent quote request failed: {error}; PowerShell fallback failed: {powershell_error}"),
            (None, None) => format!("Tencent quote fallback failed: {powershell_error}"),
        })?;
        let byte_len = fallback.len();
        let (text, _, _) = encoding_rs::GBK.decode(&fallback);
        Ok(TencentQuotePayload {
            text: text.into_owned(),
            byte_len,
            status: status.unwrap_or(200),
            transport: "powershell",
        })
    }

    #[cfg(not(windows))]
    async fn fetch_tencent_quotes_windows_fallback(
        _url: &str,
        _timeout_secs: u64,
        status: Option<u16>,
        primary_error: Option<String>,
    ) -> Result<TencentQuotePayload, String> {
        match (status, primary_error) {
            (Some(status), _) => Err(format!("Tencent quote HTTP {status}")),
            (None, Some(error)) => Err(format!("Tencent quote request failed: {error}")),
            (None, None) => Err("Tencent quote fallback failed".to_string()),
        }
    }
    let symbols: Vec<String> = codes
        .iter()
        .filter_map(|code| tencent_symbol(code))
        .collect();
    if symbols.is_empty() {
        return Ok(TencentQuotePayload {
            text: String::new(),
            byte_len: 0,
            status: 0,
            transport: "none",
        });
    }
    let url = format!("{TENCENT_QUOTE_ENDPOINT}{}", symbols.join(","));
    match client.get(&url).timeout(request_timeout).send().await {
        Ok(response) => {
            let status = response.status();
            if !status.is_success() {
                return fetch_tencent_quotes_windows_fallback(
                    &url,
                    request_timeout.as_secs().max(1),
                    Some(status.as_u16()),
                    None,
                )
                .await;
            }
            let bytes = response
                .bytes()
                .await
                .map_err(|error| format!("Tencent quote body read failed: {error}"))?;
            let byte_len = bytes.len();
            let (text, _, _) = encoding_rs::GBK.decode(&bytes);
            Ok(TencentQuotePayload {
                text: text.into_owned(),
                byte_len,
                status: status.as_u16(),
                transport: "reqwest",
            })
        }
        Err(error) => {
            fetch_tencent_quotes_windows_fallback(
                &url,
                request_timeout.as_secs().max(1),
                None,
                Some(error.to_string()),
            )
            .await
        }
    }
}

pub(crate) fn parse_tencent_quotes(
    text: &str,
    seed_stocks: &HashMap<String, serde_json::Map<String, Value>>,
    use_previous_close: bool,
) -> Vec<serde_json::Map<String, Value>> {
    let mut stocks = Vec::new();
    for raw_line in text.split(';') {
        let line = raw_line.trim();
        if line.is_empty() || !line.contains('=') || !line.contains('"') {
            continue;
        }
        let Some(left) = line.split('=').next() else {
            continue;
        };
        let key = left.rsplit('_').next().unwrap_or("").trim();
        let Some(code) = normalize_stock_code(key) else {
            continue;
        };
        let values: Vec<&str> = line.split('"').nth(1).unwrap_or("").split('~').collect();
        if values.len() < 53 {
            continue;
        }
        let name = values.get(1).map(|value| value.trim()).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let price = if use_previous_close {
            parse_number(values.get(4))
                .filter(|value| *value > 0.0)
                .or_else(|| parse_number(values.get(3)).filter(|value| *value > 0.0))
        } else {
            parse_number(values.get(3))
                .filter(|value| *value > 0.0)
                .or_else(|| parse_number(values.get(4)).filter(|value| *value > 0.0))
        };
        let Some(price) = price else {
            continue;
        };

        let existing = seed_stocks.get(&code);
        let mut stock = existing.cloned().unwrap_or_default();
        let pe = first_positive_number(&[values.get(39), values.get(52)])
            .or_else(|| existing.and_then(|object| object_f64(object, "pe")));
        let pb = parse_number(values.get(46))
            .filter(|value| *value > 0.0)
            .or_else(|| existing.and_then(|object| object_f64(object, "pb")));
        // Tencent fields 44/45 are circulating/total market cap in 亿元,
        // while 72/73 are the corresponding share counts.
        let circulating_market_cap = parse_number(values.get(44))
            .filter(|value| *value > 0.0)
            .or_else(|| {
                existing.and_then(|object| object_f64(object, "circulating_market_cap_billion"))
            });
        let market_cap = parse_number(values.get(45))
            .filter(|value| *value > 0.0)
            .or_else(|| existing.and_then(|object| object_f64(object, "market_cap_billion")));
        let circulating_shares = parse_number(values.get(72))
            .filter(|value| *value > 0.0)
            .or_else(|| existing.and_then(|object| object_f64(object, "circulating_shares")));
        let total_shares = parse_number(values.get(73))
            .filter(|value| *value > 0.0)
            .or_else(|| existing.and_then(|object| object_f64(object, "total_shares")));
        let change_pct = parse_number(values.get(32)).map(|value| value / 100.0);
        let volume = parse_number(values.get(6)).map(|value| value * 100.0);
        let amount = parse_number(values.get(37)).map(|value| value * 10_000.0);
        let turnover_rate = parse_number(values.get(38)).map(|value| value / 100.0);
        let volume_ratio = parse_number(values.get(49));
        let quote_time = values
            .get(30)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| existing.and_then(|object| object_string(object, "quote_time")));
        let roe = existing
            .and_then(|object| object_f64(object, "roe"))
            .or_else(|| estimate_roe(pe, pb));
        let industry = existing
            .and_then(|object| object_string(object, "industry"))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| board_label(&code).to_string());
        let is_st = name.to_ascii_uppercase().contains("ST")
            || existing
                .and_then(|object| object.get("is_st"))
                .and_then(Value::as_bool)
                .unwrap_or(false);

        stock.insert("code".to_string(), json!(code));
        stock.insert("name".to_string(), json!(name));
        stock.insert("industry".to_string(), json!(industry));
        stock.insert("is_st".to_string(), json!(is_st));
        stock.insert("price".to_string(), json!(price));
        stock.insert("pe".to_string(), json!(pe));
        stock.insert("pb".to_string(), json!(pb));
        stock.insert("roe".to_string(), json!(roe));
        stock.insert("market_cap_billion".to_string(), json!(market_cap));
        stock.insert(
            "circulating_market_cap_billion".to_string(),
            json!(circulating_market_cap),
        );
        stock.insert("total_shares".to_string(), json!(total_shares));
        stock.insert("circulating_shares".to_string(), json!(circulating_shares));
        stock.insert("change_pct".to_string(), json!(change_pct));
        stock.insert("volume".to_string(), json!(volume));
        stock.insert("amount".to_string(), json!(amount));
        stock.insert("turnover_rate".to_string(), json!(turnover_rate));
        stock.insert("volume_ratio".to_string(), json!(volume_ratio));
        stock.insert("quote_time".to_string(), json!(quote_time));
        stock.insert(
            "dividend_yield".to_string(),
            json!(existing.and_then(|object| object_f64(object, "dividend_yield"))),
        );
        stocks.push(stock);
    }
    stocks
}

pub(crate) fn seed_stock_maps(seed: &Value) -> (HashMap<String, serde_json::Map<String, Value>>, Vec<String>) {
    let mut stocks = HashMap::new();
    let mut codes = Vec::new();
    if let Some(items) = seed.get("stocks").and_then(Value::as_array) {
        for item in items {
            let Some(object) = item.as_object() else {
                continue;
            };
            let Some(code) = object
                .get("code")
                .and_then(Value::as_str)
                .and_then(normalize_stock_code)
            else {
                continue;
            };
            codes.push(code.clone());
            stocks.insert(code, object.clone());
        }
    }
    (stocks, codes)
}

pub(crate) fn build_candidate_codes(
    seed_codes: &[String],
    scan_candidates: bool,
    max_candidates: usize,
) -> Vec<String> {
    // The candidate list defines pagination windows. It MUST be stable across
    // the repeated invocations of a single full rebuild, otherwise `batch_start`
    // from the frontend drifts as the in-memory seed grows and codes get skipped
    // or re-fetched. When scanning, always emit the deterministic scan order
    // first and append only the seed-only codes (outside the scan ranges) at the
    // end. Seed DATA is merged/preserved separately, so ordering here only
    // decides which codes each batch fetches.
    let mut candidate_codes = Vec::new();
    if scan_candidates {
        append_tencent_candidate_codes(&mut candidate_codes);
    }
    candidate_codes.extend(seed_codes.iter().cloned());
    dedupe_stock_codes(&mut candidate_codes);
    if candidate_codes.len() > max_candidates {
        candidate_codes.truncate(max_candidates);
    }
    candidate_codes
}

pub(crate) fn append_tencent_candidate_codes(codes: &mut Vec<String>) {
    append_interleaved_ranges(
        codes,
        &[
            ("SZ", 1, 3999),
            ("SH", 600000, 605999),
            ("SZ", 300000, 301999),
            ("SH", 688000, 689999),
            ("BJ", 920000, 920999),
        ],
    );
}

pub(crate) fn append_interleaved_ranges(codes: &mut Vec<String>, ranges: &[(&str, u32, u32)]) {
    let max_len = ranges
        .iter()
        .map(|(_, start, end)| end.saturating_sub(*start))
        .max()
        .unwrap_or(0);
    for offset in 0..=max_len {
        for (market, start, end) in ranges {
            let value = start.saturating_add(offset);
            if value <= *end {
                codes.push(format!("{value:06}.{market}"));
            }
        }
    }
}

pub(crate) fn dedupe_stock_codes(codes: &mut Vec<String>) {
    let mut seen = HashSet::new();
    codes.retain(|code| seen.insert(code.clone()));
}

pub(crate) fn tencent_symbol(code: &str) -> Option<String> {
    let normalized = normalize_stock_code(code)?;
    let digits = &normalized[..6];
    if normalized.ends_with(".SH") {
        Some(format!("sh{digits}"))
    } else if normalized.ends_with(".BJ") {
        Some(format!("bj{digits}"))
    } else {
        Some(format!("sz{digits}"))
    }
}

pub(crate) fn normalize_stock_code(value: &str) -> Option<String> {
    let raw = value.trim().to_ascii_uppercase();
    if raw.is_empty() {
        return None;
    }
    if let Some(digits) = raw.strip_prefix("SH").filter(|digits| valid_digits(digits)) {
        return Some(format!("{digits}.SH"));
    }
    if let Some(digits) = raw.strip_prefix("SZ").filter(|digits| valid_digits(digits)) {
        return Some(format!("{digits}.SZ"));
    }
    if let Some(digits) = raw.strip_prefix("BJ").filter(|digits| valid_digits(digits)) {
        return Some(format!("{digits}.BJ"));
    }
    if let Some((digits, market)) = raw.split_once('.') {
        if valid_digits(digits) && matches!(market, "SH" | "SZ" | "BJ") {
            return Some(format!("{digits}.{market}"));
        }
    }
    let digits: String = raw
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .take(6)
        .collect();
    if !valid_digits(&digits) {
        return None;
    }
    Some(format!("{}.{}", digits, infer_market(&digits)))
}

pub(crate) fn valid_digits(value: &str) -> bool {
    value.len() == 6 && value.chars().all(|ch| ch.is_ascii_digit())
}

pub(crate) fn infer_market(digits: &str) -> &'static str {
    if digits.starts_with('6') || digits.starts_with('9') || digits.starts_with('5') {
        "SH"
    } else if digits.starts_with('4') || digits.starts_with('8') {
        "BJ"
    } else {
        "SZ"
    }
}

pub(crate) fn board_label(code: &str) -> &'static str {
    let digits = &code[..6];
    if code.ends_with(".BJ") {
        "\u{5317}\u{4ea4}\u{6240}"
    } else if digits.starts_with("688") {
        "\u{79d1}\u{521b}\u{677f}"
    } else if digits.starts_with("300") || digits.starts_with("301") {
        "\u{521b}\u{4e1a}\u{677f}"
    } else if code.ends_with(".SH") {
        "\u{6caa}\u{5e02}A\u{80a1}"
    } else {
        "\u{6df1}\u{5e02}A\u{80a1}"
    }
}

pub(crate) fn parse_number(value: Option<&&str>) -> Option<f64> {
    let raw = value?.trim();
    if raw.is_empty() || matches!(raw, "-" | "None" | "nan") {
        return None;
    }
    raw.parse::<f64>().ok().filter(|value| value.is_finite())
}

pub(crate) fn first_positive_number(values: &[Option<&&str>]) -> Option<f64> {
    values
        .iter()
        .find_map(|value| parse_number(*value).filter(|number| *number > 0.0))
}

pub(crate) fn estimate_roe(pe: Option<f64>, pb: Option<f64>) -> Option<f64> {
    let pe = pe.filter(|value| *value > 0.0)?;
    let pb = pb.filter(|value| *value > 0.0)?;
    Some(pb / pe)
}

pub(crate) fn object_f64(object: &serde_json::Map<String, Value>, key: &str) -> Option<f64> {
    object
        .get(key)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

pub(crate) fn cache_epoch_ms(value: Option<&Value>) -> Option<u128> {
    value.and_then(|value| {
        value.as_u64().map(u128::from).or_else(|| {
            value.as_str().and_then(|text| {
                let text = text.trim();
                text.parse::<u128>()
                    .ok()
                    .or_else(|| parse_cache_datetime_epoch_ms(text))
            })
        })
    })
}

pub(crate) fn parse_cache_datetime_epoch_ms(text: &str) -> Option<u128> {
    let date = text.get(0..10)?;
    let year = date.get(0..4)?.parse::<i32>().ok()?;
    let month = date.get(5..7)?.parse::<u32>().ok()?;
    let day = date.get(8..10)?.parse::<u32>().ok()?;
    if date.get(4..5)? != "-" || date.get(7..8)? != "-" {
        return None;
    }
    let mut hour = 0u32;
    let mut minute = 0u32;
    let mut second = 0u32;
    let mut millis = 0u32;
    let mut offset_seconds = 8 * 60 * 60;
    if text.len() >= 19 && matches!(text.as_bytes().get(10), Some(b'T' | b't' | b' ')) {
        hour = text.get(11..13)?.parse::<u32>().ok()?;
        minute = text.get(14..16)?.parse::<u32>().ok()?;
        second = text.get(17..19)?.parse::<u32>().ok()?;
        let mut rest = text.get(19..).unwrap_or("");
        if text.get(13..14)? != ":" || text.get(16..17)? != ":" {
            return None;
        }
        if let Some(fraction) = rest.strip_prefix('.') {
            let digits: String = fraction
                .chars()
                .take_while(|ch| ch.is_ascii_digit())
                .take(3)
                .collect();
            if !digits.is_empty() {
                let padded = format!("{digits:0<3}");
                millis = padded.parse::<u32>().ok()?;
            }
            rest = &fraction[digits.len()..];
        }
        if let Some(tz) = rest.strip_prefix('Z').or_else(|| rest.strip_prefix('z')) {
            let _ = tz;
            offset_seconds = 0;
        } else if rest.starts_with('+') || rest.starts_with('-') {
            offset_seconds = parse_timezone_offset_seconds(rest)?;
        }
    }
    if month == 0 || month > 12 || day == 0 || day > 31 || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let days = days_from_civil_epoch(year, month, day)?;
    let epoch_seconds =
        days * 86_400 + i128::from(hour * 3600 + minute * 60 + second) - i128::from(offset_seconds);
    if epoch_seconds < 0 {
        return None;
    }
    let epoch_ms = epoch_seconds
        .checked_mul(1000)?
        .checked_add(i128::from(millis))?;
    u128::try_from(epoch_ms).ok()
}

pub(crate) fn parse_timezone_offset_seconds(text: &str) -> Option<i32> {
    let sign = if text.starts_with('-') { -1 } else { 1 };
    let body = text.get(1..)?;
    let hour = body.get(0..2)?.parse::<i32>().ok()?;
    let minute = if body.get(2..3) == Some(":") {
        body.get(3..5).unwrap_or("00").parse::<i32>().ok()?
    } else if body.len() >= 4 {
        body.get(2..4)?.parse::<i32>().ok()?
    } else {
        0
    };
    if hour > 23 || minute > 59 {
        return None;
    }
    Some(sign * (hour * 3600 + minute * 60))
}

pub(crate) fn local_yyyymmdd_from_epoch_ms(epoch_ms: u128) -> Option<String> {
    let seconds = i128::try_from(epoch_ms / 1000).ok()? + 8 * 60 * 60;
    let days = seconds.div_euclid(86_400);
    let (year, month, day) = civil_from_days(days)?;
    Some(format!("{year:04}{month:02}{day:02}"))
}

pub(crate) fn market_quote_cache_stale(
    generated_at_epoch_ms: Option<u128>,
    now_epoch_ms: u128,
    quote_coverage_ratio: Option<f64>,
) -> bool {
    let Some(quote_date) = generated_at_epoch_ms.and_then(local_yyyymmdd_from_epoch_ms) else {
        return true;
    };
    let Some(expected_date) = expected_market_quote_date_from_epoch_ms(now_epoch_ms) else {
        return true;
    };
    let coverage_is_complete = quote_coverage_ratio.is_some_and(|ratio| {
        ratio.is_finite() && ratio + f64::EPSILON >= gp_core::MIN_MARKET_BREADTH_COVERAGE
    });
    quote_date != expected_date || !coverage_is_complete
}

pub(crate) fn expected_market_quote_date_from_epoch_ms(epoch_ms: u128) -> Option<String> {
    let seconds = i128::try_from(epoch_ms / 1000).ok()? + 8 * 60 * 60;
    let mut days = seconds.div_euclid(86_400);
    let seconds_in_day = seconds.rem_euclid(86_400);
    let minutes = seconds_in_day / 60;
    let weekday = weekday_from_days_since_epoch(days);
    if weekday == 6 {
        days -= 1;
    } else if weekday == 0 {
        days -= 2;
    } else if minutes < 9 * 60 + 30 {
        days -= 1;
    }
    while matches!(weekday_from_days_since_epoch(days), 0 | 6) {
        days -= 1;
    }
    let (year, month, day) = civil_from_days(days)?;
    Some(format!("{year:04}{month:02}{day:02}"))
}

pub(crate) fn weekday_from_days_since_epoch(days_since_epoch: i128) -> i128 {
    (days_since_epoch + 4).rem_euclid(7)
}

pub(crate) fn days_from_civil_epoch(year: i32, month: u32, day: u32) -> Option<i128> {
    let month_i = i128::from(month);
    let day_i = i128::from(day);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let y = i128::from(year) - i128::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = month_i + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2).div_euclid(5) + day_i - 1;
    let doe = yoe * 365 + yoe.div_euclid(4) - yoe.div_euclid(100) + doy;
    Some(era * 146_097 + doe - 719_468)
}

pub(crate) fn civil_from_days(days_since_epoch: i128) -> Option<(i32, u32, u32)> {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096).div_euclid(365);
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2).div_euclid(153);
    let day = doy - (153 * mp + 2).div_euclid(5) + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if month <= 2 { 1 } else { 0 };
    Some((
        i32::try_from(year).ok()?,
        u32::try_from(month).ok()?,
        u32::try_from(day).ok()?,
    ))
}

pub(crate) fn object_string(object: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    object.get(key).and_then(Value::as_str).map(str::to_string)
}

pub(crate) fn filter_seed_relations(seed: &Value, valid_codes: &HashSet<String>) -> Value {
    let mut relations = Vec::new();
    if let Some(items) = seed.get("relations").and_then(Value::as_array) {
        for item in items {
            let source = item
                .get("source_code")
                .and_then(Value::as_str)
                .and_then(normalize_stock_code);
            let target = item
                .get("target_code")
                .and_then(Value::as_str)
                .and_then(normalize_stock_code);
            if source
                .as_ref()
                .map(|code| valid_codes.contains(code))
                .unwrap_or(false)
                && target
                    .as_ref()
                    .map(|code| valid_codes.contains(code))
                    .unwrap_or(false)
            {
                relations.push(item.clone());
            }
        }
    }
    Value::Array(relations)
}

pub(crate) fn filter_seed_histories(seed: &Value, valid_codes: &HashSet<String>) -> Value {
    let mut histories = serde_json::Map::new();
    let benchmark_codes = adaptive_benchmark_codes()
        .into_iter()
        .collect::<HashSet<_>>();
    if let Some(items) = seed.get("histories").and_then(Value::as_object) {
        for (raw_code, history) in items {
            let Some(code) = normalize_stock_code(raw_code) else {
                continue;
            };
            if valid_codes.contains(&code) || benchmark_codes.contains(code.as_str()) {
                histories.insert(code, history.clone());
            }
        }
    }
    Value::Object(histories)
}

pub(crate) fn typed_history_cache_has_bars(
    data: &gp_core::CoreDataSet,
    code: &str,
    start_date: &str,
    end_date: &str,
    min_bars: usize,
) -> bool {
    let normalized = normalize_stock_code(code).unwrap_or_else(|| code.to_string());
    let Some(rows) = data
        .histories
        .get(code)
        .or_else(|| data.histories.get(&normalized))
    else {
        return false;
    };
    let start_key = compact_date_key(start_date).unwrap_or_else(|| "00000000".to_string());
    let end_key = compact_date_key(end_date).unwrap_or_else(|| "99999999".to_string());
    let mut dates = HashSet::new();
    rows.iter()
        .filter(|row| row.close.is_finite() && row.close > 0.0)
        .filter_map(|row| compact_date_key(&row.date))
        .filter(|date| date >= &start_key && date <= &end_key)
        .filter(|date| dates.insert(date.clone()))
        .take(min_bars)
        .count()
        >= min_bars
}

pub(crate) fn append_result_notes(result: &mut Value, extra_notes: Vec<String>) {
    if extra_notes.is_empty() || !result.is_object() {
        return;
    }
    let notes = result
        .as_object_mut()
        .expect("result object checked")
        .entry("notes".to_string())
        .or_insert_with(|| json!([]));
    if let Some(items) = notes.as_array_mut() {
        for note in extra_notes {
            if !note.trim().is_empty() {
                items.push(Value::String(note));
            }
        }
    }
}

pub(crate) fn main_fund_involvement(net_ratio: f64) -> &'static str {
    let magnitude = net_ratio.abs();
    if magnitude >= 8.0 {
        "高"
    } else if magnitude >= 3.0 {
        "中"
    } else {
        "低"
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
        let powershell_result = tokio::task::spawn_blocking(move || {
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

pub(crate) fn extract_json_after_var(html: &str, var_name: &str) -> Option<String> {
    let marker = format!("var {var_name}=");
    let start = html.find(&marker)? + marker.len();
    extract_balanced_json(&html[start..])
}

pub(crate) fn extract_balanced_json(raw: &str) -> Option<String> {
    let mut start = None;
    for (index, ch) in raw.char_indices() {
        if ch == '{' || ch == '[' {
            start = Some((index, ch));
            break;
        }
    }
    let (start_index, open) = start?;
    let close = if open == '{' { '}' } else { ']' };
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, ch) in raw[start_index..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
        } else if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return Some(raw[start_index..start_index + offset + ch.len_utf8()].to_string());
            }
        }
    }
    None
}

pub(crate) fn clean_html_text(value: &str) -> String {
    value
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

pub(crate) fn score_sentiment_label(score: f64) -> &'static str {
    if score >= 60.0 {
        "positive"
    } else if score <= 40.0 {
        "negative"
    } else {
        "uncertain"
    }
}

pub(crate) fn round2_value(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

pub(crate) fn format_amount_wan(value: f64) -> String {
    if value.abs() >= 100_000_000.0 {
        format!("{:.2} 亿", value / 100_000_000.0)
    } else if value.abs() >= 10_000.0 {
        format!("{:.2} 万", value / 10_000.0)
    } else {
        format_number_like(value)
    }
}

pub(crate) fn institution_buy_sell_ratio(buy: f64, sell: f64) -> String {
    if sell.abs() > f64::EPSILON {
        format_number_like(buy / sell)
    } else if buy > 0.0 {
        "∞".to_string()
    } else {
        "-".to_string()
    }
}

pub(crate) fn format_number_like(value: f64) -> String {
    if value.abs() >= 100.0 {
        format!("{value:.0}")
    } else if value.abs() >= 10.0 {
        format!("{value:.2}")
    } else {
        format!("{value:.3}")
    }
}

pub(crate) fn compact_count(value: f64) -> String {
    if value >= 10_000.0 {
        format!("{:.1}万", value / 10_000.0)
    } else {
        format_number_like(value)
    }
}

pub(crate) fn window_days(start_date: &str, end_date: &str) -> i64 {
    let start = compact_date_key(start_date)
        .and_then(|key| days_from_civil_key(&key))
        .unwrap_or(0);
    let end = compact_date_key(end_date)
        .and_then(|key| days_from_civil_key(&key))
        .unwrap_or(start);
    (end - start + 1).max(1)
}

pub(crate) fn fallback_today_date() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| (duration.as_secs() / 86_400) as i64)
        .unwrap_or(0);
    civil_date_from_days(days)
}

pub(crate) fn days_from_civil_key(key: &str) -> Option<i64> {
    if key.len() != 8 {
        return None;
    }
    let year = key.get(0..4)?.parse::<i32>().ok()?;
    let month = key.get(4..6)?.parse::<u32>().ok()?;
    let day = key.get(6..8)?.parse::<u32>().ok()?;
    Some(days_from_civil(year, month, day))
}

pub(crate) fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = year as i64 - if month <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = month as i64 + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub(crate) fn civil_date_from_days(days_since_epoch: i64) -> String {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if month <= 2 { 1 } else { 0 };
    format!("{year:04}-{month:02}-{day:02}")
}

pub(crate) struct QuarterlyEpsFetchResult {
    pub(crate) points: Vec<Value>,
    pub(crate) sources: Vec<String>,
    pub(crate) errors: Vec<String>,
}

pub(crate) async fn fetch_quarterly_eps_chain(code: &str) -> QuarterlyEpsFetchResult {
    let timeout = Duration::from_secs(FINANCIAL_REQUEST_TIMEOUT_SECS);
    let client = match build_tencent_http_client("Mozilla/5.0 GuXuanYou/0.3 financial", timeout) {
        Ok(client) => client,
        Err(error) => {
            return QuarterlyEpsFetchResult {
                points: Vec::new(),
                sources: Vec::new(),
                errors: vec![format!("财报 HTTP 客户端创建失败：{error}")],
            };
        }
    };
    let mut points = Vec::new();
    let mut sources = Vec::new();
    let mut errors = Vec::new();

    match fetch_ths_quarterly_eps(&client, code).await {
        Ok(rows) if !rows.is_empty() => {
            points.extend(rows);
            sources.push("同花顺财务摘要".to_string());
        }
        Ok(_) => errors.push("同花顺：返回空 EPS 明细".to_string()),
        Err(error) => errors.push(format!("同花顺：{error}")),
    }
    points = sort_dedup_quarterly_eps(points);

    if points.len() < COMPLETE_QUARTERLY_EPS_POINTS {
        match fetch_sina_quarterly_eps(&client, code).await {
            Ok(rows) if !rows.is_empty() => {
                points.extend(rows);
                sources.push("新浪财经财务指标".to_string());
            }
            Ok(_) => errors.push("新浪财经：返回空 EPS 明细".to_string()),
            Err(error) => errors.push(format!("新浪财经：{error}")),
        }
        points = sort_dedup_quarterly_eps(points);
    }

    if points.is_empty() {
        errors.push("东财财报源已禁用；仅保留同花顺/新浪与本地缓存参与财报补全。".to_string());
    }

    QuarterlyEpsFetchResult {
        points,
        sources,
        errors,
    }
}

pub(crate) async fn fetch_ths_quarterly_eps(
    client: &reqwest::Client,
    code: &str,
) -> Result<Vec<Value>, String> {
    let normalized =
        normalize_stock_code(code).ok_or_else(|| format!("无法识别同花顺财报代码：{code}"))?;
    let digits = normalized
        .get(..6)
        .ok_or_else(|| format!("无法识别同花顺财报代码：{code}"))?;
    let market =
        ths_market_code(&normalized).ok_or_else(|| format!("同花顺暂不支持该市场：{code}"))?;
    let market_string = market.to_string();
    let url = reqwest::Url::parse_with_params(
        THS_FINANCIAL_ENDPOINT,
        &[
            ("code", digits),
            ("id", "client_stock_importance"),
            ("market", market_string.as_str()),
            ("type", "stock"),
            ("page", "1"),
            ("size", "50"),
            ("period", "0"),
        ],
    )
    .map_err(|error| error.to_string())?;
    let url_text = url.to_string();
    let text = match client.get(url.clone()).send().await {
        Ok(response) if response.status().is_success() => {
            response.text().await.map_err(|error| error.to_string())?
        }
        Ok(response) => {
            let primary_error = format!("HTTP {}", response.status().as_u16());
            let bytes = powershell_http_get_bytes(&url_text, FINANCIAL_REQUEST_TIMEOUT_SECS)
                .map_err(|fallback_error| {
                    format!("{primary_error}; PowerShell fallback: {fallback_error}")
                })?;
            decode_utf8_lossy(bytes)
        }
        Err(error) => {
            let primary_error = error.to_string();
            let bytes = powershell_http_get_bytes(&url_text, FINANCIAL_REQUEST_TIMEOUT_SECS)
                .map_err(|fallback_error| {
                    format!("{primary_error}; PowerShell fallback: {fallback_error}")
                })?;
            decode_utf8_lossy(bytes)
        }
    };
    parse_ths_quarterly_eps_json(&text)
}

pub(crate) fn parse_ths_quarterly_eps_json(text: &str) -> Result<Vec<Value>, String> {
    let value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let rows = value
        .get("data")
        .and_then(|data| data.get("data"))
        .and_then(Value::as_array)
        .ok_or_else(|| "响应缺少 data.data".to_string())?;
    let mut points = Vec::new();
    for row in rows {
        let Some(object) = row.as_object() else {
            continue;
        };
        let period = object_string_any(
            object,
            &[
                "date",
                "report_date",
                "report",
                "report_name",
                "quarter_name",
            ],
        )
        .and_then(|raw| financial_period_from_text(&raw));
        let Some(period) = period else {
            continue;
        };
        let Some(index_list) = object.get("index_list").and_then(Value::as_object) else {
            continue;
        };
        for (metric_name, metric_values) in index_list {
            if !financial_metric_is_eps(metric_name) {
                continue;
            }
            if let Some(eps) = eps_metric_value(metric_values) {
                points.push(quarterly_eps_value(&period, eps, "同花顺财务摘要"));
                break;
            }
        }
    }
    Ok(sort_dedup_quarterly_eps(points))
}

pub(crate) async fn fetch_sina_quarterly_eps(
    client: &reqwest::Client,
    code: &str,
) -> Result<Vec<Value>, String> {
    let normalized =
        normalize_stock_code(code).ok_or_else(|| format!("无法识别新浪财报代码：{code}"))?;
    let digits = normalized
        .get(..6)
        .ok_or_else(|| format!("无法识别新浪财报代码：{code}"))?;
    let current_year = current_calendar_year_utc();
    let mut points = Vec::new();
    let mut errors = Vec::new();
    for year in [current_year, current_year - 1, current_year - 2] {
        let url = format!(
            "{SINA_FINANCIAL_GUIDELINE_ENDPOINT}/stockid/{digits}/ctrl/{year}/displaytype/4.phtml"
        );
        let bytes_result = match client.get(&url).send().await {
            Ok(response) if response.status().is_success() => response
                .bytes()
                .await
                .map(|bytes| bytes.to_vec())
                .map_err(|error| error.to_string()),
            Ok(response) => Err(format!("HTTP {}", response.status().as_u16())),
            Err(error) => Err(error.to_string()),
        };
        let bytes = match bytes_result {
            Ok(bytes) => bytes,
            Err(primary_error) => {
                match powershell_http_get_bytes(&url, FINANCIAL_REQUEST_TIMEOUT_SECS) {
                    Ok(bytes) => bytes,
                    Err(fallback_error) => {
                        errors.push(format!(
                            "{} 年 {}; PowerShell fallback: {}",
                            year, primary_error, fallback_error
                        ));
                        continue;
                    }
                }
            }
        };
        let (text, _, _) = encoding_rs::GBK.decode(&bytes);
        points.extend(parse_sina_quarterly_eps_html(&text));
        points = sort_dedup_quarterly_eps(points);
        if points.len() >= COMPLETE_QUARTERLY_EPS_POINTS {
            break;
        }
    }
    if points.is_empty() && !errors.is_empty() {
        return Err(errors.join("；"));
    }
    Ok(points)
}

pub(crate) fn parse_sina_quarterly_eps_html(text: &str) -> Vec<Value> {
    let html = text
        .replace("<TR", "<tr")
        .replace("</TR", "</tr")
        .replace("<TD", "<td")
        .replace("</TD", "</td")
        .replace("<TH", "<th")
        .replace("</TH", "</th");
    let mut header_periods: Vec<String> = Vec::new();
    let mut points = Vec::new();
    for row in html.split("<tr") {
        let cells = extract_html_cells(row);
        if cells.is_empty() {
            continue;
        }
        let periods = cells
            .iter()
            .filter_map(|cell| financial_period_from_text(cell))
            .collect::<Vec<_>>();
        if periods.len() >= 2 {
            header_periods = periods;
        }
        let first = cells.first().map(String::as_str).unwrap_or("");
        if !financial_metric_is_eps(first) || header_periods.is_empty() {
            continue;
        }
        for (period, value_cell) in header_periods.iter().zip(cells.iter().skip(1)) {
            if let Some(eps) = parse_f64_str(value_cell) {
                points.push(quarterly_eps_value(period, eps, "新浪财经财务指标"));
            }
        }
    }
    sort_dedup_quarterly_eps(points)
}

pub(crate) fn extract_html_cells(row: &str) -> Vec<String> {
    let lower = row.to_ascii_lowercase();
    let mut cells = Vec::new();
    let mut index = 0;
    while index < lower.len() {
        let next_td = lower[index..].find("<td").map(|pos| index + pos);
        let next_th = lower[index..].find("<th").map(|pos| index + pos);
        let Some(start) = (match (next_td, next_th) {
            (Some(td), Some(th)) => Some(td.min(th)),
            (Some(td), None) => Some(td),
            (None, Some(th)) => Some(th),
            (None, None) => None,
        }) else {
            break;
        };
        let tag = if lower[start..].starts_with("<th") {
            "th"
        } else {
            "td"
        };
        let Some(content_start) = lower[start..].find('>').map(|pos| start + pos + 1) else {
            break;
        };
        let close_tag = format!("</{tag}>");
        let Some(content_end) = lower[content_start..]
            .find(&close_tag)
            .map(|pos| content_start + pos)
        else {
            break;
        };
        let cell = strip_html_tags(&row[content_start..content_end]);
        if !cell.trim().is_empty() {
            cells.push(cell);
        }
        index = content_end + close_tag.len();
    }
    cells
}

pub(crate) fn strip_html_tags(value: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in value.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .replace("&amp;", "&")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn merge_basic_financial_from_stock(data: &mut Value, code: &str) -> bool {
    let Some(stock) = stock_object(data, code).cloned() else {
        return false;
    };
    let mut changed = false;
    let entry = financial_entry_mut(data, code);
    if !entry_contains_number(entry, "latest_eps") {
        if let Some(value) =
            object_number_any_loose(&stock, &["latest_eps", "eps", "EPSJB", "BASIC_EPS"])
        {
            entry.insert("latest_eps".to_string(), json!(value));
            changed = true;
        }
    }
    if !entry_contains_number(entry, "latest_bps") {
        if let Some(value) = object_number_any_loose(&stock, &["latest_bps", "bps", "BPS"]) {
            entry.insert("latest_bps".to_string(), json!(value));
            changed = true;
        }
    }
    if !entry.contains_key("period") {
        if let Some(period) =
            object_string_any(&stock, &["period", "latest_period", "report_period"])
                .and_then(|raw| financial_period_from_text(&raw))
        {
            entry.insert("period".to_string(), json!(period));
            changed = true;
        }
    }
    let period = object_string(entry, "period").and_then(|raw| financial_period_from_text(&raw));
    let eps = object_number_any_loose(entry, &["latest_eps"]);
    if let (Some(period), Some(eps)) = (period, eps) {
        changed |= upsert_entry_quarterly_eps(entry, &period, eps, "通达信基础财务");
    }
    if changed {
        append_financial_source(entry, "通达信基础财务");
    }
    changed
}

pub(crate) fn merge_quarterly_eps_points(data: &mut Value, code: &str, points: Vec<Value>) {
    let entry = financial_entry_mut(data, code);
    let mut merged = entry
        .get("quarterly_eps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    merged.extend(points);
    let normalized = sort_dedup_quarterly_eps(merged);
    if let Some(first) = normalized.first() {
        if !entry_contains_number(entry, "latest_eps") {
            if let Some(value) = first.get("value").and_then(Value::as_f64) {
                entry.insert("latest_eps".to_string(), json!(value));
            }
        }
        if !entry.contains_key("period") {
            if let Some(period) = first.get("period").and_then(Value::as_str) {
                entry.insert("period".to_string(), json!(period));
            }
        }
    }
    entry.insert("quarterly_eps".to_string(), Value::Array(normalized));
    append_financial_source(entry, "同花顺/新浪季度 EPS");
}

pub(crate) fn financial_quarterly_eps_count(data: &Value, code: &str) -> usize {
    data.get("financials")
        .and_then(Value::as_object)
        .and_then(|financials| financials.get(code))
        .and_then(|entry| entry.get("quarterly_eps"))
        .and_then(Value::as_array)
        .map(|rows| normalize_quarterly_eps(Some(&Value::Array(rows.clone()))).len())
        .unwrap_or(0)
}

pub(crate) fn financial_entry_mut<'a>(
    data: &'a mut Value,
    code: &str,
) -> &'a mut serde_json::Map<String, Value> {
    if !data.is_object() {
        *data = json!({});
    }
    let object = data.as_object_mut().expect("data object just initialized");
    let financials = object
        .entry("financials".to_string())
        .or_insert_with(|| json!({}));
    if !financials.is_object() {
        *financials = json!({});
    }
    let entry = financials
        .as_object_mut()
        .expect("financials object just initialized")
        .entry(code.to_string())
        .or_insert_with(|| json!({}));
    if !entry.is_object() {
        *entry = json!({});
    }
    entry
        .as_object_mut()
        .expect("financial entry object just initialized")
}

pub(crate) fn stock_object<'a>(data: &'a Value, code: &str) -> Option<&'a serde_json::Map<String, Value>> {
    data.get("stocks")?
        .as_array()?
        .iter()
        .filter_map(Value::as_object)
        .find(|stock| {
            stock
                .get("code")
                .and_then(Value::as_str)
                .and_then(normalize_stock_code)
                .as_deref()
                == Some(code)
        })
}

pub(crate) fn entry_contains_number(entry: &serde_json::Map<String, Value>, field: &str) -> bool {
    entry
        .get(field)
        .and_then(|value| json_f64(Some(value)))
        .map(|value| value.is_finite())
        .unwrap_or(false)
}

pub(crate) fn upsert_entry_quarterly_eps(
    entry: &mut serde_json::Map<String, Value>,
    period: &str,
    value: f64,
    source: &str,
) -> bool {
    let mut rows = entry
        .get("quarterly_eps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if rows.iter().any(|row| {
        row.get("period")
            .and_then(Value::as_str)
            .map(|existing| existing.eq_ignore_ascii_case(period))
            .unwrap_or(false)
    }) {
        return false;
    }
    rows.push(quarterly_eps_value(period, value, source));
    entry.insert(
        "quarterly_eps".to_string(),
        Value::Array(sort_dedup_quarterly_eps(rows)),
    );
    true
}

pub(crate) fn push_financial_note(data: &mut Value, code: &str, note: String) -> bool {
    let entry = financial_entry_mut(data, code);
    let notes = entry
        .entry("notes".to_string())
        .or_insert_with(|| json!([]));
    if !notes.is_array() {
        *notes = json!([]);
    }
    let rows = notes.as_array_mut().expect("notes array just initialized");
    if rows.iter().any(|row| row.as_str() == Some(note.as_str())) {
        return false;
    }
    rows.push(Value::String(note));
    true
}

pub(crate) fn append_financial_source(entry: &mut serde_json::Map<String, Value>, source: &str) {
    let mut sources = entry
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or("")
        .split('/')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if !sources.iter().any(|item| item == source) {
        sources.push(source.to_string());
    }
    if !sources.is_empty() {
        entry.insert("source".to_string(), json!(sources.join(" / ")));
    }
}

pub(crate) fn sort_dedup_quarterly_eps(points: Vec<Value>) -> Vec<Value> {
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    for point in points {
        let Some(object) = point.as_object() else {
            continue;
        };
        let Some(period) = object
            .get("period")
            .and_then(Value::as_str)
            .and_then(financial_period_from_text)
        else {
            continue;
        };
        let Some(value) = object.get("value").and_then(|value| json_f64(Some(value))) else {
            continue;
        };
        if !seen.insert(period.clone()) {
            continue;
        }
        let source = object
            .get("source")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("财报源");
        rows.push(quarterly_eps_value(&period, value, source));
    }
    rows.sort_by(|left, right| {
        let left_period = left.get("period").and_then(Value::as_str).unwrap_or("");
        let right_period = right.get("period").and_then(Value::as_str).unwrap_or("");
        right_period.cmp(left_period)
    });
    rows.truncate(12);
    rows
}

pub(crate) fn quarterly_eps_value(period: &str, value: f64, source: &str) -> Value {
    json!({
        "period": period,
        "value": value,
        "source": source,
    })
}

pub(crate) fn object_number_any_loose(
    object: &serde_json::Map<String, Value>,
    fields: &[&str],
) -> Option<f64> {
    fields
        .iter()
        .find_map(|field| object.get(*field).and_then(|value| json_f64(Some(value))))
}

pub(crate) fn eps_metric_value(value: &Value) -> Option<f64> {
    if let Some(object) = value.as_object() {
        for key in ["value", "data", "val", "num", "latest", "amount", "single"] {
            if let Some(number) = object.get(key).and_then(value_first_finite_number) {
                return Some(number);
            }
        }
    }
    value_first_finite_number(value)
}

pub(crate) fn value_first_finite_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(_) | Value::String(_) => json_f64(Some(value)),
        Value::Array(items) => items.iter().find_map(value_first_finite_number),
        Value::Object(object) => {
            for key in ["value", "data", "val", "num", "latest"] {
                if let Some(number) = object.get(key).and_then(value_first_finite_number) {
                    return Some(number);
                }
            }
            object.values().find_map(value_first_finite_number)
        }
        _ => None,
    }
}

pub(crate) fn financial_metric_is_eps(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    (name.contains("每股收益") || upper.contains("EPS")) && !name.contains("每股净资产")
}

pub(crate) fn financial_period_from_text(raw: &str) -> Option<String> {
    let trimmed = raw.trim().to_ascii_uppercase();
    if valid_financial_period_key(&trimmed) {
        return Some(trimmed);
    }
    let digits = trimmed
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.len() >= 8 {
        let year = digits.get(0..4)?.parse::<u32>().ok()?;
        let month = digits.get(4..6)?.parse::<u32>().ok()?;
        let quarter = match month {
            1..=3 => 1,
            4..=6 => 2,
            7..=9 => 3,
            10..=12 => 4,
            _ => return None,
        };
        if (2000..=2099).contains(&year) {
            return Some(format!("{year}Q{quarter}"));
        }
    }
    let year = trimmed
        .split(|ch: char| !ch.is_ascii_digit())
        .find(|part| part.len() == 4 && part.starts_with("20"))?;
    let quarter = if trimmed.contains("Q1") || trimmed.contains("一季") || trimmed.contains("第1季")
    {
        1
    } else if trimmed.contains("Q2")
        || trimmed.contains("二季")
        || trimmed.contains("中报")
        || trimmed.contains("半年")
    {
        2
    } else if trimmed.contains("Q3") || trimmed.contains("三季") || trimmed.contains("第3季") {
        3
    } else if trimmed.contains("Q4")
        || trimmed.contains("四季")
        || trimmed.contains("年报")
        || trimmed.contains("年度")
    {
        4
    } else {
        return None;
    };
    Some(format!("{year}Q{quarter}"))
}

pub(crate) fn ths_market_code(code: &str) -> Option<u16> {
    let normalized = normalize_stock_code(code)?;
    let digits = normalized.get(..6)?;
    if digits.starts_with("000")
        || digits.starts_with("001")
        || digits.starts_with("002")
        || digits.starts_with("003")
        || digits.starts_with("300")
        || digits.starts_with("301")
    {
        Some(33)
    } else if digits.starts_with("600")
        || digits.starts_with("601")
        || digits.starts_with("603")
        || digits.starts_with("605")
        || digits.starts_with("688")
    {
        Some(17)
    } else if digits.starts_with("920") || digits.starts_with("8") || digits.starts_with("4") {
        Some(151)
    } else {
        None
    }
}

pub(crate) fn current_calendar_year_utc() -> i32 {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| (duration.as_secs() / 86_400) as i64)
        .unwrap_or(0);
    civil_year_from_days(days)
}

pub(crate) fn civil_year_from_days(days_since_epoch: i64) -> i32 {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = mp + if mp < 10 { 3 } else { -9 };
    (y + if month <= 2 { 1 } else { 0 }) as i32
}

pub(crate) fn payload_history_rows(payload: &Value) -> Option<Vec<Value>> {
    let rows = payload.get("history")?.as_array()?;
    let normalized = rows
        .iter()
        .filter_map(normalize_history_bar_value)
        .collect::<Vec<_>>();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

pub(crate) fn normalize_history_bar_value(row: &Value) -> Option<Value> {
    let object = row.as_object()?;
    let date = normalize_history_date(object.get("date")?.as_str()?)?;
    let close = json_f64(object.get("close"))?;
    Some(json!({
        "date": date,
        "open": json_f64(object.get("open")).unwrap_or(close),
        "high": json_f64(object.get("high")).unwrap_or(close),
        "low": json_f64(object.get("low")).unwrap_or(close),
        "close": close,
        "volume": json_f64(object.get("volume")),
        "capital": json_f64(object.get("capital")),
    }))
}

pub(crate) fn insert_history_rows(data: &mut Value, code: &str, rows: Vec<Value>) {
    if !data.is_object() {
        *data = json!({});
    }
    let object = data.as_object_mut().expect("data object just initialized");
    let histories = object
        .entry("histories".to_string())
        .or_insert_with(|| json!({}));
    if !histories.is_object() {
        *histories = json!({});
    }
    histories
        .as_object_mut()
        .expect("histories object just initialized")
        .insert(code.to_string(), Value::Array(rows));
}

pub(crate) fn history_cache_has_bars(
    data: &Value,
    code: &str,
    start_date: &str,
    end_date: &str,
    min_bars: usize,
) -> bool {
    let Some(rows) = data
        .get("histories")
        .and_then(Value::as_object)
        .and_then(|histories| histories.get(code))
        .and_then(Value::as_array)
    else {
        return false;
    };
    let start_key = compact_date_key(start_date).unwrap_or_else(|| "00000000".to_string());
    let end_key = compact_date_key(end_date).unwrap_or_else(|| "99999999".to_string());
    rows.iter()
        .filter_map(|row| row.get("date").and_then(Value::as_str))
        .filter_map(compact_date_key)
        .filter(|date| date >= &start_key && date <= &end_key)
        .take(min_bars)
        .count()
        >= min_bars
}

pub(crate) async fn fetch_observe_daily_history(
    code: &str,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<Value>, String> {
    let timeout = Duration::from_secs(OBSERVE_HISTORY_TIMEOUT_SECS);
    let client = build_tencent_http_client("Mozilla/5.0 GuXuanYou/0.3 observe history", timeout)?;
    let mut errors = Vec::new();
    match fetch_tencent_daily_history(&client, code, start_date, end_date).await {
        Ok(rows) if !rows.is_empty() => return Ok(rows),
        Ok(_) => errors.push("腾讯日线返回空数据".to_string()),
        Err(error) => errors.push(format!("腾讯日线：{error}")),
    }
    match fetch_eastmoney_daily_history(&client, code, start_date, end_date).await {
        Ok(rows) if !rows.is_empty() => return Ok(rows),
        Ok(_) => errors.push("东方财富日线返回空数据".to_string()),
        Err(error) => errors.push(format!("东方财富日线：{error}")),
    }
    Err(errors.join("\u{ff1b}"))
}

pub(crate) async fn fetch_daily_history_text(
    client: &reqwest::Client,
    url: &str,
    label: &str,
) -> Result<String, String> {
    let primary_error = match client.get(url).send().await {
        Ok(response) if response.status().is_success() => match response.text().await {
            Ok(text) => return Ok(text),
            Err(error) => format!("{label} response read failed: {error}"),
        },
        Ok(response) => format!("{label} HTTP {}", response.status().as_u16()),
        Err(error) => format!("{label} request failed: {error}"),
    };

    #[cfg(windows)]
    {
        let fallback_url = url.to_string();
        let bytes = tokio::task::spawn_blocking(move || {
            powershell_http_get_bytes(&fallback_url, OBSERVE_HISTORY_TIMEOUT_SECS)
        })
        .await
        .map_err(|error| format!("{primary_error}; PowerShell fallback task failed: {error}"))?
        .map_err(|error| format!("{primary_error}; PowerShell fallback failed: {error}"))?;
        return Ok(decode_utf8_lossy(bytes));
    }

    #[cfg(not(windows))]
    Err(primary_error)
}

pub(crate) async fn fetch_eastmoney_daily_history(
    client: &reqwest::Client,
    code: &str,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<Value>, String> {
    let digits = code
        .get(..6)
        .ok_or_else(|| format!("\u{65e0}\u{6548}\u{80a1}\u{7968}\u{4ee3}\u{7801}\u{ff1a}{code}"))?;
    let market = eastmoney_market_code(code).ok_or_else(|| {
        format!("\u{65e0}\u{6cd5}\u{8bc6}\u{522b}\u{884c}\u{60c5}\u{4ee3}\u{7801}\u{ff1a}{code}")
    })?;
    let secid = format!("{market}.{digits}");
    let beg = compact_date_key(start_date).unwrap_or_else(|| "0".to_string());
    let end = compact_date_key(end_date).unwrap_or_else(|| "20500000".to_string());
    let url = format!(
        "{EASTMONEY_KLINE_ENDPOINT}?fields1=f1,f2,f3,f4,f5,f6&fields2=f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61&ut=7eea3edcaed734bea9cbfc24409ed989&klt=101&fqt=0&secid={secid}&beg={beg}&end={end}",
    );
    let text = fetch_daily_history_text(client, &url, "Eastmoney daily history").await?;
    let value: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    Ok(value
        .get("data")
        .and_then(|data| data.get("klines"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(parse_eastmoney_kline_row)
        .collect())
}

pub(crate) async fn fetch_tencent_daily_history(
    client: &reqwest::Client,
    code: &str,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<Value>, String> {
    let symbol = tencent_symbol(code).ok_or_else(|| {
        format!("\u{65e0}\u{6cd5}\u{8bc6}\u{522b}\u{884c}\u{60c5}\u{4ee3}\u{7801}\u{ff1a}{code}")
    })?;
    let param = tencent_daily_history_param(&symbol);
    let url = format!(
        "{TENCENT_DAILY_KLINE_ENDPOINT}?param={}",
        param.replace(',', "%2C")
    );
    let text = fetch_daily_history_text(client, &url, "Tencent daily history").await?;
    let value: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    let rows = value
        .get("data")
        .and_then(|data| data.get(&symbol))
        .and_then(|stock| stock.get("day").or_else(|| stock.get("qfqday")))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(parse_tencent_kline_row)
        .collect();
    Ok(filter_daily_history_rows_by_date(
        rows, start_date, end_date,
    ))
}

pub(crate) fn tencent_daily_history_param(symbol: &str) -> String {
    format!("{symbol},day,,,{OBSERVE_DAILY_HISTORY_LIMIT},")
}

pub(crate) fn filter_daily_history_rows_by_date(
    rows: Vec<Value>,
    start_date: &str,
    end_date: &str,
) -> Vec<Value> {
    let start_key = compact_date_key(start_date).unwrap_or_else(|| "00000000".to_string());
    let end_key = compact_date_key(end_date).unwrap_or_else(|| "99999999".to_string());
    rows.into_iter()
        .filter(|row| {
            let Some(date) = row
                .get("date")
                .and_then(Value::as_str)
                .and_then(compact_date_key)
            else {
                return false;
            };
            date.as_str() >= start_key.as_str() && date.as_str() <= end_key.as_str()
        })
        .collect()
}

pub(crate) fn eastmoney_market_code(code: &str) -> Option<u8> {
    if code.ends_with(".SH") {
        Some(1)
    } else if code.ends_with(".SZ") || code.ends_with(".BJ") {
        Some(0)
    } else {
        None
    }
}

pub(crate) fn parse_eastmoney_kline_row(raw: &str) -> Option<Value> {
    let parts = raw.split(',').collect::<Vec<_>>();
    if parts.len() < 7 {
        return None;
    }
    let close = parse_f64_str(parts.get(2).copied()?)?;
    Some(json!({
        "date": normalize_history_date(parts.first().copied()?)?,
        "open": parse_f64_str(parts.get(1).copied()?).unwrap_or(close),
        "close": close,
        "high": parse_f64_str(parts.get(3).copied()?).unwrap_or(close),
        "low": parse_f64_str(parts.get(4).copied()?).unwrap_or(close),
        "volume": parse_f64_str(parts.get(5).copied()?),
        "capital": Value::Null,
    }))
}

pub(crate) fn parse_tencent_kline_row(raw: &Value) -> Option<Value> {
    let parts = raw.as_array()?;
    let close = json_f64(parts.get(2))?;
    Some(json!({
        "date": normalize_history_date(parts.first()?.as_str()?)?,
        "open": json_f64(parts.get(1)).unwrap_or(close),
        "close": close,
        "high": json_f64(parts.get(3)).unwrap_or(close),
        "low": json_f64(parts.get(4)).unwrap_or(close),
        "volume": json_f64(parts.get(5)),
        "capital": Value::Null,
    }))
}

pub(crate) fn json_f64(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64().filter(|value| value.is_finite()),
        Value::String(raw) => parse_f64_str(raw),
        _ => None,
    }
}

pub(crate) fn parse_f64_str(raw: &str) -> Option<f64> {
    let value = raw.trim().replace(',', "");
    if value.is_empty() || matches!(value.as_str(), "-" | "None" | "nan") {
        return None;
    }
    value.parse::<f64>().ok().filter(|value| value.is_finite())
}

pub(crate) fn normalize_history_date(raw: &str) -> Option<String> {
    let key = compact_date_key(raw)?;
    Some(format!("{}-{}-{}", &key[..4], &key[4..6], &key[6..8]))
}

pub(crate) fn compact_date_key(raw: &str) -> Option<String> {
    let digits = raw
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .take(8)
        .collect::<String>();
    if digits.len() == 8 {
        Some(digits)
    } else {
        None
    }
}

pub(crate) fn cached_market_data(app: &tauri::AppHandle) -> Result<Value, String> {
    let cache = read_mobile_market_data(app)?;
    if !cache
        .get("exists")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err("股票池为空，请先联网更新股票池。".to_string());
    }
    cache
        .get("data")
        .cloned()
        .ok_or_else(|| "股票池缓存缺少 data 字段，请清理缓存后重新联网更新。".to_string())
}

pub(crate) fn strip_core_side_payload_fields(mut payload: Value) -> Value {
    if let Some(object) = payload.as_object_mut() {
        object.remove("financial_snapshot");
    }
    payload
}

pub(crate) fn market_data_status(app: &tauri::AppHandle) -> Result<Value, String> {
    let cache = read_mobile_market_data_record(app, false)?;
    let exists = cache
        .get("exists")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !exists {
        return Ok(json!({
            "source": "tencent",
            "universe_count": 0,
            "cache_bytes": 0,
            "cache_limit_bytes": 0,
            "universe_updated_at": Value::Null,
            "quote_generated_at": Value::Null,
            "quote_trade_date": Value::Null,
            "current_trade_date": local_yyyymmdd_from_epoch_ms(epoch_millis()),
            "stale": true,
            "policy": { "mode": "empty" },
            "notes": ["mobile market cache is empty; refresh is required"]
        }));
    }
    let summary = cache.get("summary").cloned().unwrap_or_else(|| json!({}));
    let stock_count = summary
        .get("stock_count")
        .and_then(Value::as_u64)
        .or_else(|| cache.get("stock_count").and_then(Value::as_u64))
        .unwrap_or(0);
    let mut notes = vec![format!(
        "Tauri/Rust cached universe currently contains {stock_count} stocks."
    )];
    if let Some(data_notes) = cache.get("data_notes").and_then(Value::as_array) {
        notes.extend(
            data_notes
                .iter()
                .filter_map(Value::as_str)
                .take(2)
                .map(ToOwned::to_owned),
        );
    }
    if let Some(warnings) = summary.get("warnings").and_then(Value::as_array) {
        notes.extend(
            warnings
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned),
        );
    }
    let generated_at_epoch_ms = cache_epoch_ms(cache.get("generated_at_epoch_ms"))
        .or_else(|| cache_epoch_ms(cache.get("generated_at")));
    let updated_at_epoch_ms = cache_epoch_ms(cache.get("updated_at_epoch_ms"));
    let now_epoch_ms = epoch_millis();
    let quote_date = generated_at_epoch_ms.and_then(local_yyyymmdd_from_epoch_ms);
    let current_date = expected_market_quote_date_from_epoch_ms(now_epoch_ms);
    let quote_coverage_ratio = cache.get("quote_coverage_ratio").and_then(Value::as_f64);
    let quote_requested = cache
        .get("quote_requested")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let quote_observed = cache
        .get("quote_observed")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let stale = market_quote_cache_stale(generated_at_epoch_ms, now_epoch_ms, quote_coverage_ratio);
    if stale {
        notes.push(format!(
            "cached Tencent quote snapshot is stale or incomplete: {quote_observed}/{quote_requested} same-day changes; refresh before rotation screening"
        ));
    }
    Ok(json!({
        "source": "tencent",
        "universe_count": stock_count,
        "cache_bytes": cache.get("bytes").and_then(Value::as_u64).unwrap_or(0),
        "cache_limit_bytes": 0,
        "universe_updated_at": updated_at_epoch_ms.map(|value| json!(value)).unwrap_or(Value::Null),
        "quote_generated_at": generated_at_epoch_ms.map(|value| json!(value)).unwrap_or(Value::Null),
        "quote_trade_date": quote_date,
        "quote_coverage_trade_date": cache.get("quote_coverage_trade_date").cloned().unwrap_or(Value::Null),
        "quote_requested": quote_requested,
        "quote_observed": quote_observed,
        "quote_coverage_ratio": quote_coverage_ratio.map(|value| json!(value)).unwrap_or(Value::Null),
        "current_trade_date": current_date,
        "stale": stale,
        "policy": { "mode": "tauri_native", "source": "cache" },
        "notes": notes
    }))
}

pub(crate) fn read_mobile_market_data(app: &tauri::AppHandle) -> Result<Value, String> {
    read_mobile_market_data_record(app, true)
}

pub(crate) fn read_mobile_market_data_record(
    app: &tauri::AppHandle,
    include_data: bool,
) -> Result<Value, String> {
    let path = mobile_market_data_path(app)?;
    if !path.exists() {
        forget_mobile_market_data_cache(&path);
        return Ok(json!({
            "exists": false,
            "bytes": 0,
            "path": path.display().to_string(),
            "notes": ["mobile market cache is empty"]
        }));
    }
    let bytes = fs::metadata(&path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let modified_at_epoch_ms = file_modified_millis(&path);
    if let Some(entry) = cached_mobile_market_data_entry(&path, bytes, modified_at_epoch_ms) {
        return Ok(market_cache_record(
            &path,
            entry.bytes,
            entry.modified_at_epoch_ms,
            entry.summary,
            entry.data.as_ref(),
            include_data,
            "mobile market cache loaded from memory",
        ));
    }

    let _update_guard = mobile_market_update_lock()
        .lock()
        .map_err(|_| "mobile market update lock is poisoned".to_string())?;
    let bytes = fs::metadata(&path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let modified_at_epoch_ms = file_modified_millis(&path);
    if let Some(entry) = cached_mobile_market_data_entry(&path, bytes, modified_at_epoch_ms) {
        return Ok(market_cache_record(
            &path,
            entry.bytes,
            entry.modified_at_epoch_ms,
            entry.summary,
            entry.data.as_ref(),
            include_data,
            "mobile market cache loaded from memory",
        ));
    }

    let mut data = read_json_file(&path)?;
    apply_persisted_market_data_patches(app, &mut data)?;
    let (typed, summary) = parse_mobile_market_data_snapshot(&data, "cached mobile market data")?;
    remember_mobile_market_data_cache(&path, bytes, modified_at_epoch_ms, &data, typed, &summary);
    Ok(market_cache_record(
        &path,
        bytes,
        modified_at_epoch_ms,
        summary,
        &data,
        include_data,
        "mobile market cache loaded",
    ))
}

pub(crate) fn write_mobile_market_data(app: &tauri::AppHandle, payload: Value) -> Result<Value, String> {
    write_mobile_market_data_record(app, payload, true)
}

pub(crate) fn write_mobile_market_data_record(
    app: &tauri::AppHandle,
    payload: Value,
    include_data: bool,
) -> Result<Value, String> {
    let _update_guard = mobile_market_update_lock()
        .lock()
        .map_err(|_| "mobile market update lock is poisoned".to_string())?;
    let (typed, summary) = parse_mobile_market_data_snapshot(&payload, "mobile market data")?;
    let path = mobile_market_data_path(app)?;
    let root = path
        .parent()
        .ok_or_else(|| "mobile market cache path has no parent".to_string())?;
    fs::create_dir_all(root)
        .map_err(|error| format!("create mobile market cache dir failed: {error}"))?;
    let tmp_path = root.join(format!(
        "{}.tmp-{}",
        MOBILE_MARKET_DATA_FILE,
        epoch_millis()
    ));
    let bytes = serde_json::to_vec(&payload)
        .map_err(|error| format!("serialize mobile market data failed: {error}"))?;
    write_mobile_market_data_with_retry(&tmp_path, &path, &bytes)?;
    clear_mobile_market_data_patches(app)?;
    remember_mobile_market_data_cache(
        &path,
        bytes.len() as u64,
        file_modified_millis(&path),
        &payload,
        typed,
        &summary,
    );
    remember_refresh_seed(app, &payload);
    Ok(market_cache_record(
        &path,
        bytes.len() as u64,
        file_modified_millis(&path),
        summary,
        &payload,
        include_data,
        "mobile market cache written",
    ))
}

pub(crate) fn retry_with_attempts<T, F>(attempts: usize, delay_ms: u64, mut op: F) -> Result<T, String>
where
    F: FnMut(usize) -> Result<T, String>,
{
    let attempts = attempts.max(1);
    let mut last_error: Option<String> = None;
    for attempt in 0..attempts {
        match op(attempt) {
            Ok(value) => return Ok(value),
            Err(error) => {
                last_error = Some(error);
                if attempt + 1 < attempts {
                    std::thread::sleep(Duration::from_millis(delay_ms));
                }
            }
        }
    }
    Err(last_error.unwrap_or_else(|| "operation failed".to_string()))
}

pub(crate) fn write_mobile_market_data_with_retry(
    tmp_path: &Path,
    path: &Path,
    bytes: &[u8],
) -> Result<(), String> {
    retry_with_attempts(
        MOBILE_MARKET_WRITE_RETRY_ATTEMPTS,
        MOBILE_MARKET_WRITE_RETRY_DELAY_MS,
        |_| write_mobile_market_data_once(tmp_path, path, bytes),
    )
}

pub(crate) fn write_mobile_market_data_once(tmp_path: &Path, path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::write(tmp_path, bytes)
        .map_err(|error| format!("write mobile market cache temp failed: {error}"))?;
    if path.exists() {
        fs::remove_file(path)
            .map_err(|error| format!("replace old mobile market cache failed: {error}"))?;
    }
    fs::rename(tmp_path, path)
        .map_err(|error| format!("commit mobile market cache failed: {error}"))?;
    Ok(())
}

pub(crate) fn market_quote_coverage(data: &Value, target_date: Option<&str>) -> (usize, usize, f64) {
    let stocks = data
        .get("stocks")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let requested = stocks.len();
    let target = target_date.and_then(compact_date_key);
    let observed = target
        .as_ref()
        .map(|target| {
            stocks
                .iter()
                .filter(|stock| {
                    stock
                        .get("quote_time")
                        .and_then(Value::as_str)
                        .and_then(compact_date_key)
                        .is_some_and(|quote_date| quote_date == *target)
                        && stock
                            .get("change_pct")
                            .and_then(Value::as_f64)
                            .is_some_and(f64::is_finite)
                })
                .count()
        })
        .unwrap_or(0);
    let ratio = if requested == 0 {
        0.0
    } else {
        observed as f64 / requested as f64
    };
    (requested, observed, ratio)
}

pub(crate) fn market_cache_record(
    path: &Path,
    bytes: u64,
    updated_at_epoch_ms: Option<u128>,
    summary: Value,
    data: &Value,
    include_data: bool,
    note: &str,
) -> Value {
    let stock_count = summary
        .get("stock_count")
        .and_then(Value::as_u64)
        .or_else(|| {
            data.get("stocks")
                .and_then(Value::as_array)
                .map(|stocks| stocks.len() as u64)
        })
        .unwrap_or(0);
    let generated_at = data
        .get("generated_at_epoch_ms")
        .cloned()
        .or_else(|| data.get("generated_at").cloned())
        .unwrap_or(Value::Null);
    let data_notes = data.get("notes").cloned().unwrap_or_else(|| json!([]));
    let generated_at_epoch_ms = cache_epoch_ms(Some(&generated_at));
    let quote_coverage_trade_date = generated_at_epoch_ms.and_then(local_yyyymmdd_from_epoch_ms);
    let (quote_requested, quote_observed, quote_coverage_ratio) =
        market_quote_coverage(data, quote_coverage_trade_date.as_deref());
    let mut record = serde_json::Map::new();
    record.insert("exists".to_string(), json!(true));
    record.insert("bytes".to_string(), json!(bytes));
    record.insert("path".to_string(), json!(path.display().to_string()));
    record.insert(
        "updated_at_epoch_ms".to_string(),
        updated_at_epoch_ms
            .map(|value| json!(value))
            .unwrap_or(Value::Null),
    );
    record.insert("summary".to_string(), summary);
    record.insert("stock_count".to_string(), json!(stock_count));
    record.insert("generated_at".to_string(), generated_at);
    record.insert(
        "generated_at_epoch_ms".to_string(),
        generated_at_epoch_ms
            .map(|value| json!(value))
            .unwrap_or(Value::Null),
    );
    record.insert(
        "quote_coverage_trade_date".to_string(),
        quote_coverage_trade_date
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    record.insert("quote_requested".to_string(), json!(quote_requested));
    record.insert("quote_observed".to_string(), json!(quote_observed));
    record.insert(
        "quote_coverage_ratio".to_string(),
        json!(quote_coverage_ratio),
    );
    record.insert("data_notes".to_string(), data_notes);
    if include_data {
        record.insert("data".to_string(), data.clone());
    }
    record.insert("notes".to_string(), json!([note]));
    Value::Object(record)
}

pub(crate) fn cached_mobile_market_data_entry(
    path: &Path,
    bytes: u64,
    modified_at_epoch_ms: Option<u128>,
) -> Option<MobileMarketDataCacheEntry> {
    let slot = mobile_market_data_cache().lock().ok()?;
    let entry = slot.get(path)?;
    if entry.bytes == bytes && entry.modified_at_epoch_ms == modified_at_epoch_ms {
        Some(entry.clone())
    } else {
        None
    }
}

pub(crate) fn market_data_patch_for_codes(data: &Value, codes: &[String]) -> Value {
    let codes = codes
        .iter()
        .filter_map(|code| normalize_stock_code(code))
        .collect::<HashSet<_>>();
    let stocks = data
        .get("stocks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|stock| {
            stock
                .get("code")
                .and_then(Value::as_str)
                .and_then(normalize_stock_code)
                .map(|code| codes.contains(&code))
                .unwrap_or(false)
        })
        .cloned()
        .collect::<Vec<_>>();
    let section = |name: &str| {
        let mut selected = serde_json::Map::new();
        if let Some(items) = data.get(name).and_then(Value::as_object) {
            for (raw_code, value) in items {
                if normalize_stock_code(raw_code)
                    .map(|code| codes.contains(&code))
                    .unwrap_or(false)
                {
                    selected.insert(raw_code.clone(), value.clone());
                }
            }
        }
        Value::Object(selected)
    };
    json!({
        "schema_version": 1,
        "updated_at_epoch_ms": epoch_millis(),
        "stocks": stocks,
        "histories": section("histories"),
        "financials": section("financials"),
        "factor_snapshots": section("factor_snapshots"),
        "capital_evidence": section("capital_evidence")
    })
}

pub(crate) fn apply_market_data_patch(data: &mut Value, patch: &Value) {
    let Some(target) = data.as_object_mut() else {
        return;
    };
    if let Some(patch_stocks) = patch.get("stocks").and_then(Value::as_array) {
        let mut replacements = patch_stocks
            .iter()
            .filter_map(|stock| {
                let code = stock
                    .get("code")
                    .and_then(Value::as_str)
                    .and_then(normalize_stock_code)?;
                Some((code, stock.clone()))
            })
            .collect::<HashMap<_, _>>();
        let stocks = target
            .entry("stocks".to_string())
            .or_insert_with(|| json!([]));
        if let Some(items) = stocks.as_array_mut() {
            for item in items.iter_mut() {
                let Some(code) = item
                    .get("code")
                    .and_then(Value::as_str)
                    .and_then(normalize_stock_code)
                else {
                    continue;
                };
                if let Some(replacement) = replacements.remove(&code) {
                    *item = replacement;
                }
            }
            items.extend(replacements.into_values());
        }
    }
    for name in [
        "histories",
        "financials",
        "factor_snapshots",
        "capital_evidence",
    ] {
        let Some(patch_items) = patch.get(name).and_then(Value::as_object) else {
            continue;
        };
        let target_items = target.entry(name.to_string()).or_insert_with(|| json!({}));
        if let Some(target_items) = target_items.as_object_mut() {
            for (code, value) in patch_items {
                target_items.insert(code.clone(), value.clone());
            }
        }
    }
}

pub(crate) fn mobile_market_patch_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let path = mobile_market_data_path(app)?;
    let root = path
        .parent()
        .ok_or_else(|| "mobile market cache path has no parent".to_string())?;
    Ok(root.join(MOBILE_MARKET_PATCH_DIR))
}

pub(crate) fn apply_persisted_market_data_patches(
    app: &tauri::AppHandle,
    data: &mut Value,
) -> Result<usize, String> {
    let root = mobile_market_patch_dir(app)?;
    if !root.exists() {
        return Ok(0);
    }
    let mut paths = fs::read_dir(&root)
        .map_err(|error| format!("read mobile market patches failed: {error}"))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    paths.sort();
    let mut applied = 0usize;
    for path in paths {
        let patch = read_json_file(&path)?;
        apply_market_data_patch(data, &patch);
        applied += 1;
    }
    Ok(applied)
}

pub(crate) fn clear_mobile_market_data_patches(app: &tauri::AppHandle) -> Result<(), String> {
    let root = mobile_market_patch_dir(app)?;
    if root.exists() {
        fs::remove_dir_all(&root)
            .map_err(|error| format!("clear mobile market patches failed: {error}"))?;
    }
    Ok(())
}

pub(crate) fn market_data_patch_codes(patch: &Value) -> Vec<String> {
    let mut codes = HashSet::new();
    if let Some(stocks) = patch.get("stocks").and_then(Value::as_array) {
        codes.extend(stocks.iter().filter_map(|stock| {
            stock
                .get("code")
                .and_then(Value::as_str)
                .and_then(normalize_stock_code)
        }));
    }
    for section in [
        "histories",
        "financials",
        "factor_snapshots",
        "capital_evidence",
    ] {
        if let Some(items) = patch.get(section).and_then(Value::as_object) {
            codes.extend(items.keys().filter_map(|code| normalize_stock_code(code)));
        }
    }
    let mut codes = codes.into_iter().collect::<Vec<_>>();
    codes.sort();
    codes
}

pub(crate) fn persist_market_data_patch_sync(app: &tauri::AppHandle, patch: &Value) -> Result<usize, String> {
    let root = mobile_market_patch_dir(app)?;
    fs::create_dir_all(&root)
        .map_err(|error| format!("create mobile market patch dir failed: {error}"))?;
    let mut written = 0usize;
    for code in market_data_patch_codes(patch) {
        let safe_code = sanitize_path_part(&code);
        let path = root.join(format!("{safe_code}.json"));
        let mut merged_patch = if path.exists() {
            read_json_file(&path)?
        } else {
            json!({})
        };
        let code_patch = market_data_patch_for_codes(patch, std::slice::from_ref(&code));
        apply_market_data_patch(&mut merged_patch, &code_patch);
        let bytes = serde_json::to_vec(&merged_patch)
            .map_err(|error| format!("serialize mobile market patch failed: {error}"))?;
        let tmp_path = root.join(format!("{safe_code}.tmp-{}", epoch_millis()));
        write_mobile_market_data_once(&tmp_path, &path, &bytes)?;
        written += 1;
    }
    Ok(written)
}

pub(crate) async fn persist_market_data_patch_updates(
    app: tauri::AppHandle,
    patch: Value,
) -> Result<usize, String> {
    crate::runtime::run_io_bound("persist market data patches", move || {
        let _update_guard = mobile_market_update_lock()
            .lock()
            .map_err(|_| "mobile market update lock is poisoned".to_string())?;
        let written = persist_market_data_patch_sync(&app, &patch)?;
        let path = mobile_market_data_path(&app)?;
        let mut data = if let Ok(slot) = mobile_market_data_cache().lock() {
            slot.get(&path)
                .map(|entry| entry.data.as_ref().clone())
                .unwrap_or(Value::Null)
        } else {
            Value::Null
        };
        if data.is_null() {
            data = read_json_file(&path)?;
            apply_persisted_market_data_patches(&app, &mut data)?;
        } else {
            apply_market_data_patch(&mut data, &patch);
        }
        let bytes = fs::metadata(&path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let modified_at_epoch_ms = file_modified_millis(&path);
        let (typed, summary) = parse_mobile_market_data_snapshot(&data, "updated market data")?;
        remember_mobile_market_data_cache(
            &path,
            bytes,
            modified_at_epoch_ms,
            &data,
            typed,
            &summary,
        );
        Ok(written)
    })
    .await?
}

pub(crate) async fn persist_market_data_updates(
    app: tauri::AppHandle,
    data: Value,
    codes: Vec<String>,
) -> Result<usize, String> {
    let patch = market_data_patch_for_codes(&data, &codes);
    persist_market_data_patch_updates(app, patch).await
}

pub(crate) fn parse_mobile_market_data_snapshot(
    data: &Value,
    label: &str,
) -> Result<(Arc<gp_core::CoreDataSet>, Value), String> {
    let typed = serde_json::from_value::<gp_core::CoreDataSet>(data.clone())
        .map_err(|error| format!("{label} parse failed: {error}"))?;
    let summary = gp_core::validate_data_set(&typed)
        .map_err(|error| format!("{label} validation failed: {error}"))?;
    let summary = serde_json::to_value(summary)
        .map_err(|error| format!("{label} summary serialization failed: {error}"))?;
    Ok((Arc::new(typed), summary))
}

pub(crate) fn cached_market_data_snapshot(
    app: &tauri::AppHandle,
) -> Result<Arc<gp_core::CoreDataSet>, String> {
    let path = mobile_market_data_path(app)?;
    let cache = read_mobile_market_data_record(app, false)?;
    if !cache
        .get("exists")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err("股票池为空，请先联网更新股票池。".to_string());
    }
    let slot = mobile_market_data_cache()
        .lock()
        .map_err(|_| "mobile market cache lock is poisoned".to_string())?;
    slot.get(&path)
        .map(|entry| Arc::clone(&entry.typed))
        .ok_or_else(|| "mobile market typed snapshot is unavailable".to_string())
}

pub(crate) fn remember_mobile_market_data_cache(
    path: &Path,
    bytes: u64,
    modified_at_epoch_ms: Option<u128>,
    data: &Value,
    typed: Arc<gp_core::CoreDataSet>,
    summary: &Value,
) {
    if let Ok(mut slot) = mobile_market_data_cache().lock() {
        slot.insert(
            path.to_path_buf(),
            MobileMarketDataCacheEntry {
                bytes,
                modified_at_epoch_ms,
                data: Arc::new(data.clone()),
                typed,
                summary: summary.clone(),
            },
        );
    }
}

pub(crate) fn forget_mobile_market_data_cache(path: &Path) {
    if let Ok(mut slot) = mobile_market_data_cache().lock() {
        slot.remove(path);
    }
}

pub(crate) fn optional_trimmed_string(value: Option<&Value>) -> Option<String> {
    let trimmed = value?.as_str()?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub(crate) fn mobile_market_data_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("get app data dir failed: {error}"))?;
    root.push("market");
    root.push(MOBILE_MARKET_DATA_FILE);
    Ok(root)
}

pub(crate) fn file_modified_millis(path: &Path) -> Option<u128> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis())
}

pub(crate) fn read_json_file(path: &Path) -> Result<Value, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("读取 JSON 失败：{}：{error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("解析 JSON 失败：{}：{error}", path.display()))
}

pub(crate) fn find_first_current_manifest(root: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path().join("current_manifest.json");
        if path.exists() {
            return Some(path);
        }
    }
    None
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

pub(crate) fn sanitize_path_part(value: &str) -> String {
    let mut part: String = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
        .take(120)
        .collect();
    if part.is_empty() {
        part.push_str("unknown");
    }
    part
}
