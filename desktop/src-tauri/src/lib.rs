use base64::{engine::general_purpose, Engine as _};
use futures::stream::{self, FuturesUnordered, StreamExt};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
#[cfg(not(mobile))]
use std::process::Command;
#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use stock_optimizer_core as gp_core;
use tauri::{AppHandle, Emitter, Manager};

#[cfg(not(mobile))]
use tauri::{webview::PageLoadEvent, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_shell::ShellExt;

mod agent_harness;
mod agent_ledger;
mod news_rag;
mod rag_pack;
mod research;
#[cfg(target_os = "windows")]
mod research_embeddings;
mod research_import;
mod rig_runtime;
mod runtime;

const MOBILE_MARKET_DATA_FILE: &str = "mobile-market-data.json";
const WATCHLIST_DB_FILE: &str = "watchlist.sqlite";
const ADAPTIVE_SCREEN_DB_FILE: &str = "adaptive-screen.sqlite";
const MOBILE_MARKET_PATCH_DIR: &str = "mobile-market-data-patches";
const MOBILE_MARKET_WRITE_RETRY_ATTEMPTS: usize = 3;
const MOBILE_MARKET_WRITE_RETRY_DELAY_MS: u64 = 50;
const TENCENT_QUOTE_ENDPOINT: &str = "https://qt.gtimg.cn/q=";
const EASTMONEY_KLINE_ENDPOINT: &str = "https://push2his.eastmoney.com/api/qt/stock/kline/get";
const EASTMONEY_FUND_FLOW_ENDPOINT: &str =
    "https://push2his.eastmoney.com/api/qt/stock/fflow/daykline/get";
const EASTMONEY_DATACENTER_ENDPOINT: &str = "https://datacenter-web.eastmoney.com/api/data/v1/get";
const EASTMONEY_SECURITIES_ENDPOINT: &str =
    "https://datacenter.eastmoney.com/securities/api/data/v1/get";
const TENCENT_DAILY_KLINE_ENDPOINT: &str = "https://web.ifzq.gtimg.cn/appstock/app/fqkline/get";
const TENCENT_BATCH_SIZE: usize = 120;
const TENCENT_FETCH_CONCURRENCY: usize = 12;
const TENCENT_DEFAULT_MAX_CANDIDATES: usize = 8_000;
const TENCENT_DEFAULT_MAX_FAILED_BATCHES: usize = 4;
const TENCENT_CONNECT_TIMEOUT_SECS: u64 = 3;
const TENCENT_REQUEST_TIMEOUT_SECS: u64 = 6;
const TENCENT_BATCH_TIMEOUT_SECS: u64 = 8;
const TENCENT_NETWORK_PROBE_TIMEOUT_SECS: u64 = 4;
const OBSERVE_TOTAL_TIMEOUT_SECS: u64 = 25;
const OBSERVE_MOBILE_FAST_TOTAL_TIMEOUT_SECS: u64 = 35;
const OBSERVE_FINANCIAL_TOTAL_TIMEOUT_SECS: u64 = 10;
const OBSERVE_FUNDAMENTAL_TOTAL_TIMEOUT_SECS: u64 = 8;
const OBSERVE_FUNDAMENTAL_PREFERRED_TIMEOUT_SECS: u64 = 3;
const OBSERVE_FUNDAMENTAL_REFRESH_INTERVAL_MS: u128 = 24 * 60 * 60 * 1_000;
const OBSERVE_HISTORY_TOTAL_TIMEOUT_SECS: u64 = 12;
const OBSERVE_HISTORY_TIMEOUT_SECS: u64 = 8;
const TREND_SCREEN_HISTORY_TIMEOUT_SECS: u64 = 18;
const TREND_SCREEN_HISTORY_CONCURRENCY: usize = 6;
const TREND_SCREEN_HISTORY_PREFETCH_LIMIT: usize = 80;
const MIN_TREND_SCREEN_HISTORY_BARS: usize = 45;
const ADAPTIVE_SCREEN_TOTAL_TIMEOUT_SECS: u64 = 120;
const ADAPTIVE_SCREEN_HISTORY_CONCURRENCY: usize = 6;
const ADAPTIVE_SCREEN_HISTORY_PREFETCH_TIMEOUT_SECS: u64 = 90;
const ADAPTIVE_SCREEN_HISTORY_PREFETCH_LIMIT: usize = 80;
const MIN_ADAPTIVE_SCREEN_HISTORY_BARS: usize = 60;
const ADAPTIVE_RELEASE_MIN_OOS_FOLDS: usize = 60;
const BACKTEST_HISTORY_TIMEOUT_SECS: u64 = 30;
const ADAPTIVE_RELEASE_BACKTEST_HISTORY_TIMEOUT_SECS: u64 = 180;
const BACKTEST_HISTORY_CONCURRENCY: usize = 6;
const MIN_BACKTEST_HISTORY_BARS: usize = 2;
const OBSERVE_CAPITAL_TOTAL_TIMEOUT_SECS: u64 = 10;
const OBSERVE_CAPITAL_REQUEST_TIMEOUT_SECS: u64 = 6;
const OBSERVE_LHB_SEAT_REQUEST_TIMEOUT_SECS: u64 = 3;
const OBSERVE_GUBA_MAX_POSTS: usize = 10;
const MIN_OBSERVE_HISTORY_BARS: usize = 3;
const MIN_FULL_OBSERVE_HISTORY_BARS: usize = 750;
// Tencent fqkline rejects very large count values with "param error".
const OBSERVE_DAILY_HISTORY_LIMIT: usize = 2_000;
const FINANCIAL_REQUEST_TIMEOUT_SECS: u64 = 6;
const MAX_TENCENT_WEBVIEW_QUOTE_BYTES: usize = 1_048_576;
const COMPLETE_QUARTERLY_EPS_POINTS: usize = 8;
const MAX_CACHED_HTTP_CLIENTS: usize = 16;
const LLM_MODEL_LIST_MAX_BYTES: usize = 2 * 1024 * 1024;
const THS_FINANCIAL_ENDPOINT: &str =
    "https://basic.10jqka.com.cn/basicapi/finance/index/v1/app_data/";
const SINA_FINANCIAL_GUIDELINE_ENDPOINT: &str =
    "https://money.finance.sina.com.cn/corp/go.php/vFD_FinancialGuideLine";
const SCREEN_STOCK_FINANCIAL_FIELDS: [&str; 5] = [
    "deducted_net_profit_billion",
    "deducted_net_profit_margin",
    "deducted_net_profit_growth_rate",
    "latest_eps",
    "latest_bps",
];

static REFRESH_SEED_CACHE: OnceLock<Mutex<HashMap<PathBuf, Value>>> = OnceLock::new();
static REFRESH_FINANCIAL_SNAPSHOT_CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<Value>>>> =
    OnceLock::new();
static MOBILE_MARKET_DATA_CACHE: OnceLock<Mutex<HashMap<PathBuf, MobileMarketDataCacheEntry>>> =
    OnceLock::new();
static SCREEN_STOCK_OVERLAY_CACHE: OnceLock<Mutex<HashMap<PathBuf, ScreenStockOverlayCacheEntry>>> =
    OnceLock::new();
static MOBILE_MARKET_UPDATE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static HTTP_CLIENT_CACHE: OnceLock<Mutex<HashMap<HttpClientCacheKey, reqwest::Client>>> =
    OnceLock::new();
#[cfg(target_os = "windows")]
static RESEARCH_EMBEDDING_JOB_GATE: ResearchEmbeddingJobGate = ResearchEmbeddingJobGate::new();

#[cfg(target_os = "windows")]
struct ResearchEmbeddingJobGate {
    running: AtomicBool,
    pending: AtomicBool,
}

#[cfg(target_os = "windows")]
impl ResearchEmbeddingJobGate {
    const fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            pending: AtomicBool::new(false),
        }
    }

    fn request(&self) -> bool {
        self.pending.store(true, AtomicOrdering::Release);
        self.running
            .compare_exchange(false, true, AtomicOrdering::AcqRel, AtomicOrdering::Acquire)
            .is_ok()
    }

    fn begin_cycle(&self) {
        self.pending.store(false, AtomicOrdering::Release);
    }

    fn finish_cycle(&self) -> bool {
        if self.pending.load(AtomicOrdering::Acquire) {
            return true;
        }
        self.running.store(false, AtomicOrdering::Release);
        if !self.pending.swap(false, AtomicOrdering::AcqRel) {
            return false;
        }
        self.running
            .compare_exchange(false, true, AtomicOrdering::AcqRel, AtomicOrdering::Acquire)
            .is_ok()
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct HttpClientCacheKey {
    user_agent: String,
    timeout_ms: u128,
    proxy: Option<String>,
}

#[derive(Clone)]
struct MobileMarketDataCacheEntry {
    bytes: u64,
    modified_at_epoch_ms: Option<u128>,
    data: Arc<Value>,
    typed: Arc<gp_core::CoreDataSet>,
    summary: Value,
}

#[derive(Clone)]
struct ScreenStockOverlayCacheEntry {
    data: Arc<gp_core::CoreDataSet>,
    financial_snapshot: Arc<Value>,
    stocks: Arc<Vec<gp_core::StockItem>>,
}

struct PreparedTrendScreen {
    data: Arc<gp_core::CoreDataSet>,
    stock_override: Option<Arc<Vec<gp_core::StockItem>>>,
    history_override: HashMap<String, Vec<gp_core::HistoryBar>>,
    request: gp_core::TrendScreenRequest,
    notes: Vec<String>,
}

struct PreparedAdaptiveScreen {
    data: Arc<gp_core::CoreDataSet>,
    stock_override: Option<Arc<Vec<gp_core::StockItem>>>,
    candidate_codes: Vec<String>,
    history_override: HashMap<String, Vec<gp_core::HistoryBar>>,
    request: gp_core::AdaptiveScreenRequest,
    recent_exposure: Vec<gp_core::AdaptiveRecentExposure>,
    notes: Vec<String>,
    cache_hit: bool,
}

struct AdaptiveHistoryFetchOutcome {
    results: Vec<(String, Result<Vec<Value>, String>)>,
    timed_out: bool,
}

struct PreparedBacktest {
    data: Arc<gp_core::CoreDataSet>,
    stock_override: Option<Arc<Vec<gp_core::StockItem>>>,
    history_override: HashMap<String, Vec<gp_core::HistoryBar>>,
    request: gp_core::BacktestRequest,
    notes: Vec<String>,
}

#[tauri::command]
async fn api_observe(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    Box::pin(api_observe_inner(app, payload)).await
}

async fn api_observe_inner(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let fallback_payload = payload.clone();
    let mobile_fast_observe = payload
        .get("mobile_fast_observe")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let observe_timeout_secs = if mobile_fast_observe {
        OBSERVE_MOBILE_FAST_TOTAL_TIMEOUT_SECS
    } else {
        OBSERVE_TOTAL_TIMEOUT_SECS
    };
    let observe_network = Box::pin(runtime::with_heavy_network_permit(
        "api_observe_network",
        observe_core_payload_with_cached_history(&app, payload),
    ));
    let observe_payload =
        tokio::time::timeout(Duration::from_secs(observe_timeout_secs), observe_network).await;

    match observe_payload {
        Ok(Ok((core_payload, notes))) => match run_observe_calculation(&core_payload).await {
            Ok(mut result) => {
                enrich_observe_stock_quote_fields(&mut result, &core_payload);
                for note in notes {
                    append_observe_note(&mut result, note);
                }
                Ok(result)
            }
            Err(error) => Ok(observe_error_result(
                &core_payload,
                &fallback_payload,
                vec![format!("观察计算失败：{error}")],
            )),
        },
        Ok(Err(error)) => Ok(observe_error_result(
            &Value::Null,
            &fallback_payload,
            vec![format!("观察数据准备失败：{error}")],
        )),
        Err(_) => match observe_core_payload_from_cache(&app, fallback_payload.clone()) {
            Ok(core_payload) => match run_observe_calculation(&core_payload).await {
                Ok(mut result) => {
                    enrich_observe_stock_quote_fields(&mut result, &core_payload);
                    append_observe_note(
                        &mut result,
                        format!("观察在线补全超过 {observe_timeout_secs} 秒，已返回本地缓存结果。"),
                    );
                    Ok(result)
                }
                Err(error) => Ok(observe_error_result(
                    &core_payload,
                    &fallback_payload,
                    vec![
                        format!("观察在线补全超过 {observe_timeout_secs} 秒，已返回本地缓存结果。"),
                        format!("观察计算失败：{error}"),
                    ],
                )),
            },
            Err(error) => Ok(observe_error_result(
                &Value::Null,
                &fallback_payload,
                vec![
                    format!("观察在线补全超过 {observe_timeout_secs} 秒，且无法读取本地缓存。"),
                    error,
                ],
            )),
        },
    }
}

async fn run_observe_calculation(core_payload: &Value) -> Result<Value, String> {
    let payload = core_payload.clone();
    runtime::run_cpu_bound("api_observe_calculation", move || {
        gp_core::observe_with_data_value(payload).map_err(|error| error.to_string())
    })
    .await?
}

fn enrich_observe_stock_quote_fields(result: &mut Value, core_payload: &Value) {
    const QUOTE_FIELDS: [&str; 4] = [
        "market_cap_billion",
        "circulating_market_cap_billion",
        "total_shares",
        "circulating_shares",
    ];

    let result_code = result
        .get("stock")
        .and_then(|stock| stock.get("code"))
        .and_then(Value::as_str)
        .and_then(normalize_stock_code);
    let Some(result_code) = result_code else {
        return;
    };
    let source_stock = core_payload
        .get("data")
        .and_then(|data| data.get("stocks"))
        .and_then(Value::as_array)
        .and_then(|stocks| {
            stocks.iter().find(|stock| {
                stock
                    .get("code")
                    .and_then(Value::as_str)
                    .and_then(normalize_stock_code)
                    .is_some_and(|code| code == result_code)
            })
        });
    let (Some(source_stock), Some(target_stock)) = (
        source_stock.and_then(Value::as_object),
        result.get_mut("stock").and_then(Value::as_object_mut),
    ) else {
        return;
    };

    for field in QUOTE_FIELDS {
        if let Some(value) = source_stock.get(field).filter(|value| {
            value
                .as_f64()
                .is_some_and(|number| number.is_finite() && number > 0.0)
        }) {
            target_stock.insert(field.to_string(), value.clone());
        }
    }
}

fn observe_needs_exact_share_refresh(data: &Value, code: &str) -> bool {
    stock_object(data, code).is_none_or(|stock| {
        ["total_shares", "circulating_shares"]
            .iter()
            .any(|field| object_f64(stock, field).is_none_or(|value| value <= 0.0))
    })
}

fn observe_needs_fundamental_supplement(data: &Value, code: &str) -> bool {
    let entry = data
        .get("financials")
        .and_then(Value::as_object)
        .and_then(|financials| financials.get(code))
        .and_then(Value::as_object);
    let missing = [
        "goodwill_to_net_assets",
        "pledged_share_ratio",
        "dividend_yield",
        "dividend_payout_ratio",
    ]
    .iter()
    .any(|field| {
        entry
            .and_then(|item| finite_object_number(item, field))
            .is_none()
    });
    if missing {
        return true;
    }
    let updated_at =
        entry.and_then(|item| cache_epoch_ms(item.get("supplement_updated_at_epoch_ms")));
    updated_at.is_none_or(|updated_at| {
        epoch_millis().saturating_sub(updated_at) > OBSERVE_FUNDAMENTAL_REFRESH_INTERVAL_MS
    })
}

async fn fetch_observe_quote_snapshot(
    code: &str,
    seed_stock: Option<serde_json::Map<String, Value>>,
    payload: &Value,
) -> Result<serde_json::Map<String, Value>, String> {
    let client = build_http_client_with_proxy(
        "Mozilla/5.0 GuXuanYou/0.3 observe-quote",
        Duration::from_secs(TENCENT_REQUEST_TIMEOUT_SECS),
        Some(payload),
    )?;
    let quote = fetch_tencent_quotes(
        &client,
        &[code.to_string()],
        Duration::from_secs(TENCENT_REQUEST_TIMEOUT_SECS),
    )
    .await?;
    let seed = HashMap::from([(code.to_string(), seed_stock.unwrap_or_default())]);
    parse_tencent_quotes(&quote.text, &seed, false)
        .into_iter()
        .find(|stock| {
            let code_matches = stock
                .get("code")
                .and_then(Value::as_str)
                .and_then(normalize_stock_code)
                .is_some_and(|parsed| parsed == code);
            let has_exact_shares = ["total_shares", "circulating_shares"]
                .iter()
                .all(|field| object_f64(stock, field).is_some_and(|value| value > 0.0));
            code_matches && has_exact_shares
        })
        .ok_or_else(|| format!("Tencent quote did not return exact share data for {code}"))
}

async fn fetch_eastmoney_public_json(
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

async fn fetch_eastmoney_public_json_with_direct_retry(
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

fn normalize_eastmoney_public_json(
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

fn eastmoney_result_rows(value: &Value) -> &[Value] {
    value
        .get("result")
        .and_then(|result| result.get("data"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn parse_goodwill_to_net_assets(value: &Value) -> Option<f64> {
    let row = eastmoney_result_rows(value).first()?.as_object()?;
    let parent_equity = json_f64(row.get("TOTAL_PARENT_EQUITY")).filter(|value| *value > 0.0)?;
    let goodwill = json_f64(row.get("GOODWILL")).unwrap_or(0.0).max(0.0);
    Some(goodwill / parent_equity * 100.0)
}

fn parse_latest_pledged_share_ratio(value: &Value) -> Option<f64> {
    let rows = eastmoney_result_rows(value);
    if rows.is_empty() {
        return Some(0.0);
    }
    rows.first()
        .and_then(Value::as_object)
        .and_then(|row| json_f64(row.get("PLEDGE_RATIO")))
}

fn parse_latest_dividend_metrics(value: &Value, price: Option<f64>) -> (Option<f64>, Option<f64>) {
    let rows = eastmoney_result_rows(value);
    if rows.is_empty() {
        return (Some(0.0), Some(0.0));
    }
    let row = latest_dividend_row(value);
    let Some(row) = row else {
        return (None, None);
    };
    let cash_per_share = json_f64(row.get("PRETAX_BONUS_RMB"))
        .unwrap_or(0.0)
        .max(0.0)
        / 10.0;
    let dividend_yield = price
        .filter(|value| *value > 0.0)
        .map(|value| cash_per_share / value * 100.0)
        .or_else(|| json_f64(row.get("DIVIDENT_RATIO")).map(|value| value * 100.0));
    let payout_ratio = json_f64(row.get("BASIC_EPS"))
        .filter(|value| *value > 0.0)
        .map(|eps| cash_per_share / eps * 100.0);
    (dividend_yield, payout_ratio)
}

fn latest_dividend_row(value: &Value) -> Option<&serde_json::Map<String, Value>> {
    let rows = eastmoney_result_rows(value);
    rows.iter()
        .filter_map(Value::as_object)
        .find(|row| {
            row.get("EX_DIVIDEND_DATE")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
        })
        .or_else(|| rows.first().and_then(Value::as_object))
}

fn eastmoney_metric_period(
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

async fn fetch_observe_fundamental_supplement(
    code: &str,
    price: Option<f64>,
    payload: &Value,
) -> Result<(serde_json::Map<String, Value>, Vec<String>), String> {
    let digits = code
        .get(..6)
        .filter(|value| value.chars().all(|ch| ch.is_ascii_digit()))
        .ok_or_else(|| format!("invalid stock code for fundamentals: {code}"))?;
    let client = build_http_client_with_proxy(
        "Mozilla/5.0 GuXuanYou/0.3 observe-fundamentals",
        Duration::from_secs(OBSERVE_FUNDAMENTAL_TOTAL_TIMEOUT_SECS),
        Some(payload),
    )?;
    let direct_client = build_direct_http_client(
        "Mozilla/5.0 GuXuanYou/0.3 observe-fundamentals",
        Duration::from_secs(OBSERVE_FUNDAMENTAL_TOTAL_TIMEOUT_SECS),
    )?;
    let balance_url = format!(
        "{EASTMONEY_SECURITIES_ENDPOINT}?reportName=RPT_F10_FINANCE_GBALANCE&columns=SECUCODE,REPORT_DATE,REPORT_DATE_NAME,GOODWILL,TOTAL_PARENT_EQUITY&filter=(SECUCODE%3D%22{code}%22)&pageNumber=1&pageSize=1&sortTypes=-1&sortColumns=REPORT_DATE&source=HSF10&client=PC"
    );
    let pledge_url = format!(
        "{EASTMONEY_DATACENTER_ENDPOINT}?reportName=RPTA_APP_PLEDGERATIO&columns=SECURITY_CODE,TRADE_DATE,PLEDGE_RATIO&sortColumns=TRADE_DATE&sortTypes=-1&filter=(SECURITY_CODE%3D%22{digits}%22)&pageNumber=1&pageSize=1&source=DataCenter&client=APP"
    );
    let dividend_url = format!(
        "{EASTMONEY_DATACENTER_ENDPOINT}?reportName=RPT_SHAREBONUS_DET&columns=SECUCODE,SECURITY_CODE,REPORT_DATE,PRETAX_BONUS_RMB,BASIC_EPS,DIVIDENT_RATIO,EX_DIVIDEND_DATE&sortColumns=REPORT_DATE&sortTypes=-1&filter=(SECURITY_CODE%3D%22{digits}%22)&pageNumber=1&pageSize=5&source=WEB&client=WEB"
    );
    let (balance, pledge, dividend) = futures::join!(
        fetch_eastmoney_public_json_with_direct_retry(
            &client,
            &direct_client,
            &balance_url,
            "Eastmoney balance sheet",
            false,
        ),
        fetch_eastmoney_public_json_with_direct_retry(
            &client,
            &direct_client,
            &pledge_url,
            "Eastmoney pledge ratio",
            true,
        ),
        fetch_eastmoney_public_json_with_direct_retry(
            &client,
            &direct_client,
            &dividend_url,
            "Eastmoney dividend history",
            true,
        ),
    );

    let mut fields = serde_json::Map::new();
    let mut notes = Vec::new();
    match balance {
        Ok(value) => {
            if let Some(ratio) = parse_goodwill_to_net_assets(&value) {
                fields.insert("goodwill_to_net_assets".to_string(), json!(ratio));
            }
            if let Some(period) = eastmoney_metric_period(
                eastmoney_result_rows(&value)
                    .first()
                    .and_then(Value::as_object),
                &["REPORT_DATE_NAME", "REPORT_DATE"],
            ) {
                fields.insert("goodwill_period".to_string(), json!(period));
            }
        }
        Err(error) => notes.push(error),
    }
    match pledge {
        Ok(value) => {
            if let Some(ratio) = parse_latest_pledged_share_ratio(&value) {
                fields.insert("pledged_share_ratio".to_string(), json!(ratio));
            }
            if let Some(period) = eastmoney_metric_period(
                eastmoney_result_rows(&value)
                    .first()
                    .and_then(Value::as_object),
                &["TRADE_DATE"],
            ) {
                fields.insert("pledged_share_period".to_string(), json!(period));
            }
        }
        Err(error) => notes.push(error),
    }
    match dividend {
        Ok(value) => {
            let (dividend_yield, payout_ratio) = parse_latest_dividend_metrics(&value, price);
            if let Some(value) = dividend_yield {
                fields.insert("dividend_yield".to_string(), json!(value));
            }
            if let Some(value) = payout_ratio {
                fields.insert("dividend_payout_ratio".to_string(), json!(value));
            }
            if let Some(period) =
                eastmoney_metric_period(latest_dividend_row(&value), &["REPORT_DATE"])
            {
                fields.insert("dividend_period".to_string(), json!(period));
            }
        }
        Err(error) => notes.push(error),
    }
    if fields.is_empty() {
        return Err(if notes.is_empty() {
            "Eastmoney fundamentals returned no usable metrics".to_string()
        } else {
            notes.join(" | ")
        });
    }
    Ok((fields, notes))
}

fn merge_observe_quote_snapshot(
    data: &mut Value,
    code: &str,
    quote: &serde_json::Map<String, Value>,
) -> bool {
    const FIELDS: [&str; 5] = [
        "market_cap_billion",
        "circulating_market_cap_billion",
        "total_shares",
        "circulating_shares",
        "quote_time",
    ];
    let Some(stock) = data
        .get_mut("stocks")
        .and_then(Value::as_array_mut)
        .and_then(|stocks| {
            stocks.iter_mut().find(|stock| {
                stock
                    .get("code")
                    .and_then(Value::as_str)
                    .and_then(normalize_stock_code)
                    .is_some_and(|stock_code| stock_code == code)
            })
        })
        .and_then(Value::as_object_mut)
    else {
        return false;
    };
    let mut changed = false;
    for field in FIELDS {
        let Some(value) = quote.get(field).filter(|value| !value.is_null()) else {
            continue;
        };
        if stock.get(field) != Some(value) {
            stock.insert(field.to_string(), value.clone());
            changed = true;
        }
    }
    changed
}

fn merge_observe_fundamental_supplement(
    data: &mut Value,
    code: &str,
    fields: serde_json::Map<String, Value>,
) -> bool {
    let entry = financial_entry_mut(data, code);
    let mut changed = false;
    for (field, value) in fields {
        let valid_number = json_f64(Some(&value)).is_some();
        let valid_period = field.ends_with("_period")
            && value.as_str().is_some_and(|value| !value.trim().is_empty());
        if (valid_number || valid_period) && entry.get(&field) != Some(&value) {
            entry.insert(field, value);
            changed = true;
        }
    }
    if !entry.is_empty() {
        let updated_at = json!(epoch_millis().to_string());
        if entry.get("supplement_updated_at_epoch_ms") != Some(&updated_at) {
            entry.insert("supplement_updated_at_epoch_ms".to_string(), updated_at);
            changed = true;
        }
        append_financial_source(entry, "东方财富财报/质押/分红公开数据");
    }
    changed
}

#[tauri::command]
async fn core_screen(payload: Value) -> Result<Value, String> {
    runtime::run_cpu_bound("core_screen", move || {
        gp_core::screen_value(payload).map_err(|error| error.to_string())
    })
    .await?
}

#[tauri::command]
async fn core_screen_with_data(payload: Value) -> Result<Value, String> {
    runtime::run_cpu_bound("core_screen_with_data", move || {
        gp_core::screen_with_data_value(payload).map_err(|error| error.to_string())
    })
    .await?
}

#[tauri::command]
async fn core_graph_screen(payload: Value) -> Result<Value, String> {
    runtime::run_cpu_bound("core_graph_screen", move || {
        gp_core::graph_screen_value(payload).map_err(|error| error.to_string())
    })
    .await?
}

#[tauri::command]
async fn core_graph_screen_with_data(payload: Value) -> Result<Value, String> {
    runtime::run_cpu_bound("core_graph_screen_with_data", move || {
        gp_core::graph_screen_with_data_value(payload).map_err(|error| error.to_string())
    })
    .await?
}

#[tauri::command]
async fn core_backtest(payload: Value) -> Result<Value, String> {
    runtime::run_cpu_bound("core_backtest", move || {
        gp_core::backtest_value(payload).map_err(|error| error.to_string())
    })
    .await?
}

#[tauri::command]
async fn core_backtest_with_data(payload: Value) -> Result<Value, String> {
    runtime::run_cpu_bound("core_backtest_with_data", move || {
        gp_core::backtest_with_data_value(payload).map_err(|error| error.to_string())
    })
    .await?
}

#[tauri::command]
async fn core_trend(payload: Value) -> Result<Value, String> {
    runtime::run_cpu_bound("core_trend", move || {
        gp_core::trend_value(payload).map_err(|error| error.to_string())
    })
    .await?
}

#[tauri::command]
async fn core_trend_with_data(payload: Value) -> Result<Value, String> {
    runtime::run_cpu_bound("core_trend_with_data", move || {
        gp_core::trend_with_data_value(payload).map_err(|error| error.to_string())
    })
    .await?
}

#[tauri::command]
async fn core_trend_screen(payload: Value) -> Result<Value, String> {
    runtime::run_cpu_bound("core_trend_screen", move || {
        gp_core::trend_screen_value(payload).map_err(|error| error.to_string())
    })
    .await?
}

#[tauri::command]
async fn core_trend_screen_with_data(payload: Value) -> Result<Value, String> {
    runtime::run_cpu_bound("core_trend_screen_with_data", move || {
        gp_core::trend_screen_with_data_value(payload).map_err(|error| error.to_string())
    })
    .await?
}

#[tauri::command]
async fn core_agent(payload: Value) -> Result<Value, String> {
    runtime::run_cpu_bound("core_agent", move || {
        gp_core::agent_value(payload).map_err(|error| error.to_string())
    })
    .await?
}

#[tauri::command]
async fn core_agent_with_data(payload: Value) -> Result<Value, String> {
    runtime::run_cpu_bound("core_agent_with_data", move || {
        gp_core::agent_with_data_value(payload).map_err(|error| error.to_string())
    })
    .await?
}

#[tauri::command]
async fn core_mobile_stock_skill(payload: Value) -> Result<Value, String> {
    runtime::run_cpu_bound("core_mobile_stock_skill", move || {
        gp_core::mobile_stock_skill_value(payload).map_err(|error| error.to_string())
    })
    .await?
}

#[tauri::command]
fn api_market_status(app: tauri::AppHandle) -> Result<Value, String> {
    market_data_status(&app)
}

#[tauri::command]
async fn api_market_refresh(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    core_mobile_market_data_refresh_tencent(app, payload).await
}

#[tauri::command]
fn api_market_ingest_tencent_quotes(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    core_mobile_market_data_ingest_tencent_quotes(app, payload)
}

#[tauri::command]
fn api_market_clear_cache(app: tauri::AppHandle) -> Result<Value, String> {
    let cleared = core_mobile_market_data_clear(app.clone())?;
    Ok(json!({
        "removed_files": if cleared.get("removed").and_then(Value::as_bool).unwrap_or(false) { 1 } else { 0 },
        "removed_bytes": cleared.get("removed_bytes").and_then(Value::as_u64).unwrap_or(0),
        "status": market_data_status(&app)?,
        "notes": cleared.get("notes").cloned().unwrap_or_else(|| json!([]))
    }))
}

#[tauri::command]
async fn api_screen(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    if legacy_screen_requested(&payload) {
        return api_legacy_screen(app, payload).await;
    }
    adaptive_screen_with_timeout(
        Duration::from_secs(ADAPTIVE_SCREEN_TOTAL_TIMEOUT_SECS),
        api_adaptive_screen(app, payload),
    )
    .await
}

fn legacy_screen_requested(payload: &Value) -> bool {
    payload
        .get("internal_algorithm")
        .and_then(Value::as_str)
        .is_some_and(|algorithm| algorithm.eq_ignore_ascii_case("legacy_balanced"))
}

async fn adaptive_screen_with_timeout<T>(
    duration: Duration,
    future: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    tokio::time::timeout(duration, future).await.map_err(|_| {
        format!(
            "智能选股端到端计算超过 {ADAPTIVE_SCREEN_TOTAL_TIMEOUT_SECS} 秒，请检查网络或刷新行情后重试"
        )
    })?
}

async fn api_adaptive_screen(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let started_at = Instant::now();
    let prepared = prepare_adaptive_screen(&app, payload).await?;
    let PreparedAdaptiveScreen {
        data,
        stock_override,
        candidate_codes,
        history_override,
        request,
        recent_exposure,
        notes,
        cache_hit,
    } = prepared;
    emit_adaptive_screen_progress(
        &app,
        request.run_id.as_deref(),
        "regime",
        82,
        "判断市场状态",
    );
    let calculation_request = request.clone();
    let mut result = runtime::run_cpu_bound("api_screen", move || {
        let calculation_as_of = calculation_request.as_of_date.as_deref();
        let universe = stock_override
            .as_deref()
            .map(Vec::as_slice)
            .unwrap_or(data.stocks.as_slice());
        let mut histories = candidate_codes
            .iter()
            .map(String::as_str)
            .chain(adaptive_benchmark_codes())
            .filter_map(|code| {
                data.histories.get(code).map(|rows| {
                    (
                        code.to_string(),
                        adaptive_history_window(rows, calculation_as_of),
                    )
                })
            })
            .collect::<HashMap<_, _>>();
        histories.extend(
            history_override
                .into_iter()
                .map(|(code, rows)| (code, adaptive_history_window(&rows, calculation_as_of))),
        );
        let benchmarks = adaptive_benchmark_codes()
            .into_iter()
            .filter_map(|code| {
                histories
                    .get(code)
                    .cloned()
                    .map(|rows| (code.to_string(), rows))
            })
            .collect::<HashMap<_, _>>();
        let point_in_time_universe =
            adaptive_point_in_time_universe(universe, &histories, calculation_as_of);
        let result = gp_core::adaptive_screen_stocks(
            &point_in_time_universe,
            &histories,
            &benchmarks,
            &recent_exposure,
            &calculation_request,
        )
        .map_err(|error| error.to_string())?;
        serde_json::to_value(result).map_err(|error| error.to_string())
    })
    .await??;
    append_result_notes(&mut result, notes);
    emit_adaptive_screen_progress(
        &app,
        request.run_id.as_deref(),
        "ranking",
        94,
        "生成主榜与探索榜",
    );
    let exposure_app = app.clone();
    let exposure_result = result.clone();
    let exposure_date = result
        .pointer("/market_regime/as_of_date")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            local_yyyymmdd_from_epoch_ms(epoch_millis()).unwrap_or_else(|| "19700101".to_string())
        });
    let exposure_write_date = exposure_date.clone();
    let exposure_write = runtime::run_io_bound("adaptive_screen_exposure_write", move || {
        adaptive_exposure_record_sync(&exposure_app, &exposure_result, &exposure_write_date)
    })
    .await?;
    if let Err(error) = exposure_write {
        append_result_notes(&mut result, vec![format!("近期曝光记录写入失败：{error}")]);
    }
    emit_adaptive_screen_progress(&app, request.run_id.as_deref(), "complete", 100, "选股完成");
    let run_app = app.clone();
    let run_result = result.clone();
    let run_id = request.run_id.clone();
    let release_evidence_qualified = adaptive_release_screen_request_qualified(&request);
    let elapsed_millis = started_at.elapsed().as_millis().min(u64::MAX as u128) as u64;
    tauri::async_runtime::spawn(async move {
        let _ = runtime::run_io_bound("adaptive_screen_run_record", move || {
            adaptive_release_run_record_sync(
                &run_app,
                run_id.as_deref(),
                &run_result,
                &exposure_date,
                elapsed_millis,
                cache_hit,
                release_evidence_qualified,
            )
        })
        .await;
    });
    Ok(result)
}

async fn api_legacy_screen(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let data = cached_market_data_snapshot(&app)?;
    let stock_override = screen_stock_override(&app, &data, &payload)?;
    let criteria = legacy_screen_criteria_from_payload(payload)?;
    let mut result = runtime::run_cpu_bound("api_legacy_screen", move || {
        let result = match stock_override.as_deref() {
            Some(stocks) => gp_core::screen_stocks(stocks, &criteria),
            None => gp_core::screen_with_data(data.as_ref(), &criteria)
                .map_err(|error| error.to_string())?,
        };
        serde_json::to_value(result).map_err(|error| error.to_string())
    })
    .await??;
    if let Some(object) = result.as_object_mut() {
        object.insert(
            "algorithm_version".to_string(),
            Value::String("legacy_balanced".to_string()),
        );
        object.insert(
            "rollout".to_string(),
            json!({
                "adaptive_available": true,
                "adaptive_default_enabled": true,
                "reason": "legacy_balanced was requested explicitly for compatibility"
            }),
        );
    }
    append_result_notes(
        &mut result,
        vec![
            "本次选股按显式兼容请求使用 legacy_balanced；未指定算法时默认使用 adaptive_swing_v1。"
                .to_string(),
        ],
    );
    Ok(result)
}

fn legacy_screen_criteria_from_payload(
    mut payload: Value,
) -> Result<gp_core::ScreenCriteria, String> {
    let nested = payload.get("criteria").is_some();
    let legacy_limit = if nested {
        payload.get("primary_limit")
    } else {
        payload.get("limit")
    }
    .and_then(Value::as_u64)
    .and_then(|value| usize::try_from(value).ok())
    .unwrap_or(10)
    .clamp(1, 50);
    if let Some(criteria) = payload.get("criteria").cloned() {
        payload = criteria;
    }
    if let Some(object) = payload.as_object_mut() {
        object.remove("internal_algorithm");
        object.insert("limit".to_string(), json!(legacy_limit));
    }
    serde_json::from_value::<gp_core::ScreenCriteria>(strip_core_side_payload_fields(payload))
        .map_err(|error| format!("invalid legacy screen request: {error}"))
}

fn adaptive_screen_request_from_payload(
    payload: Value,
) -> Result<gp_core::AdaptiveScreenRequest, String> {
    let payload = strip_core_side_payload_fields(payload);
    if payload.get("criteria").is_some() {
        serde_json::from_value(payload)
            .map_err(|error| format!("invalid adaptive screen request: {error}"))
    } else {
        let criteria = serde_json::from_value::<gp_core::ScreenCriteria>(payload)
            .map_err(|error| format!("invalid legacy screen request: {error}"))?;
        Ok(gp_core::AdaptiveScreenRequest {
            criteria,
            ..gp_core::AdaptiveScreenRequest::default()
        })
    }
}

fn adaptive_benchmark_codes() -> [&'static str; 3] {
    ["000001.SH", "399001.SZ", "399006.SZ"]
}

fn adaptive_required_history_codes(candidates: &[String]) -> Vec<String> {
    let mut required_codes = candidates.to_vec();
    required_codes.extend(adaptive_benchmark_codes().into_iter().map(str::to_string));
    dedupe_stock_codes(&mut required_codes);
    required_codes
}

fn adaptive_missing_history_codes(
    data: &gp_core::CoreDataSet,
    required_codes: &[String],
    target_history_date: Option<&str>,
) -> Vec<String> {
    required_codes
        .iter()
        .filter(|code| {
            !adaptive_history_cache_is_usable(
                data,
                code,
                target_history_date,
                MIN_ADAPTIVE_SCREEN_HISTORY_BARS,
            )
        })
        .cloned()
        .collect()
}

fn emit_adaptive_screen_progress(
    app: &tauri::AppHandle,
    run_id: Option<&str>,
    stage: &str,
    percent: usize,
    message: &str,
) {
    let _ = app.emit(
        "adaptive-screen-progress",
        adaptive_screen_progress_payload(run_id, stage, percent, message),
    );
}

fn adaptive_screen_progress_payload(
    run_id: Option<&str>,
    stage: &str,
    percent: usize,
    message: &str,
) -> Value {
    json!({
        "run_id": run_id,
        "stage": stage,
        "percent": percent,
        "message": message,
    })
}

#[tauri::command]
async fn api_sector_screen(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let data = cached_market_data_snapshot(&app)?;
    let stock_override = screen_stock_override(&app, &data, &payload)?;
    let request = serde_json::from_value::<gp_core::SectorScreenRequest>(
        strip_core_side_payload_fields(payload),
    )
    .map_err(|error| format!("invalid sector screen request: {error}"))?;
    runtime::run_cpu_bound("api_sector_screen", move || {
        let result = match stock_override.as_deref() {
            Some(stocks) => gp_core::sector_screen_stocks(stocks, &request),
            None => gp_core::sector_screen_with_data(data.as_ref(), &request)
                .map_err(|error| error.to_string())?,
        };
        serde_json::to_value(result).map_err(|error| error.to_string())
    })
    .await?
}

#[tauri::command]
async fn api_custom_screen(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    run_graph_screen_command("api_custom_screen", app, payload).await
}

#[tauri::command]
async fn api_graph_screen(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    run_graph_screen_command("api_graph_screen", app, payload).await
}

#[tauri::command]
async fn api_trend_analyze(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let data = cached_market_data_snapshot(&app)?;
    let stock_override = screen_stock_override(&app, &data, &payload)?;
    let request = serde_json::from_value::<gp_core::TrendIndicatorRequest>(
        strip_core_side_payload_fields(payload),
    )
    .map_err(|error| format!("invalid trend request: {error}"))?;
    runtime::run_cpu_bound("api_trend_analyze", move || {
        let source = match stock_override.as_deref() {
            Some(stocks) => gp_core::StaticDataSource::with_stocks(data.as_ref(), stocks),
            None => gp_core::StaticDataSource::new(data.as_ref()),
        };
        let result =
            gp_core::trend_with_source(&source, &request).map_err(|error| error.to_string())?;
        serde_json::to_value(result).map_err(|error| error.to_string())
    })
    .await?
}

#[tauri::command]
async fn api_trend_screen(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    api_trend_screen_inner(app, payload).await
}

async fn api_trend_screen_inner(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let prepared = tokio::time::timeout(
        Duration::from_secs(TREND_SCREEN_HISTORY_TIMEOUT_SECS),
        prepare_trend_screen(&app, payload),
    )
    .await
    .map_err(|_| format!("trend screen history prefetch exceeded {TREND_SCREEN_HISTORY_TIMEOUT_SECS}s; retry after refreshing market data."))??;
    let PreparedTrendScreen {
        data,
        stock_override,
        history_override,
        request,
        notes,
    } = prepared;
    let mut result = runtime::run_cpu_bound("api_trend_screen", move || {
        let history_override = (!history_override.is_empty()).then_some(&history_override);
        let source = gp_core::StaticDataSource::with_overrides(
            data.as_ref(),
            stock_override.as_deref().map(Vec::as_slice),
            history_override,
        );
        let result = gp_core::trend_screen_with_source(&source, &request)
            .map_err(|error| error.to_string())?;
        serde_json::to_value(result).map_err(|error| error.to_string())
    })
    .await??;
    append_result_notes(&mut result, notes);
    Ok(result)
}

fn backtest_history_timeout_secs(payload: &Value) -> u64 {
    if payload
        .get(stringify!(internal_release_validation))
        .and_then(Value::as_bool)
        == Some(true)
    {
        ADAPTIVE_RELEASE_BACKTEST_HISTORY_TIMEOUT_SECS
    } else {
        BACKTEST_HISTORY_TIMEOUT_SECS
    }
}

#[tauri::command]
async fn api_backtest(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let history_timeout_secs = backtest_history_timeout_secs(&payload);
    let prepared = tokio::time::timeout(
        Duration::from_secs(history_timeout_secs),
        prepare_backtest(&app, payload),
    )
    .await
    .map_err(|_| {
        format!(
            "backtest history prefetch exceeded {history_timeout_secs}s; retry after refreshing market data."
        )
    })??;
    let PreparedBacktest {
        data,
        stock_override,
        history_override,
        request,
        notes,
    } = prepared;
    let adaptive_backtest = request
        .strategy_mode
        .trim()
        .to_ascii_lowercase()
        .starts_with("adaptive_swing_v1");
    let operational_evidence = if adaptive_backtest {
        let evidence_app = app.clone();
        runtime::run_io_bound("adaptive_release_operational_evidence", move || {
            adaptive_release_operational_evidence_sync(&evidence_app)
        })
        .await??
    } else {
        (None, None, None)
    };
    let calculation = runtime::run_cpu_bound("api_backtest", move || {
        let history_override = (!history_override.is_empty()).then_some(&history_override);
        let source = gp_core::StaticDataSource::with_overrides(
            data.as_ref(),
            stock_override.as_deref().map(Vec::as_slice),
            history_override,
        );
        let result =
            gp_core::backtest_with_source(&source, &request).map_err(|error| error.to_string())?;
        let mut value = serde_json::to_value(&result).map_err(|error| error.to_string())?;
        if adaptive_backtest {
            let mut legacy_request = request.clone();
            legacy_request.strategy_mode = "walk_forward".to_string();
            legacy_request.criteria.score_profile = "balanced".to_string();
            let legacy = gp_core::backtest_with_source(&source, &legacy_request)
                .map_err(|error| format!("legacy balanced comparison failed: {error}"))?;
            let precision_pair = (request.top_n == 10)
                .then_some((legacy.metrics.precision_at_n, result.metrics.precision_at_n));
            let release_qualification =
                adaptive_release_backtest_qualification(&request, result.metrics.oos_fold_count);
            let gate_input = gp_core::AdaptiveReleaseGateInput {
                release_configuration_qualified: release_qualification
                    .get("qualified")
                    .and_then(Value::as_bool),
                legacy_annualized_return: legacy.metrics.annualized_return,
                adaptive_annualized_return: result.metrics.annualized_return,
                legacy_max_drawdown: legacy.metrics.max_drawdown,
                adaptive_max_drawdown: result.metrics.max_drawdown,
                legacy_precision_at_10: precision_pair.and_then(|pair| pair.0),
                adaptive_precision_at_10: precision_pair.and_then(|pair| pair.1),
                max_primary_industry_count: adaptive_backtest_max_industry_count(
                    &result,
                    data.as_ref(),
                ),
                average_adjacent_jaccard: adaptive_backtest_average_jaccard(&result),
                five_run_unique_coverage: operational_evidence.0,
                first_run_millis: operational_evidence.1,
                cached_run_millis: operational_evidence.2,
            };
            let gate = gp_core::evaluate_adaptive_release_gate(&gate_input);
            if let Some(object) = value.as_object_mut() {
                object.insert(
                    "legacy_balanced_backtest".to_string(),
                    serde_json::to_value(legacy).map_err(|error| error.to_string())?,
                );
                object.insert(
                    "adaptive_release_gate".to_string(),
                    serde_json::to_value(gate).map_err(|error| error.to_string())?,
                );
                object.insert(
                    "adaptive_release_gate_input".to_string(),
                    serde_json::to_value(gate_input).map_err(|error| error.to_string())?,
                );
                object.insert(
                    "adaptive_release_qualification".to_string(),
                    release_qualification,
                );
            }
        }
        Ok(value)
    })
    .await?;
    let mut result = calculation.map_err(|error| {
        if notes.is_empty() {
            error
        } else {
            format!("{error}；数据准备：{}", notes.join(" | "))
        }
    })?;
    if adaptive_backtest {
        let gate_input = serde_json::from_value::<gp_core::AdaptiveReleaseGateInput>(
            result
                .get("adaptive_release_gate_input")
                .cloned()
                .ok_or_else(|| "adaptive release gate input is missing".to_string())?,
        )
        .map_err(|error| format!("invalid adaptive release gate input: {error}"))?;
        let gate_report = serde_json::from_value::<gp_core::AdaptiveReleaseGateReport>(
            result
                .get("adaptive_release_gate")
                .cloned()
                .ok_or_else(|| "adaptive release gate report is missing".to_string())?,
        )
        .map_err(|error| format!("invalid adaptive release gate report: {error}"))?;
        let qualification = result
            .get("adaptive_release_qualification")
            .cloned()
            .ok_or_else(|| "adaptive release qualification is missing".to_string())?;
        let gate_app = app.clone();
        let gate_write = runtime::run_io_bound("adaptive_release_gate_store", move || {
            adaptive_release_gate_store_sync(&gate_app, &gate_input, &gate_report, &qualification)
        })
        .await?;
        if let Err(error) = gate_write {
            append_result_notes(&mut result, vec![format!("发布门槛报告写入失败：{error}")]);
        }
    }
    append_result_notes(&mut result, notes);
    Ok(result)
}

fn adaptive_backtest_max_industry_count(
    result: &gp_core::BacktestResult,
    data: &gp_core::CoreDataSet,
) -> Option<usize> {
    let counts = result
        .walk_forward_folds
        .iter()
        .filter(|fold| !fold.selected_symbols.is_empty())
        .map(|fold| adaptive_backtest_fold_max_industry_count(fold, data))
        .collect::<Option<Vec<_>>>()?;
    counts.into_iter().max()
}

fn adaptive_backtest_fold_max_industry_count(
    fold: &gp_core::WalkForwardFold,
    data: &gp_core::CoreDataSet,
) -> Option<usize> {
    let as_of = compact_date_key(
        fold.signal_date
            .as_deref()
            .unwrap_or(fold.selection_date.as_str()),
    )?;
    let mut counts = HashMap::<String, usize>::new();
    for code in &fold.selected_symbols {
        let normalized = normalize_stock_code(code).unwrap_or_else(|| code.to_ascii_uppercase());
        let industry = data
            .factor_snapshots
            .get(code)
            .or_else(|| data.factor_snapshots.get(&normalized))
            .into_iter()
            .flatten()
            .filter_map(|snapshot| {
                let available = snapshot
                    .available_date
                    .as_deref()
                    .and_then(compact_date_key)?;
                let industry = snapshot
                    .industry
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())?;
                (available <= as_of).then(|| (available, industry.to_string()))
            })
            .max_by(|left, right| left.0.cmp(&right.0))
            .map(|(_, industry)| industry)?;
        *counts.entry(industry).or_default() += 1;
    }
    counts.values().copied().max()
}

fn adaptive_backtest_average_jaccard(result: &gp_core::BacktestResult) -> Option<f64> {
    let values = result
        .walk_forward_folds
        .windows(2)
        .filter_map(|pair| {
            let left = pair[0]
                .selected_symbols
                .iter()
                .map(|code| code.to_ascii_uppercase())
                .collect::<HashSet<_>>();
            let right = pair[1]
                .selected_symbols
                .iter()
                .map(|code| code.to_ascii_uppercase())
                .collect::<HashSet<_>>();
            let union = left.union(&right).count();
            (union > 0).then_some(left.intersection(&right).count() as f64 / union as f64)
        })
        .collect::<Vec<_>>();
    (!values.is_empty()).then_some(values.iter().sum::<f64>() / values.len() as f64)
}

fn adaptive_release_implementation_fingerprint() -> &'static str {
    static FINGERPRINT: OnceLock<String> = OnceLock::new();
    FINGERPRINT
        .get_or_init(|| {
            let mut hasher = Sha256::new();
            hasher.update(b"adaptive_swing_v1-release-contract-v2");
            hasher.update(include_bytes!(
                "../../../native/gp-core/src/adaptive_screen.rs"
            ));
            hasher.update(include_bytes!("../../../native/gp-core/src/lib.rs"));
            hasher.update(include_bytes!("lib.rs"));
            let digest = hasher.finalize();
            digest.iter().map(|byte| format!("{byte:02x}")).collect()
        })
        .as_str()
}

fn adaptive_release_criteria_is_full_universe(criteria: &gp_core::ScreenCriteria) -> bool {
    criteria.min_roe.is_none()
        && criteria.max_pe.is_none()
        && criteria.max_pb.is_none()
        && criteria.min_market_cap_billion.is_none()
        && criteria.min_deducted_net_profit_billion.is_none()
        && criteria.min_deducted_net_profit_margin.is_none()
        && criteria.min_deducted_net_profit_growth_rate.is_none()
        && criteria
            .industry
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        && criteria
            .market_scope
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        && !criteria.include_st
}

fn adaptive_release_screen_request_qualified(request: &gp_core::AdaptiveScreenRequest) -> bool {
    request.mode.trim().eq_ignore_ascii_case("auto")
        && request.horizon.trim().eq_ignore_ascii_case("swing_10_30d")
        && request.primary_limit == 10
        && request.exploration_limit == 10
        && adaptive_release_criteria_is_full_universe(&request.criteria)
}

fn adaptive_release_requested_mode(strategy_mode: &str) -> String {
    let normalized = strategy_mode.trim().to_ascii_lowercase();
    normalized
        .split_once(':')
        .map(|(_, mode)| mode)
        .filter(|mode| matches!(*mode, "auto" | "range" | "trend" | "defensive"))
        .unwrap_or("auto")
        .to_string()
}

fn adaptive_release_backtest_qualification(
    request: &gp_core::BacktestRequest,
    oos_fold_count: usize,
) -> Value {
    let mode = adaptive_release_requested_mode(&request.strategy_mode);
    let full_universe = request.source.trim().eq_ignore_ascii_case("criteria")
        && request.stock_codes.is_empty()
        && adaptive_release_criteria_is_full_universe(&request.criteria);
    let qualified = mode == "auto"
        && full_universe
        && request.top_n == 10
        && request
            .rebalance_frequency
            .trim()
            .eq_ignore_ascii_case("monthly")
        && (request.transaction_cost_bps - 10.0).abs() <= f64::EPSILON
        && request
            .benchmark
            .trim()
            .eq_ignore_ascii_case("candidate_equal_weight")
        && oos_fold_count >= ADAPTIVE_RELEASE_MIN_OOS_FOLDS;
    json!({
        "implementation_fingerprint": adaptive_release_implementation_fingerprint(),
        "qualified": qualified,
        "mode": mode,
        "full_universe": full_universe,
        "source": request.source.as_str(),
        "top_n": request.top_n,
        "start_date": request.start_date.as_str(),
        "end_date": request.end_date.as_str(),
        "rebalance_frequency": request.rebalance_frequency.as_str(),
        "transaction_cost_bps": request.transaction_cost_bps,
        "benchmark": request.benchmark.as_str(),
        "oos_fold_count": oos_fold_count,
    })
}

#[tauri::command]
fn api_stock_search(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
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
fn api_health() -> Result<Value, String> {
    Ok(json!({"status": "ok", "runtime": "tauri"}))
}

#[tauri::command]
fn api_strategies() -> Result<Value, String> {
    Ok(json!({
        "strategies": [
            {
                "id": "quality_value",
                "name": "质量价值",
                "description": "低估值且净资产收益率为正。"
            },
            {
                "id": "defensive_dividend",
                "name": "防御分红",
                "description": "盈利稳定、估值适中、分红较好。"
            }
        ]
    }))
}

#[tauri::command]
async fn api_news_rag(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let mut result = runtime::with_heavy_network_permit(
        "api_news_rag",
        news_rag::api_news_rag_impl(app.clone(), payload),
    )
    .await?;
    match research::ingest_news_cache(&app) {
        Ok(_) => schedule_research_embeddings(app.clone()),
        Err(error) => {
            let warning = Value::String(format!("research message ingest failed: {error}"));
            if let Some(notes) = result.get_mut("notes").and_then(Value::as_array_mut) {
                notes.push(warning);
            } else {
                result["notes"] = Value::Array(vec![warning]);
            }
        }
    }
    Ok(result)
}

#[tauri::command]
async fn api_research_overview(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    runtime::run_io_bound("api_research_overview", move || {
        research::with_app_store(&app, |store| {
            let mut overview = store.overview(&payload)?;
            #[cfg(target_os = "windows")]
            {
                let index = store.index_status()?;
                let embedding_count = index
                    .get("embedding_count")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let mut vector =
                    research_embeddings::model_status(&research_embedding_model_dir(&app));
                let model_ready = vector
                    .get("ready")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                vector["ready"] = Value::Bool(model_ready && embedding_count > 0);
                vector["embedding_count"] = json!(embedding_count);
                overview["retrieval"]["vector"] = vector;
            }
            Ok(overview)
        })
    })
    .await?
}

#[tauri::command]
async fn api_research_messages(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    runtime::run_io_bound("api_research_messages", move || {
        research::with_app_store(&app, |store| store.messages(&payload))
    })
    .await?
}

#[tauri::command]
async fn api_research_mark_read(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    runtime::run_io_bound("api_research_mark_read", move || {
        research::with_app_store(&app, |store| store.mark_read(&payload))
    })
    .await?
}

#[tauri::command]
async fn api_research_query(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let retrieval_app = app.clone();
    let retrieval_payload = payload.clone();
    let (database_generation, mut response) =
        runtime::run_cpu_bound("api_research_query", move || {
            research::with_app_store_snapshot(&retrieval_app, |store| {
                let response = {
                    #[cfg(target_os = "windows")]
                    {
                        let query_vector_result = retrieval_payload
                            .get("query")
                            .and_then(Value::as_str)
                            .map(|query| {
                                research_embeddings::embed(
                                    &research_embedding_model_dir(&retrieval_app),
                                    &[query.to_string()],
                                )
                            })
                            .transpose();
                        match query_vector_result {
                            Ok(vectors) => {
                                let query_vector =
                                    vectors.and_then(|items| items.into_iter().next());
                                store.query_with_vector(
                                    &retrieval_payload,
                                    query_vector.as_deref(),
                                )?
                            }
                            Err(error) => {
                                let mut fallback = store.query(&retrieval_payload)?;
                                fallback["vector_warning"] = Value::String(error);
                                fallback
                            }
                        }
                    }
                    #[cfg(not(target_os = "windows"))]
                    {
                        store.query(&retrieval_payload)?
                    }
                };
                Ok::<Value, String>(response)
            })
        })
        .await??;

    let citations = response
        .get("citations")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let community_only = response
        .get("community_only")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if community_only {
        response["model_warning"] =
            Value::String("仅命中社区信息，已保持证据模式，不调用模型生成事实结论。".to_string());
    } else if payload.get("llm").is_some() && !citations.is_empty() {
        let question = payload
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match runtime::with_heavy_network_permit(
            "api_research_answer",
            news_rag::synthesize_research_answer(payload.get("llm"), question, &citations),
        )
        .await
        {
            Ok(Some(answer)) => match research::validate_model_answer(&answer, &citations) {
                Ok(()) => {
                    response["answer"] = Value::String(answer);
                    response["mode"] = Value::String("model".to_string());
                }
                Err(error) => {
                    response["model_warning"] = Value::String(error);
                }
            },
            Ok(None) => {}
            Err(error) => {
                response["model_warning"] = Value::String(error);
            }
        }
    }

    let thread_id = payload
        .get("thread_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let question = payload
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let saved_response = response.clone();
    runtime::run_io_bound("api_research_save_answer", move || {
        research::with_app_store_at_generation(&app, database_generation, |store| {
            if let Some(thread_id) = thread_id.as_deref() {
                store.save_answer(thread_id, &question, &saved_response)?;
            }
            Ok(())
        })
    })
    .await??;
    Ok(response)
}

#[tauri::command]
async fn api_research_refresh(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let news = runtime::with_heavy_network_permit(
        "api_research_refresh",
        news_rag::api_news_rag_impl(app.clone(), payload),
    )
    .await?;
    let imported = {
        let app = app.clone();
        runtime::run_io_bound("api_research_ingest_news", move || {
            research::ingest_news_cache(&app)
        })
        .await??
    };
    schedule_research_embeddings(app);
    Ok(json!({"news": news, "imported": imported}))
}

#[tauri::command]
async fn api_research_threads(app: tauri::AppHandle) -> Result<Value, String> {
    runtime::run_io_bound("api_research_threads", move || {
        research::with_app_store(&app, |store| store.threads())
    })
    .await?
}

#[tauri::command]
async fn api_research_thread_create(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    runtime::run_io_bound("api_research_thread_create", move || {
        research::with_app_store(&app, |store| store.create_thread(&payload))
    })
    .await?
}

#[tauri::command]
async fn api_research_thread_detail(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    runtime::run_io_bound("api_research_thread_detail", move || {
        let thread_id = payload
            .get("thread_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "thread_id is required".to_string())?;
        research::with_app_store(&app, |store| store.thread(thread_id))
    })
    .await?
}

#[tauri::command]
async fn api_research_thread_delete(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let thread_id = payload
        .get("thread_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "thread_id is required".to_string())?
        .to_string();
    if thread_id.len() > research::MAX_RESEARCH_THREAD_ID_BYTES {
        return Err(format!(
            "thread_id exceeds {} bytes",
            research::MAX_RESEARCH_THREAD_ID_BYTES
        ));
    }
    runtime::run_io_bound("api_research_thread_delete", move || {
        research::with_app_store(&app, |store| {
            let deleted = store.delete_thread(&thread_id)?;
            Ok(json!({"deleted": deleted}))
        })
    })
    .await?
}

#[tauri::command]
async fn api_research_index_status(app: tauri::AppHandle) -> Result<Value, String> {
    runtime::run_io_bound("api_research_index_status", move || {
        research::with_app_store(&app, |store| {
            let mut status = store.index_status()?;
            status["documents"] = store.document_statuses()?["items"].clone();
            #[cfg(target_os = "windows")]
            let vector_status =
                research_embeddings::model_status(&research_embedding_model_dir(&app));
            #[cfg(not(target_os = "windows"))]
            let vector_status = status["vector"].clone();
            Ok(finalize_research_index_status(status, vector_status))
        })
    })
    .await?
}

fn finalize_research_index_status(mut status: Value, vector_status: Value) -> Value {
    let count = |field: &str| status.get(field).and_then(Value::as_i64).unwrap_or(0);
    let model_ready = vector_status
        .get("ready")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let hybrid_ready = status
        .get("fts_healthy")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && model_ready
        && count("embedding_count") > 0
        && count("embedding_pending_count") == 0
        && count("embedding_stale_count") == 0
        && count("embedding_orphan_count") == 0
        && count("embedding_invalid_count") == 0;
    status["vector"] = vector_status;
    status["hybrid_ready"] = Value::Bool(hybrid_ready);
    status
}

#[tauri::command]
async fn api_research_rebuild_index(app: tauri::AppHandle) -> Result<Value, String> {
    runtime::run_cpu_bound("api_research_rebuild_index", move || {
        research::with_app_store(&app, |store| store.rebuild_fts())
    })
    .await?
}

#[tauri::command]
async fn api_research_rebuild_embeddings(app: tauri::AppHandle) -> Result<Value, String> {
    #[cfg(target_os = "windows")]
    {
        runtime::run_cpu_bound("api_research_rebuild_embeddings", move || {
            rebuild_research_embeddings(&app)
        })
        .await?
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        Err("Android uses BM25 only and does not load the Windows embedding model".to_string())
    }
}

#[cfg(target_os = "windows")]
fn rebuild_research_embeddings(app: &tauri::AppHandle) -> Result<Value, String> {
    research::with_app_store(app, |store| {
        let model_dir = research_embedding_model_dir(app);
        let mut stored = 0usize;
        loop {
            let pending = store.pending_embedding_chunks(256)?;
            if pending.is_empty() {
                break;
            }
            let texts = pending
                .iter()
                .map(|item| item.text.clone())
                .collect::<Vec<_>>();
            let vectors = research_embeddings::embed(&model_dir, &texts)?;
            if vectors.len() != pending.len() {
                return Err("embedding service returned an unexpected batch size".to_string());
            }
            let items = pending.into_iter().zip(vectors).collect::<Vec<_>>();
            stored += items.len();
            store.store_embeddings(&items, research_embeddings::MODEL_ID)?;
        }
        Ok(json!({
            "stored": stored,
            "model_id": research_embeddings::MODEL_ID,
            "notes": if stored == 0 {
                vec!["所有分块均已有当前内容哈希对应的向量。"]
            } else {
                Vec::<&str>::new()
            }
        }))
    })
}

#[cfg(target_os = "windows")]
fn schedule_research_embeddings(app: tauri::AppHandle) {
    if !RESEARCH_EMBEDDING_JOB_GATE.request() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        loop {
            RESEARCH_EMBEDDING_JOB_GATE.begin_cycle();
            let worker_app = app.clone();
            let result = runtime::run_cpu_bound("background_research_embeddings", move || {
                rebuild_research_embeddings(&worker_app)
            })
            .await
            .and_then(|result| result);
            let event = match result {
                Ok(status) => json!({"ok": true, "status": status}),
                Err(error) => json!({"ok": false, "error": error}),
            };
            let _ = app.emit("research-embedding-status", event);
            if !RESEARCH_EMBEDDING_JOB_GATE.finish_cycle() {
                break;
            }
        }
    });
}

#[cfg(not(target_os = "windows"))]
fn schedule_research_embeddings(_app: tauri::AppHandle) {}

#[tauri::command]
async fn api_research_import_url(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    #[cfg(mobile)]
    {
        let _ = (app, payload);
        Err("URL research import is only available on desktop".to_string())
    }
    #[cfg(not(mobile))]
    {
        let result = runtime::with_heavy_network_permit(
            "api_research_import_url",
            research_import::import_url(&app, &payload),
        )
        .await?;
        schedule_research_embeddings(app);
        Ok(result)
    }
}

#[tauri::command]
async fn api_research_import_pdf(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    #[cfg(mobile)]
    {
        let _ = (app, payload);
        Err("PDF research import is only available on desktop".to_string())
    }
    #[cfg(not(mobile))]
    {
        let worker_app = app.clone();
        let result = runtime::run_cpu_bound("api_research_import_pdf", move || {
            research_import::import_pdf(&worker_app, &payload)
        })
        .await??;
        schedule_research_embeddings(app);
        Ok(result)
    }
}

#[tauri::command]
async fn api_research_pack_export(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    runtime::run_io_bound("api_research_pack_export", move || {
        research::export_app_pack(&app, &payload)
    })
    .await?
}

#[tauri::command]
async fn api_research_pack_import(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let worker_app = app.clone();
    let result = runtime::run_io_bound("api_research_pack_import", move || {
        research::import_app_pack(&worker_app, &payload)
    })
    .await??;
    schedule_research_embeddings(app);
    Ok(result)
}

#[tauri::command]
async fn api_research_pack_rollback(app: tauri::AppHandle) -> Result<Value, String> {
    runtime::run_io_bound("api_research_pack_rollback", move || {
        research::rollback_app_pack(&app)
    })
    .await?
}

#[cfg(target_os = "windows")]
fn research_embedding_model_dir(app: &tauri::AppHandle) -> PathBuf {
    let bundled = app
        .path()
        .resource_dir()
        .ok()
        .map(|root| root.join("models").join("bge-small-zh-v1.5-int8"));
    if let Some(path) = bundled.filter(|path| path.join("model_quantized.onnx").exists()) {
        return path;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models/bge-small-zh-v1.5-int8")
}

#[tauri::command]
fn api_data_sources(app: tauri::AppHandle) -> Result<Value, String> {
    Ok(json!({
        "current": "tauri",
        "available": [{"id": "tauri", "name": "Tauri/Rust", "description": "Tauri/Rust native market cache and Tencent refresh path."}],
        "status": market_data_status(&app)?
    }))
}

#[tauri::command]
fn api_stock_get(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
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
fn api_minutes(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
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
fn api_order_book(_app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
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
async fn api_watchlist_list(app: tauri::AppHandle) -> Result<Value, String> {
    runtime::run_io_bound("api_watchlist_list", move || watchlist_list_sync(&app)).await?
}

#[tauri::command]
async fn api_watchlist_replace(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    runtime::run_io_bound("api_watchlist_replace", move || {
        let items = watchlist_items_from_payload(&payload)?;
        watchlist_replace_sync(&app, items)
    })
    .await?
}

#[tauri::command]
async fn api_watchlist_add(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    runtime::run_io_bound("api_watchlist_add", move || {
        let item = watchlist_item_from_value(&payload)?;
        watchlist_upsert_sync(&app, item)?;
        watchlist_list_sync(&app)
    })
    .await?
}

#[tauri::command]
async fn api_watchlist_remove(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    runtime::run_io_bound("api_watchlist_remove", move || {
        let code = payload
            .get("code")
            .and_then(Value::as_str)
            .and_then(normalize_stock_code)
            .ok_or_else(|| "watchlist code is required".to_string())?;
        let conn = open_watchlist_db(&app)?;
        conn.execute("DELETE FROM watchlist WHERE code = ?1", params![code])
            .map_err(|error| format!("delete watchlist item failed: {error}"))?;
        watchlist_rows(&conn)
    })
    .await?
}

#[tauri::command]
async fn api_watchlist_clear(app: tauri::AppHandle) -> Result<Value, String> {
    runtime::run_io_bound("api_watchlist_clear", move || {
        let conn = open_watchlist_db(&app)?;
        conn.execute("DELETE FROM watchlist", [])
            .map_err(|error| format!("clear watchlist failed: {error}"))?;
        Ok(json!([]))
    })
    .await?
}

#[tauri::command]
fn api_rag_pack_status(app: tauri::AppHandle) -> Result<Value, String> {
    rag_pack::rag_pack_status(app)
}
#[tauri::command]
fn api_rag_pack_build(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    rag_pack::rag_pack_build(app, payload)
}
#[tauri::command]
fn api_rag_pack_build_from_news_cache(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    rag_pack::rag_pack_build_from_news_cache(app, payload)
}
#[tauri::command]
fn api_rag_pack_query(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    rag_pack::rag_pack_query(app, payload)
}
#[tauri::command]
fn api_upstream_rag_status(app: tauri::AppHandle) -> Result<Value, String> {
    rag_pack::upstream_rag_status(app)
}
#[tauri::command]
fn api_upstream_rag_build(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let data = cached_market_data(&app)?;
    rag_pack::upstream_rag_build(app, payload, data)
}
#[tauri::command]
fn api_upstream_rag_transfer_start(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    rag_pack::upstream_rag_transfer_start(app, payload)
}

#[tauri::command]
async fn api_agent_stream(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let mut payload = payload;
    let fields = payload
        .as_object_mut()
        .ok_or_else(|| "Agent payload must be an object".to_string())?;
    // Normalize once and write the normalized value back: the ledger trims `run_id` when it
    // inserts the row, so completing with the raw value would match no row and strand the run.
    let run_id = fields
        .get("run_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(agent_ledger::next_run_id);
    fields.insert("run_id".to_string(), Value::String(run_id.clone()));
    agent_harness::validate_payload(&payload)?;
    let started_at_epoch_ms = agent_ledger::current_epoch_millis();
    let ledger_llm = payload.get("llm").cloned();
    let ledger_payload = payload.clone();
    let start_app = app.clone();
    let ledger_started = match runtime::run_io_bound("agent_run_start", move || {
        agent_ledger::with_app_store(&start_app, |store| {
            store.start_run(&ledger_payload, started_at_epoch_ms)
        })
    })
    .await
    .and_then(|result| result)
    {
        Ok(()) => true,
        Err(error) if agent_ledger::is_conversation_deleted_error(&error) => {
            return Err(error);
        }
        Err(error) => {
            eprintln!("agent run ledger start failed; continuing without persistence: {error}");
            false
        }
    };

    let events = Arc::new(Mutex::new(Vec::<Value>::new()));
    let sink_events = Arc::clone(&events);
    let event_app = app.clone();
    let execution = match cached_market_data(&app) {
        Ok(data) => {
            rig_runtime::execute_with_event_sink(payload, data, move |event| {
                // The terminal `result` event carries the whole response, which `complete_run`
                // already stores in `result_json`. Capturing it too would double every row and
                // every detail payload. The webview still receives it for the live stream.
                if event.get("type").and_then(Value::as_str) != Some("result") {
                    if let Ok(mut captured) = sink_events.lock() {
                        captured.push(event.clone());
                    }
                }
                let _ = event_app.emit("agent-stream-event", event);
            })
            .await
        }
        Err(error) => Err(error),
    };
    let completed_at_epoch_ms = agent_ledger::current_epoch_millis();
    let captured_events = events.lock().map(|items| items.clone()).unwrap_or_default();

    match execution {
        Ok(outcome) => {
            let response = outcome.response;
            if ledger_started {
                let ledger_app = app.clone();
                let ledger_events =
                    agent_harness::redact_persisted_events(&captured_events, ledger_llm.as_ref());
                let ledger_response =
                    agent_harness::redact_persisted_response(&response, ledger_llm.as_ref());
                let completion = runtime::run_io_bound("agent_run_complete", move || {
                    agent_ledger::with_app_store(&ledger_app, |store| {
                        store.complete_run(
                            &run_id,
                            &ledger_events,
                            &ledger_response,
                            completed_at_epoch_ms,
                        )
                    })
                })
                .await
                .and_then(|result| result);
                if let Err(error) = completion {
                    eprintln!("agent run ledger completion failed: {error}");
                }
            }
            Ok(response)
        }
        Err(error) => {
            if ledger_started {
                let ledger_app = app.clone();
                let ledger_events =
                    agent_harness::redact_persisted_events(&captured_events, ledger_llm.as_ref());
                let ledger_error =
                    agent_harness::redact_persisted_error(&error, ledger_llm.as_ref());
                let failure_recording = runtime::run_io_bound("agent_run_fail", move || {
                    agent_ledger::with_app_store(&ledger_app, |store| {
                        store.fail_run(
                            &run_id,
                            &ledger_events,
                            &ledger_error,
                            completed_at_epoch_ms,
                        )
                    })
                })
                .await
                .and_then(|result| result);
                if let Err(ledger_error) = failure_recording {
                    eprintln!("agent run ledger failure recording failed: {ledger_error}");
                }
            }
            Err(error)
        }
    }
}

#[tauri::command]
async fn api_agent_run_list(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let limit = payload
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(50)
        .min(200) as usize;
    let conversation_id = payload
        .get("conversation_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if conversation_id
        .as_ref()
        .is_some_and(|value| value.len() > agent_ledger::MAX_AGENT_LEDGER_ID_BYTES)
    {
        return Err(format!(
            "conversation_id exceeds {} bytes",
            agent_ledger::MAX_AGENT_LEDGER_ID_BYTES
        ));
    }
    runtime::run_io_bound("api_agent_run_list", move || {
        agent_ledger::with_app_store(&app, |store| {
            let runs = store.list_runs(limit, conversation_id.as_deref())?;
            Ok(json!({"runs": runs}))
        })
    })
    .await?
}

#[tauri::command]
async fn api_agent_run_metrics(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let limit = payload
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(200)
        .min(2_000) as usize;
    let conversation_id = payload
        .get("conversation_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if conversation_id
        .as_ref()
        .is_some_and(|value| value.len() > agent_ledger::MAX_AGENT_LEDGER_ID_BYTES)
    {
        return Err(format!(
            "conversation_id exceeds {} bytes",
            agent_ledger::MAX_AGENT_LEDGER_ID_BYTES
        ));
    }
    runtime::run_io_bound("api_agent_run_metrics", move || {
        agent_ledger::with_app_store(&app, |store| {
            store.metrics(limit, conversation_id.as_deref())
        })
    })
    .await?
}

fn agent_run_detail_response(run: Option<Value>) -> Value {
    let run = run.map(|mut record| {
        if let Some(object) = record.as_object_mut() {
            object.remove("request");
        }
        record
    });
    json!({"run": run})
}

#[tauri::command]
async fn api_agent_run_get(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let run_id = payload
        .get("run_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "run_id is required".to_string())?
        .to_string();
    if run_id.len() > agent_ledger::MAX_AGENT_LEDGER_ID_BYTES {
        return Err(format!(
            "run_id exceeds {} bytes",
            agent_ledger::MAX_AGENT_LEDGER_ID_BYTES
        ));
    }
    runtime::run_io_bound("api_agent_run_get", move || {
        agent_ledger::with_app_store(&app, |store| {
            Ok(agent_run_detail_response(store.get_run(&run_id)?))
        })
    })
    .await?
}

#[tauri::command]
async fn api_agent_run_delete_conversation(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let conversation_id = payload
        .get("conversation_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "conversation_id is required".to_string())?
        .to_string();
    if conversation_id.len() > agent_ledger::MAX_AGENT_LEDGER_ID_BYTES {
        return Err(format!(
            "conversation_id exceeds {} bytes",
            agent_ledger::MAX_AGENT_LEDGER_ID_BYTES
        ));
    }
    runtime::run_io_bound("api_agent_run_delete_conversation", move || {
        agent_ledger::with_app_store(&app, |store| {
            let deleted = store.delete_conversation_runs(&conversation_id)?;
            Ok(json!({"deleted": deleted}))
        })
    })
    .await?
}

#[cfg(test)]
mod agent_run_api_tests {
    use super::{agent_run_detail_response, finalize_research_index_status};
    use serde_json::{json, Value};

    #[test]
    fn detail_response_removes_request_and_preserves_replay_fields() {
        let response = agent_run_detail_response(Some(json!({
            "run_id": "run-1",
            "request": {"llm": {"base_url": "https://example.test"}},
            "events": [{"type": "status", "stage": "tools"}],
            "result": {"reply": "done"},
            "status": "completed"
        })));

        assert!(response["run"].get("request").is_none());
        assert_eq!(response["run"]["events"][0]["stage"], "tools");
        assert_eq!(response["run"]["result"]["reply"], "done");
        assert_eq!(response["run"]["status"], "completed");
    }

    #[test]
    fn detail_response_keeps_missing_run_as_null() {
        assert_eq!(agent_run_detail_response(None)["run"], Value::Null);
    }

    #[test]
    fn hybrid_readiness_requires_verified_model_and_complete_nonempty_vectors() {
        let base = json!({
            "fts_healthy": true,
            "embedding_count": 1,
            "embedding_pending_count": 0,
            "embedding_stale_count": 0,
            "embedding_orphan_count": 0,
            "embedding_invalid_count": 0
        });
        assert_eq!(
            finalize_research_index_status(base.clone(), json!({"ready": false}))["hybrid_ready"],
            false
        );
        assert_eq!(
            finalize_research_index_status(base.clone(), json!({"ready": true}))["hybrid_ready"],
            true
        );

        let mut empty = base.clone();
        empty["embedding_count"] = json!(0);
        assert_eq!(
            finalize_research_index_status(empty, json!({"ready": true}))["hybrid_ready"],
            false
        );
        let mut pending = base;
        pending["embedding_pending_count"] = json!(1);
        assert_eq!(
            finalize_research_index_status(pending, json!({"ready": true}))["hybrid_ready"],
            false
        );
    }
}
#[tauri::command]
async fn core_validate_data_source(payload: Value) -> Result<Value, String> {
    runtime::run_cpu_bound("core_validate_data_source", move || {
        gp_core::validate_data_source_value(payload).map_err(|error| error.to_string())
    })
    .await?
}

#[tauri::command]
fn core_mobile_market_data_read(app: tauri::AppHandle) -> Result<Value, String> {
    read_mobile_market_data(&app)
}

#[tauri::command]
fn core_mobile_market_data_write(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    write_mobile_market_data(&app, payload)
}

#[tauri::command]
fn core_mobile_market_data_clear(app: tauri::AppHandle) -> Result<Value, String> {
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
async fn core_mobile_network_probe(payload: Option<Value>) -> Result<Value, String> {
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

async fn probe_mobile_url(
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

#[tauri::command]
async fn api_llm_models(payload: Value) -> Result<Value, String> {
    let base_url = payload
        .get("base_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "请先填写供应商接口地址。".to_string())?;
    let provider = payload
        .get("provider")
        .and_then(Value::as_str)
        .unwrap_or("openai-compatible")
        .trim()
        .to_ascii_lowercase();
    let api_format = llm_api_format(&payload);
    let api_key = payload
        .get("api_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let timeout_seconds = payload_usize_field(&payload, "timeout_seconds", 60, 10, 120);
    let user_agent = payload
        .get("custom_user_agent")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("gp-assistant/0.4 llm-model-catalog");
    if user_agent.len() > 256 || user_agent.contains(['\r', '\n']) {
        return Err("自定义 User-Agent 格式不正确。".to_string());
    }
    let endpoint = llm_models_endpoint(base_url, api_format)?;
    let client = build_http_client_with_proxy(
        user_agent,
        Duration::from_secs(timeout_seconds as u64),
        Some(&payload),
    )?;

    let mut request = client
        .get(endpoint.clone())
        .header("Accept", "application/json");
    if llm_models_uses_anthropic_auth(base_url, api_format) {
        request = request.header("anthropic-version", "2023-06-01");
        if !api_key.is_empty() {
            request = request.header("x-api-key", api_key);
        }
    } else if !api_key.is_empty() {
        request = request.bearer_auth(api_key);
    }

    let response = request
        .send()
        .await
        .map_err(|error| format!("连接供应商失败：{error}"))?;
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|bytes| bytes > LLM_MODEL_LIST_MAX_BYTES as u64)
    {
        return Err("供应商返回的模型列表过大，已停止读取。".to_string());
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("读取供应商模型列表失败：{error}"))?;
        if body.len().saturating_add(chunk.len()) > LLM_MODEL_LIST_MAX_BYTES {
            return Err("供应商返回的模型列表过大，已停止读取。".to_string());
        }
        body.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        return Err(llm_models_http_error(status.as_u16(), &body, api_key));
    }

    let response_json: Value = serde_json::from_slice(&body)
        .map_err(|error| format!("供应商模型列表不是有效 JSON：{error}"))?;
    let models = parse_llm_model_options(&response_json);
    Ok(json!({
        "provider": provider,
        "endpoint": endpoint.to_string(),
        "count": models.len(),
        "models": models,
    }))
}

#[tauri::command]
async fn api_llm_test(payload: Value) -> Result<Value, String> {
    let base_url = payload
        .get("base_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "请先填写供应商接口地址。".to_string())?;
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "请先填写默认模型。".to_string())?;
    let api_key = payload
        .get("api_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let api_format = llm_api_format(&payload);
    let full_url = payload.get("endpoint_mode").and_then(Value::as_str) == Some("full_url");
    let endpoint = llm_inference_endpoint(base_url, api_format, full_url)?;
    let custom_user_agent = payload
        .get("custom_user_agent")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("gp-assistant/0.4 llm-connection-test");
    if custom_user_agent.len() > 256 || custom_user_agent.contains(['\r', '\n']) {
        return Err("自定义 User-Agent 格式不正确。".to_string());
    }
    let timeout_seconds = payload_usize_field(&payload, "timeout_seconds", 30, 5, 120);
    let client = build_http_client_with_proxy(
        custom_user_agent,
        Duration::from_secs(timeout_seconds as u64),
        Some(&payload),
    )?;
    let body = match api_format {
        "openai_responses" => json!({
            "model": model,
            "input": "Reply with OK.",
            "max_output_tokens": 16,
        }),
        "anthropic_messages" => json!({
            "model": model,
            "messages": [{"role": "user", "content": "Reply with OK."}],
            "max_tokens": 16,
        }),
        _ => json!({
            "model": model,
            "messages": [{"role": "user", "content": "Reply with OK."}],
            "max_tokens": 16,
        }),
    };
    let body =
        serde_json::to_vec(&body).map_err(|error| format!("序列化连接测试请求失败：{error}"))?;
    let mut request = client
        .post(endpoint.clone())
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .body(body);
    if api_format == "anthropic_messages" {
        request = request.header("anthropic-version", "2023-06-01");
        if !api_key.is_empty() {
            request = request.header("x-api-key", api_key);
        }
    } else if !api_key.is_empty() {
        request = request.bearer_auth(api_key);
    }

    let started_at = epoch_millis();
    let response = request
        .send()
        .await
        .map_err(|error| format!("连接供应商失败：{error}"))?;
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|bytes| bytes > LLM_MODEL_LIST_MAX_BYTES as u64)
    {
        return Err("供应商测试响应过大，已停止读取。".to_string());
    }
    let mut response_body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("读取供应商测试响应失败：{error}"))?;
        if response_body.len().saturating_add(chunk.len()) > LLM_MODEL_LIST_MAX_BYTES {
            return Err("供应商测试响应过大，已停止读取。".to_string());
        }
        response_body.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        return Err(llm_connection_http_error(
            status.as_u16(),
            &response_body,
            api_key,
        ));
    }
    let response_json: Value = serde_json::from_slice(&response_body)
        .map_err(|error| format!("供应商测试响应不是有效 JSON：{error}"))?;
    if !llm_test_response_has_content(&response_json, api_format) {
        return Err("供应商已响应，但返回格式与所选协议不匹配。".to_string());
    }
    Ok(json!({
        "ok": true,
        "endpoint": endpoint.to_string(),
        "status": status.as_u16(),
        "elapsed_ms": epoch_millis().saturating_sub(started_at),
        "api_format": api_format,
    }))
}

pub(crate) fn llm_inference_endpoint(
    base_url: &str,
    api_format: &str,
    full_url: bool,
) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse(base_url.trim())
        .map_err(|_| "供应商接口地址格式不正确。".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("供应商接口地址仅支持有效的 http 或 https 地址。".to_string());
    }
    if url.scheme() == "http" && !llm_plain_http_host_allowed(url.host_str()) {
        return Err("公网模型接口必须使用 HTTPS；HTTP 仅允许本机或私有局域网地址。".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("供应商接口地址不能包含用户名或密码。".to_string());
    }
    if full_url {
        if api_format == "anthropic_messages" {
            if url.query().is_some() || url.fragment().is_some() {
                return Err("Anthropic 完整 URL 不能包含查询参数或片段。".to_string());
            }
            if !url.path().trim_end_matches('/').ends_with("/v1/messages") {
                return Err("Anthropic 完整 URL 必须以 /v1/messages 结尾。".to_string());
            }
        }
        return Ok(url);
    }
    let suffix = match api_format {
        "openai_responses" => "/responses",
        "anthropic_messages" => "/v1/messages",
        _ => "/chat/completions",
    };
    let mut path = url.path().trim_end_matches('/').to_string();
    for known_suffix in [
        "/chat/completions",
        "/responses",
        "/v1/messages",
        "/messages",
    ] {
        if let Some(base) = path.strip_suffix(known_suffix) {
            path = base.to_string();
            break;
        }
    }
    if api_format == "anthropic_messages" {
        if let Some(base) = path.strip_suffix("/v1") {
            path = base.to_string();
        }
    }
    if !path.ends_with(suffix) {
        path.push_str(suffix);
    }
    if !path.starts_with('/') {
        path.insert(0, '/');
    }
    url.set_path(&path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn llm_test_response_has_content(response: &Value, api_format: &str) -> bool {
    match api_format {
        "openai_responses" => {
            response
                .get("output_text")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
                || response
                    .get("output")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|item| item.get("content").and_then(Value::as_array))
                    .flatten()
                    .any(|content| content.get("text").and_then(Value::as_str).is_some())
        }
        "anthropic_messages" => response
            .get("content")
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items
                    .iter()
                    .any(|item| item.get("text").and_then(Value::as_str).is_some())
            }),
        _ => response
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .is_some(),
    }
}

fn llm_connection_http_error(status: u16, body: &[u8], api_key: &str) -> String {
    if matches!(status, 401 | 403) {
        return format!("供应商鉴权失败，请检查 API 密钥（HTTP {status}）。");
    }
    let detail = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|error| error.get("message").or(Some(error)))
                .and_then(Value::as_str)
                .or_else(|| value.get("message").and_then(Value::as_str))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| redact_llm_error_detail(&truncate_for_note(value, 180), api_key))
        });
    detail
        .map(|message| format!("供应商返回 HTTP {status}：{message}"))
        .unwrap_or_else(|| format!("供应商返回 HTTP {status}，连接测试失败。"))
}

fn llm_models_endpoint(base_url: &str, api_format: &str) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse(base_url.trim())
        .map_err(|_| "供应商接口地址格式不正确。".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("供应商接口地址仅支持 http 或 https。".to_string());
    }
    if url.host_str().is_none() {
        return Err("供应商接口地址缺少主机名。".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("供应商接口地址不能包含用户名或密码。".to_string());
    }
    if url.scheme() == "http" && !llm_plain_http_host_allowed(url.host_str()) {
        return Err("公网模型接口必须使用 HTTPS；HTTP 仅允许本机或私有局域网地址。".to_string());
    }

    if is_official_deepseek_anthropic_base_url(&url, api_format) {
        url.set_path("/models");
        url.set_query(None);
        url.set_fragment(None);
        return Ok(url);
    }

    let mut path = url.path().trim_end_matches('/').to_string();
    for suffix in [
        "/chat/completions",
        "/responses",
        "/v1/messages",
        "/messages",
        "/models",
    ] {
        if let Some(base) = path.strip_suffix(suffix) {
            path = base.to_string();
            break;
        }
    }
    if api_format == "anthropic_messages" {
        if let Some(base) = path.strip_suffix("/v1") {
            path = base.to_string();
        }
    }
    let suffix = if api_format == "anthropic_messages" {
        "/v1/models"
    } else {
        "/models"
    };
    if !path.ends_with(suffix) {
        path.push_str(suffix);
    }
    if path.is_empty() || !path.starts_with('/') {
        path.insert(0, '/');
    }
    url.set_path(&path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn llm_api_format(payload: &Value) -> &'static str {
    match payload.get("api_format").and_then(Value::as_str) {
        Some("openai_responses") => "openai_responses",
        Some("anthropic_messages") => "anthropic_messages",
        Some("openai_chat") => "openai_chat",
        None if payload
            .get("provider")
            .and_then(Value::as_str)
            .is_some_and(|provider| {
                provider.eq_ignore_ascii_case("anthropic-compatible")
                    || provider.eq_ignore_ascii_case("anthropic")
            }) =>
        {
            "anthropic_messages"
        }
        _ => "openai_chat",
    }
}

fn llm_models_uses_anthropic_auth(base_url: &str, api_format: &str) -> bool {
    api_format == "anthropic_messages"
        && !reqwest::Url::parse(base_url)
            .ok()
            .is_some_and(|url| is_official_deepseek_anthropic_base_url(&url, api_format))
}

fn is_official_deepseek_anthropic_base_url(url: &reqwest::Url, api_format: &str) -> bool {
    api_format == "anthropic_messages"
        && url
            .host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case("api.deepseek.com"))
        && url.path().trim_end_matches('/') == "/anthropic"
}

fn llm_plain_http_host_allowed(host: Option<&str>) -> bool {
    let Some(host) = host else {
        return false;
    };
    host.eq_ignore_ascii_case("localhost")
        || host.ends_with(".localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| match address {
                std::net::IpAddr::V4(address) => {
                    address.is_loopback() || address.is_private() || address.is_link_local()
                }
                std::net::IpAddr::V6(address) => {
                    address.is_loopback()
                        || address.is_unique_local()
                        || address.is_unicast_link_local()
                }
            })
}

fn parse_llm_model_options(response: &Value) -> Vec<Value> {
    let rows = response
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| response.get("models").and_then(Value::as_array))
        .or_else(|| {
            response
                .get("result")
                .and_then(|value| value.get("data"))
                .and_then(Value::as_array)
        });
    let Some(rows) = rows else {
        return Vec::new();
    };

    let mut seen = HashSet::new();
    let mut models = Vec::new();
    for row in rows {
        let id = row
            .as_str()
            .or_else(|| row.get("id").and_then(Value::as_str))
            .or_else(|| row.get("model").and_then(Value::as_str))
            .or_else(|| row.get("name").and_then(Value::as_str))
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.len() <= 256);
        let Some(id) = id else {
            continue;
        };
        if !seen.insert(id.to_string()) {
            continue;
        }
        let display_name = row
            .get("display_name")
            .and_then(Value::as_str)
            .or_else(|| row.get("name").and_then(Value::as_str))
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != id);
        let owned_by = row
            .get("owned_by")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        models.push(json!({
            "id": id,
            "name": display_name,
            "owned_by": owned_by,
        }));
    }
    models
}

fn llm_models_http_error(status: u16, body: &[u8], api_key: &str) -> String {
    match status {
        401 | 403 => return format!("供应商鉴权失败，请检查 API 密钥（HTTP {status}）。"),
        404 => {
            return "未找到模型列表接口（HTTP 404），请确认接口地址包含正确的版本路径，例如 /v1。"
                .to_string()
        }
        _ => {}
    }
    let detail = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|error| error.get("message").or(Some(error)))
                .and_then(Value::as_str)
                .or_else(|| value.get("message").and_then(Value::as_str))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| redact_llm_error_detail(&truncate_for_note(value, 180), api_key))
        });
    detail
        .map(|message| format!("供应商返回 HTTP {status}：{message}"))
        .unwrap_or_else(|| format!("供应商返回 HTTP {status}，未能拉取模型列表。"))
}

fn redact_llm_error_detail(detail: &str, api_key: &str) -> String {
    let api_key = api_key.trim();
    if api_key.len() < 4 {
        return detail.to_string();
    }
    detail.replace(api_key, "[已隐藏密钥]")
}

pub(crate) fn build_tencent_http_client(
    user_agent: &str,
    timeout: Duration,
) -> Result<reqwest::Client, String> {
    build_http_client_with_proxy(user_agent, timeout, None)
}

fn build_direct_http_client(
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

fn http_client_cache() -> &'static Mutex<HashMap<HttpClientCacheKey, reqwest::Client>> {
    HTTP_CLIENT_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(target_os = "android")]
fn apply_android_tls_backend(
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
fn apply_android_tls_backend(
    builder: reqwest::ClientBuilder,
) -> Result<reqwest::ClientBuilder, String> {
    Ok(builder)
}

fn apply_proxy_url(
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

fn normalize_manual_proxy_url(value: &str) -> String {
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
fn powershell_http_get_bytes(url: &str, timeout_secs: u64) -> Result<Vec<u8>, String> {
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
fn powershell_http_get_bytes(_url: &str, _timeout_secs: u64) -> Result<Vec<u8>, String> {
    Err("PowerShell HTTP fallback only runs on Windows".to_string())
}

#[cfg(windows)]
fn powershell_http_get_bytes_with_headers(
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
fn powershell_http_get_bytes_with_headers(
    _url: &str,
    _timeout_secs: u64,
    _user_agent: &str,
    _referer: &str,
) -> Result<Vec<u8>, String> {
    Err("PowerShell HTTP fallback only runs on Windows".to_string())
}

fn decode_utf8_lossy(bytes: Vec<u8>) -> String {
    match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => String::from_utf8_lossy(&error.into_bytes()).to_string(),
    }
}

fn payload_usize_field(value: &Value, key: &str, default: usize, min: usize, max: usize) -> usize {
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

fn truncate_for_note(value: &str, max_chars: usize) -> String {
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
async fn core_mobile_market_data_refresh_tencent(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    runtime::with_market_refresh_permit(
        "core_mobile_market_data_refresh_tencent",
        core_mobile_market_data_refresh_tencent_inner(app, payload),
    )
    .await
}

async fn core_mobile_market_data_refresh_tencent_inner(
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

fn core_mobile_market_data_ingest_tencent_quotes(
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

fn refresh_seed_cache() -> &'static Mutex<HashMap<PathBuf, Value>> {
    REFRESH_SEED_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn refresh_financial_snapshot_cache() -> &'static Mutex<HashMap<PathBuf, Arc<Value>>> {
    REFRESH_FINANCIAL_SNAPSHOT_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn mobile_market_data_cache() -> &'static Mutex<HashMap<PathBuf, MobileMarketDataCacheEntry>> {
    MOBILE_MARKET_DATA_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn screen_stock_overlay_cache() -> &'static Mutex<HashMap<PathBuf, ScreenStockOverlayCacheEntry>> {
    SCREEN_STOCK_OVERLAY_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn mobile_market_update_lock() -> &'static Mutex<()> {
    MOBILE_MARKET_UPDATE_LOCK.get_or_init(|| Mutex::new(()))
}

fn cache_context_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    mobile_market_data_path(app).ok()
}

fn remember_refresh_seed(app: &tauri::AppHandle, seed: &Value) {
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

fn clear_refresh_seed(app: &tauri::AppHandle) {
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

fn remember_refresh_financial_snapshot(app: &tauri::AppHandle, snapshot: &Value) {
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

fn refresh_financial_snapshot_payload(app: &tauri::AppHandle, payload: &Value) -> Value {
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

fn financial_snapshot_payload_present(value: &Value) -> bool {
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

fn refresh_seed_payload(app: &tauri::AppHandle, payload: &Value) -> Value {
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

fn market_data_payload_present(value: &Value) -> bool {
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

#[tauri::command]
fn core_upstream_rag_import(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let manifest_value = payload
        .get("manifest")
        .cloned()
        .ok_or_else(|| "缺少 manifest。".to_string())?;
    let mut manifest = match manifest_value {
        Value::Object(map) => map,
        _ => return Err("manifest 必须是 JSON 对象。".to_string()),
    };
    let pack_base64 = payload
        .get("pack_base64")
        .and_then(Value::as_str)
        .ok_or_else(|| "缺少 pack_base64。".to_string())?;
    let pack_bytes = general_purpose::STANDARD
        .decode(pack_base64)
        .map_err(|error| format!("RAG 包解码失败：{error}"))?;

    let expected_sha256 = manifest
        .get("sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| "manifest 缺少 sha256。".to_string())?;
    let actual_sha256 = sha256_hex(&pack_bytes);
    if !expected_sha256.eq_ignore_ascii_case(&actual_sha256) {
        return Err(format!(
            "RAG 包 sha256 校验失败：预期 {expected_sha256}，实际 {actual_sha256}。"
        ));
    }
    if let Some(expected_size) = manifest.get("file_size").and_then(Value::as_u64) {
        if expected_size != pack_bytes.len() as u64 {
            return Err(format!(
                "RAG 包大小校验失败：预期 {expected_size} 字节，实际 {} 字节。",
                pack_bytes.len()
            ));
        }
    }

    let stock_code = manifest
        .get("target_stock_code")
        .and_then(Value::as_str)
        .ok_or_else(|| "manifest 缺少 target_stock_code。".to_string())?;
    let pack_version = manifest
        .get("pack_version")
        .and_then(Value::as_str)
        .ok_or_else(|| "manifest 缺少 pack_version。".to_string())?;
    let stock_code_owned = stock_code.to_string();
    let pack_version_owned = pack_version.to_string();

    let root = upstream_rag_mobile_root(&app)?;
    let stock_dir = root.join(sanitize_path_part(&stock_code_owned));
    fs::create_dir_all(&stock_dir).map_err(|error| format!("创建 RAG 目录失败：{error}"))?;

    let version_dir = stock_dir.join(sanitize_path_part(&pack_version_owned));
    let tmp_dir = stock_dir.join(format!(
        "{}.tmp-{}",
        sanitize_path_part(&pack_version_owned),
        epoch_millis()
    ));
    if tmp_dir.exists() {
        fs::remove_dir_all(&tmp_dir).map_err(|error| format!("清理临时目录失败：{error}"))?;
    }
    fs::create_dir_all(&tmp_dir).map_err(|error| format!("创建临时目录失败：{error}"))?;

    let tmp_pack_path = tmp_dir.join("rag_pack.sqlite");
    let tmp_manifest_path = tmp_dir.join("manifest.json");
    fs::write(&tmp_pack_path, &pack_bytes).map_err(|error| format!("写入 RAG 包失败：{error}"))?;
    manifest.insert(
        "_local_pack_path".to_string(),
        json!(version_dir.join("rag_pack.sqlite").display().to_string()),
    );
    manifest.insert(
        "_local_manifest_path".to_string(),
        json!(version_dir.join("manifest.json").display().to_string()),
    );
    manifest.insert("_imported_at_epoch_ms".to_string(), json!(epoch_millis()));
    fs::write(
        &tmp_manifest_path,
        serde_json::to_vec_pretty(&Value::Object(manifest.clone()))
            .map_err(|error| format!("序列化 manifest 失败：{error}"))?,
    )
    .map_err(|error| format!("写入 manifest 失败：{error}"))?;

    if version_dir.exists() {
        fs::remove_dir_all(&version_dir).map_err(|error| format!("替换旧版本目录失败：{error}"))?;
    }
    fs::rename(&tmp_dir, &version_dir).map_err(|error| format!("提交 RAG 包失败：{error}"))?;

    let current_manifest_path = stock_dir.join("current_manifest.json");
    let previous_manifest_path = stock_dir.join("previous_manifest.json");
    if current_manifest_path.exists() {
        fs::copy(&current_manifest_path, &previous_manifest_path)
            .map_err(|error| format!("保存回滚 manifest 失败：{error}"))?;
    }
    fs::write(
        &current_manifest_path,
        serde_json::to_vec_pretty(&Value::Object(manifest.clone()))
            .map_err(|error| format!("序列化 current manifest 失败：{error}"))?,
    )
    .map_err(|error| format!("更新 current manifest 失败：{error}"))?;

    Ok(json!({
        "imported": true,
        "root": root.display().to_string(),
        "stock_code": stock_code_owned,
        "pack_version": pack_version_owned,
        "manifest": Value::Object(manifest),
        "notes": ["已校验 sha256 并完成原子替换。"]
    }))
}

#[tauri::command]
fn core_upstream_rag_list(app: tauri::AppHandle) -> Result<Value, String> {
    let root = upstream_rag_mobile_root(&app)?;
    let mut packs = Vec::new();
    if root.exists() {
        for stock_entry in
            fs::read_dir(&root).map_err(|error| format!("读取 RAG 目录失败：{error}"))?
        {
            let stock_entry =
                stock_entry.map_err(|error| format!("读取 RAG 子目录失败：{error}"))?;
            let stock_dir = stock_entry.path();
            if !stock_dir.is_dir() {
                continue;
            }
            let current_version = read_json_file(&stock_dir.join("current_manifest.json"))
                .ok()
                .and_then(|value| {
                    value
                        .get("pack_version")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
            for version_entry in fs::read_dir(&stock_dir)
                .map_err(|error| format!("读取 RAG 版本目录失败：{error}"))?
            {
                let version_entry =
                    version_entry.map_err(|error| format!("读取 RAG 版本失败：{error}"))?;
                let version_dir = version_entry.path();
                if !version_dir.is_dir() {
                    continue;
                }
                if let Ok(mut manifest) = read_json_file(&version_dir.join("manifest.json")) {
                    let pack_version = manifest
                        .get("pack_version")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    if let Value::Object(ref mut map) = manifest {
                        map.insert(
                            "current".to_string(),
                            json!(Some(pack_version.clone()) == current_version),
                        );
                    }
                    packs.push(manifest);
                }
            }
        }
    }
    let notes = if packs.is_empty() {
        vec!["安卓端尚未导入上下游 RAG 包。"]
    } else {
        Vec::<&str>::new()
    };
    Ok(json!({
        "root": root.display().to_string(),
        "packs": packs,
        "notes": notes
    }))
}

#[tauri::command]
fn core_upstream_rag_detail(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let root = upstream_rag_mobile_root(&app)?;
    let stock_code = payload
        .get("stock_code")
        .and_then(Value::as_str)
        .unwrap_or("");
    let pack_version = payload
        .get("pack_version")
        .and_then(Value::as_str)
        .unwrap_or("");
    let manifest_path = if !stock_code.is_empty() && !pack_version.is_empty() {
        root.join(sanitize_path_part(stock_code))
            .join(sanitize_path_part(pack_version))
            .join("manifest.json")
    } else if !stock_code.is_empty() {
        root.join(sanitize_path_part(stock_code))
            .join("current_manifest.json")
    } else {
        find_first_current_manifest(&root)
            .ok_or_else(|| "安卓端尚未导入上下游 RAG 包。".to_string())?
    };
    let manifest = read_json_file(&manifest_path)?;
    Ok(json!({
        "manifest": manifest,
        "notes": []
    }))
}

#[tauri::command]
fn core_upstream_rag_rollback(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let root = upstream_rag_mobile_root(&app)?;
    let stock_code = payload
        .get("stock_code")
        .and_then(Value::as_str)
        .ok_or_else(|| "缺少 stock_code。".to_string())?;
    let stock_dir = root.join(sanitize_path_part(stock_code));
    let current_manifest_path = stock_dir.join("current_manifest.json");
    let previous_manifest_path = stock_dir.join("previous_manifest.json");
    if !previous_manifest_path.exists() {
        return Err("没有可回滚的上一个 RAG 包。".to_string());
    }
    let current_bytes = fs::read(&current_manifest_path).ok();
    let previous_bytes = fs::read(&previous_manifest_path)
        .map_err(|error| format!("读取回滚 manifest 失败：{error}"))?;
    fs::write(&current_manifest_path, previous_bytes)
        .map_err(|error| format!("恢复 current manifest 失败：{error}"))?;
    if let Some(bytes) = current_bytes {
        fs::write(&previous_manifest_path, bytes)
            .map_err(|error| format!("更新 previous manifest 失败：{error}"))?;
    }
    let manifest = read_json_file(&current_manifest_path)?;
    Ok(json!({
        "rolled_back": true,
        "manifest": manifest,
        "notes": ["已切换到上一个 RAG 包。"]
    }))
}

#[tauri::command]
#[allow(deprecated)]
fn open_external_url(app: AppHandle, url: String) -> Result<(), String> {
    let trimmed = url.trim();
    let parsed = reqwest::Url::parse(trimmed).map_err(|_| "来源链接格式不正确".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("只允许打开 http/https 来源链接".to_string());
    }
    app.shell()
        .open(parsed.as_str().to_string(), None)
        .map_err(|error| error.to_string())
}

struct TencentRefreshResult {
    dataset: Value,
    requested: usize,
    fetched: usize,
    preserved: usize,
    failed_batches: usize,
    empty_batches: usize,
    error_samples: Vec<String>,
    stopped_early: bool,
    stop_reason: Option<String>,
    batch_start: usize,
    batch_count: usize,
    next_batch_start: usize,
    total_batches: usize,
    done: bool,
    processed_codes: usize,
    total_candidates: usize,
}

struct TencentQuotePayload {
    text: String,
    byte_len: usize,
    status: u16,
    transport: &'static str,
}

fn emit_market_refresh_log(app: &tauri::AppHandle, stage: &str, tone: &str, payload: Value) {
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

async fn refresh_tencent_market_data(
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

fn ingest_tencent_market_data(
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

fn append_preserved_seed_stocks(
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

fn append_all_preserved_seed_stocks(
    seed_stocks: &HashMap<String, serde_json::Map<String, Value>>,
    enriched_stocks: &HashMap<String, serde_json::Map<String, Value>>,
    stocks: &mut Vec<Value>,
    seen: &mut HashSet<String>,
) -> usize {
    let mut codes: Vec<String> = seed_stocks.keys().cloned().collect();
    codes.sort();
    append_preserved_seed_stocks(&codes, enriched_stocks, stocks, seen)
}

fn enriched_stock_maps(
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

fn merge_stock_financial_fields(
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

fn filtered_financial_snapshot_map(
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

fn merge_financials_object(target: &mut serde_json::Map<String, Value>, value: Option<&Value>) {
    let Some(object) = value.and_then(Value::as_object) else {
        return;
    };
    for (raw_code, item) in object {
        merge_financial_entry(target, raw_code, item);
    }
}

fn merge_financials_array(target: &mut serde_json::Map<String, Value>, value: Option<&Value>) {
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

fn merge_observe_financial_snapshot(data: &mut Value, code: &str, snapshot: &Value) -> bool {
    let mut entries = serde_json::Map::new();
    if let Some(existing) = data
        .get("financials")
        .and_then(Value::as_object)
        .and_then(|financials| financials.get(code))
        .cloned()
    {
        entries.insert(code.to_string(), existing);
    }
    merge_financials_object(&mut entries, snapshot.get("financials"));
    merge_financials_array(&mut entries, snapshot.get("stocks"));
    let Some(next) = entries.remove(code) else {
        return false;
    };
    let previous = data
        .get("financials")
        .and_then(Value::as_object)
        .and_then(|financials| financials.get(code))
        .cloned();
    if previous.as_ref() == Some(&next) {
        return false;
    }
    let entry = financial_entry_mut(data, code);
    if let Some(next_object) = next.as_object() {
        for (key, value) in next_object {
            entry.insert(key.clone(), value.clone());
        }
    }
    true
}
fn merge_financial_entry(
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

fn normalize_quarterly_eps(value: Option<&Value>) -> Vec<Value> {
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

fn valid_financial_period_key(period: &str) -> bool {
    let bytes = period.as_bytes();
    bytes.len() == 6
        && bytes[0..4].iter().all(|byte| byte.is_ascii_digit())
        && bytes[4].eq_ignore_ascii_case(&b'Q')
        && matches!(bytes[5], b'1'..=b'4')
}

fn finite_object_number_any(
    object: &serde_json::Map<String, Value>,
    fields: &[&str],
) -> Option<f64> {
    fields
        .iter()
        .find_map(|field| finite_object_number(object, field))
}

fn object_string_any(object: &serde_json::Map<String, Value>, fields: &[&str]) -> Option<String> {
    fields.iter().find_map(|field| object_string(object, field))
}
fn finite_object_number(object: &serde_json::Map<String, Value>, field: &str) -> Option<f64> {
    object
        .get(field)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

fn candidate_batch_window(
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

async fn fetch_tencent_quotes(
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

fn parse_tencent_quotes(
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

fn seed_stock_maps(seed: &Value) -> (HashMap<String, serde_json::Map<String, Value>>, Vec<String>) {
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

fn build_candidate_codes(
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

fn append_tencent_candidate_codes(codes: &mut Vec<String>) {
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

fn append_interleaved_ranges(codes: &mut Vec<String>, ranges: &[(&str, u32, u32)]) {
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

fn dedupe_stock_codes(codes: &mut Vec<String>) {
    let mut seen = HashSet::new();
    codes.retain(|code| seen.insert(code.clone()));
}

fn tencent_symbol(code: &str) -> Option<String> {
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

fn normalize_stock_code(value: &str) -> Option<String> {
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

fn valid_digits(value: &str) -> bool {
    value.len() == 6 && value.chars().all(|ch| ch.is_ascii_digit())
}

fn infer_market(digits: &str) -> &'static str {
    if digits.starts_with('6') || digits.starts_with('9') || digits.starts_with('5') {
        "SH"
    } else if digits.starts_with('4') || digits.starts_with('8') {
        "BJ"
    } else {
        "SZ"
    }
}

fn board_label(code: &str) -> &'static str {
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

fn parse_number(value: Option<&&str>) -> Option<f64> {
    let raw = value?.trim();
    if raw.is_empty() || matches!(raw, "-" | "None" | "nan") {
        return None;
    }
    raw.parse::<f64>().ok().filter(|value| value.is_finite())
}

fn first_positive_number(values: &[Option<&&str>]) -> Option<f64> {
    values
        .iter()
        .find_map(|value| parse_number(*value).filter(|number| *number > 0.0))
}

fn estimate_roe(pe: Option<f64>, pb: Option<f64>) -> Option<f64> {
    let pe = pe.filter(|value| *value > 0.0)?;
    let pb = pb.filter(|value| *value > 0.0)?;
    Some(pb / pe)
}

fn object_f64(object: &serde_json::Map<String, Value>, key: &str) -> Option<f64> {
    object
        .get(key)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

fn cache_epoch_ms(value: Option<&Value>) -> Option<u128> {
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

fn parse_cache_datetime_epoch_ms(text: &str) -> Option<u128> {
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

fn parse_timezone_offset_seconds(text: &str) -> Option<i32> {
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
fn local_yyyymmdd_from_epoch_ms(epoch_ms: u128) -> Option<String> {
    let seconds = i128::try_from(epoch_ms / 1000).ok()? + 8 * 60 * 60;
    let days = seconds.div_euclid(86_400);
    let (year, month, day) = civil_from_days(days)?;
    Some(format!("{year:04}{month:02}{day:02}"))
}

fn market_quote_cache_stale(
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

fn expected_market_quote_date_from_epoch_ms(epoch_ms: u128) -> Option<String> {
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

fn weekday_from_days_since_epoch(days_since_epoch: i128) -> i128 {
    (days_since_epoch + 4).rem_euclid(7)
}

fn days_from_civil_epoch(year: i32, month: u32, day: u32) -> Option<i128> {
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
fn civil_from_days(days_since_epoch: i128) -> Option<(i32, u32, u32)> {
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

fn object_string(object: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    object.get(key).and_then(Value::as_str).map(str::to_string)
}

fn filter_seed_relations(seed: &Value, valid_codes: &HashSet<String>) -> Value {
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

fn filter_seed_histories(seed: &Value, valid_codes: &HashSet<String>) -> Value {
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

fn adaptive_release_validation_force_cold_start(payload: &Value) -> bool {
    payload
        .get(stringify!(internal_release_validation_cold_start))
        .and_then(Value::as_bool)
        == Some(true)
}

fn adaptive_release_force_cold_start_code(missing: &mut Vec<String>, required_codes: &[String]) {
    if let Some(code) = required_codes.first() {
        if !missing.contains(code) {
            missing.push(code.clone());
        }
    }
}

fn adaptive_history_progress_percent(completed: usize, total: usize) -> usize {
    if total == 0 {
        return 72;
    }
    let completed = completed.min(total);
    24 + ((completed * 48 + total - 1) / total).min(48)
}

async fn collect_adaptive_history_results<F, Fut, P>(
    fetch_codes: Vec<String>,
    history_start_date: String,
    timeout: Duration,
    concurrency: usize,
    fetcher: F,
    mut on_progress: P,
) -> AdaptiveHistoryFetchOutcome
where
    F: Fn(&str, &str) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<Vec<Value>, String>> + Send + 'static,
    P: FnMut(usize, usize) + Send,
{
    let total = fetch_codes.len();
    if total == 0 {
        return AdaptiveHistoryFetchOutcome {
            results: Vec::new(),
            timed_out: false,
        };
    }

    let fetcher = Arc::new(fetcher);
    let make_fetch = {
        let fetcher = Arc::clone(&fetcher);
        let history_start_date = history_start_date.clone();
        move |code: String| {
            let fetcher = Arc::clone(&fetcher);
            let history_start_date = history_start_date.clone();
            async move {
                let result = fetcher(&code, &history_start_date).await;
                (code, result)
            }
        }
    };
    let mut codes = fetch_codes.into_iter();
    let mut pending = FuturesUnordered::new();
    let concurrency = concurrency.max(1);
    for _ in 0..concurrency {
        if let Some(code) = codes.next() {
            pending.push(make_fetch(code));
        }
    }

    let deadline = Instant::now() + timeout;
    let mut results = Vec::with_capacity(total);
    let mut completed = 0usize;
    let mut timed_out = false;
    while completed < total {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            timed_out = true;
            break;
        }
        match tokio::time::timeout(remaining, pending.next()).await {
            Ok(Some(result)) => {
                completed += 1;
                on_progress(completed, total);
                results.push(result);
                if let Some(code) = codes.next() {
                    pending.push(make_fetch(code));
                }
            }
            Ok(None) => break,
            Err(_) => {
                timed_out = true;
                break;
            }
        }
    }

    AdaptiveHistoryFetchOutcome { results, timed_out }
}

async fn prepare_adaptive_screen(
    app: &tauri::AppHandle,
    payload: Value,
) -> Result<PreparedAdaptiveScreen, String> {
    let force_cold_start = adaptive_release_validation_force_cold_start(&payload);
    let data = cached_market_data_snapshot(app)?;
    let stock_override = screen_stock_override(app, &data, &payload)?;
    let mut request = adaptive_screen_request_from_payload(payload)?;
    emit_adaptive_screen_progress(
        app,
        request.run_id.as_deref(),
        "candidate_scan",
        8,
        "初选候选池",
    );
    let candidate_data = Arc::clone(&data);
    let candidate_stocks = stock_override.clone();
    let criteria = request.criteria.clone();
    let candidates = runtime::run_cpu_bound("adaptive_screen_candidates", move || {
        let universe = candidate_stocks
            .as_deref()
            .map(Vec::as_slice)
            .unwrap_or(candidate_data.stocks.as_slice());
        gp_core::adaptive_candidate_codes(
            universe,
            &criteria,
            ADAPTIVE_SCREEN_HISTORY_PREFETCH_LIMIT,
        )
    })
    .await?;
    emit_adaptive_screen_progress(
        app,
        request.run_id.as_deref(),
        "history_fetch",
        24,
        "补齐120日行情",
    );

    let required_codes = adaptive_required_history_codes(&candidates);
    let target_history_date = request
        .as_of_date
        .as_deref()
        .and_then(compact_date_key)
        .or_else(|| {
            let universe = stock_override
                .as_deref()
                .map(Vec::as_slice)
                .unwrap_or(data.stocks.as_slice());
            adaptive_quote_target_date(universe, &candidates)
        })
        .or_else(|| expected_market_quote_date_from_epoch_ms(epoch_millis()));
    let mut missing = adaptive_missing_history_codes(
        data.as_ref(),
        &required_codes,
        target_history_date.as_deref(),
    );
    if force_cold_start {
        adaptive_release_force_cold_start_code(&mut missing, &required_codes);
    }
    let cache_hit = missing.is_empty();

    let mut history_override = HashMap::new();
    let mut notes = Vec::new();
    let mut history_prefetch_timed_out = false;
    if missing.is_empty() {
        notes.push(format!(
            "自适应选股复用本地日线缓存：候选 {} 只、宽基指数 3 个。",
            candidates.len()
        ));
    } else {
        let fetch_codes = missing.clone();
        let history_start_date = adaptive_history_start_date();
        let progress_app = app.clone();
        let progress_run_id = request.run_id.clone();
        let fetch_outcome =
            runtime::with_heavy_network_permit("adaptive_screen_history_fetch", async move {
                let outcome =
                    collect_adaptive_history_results(
                        fetch_codes,
                        history_start_date,
                        Duration::from_secs(ADAPTIVE_SCREEN_HISTORY_PREFETCH_TIMEOUT_SECS),
                        ADAPTIVE_SCREEN_HISTORY_CONCURRENCY,
                        |code, start_date| {
                            let code = code.to_string();
                            let start_date = start_date.to_string();
                            async move {
                                fetch_observe_daily_history(&code, &start_date, "20501231").await
                            }
                        },
                        move |completed, total| {
                            let message = format!("补齐120日行情 {completed}/{total}");
                            emit_adaptive_screen_progress(
                                &progress_app,
                                progress_run_id.as_deref(),
                                "history_fetch",
                                adaptive_history_progress_percent(completed, total),
                                &message,
                            );
                        },
                    )
                    .await;
                Ok(outcome)
            })
            .await?;
        let AdaptiveHistoryFetchOutcome {
            results: fetches,
            timed_out,
        } = fetch_outcome;
        history_prefetch_timed_out = timed_out;
        if timed_out {
            notes.push(format!(
                "历史行情预取达到 {} 秒预算，使用已完成的行情继续计算。",
                ADAPTIVE_SCREEN_HISTORY_PREFETCH_TIMEOUT_SECS
            ));
        }
        let mut history_patch = serde_json::Map::new();
        let mut failed = 0usize;
        for (code, result) in fetches {
            match result {
                Ok(rows) if !rows.is_empty() => {
                    let rows = rows
                        .into_iter()
                        .rev()
                        .take(120)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect::<Vec<_>>();
                    let typed = serde_json::from_value::<Vec<gp_core::HistoryBar>>(Value::Array(
                        rows.clone(),
                    ))
                    .map_err(|error| {
                        format!("adaptive screen history parse failed for {code}: {error}")
                    })?;
                    history_patch.insert(code.clone(), Value::Array(rows));
                    history_override.insert(code, typed);
                }
                _ => failed += 1,
            }
        }
        if !history_patch.is_empty() {
            if let Err(error) = persist_market_data_patch_updates(
                app.clone(),
                json!({ "histories": history_patch }),
            )
            .await
            {
                notes.push(format!("行情已获取但写入本地缓存失败：{error}"));
            }
        }
        notes.push(format!(
            "自适应选股日线预取：需要 {} 个标的，补取成功 {} 个，失败 {} 个。",
            required_codes.len(),
            history_override.len(),
            failed
        ));
    }
    request.as_of_date =
        latest_adaptive_data_date(data.as_ref(), &history_override, &required_codes)
            .or(target_history_date);
    emit_adaptive_screen_progress(
        app,
        request.run_id.as_deref(),
        "history_fetch",
        72,
        if history_prefetch_timed_out {
            "历史行情预取已达时限，使用已完成数据继续"
        } else {
            "历史行情准备完成"
        },
    );

    let exposure_app = app.clone();
    let exposure_date = request.as_of_date.clone();
    let recent_exposure = runtime::run_io_bound("adaptive_screen_exposure_read", move || {
        adaptive_exposure_recent_sync(&exposure_app, exposure_date.as_deref())
    })
    .await??;
    Ok(PreparedAdaptiveScreen {
        data,
        stock_override,
        candidate_codes: candidates,
        history_override,
        request,
        recent_exposure,
        notes,
        cache_hit,
    })
}

fn adaptive_history_start_date() -> String {
    const LOOKBACK_MILLIS: u128 = 220 * 24 * 60 * 60 * 1_000;
    local_yyyymmdd_from_epoch_ms(epoch_millis().saturating_sub(LOOKBACK_MILLIS))
        .unwrap_or_else(|| "20200101".to_string())
}

fn adaptive_history_window(
    rows: &[gp_core::HistoryBar],
    as_of_date: Option<&str>,
) -> Vec<gp_core::HistoryBar> {
    let target = match as_of_date {
        Some(value) => match compact_date_key(value) {
            Some(target) => Some(target),
            None => return Vec::new(),
        },
        None => None,
    };
    let window = rows
        .iter()
        .filter(|bar| {
            target.as_ref().is_none_or(|target| {
                compact_date_key(&bar.date).is_some_and(|date| date.as_str() <= target.as_str())
            })
        })
        .rev()
        .take(120)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>();
    if target.is_some() && window.last().and_then(|bar| compact_date_key(&bar.date)) != target {
        return Vec::new();
    }
    window
}

fn adaptive_point_in_time_universe(
    universe: &[gp_core::StockItem],
    histories: &HashMap<String, Vec<gp_core::HistoryBar>>,
    as_of_date: Option<&str>,
) -> Vec<gp_core::StockItem> {
    let target = as_of_date.and_then(compact_date_key);
    universe
        .iter()
        .map(|stock| {
            let mut point_in_time = stock.clone();
            let quote_matches_target = target.as_ref().is_some_and(|target| {
                stock
                    .quote_time
                    .as_deref()
                    .and_then(compact_date_key)
                    .is_some_and(|quote_date| quote_date == *target)
            });
            let bars = histories.get(&stock.code);
            if let Some(latest) = bars.and_then(|rows| rows.last()) {
                point_in_time.price = latest.close;
                point_in_time.volume = latest.volume;
                point_in_time.change_pct = bars.and_then(|rows| {
                    let previous = rows.get(rows.len().checked_sub(2)?)?.close;
                    (previous.is_finite() && previous > 0.0 && latest.close.is_finite())
                        .then_some(latest.close / previous - 1.0)
                });
            } else if target.is_some() && !quote_matches_target {
                point_in_time.price = 0.0;
                point_in_time.change_pct = None;
                point_in_time.volume = None;
            }
            if !quote_matches_target {
                // These quote fields cannot be reconstructed reliably from OHLCV. Keeping a
                // newer snapshot would leak future liquidity and overheat information.
                point_in_time.amount = None;
                point_in_time.turnover_rate = None;
                point_in_time.volume_ratio = None;
            }
            point_in_time.quote_time = target.clone();
            point_in_time
        })
        .collect()
}

fn latest_adaptive_data_date(
    data: &gp_core::CoreDataSet,
    history_override: &HashMap<String, Vec<gp_core::HistoryBar>>,
    required_codes: &[String],
) -> Option<String> {
    required_codes
        .iter()
        .filter_map(|code| {
            history_override
                .get(code)
                .or_else(|| data.histories.get(code))
                .and_then(|bars| {
                    bars.iter()
                        .filter_map(|bar| compact_date_key(&bar.date))
                        .max()
                })
        })
        .min()
}

fn adaptive_quote_target_date(
    universe: &[gp_core::StockItem],
    candidate_codes: &[String],
) -> Option<String> {
    let candidates = candidate_codes
        .iter()
        .map(|code| code.to_ascii_uppercase())
        .collect::<HashSet<_>>();
    universe
        .iter()
        .filter(|stock| candidates.contains(&stock.code.to_ascii_uppercase()))
        .filter_map(|stock| stock.quote_time.as_deref())
        .filter_map(compact_date_key)
        .max()
}

fn adaptive_history_cache_is_usable(
    data: &gp_core::CoreDataSet,
    code: &str,
    target_date: Option<&str>,
    min_bars: usize,
) -> bool {
    if !typed_history_cache_has_bars(data, code, "20200101", "20501231", min_bars) {
        return false;
    }
    let normalized = normalize_stock_code(code).unwrap_or_else(|| code.to_string());
    let latest = data
        .histories
        .get(code)
        .or_else(|| data.histories.get(&normalized))
        .into_iter()
        .flatten()
        .filter_map(|bar| compact_date_key(&bar.date))
        .max();
    match (latest, target_date) {
        (Some(latest), Some(target)) => latest.as_str() >= target,
        (Some(_), None) => true,
        _ => false,
    }
}

async fn prepare_trend_screen(
    app: &tauri::AppHandle,
    payload: Value,
) -> Result<PreparedTrendScreen, String> {
    let data = cached_market_data_snapshot(app)?;
    let stock_override = screen_stock_override(app, &data, &payload)?;
    let request = serde_json::from_value::<gp_core::TrendScreenRequest>(
        strip_core_side_payload_fields(payload),
    )
    .map_err(|error| format!("invalid trend screen request: {error}"))?;
    let criteria = request.criteria.clone();
    let candidate_data = Arc::clone(&data);
    let candidate_stocks = stock_override.clone();
    let candidate_result = runtime::run_cpu_bound("api_trend_screen_candidates", move || {
        let universe = candidate_stocks
            .as_deref()
            .map(Vec::as_slice)
            .unwrap_or(candidate_data.stocks.as_slice());
        gp_core::screen_stocks(universe, &criteria)
    })
    .await?;
    let candidates = trend_history_prefetch_codes_from_result(&candidate_result, request.limit);
    let missing = candidates
        .iter()
        .filter(|code| {
            !typed_history_cache_has_bars(
                data.as_ref(),
                code,
                &request.start_date,
                &request.end_date,
                MIN_TREND_SCREEN_HISTORY_BARS,
            )
        })
        .take(TREND_SCREEN_HISTORY_PREFETCH_LIMIT)
        .cloned()
        .collect::<Vec<_>>();
    let mut notes = Vec::new();
    if missing.is_empty() {
        notes.push(format!(
            "Trend screen reused cached OHLCV history for {} candidates.",
            candidates.len()
        ));
        return Ok(PreparedTrendScreen {
            data,
            stock_override,
            history_override: HashMap::new(),
            request,
            notes,
        });
    }

    let start_date = request.start_date.clone();
    let end_date = request.end_date.clone();
    let fetch_missing = missing.clone();
    let fetches =
        runtime::with_heavy_network_permit("api_trend_screen_history_fetch", async move {
            let results = stream::iter(fetch_missing)
                .map(|code| {
                    let start_date = start_date.clone();
                    let end_date = end_date.clone();
                    async move {
                        let result =
                            fetch_observe_daily_history(&code, &start_date, &end_date).await;
                        (code, result)
                    }
                })
                .buffer_unordered(TREND_SCREEN_HISTORY_CONCURRENCY)
                .collect::<Vec<_>>()
                .await;
            Ok(results)
        })
        .await?;

    let mut history_override = HashMap::new();
    let mut history_patch = serde_json::Map::new();
    let mut fetched = 0usize;
    let mut failed = 0usize;
    for (code, result) in fetches {
        match result {
            Ok(rows) if !rows.is_empty() => {
                let typed =
                    serde_json::from_value::<Vec<gp_core::HistoryBar>>(Value::Array(rows.clone()))
                        .map_err(|error| {
                            format!("trend history parse failed for {code}: {error}")
                        })?;
                history_patch.insert(code.clone(), Value::Array(rows));
                history_override.insert(code, typed);
                fetched += 1;
            }
            _ => failed += 1,
        }
    }
    if !history_patch.is_empty() {
        let patch = json!({ "histories": history_patch });
        if let Err(error) = persist_market_data_patch_updates(app.clone(), patch).await {
            notes.push(format!(
                "Trend screen fetched OHLCV for {fetched} candidates, but cache patch write failed: {error}"
            ));
        }
    }
    notes.push(format!(
        "Trend screen OHLCV prefetch: candidates {}, missing {}, fetched {fetched}, failed {failed}. Algorithm uses MA/KDJ/MACD/SWL/SWS, quant score, volume-price heat, and quality overlays.",
        candidates.len(),
        missing.len()
    ));
    Ok(PreparedTrendScreen {
        data,
        stock_override,
        history_override,
        request,
        notes,
    })
}

async fn prepare_backtest(
    app: &tauri::AppHandle,
    payload: Value,
) -> Result<PreparedBacktest, String> {
    let data = cached_market_data_snapshot(app)?;
    let stock_override = screen_stock_override(app, &data, &payload)?;
    let request =
        serde_json::from_value::<gp_core::BacktestRequest>(strip_core_side_payload_fields(payload))
            .map_err(|error| format!("invalid backtest request: {error}"))?;
    let adaptive_backtest = request
        .strategy_mode
        .trim()
        .to_ascii_lowercase()
        .starts_with("adaptive_swing_v1");

    if request
        .strategy_mode
        .trim()
        .eq_ignore_ascii_case("walk_forward")
    {
        return Ok(PreparedBacktest {
            data,
            stock_override,
            history_override: HashMap::new(),
            request,
            notes: Vec::new(),
        });
    }

    let mut candidates = if adaptive_backtest {
        let candidate_data = Arc::clone(&data);
        let candidate_stocks = stock_override.clone();
        let candidate_request = request.clone();
        runtime::run_cpu_bound("api_backtest_requirements", move || {
            backtest_history_requirements(
                candidate_data.as_ref(),
                candidate_stocks.as_deref().map(Vec::as_slice),
                &candidate_request,
            )
        })
        .await??
    } else {
        let candidate_data = Arc::clone(&data);
        let candidate_stocks = stock_override.clone();
        let candidate_request = request.clone();
        runtime::run_cpu_bound("api_backtest_candidates", move || {
            let universe = candidate_stocks
                .as_deref()
                .map(Vec::as_slice)
                .unwrap_or(candidate_data.stocks.as_slice());
            backtest_history_prefetch_codes(universe, &candidate_request)
        })
        .await?
    };
    if adaptive_backtest {
        candidates.extend(adaptive_benchmark_codes().into_iter().map(str::to_string));
        dedupe_stock_codes(&mut candidates);
    }
    let history_start_date = if adaptive_backtest {
        "19900101".to_string()
    } else {
        request.start_date.clone()
    };
    let minimum_bars = if adaptive_backtest {
        MIN_ADAPTIVE_SCREEN_HISTORY_BARS
    } else {
        MIN_BACKTEST_HISTORY_BARS
    };
    let missing = candidates
        .iter()
        .filter(|code| {
            !typed_history_cache_has_bars(
                data.as_ref(),
                code,
                &history_start_date,
                &request.end_date,
                minimum_bars,
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut notes = Vec::new();
    if missing.is_empty() {
        notes.push(format!(
            "已复用 {} 只入选股票的本地日线缓存。",
            candidates.len()
        ));
        return Ok(PreparedBacktest {
            data,
            stock_override,
            history_override: HashMap::new(),
            request,
            notes,
        });
    }

    let start_date = history_start_date;
    let end_date = request.end_date.clone();
    let fetch_missing = missing.clone();
    let fetches = runtime::with_heavy_network_permit("api_backtest_history_fetch", async move {
        let results = stream::iter(fetch_missing)
            .map(|code| {
                let start_date = start_date.clone();
                let end_date = end_date.clone();
                async move {
                    let result = fetch_observe_daily_history(&code, &start_date, &end_date).await;
                    (code, result)
                }
            })
            .buffer_unordered(BACKTEST_HISTORY_CONCURRENCY)
            .collect::<Vec<_>>()
            .await;
        Ok(results)
    })
    .await?;

    let mut history_override = HashMap::new();
    let mut history_patch = serde_json::Map::new();
    let mut fetched = 0usize;
    let mut failed = 0usize;
    let mut failure_samples = Vec::new();
    for (code, result) in fetches {
        match result {
            Ok(rows) if backtest_history_rows_are_usable(&rows) => {
                let typed =
                    serde_json::from_value::<Vec<gp_core::HistoryBar>>(Value::Array(rows.clone()))
                        .map_err(|error| {
                            format!("backtest history parse failed for {code}: {error}")
                        })?;
                history_patch.insert(code.clone(), Value::Array(rows));
                history_override.insert(code, typed);
                fetched += 1;
            }
            Ok(rows) => {
                failed += 1;
                if failure_samples.len() < 3 {
                    failure_samples.push(format!(
                        "{code}: daily-history sources returned {} rows; at least {MIN_BACKTEST_HISTORY_BARS} are required",
                        rows.len()
                    ));
                }
            }
            Err(error) => {
                failed += 1;
                if failure_samples.len() < 3 {
                    failure_samples.push(format!("{code}: {error}"));
                }
            }
        }
    }
    if fetched == 0 && !missing.is_empty() {
        return Err(format!(
            "无法获取回测所需的历史日线（{} 只）：{}",
            missing.len(),
            failure_samples.join("；")
        ));
    }
    if !history_patch.is_empty() {
        let patch = json!({ "histories": history_patch });
        if let Err(error) = persist_market_data_patch_updates(app.clone(), patch).await {
            notes.push(format!(
                "Backtest fetched daily history for {fetched} stocks, but cache patch write failed: {error}"
            ));
        }
    }
    let fetched_bars = history_override.values().map(Vec::len).sum::<usize>();
    notes.push(format!(
        "回测数据准备：入选 {} 只，需联网补取 {} 只，成功 {fetched} 只（共 {fetched_bars} 根日线），失败 {failed} 只。",
        candidates.len(),
        missing.len()
    ));
    Ok(PreparedBacktest {
        data,
        stock_override,
        history_override,
        request,
        notes,
    })
}

fn backtest_history_prefetch_codes(
    universe: &[gp_core::StockItem],
    request: &gp_core::BacktestRequest,
) -> Vec<String> {
    gp_core::backtest_selected_symbols(universe, request)
}

fn backtest_history_requirements(
    data: &gp_core::CoreDataSet,
    stocks: Option<&[gp_core::StockItem]>,
    request: &gp_core::BacktestRequest,
) -> Result<Vec<String>, String> {
    let source = gp_core::StaticDataSource::with_overrides(data, stocks, None);
    gp_core::backtest_required_history_symbols(&source, request).map_err(|error| error.to_string())
}

fn typed_history_cache_has_bars(
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

fn backtest_history_rows_are_usable(rows: &[Value]) -> bool {
    let mut dates = HashSet::new();
    rows.iter()
        .filter_map(|row| {
            let close = json_f64(row.get("close"))?;
            (close > 0.0)
                .then(|| row.get("date").and_then(Value::as_str))
                .flatten()
                .and_then(compact_date_key)
        })
        .filter(|date| dates.insert(date.clone()))
        .take(MIN_BACKTEST_HISTORY_BARS)
        .count()
        >= MIN_BACKTEST_HISTORY_BARS
}

fn trend_history_prefetch_codes_from_result(
    candidate_result: &gp_core::ScreenResult,
    limit: usize,
) -> Vec<String> {
    let pool_size = TREND_SCREEN_HISTORY_PREFETCH_LIMIT
        .max(limit.saturating_mul(5))
        .max(50)
        .min(200);
    normalize_trend_prefetch_codes(
        candidate_result
            .items
            .iter()
            .take(pool_size)
            .map(|item| item.stock.code.as_str()),
    )
}

fn normalize_trend_prefetch_codes<'a>(codes: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for code in codes {
        if let Some(code) = normalize_stock_code(code) {
            if seen.insert(code.clone()) {
                normalized.push(code);
            }
        }
    }
    normalized
}

#[cfg(test)]
fn trend_history_prefetch_codes(candidate_result: &Value, limit: usize) -> Vec<String> {
    let pool_size = TREND_SCREEN_HISTORY_PREFETCH_LIMIT
        .max(limit.saturating_mul(5))
        .max(50)
        .min(200);
    normalize_trend_prefetch_codes(
        candidate_result
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(pool_size)
            .filter_map(|item| {
                item.get("stock")
                    .and_then(|stock| stock.get("code"))
                    .and_then(Value::as_str)
            }),
    )
}

fn append_result_notes(result: &mut Value, extra_notes: Vec<String>) {
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

async fn observe_core_payload_with_cached_history(
    app: &tauri::AppHandle,
    payload: Value,
) -> Result<(Value, Vec<String>), String> {
    let mut data = cached_market_data(app)?;
    let code = payload
        .get("code")
        .and_then(Value::as_str)
        .and_then(normalize_stock_code)
        .ok_or_else(|| "观察请求缺少有效股票代码。".to_string())?;
    let start_date = payload
        .get("start_date")
        .and_then(Value::as_str)
        .unwrap_or("20200101")
        .to_string();
    let end_date = payload
        .get("end_date")
        .and_then(Value::as_str)
        .unwrap_or("20501231")
        .to_string();
    let mobile_fast_observe = payload
        .get("mobile_fast_observe")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut notes = Vec::new();
    let mut data_changed = false;

    if let Some(snapshot) = payload.get("financial_snapshot") {
        let before = financial_quarterly_eps_count(&data, &code);
        if merge_observe_financial_snapshot(&mut data, &code, snapshot) {
            data_changed = true;
            let after = financial_quarterly_eps_count(&data, &code);
            if after > before {
                notes.push(format!(
                    "观察页已合并移动端内置财务快照：{code} 新增 {} 期 EPS，当前 {after} 期。",
                    after.saturating_sub(before)
                ));
            } else {
                notes.push(format!("观察页已合并移动端内置财务快照：{code}。"));
            }
        }
    }

    let payload_financial_points = normalize_quarterly_eps(payload.get("financial_eps_points"));
    if !payload_financial_points.is_empty() {
        let before = financial_quarterly_eps_count(&data, &code);
        let provided_count = payload_financial_points.len();
        merge_quarterly_eps_points(&mut data, &code, payload_financial_points);
        let after = financial_quarterly_eps_count(&data, &code);
        if after > before {
            data_changed = true;
            notes.push(format!(
                "季度 EPS 已通过 WebView 财报源预取并写入本地缓存：{code} 新增 {} 期，当前 {after} 期。",
                after.saturating_sub(before)
            ));
        } else if provided_count > 0 {
            notes.push(format!(
                "季度 EPS WebView 财报源返回 {provided_count} 期，均已存在于本地缓存。"
            ));
        }
    }
    if let Some(extra_notes) = payload.get("financial_eps_notes").and_then(Value::as_array) {
        for note in extra_notes.iter().filter_map(Value::as_str).take(4) {
            if !note.trim().is_empty() {
                notes.push(format!(
                    "WebView 财报 EPS：{}",
                    truncate_for_note(note.trim(), 240)
                ));
            }
        }
    }

    // Financial: run the local snapshot merge first, then decide whether an online EPS fetch
    // is still needed — so the network part can run concurrently with capital/history below.
    let financial_cached_count;
    let financial_current_count;
    let need_eps;
    if mobile_fast_observe {
        if merge_basic_financial_from_stock(&mut data, &code) {
            data_changed = true;
        }
        notes
            .push("移动端快速观察：已使用本地财务快照，跳过同花顺/新浪在线 EPS 补全。".to_string());
        financial_cached_count = 0;
        financial_current_count = 0;
        need_eps = false;
    } else {
        financial_cached_count = financial_quarterly_eps_count(&data, &code);
        if merge_basic_financial_from_stock(&mut data, &code) {
            data_changed = true;
        }
        financial_current_count = financial_quarterly_eps_count(&data, &code);
        need_eps = financial_current_count < COMPLETE_QUARTERLY_EPS_POINTS;
    }

    // History: decide whether an online daily-history fetch is needed (WebView-provided rows and
    // a sufficiently stocked local cache both make it unnecessary).
    let requested_series_limit = payload_usize_field(
        &payload,
        "series_limit",
        120,
        20,
        OBSERVE_DAILY_HISTORY_LIMIT,
    );
    let required_cached_history_bars = if requested_series_limit > 500 {
        MIN_FULL_OBSERVE_HISTORY_BARS
    } else {
        MIN_OBSERVE_HISTORY_BARS
    };
    let webview_history_rows = payload_history_rows(&payload);
    let cache_lacks_history = webview_history_rows.is_none()
        && !history_cache_has_bars(
            &data,
            &code,
            &start_date,
            &end_date,
            required_cached_history_bars,
        );
    let need_history =
        cache_lacks_history && (!mobile_fast_observe || requested_series_limit > 500);
    let need_exact_share_refresh = observe_needs_exact_share_refresh(&data, &code);
    let quote_seed_stock = stock_object(&data, &code).cloned();
    let stock_price = quote_seed_stock
        .as_ref()
        .and_then(|stock| object_f64(stock, "price"));
    let need_fundamental_supplement = observe_needs_fundamental_supplement(&data, &code);

    // Capital evidence, online EPS, and daily history are independent network groups — fetch them
    // concurrently, then merge each result into `data` sequentially below.
    let capital_fetch_timeout = if mobile_fast_observe {
        12
    } else {
        OBSERVE_CAPITAL_TOTAL_TIMEOUT_SECS
    };
    let (capital_outcome, eps_outcome, history_outcome, quote_outcome, supplement_outcome) = futures::join!(
        tokio::time::timeout(
            Duration::from_secs(capital_fetch_timeout),
            fetch_observe_capital_evidence_items(&code, &start_date, &end_date, Some(&payload)),
        ),
        async {
            if need_eps {
                Some(
                    tokio::time::timeout(
                        Duration::from_secs(OBSERVE_FINANCIAL_TOTAL_TIMEOUT_SECS),
                        fetch_quarterly_eps_chain(&code),
                    )
                    .await,
                )
            } else {
                None
            }
        },
        async {
            if need_history {
                Some(
                    tokio::time::timeout(
                        Duration::from_secs(OBSERVE_HISTORY_TOTAL_TIMEOUT_SECS),
                        fetch_observe_daily_history(&code, &start_date, &end_date),
                    )
                    .await,
                )
            } else {
                None
            }
        },
        async {
            if need_exact_share_refresh {
                Some(
                    tokio::time::timeout(
                        Duration::from_secs(TENCENT_BATCH_TIMEOUT_SECS),
                        fetch_observe_quote_snapshot(&code, quote_seed_stock, &payload),
                    )
                    .await,
                )
            } else {
                None
            }
        },
        async {
            if need_fundamental_supplement {
                Some(
                    tokio::time::timeout(
                        Duration::from_secs(OBSERVE_FUNDAMENTAL_TOTAL_TIMEOUT_SECS),
                        fetch_observe_fundamental_supplement(&code, stock_price, &payload),
                    )
                    .await,
                )
            } else {
                None
            }
        },
    );

    if need_exact_share_refresh {
        match quote_outcome {
            Some(Ok(Ok(quote))) => {
                if merge_observe_quote_snapshot(&mut data, &code, &quote) {
                    data_changed = true;
                    notes.push(format!("已从腾讯实时行情补全 {code} 的总股本和流通股。"));
                }
            }
            Some(Ok(Err(error))) => notes.push(format!("精确股本补全失败：{error}")),
            Some(Err(_)) => notes.push(format!(
                "精确股本补全超过 {TENCENT_BATCH_TIMEOUT_SECS} 秒，保留本地缓存。"
            )),
            None => {}
        }
    }

    if need_fundamental_supplement {
        match supplement_outcome {
            Some(Ok(Ok((fields, supplement_notes)))) => {
                if merge_observe_fundamental_supplement(&mut data, &code, fields) {
                    data_changed = true;
                    notes.push(format!(
                        "已从东方财富公开数据补全 {code} 的商誉、质押和分红指标。"
                    ));
                }
                notes.extend(
                    supplement_notes
                        .into_iter()
                        .map(|note| format!("专项基本面补全：{note}")),
                );
            }
            Some(Ok(Err(error))) => notes.push(format!("专项基本面补全失败：{error}")),
            Some(Err(_)) => notes.push(format!(
                "专项基本面补全超过 {OBSERVE_FUNDAMENTAL_TOTAL_TIMEOUT_SECS} 秒，保留本地财报快照。"
            )),
            None => {}
        }
    }

    // Merge capital evidence.
    match capital_outcome {
        Ok((items, capital_notes)) => {
            if !items.is_empty() {
                data_changed |= merge_capital_evidence_items(&mut data, &code, items, &end_date);
            }
            notes.extend(capital_notes);
            if mobile_fast_observe {
                notes.push(
                    "Android short source capital evidence attempted with mobile proxy settings."
                        .to_string(),
                );
            }
        }
        Err(_) => {
            data_changed |= merge_capital_evidence_items(
                &mut data,
                &code,
                vec![
                    guba_status_item(
                        &code,
                        &end_date,
                        "东方财富股吧请求超时，未取得社区情绪证据。",
                    ),
                    eastmoney_lhb_unavailable_item(
                        &code,
                        &start_date,
                        &end_date,
                        "东方财富龙虎榜机构统计请求超时。",
                    ),
                    eastmoney_fund_flow_unavailable_item(
                        &code,
                        &end_date,
                        "东方财富当日主力资金请求超时。",
                    ),
                ],
                &end_date,
            );
            notes.push(format!(
                "综合资金证据联网补全超过 {capital_fetch_timeout} 秒，已写入超时状态证据。"
            ));
        }
    }

    // Merge online EPS (financial).
    if need_eps {
        let fetch = match eps_outcome {
            Some(Ok(fetch)) => fetch,
            _ => QuarterlyEpsFetchResult {
                points: Vec::new(),
                sources: Vec::new(),
                errors: vec![format!(
                    "在线财报补全超过 {OBSERVE_FINANCIAL_TOTAL_TIMEOUT_SECS} 秒，已跳过同花顺/新浪补充。"
                )],
            },
        };
        if !fetch.points.is_empty() {
            let before = financial_quarterly_eps_count(&data, &code);
            merge_quarterly_eps_points(&mut data, &code, fetch.points);
            let after = financial_quarterly_eps_count(&data, &code);
            if after > before {
                data_changed = true;
            }
            let sources = if fetch.sources.is_empty() {
                "在线财报源".to_string()
            } else {
                fetch.sources.join(" / ")
            };
            let note = format!(
                "季度 EPS 已按优先级补全：本地缓存 {financial_cached_count} 期，通达信基础财务 {financial_current_count} 期，{sources} 后当前 {after} 期。"
            );
            if push_financial_note(&mut data, &code, note.clone()) {
                data_changed = true;
            }
            notes.push(note);
        } else if financial_current_count < COMPLETE_QUARTERLY_EPS_POINTS {
            let detail = if fetch.errors.is_empty() {
                "在线财报源没有返回可用 EPS 行".to_string()
            } else {
                fetch.errors.join("；")
            };
            let note = format!(
                "季度 EPS 明细仍不足：本地缓存 {financial_cached_count} 期，通达信基础财务 {financial_current_count} 期；已尝试同花顺和新浪财经，{detail}。"
            );
            if push_financial_note(&mut data, &code, note.clone()) {
                data_changed = true;
            }
            notes.push(note);
        }
    }

    // Merge daily history.
    if let Some(rows) = webview_history_rows {
        let count = rows.len();
        insert_history_rows(&mut data, &code, rows);
        data_changed = true;
        notes.push(format!(
            "观察日线历史已通过 WebView 预取并写入本地缓存：{code} {count} 条。"
        ));
    } else if cache_lacks_history {
        if mobile_fast_observe {
            notes.push(
                "移动端快速观察：未命中 WebView 日线/本地历史时，不等待 Rust 在线日线补全。"
                    .to_string(),
            );
        } else {
            match history_outcome {
                Some(Ok(Ok(rows))) if !rows.is_empty() => {
                    let count = rows.len();
                    insert_history_rows(&mut data, &code, rows);
                    data_changed = true;
                    notes.push(format!(
                        "观察日线历史已按需联网更新并写入本地缓存：{code} {count} 条。"
                    ));
                }
                Some(Ok(Ok(_))) => {
                    notes.push(format!("观察日线历史为空：{code}。"));
                }
                Some(Ok(Err(error))) => {
                    notes.push(format!("观察日线历史拉取失败：{error}"));
                }
                Some(Err(_)) => {
                    notes.push(format!(
                        "观察日线历史拉取超过 {OBSERVE_HISTORY_TOTAL_TIMEOUT_SECS} 秒，已跳过在线补充。"
                    ));
                }
                None => {}
            }
        }
    }

    if data_changed {
        let persist_data = data.clone();
        if let Err(error) =
            persist_market_data_updates(app.clone(), persist_data, vec![code.clone()]).await
        {
            notes.push(format!("观察缓存补丁写入失败：{error}"));
        }
    }

    let mut observe_request = payload.clone();
    if let Some(map) = observe_request.as_object_mut() {
        map.remove("history");
        map.remove("financial_eps_points");
        map.remove("financial_eps_notes");
        map.remove("financial_snapshot");
        map.remove("mobile_fast_observe");
    }
    let mut request = serde_json::Map::new();
    request.insert("data".to_string(), data);
    request.insert("request".to_string(), observe_request);
    Ok((Value::Object(request), notes))
}
fn observe_core_payload_from_cache(
    app: &tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let data = cached_market_data(app)?;
    let mut observe_request = payload;
    if let Some(map) = observe_request.as_object_mut() {
        map.remove("history");
        map.remove("financial_eps_points");
        map.remove("financial_eps_notes");
        map.remove("financial_snapshot");
        map.remove("mobile_fast_observe");
    }
    let mut request = serde_json::Map::new();
    request.insert("data".to_string(), data);
    request.insert("request".to_string(), observe_request);
    Ok(Value::Object(request))
}

fn observe_error_result(
    core_payload: &Value,
    request_payload: &Value,
    notes: Vec<String>,
) -> Value {
    let code = request_payload
        .get("code")
        .and_then(Value::as_str)
        .and_then(normalize_stock_code)
        .unwrap_or_else(|| {
            request_payload
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        });
    let stock = core_payload
        .get("data")
        .and_then(|data| data.get("stocks"))
        .and_then(Value::as_array)
        .and_then(|stocks| {
            stocks.iter().find(|stock| {
                stock
                    .get("code")
                    .and_then(Value::as_str)
                    .and_then(normalize_stock_code)
                    .map(|stock_code| stock_code == code)
                    .unwrap_or(false)
            })
        })
        .cloned()
        .unwrap_or_else(|| json!({"code": code, "name": code, "industry": "", "price": null}));
    json!({
        "source": "tdx",
        "stock": stock,
        "financial_indicators": Value::Null,
        "trend": Value::Null,
        "capital_evidence": Value::Null,
        "order_book": Value::Null,
        "notes": notes,
    })
}
fn append_observe_note(result: &mut Value, note: String) {
    if let Some(notes) = result.get_mut("notes").and_then(Value::as_array_mut) {
        notes.push(Value::String(note));
    } else if let Some(object) = result.as_object_mut() {
        object.insert("notes".to_string(), Value::Array(vec![Value::String(note)]));
    }
}

async fn fetch_observe_capital_evidence_items(
    code: &str,
    start_date: &str,
    end_date: &str,
    network_payload: Option<&Value>,
) -> (Vec<Value>, Vec<String>) {
    let mut notes = Vec::new();
    let mut items = Vec::new();
    let timeout = Duration::from_secs(OBSERVE_CAPITAL_REQUEST_TIMEOUT_SECS);
    let client = match build_http_client_with_proxy(
        "Mozilla/5.0 GuXuanYou/0.3 capital evidence",
        timeout,
        network_payload,
    ) {
        Ok(client) => client,
        Err(error) => {
            return (
                Vec::new(),
                vec![format!("综合资金证据 HTTP 客户端创建失败：{error}")],
            );
        }
    };

    // The three evidence sources are independent, so keep their network latency concurrent.
    let (fund_flow_fetch, guba_fetch, lhb_fetch) = futures::join!(
        fetch_eastmoney_main_fund_flow(&client, code, end_date),
        fetch_eastmoney_guba_sentiment(&client, code),
        fetch_eastmoney_institution_lhb(&client, code, start_date, end_date),
    );
    match fund_flow_fetch {
        Ok(item) => {
            let date = item.get("date").and_then(Value::as_str).unwrap_or(end_date);
            notes.push(format!(
                "当日主力资金已接入东方财富个股资金流：{code}，数据日 {date}。"
            ));
            items.push(item);
        }
        Err(error) => {
            let detail = format!(
                "东方财富当日主力资金抓取失败：{}",
                truncate_for_note(&error, 180)
            );
            items.push(eastmoney_fund_flow_unavailable_item(
                code, end_date, &detail,
            ));
            notes.push(detail);
        }
    }
    match guba_fetch {
        Ok(mut guba_items) if !guba_items.is_empty() => {
            let count = guba_items.len();
            items.append(&mut guba_items);
            notes.push(format!(
                "消息情绪已接入东方财富股吧：{code} 命中 {count} 条帖子，社区内容仅作情绪线索。"
            ));
        }
        Ok(_) => {
            items.push(guba_status_item(
                code,
                end_date,
                "东方财富股吧暂无可纳入评分的帖子。",
            ));
            notes.push(format!("东方财富股吧暂无可纳入评分的帖子：{code}。"));
        }
        Err(error) => {
            let detail = format!(
                "东方财富股吧情绪抓取失败：{}",
                truncate_for_note(&error, 180)
            );
            items.push(guba_status_item(code, end_date, &detail));
            notes.push(detail);
        }
    }

    match lhb_fetch {
        Ok(item) => {
            let title = item
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("东方财富龙虎榜机构席位");
            notes.push(format!("机构席位已接入东方财富龙虎榜机构统计：{title}。"));
            items.push(item);
        }
        Err(error) => {
            let detail = format!(
                "东方财富龙虎榜机构统计抓取失败：{}",
                truncate_for_note(&error, 180)
            );
            items.push(eastmoney_lhb_unavailable_item(
                code, start_date, end_date, &detail,
            ));
            notes.push(detail);
        }
    }

    (items, notes)
}

async fn fetch_eastmoney_main_fund_flow(
    client: &reqwest::Client,
    code: &str,
    end_date: &str,
) -> Result<Value, String> {
    let normalized =
        normalize_stock_code(code).ok_or_else(|| format!("无效资金流股票代码：{code}"))?;
    let digits = normalized
        .get(..6)
        .ok_or_else(|| format!("无效资金流股票代码：{code}"))?;
    let market = eastmoney_market_code(&normalized)
        .ok_or_else(|| format!("无法识别资金流股票代码：{code}"))?;
    let secid = format!("{market}.{digits}");
    let url = reqwest::Url::parse_with_params(
        EASTMONEY_FUND_FLOW_ENDPOINT,
        &[
            ("lmt", "20"),
            ("klt", "101"),
            ("secid", secid.as_str()),
            ("fields1", "f1,f2,f3,f7"),
            (
                "fields2",
                "f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61,f62,f63",
            ),
            ("ut", "7eea3edcaed734bea9cbfc24409ed989"),
        ],
    )
    .map_err(|error| error.to_string())?;
    let text = http_get_text_with_headers_first(
        client,
        &url.to_string(),
        OBSERVE_CAPITAL_REQUEST_TIMEOUT_SECS,
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        "https://data.eastmoney.com/zjlx/detail.html",
    )
    .await?;
    parse_eastmoney_main_fund_flow_item(&text, &normalized, end_date)
}

fn parse_eastmoney_main_fund_flow_item(
    text: &str,
    _code: &str,
    end_date: &str,
) -> Result<Value, String> {
    let value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let rows = value
        .get("data")
        .and_then(|data| data.get("klines"))
        .and_then(Value::as_array)
        .ok_or_else(|| "资金流接口没有返回日线数据".to_string())?;
    let requested_end = normalize_history_date(end_date);
    let latest = rows
        .iter()
        .filter_map(Value::as_str)
        .filter_map(parse_eastmoney_main_fund_flow_row)
        .filter(|row| {
            requested_end
                .as_deref()
                .map(|end| row.0.as_str() <= end)
                .unwrap_or(true)
        })
        .max_by(|left, right| left.0.cmp(&right.0))
        .ok_or_else(|| format!("资金流接口在 {end_date} 之前没有可用交易日数据"))?;
    let (trade_date, net_amount, net_ratio) = latest;
    let involvement = main_fund_involvement(net_ratio);
    let score = main_fund_flow_score(net_ratio);
    let conclusion = main_fund_flow_plain_conclusion(net_ratio);
    Ok(json!({
        "category": "fund_flow",
        "source": "东方财富个股资金流",
        "title": "当日主力资金流",
        "date": trade_date,
        "metrics": {
            "主力净流入额": format_amount_wan(net_amount),
            "主力净流入额原值": format!("{net_amount:.2}"),
            "主力净占比": format!("{net_ratio:.2}%"),
            "主力介入度": format!("{}（{:.2}%）", involvement, net_ratio.abs()),
            "介入度口径": "按主力净占比绝对值分档：低 <3%，中 3%-8%，高 >=8%",
            "通俗结论": conclusion,
            "证据类型": "外部个股资金流",
        },
        "sentiment": score_sentiment_label(score),
        "weight": 0.35,
        "confidence": "中",
        "url": "https://data.eastmoney.com/zjlx/detail.html",
        "score": round2_value(score),
        "note": "东方财富个股资金流最新交易日口径；主力介入度由主力净占比绝对值分档，高介入只表示主力交易影响较大，不代表方向利好。",
    }))
}

fn parse_eastmoney_main_fund_flow_row(raw: &str) -> Option<(String, f64, f64)> {
    let parts = raw.split(',').collect::<Vec<_>>();
    if parts.len() < 7 {
        return None;
    }
    Some((
        normalize_history_date(parts.first().copied()?)?,
        parse_f64_str(parts.get(1).copied()?)?,
        parse_f64_str(parts.get(6).copied()?)?,
    ))
}

fn main_fund_involvement(net_ratio: f64) -> &'static str {
    let magnitude = net_ratio.abs();
    if magnitude >= 8.0 {
        "高"
    } else if magnitude >= 3.0 {
        "中"
    } else {
        "低"
    }
}

fn main_fund_flow_score(net_ratio: f64) -> f64 {
    round2_value((50.0 + net_ratio.clamp(-16.0, 16.0) * 2.5).clamp(10.0, 90.0))
}

fn main_fund_flow_plain_conclusion(net_ratio: f64) -> String {
    let magnitude = net_ratio.abs();
    let direction = if net_ratio > 0.05 {
        "净买入"
    } else if net_ratio < -0.05 {
        "净卖出"
    } else {
        "净流入接近持平"
    };
    let strength = match main_fund_involvement(net_ratio) {
        "高" => "影响较大",
        "中" => "影响中等",
        _ => "影响有限",
    };
    if magnitude <= 0.05 {
        "主力净流入接近零，当天没有明确的资金方向。".to_string()
    } else {
        format!(
            "按成交占比看，每 100 元成交约有 {magnitude:.2} 元形成主力{direction}，当天主力交易对价格的{strength}。"
        )
    }
}

fn eastmoney_fund_flow_unavailable_item(code: &str, end_date: &str, detail: &str) -> Value {
    json!({
        "category": "fund_flow_status",
        "source": "东方财富个股资金流",
        "title": "当日主力资金流暂不可用",
        "date": normalize_history_date(end_date),
        "metrics": {
            "状态": "接口不可用",
            "查询截至": normalize_history_date(end_date).unwrap_or_else(|| end_date.to_string()),
            "失败原因": detail,
            "股票": code,
        },
        "sentiment": "uncertain",
        "weight": 0.35,
        "confidence": "低",
        "url": "https://data.eastmoney.com/zjlx/detail.html",
        "score": Value::Null,
        "note": "未取得真实主力资金流；不能用本地量价代理替代主力净流入额或净占比。",
    })
}

async fn fetch_eastmoney_guba_sentiment(
    client: &reqwest::Client,
    code: &str,
) -> Result<Vec<Value>, String> {
    let normalized =
        normalize_stock_code(code).ok_or_else(|| format!("无效股吧股票代码：{code}"))?;
    let digits = normalized
        .get(..6)
        .ok_or_else(|| format!("无效股吧股票代码：{code}"))?;
    let url = format!("https://guba.eastmoney.com/list,{digits}.html");
    let text = http_get_text_with_headers_first(
        client,
        &url,
        OBSERVE_CAPITAL_REQUEST_TIMEOUT_SECS,
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        "https://guba.eastmoney.com/",
    )
    .await?;
    Ok(parse_eastmoney_guba_items(&text, &normalized, &url))
}

fn parse_eastmoney_guba_items(html: &str, code: &str, list_url: &str) -> Vec<Value> {
    let Some(raw) = extract_json_after_var(html, "article_list") else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return Vec::new();
    };
    let Some(rows) = value.get("re").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut ranked_rows = rows.iter().collect::<Vec<_>>();
    ranked_rows.sort_by(|left, right| {
        guba_post_heat(right)
            .partial_cmp(&guba_post_heat(left))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    for row in ranked_rows {
        if items.len() >= OBSERVE_GUBA_MAX_POSTS {
            break;
        }
        let Some(object) = row.as_object() else {
            continue;
        };
        let title = object_string_any(object, &["post_title"])
            .map(|value| clean_html_text(&value))
            .unwrap_or_default();
        if title.trim().is_empty() || !seen.insert(title.clone()) {
            continue;
        }
        let summary = guba_post_summary(object, &title);
        let date = object_string_any(object, &["post_publish_time", "post_display_time"])
            .and_then(|value| normalize_guba_datetime(&value));
        let post_id = object_string_any(object, &["post_id"]).unwrap_or_default();
        let url = if post_id.trim().is_empty() {
            list_url.to_string()
        } else {
            let digits = code.get(..6).unwrap_or(code);
            format!(
                "https://guba.eastmoney.com/news,{digits},{}.html",
                post_id.trim()
            )
        };
        let score = guba_sentiment_score(&title, &summary, object);
        let metrics = json!({
            "标题": title,
            "评论数": object_number_any_loose(object, &["post_comment_count"]).map(compact_count).unwrap_or_else(|| "0".to_string()),
            "阅读数": object_number_any_loose(object, &["post_click_count"]).map(compact_count).unwrap_or_else(|| "0".to_string()),
            "多空标记": guba_bullish_bearish_label(object.get("bullish_bearish")),
            "证据分": format!("{score:.1}"),
        });
        items.push(json!({
            "category": "community_sentiment",
            "source": "东方财富股吧",
            "title": title,
            "date": date,
            "metrics": metrics,
            "sentiment": score_sentiment_label(score),
            "weight": 0.15,
            "confidence": "低",
            "url": url,
            "score": round2_value(score),
            "note": format!("东方财富股吧帖子：{}。社区讨论只作情绪/传闻信号，不直接作为买卖结论。", truncate_for_note(&summary, 120)),
        }));
    }
    items
}

fn guba_post_heat(value: &Value) -> f64 {
    let Some(object) = value.as_object() else {
        return 0.0;
    };
    let clicks = object_number_any_loose(object, &["post_click_count"]).unwrap_or(0.0);
    let comments = object_number_any_loose(object, &["post_comment_count"]).unwrap_or(0.0);
    let likes =
        object_number_any_loose(object, &["post_like_count", "post_forward_count"]).unwrap_or(0.0);
    clicks + comments * 25.0 + likes * 8.0
}

fn guba_post_summary(object: &serde_json::Map<String, Value>, title: &str) -> String {
    let mut parts = Vec::new();
    if let Some(nickname) = object_string_any(object, &["user_nickname"]) {
        let nickname = clean_html_text(&nickname);
        if !nickname.is_empty() {
            parts.push(format!("作者 {nickname}"));
        }
    }
    if let Some(clicks) = object_number_any_loose(object, &["post_click_count"]) {
        parts.push(format!("阅读 {}", compact_count(clicks)));
    }
    if let Some(comments) = object_number_any_loose(object, &["post_comment_count"]) {
        parts.push(format!("评论 {}", compact_count(comments)));
    }
    if parts.is_empty() {
        title.to_string()
    } else {
        parts.join("，")
    }
}

async fn fetch_eastmoney_institution_lhb(
    client: &reqwest::Client,
    code: &str,
    start_date: &str,
    end_date: &str,
) -> Result<Value, String> {
    let normalized =
        normalize_stock_code(code).ok_or_else(|| format!("无效龙虎榜股票代码：{code}"))?;
    let digits = normalized
        .get(..6)
        .ok_or_else(|| format!("无效龙虎榜股票代码：{code}"))?;
    let start = normalize_history_date(start_date).unwrap_or_else(|| fallback_lhb_start_date());
    let end = normalize_history_date(end_date).unwrap_or_else(|| fallback_today_date());
    let filter =
        format!("(TRADE_DATE>='{start}')(TRADE_DATE<='{end}')(SECURITY_CODE=\"{digits}\")");
    let url = reqwest::Url::parse_with_params(
        "https://datacenter-web.eastmoney.com/api/data/v1/get",
        &[
            ("sortColumns", "NET_BUY_AMT,TRADE_DATE,SECURITY_CODE"),
            ("sortTypes", "-1,-1,1"),
            ("pageSize", "50"),
            ("pageNumber", "1"),
            ("reportName", "RPT_ORGANIZATION_TRADE_DETAILS"),
            ("columns", "ALL"),
            ("source", "WEB"),
            ("client", "WEB"),
            ("filter", filter.as_str()),
        ],
    )
    .map_err(|error| error.to_string())?;
    let text = http_get_text_with_headers_first(
        client,
        &url.to_string(),
        OBSERVE_CAPITAL_REQUEST_TIMEOUT_SECS,
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        "https://data.eastmoney.com/stock/jgmmtj.html",
    )
    .await?;
    let mut item = parse_eastmoney_lhb_item(&text, &normalized, &start, &end)?;
    if item.get("category").and_then(Value::as_str) != Some("institution_lhb") {
        return Ok(item);
    }
    let Some(trade_date) = item
        .get("date")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
    else {
        return Ok(item);
    };
    let (buy_rows, sell_rows) = futures::join!(
        fetch_eastmoney_lhb_seat_side(client, digits, &trade_date, true),
        fetch_eastmoney_lhb_seat_side(client, digits, &trade_date, false),
    );
    let (seats, status, note) = match (buy_rows, sell_rows) {
        (Ok(buy), Ok(sell)) => (
            merge_eastmoney_lhb_seats(buy, sell),
            "complete",
            "营业部名称和买卖额来自公开龙虎榜；行为手法仅按当日榜单特征推断。".to_string(),
        ),
        (Ok(buy), Err(error)) => (
            merge_eastmoney_lhb_seats(buy, Vec::new()),
            "partial",
            format!("卖方席位明细暂不可用：{}", truncate_for_note(&error, 120)),
        ),
        (Err(error), Ok(sell)) => (
            merge_eastmoney_lhb_seats(Vec::new(), sell),
            "partial",
            format!("买方席位明细暂不可用：{}", truncate_for_note(&error, 120)),
        ),
        (Err(buy_error), Err(sell_error)) => (
            Vec::new(),
            "unavailable",
            format!(
                "席位明细暂不可用：买方 {}；卖方 {}",
                truncate_for_note(&buy_error, 80),
                truncate_for_note(&sell_error, 80)
            ),
        ),
    };
    if let Some(metrics) = item.get_mut("metrics").and_then(Value::as_object_mut) {
        metrics.insert("公开席位数".to_string(), json!(seats.len()));
    }
    if let Some(object) = item.as_object_mut() {
        object.insert("seats".to_string(), Value::Array(seats));
        object.insert("seat_detail_status".to_string(), json!(status));
        object.insert("seat_detail_note".to_string(), json!(note));
    }
    Ok(item)
}

fn parse_eastmoney_lhb_item(
    text: &str,
    code: &str,
    start_date: &str,
    end_date: &str,
) -> Result<Value, String> {
    let value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let rows = value
        .get("result")
        .and_then(|result| result.get("data"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let digits = code.get(..6).unwrap_or(code);
    let mut best: Option<&Value> = None;
    let mut best_abs = -1.0;
    for row in &rows {
        let Some(object) = row.as_object() else {
            continue;
        };
        let row_code = object_string_any(object, &["SECURITY_CODE"]);
        if row_code.as_deref() != Some(digits) {
            continue;
        }
        let net = object_number_any_loose(object, &["NET_BUY_AMT"])
            .unwrap_or(0.0)
            .abs();
        if net > best_abs {
            best_abs = net;
            best = Some(row);
        }
    }
    let Some(row) = best.and_then(Value::as_object) else {
        return Ok(eastmoney_lhb_no_hit_item(code, start_date, end_date));
    };
    let buy = object_number_any_loose(row, &["BUY_AMT"]).unwrap_or(0.0);
    let sell = object_number_any_loose(row, &["SELL_AMT"]).unwrap_or(0.0);
    let net = object_number_any_loose(row, &["NET_BUY_AMT"]).unwrap_or(buy - sell);
    let ratio = object_number_any_loose(row, &["RATIO"]);
    let score = institution_lhb_score(net, buy, sell, ratio);
    let trade_date =
        object_string_any(row, &["TRADE_DATE"]).and_then(|value| normalize_history_date(&value));
    let reason =
        object_string_any(row, &["EXPLANATION"]).unwrap_or_else(|| "龙虎榜机构统计".to_string());
    Ok(json!({
        "category": "institution_lhb",
        "source": "东方财富龙虎榜机构统计",
        "title": "东方财富龙虎榜机构席位",
        "date": trade_date,
        "metrics": {
            "机构买入额": format_amount_wan(buy),
            "机构卖出额": format_amount_wan(sell),
            "机构净买额": format_amount_wan(net),
            "机构买卖比": institution_buy_sell_ratio(buy, sell),
            "净买额占成交额比": ratio.map(|value| format!("{}%", format_number_like(value))).unwrap_or_else(|| "-".to_string()),
            "买方机构数": object_number_any_loose(row, &["BUY_TIMES", "BUY_COUNT"]).map(format_number_like).unwrap_or_else(|| "-".to_string()),
            "卖方机构数": object_number_any_loose(row, &["SELL_TIMES", "SELL_COUNT"]).map(format_number_like).unwrap_or_else(|| "-".to_string()),
            "上榜原因": reason,
            "证据分": format!("{score:.1}"),
        },
        "sentiment": score_sentiment_label(score),
        "weight": 0.25,
        "confidence": "高",
        "url": "https://data.eastmoney.com/stock/jgmmtj.html",
        "score": round2_value(score),
        "note": "东方财富龙虎榜机构买卖每日统计；口径为公开龙虎榜机构专用席位，不等同于全部机构持仓变化。",
    }))
}

#[derive(Clone, Debug, Default)]
struct EastmoneyLhbSeatRow {
    key: String,
    seat_code: Option<String>,
    name: String,
    trade_date: Option<String>,
    buy_amount: Option<f64>,
    sell_amount: Option<f64>,
    buy_ratio: Option<f64>,
    sell_ratio: Option<f64>,
    change_rate: Option<f64>,
    reason: Option<String>,
    three_day_rise_probability: Option<f64>,
    three_day_activity_count: Option<f64>,
}

async fn fetch_eastmoney_lhb_seat_side(
    client: &reqwest::Client,
    code: &str,
    trade_date: &str,
    buy_side: bool,
) -> Result<Vec<EastmoneyLhbSeatRow>, String> {
    let report_name = if buy_side {
        "RPT_BILLBOARD_DAILYDETAILSBUY"
    } else {
        "RPT_BILLBOARD_DAILYDETAILSSELL"
    };
    let sort_column = if buy_side { "BUY" } else { "SELL" };
    let filter = format!(r#"(TRADE_DATE='{trade_date}')(SECURITY_CODE="{code}")"#);
    let url = reqwest::Url::parse_with_params(
        EASTMONEY_DATACENTER_ENDPOINT,
        &[
            ("sortColumns", sort_column),
            ("sortTypes", "-1"),
            ("pageSize", "50"),
            ("pageNumber", "1"),
            ("reportName", report_name),
            ("columns", "ALL"),
            ("source", "WEB"),
            ("client", "WEB"),
            ("filter", filter.as_str()),
        ],
    )
    .map_err(|error| error.to_string())?;
    let text = http_get_text_with_headers_first(
        client,
        &url.to_string(),
        OBSERVE_LHB_SEAT_REQUEST_TIMEOUT_SECS,
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        "https://data.eastmoney.com/stock/tradedetail.html",
    )
    .await?;
    parse_eastmoney_lhb_seat_side(&text, code, buy_side)
}

fn parse_eastmoney_lhb_seat_side(
    text: &str,
    code: &str,
    buy_side: bool,
) -> Result<Vec<EastmoneyLhbSeatRow>, String> {
    let value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let rows = value
        .get("result")
        .and_then(|result| result.get("data"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut parsed = Vec::new();
    for row in rows {
        let Some(object) = row.as_object() else {
            continue;
        };
        if object_string_any(object, &["SECURITY_CODE"]).as_deref() != Some(code) {
            continue;
        }
        let Some(name) = object_string_any(object, &["OPERATEDEPT_NAME"]) else {
            continue;
        };
        let seat_code = object_string_any(object, &["OPERATEDEPT_CODE"]);
        let key = seat_code.clone().unwrap_or_else(|| name.clone());
        parsed.push(EastmoneyLhbSeatRow {
            key,
            seat_code,
            name,
            trade_date: object_string_any(object, &["TRADE_DATE"])
                .and_then(|value| normalize_history_date(&value)),
            buy_amount: buy_side
                .then(|| object_number_any_loose(object, &["BUY"]))
                .flatten(),
            sell_amount: (!buy_side)
                .then(|| object_number_any_loose(object, &["SELL"]))
                .flatten(),
            buy_ratio: buy_side
                .then(|| object_number_any_loose(object, &["TOTAL_BUYRIO", "TOTAL_BUY_RATIO"]))
                .flatten(),
            sell_ratio: (!buy_side)
                .then(|| object_number_any_loose(object, &["TOTAL_SELLRIO", "TOTAL_SELL_RATIO"]))
                .flatten(),
            change_rate: object_number_any_loose(object, &["CHANGE_RATE"]),
            reason: object_string_any(object, &["EXPLANATION"]),
            three_day_rise_probability: object_number_any_loose(object, &["RISE_PROBABILITY_3DAY"]),
            three_day_activity_count: object_number_any_loose(
                object,
                &["TOTAL_BUYER_SALESTIMES_3DAY", "TOTAL_SELLER_BUYTIMES_3DAY"],
            ),
        });
    }
    Ok(parsed)
}

fn merge_eastmoney_lhb_seats(
    buy_rows: Vec<EastmoneyLhbSeatRow>,
    sell_rows: Vec<EastmoneyLhbSeatRow>,
) -> Vec<Value> {
    let mut merged: HashMap<String, EastmoneyLhbSeatRow> = HashMap::new();
    for row in buy_rows.into_iter().chain(sell_rows) {
        if let Some(current) = merged.get_mut(&row.key) {
            current.buy_amount = current.buy_amount.or(row.buy_amount);
            current.sell_amount = current.sell_amount.or(row.sell_amount);
            current.buy_ratio = current.buy_ratio.or(row.buy_ratio);
            current.sell_ratio = current.sell_ratio.or(row.sell_ratio);
            current.trade_date = current.trade_date.clone().or(row.trade_date);
            current.change_rate = current.change_rate.or(row.change_rate);
            current.reason = current.reason.clone().or(row.reason);
            current.three_day_rise_probability = current
                .three_day_rise_probability
                .or(row.three_day_rise_probability);
            current.three_day_activity_count = current
                .three_day_activity_count
                .or(row.three_day_activity_count);
        } else {
            merged.insert(row.key.clone(), row);
        }
    }
    let mut rows = merged.into_values().collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        let left_total = left.buy_amount.unwrap_or(0.0) + left.sell_amount.unwrap_or(0.0);
        let right_total = right.buy_amount.unwrap_or(0.0) + right.sell_amount.unwrap_or(0.0);
        right_total
            .partial_cmp(&left_total)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    rows.into_iter().map(eastmoney_lhb_seat_value).collect()
}

fn eastmoney_lhb_seat_value(row: EastmoneyLhbSeatRow) -> Value {
    let direction = match (row.buy_amount.is_some(), row.sell_amount.is_some()) {
        (true, true) => "both",
        (true, false) => "buy",
        (false, true) => "sell",
        (false, false) => "unknown",
    };
    let net_amount = match (row.buy_amount, row.sell_amount) {
        (Some(buy), Some(sell)) => Some(buy - sell),
        _ => None,
    };
    json!({
        "seat_code": row.seat_code,
        "name": row.name,
        "trade_date": row.trade_date,
        "buy_amount": row.buy_amount,
        "sell_amount": row.sell_amount,
        "net_amount": net_amount,
        "buy_ratio": row.buy_ratio,
        "sell_ratio": row.sell_ratio,
        "direction": direction,
        "change_rate": row.change_rate,
        "reason": row.reason,
        "three_day_rise_probability": row.three_day_rise_probability,
        "three_day_activity_count": row.three_day_activity_count,
    })
}

fn guba_status_item(code: &str, end_date: &str, detail: &str) -> Value {
    json!({
        "category": "community_sentiment",
        "source": "东方财富股吧",
        "title": "东方财富股吧暂无可用情绪证据",
        "date": normalize_history_date(end_date),
        "metrics": {
            "状态": detail,
            "查询窗口": normalize_history_date(end_date).unwrap_or_else(|| end_date.to_string()),
            "已尝试信源": "东方财富股吧",
            "股票": code,
        },
        "sentiment": "uncertain",
        "weight": 0.15,
        "confidence": "低",
        "url": guba_list_url(code),
        "score": Value::Null,
        "note": "未取得可纳入评分的东方财富股吧帖子；该桶保留中性权重。",
    })
}

fn guba_list_url(code: &str) -> String {
    let digits = normalize_stock_code(code)
        .and_then(|value| value.get(..6).map(ToOwned::to_owned))
        .unwrap_or_else(|| {
            code.chars()
                .filter(|ch| ch.is_ascii_digit())
                .take(6)
                .collect()
        });
    format!("https://guba.eastmoney.com/list,{digits}.html")
}

fn eastmoney_lhb_unavailable_item(
    code: &str,
    start_date: &str,
    end_date: &str,
    detail: &str,
) -> Value {
    json!({
        "category": "institution_lhb_status",
        "source": "东方财富龙虎榜机构统计",
        "title": "东方财富龙虎榜机构统计不可用",
        "date": normalize_history_date(end_date),
        "metrics": {
            "状态": "接口不可用",
            "查询窗口": format!("{} - {}", normalize_history_date(start_date).unwrap_or_else(|| start_date.to_string()), normalize_history_date(end_date).unwrap_or_else(|| end_date.to_string())),
            "已尝试信源": "东方财富龙虎榜机构买卖每日统计",
            "失败原因": detail,
            "股票": code,
        },
        "sentiment": "uncertain",
        "weight": 0.25,
        "confidence": "低",
        "url": "https://data.eastmoney.com/stock/jgmmtj.html",
        "score": Value::Null,
        "note": "东方财富龙虎榜机构统计本次不可用；机构席位不参与加减分。",
    })
}

fn eastmoney_lhb_no_hit_item(code: &str, start_date: &str, end_date: &str) -> Value {
    let days = window_days(start_date, end_date);
    json!({
        "category": "institution_lhb_status",
        "source": "东方财富龙虎榜机构统计",
        "title": format!("近 {days} 日未上龙虎榜机构席位"),
        "date": end_date,
        "metrics": {
            "状态": format!("近 {days} 日未上榜"),
            "查询窗口": format!("{start_date} - {end_date}"),
            "已尝试信源": "东方财富龙虎榜机构买卖每日统计",
            "股票": code,
        },
        "sentiment": "uncertain",
        "weight": 0.25,
        "confidence": "中",
        "url": "https://data.eastmoney.com/stock/jgmmtj.html",
        "score": Value::Null,
        "note": "没有龙虎榜机构专用席位记录，不代表机构没有买卖；只说明查询窗口内未公开上榜。",
    })
}

fn merge_capital_evidence_items(
    data: &mut Value,
    code: &str,
    items: Vec<Value>,
    end_date: &str,
) -> bool {
    if !data.is_object() {
        *data = json!({});
    }
    let object = data.as_object_mut().expect("data object just initialized");
    let evidence_map = object
        .entry("capital_evidence".to_string())
        .or_insert_with(|| json!({}));
    if !evidence_map.is_object() {
        *evidence_map = json!({});
    }
    let evidence_object = evidence_map
        .as_object_mut()
        .expect("capital evidence object just initialized");
    let existing = evidence_object
        .get(code)
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut merged_items = existing
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let new_items = items;
    let has_community_batch = new_items
        .iter()
        .any(|item| item.get("category").and_then(Value::as_str) == Some("community_sentiment"));
    if has_community_batch {
        merged_items.retain(|old| {
            !matches!(
                old.get("category").and_then(Value::as_str),
                Some("community_sentiment" | "message_sentiment_status")
            )
        });
    }
    for item in new_items {
        let category = item.get("category").and_then(Value::as_str).unwrap_or("");
        if category != "community_sentiment" {
            let replacement_categories = match category {
                "institution_lhb" | "institution_lhb_status" => {
                    vec!["institution_lhb", "institution_lhb_status"]
                }
                "message_sentiment_status" => {
                    vec!["community_sentiment", "message_sentiment_status"]
                }
                _ => vec![category],
            };
            merged_items.retain(|old| {
                let old_category = old.get("category").and_then(Value::as_str).unwrap_or("");
                if matches!(category, "fund_flow" | "fund_flow_status") {
                    if !matches!(old_category, "fund_flow" | "fund_flow_status") {
                        return true;
                    }
                    let incoming_is_proxy = is_local_fund_flow_proxy_value(&item);
                    let old_is_proxy = is_local_fund_flow_proxy_value(old);
                    return incoming_is_proxy != old_is_proxy;
                }
                !replacement_categories.contains(&old_category)
            });
        }
        merged_items.push(item);
    }
    evidence_object.insert(
        code.to_string(),
        json!({
            "stock_code": code,
            "generated_at": epoch_millis().to_string(),
            "composite_score": Value::Null,
            "confidence": "中",
            "model_used": false,
            "as_of_trade_date": normalize_history_date(end_date),
            "freshness": "refreshed",
            "contributions": {},
            "summary": "已尝试接入东方财富当日主力资金、股吧情绪与龙虎榜机构统计，最终分数由 Rust 规则合成。",
            "sections": [],
            "items": merged_items,
            "notes": ["主力资金为东方财富个股资金流口径；股吧仅作社区情绪线索；龙虎榜机构统计为公开机构专用席位口径。"],
        }),
    );
    true
}

fn is_local_fund_flow_proxy_value(item: &Value) -> bool {
    item.get("title")
        .and_then(Value::as_str)
        .map(|title| title.contains("量价资金代理"))
        .unwrap_or(false)
        || item
            .get("source")
            .and_then(Value::as_str)
            .map(|source| source.contains("Tauri/Rust"))
            .unwrap_or(false)
        || item
            .get("metrics")
            .and_then(Value::as_object)
            .and_then(|metrics| metrics.get("证据类型"))
            .and_then(Value::as_str)
            == Some("本地日线量价代理")
}

async fn http_get_text_with_headers_first(
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

fn extract_json_after_var(html: &str, var_name: &str) -> Option<String> {
    let marker = format!("var {var_name}=");
    let start = html.find(&marker)? + marker.len();
    extract_balanced_json(&html[start..])
}

fn extract_balanced_json(raw: &str) -> Option<String> {
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

fn clean_html_text(value: &str) -> String {
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

fn normalize_guba_datetime(value: &str) -> Option<String> {
    let text = value.trim();
    if text.len() >= 10 {
        normalize_history_date(&text[..10])
    } else {
        None
    }
}

fn guba_sentiment_score(
    title: &str,
    summary: &str,
    object: &serde_json::Map<String, Value>,
) -> f64 {
    let text = format!("{title} {summary}");
    let mut score: f64 = 50.0;
    if contains_any(
        &text,
        &[
            "利好", "上涨", "看多", "突破", "增长", "改善", "中标", "订单", "企稳", "买入", "加仓",
        ],
    ) {
        score += 18.0;
    }
    if contains_any(
        &text,
        &[
            "利空", "下跌", "看空", "亏损", "风险", "承压", "减持", "出货", "破位", "调查",
        ],
    ) {
        score -= 18.0;
    }
    if let Some(flag) = object.get("bullish_bearish").and_then(Value::as_i64) {
        if flag > 0 {
            score += 8.0;
        } else if flag < 0 {
            score -= 8.0;
        }
    }
    round2_value(score.clamp(0.0, 100.0))
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn guba_bullish_bearish_label(value: Option<&Value>) -> &'static str {
    match value.and_then(Value::as_i64).unwrap_or(0) {
        flag if flag > 0 => "看多",
        flag if flag < 0 => "看空",
        _ => "未标注",
    }
}

fn score_sentiment_label(score: f64) -> &'static str {
    if score >= 60.0 {
        "positive"
    } else if score <= 40.0 {
        "negative"
    } else {
        "uncertain"
    }
}

fn institution_lhb_score(net: f64, buy: f64, sell: f64, ratio: Option<f64>) -> f64 {
    let total = (buy.abs() + sell.abs()).max(1.0);
    let directional = (net / total * 35.0).clamp(-35.0, 35.0);
    let ratio_score = ratio.unwrap_or(0.0).clamp(-15.0, 15.0);
    round2_value((50.0 + directional + ratio_score).clamp(0.0, 100.0))
}

fn round2_value(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn format_amount_wan(value: f64) -> String {
    if value.abs() >= 100_000_000.0 {
        format!("{:.2} 亿", value / 100_000_000.0)
    } else if value.abs() >= 10_000.0 {
        format!("{:.2} 万", value / 10_000.0)
    } else {
        format_number_like(value)
    }
}

fn institution_buy_sell_ratio(buy: f64, sell: f64) -> String {
    if sell.abs() > f64::EPSILON {
        format_number_like(buy / sell)
    } else if buy > 0.0 {
        "∞".to_string()
    } else {
        "-".to_string()
    }
}

fn format_number_like(value: f64) -> String {
    if value.abs() >= 100.0 {
        format!("{value:.0}")
    } else if value.abs() >= 10.0 {
        format!("{value:.2}")
    } else {
        format!("{value:.3}")
    }
}

fn compact_count(value: f64) -> String {
    if value >= 10_000.0 {
        format!("{:.1}万", value / 10_000.0)
    } else {
        format_number_like(value)
    }
}

fn window_days(start_date: &str, end_date: &str) -> i64 {
    let start = compact_date_key(start_date)
        .and_then(|key| days_from_civil_key(&key))
        .unwrap_or(0);
    let end = compact_date_key(end_date)
        .and_then(|key| days_from_civil_key(&key))
        .unwrap_or(start);
    (end - start + 1).max(1)
}

fn fallback_today_date() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| (duration.as_secs() / 86_400) as i64)
        .unwrap_or(0);
    civil_date_from_days(days)
}

fn fallback_lhb_start_date() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| (duration.as_secs() / 86_400) as i64 - 30)
        .unwrap_or(0);
    civil_date_from_days(days)
}

fn days_from_civil_key(key: &str) -> Option<i64> {
    if key.len() != 8 {
        return None;
    }
    let year = key.get(0..4)?.parse::<i32>().ok()?;
    let month = key.get(4..6)?.parse::<u32>().ok()?;
    let day = key.get(6..8)?.parse::<u32>().ok()?;
    Some(days_from_civil(year, month, day))
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = year as i64 - if month <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = month as i64 + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_date_from_days(days_since_epoch: i64) -> String {
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

struct QuarterlyEpsFetchResult {
    points: Vec<Value>,
    sources: Vec<String>,
    errors: Vec<String>,
}

async fn fetch_quarterly_eps_chain(code: &str) -> QuarterlyEpsFetchResult {
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

async fn fetch_ths_quarterly_eps(
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

fn parse_ths_quarterly_eps_json(text: &str) -> Result<Vec<Value>, String> {
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

async fn fetch_sina_quarterly_eps(
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

fn parse_sina_quarterly_eps_html(text: &str) -> Vec<Value> {
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

fn extract_html_cells(row: &str) -> Vec<String> {
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

fn strip_html_tags(value: &str) -> String {
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

fn merge_basic_financial_from_stock(data: &mut Value, code: &str) -> bool {
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

fn merge_quarterly_eps_points(data: &mut Value, code: &str, points: Vec<Value>) {
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

fn financial_quarterly_eps_count(data: &Value, code: &str) -> usize {
    data.get("financials")
        .and_then(Value::as_object)
        .and_then(|financials| financials.get(code))
        .and_then(|entry| entry.get("quarterly_eps"))
        .and_then(Value::as_array)
        .map(|rows| normalize_quarterly_eps(Some(&Value::Array(rows.clone()))).len())
        .unwrap_or(0)
}

fn financial_entry_mut<'a>(
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

fn stock_object<'a>(data: &'a Value, code: &str) -> Option<&'a serde_json::Map<String, Value>> {
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

fn entry_contains_number(entry: &serde_json::Map<String, Value>, field: &str) -> bool {
    entry
        .get(field)
        .and_then(|value| json_f64(Some(value)))
        .map(|value| value.is_finite())
        .unwrap_or(false)
}

fn upsert_entry_quarterly_eps(
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

fn push_financial_note(data: &mut Value, code: &str, note: String) -> bool {
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

fn append_financial_source(entry: &mut serde_json::Map<String, Value>, source: &str) {
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

fn sort_dedup_quarterly_eps(points: Vec<Value>) -> Vec<Value> {
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

fn quarterly_eps_value(period: &str, value: f64, source: &str) -> Value {
    json!({
        "period": period,
        "value": value,
        "source": source,
    })
}

fn object_number_any_loose(
    object: &serde_json::Map<String, Value>,
    fields: &[&str],
) -> Option<f64> {
    fields
        .iter()
        .find_map(|field| object.get(*field).and_then(|value| json_f64(Some(value))))
}

fn eps_metric_value(value: &Value) -> Option<f64> {
    if let Some(object) = value.as_object() {
        for key in ["value", "data", "val", "num", "latest", "amount", "single"] {
            if let Some(number) = object.get(key).and_then(value_first_finite_number) {
                return Some(number);
            }
        }
    }
    value_first_finite_number(value)
}

fn value_first_finite_number(value: &Value) -> Option<f64> {
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

fn financial_metric_is_eps(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    (name.contains("每股收益") || upper.contains("EPS")) && !name.contains("每股净资产")
}

fn financial_period_from_text(raw: &str) -> Option<String> {
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

fn ths_market_code(code: &str) -> Option<u16> {
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

fn current_calendar_year_utc() -> i32 {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| (duration.as_secs() / 86_400) as i64)
        .unwrap_or(0);
    civil_year_from_days(days)
}

fn civil_year_from_days(days_since_epoch: i64) -> i32 {
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
fn payload_history_rows(payload: &Value) -> Option<Vec<Value>> {
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

fn normalize_history_bar_value(row: &Value) -> Option<Value> {
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

fn insert_history_rows(data: &mut Value, code: &str, rows: Vec<Value>) {
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

fn history_cache_has_bars(
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

async fn fetch_observe_daily_history(
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

async fn fetch_daily_history_text(
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

async fn fetch_eastmoney_daily_history(
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

async fn fetch_tencent_daily_history(
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

fn tencent_daily_history_param(symbol: &str) -> String {
    format!("{symbol},day,,,{OBSERVE_DAILY_HISTORY_LIMIT},")
}

fn filter_daily_history_rows_by_date(
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

fn eastmoney_market_code(code: &str) -> Option<u8> {
    if code.ends_with(".SH") {
        Some(1)
    } else if code.ends_with(".SZ") || code.ends_with(".BJ") {
        Some(0)
    } else {
        None
    }
}

fn parse_eastmoney_kline_row(raw: &str) -> Option<Value> {
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

fn parse_tencent_kline_row(raw: &Value) -> Option<Value> {
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

fn json_f64(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64().filter(|value| value.is_finite()),
        Value::String(raw) => parse_f64_str(raw),
        _ => None,
    }
}

fn parse_f64_str(raw: &str) -> Option<f64> {
    let value = raw.trim().replace(',', "");
    if value.is_empty() || matches!(value.as_str(), "-" | "None" | "nan") {
        return None;
    }
    value.parse::<f64>().ok().filter(|value| value.is_finite())
}

fn normalize_history_date(raw: &str) -> Option<String> {
    let key = compact_date_key(raw)?;
    Some(format!("{}-{}-{}", &key[..4], &key[4..6], &key[6..8]))
}

fn compact_date_key(raw: &str) -> Option<String> {
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

fn cached_market_data(app: &tauri::AppHandle) -> Result<Value, String> {
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

fn screen_financial_snapshot(app: &tauri::AppHandle, payload: &Value) -> Option<Arc<Value>> {
    if let Some(snapshot) = payload
        .get("financial_snapshot")
        .filter(|snapshot| financial_snapshot_payload_present(snapshot))
    {
        let snapshot = Arc::new(snapshot.clone());
        if let Some(path) = cache_context_path(app) {
            if let Ok(mut slot) = refresh_financial_snapshot_cache().lock() {
                slot.insert(path, Arc::clone(&snapshot));
            }
        }
        return Some(snapshot);
    }
    let path = cache_context_path(app)?;
    refresh_financial_snapshot_cache()
        .lock()
        .ok()?
        .get(&path)
        .cloned()
}

fn screen_stock_override(
    app: &tauri::AppHandle,
    data: &Arc<gp_core::CoreDataSet>,
    payload: &Value,
) -> Result<Option<Arc<Vec<gp_core::StockItem>>>, String> {
    let Some(financial_snapshot) = screen_financial_snapshot(app, payload) else {
        return Ok(None);
    };
    let path = mobile_market_data_path(app)?;
    if let Ok(slot) = screen_stock_overlay_cache().lock() {
        if let Some(entry) = slot.get(&path) {
            if Arc::ptr_eq(&entry.data, data)
                && entry.financial_snapshot.as_ref() == financial_snapshot.as_ref()
            {
                return Ok(Some(Arc::clone(&entry.stocks)));
            }
        }
    }

    let mut stock_data = json!({ "stocks": &data.stocks });
    merge_screen_financial_snapshot_into_data(&mut stock_data, financial_snapshot.as_ref());
    let stocks = stock_data
        .as_object_mut()
        .and_then(|object| object.remove("stocks"))
        .unwrap_or_else(|| json!([]));
    let stocks = Arc::new(
        serde_json::from_value(stocks)
            .map_err(|error| format!("screen stock overlay parse failed: {error}"))?,
    );
    if let Ok(mut slot) = screen_stock_overlay_cache().lock() {
        slot.insert(
            path,
            ScreenStockOverlayCacheEntry {
                data: Arc::clone(data),
                financial_snapshot,
                stocks: Arc::clone(&stocks),
            },
        );
    }
    Ok(Some(stocks))
}

async fn run_graph_screen_command(
    label: &'static str,
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let data = cached_market_data_snapshot(&app)?;
    let stock_override = screen_stock_override(&app, &data, &payload)?;
    let request = serde_json::from_value::<gp_core::GraphScreenRequest>(
        strip_core_side_payload_fields(payload),
    )
    .map_err(|error| format!("invalid graph screen request: {error}"))?;
    runtime::run_cpu_bound(label, move || {
        let source = match stock_override.as_deref() {
            Some(stocks) => gp_core::StaticDataSource::with_stocks(data.as_ref(), stocks),
            None => gp_core::StaticDataSource::new(data.as_ref()),
        };
        let result = gp_core::graph_screen_with_source(&source, &request)
            .map_err(|error| error.to_string())?;
        serde_json::to_value(result).map_err(|error| error.to_string())
    })
    .await?
}
fn merge_screen_financial_snapshot_into_data(data: &mut Value, financial_snapshot: &Value) {
    if !financial_snapshot_payload_present(financial_snapshot) {
        return;
    }
    let (seed_stocks, seed_codes) = seed_stock_maps(data);
    if seed_codes.is_empty() {
        return;
    }
    let enriched_stocks = enriched_stock_maps(&seed_stocks, financial_snapshot);
    let mut stocks = Vec::with_capacity(seed_codes.len());
    let mut seen = HashSet::new();
    append_preserved_seed_stocks(&seed_codes, &enriched_stocks, &mut stocks, &mut seen);
    if let Some(object) = data.as_object_mut() {
        object.insert("stocks".to_string(), Value::Array(stocks));
    }
}

fn strip_core_side_payload_fields(mut payload: Value) -> Value {
    if let Some(object) = payload.as_object_mut() {
        object.remove("financial_snapshot");
    }
    payload
}

fn market_data_status(app: &tauri::AppHandle) -> Result<Value, String> {
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

fn read_mobile_market_data(app: &tauri::AppHandle) -> Result<Value, String> {
    read_mobile_market_data_record(app, true)
}

fn read_mobile_market_data_record(
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

fn write_mobile_market_data(app: &tauri::AppHandle, payload: Value) -> Result<Value, String> {
    write_mobile_market_data_record(app, payload, true)
}

fn write_mobile_market_data_record(
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

fn retry_with_attempts<T, F>(attempts: usize, delay_ms: u64, mut op: F) -> Result<T, String>
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

fn write_mobile_market_data_with_retry(
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

fn write_mobile_market_data_once(tmp_path: &Path, path: &Path, bytes: &[u8]) -> Result<(), String> {
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

fn market_quote_coverage(data: &Value, target_date: Option<&str>) -> (usize, usize, f64) {
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

fn market_cache_record(
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

fn cached_mobile_market_data_entry(
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

fn market_data_patch_for_codes(data: &Value, codes: &[String]) -> Value {
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

fn apply_market_data_patch(data: &mut Value, patch: &Value) {
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
fn mobile_market_patch_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let path = mobile_market_data_path(app)?;
    let root = path
        .parent()
        .ok_or_else(|| "mobile market cache path has no parent".to_string())?;
    Ok(root.join(MOBILE_MARKET_PATCH_DIR))
}

fn apply_persisted_market_data_patches(
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

fn clear_mobile_market_data_patches(app: &tauri::AppHandle) -> Result<(), String> {
    let root = mobile_market_patch_dir(app)?;
    if root.exists() {
        fs::remove_dir_all(&root)
            .map_err(|error| format!("clear mobile market patches failed: {error}"))?;
    }
    Ok(())
}

fn market_data_patch_codes(patch: &Value) -> Vec<String> {
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

fn persist_market_data_patch_sync(app: &tauri::AppHandle, patch: &Value) -> Result<usize, String> {
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

async fn persist_market_data_patch_updates(
    app: tauri::AppHandle,
    patch: Value,
) -> Result<usize, String> {
    runtime::run_io_bound("persist market data patches", move || {
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

async fn persist_market_data_updates(
    app: tauri::AppHandle,
    data: Value,
    codes: Vec<String>,
) -> Result<usize, String> {
    let patch = market_data_patch_for_codes(&data, &codes);
    persist_market_data_patch_updates(app, patch).await
}
fn parse_mobile_market_data_snapshot(
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

fn cached_market_data_snapshot(
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

fn remember_mobile_market_data_cache(
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

fn forget_mobile_market_data_cache(path: &Path) {
    if let Ok(mut slot) = mobile_market_data_cache().lock() {
        slot.remove(path);
    }
}

#[derive(Clone, Debug)]
struct WatchlistRecord {
    code: String,
    name: Option<String>,
    industry: Option<String>,
    added_at: String,
    source: Option<String>,
    screen_criteria_summary: Option<String>,
}

fn adaptive_screen_db_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("get app data dir failed: {error}"))?;
    root.push("screening");
    fs::create_dir_all(&root)
        .map_err(|error| format!("create screening dir failed: {}: {error}", root.display()))?;
    root.push(ADAPTIVE_SCREEN_DB_FILE);
    Ok(root)
}

fn initialize_adaptive_exposure_db(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         CREATE TABLE IF NOT EXISTS adaptive_screen_exposure (
             code TEXT NOT NULL,
             trade_date TEXT NOT NULL,
             bucket TEXT NOT NULL,
             mode TEXT NOT NULL,
             algorithm_version TEXT NOT NULL,
             selected_at INTEGER NOT NULL,
             PRIMARY KEY (code, trade_date, bucket)
         );
         CREATE INDEX IF NOT EXISTS idx_adaptive_exposure_date
           ON adaptive_screen_exposure(trade_date DESC, selected_at DESC);
         CREATE TABLE IF NOT EXISTS adaptive_screen_runs (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             run_id TEXT NOT NULL UNIQUE,
             trade_date TEXT NOT NULL,
             selected_codes_json TEXT NOT NULL,
             elapsed_millis INTEGER NOT NULL,
             cache_hit INTEGER NOT NULL,
             algorithm_version TEXT NOT NULL,
             recorded_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_adaptive_runs_recent
           ON adaptive_screen_runs(id DESC, recorded_at DESC);
         CREATE TABLE IF NOT EXISTS adaptive_screen_runs_v2 (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             run_id TEXT NOT NULL,
             implementation_fingerprint TEXT NOT NULL,
             trade_date TEXT NOT NULL,
             selected_codes_json TEXT NOT NULL,
             elapsed_millis INTEGER NOT NULL,
             cache_hit INTEGER NOT NULL,
             algorithm_version TEXT NOT NULL,
             recorded_at INTEGER NOT NULL,
             UNIQUE (run_id, implementation_fingerprint)
         );
         CREATE INDEX IF NOT EXISTS idx_adaptive_runs_v2_recent
           ON adaptive_screen_runs_v2(implementation_fingerprint, recorded_at ASC, id ASC);
         CREATE TABLE IF NOT EXISTS adaptive_screen_runs_v3 (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             run_id TEXT NOT NULL,
             implementation_fingerprint TEXT NOT NULL,
             release_evidence_qualified INTEGER NOT NULL,
             trade_date TEXT NOT NULL,
             selected_codes_json TEXT NOT NULL,
             elapsed_millis INTEGER NOT NULL,
             cache_hit INTEGER NOT NULL,
             algorithm_version TEXT NOT NULL,
             recorded_at INTEGER NOT NULL,
             UNIQUE (run_id, implementation_fingerprint)
         );
         CREATE INDEX IF NOT EXISTS idx_adaptive_runs_v3_recent
           ON adaptive_screen_runs_v3(implementation_fingerprint, release_evidence_qualified, recorded_at ASC, id ASC);
         CREATE TABLE IF NOT EXISTS adaptive_release_gate_reports (
             algorithm_version TEXT PRIMARY KEY,
             input_json TEXT NOT NULL,
             report_json TEXT NOT NULL,
             passed INTEGER NOT NULL,
             updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS adaptive_release_gate_reports_v2 (
             algorithm_version TEXT NOT NULL,
             implementation_fingerprint TEXT NOT NULL,
             qualification_json TEXT NOT NULL,
             input_json TEXT NOT NULL,
             report_json TEXT NOT NULL,
             passed INTEGER NOT NULL,
             updated_at INTEGER NOT NULL,
             PRIMARY KEY (algorithm_version, implementation_fingerprint)
         );",
    )
    .map_err(|error| format!("initialize adaptive screen sqlite failed: {error}"))
}

fn open_adaptive_screen_db(app: &tauri::AppHandle) -> Result<Connection, String> {
    let path = adaptive_screen_db_path(app)?;
    let conn = Connection::open(&path).map_err(|error| {
        format!(
            "open adaptive screen sqlite failed: {}: {error}",
            path.display()
        )
    })?;
    initialize_adaptive_exposure_db(&conn)?;
    Ok(conn)
}

fn adaptive_selected_codes(result: &Value) -> Vec<String> {
    let mut codes = result
        .get("groups")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|group| {
            group
                .get("items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|item| item.pointer("/stock/code").and_then(Value::as_str))
                .map(|code| code.to_ascii_uppercase())
        })
        .collect::<Vec<_>>();
    codes.sort();
    codes.dedup();
    codes
}

fn adaptive_release_run_record_rows(
    conn: &Connection,
    run_id: Option<&str>,
    result: &Value,
    trade_date: &str,
    elapsed_millis: u64,
    cache_hit: bool,
    release_evidence_qualified: bool,
) -> Result<(), String> {
    initialize_adaptive_exposure_db(conn)?;
    let trade_date = compact_date_key(trade_date)
        .ok_or_else(|| "adaptive screen run trade date is invalid".to_string())?;
    let recorded_at = epoch_millis().min(i64::MAX as u128) as i64;
    let keep_after = recorded_at.saturating_sub(30 * 24 * 60 * 60 * 1_000);
    let run_id = run_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("compat-{trade_date}-{recorded_at}"));
    let selected_codes_json = serde_json::to_string(&adaptive_selected_codes(result))
        .map_err(|error| format!("serialize adaptive run symbols failed: {error}"))?;
    let algorithm_version = result
        .get("algorithm_version")
        .and_then(Value::as_str)
        .unwrap_or("adaptive_swing_v1");
    let implementation_fingerprint = adaptive_release_implementation_fingerprint();
    conn.execute(
        "DELETE FROM adaptive_screen_runs_v3 WHERE recorded_at < ?1",
        params![keep_after],
    )
    .map_err(|error| format!("prune adaptive run evidence failed: {error}"))?;
    conn.execute(
        "INSERT INTO adaptive_screen_runs_v3
           (run_id, implementation_fingerprint, release_evidence_qualified, trade_date, selected_codes_json, elapsed_millis, cache_hit, algorithm_version, recorded_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(run_id, implementation_fingerprint) DO UPDATE SET
           release_evidence_qualified = excluded.release_evidence_qualified,
           trade_date = excluded.trade_date,
           selected_codes_json = excluded.selected_codes_json,
           elapsed_millis = excluded.elapsed_millis,
           cache_hit = excluded.cache_hit,
           algorithm_version = excluded.algorithm_version,
           recorded_at = excluded.recorded_at",
        params![
            run_id,
            implementation_fingerprint,
            i64::from(release_evidence_qualified),
            trade_date,
            selected_codes_json,
            elapsed_millis.min(i64::MAX as u64) as i64,
            i64::from(cache_hit),
            algorithm_version,
            recorded_at
        ],
    )
    .map_err(|error| format!("record adaptive run evidence failed: {error}"))?;
    adaptive_release_gate_recompute_operational_rows(conn)?;
    Ok(())
}

fn adaptive_release_run_record_sync(
    app: &tauri::AppHandle,
    run_id: Option<&str>,
    result: &Value,
    trade_date: &str,
    elapsed_millis: u64,
    cache_hit: bool,
    release_evidence_qualified: bool,
) -> Result<(), String> {
    adaptive_release_run_record_rows(
        &open_adaptive_screen_db(app)?,
        run_id,
        result,
        trade_date,
        elapsed_millis,
        cache_hit,
        release_evidence_qualified,
    )
}

fn adaptive_release_operational_evidence_rows(
    conn: &Connection,
) -> Result<(Option<usize>, Option<u64>, Option<u64>), String> {
    initialize_adaptive_exposure_db(conn)?;
    let keep_after =
        (epoch_millis().min(i64::MAX as u128) as i64).saturating_sub(30 * 24 * 60 * 60 * 1_000);
    let recent_json = {
        let mut statement = conn
            .prepare(
                "SELECT selected_codes_json
                 FROM adaptive_screen_runs_v3
                 WHERE algorithm_version = 'adaptive_swing_v1'
                   AND implementation_fingerprint = ?1
                   AND release_evidence_qualified = 1
                   AND recorded_at >= ?2
                 ORDER BY recorded_at ASC, id ASC",
            )
            .map_err(|error| format!("prepare adaptive run coverage query failed: {error}"))?;
        let rows = statement
            .query_map(
                params![adaptive_release_implementation_fingerprint(), keep_after],
                |row| row.get::<_, String>(0),
            )
            .map_err(|error| format!("query adaptive run coverage failed: {error}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("read adaptive run coverage failed: {error}"))?
    };
    let parsed_runs = recent_json
        .into_iter()
        .map(|encoded| {
            serde_json::from_str::<Vec<String>>(&encoded)
                .map(|codes| {
                    codes
                        .into_iter()
                        .map(|code| code.to_ascii_uppercase())
                        .collect::<HashSet<_>>()
                })
                .map_err(|error| format!("parse adaptive run symbols failed: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let five_run_unique_coverage = if parsed_runs.len() >= 5 {
        parsed_runs
            .windows(5)
            .map(|window| {
                window
                    .iter()
                    .flat_map(|codes| codes.iter().cloned())
                    .collect::<HashSet<_>>()
                    .len()
            })
            .min()
    } else {
        None
    };
    let max_elapsed = |cache_hit: bool| -> Result<Option<u64>, String> {
        let value = conn
            .query_row(
                "SELECT MAX(elapsed_millis)
                 FROM adaptive_screen_runs_v3
                 WHERE algorithm_version = 'adaptive_swing_v1'
                   AND implementation_fingerprint = ?1
                   AND release_evidence_qualified = 1
                   AND cache_hit = ?2
                   AND recorded_at >= ?3",
                params![
                    adaptive_release_implementation_fingerprint(),
                    i64::from(cache_hit),
                    keep_after
                ],
                |row| row.get::<_, Option<i64>>(0),
            )
            .map_err(|error| format!("query adaptive run latency failed: {error}"))?;
        Ok(value.and_then(|value| u64::try_from(value).ok()))
    };
    Ok((
        five_run_unique_coverage,
        max_elapsed(false)?,
        max_elapsed(true)?,
    ))
}

fn adaptive_release_operational_evidence_sync(
    app: &tauri::AppHandle,
) -> Result<(Option<usize>, Option<u64>, Option<u64>), String> {
    adaptive_release_operational_evidence_rows(&open_adaptive_screen_db(app)?)
}

fn adaptive_release_gate_store_rows(
    conn: &Connection,
    input: &gp_core::AdaptiveReleaseGateInput,
    report: &gp_core::AdaptiveReleaseGateReport,
    qualification: &Value,
) -> Result<(), String> {
    initialize_adaptive_exposure_db(conn)?;
    let input_json = serde_json::to_string(input)
        .map_err(|error| format!("serialize adaptive release input failed: {error}"))?;
    let report_json = serde_json::to_string(report)
        .map_err(|error| format!("serialize adaptive release report failed: {error}"))?;
    let qualification_json = serde_json::to_string(qualification)
        .map_err(|error| format!("serialize adaptive release qualification failed: {error}"))?;
    conn.execute(
        "INSERT INTO adaptive_release_gate_reports_v2
           (algorithm_version, implementation_fingerprint, qualification_json, input_json, report_json, passed, updated_at)
         VALUES ('adaptive_swing_v1', ?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(algorithm_version, implementation_fingerprint) DO UPDATE SET
           qualification_json = excluded.qualification_json,
           input_json = excluded.input_json,
           report_json = excluded.report_json,
           passed = excluded.passed,
           updated_at = excluded.updated_at",
        params![
            adaptive_release_implementation_fingerprint(),
            qualification_json,
            input_json,
            report_json,
            i64::from(report.passed),
            epoch_millis().min(i64::MAX as u128) as i64
        ],
    )
    .map_err(|error| format!("store adaptive release gate failed: {error}"))?;
    Ok(())
}

fn adaptive_release_gate_context_rows(
    conn: &Connection,
) -> Result<Option<(gp_core::AdaptiveReleaseGateInput, Value)>, String> {
    let encoded = conn
        .query_row(
            "SELECT input_json, qualification_json
             FROM adaptive_release_gate_reports_v2
             WHERE algorithm_version = 'adaptive_swing_v1'
               AND implementation_fingerprint = ?1",
            params![adaptive_release_implementation_fingerprint()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|error| format!("load adaptive release gate context failed: {error}"))?;
    encoded
        .map(|(input_json, qualification_json)| {
            let input = serde_json::from_str::<gp_core::AdaptiveReleaseGateInput>(&input_json)
                .map_err(|error| format!("parse adaptive release input failed: {error}"))?;
            let qualification = serde_json::from_str::<Value>(&qualification_json)
                .map_err(|error| format!("parse adaptive release qualification failed: {error}"))?;
            Ok((input, qualification))
        })
        .transpose()
}

fn adaptive_release_gate_recompute_operational_rows(conn: &Connection) -> Result<(), String> {
    let Some((mut input, qualification)) = adaptive_release_gate_context_rows(conn)? else {
        return Ok(());
    };
    let evidence = adaptive_release_operational_evidence_rows(conn)?;
    input.five_run_unique_coverage = evidence.0;
    input.first_run_millis = evidence.1;
    input.cached_run_millis = evidence.2;
    let report = gp_core::evaluate_adaptive_release_gate(&input);
    adaptive_release_gate_store_rows(conn, &input, &report, &qualification)
}

#[cfg(test)]
fn adaptive_release_gate_load_rows(
    conn: &Connection,
) -> Result<Option<gp_core::AdaptiveReleaseGateReport>, String> {
    initialize_adaptive_exposure_db(conn)?;
    let encoded = conn
        .query_row(
            "SELECT input_json, report_json, qualification_json, passed
             FROM adaptive_release_gate_reports_v2
             WHERE algorithm_version = 'adaptive_swing_v1'
               AND implementation_fingerprint = ?1",
            params![adaptive_release_implementation_fingerprint()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("load adaptive release gate failed: {error}"))?;
    encoded
        .map(
            |(input_json, report_json, qualification_json, stored_passed)| {
                let input = serde_json::from_str::<gp_core::AdaptiveReleaseGateInput>(&input_json)
                    .map_err(|error| format!("parse adaptive release input failed: {error}"))?;
                let qualification =
                    serde_json::from_str::<Value>(&qualification_json).map_err(|error| {
                        format!("parse adaptive release qualification failed: {error}")
                    })?;
                let mut report =
                    serde_json::from_str::<gp_core::AdaptiveReleaseGateReport>(&report_json)
                        .map_err(|error| format!("parse adaptive release gate failed: {error}"))?;
                report.passed = report.passed
                    && stored_passed == 1
                    && input.release_configuration_qualified == Some(true)
                    && qualification.get("qualified").and_then(Value::as_bool) == Some(true)
                    && qualification
                        .get("implementation_fingerprint")
                        .and_then(Value::as_str)
                        == Some(adaptive_release_implementation_fingerprint());
                Ok(report)
            },
        )
        .transpose()
}

fn adaptive_release_gate_store_sync(
    app: &tauri::AppHandle,
    input: &gp_core::AdaptiveReleaseGateInput,
    report: &gp_core::AdaptiveReleaseGateReport,
    qualification: &Value,
) -> Result<(), String> {
    adaptive_release_gate_store_rows(&open_adaptive_screen_db(app)?, input, report, qualification)
}

#[cfg(test)]
fn adaptive_release_gate_refresh_and_load_rows(
    conn: &Connection,
) -> Result<Option<gp_core::AdaptiveReleaseGateReport>, String> {
    adaptive_release_gate_recompute_operational_rows(conn)?;
    adaptive_release_gate_load_rows(conn)
}

fn adaptive_exposure_recent_rows(
    conn: &Connection,
    current_trade_date: Option<&str>,
) -> Result<Vec<gp_core::AdaptiveRecentExposure>, String> {
    let cutoff_date = current_trade_date
        .and_then(compact_date_key)
        .unwrap_or_else(|| "99999999".to_string());
    let mut statement = conn
        .prepare(
            "SELECT code, trade_date, bucket
             FROM adaptive_screen_exposure
             WHERE trade_date < ?1
               AND
             trade_date IN (
                  SELECT DISTINCT trade_date
                  FROM adaptive_screen_exposure
                  WHERE trade_date < ?1
                  ORDER BY trade_date DESC
                  LIMIT 5
              )
             ORDER BY trade_date DESC, code ASC, bucket ASC",
        )
        .map_err(|error| format!("prepare adaptive exposure query failed: {error}"))?;
    let rows = statement
        .query_map(params![cutoff_date], |row| {
            Ok(gp_core::AdaptiveRecentExposure {
                code: row.get(0)?,
                trade_date: row.get(1)?,
                bucket: row.get(2)?,
            })
        })
        .map_err(|error| format!("query adaptive exposure failed: {error}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("read adaptive exposure failed: {error}"))
}

fn adaptive_exposure_recent_sync(
    app: &tauri::AppHandle,
    current_trade_date: Option<&str>,
) -> Result<Vec<gp_core::AdaptiveRecentExposure>, String> {
    adaptive_exposure_recent_rows(&open_adaptive_screen_db(app)?, current_trade_date)
}

fn adaptive_exposure_record_sync(
    app: &tauri::AppHandle,
    result: &Value,
    trade_date: &str,
) -> Result<(), String> {
    let mut conn = open_adaptive_screen_db(app)?;
    adaptive_exposure_record_rows(&mut conn, result, trade_date)
}

fn adaptive_exposure_record_rows(
    conn: &mut Connection,
    result: &Value,
    trade_date: &str,
) -> Result<(), String> {
    initialize_adaptive_exposure_db(conn)?;
    let trade_date = compact_date_key(trade_date)
        .ok_or_else(|| "adaptive screen trade date is invalid".to_string())?;
    let selected_at = epoch_millis() as i64;
    let keep_after = selected_at.saturating_sub(30 * 24 * 60 * 60 * 1_000);
    let algorithm_version = result
        .get("algorithm_version")
        .and_then(Value::as_str)
        .unwrap_or("adaptive_swing_v1");
    let mode = result
        .pointer("/market_regime/effective")
        .and_then(Value::as_str)
        .unwrap_or("auto");
    let selected = result
        .get("groups")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|group| {
            let bucket = group
                .get("key")
                .and_then(Value::as_str)
                .unwrap_or("primary")
                .to_string();
            group
                .get("items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(move |item| {
                    item.pointer("/stock/code")
                        .and_then(Value::as_str)
                        .map(|code| (code.to_ascii_uppercase(), bucket.clone()))
                })
        })
        .collect::<Vec<_>>();
    let transaction = conn
        .transaction()
        .map_err(|error| format!("begin adaptive exposure transaction failed: {error}"))?;
    transaction
        .execute(
            "DELETE FROM adaptive_screen_exposure WHERE selected_at < ?1",
            params![keep_after],
        )
        .map_err(|error| format!("prune adaptive exposure failed: {error}"))?;
    {
        let mut statement = transaction
            .prepare(
                "INSERT INTO adaptive_screen_exposure
                   (code, trade_date, bucket, mode, algorithm_version, selected_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(code, trade_date, bucket) DO UPDATE SET
                   mode = excluded.mode,
                   algorithm_version = excluded.algorithm_version,
                   selected_at = excluded.selected_at",
            )
            .map_err(|error| format!("prepare adaptive exposure insert failed: {error}"))?;
        for (code, bucket) in selected {
            transaction
                .execute(
                    "DELETE FROM adaptive_screen_exposure WHERE code = ?1 AND trade_date = ?2",
                    params![code, trade_date],
                )
                .map_err(|error| format!("deduplicate adaptive exposure failed: {error}"))?;
            statement
                .execute(params![
                    code,
                    trade_date,
                    bucket,
                    mode,
                    algorithm_version,
                    selected_at
                ])
                .map_err(|error| format!("insert adaptive exposure failed: {error}"))?;
        }
    }
    transaction
        .commit()
        .map_err(|error| format!("commit adaptive exposure failed: {error}"))
}

fn watchlist_db_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("get app data dir failed: {error}"))?;
    root.push("watchlist");
    fs::create_dir_all(&root)
        .map_err(|error| format!("create watchlist dir failed: {}: {error}", root.display()))?;
    root.push(WATCHLIST_DB_FILE);
    Ok(root)
}

fn open_watchlist_db(app: &tauri::AppHandle) -> Result<Connection, String> {
    let path = watchlist_db_path(app)?;
    let conn = Connection::open(&path)
        .map_err(|error| format!("open watchlist sqlite failed: {}: {error}", path.display()))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         CREATE TABLE IF NOT EXISTS watchlist (
             code TEXT PRIMARY KEY NOT NULL,
             name TEXT,
             industry TEXT,
             added_at TEXT NOT NULL,
             source TEXT,
             screen_criteria_summary TEXT,
             updated_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_watchlist_added_at ON watchlist(added_at DESC);",
    )
    .map_err(|error| format!("initialize watchlist sqlite failed: {error}"))?;
    Ok(conn)
}

fn watchlist_list_sync(app: &tauri::AppHandle) -> Result<Value, String> {
    let conn = open_watchlist_db(app)?;
    watchlist_rows(&conn)
}

fn watchlist_replace_sync(
    app: &tauri::AppHandle,
    items: Vec<WatchlistRecord>,
) -> Result<Value, String> {
    let mut conn = open_watchlist_db(app)?;
    let tx = conn
        .transaction()
        .map_err(|error| format!("begin watchlist transaction failed: {error}"))?;
    tx.execute("DELETE FROM watchlist", [])
        .map_err(|error| format!("clear watchlist before replace failed: {error}"))?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO watchlist (code, name, industry, added_at, source, screen_criteria_summary, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .map_err(|error| format!("prepare watchlist replace failed: {error}"))?;
        for item in normalize_watchlist_records(items) {
            stmt.execute(params![
                item.code,
                item.name,
                item.industry,
                item.added_at,
                item.source,
                item.screen_criteria_summary,
                epoch_millis() as i64,
            ])
            .map_err(|error| format!("insert watchlist item failed: {error}"))?;
        }
    }
    tx.commit()
        .map_err(|error| format!("commit watchlist replace failed: {error}"))?;
    watchlist_rows(&conn)
}

fn watchlist_upsert_sync(app: &tauri::AppHandle, item: WatchlistRecord) -> Result<(), String> {
    let conn = open_watchlist_db(app)?;
    let item = normalize_watchlist_records(vec![item])
        .into_iter()
        .next()
        .ok_or_else(|| "watchlist code is required".to_string())?;
    conn.execute(
        "INSERT INTO watchlist (code, name, industry, added_at, source, screen_criteria_summary, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(code) DO UPDATE SET
           name = excluded.name,
           industry = excluded.industry,
           source = excluded.source,
           screen_criteria_summary = excluded.screen_criteria_summary,
           updated_at = excluded.updated_at",
        params![
            item.code,
            item.name,
            item.industry,
            item.added_at,
            item.source,
            item.screen_criteria_summary,
            epoch_millis() as i64,
        ],
    )
    .map_err(|error| format!("upsert watchlist item failed: {error}"))?;
    Ok(())
}

fn watchlist_rows(conn: &Connection) -> Result<Value, String> {
    let mut stmt = conn
        .prepare(
            "SELECT code, name, industry, added_at, source, screen_criteria_summary
             FROM watchlist
             ORDER BY added_at DESC, updated_at DESC, code ASC",
        )
        .map_err(|error| format!("prepare watchlist query failed: {error}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(json!({
                "code": row.get::<_, String>(0)?,
                "name": row.get::<_, Option<String>>(1)?,
                "industry": row.get::<_, Option<String>>(2)?,
                "added_at": row.get::<_, String>(3)?,
                "source": row.get::<_, Option<String>>(4)?,
                "screenCriteriaSummary": row.get::<_, Option<String>>(5)?,
            }))
        })
        .map_err(|error| format!("query watchlist failed: {error}"))?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row.map_err(|error| format!("read watchlist row failed: {error}"))?);
    }
    Ok(Value::Array(items))
}

fn watchlist_items_from_payload(payload: &Value) -> Result<Vec<WatchlistRecord>, String> {
    let items = payload
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| "watchlist items array is required".to_string())?;
    items.iter().map(watchlist_item_from_value).collect()
}

fn watchlist_item_from_value(value: &Value) -> Result<WatchlistRecord, String> {
    let code = value
        .get("code")
        .and_then(Value::as_str)
        .and_then(normalize_stock_code)
        .ok_or_else(|| "watchlist code is required".to_string())?;
    Ok(WatchlistRecord {
        code,
        name: optional_trimmed_string(value.get("name")),
        industry: optional_trimmed_string(value.get("industry")),
        added_at: optional_trimmed_string(value.get("added_at"))
            .unwrap_or_else(|| epoch_millis().to_string()),
        source: optional_trimmed_string(value.get("source")),
        screen_criteria_summary: optional_trimmed_string(value.get("screenCriteriaSummary"))
            .or_else(|| optional_trimmed_string(value.get("screen_criteria_summary"))),
    })
}

fn normalize_watchlist_records(items: Vec<WatchlistRecord>) -> Vec<WatchlistRecord> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for item in items {
        if item.code.is_empty() || !seen.insert(item.code.clone()) {
            continue;
        }
        normalized.push(item);
    }
    normalized
}

fn optional_trimmed_string(value: Option<&Value>) -> Option<String> {
    let trimmed = value?.as_str()?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn mobile_market_data_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("get app data dir failed: {error}"))?;
    root.push("market");
    root.push(MOBILE_MARKET_DATA_FILE);
    Ok(root)
}
fn file_modified_millis(path: &Path) -> Option<u128> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis())
}

fn upstream_rag_mobile_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("获取应用数据目录失败：{error}"))?;
    root.push("upstream_rag");
    Ok(root)
}

fn read_json_file(path: &Path) -> Result<Value, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("读取 JSON 失败：{}：{error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("解析 JSON 失败：{}：{error}", path.display()))
}

fn find_first_current_manifest(root: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path().join("current_manifest.json");
        if path.exists() {
            return Some(path);
        }
    }
    None
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn sanitize_path_part(value: &str) -> String {
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

fn schedule_research_maintenance(app: tauri::AppHandle) {
    schedule_research_embeddings(app.clone());
    tauri::async_runtime::spawn(async move {
        loop {
            let worker_app = app.clone();
            let result = runtime::run_io_bound("background_research_retention", move || {
                research::with_app_store(&worker_app, |store| store.prune_retention())
            })
            .await
            .and_then(|result| result);
            let event = match result {
                Ok(status) => json!({"ok": true, "status": status}),
                Err(error) => json!({"ok": false, "error": error}),
            };
            let _ = app.emit("research-retention-status", event);
            tokio::time::sleep(Duration::from_secs(6 * 60 * 60)).await;
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            core_screen,
            core_screen_with_data,
            core_graph_screen,
            core_graph_screen_with_data,
            core_backtest,
            core_backtest_with_data,
            core_trend,
            core_trend_with_data,
            core_trend_screen,
            core_trend_screen_with_data,
            core_agent,
            core_agent_with_data,
            core_mobile_stock_skill,
            api_health,
            api_strategies,
            api_market_status,
            api_data_sources,
            api_market_refresh,
            api_market_ingest_tencent_quotes,
            api_market_clear_cache,
            api_screen,
            api_sector_screen,
            api_custom_screen,
            api_graph_screen,
            api_trend_analyze,
            api_trend_screen,
            api_observe,
            api_backtest,
            api_stock_search,
            api_stock_get,
            api_minutes,
            api_order_book,
            api_watchlist_list,
            api_watchlist_replace,
            api_watchlist_add,
            api_watchlist_remove,
            api_watchlist_clear,
            api_news_rag,
            api_research_overview,
            api_research_messages,
            api_research_mark_read,
            api_research_query,
            api_research_refresh,
            api_research_threads,
            api_research_thread_create,
            api_research_thread_detail,
            api_research_thread_delete,
            api_research_index_status,
            api_research_rebuild_index,
            api_research_rebuild_embeddings,
            api_research_import_url,
            api_research_import_pdf,
            api_research_pack_export,
            api_research_pack_import,
            api_research_pack_rollback,
            api_rag_pack_status,
            api_rag_pack_build,
            api_rag_pack_build_from_news_cache,
            api_rag_pack_query,
            api_upstream_rag_status,
            api_upstream_rag_build,
            api_upstream_rag_transfer_start,
            api_agent_stream,
            api_agent_run_list,
            api_agent_run_metrics,
            api_agent_run_get,
            api_agent_run_delete_conversation,
            api_llm_models,
            api_llm_test,
            core_validate_data_source,
            core_mobile_market_data_read,
            core_mobile_market_data_write,
            core_mobile_market_data_clear,
            core_mobile_network_probe,
            core_mobile_market_data_refresh_tencent,
            core_upstream_rag_import,
            core_upstream_rag_list,
            core_upstream_rag_detail,
            core_upstream_rag_rollback,
            open_external_url
        ])
        .plugin(tauri_plugin_shell::init());

    #[cfg(not(mobile))]
    let builder = builder.setup(|app| setup_desktop(app).map_err(Into::into));

    #[cfg(mobile)]
    let builder = builder.setup(|app| {
        schedule_research_maintenance(app.handle().clone());
        Ok(())
    });

    builder
        .run(tauri::generate_context!())
        .expect("error while running 股选优");
}

#[cfg(not(mobile))]
fn setup_desktop(app: &mut tauri::App) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("股选优")
        .inner_size(1280.0, 860.0)
        .min_inner_size(960.0, 680.0)
        .visible(false)
        .on_page_load(|window, payload| {
            if matches!(payload.event(), PageLoadEvent::Finished) {
                let _ = window.show();
                let _ = window.set_focus();
            }
        })
        .build()?;

    schedule_research_maintenance(app.handle().clone());

    Ok(())
}

#[cfg(test)]
mod tests;
