use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use stock_optimizer_core as gp_core;

use crate::market::{EASTMONEY_DATACENTER_ENDPOINT, EASTMONEY_SECURITIES_ENDPOINT, TENCENT_BATCH_TIMEOUT_SECS};

pub(crate) const EASTMONEY_FUND_FLOW_ENDPOINT: &str =
    "https://push2his.eastmoney.com/api/qt/stock/fflow/daykline/get";

pub(crate) const OBSERVE_TOTAL_TIMEOUT_SECS: u64 = 25;

pub(crate) const OBSERVE_MOBILE_FAST_TOTAL_TIMEOUT_SECS: u64 = 35;

pub(crate) const OBSERVE_FINANCIAL_TOTAL_TIMEOUT_SECS: u64 = 10;

pub(crate) const OBSERVE_FUNDAMENTAL_TOTAL_TIMEOUT_SECS: u64 = 8;

pub(crate) const OBSERVE_FUNDAMENTAL_REFRESH_INTERVAL_MS: u128 = 24 * 60 * 60 * 1_000;

pub(crate) const OBSERVE_HISTORY_TOTAL_TIMEOUT_SECS: u64 = 12;

pub(crate) const OBSERVE_CAPITAL_TOTAL_TIMEOUT_SECS: u64 = 10;

pub(crate) const OBSERVE_CAPITAL_REQUEST_TIMEOUT_SECS: u64 = 6;

pub(crate) const OBSERVE_LHB_SEAT_REQUEST_TIMEOUT_SECS: u64 = 3;

pub(crate) const OBSERVE_GUBA_MAX_POSTS: usize = 10;

pub(crate) const MIN_OBSERVE_HISTORY_BARS: usize = 3;

pub(crate) const MIN_FULL_OBSERVE_HISTORY_BARS: usize = 750;

#[tauri::command]
pub(crate) async fn api_observe(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    Box::pin(api_observe_inner(app, payload)).await
}

