use crate::market::cache::records::cached_market_data;
use crate::market::cache::records::market_data_status;
use crate::market::cache::records::read_mobile_market_data;
use crate::market::cache::records::write_mobile_market_data;
use crate::market::refresh::core_mobile_market_data_ingest_tencent_quotes;
use crate::market::store::files::mobile_market_data_path;
use crate::market_domain::codes::normalize_stock_code;
use crate::network::http::build_http_client_with_proxy;
use crate::network::http::probe_mobile_url;
use crate::network::http::TENCENT_NETWORK_PROBE_TIMEOUT_SECS;
use crate::network::proxy::proxy_from_payload;
use crate::network::proxy::proxy_mode_from_payload;
use crate::wire_values::payload_usize_field;
use crate::wire_values::sha256_hex;
use serde_json::json;
use serde_json::Value;
use std::time::Duration;
use stock_optimizer_core as gp_core;

pub(crate) async fn api_market_status(app: tauri::AppHandle) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut status = market_data_status(&app)?;
        status["external_data"] = crate::external_data::status_for_app(&app)?;
        Ok(status)
    })
    .await
    .map_err(|error| format!("market status worker failed: {error}"))?
}

pub(crate) async fn api_market_refresh(
    app: tauri::AppHandle,
    mut payload: Value,
) -> Result<Value, String> {
    use crate::external_data::{DataDomain, Market, RefreshRequest, RefreshRunState, RefreshScope};
    let now = crate::agent_ledger::current_epoch_millis();
    let request_id = payload
        .get("request_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("cn-market-refresh-{now}"));
    if let Some(object) = payload.as_object_mut() {
        object.insert("request_id".into(), Value::String(request_id.clone()));
        object.insert("data_policy".into(), Value::String("refresh".into()));
    }
    let request = RefreshRequest {
        domain: DataDomain::Quote,
        market: Market::China,
        scope: RefreshScope::MarketWide,
        request_id: Some(request_id.clone()),
    };
    let idempotency_key = format!("cn-{}", sha256_hex(request_id.as_bytes()));
    let mut store = crate::external_data::open_app_store(&app)?;
    let (run, is_new) = store.create_refresh_run(&request, &idempotency_key, now)?;
    if !is_new
        && !matches!(
            run.state,
            RefreshRunState::Queued
                | RefreshRunState::Running
                | RefreshRunState::Retryable
                | RefreshRunState::NeedsUserRetry
        )
    {
        let mut prior = serde_json::to_value(run.clone())
            .map_err(|error| format!("encode market refresh status: {error}"))?;
        prior["run_id"] = Value::String(run.run_id);
        return Ok(prior);
    }
    if matches!(
        run.state,
        RefreshRunState::Queued | RefreshRunState::Retryable | RefreshRunState::NeedsUserRetry
    ) {
        store.start_refresh_run(&run.run_id, false, now)?;
    }
    store.record_source_attempt(
        "tencent",
        DataDomain::Quote,
        Market::China,
        &now.to_string(),
    )?;
    drop(store);

    match core_mobile_market_data_refresh_tencent(app.clone(), payload).await {
        Ok(mut result) => {
            result["status"] = market_data_status(&app)?;
            let failed = result
                .get("failed_batches")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                > 0
                || result
                    .get("history_failed")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    > 0;
            let failed = failed
                || result
                    .pointer("/reference_refresh/failed")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
            let terminal = result.get("done").and_then(Value::as_bool).unwrap_or(true)
                || result
                    .get("stopped_early")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
            let state = if !terminal {
                RefreshRunState::Running
            } else if failed {
                RefreshRunState::Retryable
            } else {
                RefreshRunState::Completed
            };
            if terminal {
                crate::external_data::open_app_store(&app)?.finish_refresh_run(
                    &run.run_id,
                    state,
                    crate::agent_ledger::current_epoch_millis(),
                    failed.then_some("partial_provider_failure"),
                    failed.then_some("one or more market batches did not complete"),
                )?;
            }
            if let Some(object) = result.as_object_mut() {
                object.insert("run_id".into(), Value::String(run.run_id));
                object.insert(
                    "state".into(),
                    serde_json::to_value(state).unwrap_or(Value::Null),
                );
            }
            Ok(result)
        }
        Err(error) => {
            let lower = error.to_ascii_lowercase();
            let category = if lower.contains("timeout") || lower.contains("timed out") {
                "timeout"
            } else if lower.contains("429") {
                "http_429"
            } else if lower.contains("http 5") {
                "http_5xx"
            } else if lower.contains("connection") {
                "connection"
            } else {
                "provider_error"
            };
            let retryable = matches!(category, "timeout" | "http_429" | "http_5xx" | "connection");
            let mut store = crate::external_data::open_app_store(&app)?;
            store.record_source_failure(
                "tencent",
                DataDomain::Quote,
                Market::China,
                &crate::agent_ledger::current_epoch_millis().to_string(),
                category,
                &error,
            )?;
            store.finish_refresh_run(
                &run.run_id,
                if retryable {
                    RefreshRunState::Retryable
                } else {
                    RefreshRunState::Failed
                },
                crate::agent_ledger::current_epoch_millis(),
                Some(category),
                Some(&error),
            )?;
            Err(error)
        }
    }
}

pub(crate) fn api_market_ingest_tencent_quotes(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    core_mobile_market_data_ingest_tencent_quotes(app, payload)
}

pub(crate) fn api_market_clear_cache(app: tauri::AppHandle) -> Result<Value, String> {
    let cleared = core_mobile_market_data_clear(app.clone())?;
    Ok(json!({
        "removed_files": if cleared.get("removed").and_then(Value::as_bool).unwrap_or(false) { 1 } else { 0 },
        "removed_bytes": cleared.get("removed_bytes").and_then(Value::as_u64).unwrap_or(0),
        "status": market_data_status(&app)?,
        "notes": cleared.get("notes").cloned().unwrap_or_else(|| json!([]))
    }))
}

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

pub(crate) fn api_data_sources(app: tauri::AppHandle) -> Result<Value, String> {
    Ok(json!({
        "current": "tauri",
        "available": [{"id": "tauri", "name": "Tauri/Rust", "description": "Tauri/Rust native market cache and Tencent refresh path."}],
        "status": market_data_status(&app)?
    }))
}

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

pub(crate) async fn core_validate_data_source(payload: Value) -> Result<Value, String> {
    crate::runtime::run_cpu_bound("core_validate_data_source", move || {
        gp_core::validate_data_source_value(payload).map_err(|error| error.to_string())
    })
    .await?
}

pub(crate) fn core_mobile_market_data_read(app: tauri::AppHandle) -> Result<Value, String> {
    read_mobile_market_data(&app)
}

pub(crate) fn core_mobile_market_data_write(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    write_mobile_market_data(&app, payload)
}

pub(crate) fn core_mobile_market_data_clear(app: tauri::AppHandle) -> Result<Value, String> {
    let path = mobile_market_data_path(&app)?;
    let _ = path;
    Ok(json!({
        "removed": false,
        "removed_bytes": 0,
        "notes": ["external provider data requires a domain-scoped preview and explicit confirmation before cleanup"]
    }))
}

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

pub(crate) async fn core_mobile_market_data_refresh_tencent(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    crate::market::refresh::core_mobile_market_data_refresh_tencent(app, payload).await
}
