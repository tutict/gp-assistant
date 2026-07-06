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
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use stock_optimizer_core as gp_core;
use tauri::{AppHandle, Emitter, Manager};

#[cfg(not(mobile))]
use tauri::{webview::PageLoadEvent, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_shell::ShellExt;

mod news_rag;
mod rag_pack;
mod runtime;

const MOBILE_MARKET_DATA_FILE: &str = "mobile-market-data.json";
const MOBILE_MARKET_WRITE_RETRY_ATTEMPTS: usize = 3;
const MOBILE_MARKET_WRITE_RETRY_DELAY_MS: u64 = 50;
const TENCENT_QUOTE_ENDPOINT: &str = "https://qt.gtimg.cn/q=";
const EASTMONEY_KLINE_ENDPOINT: &str = "https://push2his.eastmoney.com/api/qt/stock/kline/get";
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
const OBSERVE_HISTORY_TOTAL_TIMEOUT_SECS: u64 = 12;
const OBSERVE_HISTORY_TIMEOUT_SECS: u64 = 8;
const OBSERVE_CAPITAL_TOTAL_TIMEOUT_SECS: u64 = 10;
const OBSERVE_CAPITAL_REQUEST_TIMEOUT_SECS: u64 = 6;
const OBSERVE_GUBA_MAX_POSTS: usize = 10;
const MIN_OBSERVE_HISTORY_BARS: usize = 3;
const MIN_FULL_OBSERVE_HISTORY_BARS: usize = 750;
const OBSERVE_DAILY_HISTORY_LIMIT: usize = 10_000;
const FINANCIAL_REQUEST_TIMEOUT_SECS: u64 = 6;
const MAX_TENCENT_WEBVIEW_QUOTE_BYTES: usize = 1_048_576;
const COMPLETE_QUARTERLY_EPS_POINTS: usize = 8;
const THS_FINANCIAL_ENDPOINT: &str =
    "https://basic.10jqka.com.cn/basicapi/finance/index/v1/app_data/";
const SINA_FINANCIAL_GUIDELINE_ENDPOINT: &str =
    "https://money.finance.sina.com.cn/corp/go.php/vFD_FinancialGuideLine";
const DEDUCTED_FINANCIAL_FIELDS: [&str; 3] = [
    "deducted_net_profit_billion",
    "deducted_net_profit_margin",
    "deducted_net_profit_growth_rate",
];

static REFRESH_SEED_CACHE: OnceLock<Mutex<HashMap<PathBuf, Value>>> = OnceLock::new();
static REFRESH_FINANCIAL_SNAPSHOT_CACHE: OnceLock<Mutex<HashMap<PathBuf, Value>>> = OnceLock::new();

#[tauri::command]
async fn api_observe(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    runtime::with_heavy_network_permit("api_observe", api_observe_inner(app, payload)).await
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
    let observe_payload = tokio::time::timeout(
        Duration::from_secs(observe_timeout_secs),
        observe_core_payload_with_cached_history(&app, payload),
    )
    .await;

    match observe_payload {
        Ok(Ok((core_payload, notes))) => {
            match gp_core::observe_with_data_value(core_payload.clone()) {
                Ok(mut result) => {
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
            }
        }
        Ok(Err(error)) => Ok(observe_error_result(
            &Value::Null,
            &fallback_payload,
            vec![format!("观察数据准备失败：{error}")],
        )),
        Err(_) => match observe_core_payload_from_cache(&app, fallback_payload.clone()) {
            Ok(core_payload) => match gp_core::observe_with_data_value(core_payload.clone()) {
                Ok(mut result) => {
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
async fn core_agent_stream_with_data(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let events = runtime::run_cpu_bound("core_agent_stream_with_data", move || {
        gp_core::agent_stream_with_data_events_value(payload).map_err(|error| error.to_string())
    })
    .await??;
    let mut final_response: Option<Value> = None;
    let mut error_message: Option<String> = None;

    for event in events {
        let event_value = serde_json::to_value(&event).map_err(|error| error.to_string())?;
        if event_value.get("type").and_then(Value::as_str) == Some("result") {
            final_response = event_value.get("response").cloned();
        }
        if event_value.get("type").and_then(Value::as_str) == Some("error") {
            error_message = event_value
                .get("message")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
        }
        let _ = app.emit("agent-stream-event", event_value);
    }

    final_response.ok_or_else(|| {
        error_message.unwrap_or_else(|| "agent stream did not produce result".to_string())
    })
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
    let core_payload = core_payload_with_cached_data(&app, "criteria", payload)?;
    runtime::run_cpu_bound("api_screen", move || {
        gp_core::screen_with_data_value(core_payload).map_err(|error| error.to_string())
    })
    .await?
}

#[tauri::command]
async fn api_sector_screen(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let core_payload = core_payload_with_cached_data(&app, "request", payload)?;
    runtime::run_cpu_bound("api_sector_screen", move || {
        gp_core::sector_screen_with_data_value(core_payload).map_err(|error| error.to_string())
    })
    .await?
}

#[tauri::command]
async fn api_custom_screen(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let core_payload = core_payload_with_cached_data(&app, "request", payload)?;
    runtime::run_cpu_bound("api_custom_screen", move || {
        gp_core::graph_screen_with_data_value(core_payload).map_err(|error| error.to_string())
    })
    .await?
}

#[tauri::command]
async fn api_graph_screen(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let core_payload = core_payload_with_cached_data(&app, "request", payload)?;
    runtime::run_cpu_bound("api_graph_screen", move || {
        gp_core::graph_screen_with_data_value(core_payload).map_err(|error| error.to_string())
    })
    .await?
}

#[tauri::command]
async fn api_trend_analyze(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let core_payload = core_payload_with_cached_data(&app, "request", payload)?;
    runtime::run_cpu_bound("api_trend_analyze", move || {
        gp_core::trend_with_data_value(core_payload).map_err(|error| error.to_string())
    })
    .await?
}

#[tauri::command]
async fn api_trend_screen(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let core_payload = core_payload_with_cached_data(&app, "request", payload)?;
    runtime::run_cpu_bound("api_trend_screen", move || {
        gp_core::trend_screen_with_data_value(core_payload).map_err(|error| error.to_string())
    })
    .await?
}

#[tauri::command]
async fn api_backtest(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let core_payload = core_payload_with_cached_data(&app, "request", payload)?;
    runtime::run_cpu_bound("api_backtest", move || {
        gp_core::backtest_with_data_value(core_payload).map_err(|error| error.to_string())
    })
    .await?
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
    runtime::with_heavy_network_permit("api_news_rag", news_rag::api_news_rag_impl(app, payload))
        .await
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
    let data = cached_market_data(&app)?;
    let message = payload
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let run_id = payload
        .get("run_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let mode = payload
        .get("mode")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let mut request = serde_json::Map::new();
    request.insert("data".to_string(), data);
    request.insert("message".to_string(), Value::String(message));
    if let Some(run_id) = run_id {
        request.insert("run_id".to_string(), Value::String(run_id));
    }
    if let Some(mode) = mode {
        request.insert("mode".to_string(), Value::String(mode));
    }
    for key in ["context", "platform", "network"] {
        if let Some(value) = payload.get(key) {
            request.insert(key.to_string(), value.clone());
        }
    }
    core_agent_stream_with_data(app, Value::Object(request)).await
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

pub(crate) fn build_tencent_http_client(
    user_agent: &str,
    timeout: Duration,
) -> Result<reqwest::Client, String> {
    build_http_client_with_proxy(user_agent, timeout, None)
}

pub(crate) fn build_http_client_with_proxy(
    user_agent: &str,
    timeout: Duration,
    payload: Option<&Value>,
) -> Result<reqwest::Client, String> {
    let builder = reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(TENCENT_CONNECT_TIMEOUT_SECS))
        .user_agent(user_agent);
    let builder = apply_android_tls_backend(builder)?;
    apply_payload_proxy(builder, payload)?
        .build()
        .map_err(|error| format!("create HTTP client failed: {error}"))
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

fn apply_payload_proxy(
    builder: reqwest::ClientBuilder,
    payload: Option<&Value>,
) -> Result<reqwest::ClientBuilder, String> {
    let Some(proxy) = proxy_from_payload(payload) else {
        return Ok(builder);
    };
    let reqwest_proxy = reqwest::Proxy::all(&proxy)
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

fn refresh_financial_snapshot_cache() -> &'static Mutex<HashMap<PathBuf, Value>> {
    REFRESH_FINANCIAL_SNAPSHOT_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
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
    } else {
        if let Ok(mut slot) = refresh_seed_cache().lock() {
            slot.clear();
        }
        if let Ok(mut slot) = refresh_financial_snapshot_cache().lock() {
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
        slot.insert(path, snapshot.clone());
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
                return cached.clone();
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
    let Some(snapshot_stocks) = financial_snapshot.get("stocks").and_then(Value::as_array) else {
        return enriched;
    };
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
        let target = enriched.entry(code.clone()).or_insert_with(|| {
            let mut row = serde_json::Map::new();
            row.insert("code".to_string(), json!(code));
            row
        });
        for field in DEDUCTED_FINANCIAL_FIELDS {
            if finite_object_number(target, field).is_some() {
                continue;
            }
            if let Some(value) = finite_object_number(object, field) {
                target.insert(field.to_string(), json!(value));
            }
        }
    }
    enriched
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
        let market_cap = parse_number(values.get(44))
            .filter(|value| *value > 0.0)
            .or_else(|| existing.and_then(|object| object_f64(object, "market_cap_billion")));
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

fn market_quote_cache_stale(generated_at_epoch_ms: Option<u128>, now_epoch_ms: u128) -> bool {
    let Some(quote_date) = generated_at_epoch_ms.and_then(local_yyyymmdd_from_epoch_ms) else {
        return true;
    };
    let Some(expected_date) = expected_market_quote_date_from_epoch_ms(now_epoch_ms) else {
        return true;
    };
    quote_date != expected_date
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
    if let Some(items) = seed.get("histories").and_then(Value::as_object) {
        for (raw_code, history) in items {
            let Some(code) = normalize_stock_code(raw_code) else {
                continue;
            };
            if valid_codes.contains(&code) {
                histories.insert(code, history.clone());
            }
        }
    }
    Value::Object(histories)
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

    // Capital evidence, online EPS, and daily history are independent network groups — fetch them
    // concurrently, then merge each result into `data` sequentially below.
    let capital_fetch_timeout = if mobile_fast_observe {
        12
    } else {
        OBSERVE_CAPITAL_TOTAL_TIMEOUT_SECS
    };
    let (capital_outcome, eps_outcome, history_outcome) = futures::join!(
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
    );

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
        if let Err(error) = write_mobile_market_data_record(app, data.clone(), false) {
            notes.push(format!("观察缓存写入失败：{error}"));
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

    // Guba sentiment and LHB institution stats hit independent endpoints — fetch concurrently.
    let (guba_fetch, lhb_fetch) = futures::join!(
        fetch_eastmoney_guba_sentiment(&client, code),
        fetch_eastmoney_institution_lhb(&client, code, start_date, end_date),
    );
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
    parse_eastmoney_lhb_item(&text, &normalized, &start, &end)
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
                old.get("category")
                    .and_then(Value::as_str)
                    .map(|old_category| !replacement_categories.contains(&old_category))
                    .unwrap_or(true)
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
            "summary": "已接入东方财富股吧情绪与东方财富龙虎榜机构统计，最终分数由 Rust 规则合成。",
            "sections": [],
            "items": merged_items,
            "notes": ["东方财富股吧仅作社区情绪线索；东方财富龙虎榜机构统计为公开机构专用席位口径。"],
        }),
    );
    true
}

async fn http_get_text_with_headers_first(
    client: &reqwest::Client,
    url: &str,
    timeout_secs: u64,
    user_agent: &str,
    referer: &str,
) -> Result<String, String> {
    match powershell_http_get_bytes_with_headers(url, timeout_secs, user_agent, referer) {
        Ok(bytes) => return Ok(decode_utf8_lossy(bytes)),
        Err(powershell_error) => match client
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
        },
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
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("HTTP {}", status.as_u16()));
    }
    let text = response.text().await.map_err(|error| error.to_string())?;
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
    let start = hyphen_date_param(start_date).unwrap_or_else(|| "2020-01-01".to_string());
    let end = hyphen_date_param(end_date).unwrap_or_else(|| "2050-12-31".to_string());
    let param = format!("{symbol},day,{start},{end},{OBSERVE_DAILY_HISTORY_LIMIT},");
    let url = format!(
        "{TENCENT_DAILY_KLINE_ENDPOINT}?param={}",
        param.replace(',', "%2C")
    );
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("HTTP {}", status.as_u16()));
    }
    let text = response.text().await.map_err(|error| error.to_string())?;
    let value: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    Ok(value
        .get("data")
        .and_then(|data| data.get(&symbol))
        .and_then(|stock| stock.get("day").or_else(|| stock.get("qfqday")))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(parse_tencent_kline_row)
        .collect())
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

fn hyphen_date_param(raw: &str) -> Option<String> {
    normalize_history_date(raw)
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

fn core_payload_with_cached_data(
    app: &tauri::AppHandle,
    payload_key: &str,
    payload: Value,
) -> Result<Value, String> {
    let data = cached_market_data(app)?;
    let mut request = serde_json::Map::new();
    request.insert("data".to_string(), data);
    request.insert(payload_key.to_string(), payload);
    Ok(Value::Object(request))
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
    let stale = market_quote_cache_stale(generated_at_epoch_ms, now_epoch_ms);
    if stale {
        notes.push(
            "cached Tencent quote date is stale; refresh before rotation screening".to_string(),
        );
    }
    Ok(json!({
        "source": "tencent",
        "universe_count": stock_count,
        "cache_bytes": cache.get("bytes").and_then(Value::as_u64).unwrap_or(0),
        "cache_limit_bytes": 0,
        "universe_updated_at": updated_at_epoch_ms.map(|value| json!(value)).unwrap_or(Value::Null),
        "quote_generated_at": generated_at_epoch_ms.map(|value| json!(value)).unwrap_or(Value::Null),
        "quote_trade_date": quote_date,
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
        return Ok(json!({
            "exists": false,
            "bytes": 0,
            "path": path.display().to_string(),
            "notes": ["mobile market cache is empty"]
        }));
    }
    let data = read_json_file(&path)?;
    let summary = gp_core::validate_data_source_value(data.clone())
        .map_err(|error| format!("cached mobile market data is invalid: {error}"))?;
    let bytes = fs::metadata(&path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    Ok(market_cache_record(
        &path,
        bytes,
        file_modified_millis(&path),
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
    let summary = gp_core::validate_data_source_value(payload.clone())
        .map_err(|error| format!("mobile market data validation failed: {error}"))?;
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
    record.insert("data_notes".to_string(), data_notes);
    if include_data {
        record.insert("data".to_string(), data.clone());
    }
    record.insert("notes".to_string(), json!([note]));
    Value::Object(record)
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
            core_agent_stream_with_data,
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
            api_news_rag,
            api_rag_pack_status,
            api_rag_pack_build,
            api_rag_pack_build_from_news_cache,
            api_rag_pack_query,
            api_upstream_rag_status,
            api_upstream_rag_build,
            api_upstream_rag_transfer_start,
            api_agent_stream,
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
    let builder = builder.setup(|_| Ok(()));

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

    Ok(())
}

#[cfg(test)]
mod tests;
