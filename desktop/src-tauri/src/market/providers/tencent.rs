use crate::market::providers::history::fetch_daily_history_text;
use crate::market_domain::codes::board_label;
use crate::market_domain::codes::normalize_stock_code;
use crate::market_domain::codes::tencent_symbol;
use crate::market_domain::financial::estimate_roe;
use crate::market_domain::history::filter_daily_history_rows_by_date;
use crate::market_domain::history::parse_tencent_kline_row;
use crate::market_domain::history::tencent_daily_history_param;
#[cfg(windows)]
use crate::network::powershell::powershell_http_get_bytes;
use crate::wire_values::object_f64;
use crate::wire_values::object_string;
use crate::wire_values::parse_number;
use serde_json::json;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

pub(crate) const TENCENT_QUOTE_ENDPOINT: &str = "https://qt.gtimg.cn/q=";

pub(crate) const TENCENT_DAILY_KLINE_ENDPOINT: &str =
    "https://web.ifzq.gtimg.cn/appstock/app/fqkline/get";

pub(crate) const TENCENT_BATCH_SIZE: usize = 120;

pub(crate) const TENCENT_FETCH_CONCURRENCY: usize = 12;

pub(crate) const TENCENT_DEFAULT_MAX_CANDIDATES: usize = 8_000;

pub(crate) const TENCENT_DEFAULT_MAX_FAILED_BATCHES: usize = 4;

pub(crate) const TENCENT_REQUEST_TIMEOUT_SECS: u64 = 6;

pub(crate) const TENCENT_BATCH_TIMEOUT_SECS: u64 = 8;

pub(crate) const MAX_TENCENT_WEBVIEW_QUOTE_BYTES: usize = 1_048_576;

pub(crate) struct TencentQuotePayload {
    pub(crate) text: String,
    pub(crate) byte_len: usize,
    pub(crate) status: u16,
    pub(crate) transport: &'static str,
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
        let fallback = crate::runtime::run_transport_bound("market blocking transport", move || powershell_http_get_bytes(&url, timeout_secs))
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
        // Field 39 is TTM PE; field 52 annualizes the current report and must
        // not replace a missing TTM value, especially when their signs differ.
        let pe = parse_number(values.get(39));
        let pb = parse_number(values.get(46))
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
            .or_else(|| {
                if existing.is_some_and(|object| object.get("financial_source").is_some()) {
                    None
                } else {
                    estimate_roe(pe, pb)
                }
            });
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
