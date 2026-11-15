use crate::market_domain::codes::adaptive_benchmark_codes;
use crate::market_domain::codes::board_label;
use crate::market_domain::codes::normalize_stock_code;
use crate::market_domain::dates::compact_date_key;
use crate::market_domain::dates::epoch_millis;
use serde_json::json;
use serde_json::Value;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use stock_optimizer_core as gp_core;

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

pub(crate) fn seed_stock_maps(
    seed: &Value,
) -> (HashMap<String, serde_json::Map<String, Value>>, Vec<String>) {
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

pub(crate) fn strip_core_side_payload_fields(mut payload: Value) -> Value {
    if let Some(object) = payload.as_object_mut() {
        object.remove("financial_snapshot");
    }
    payload
}

pub(crate) fn market_quote_coverage(
    data: &Value,
    target_date: Option<&str>,
) -> (usize, usize, f64) {
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
    let history_ranges = data
        .pointer("/cn_reference/history_ranges")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(code, _)| normalize_stock_code(code).is_some_and(|code| codes.contains(&code)))
        .map(|(code, value)| (code.clone(), value.clone()))
        .collect::<serde_json::Map<_, _>>();
    json!({
        "schema_version": 1,
        "updated_at_epoch_ms": epoch_millis(),
        "stocks": stocks,
        "histories": section("histories"),
        "financials": section("financials"),
        "factor_snapshots": section("factor_snapshots"),
        "capital_evidence": section("capital_evidence"),
        "cn_reference": { "history_ranges": history_ranges }
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
    if let Some(ranges) = patch
        .pointer("/cn_reference/history_ranges")
        .and_then(Value::as_object)
    {
        let reference = target
            .entry("cn_reference".to_string())
            .or_insert_with(|| json!({}));
        if let Some(reference) = reference.as_object_mut() {
            let histories = reference
                .entry("history_ranges".to_string())
                .or_insert_with(|| json!({}));
            if let Some(histories) = histories.as_object_mut() {
                for (code, proof) in ranges {
                    histories.insert(code.clone(), proof.clone());
                }
            }
        }
    }
}

pub(crate) fn parse_mobile_market_data_snapshot(
    data: &Value,
    label: &str,
) -> Result<(Arc<gp_core::CoreDataSet>, Value), String> {
    let normalized = crate::metric_units::normalize_stock_dataset(data)?;
    let typed = serde_json::from_value::<gp_core::CoreDataSet>(normalized)
        .map_err(|error| format!("{label} parse failed: {error}"))?;
    let summary = gp_core::validate_data_set(&typed)
        .map_err(|error| format!("{label} validation failed: {error}"))?;
    let summary = serde_json::to_value(summary)
        .map_err(|error| format!("{label} summary serialization failed: {error}"))?;
    Ok((Arc::new(typed), summary))
}
