use crate::market_domain::codes::normalize_stock_code;
use crate::wire_values::json_f64;
use crate::wire_values::object_string;
use serde_json::json;
use serde_json::Value;
use std::collections::HashMap;
use std::collections::HashSet;

pub(crate) const COMPLETE_QUARTERLY_EPS_POINTS: usize = 8;

pub(crate) const SCREEN_STOCK_FINANCIAL_FIELDS: [&str; 5] = [
    "deducted_net_profit_billion",
    "deducted_net_profit_margin",
    "deducted_net_profit_growth_rate",
    "latest_eps",
    "latest_bps",
];

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
    let current_period = target
        .get("period")
        .and_then(Value::as_str)
        .and_then(crate::observe::financial_period_rank);
    let incoming_period = source
        .get("period")
        .and_then(Value::as_str)
        .and_then(crate::observe::financial_period_rank);
    if current_period
        .is_some_and(|current| incoming_period.is_none_or(|incoming| incoming < current))
    {
        return;
    }
    let newer = incoming_period
        .zip(current_period)
        .is_some_and(|(incoming, current)| incoming > current);
    for field in SCREEN_STOCK_FINANCIAL_FIELDS {
        if !newer && finite_object_number(target, field).is_some() {
            continue;
        }
        if let Some(value) = finite_object_number(source, field) {
            target.insert(field.to_string(), json!(value));
        } else if newer {
            target.insert(field.to_string(), Value::Null);
        }
    }
    if newer {
        target.insert("period".to_string(), source["period"].clone());
    }
}

pub(crate) fn filtered_financial_snapshot_map(
    seed: &Value,
    financial_snapshot: &Value,
    valid_codes: &HashSet<String>,
) -> Value {
    let mut entries = serde_json::Map::new();
    merge_financials_array(&mut entries, seed.get("stocks"));
    merge_financials_array(&mut entries, financial_snapshot.get("stocks"));
    merge_financials_object(&mut entries, seed.get("financials"));
    merge_financials_object(&mut entries, financial_snapshot.get("financials"));

    let mut filtered = serde_json::Map::new();
    for (code, value) in entries {
        if valid_codes.contains(&code) {
            filtered.insert(code, value);
        }
    }
    Value::Object(filtered)
}

pub(crate) fn merge_financials_object(
    target: &mut serde_json::Map<String, Value>,
    value: Option<&Value>,
) {
    let Some(object) = value.and_then(Value::as_object) else {
        return;
    };
    for (raw_code, item) in object {
        merge_financial_entry(target, raw_code, item);
    }
}

pub(crate) fn merge_financials_array(
    target: &mut serde_json::Map<String, Value>,
    value: Option<&Value>,
) {
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
        merge_financial_entry(target, raw_code, object.get("financial").unwrap_or(item));
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

    let incoming_period = object_string_any(object, &["period", "latest_period", "report_period"])
        .and_then(|period| crate::observe::financial_period_rank(&period));
    let existing_period = entry
        .get("period")
        .and_then(Value::as_str)
        .and_then(crate::observe::financial_period_rank);
    if incoming_period
        .zip(existing_period)
        .is_some_and(|(incoming, existing)| incoming < existing)
    {
        return;
    }
    let different_report_source = object.get("report_fetched_at").is_some()
        && (entry.get("report_fetched_at").is_none()
            || object.get("source") != entry.get("source")
            || object.get("report_notice_date") != entry.get("report_notice_date"));
    if incoming_period.is_some()
        && (existing_period.is_none()
            || incoming_period
                .zip(existing_period)
                .is_some_and(|(incoming, existing)| incoming > existing)
            || different_report_source)
    {
        for field in [
            "latest_eps",
            "latest_bps",
            "operating_revenue_billion",
            "operating_revenue_yoy",
            "parent_net_profit_billion",
            "parent_net_profit_yoy",
            "deducted_net_profit_billion",
            "deducted_net_profit_margin",
            "deducted_net_profit_growth_rate",
            "gross_margin",
            "net_margin",
            "roe",
            "asset_liability_ratio",
            "report_notice_date",
            "report_fetched_at",
        ] {
            entry.remove(field);
        }
    }

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
        "deducted_net_profit_billion",
        "deducted_net_profit_margin",
        "deducted_net_profit_growth_rate",
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
    for field in [
        "source",
        "currency",
        "report_notice_date",
        "report_fetched_at",
    ] {
        if let Some(value) = object_string(object, field).filter(|value| !value.trim().is_empty()) {
            entry.insert(field.to_string(), json!(value));
        }
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

pub(crate) fn object_string_any(
    object: &serde_json::Map<String, Value>,
    fields: &[&str],
) -> Option<String> {
    fields.iter().find_map(|field| object_string(object, field))
}

pub(crate) fn finite_object_number(
    object: &serde_json::Map<String, Value>,
    field: &str,
) -> Option<f64> {
    object
        .get(field)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

pub(crate) fn estimate_roe(pe: Option<f64>, pb: Option<f64>) -> Option<f64> {
    let pe = pe.filter(|value| *value > 0.0)?;
    let pb = pb.filter(|value| *value > 0.0)?;
    Some(pb / pe)
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

pub(crate) fn stock_object<'a>(
    data: &'a Value,
    code: &str,
) -> Option<&'a serde_json::Map<String, Value>> {
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