pub(crate) async fn api_observe_inner(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
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
    let observe_network = Box::pin(crate::runtime::with_heavy_network_permit(
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

pub(crate) async fn run_observe_calculation(core_payload: &Value) -> Result<Value, String> {
    let payload = core_payload.clone();
    crate::runtime::run_cpu_bound("api_observe_calculation", move || {
        gp_core::observe_with_data_value(payload).map_err(|error| error.to_string())
    })
    .await?
}

pub(crate) fn enrich_observe_stock_quote_fields(result: &mut Value, core_payload: &Value) {
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
        .and_then(crate::market::normalize_stock_code);
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
                    .and_then(crate::market::normalize_stock_code)
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

pub(crate) fn observe_needs_exact_share_refresh(data: &Value, code: &str) -> bool {
    crate::market::stock_object(data, code).is_none_or(|stock| {
        ["total_shares", "circulating_shares"]
            .iter()
            .any(|field| crate::market::object_f64(stock, field).is_none_or(|value| value <= 0.0))
    })
}

pub(crate) fn observe_needs_fundamental_supplement(data: &Value, code: &str) -> bool {
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
            .and_then(|item| crate::market::finite_object_number(item, field))
            .is_none()
    });
    if missing {
        return true;
    }
    let updated_at =
        entry.and_then(|item| crate::market::cache_epoch_ms(item.get("supplement_updated_at_epoch_ms")));
    updated_at.is_none_or(|updated_at| {
        crate::market::epoch_millis().saturating_sub(updated_at) > OBSERVE_FUNDAMENTAL_REFRESH_INTERVAL_MS
    })
}

pub(crate) async fn fetch_observe_quote_snapshot(
    code: &str,
    seed_stock: Option<serde_json::Map<String, Value>>,
    payload: &Value,
) -> Result<serde_json::Map<String, Value>, String> {
    let client = crate::market::build_http_client_with_proxy(
        "Mozilla/5.0 GuXuanYou/0.3 observe-quote",
        Duration::from_secs(crate::market::TENCENT_REQUEST_TIMEOUT_SECS),
        Some(payload),
    )?;
    let quote = crate::market::fetch_tencent_quotes(
        &client,
        &[code.to_string()],
        Duration::from_secs(crate::market::TENCENT_REQUEST_TIMEOUT_SECS),
    )
    .await?;
    let seed = HashMap::from([(code.to_string(), seed_stock.unwrap_or_default())]);
    crate::market::parse_tencent_quotes(&quote.text, &seed, false)
        .into_iter()
        .find(|stock| {
            let code_matches = stock
                .get("code")
                .and_then(Value::as_str)
                .and_then(crate::market::normalize_stock_code)
                .is_some_and(|parsed| parsed == code);
            let has_exact_shares = ["total_shares", "circulating_shares"]
                .iter()
                .all(|field| crate::market::object_f64(stock, field).is_some_and(|value| value > 0.0));
            code_matches && has_exact_shares
        })
        .ok_or_else(|| format!("Tencent quote did not return exact share data for {code}"))
}

pub(crate) fn parse_goodwill_to_net_assets(value: &Value) -> Option<f64> {
    let row = crate::market::eastmoney_result_rows(value).first()?.as_object()?;
    let parent_equity = crate::market::json_f64(row.get("TOTAL_PARENT_EQUITY")).filter(|value| *value > 0.0)?;
    let goodwill = crate::market::json_f64(row.get("GOODWILL")).unwrap_or(0.0).max(0.0);
    Some(goodwill / parent_equity * 100.0)
}

pub(crate) fn parse_latest_pledged_share_ratio(value: &Value) -> Option<f64> {
    let rows = crate::market::eastmoney_result_rows(value);
    if rows.is_empty() {
        return Some(0.0);
    }
    rows.first()
        .and_then(Value::as_object)
        .and_then(|row| crate::market::json_f64(row.get("PLEDGE_RATIO")))
}

pub(crate) fn parse_latest_dividend_metrics(value: &Value, price: Option<f64>) -> (Option<f64>, Option<f64>) {
    let rows = crate::market::eastmoney_result_rows(value);
    if rows.is_empty() {
        return (Some(0.0), Some(0.0));
    }
    let row = latest_dividend_row(value);
    let Some(row) = row else {
        return (None, None);
    };
    let cash_per_share = crate::market::json_f64(row.get("PRETAX_BONUS_RMB"))
        .unwrap_or(0.0)
        .max(0.0)
        / 10.0;
    let dividend_yield = price
        .filter(|value| *value > 0.0)
        .map(|value| cash_per_share / value * 100.0)
        .or_else(|| crate::market::json_f64(row.get("DIVIDENT_RATIO")).map(|value| value * 100.0));
    let payout_ratio = crate::market::json_f64(row.get("BASIC_EPS"))
        .filter(|value| *value > 0.0)
        .map(|eps| cash_per_share / eps * 100.0);
    (dividend_yield, payout_ratio)
}

pub(crate) fn latest_dividend_row(value: &Value) -> Option<&serde_json::Map<String, Value>> {
    let rows = crate::market::eastmoney_result_rows(value);
    rows.iter()
        .filter_map(Value::as_object)
        .find(|row| {
            row.get("EX_DIVIDEND_DATE")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
        })
        .or_else(|| rows.first().and_then(Value::as_object))
}

pub(crate) async fn fetch_observe_fundamental_supplement(
    code: &str,
    price: Option<f64>,
    payload: &Value,
) -> Result<(serde_json::Map<String, Value>, Vec<String>), String> {
    let digits = code
        .get(..6)
        .filter(|value| value.chars().all(|ch| ch.is_ascii_digit()))
        .ok_or_else(|| format!("invalid stock code for fundamentals: {code}"))?;
    let client = crate::market::build_http_client_with_proxy(
        "Mozilla/5.0 GuXuanYou/0.3 observe-fundamentals",
        Duration::from_secs(OBSERVE_FUNDAMENTAL_TOTAL_TIMEOUT_SECS),
        Some(payload),
    )?;
    let direct_client = crate::market::build_direct_http_client(
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
        crate::market::fetch_eastmoney_public_json_with_direct_retry(
            &client,
            &direct_client,
            &balance_url,
            "Eastmoney balance sheet",
            false,
        ),
        crate::market::fetch_eastmoney_public_json_with_direct_retry(
            &client,
            &direct_client,
            &pledge_url,
            "Eastmoney pledge ratio",
            true,
        ),
        crate::market::fetch_eastmoney_public_json_with_direct_retry(
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
            if let Some(period) = crate::market::eastmoney_metric_period(
                crate::market::eastmoney_result_rows(&value)
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
            if let Some(period) = crate::market::eastmoney_metric_period(
                crate::market::eastmoney_result_rows(&value)
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
                crate::market::eastmoney_metric_period(latest_dividend_row(&value), &["REPORT_DATE"])
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

pub(crate) fn merge_observe_quote_snapshot(
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
                    .and_then(crate::market::normalize_stock_code)
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

pub(crate) fn merge_observe_fundamental_supplement(
    data: &mut Value,
    code: &str,
    fields: serde_json::Map<String, Value>,
) -> bool {
    let entry = crate::market::financial_entry_mut(data, code);
    let mut changed = false;
    for (field, value) in fields {
        let valid_number = crate::market::json_f64(Some(&value)).is_some();
        let valid_period = field.ends_with("_period")
            && value.as_str().is_some_and(|value| !value.trim().is_empty());
        if (valid_number || valid_period) && entry.get(&field) != Some(&value) {
            entry.insert(field, value);
            changed = true;
        }
    }
    if !entry.is_empty() {
        let updated_at = json!(crate::market::epoch_millis().to_string());
        if entry.get("supplement_updated_at_epoch_ms") != Some(&updated_at) {
            entry.insert("supplement_updated_at_epoch_ms".to_string(), updated_at);
            changed = true;
        }
        crate::market::append_financial_source(entry, "东方财富财报/质押/分红公开数据");
    }
    changed
}

pub(crate) fn merge_observe_financial_snapshot(data: &mut Value, code: &str, snapshot: &Value) -> bool {
    let mut entries = serde_json::Map::new();
    if let Some(existing) = data
        .get("financials")
        .and_then(Value::as_object)
        .and_then(|financials| financials.get(code))
        .cloned()
    {
        entries.insert(code.to_string(), existing);
    }
    crate::market::merge_financials_object(&mut entries, snapshot.get("financials"));
    crate::market::merge_financials_array(&mut entries, snapshot.get("stocks"));
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
    let entry = crate::market::financial_entry_mut(data, code);
    if let Some(next_object) = next.as_object() {
        for (key, value) in next_object {
            entry.insert(key.clone(), value.clone());
        }
    }
    true
}

pub(crate) async fn observe_core_payload_with_cached_history(
    app: &tauri::AppHandle,
    payload: Value,
) -> Result<(Value, Vec<String>), String> {
    let mut data = crate::market::cached_market_data(app)?;
    let code = payload
        .get("code")
        .and_then(Value::as_str)
        .and_then(crate::market::normalize_stock_code)
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
        let before = crate::market::financial_quarterly_eps_count(&data, &code);
        if merge_observe_financial_snapshot(&mut data, &code, snapshot) {
            data_changed = true;
            let after = crate::market::financial_quarterly_eps_count(&data, &code);
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

    let payload_financial_points = crate::market::normalize_quarterly_eps(payload.get("financial_eps_points"));
    if !payload_financial_points.is_empty() {
        let before = crate::market::financial_quarterly_eps_count(&data, &code);
        let provided_count = payload_financial_points.len();
        crate::market::merge_quarterly_eps_points(&mut data, &code, payload_financial_points);
        let after = crate::market::financial_quarterly_eps_count(&data, &code);
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
                    crate::market::truncate_for_note(note.trim(), 240)
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
        if crate::market::merge_basic_financial_from_stock(&mut data, &code) {
            data_changed = true;
        }
        notes
            .push("移动端快速观察：已使用本地财务快照，跳过同花顺/新浪在线 EPS 补全。".to_string());
        financial_cached_count = 0;
        financial_current_count = 0;
        need_eps = false;
    } else {
        financial_cached_count = crate::market::financial_quarterly_eps_count(&data, &code);
        if crate::market::merge_basic_financial_from_stock(&mut data, &code) {
            data_changed = true;
        }
        financial_current_count = crate::market::financial_quarterly_eps_count(&data, &code);
        need_eps = financial_current_count < crate::market::COMPLETE_QUARTERLY_EPS_POINTS;
    }

    // History: decide whether an online daily-history fetch is needed (WebView-provided rows and
    // a sufficiently stocked local cache both make it unnecessary).
    let requested_series_limit = crate::market::payload_usize_field(
        &payload,
        "series_limit",
        120,
        20,
        crate::market::OBSERVE_DAILY_HISTORY_LIMIT,
    );
    let required_cached_history_bars = if requested_series_limit > 500 {
        MIN_FULL_OBSERVE_HISTORY_BARS
    } else {
        MIN_OBSERVE_HISTORY_BARS
    };
    let webview_history_rows = crate::market::payload_history_rows(&payload);
    let cache_lacks_history = webview_history_rows.is_none()
        && !crate::market::history_cache_has_bars(
            &data,
            &code,
            &start_date,
            &end_date,
            required_cached_history_bars,
        );
    let need_history =
        cache_lacks_history && (!mobile_fast_observe || requested_series_limit > 500);
    let need_exact_share_refresh = observe_needs_exact_share_refresh(&data, &code);
    let quote_seed_stock = crate::market::stock_object(&data, &code).cloned();
    let stock_price = quote_seed_stock
        .as_ref()
        .and_then(|stock| crate::market::object_f64(stock, "price"));
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
                        crate::market::fetch_quarterly_eps_chain(&code),
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
                        crate::market::fetch_observe_daily_history(&code, &start_date, &end_date),
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
                        Duration::from_secs(crate::market::TENCENT_BATCH_TIMEOUT_SECS),
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
            _ => crate::market::QuarterlyEpsFetchResult {
                points: Vec::new(),
                sources: Vec::new(),
                errors: vec![format!(
                    "在线财报补全超过 {OBSERVE_FINANCIAL_TOTAL_TIMEOUT_SECS} 秒，已跳过同花顺/新浪补充。"
                )],
            },
        };
        if !fetch.points.is_empty() {
            let before = crate::market::financial_quarterly_eps_count(&data, &code);
            crate::market::merge_quarterly_eps_points(&mut data, &code, fetch.points);
            let after = crate::market::financial_quarterly_eps_count(&data, &code);
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
            if crate::market::push_financial_note(&mut data, &code, note.clone()) {
                data_changed = true;
            }
            notes.push(note);
        } else if financial_current_count < crate::market::COMPLETE_QUARTERLY_EPS_POINTS {
            let detail = if fetch.errors.is_empty() {
                "在线财报源没有返回可用 EPS 行".to_string()
            } else {
                fetch.errors.join("；")
            };
            let note = format!(
                "季度 EPS 明细仍不足：本地缓存 {financial_cached_count} 期，通达信基础财务 {financial_current_count} 期；已尝试同花顺和新浪财经，{detail}。"
            );
            if crate::market::push_financial_note(&mut data, &code, note.clone()) {
                data_changed = true;
            }
            notes.push(note);
        }
    }

    // Merge daily history.
    if let Some(rows) = webview_history_rows {
        let count = rows.len();
        crate::market::insert_history_rows(&mut data, &code, rows);
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
                    crate::market::insert_history_rows(&mut data, &code, rows);
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
            crate::market::persist_market_data_updates(app.clone(), persist_data, vec![code.clone()]).await
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

pub(crate) fn observe_core_payload_from_cache(
    app: &tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let data = crate::market::cached_market_data(app)?;
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

pub(crate) fn observe_error_result(
    core_payload: &Value,
    request_payload: &Value,
    notes: Vec<String>,
) -> Value {
    let code = request_payload
        .get("code")
        .and_then(Value::as_str)
        .and_then(crate::market::normalize_stock_code)
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
                    .and_then(crate::market::normalize_stock_code)
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

pub(crate) fn append_observe_note(result: &mut Value, note: String) {
    if let Some(notes) = result.get_mut("notes").and_then(Value::as_array_mut) {
        notes.push(Value::String(note));
    } else if let Some(object) = result.as_object_mut() {
        object.insert("notes".to_string(), Value::Array(vec![Value::String(note)]));
    }
}

pub(crate) async fn fetch_observe_capital_evidence_items(
    code: &str,
    start_date: &str,
    end_date: &str,
    network_payload: Option<&Value>,
) -> (Vec<Value>, Vec<String>) {
    let mut notes = Vec::new();
    let mut items = Vec::new();
    let timeout = Duration::from_secs(OBSERVE_CAPITAL_REQUEST_TIMEOUT_SECS);
    let client = match crate::market::build_http_client_with_proxy(
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
                crate::market::truncate_for_note(&error, 180)
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
                crate::market::truncate_for_note(&error, 180)
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
                crate::market::truncate_for_note(&error, 180)
            );
            items.push(eastmoney_lhb_unavailable_item(
                code, start_date, end_date, &detail,
            ));
            notes.push(detail);
        }
    }

    (items, notes)
}

pub(crate) async fn fetch_eastmoney_main_fund_flow(
    client: &reqwest::Client,
    code: &str,
    end_date: &str,
) -> Result<Value, String> {
    let normalized =
        crate::market::normalize_stock_code(code).ok_or_else(|| format!("无效资金流股票代码：{code}"))?;
    let digits = normalized
        .get(..6)
        .ok_or_else(|| format!("无效资金流股票代码：{code}"))?;
    let market = crate::market::eastmoney_market_code(&normalized)
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
    let text = crate::market::http_get_text_with_headers_first(
        client,
        &url.to_string(),
        OBSERVE_CAPITAL_REQUEST_TIMEOUT_SECS,
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        "https://data.eastmoney.com/zjlx/detail.html",
    )
    .await?;
    parse_eastmoney_main_fund_flow_item(&text, &normalized, end_date)
}

pub(crate) fn parse_eastmoney_main_fund_flow_item(
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
    let requested_end = crate::market::normalize_history_date(end_date);
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
    let involvement = crate::market::main_fund_involvement(net_ratio);
    let score = main_fund_flow_score(net_ratio);
    let conclusion = main_fund_flow_plain_conclusion(net_ratio);
    Ok(json!({
        "category": "fund_flow",
        "source": "东方财富个股资金流",
        "title": "当日主力资金流",
        "date": trade_date,
        "metrics": {
            "主力净流入额": crate::market::format_amount_wan(net_amount),
            "主力净流入额原值": format!("{net_amount:.2}"),
            "主力净占比": format!("{net_ratio:.2}%"),
            "主力介入度": format!("{}（{:.2}%）", involvement, net_ratio.abs()),
            "介入度口径": "按主力净占比绝对值分档：低 <3%，中 3%-8%，高 >=8%",
            "通俗结论": conclusion,
            "证据类型": "外部个股资金流",
        },
        "sentiment": crate::market::score_sentiment_label(score),
        "weight": 0.35,
        "confidence": "中",
        "url": "https://data.eastmoney.com/zjlx/detail.html",
        "score": crate::market::round2_value(score),
        "note": "东方财富个股资金流最新交易日口径；主力介入度由主力净占比绝对值分档，高介入只表示主力交易影响较大，不代表方向利好。",
    }))
}

pub(crate) fn parse_eastmoney_main_fund_flow_row(raw: &str) -> Option<(String, f64, f64)> {
    let parts = raw.split(',').collect::<Vec<_>>();
    if parts.len() < 7 {
        return None;
    }
    Some((
        crate::market::normalize_history_date(parts.first().copied()?)?,
        crate::market::parse_f64_str(parts.get(1).copied()?)?,
        crate::market::parse_f64_str(parts.get(6).copied()?)?,
    ))
}

pub(crate) fn main_fund_flow_score(net_ratio: f64) -> f64 {
    crate::market::round2_value((50.0 + net_ratio.clamp(-16.0, 16.0) * 2.5).clamp(10.0, 90.0))
}

pub(crate) fn main_fund_flow_plain_conclusion(net_ratio: f64) -> String {
    let magnitude = net_ratio.abs();
    let direction = if net_ratio > 0.05 {
        "净买入"
    } else if net_ratio < -0.05 {
        "净卖出"
    } else {
        "净流入接近持平"
    };
    let strength = match crate::market::main_fund_involvement(net_ratio) {
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

pub(crate) fn eastmoney_fund_flow_unavailable_item(code: &str, end_date: &str, detail: &str) -> Value {
    json!({
        "category": "fund_flow_status",
        "source": "东方财富个股资金流",
        "title": "当日主力资金流暂不可用",
        "date": crate::market::normalize_history_date(end_date),
        "metrics": {
            "状态": "接口不可用",
            "查询截至": crate::market::normalize_history_date(end_date).unwrap_or_else(|| end_date.to_string()),
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

pub(crate) async fn fetch_eastmoney_guba_sentiment(
    client: &reqwest::Client,
    code: &str,
) -> Result<Vec<Value>, String> {
    let normalized =
        crate::market::normalize_stock_code(code).ok_or_else(|| format!("无效股吧股票代码：{code}"))?;
    let digits = normalized
        .get(..6)
        .ok_or_else(|| format!("无效股吧股票代码：{code}"))?;
    let url = format!("https://guba.eastmoney.com/list,{digits}.html");
    let text = crate::market::http_get_text_with_headers_first(
        client,
        &url,
        OBSERVE_CAPITAL_REQUEST_TIMEOUT_SECS,
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        "https://guba.eastmoney.com/",
    )
    .await?;
    Ok(parse_eastmoney_guba_items(&text, &normalized, &url))
}

pub(crate) fn parse_eastmoney_guba_items(html: &str, code: &str, list_url: &str) -> Vec<Value> {
    let Some(raw) = crate::market::extract_json_after_var(html, "article_list") else {
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
        let title = crate::market::object_string_any(object, &["post_title"])
            .map(|value| crate::market::clean_html_text(&value))
            .unwrap_or_default();
        if title.trim().is_empty() || !seen.insert(title.clone()) {
            continue;
        }
        let summary = guba_post_summary(object, &title);
        let date = crate::market::object_string_any(object, &["post_publish_time", "post_display_time"])
            .and_then(|value| normalize_guba_datetime(&value));
        let post_id = crate::market::object_string_any(object, &["post_id"]).unwrap_or_default();
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
            "评论数": crate::market::object_number_any_loose(object, &["post_comment_count"]).map(crate::market::compact_count).unwrap_or_else(|| "0".to_string()),
            "阅读数": crate::market::object_number_any_loose(object, &["post_click_count"]).map(crate::market::compact_count).unwrap_or_else(|| "0".to_string()),
            "多空标记": guba_bullish_bearish_label(object.get("bullish_bearish")),
            "证据分": format!("{score:.1}"),
        });
        items.push(json!({
            "category": "community_sentiment",
            "source": "东方财富股吧",
            "title": title,
            "date": date,
            "metrics": metrics,
            "sentiment": crate::market::score_sentiment_label(score),
            "weight": 0.15,
            "confidence": "低",
            "url": url,
            "score": crate::market::round2_value(score),
            "note": format!("东方财富股吧帖子：{}。社区讨论只作情绪/传闻信号，不直接作为买卖结论。", crate::market::truncate_for_note(&summary, 120)),
        }));
    }
    items
}

pub(crate) fn guba_post_heat(value: &Value) -> f64 {
    let Some(object) = value.as_object() else {
        return 0.0;
    };
    let clicks = crate::market::object_number_any_loose(object, &["post_click_count"]).unwrap_or(0.0);
    let comments = crate::market::object_number_any_loose(object, &["post_comment_count"]).unwrap_or(0.0);
    let likes =
        crate::market::object_number_any_loose(object, &["post_like_count", "post_forward_count"]).unwrap_or(0.0);
    clicks + comments * 25.0 + likes * 8.0
}

pub(crate) fn guba_post_summary(object: &serde_json::Map<String, Value>, title: &str) -> String {
    let mut parts = Vec::new();
    if let Some(nickname) = crate::market::object_string_any(object, &["user_nickname"]) {
        let nickname = crate::market::clean_html_text(&nickname);
        if !nickname.is_empty() {
            parts.push(format!("作者 {nickname}"));
        }
    }
    if let Some(clicks) = crate::market::object_number_any_loose(object, &["post_click_count"]) {
        parts.push(format!("阅读 {}", crate::market::compact_count(clicks)));
    }
    if let Some(comments) = crate::market::object_number_any_loose(object, &["post_comment_count"]) {
        parts.push(format!("评论 {}", crate::market::compact_count(comments)));
    }
    if parts.is_empty() {
        title.to_string()
    } else {
        parts.join("，")
    }
}

pub(crate) async fn fetch_eastmoney_institution_lhb(
    client: &reqwest::Client,
    code: &str,
    start_date: &str,
    end_date: &str,
) -> Result<Value, String> {
    let normalized =
        crate::market::normalize_stock_code(code).ok_or_else(|| format!("无效龙虎榜股票代码：{code}"))?;
    let digits = normalized
        .get(..6)
        .ok_or_else(|| format!("无效龙虎榜股票代码：{code}"))?;
    let start = crate::market::normalize_history_date(start_date).unwrap_or_else(|| fallback_lhb_start_date());
    let end = crate::market::normalize_history_date(end_date).unwrap_or_else(|| crate::market::fallback_today_date());
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
    let text = crate::market::http_get_text_with_headers_first(
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
            format!("卖方席位明细暂不可用：{}", crate::market::truncate_for_note(&error, 120)),
        ),
        (Err(error), Ok(sell)) => (
            merge_eastmoney_lhb_seats(Vec::new(), sell),
            "partial",
            format!("买方席位明细暂不可用：{}", crate::market::truncate_for_note(&error, 120)),
        ),
        (Err(buy_error), Err(sell_error)) => (
            Vec::new(),
            "unavailable",
            format!(
                "席位明细暂不可用：买方 {}；卖方 {}",
                crate::market::truncate_for_note(&buy_error, 80),
                crate::market::truncate_for_note(&sell_error, 80)
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

pub(crate) fn parse_eastmoney_lhb_item(
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
        let row_code = crate::market::object_string_any(object, &["SECURITY_CODE"]);
        if row_code.as_deref() != Some(digits) {
            continue;
        }
        let net = crate::market::object_number_any_loose(object, &["NET_BUY_AMT"])
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
    let buy = crate::market::object_number_any_loose(row, &["BUY_AMT"]).unwrap_or(0.0);
    let sell = crate::market::object_number_any_loose(row, &["SELL_AMT"]).unwrap_or(0.0);
    let net = crate::market::object_number_any_loose(row, &["NET_BUY_AMT"]).unwrap_or(buy - sell);
    let ratio = crate::market::object_number_any_loose(row, &["RATIO"]);
    let score = institution_lhb_score(net, buy, sell, ratio);
    let trade_date =
        crate::market::object_string_any(row, &["TRADE_DATE"]).and_then(|value| crate::market::normalize_history_date(&value));
    let reason =
        crate::market::object_string_any(row, &["EXPLANATION"]).unwrap_or_else(|| "龙虎榜机构统计".to_string());
    Ok(json!({
        "category": "institution_lhb",
        "source": "东方财富龙虎榜机构统计",
        "title": "东方财富龙虎榜机构席位",
        "date": trade_date,
        "metrics": {
            "机构买入额": crate::market::format_amount_wan(buy),
            "机构卖出额": crate::market::format_amount_wan(sell),
            "机构净买额": crate::market::format_amount_wan(net),
            "机构买卖比": crate::market::institution_buy_sell_ratio(buy, sell),
            "净买额占成交额比": ratio.map(|value| format!("{}%", crate::market::format_number_like(value))).unwrap_or_else(|| "-".to_string()),
            "买方机构数": crate::market::object_number_any_loose(row, &["BUY_TIMES", "BUY_COUNT"]).map(crate::market::format_number_like).unwrap_or_else(|| "-".to_string()),
            "卖方机构数": crate::market::object_number_any_loose(row, &["SELL_TIMES", "SELL_COUNT"]).map(crate::market::format_number_like).unwrap_or_else(|| "-".to_string()),
            "上榜原因": reason,
            "证据分": format!("{score:.1}"),
        },
        "sentiment": crate::market::score_sentiment_label(score),
        "weight": 0.25,
        "confidence": "高",
        "url": "https://data.eastmoney.com/stock/jgmmtj.html",
        "score": crate::market::round2_value(score),
        "note": "东方财富龙虎榜机构买卖每日统计；口径为公开龙虎榜机构专用席位，不等同于全部机构持仓变化。",
    }))
}

#[derive(Clone, Debug, Default)]
pub(crate) struct EastmoneyLhbSeatRow {
    pub(crate) key: String,
    pub(crate) seat_code: Option<String>,
    pub(crate) name: String,
    pub(crate) trade_date: Option<String>,
    pub(crate) buy_amount: Option<f64>,
    pub(crate) sell_amount: Option<f64>,
    pub(crate) buy_ratio: Option<f64>,
    pub(crate) sell_ratio: Option<f64>,
    pub(crate) change_rate: Option<f64>,
    pub(crate) reason: Option<String>,
    pub(crate) three_day_rise_probability: Option<f64>,
    pub(crate) three_day_activity_count: Option<f64>,
}

pub(crate) async fn fetch_eastmoney_lhb_seat_side(
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
        crate::market::EASTMONEY_DATACENTER_ENDPOINT,
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
    let text = crate::market::http_get_text_with_headers_first(
        client,
        &url.to_string(),
        OBSERVE_LHB_SEAT_REQUEST_TIMEOUT_SECS,
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        "https://data.eastmoney.com/stock/tradedetail.html",
    )
    .await?;
    parse_eastmoney_lhb_seat_side(&text, code, buy_side)
}

pub(crate) fn parse_eastmoney_lhb_seat_side(
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
        if crate::market::object_string_any(object, &["SECURITY_CODE"]).as_deref() != Some(code) {
            continue;
        }
        let Some(name) = crate::market::object_string_any(object, &["OPERATEDEPT_NAME"]) else {
            continue;
        };
        let seat_code = crate::market::object_string_any(object, &["OPERATEDEPT_CODE"]);
        let key = seat_code.clone().unwrap_or_else(|| name.clone());
        parsed.push(EastmoneyLhbSeatRow {
            key,
            seat_code,
            name,
            trade_date: crate::market::object_string_any(object, &["TRADE_DATE"])
                .and_then(|value| crate::market::normalize_history_date(&value)),
            buy_amount: buy_side
                .then(|| crate::market::object_number_any_loose(object, &["BUY"]))
                .flatten(),
            sell_amount: (!buy_side)
                .then(|| crate::market::object_number_any_loose(object, &["SELL"]))
                .flatten(),
            buy_ratio: buy_side
                .then(|| crate::market::object_number_any_loose(object, &["TOTAL_BUYRIO", "TOTAL_BUY_RATIO"]))
                .flatten(),
            sell_ratio: (!buy_side)
                .then(|| crate::market::object_number_any_loose(object, &["TOTAL_SELLRIO", "TOTAL_SELL_RATIO"]))
                .flatten(),
            change_rate: crate::market::object_number_any_loose(object, &["CHANGE_RATE"]),
            reason: crate::market::object_string_any(object, &["EXPLANATION"]),
            three_day_rise_probability: crate::market::object_number_any_loose(object, &["RISE_PROBABILITY_3DAY"]),
            three_day_activity_count: crate::market::object_number_any_loose(
                object,
                &["TOTAL_BUYER_SALESTIMES_3DAY", "TOTAL_SELLER_BUYTIMES_3DAY"],
            ),
        });
    }
    Ok(parsed)
}

pub(crate) fn merge_eastmoney_lhb_seats(
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

pub(crate) fn eastmoney_lhb_seat_value(row: EastmoneyLhbSeatRow) -> Value {
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

pub(crate) fn guba_status_item(code: &str, end_date: &str, detail: &str) -> Value {
    json!({
        "category": "community_sentiment",
        "source": "东方财富股吧",
        "title": "东方财富股吧暂无可用情绪证据",
        "date": crate::market::normalize_history_date(end_date),
        "metrics": {
            "状态": detail,
            "查询窗口": crate::market::normalize_history_date(end_date).unwrap_or_else(|| end_date.to_string()),
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

pub(crate) fn guba_list_url(code: &str) -> String {
    let digits = crate::market::normalize_stock_code(code)
        .and_then(|value| value.get(..6).map(ToOwned::to_owned))
        .unwrap_or_else(|| {
            code.chars()
                .filter(|ch| ch.is_ascii_digit())
                .take(6)
                .collect()
        });
    format!("https://guba.eastmoney.com/list,{digits}.html")
}

pub(crate) fn eastmoney_lhb_unavailable_item(
    code: &str,
    start_date: &str,
    end_date: &str,
    detail: &str,
) -> Value {
    json!({
        "category": "institution_lhb_status",
        "source": "东方财富龙虎榜机构统计",
        "title": "东方财富龙虎榜机构统计不可用",
        "date": crate::market::normalize_history_date(end_date),
        "metrics": {
            "状态": "接口不可用",
            "查询窗口": format!("{} - {}", crate::market::normalize_history_date(start_date).unwrap_or_else(|| start_date.to_string()), crate::market::normalize_history_date(end_date).unwrap_or_else(|| end_date.to_string())),
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

pub(crate) fn eastmoney_lhb_no_hit_item(code: &str, start_date: &str, end_date: &str) -> Value {
    let days = crate::market::window_days(start_date, end_date);
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

pub(crate) fn merge_capital_evidence_items(
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
            "generated_at": crate::market::epoch_millis().to_string(),
            "composite_score": Value::Null,
            "confidence": "中",
            "model_used": false,
            "as_of_trade_date": crate::market::normalize_history_date(end_date),
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

pub(crate) fn is_local_fund_flow_proxy_value(item: &Value) -> bool {
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

pub(crate) fn normalize_guba_datetime(value: &str) -> Option<String> {
    let text = value.trim();
    if text.len() >= 10 {
        crate::market::normalize_history_date(&text[..10])
    } else {
        None
    }
}

pub(crate) fn guba_sentiment_score(
    title: &str,
    summary: &str,
    object: &serde_json::Map<String, Value>,
) -> f64 {
    let text = format!("{title} {summary}");
    let mut score: f64 = 50.0;
    if crate::market::contains_any(
        &text,
        &[
            "利好", "上涨", "看多", "突破", "增长", "改善", "中标", "订单", "企稳", "买入", "加仓",
        ],
    ) {
        score += 18.0;
    }
    if crate::market::contains_any(
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
    crate::market::round2_value(score.clamp(0.0, 100.0))
}

pub(crate) fn guba_bullish_bearish_label(value: Option<&Value>) -> &'static str {
    match value.and_then(Value::as_i64).unwrap_or(0) {
        flag if flag > 0 => "看多",
        flag if flag < 0 => "看空",
        _ => "未标注",
    }
}

pub(crate) fn institution_lhb_score(net: f64, buy: f64, sell: f64, ratio: Option<f64>) -> f64 {
    let total = (buy.abs() + sell.abs()).max(1.0);
    let directional = (net / total * 35.0).clamp(-35.0, 35.0);
    let ratio_score = ratio.unwrap_or(0.0).clamp(-15.0, 15.0);
    crate::market::round2_value((50.0 + directional + ratio_score).clamp(0.0, 100.0))
}

pub(crate) fn fallback_lhb_start_date() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| (duration.as_secs() / 86_400) as i64 - 30)
        .unwrap_or(0);
    crate::market::civil_date_from_days(days)
}
