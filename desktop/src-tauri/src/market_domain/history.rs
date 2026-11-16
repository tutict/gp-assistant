use crate::market_domain::codes::normalize_stock_code;
use crate::market_domain::dates::compact_date_key;
use crate::market_domain::dates::normalize_history_date;
use crate::wire_values::json_f64;
use crate::wire_values::parse_f64_str;
use serde_json::json;
use serde_json::Value;
use std::collections::HashSet;
use stock_optimizer_core as gp_core;

pub(crate) const MARKET_REFRESH_HISTORY_MIN_BARS: usize = 60;

// Tencent fqkline rejects very large count values with "param error".
pub(crate) const OBSERVE_DAILY_HISTORY_LIMIT: usize = 2_000;

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
