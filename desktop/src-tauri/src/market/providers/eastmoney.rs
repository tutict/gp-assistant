use crate::market::providers::history::fetch_daily_history_text;
use crate::market::providers::history::OBSERVE_FUNDAMENTAL_PREFERRED_TIMEOUT_SECS;
use crate::market_domain::codes::eastmoney_market_code;
use crate::market_domain::dates::compact_date_key;
use crate::market_domain::history::parse_eastmoney_kline_row;
use serde_json::json;
use serde_json::Value;
use std::time::Duration;

pub(crate) const EASTMONEY_KLINE_ENDPOINT: &str =
    "https://push2his.eastmoney.com/api/qt/stock/kline/get";

pub(crate) const EASTMONEY_DATACENTER_ENDPOINT: &str =
    "https://datacenter-web.eastmoney.com/api/data/v1/get";

pub(crate) const EASTMONEY_SECURITIES_ENDPOINT: &str =
    "https://datacenter.eastmoney.com/securities/api/data/v1/get";

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
