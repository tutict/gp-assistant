use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use stock_optimizer_core::{RateRatio, RateUnit, METRIC_CONTRACT_VERSION};

fn stock_units() -> BTreeMap<String, RateUnit> {
    [
        ("roe", RateUnit::Ratio),
        ("dividend_yield", RateUnit::Ratio),
        ("change_pct", RateUnit::Ratio),
        ("turnover_rate", RateUnit::Ratio),
        ("deducted_net_profit_margin", RateUnit::PercentagePoints),
        (
            "deducted_net_profit_growth_rate",
            RateUnit::PercentagePoints,
        ),
    ]
    .into_iter()
    .map(|(key, unit)| (key.to_string(), unit))
    .collect()
}

fn financial_units() -> BTreeMap<String, RateUnit> {
    [
        "operating_revenue_yoy",
        "parent_net_profit_yoy",
        "gross_margin",
        "net_margin",
        "roe",
        "asset_liability_ratio",
        "goodwill_to_net_assets",
        "pledged_share_ratio",
        "dividend_yield",
        "dividend_payout_ratio",
        "deducted_net_profit_margin",
        "deducted_net_profit_growth_rate",
    ]
    .into_iter()
    .map(|key| (key.to_string(), RateUnit::PercentagePoints))
    .collect()
}

fn declared_units(
    value: Option<&Value>,
    targets: &BTreeMap<String, RateUnit>,
    known_contract: bool,
) -> Result<BTreeMap<String, RateUnit>, String> {
    match value {
        Some(Value::Object(values)) => values
            .iter()
            .map(|(key, unit)| {
                if !targets.contains_key(key) {
                    return Err(format!("未知财务指标单位：{key}"));
                }
                let unit = serde_json::from_value::<RateUnit>(unit.clone())
                    .map_err(|_| format!("指标 {key} 的单位无效"))?;
                Ok((key.clone(), unit))
            })
            .collect(),
        Some(_) => Err("指标单位声明必须是对象".to_string()),
        None if known_contract => Ok(targets.clone()),
        None => Err("财务来源缺少明确单位，请提供 metric_units 或已知 v3 快照版本".to_string()),
    }
}

fn convert_row(
    row: &mut Map<String, Value>,
    sources: &BTreeMap<String, RateUnit>,
    targets: &BTreeMap<String, RateUnit>,
) -> Result<(), String> {
    for (key, target) in targets {
        let Some(value) = row.get(key) else {
            continue;
        };
        if value.is_null() || value.as_str().is_some_and(|value| value.trim().is_empty()) {
            continue;
        }
        let unit = sources
            .get(key)
            .ok_or_else(|| format!("指标 {key} 缺少明确单位"))?;
        let numeric = value
            .as_f64()
            .or_else(|| {
                value
                    .as_str()
                    .and_then(|value| value.trim().parse::<f64>().ok())
            })
            .filter(|value| value.is_finite())
            .ok_or_else(|| format!("指标 {key} 必须是有限数值"))?;
        if unit == target {
            if !value.is_number() {
                row.insert(key.clone(), json!(numeric));
            }
            continue;
        }
        let rate = RateRatio::from_unit(numeric, *unit).map_err(|error| error.to_string())?;
        let converted = match target {
            RateUnit::Ratio => rate.as_ratio(),
            RateUnit::PercentagePoints => rate
                .as_percentage_points()
                .map_err(|error| error.to_string())?,
        };
        row.insert(key.clone(), json!(converted));
    }
    Ok(())
}

fn normalize_rows(
    value: &mut Value,
    sources: &BTreeMap<String, RateUnit>,
    targets: &BTreeMap<String, RateUnit>,
) -> Result<(), String> {
    match value {
        Value::Array(rows) => {
            for row in rows {
                normalize_rows(row, sources, targets)?;
            }
        }
        Value::Object(row) => convert_row(row, sources, targets)?,
        Value::Null => (),
        _ => return Err("财务或股票记录必须是对象".to_string()),
    }
    Ok(())
}

fn has_rate_values(snapshot: &Value, targets: &BTreeMap<String, RateUnit>) -> bool {
    snapshot
        .get("financials")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|rows| rows.values())
        .chain(
            snapshot
                .get("stocks")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        )
        .any(|row| {
            targets.keys().any(|key| {
                row.get(key).is_some_and(|value| !value.is_null())
                    || row
                        .get("financial")
                        .and_then(|nested| nested.get(key))
                        .is_some_and(|value| !value.is_null())
            })
        })
}

pub(crate) fn normalize_financial_snapshot(snapshot: &Value) -> Result<Value, String> {
    let targets = financial_units();
    let known = snapshot.get("schema_version").and_then(Value::as_str)
        == Some("mobile-financial-snapshot/v3")
        || !has_rate_values(snapshot, &targets);
    let sources = declared_units(snapshot.get("metric_units"), &targets, known)?;
    let mut output = snapshot.clone();
    let object = output
        .as_object_mut()
        .ok_or_else(|| "财务快照必须是对象".to_string())?;
    if let Some(financials) = object.get_mut("financials") {
        let financials = financials
            .as_object_mut()
            .ok_or_else(|| "financials 必须是对象".to_string())?;
        for row in financials.values_mut() {
            normalize_rows(row, &sources, &targets)?;
        }
    }
    let stock_targets = stock_units();
    let mut stock_sources = sources.clone();
    stock_sources.insert("change_pct".into(), RateUnit::Ratio);
    stock_sources.insert("turnover_rate".into(), RateUnit::Ratio);
    if snapshot.get("stock_metric_units").is_some() {
        stock_sources.extend(declared_units(
            snapshot.get("stock_metric_units"),
            &stock_targets,
            false,
        )?);
    }
    if let Some(stocks) = object.get_mut("stocks").and_then(Value::as_array_mut) {
        for stock in stocks {
            let row = stock
                .as_object_mut()
                .ok_or_else(|| "股票快照记录必须是对象".to_string())?;
            let nested = row.get("financial").and_then(Value::as_object).cloned();
            let mut financial = nested.clone().unwrap_or_else(|| row.clone());
            convert_row(
                &mut financial,
                if nested.is_some() {
                    &sources
                } else {
                    &stock_sources
                },
                &targets,
            )?;
            convert_row(row, &stock_sources, &stock_targets)?;
            if targets.keys().any(|key| financial.contains_key(key)) {
                row.insert("financial".into(), Value::Object(financial));
            }
        }
    }
    object.insert(
        "schema_version".into(),
        json!("mobile-financial-snapshot/v4"),
    );
    object.insert(
        "metric_contract_version".into(),
        json!(METRIC_CONTRACT_VERSION),
    );
    object.insert(
        "metric_units".into(),
        serde_json::to_value(targets).map_err(|error| error.to_string())?,
    );
    object.insert(
        "stock_metric_units".into(),
        serde_json::to_value(stock_targets).map_err(|error| error.to_string())?,
    );
    Ok(output)
}

pub(crate) fn normalize_stock_dataset(dataset: &Value) -> Result<Value, String> {
    let targets = stock_units();
    let sources = declared_units(dataset.get("metric_units"), &targets, true)?;
    let mut output = dataset.clone();
    let object = output
        .as_object_mut()
        .ok_or_else(|| "股票数据集必须是对象".to_string())?;
    if let Some(stocks) = object.get_mut("stocks") {
        normalize_rows(stocks, &sources, &targets)?;
    }
    if let Some(snapshots) = object
        .get_mut("factor_snapshots")
        .and_then(Value::as_object_mut)
    {
        for rows in snapshots.values_mut() {
            normalize_rows(rows, &sources, &targets)?;
        }
    }
    object.insert(
        "metric_contract_version".into(),
        json!(METRIC_CONTRACT_VERSION),
    );
    object.insert(
        "metric_units".into(),
        serde_json::to_value(targets).map_err(|error| error.to_string())?,
    );
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn financial_disclosure_rates_convert_once_with_explicit_units() {
        let source = json!({
            "schema_version": "mobile-financial-snapshot/v4",
            "generated_at": "2026-07-16T10:00:00Z",
            "metric_units": {"roe": "ratio", "deducted_net_profit_growth_rate": "ratio"},
            "financials": {"000001.SZ": {"roe": 1.2, "deducted_net_profit_growth_rate": 0.00804352}}
        });
        let output = normalize_financial_snapshot(&source).unwrap();
        assert_eq!(output["financials"]["000001.SZ"]["roe"], 120.0);
        assert!(
            (output["financials"]["000001.SZ"]["deducted_net_profit_growth_rate"]
                .as_f64()
                .unwrap()
                - 0.804352)
                .abs()
                < 1e-12
        );
        assert_eq!(output["generated_at"], source["generated_at"]);
        assert_eq!(normalize_financial_snapshot(&output).unwrap(), output);
        assert_eq!(source["financials"]["000001.SZ"]["roe"], 1.2);
    }

    #[test]
    fn known_financial_v3_keeps_small_percentage_points() {
        let output = normalize_financial_snapshot(&json!({
            "schema_version": "mobile-financial-snapshot/v3",
            "financials": {"000001.SZ": {"roe": 0.07, "deducted_net_profit_growth_rate": -0.5}}
        }))
        .unwrap();
        assert_eq!(output["metric_contract_version"], 2);
        assert_eq!(output["financials"]["000001.SZ"]["roe"], 0.07);
        assert_eq!(
            output["financials"]["000001.SZ"]["deducted_net_profit_growth_rate"],
            -0.5
        );
    }

    #[test]
    fn unknown_financial_source_requires_units_and_preserves_input() {
        let source = json!({"financials": {"000001.SZ": {"roe": 0.07}}});
        assert!(normalize_financial_snapshot(&source).is_err());
        assert!(normalize_financial_snapshot(&json!({"schema_version":"mobile-financial-snapshot/v4", "metric_units": {}, "financials":{"000001.SZ":{"roe":0.07}}})).is_err());
        assert!(normalize_financial_snapshot(
            &json!({"metric_units":{"roe":"pct"}, "financials":{"000001.SZ":{"roe":0.07}}})
        )
        .is_err());
        assert_eq!(source["financials"]["000001.SZ"]["roe"], 0.07);
        assert!(normalize_financial_snapshot(
            &json!({"stocks":[{"code":"000001.SZ","financial":{"roe":0.07}}]})
        )
        .is_err());
    }

    #[test]
    fn stock_and_financial_dtos_keep_independent_declared_units() {
        let output = normalize_financial_snapshot(&json!({
            "schema_version":"mobile-financial-snapshot/v4",
            "metric_units":{"roe":"ratio"},
            "stock_metric_units":{"roe":"percentage_points"},
            "stocks":[{"code":"000001.SZ","roe":120.0}],
            "financials":{"000001.SZ":{"roe":1.2}}
        }))
        .unwrap();
        assert_eq!(output["stocks"][0]["roe"], 1.2);
        assert_eq!(output["stocks"][0]["financial"]["roe"], 120.0);
        assert_eq!(output["financials"]["000001.SZ"]["roe"], 120.0);
        assert_eq!(normalize_financial_snapshot(&output).unwrap(), output);
    }

    #[test]
    fn declared_numeric_rate_strings_become_numeric_dto_values() {
        let output = normalize_financial_snapshot(&json!({
            "schema_version":"mobile-financial-snapshot/v3",
            "financials":{"000001.SZ":{"roe":"0.07"}}
        }))
        .unwrap();
        assert_eq!(output["financials"]["000001.SZ"]["roe"], 0.07);
    }

    #[test]
    fn cached_stock_dto_preserves_wire_contract_after_explicit_conversion() {
        let source = json!({
            "schema_version": 1,
            "metric_units": {"roe":"percentage_points", "deducted_net_profit_growth_rate":"ratio"},
            "stocks": [{"code":"000001.SZ", "name":"sample", "price":10.0, "roe":120.0, "deducted_net_profit_growth_rate":0.00804352}]
        });
        let output = normalize_stock_dataset(&source).unwrap();
        assert_eq!(output["stocks"][0]["roe"], 1.2);
        assert!(
            (output["stocks"][0]["deducted_net_profit_growth_rate"]
                .as_f64()
                .unwrap()
                - 0.804352)
                .abs()
                < 1e-12
        );
        assert_eq!(normalize_stock_dataset(&output).unwrap(), output);
        let existing = json!({"stocks":[{"roe":1.2, "deducted_net_profit_growth_rate":0.804352}]});
        let normalized = normalize_stock_dataset(&existing).unwrap();
        assert_eq!(normalized["stocks"], existing["stocks"]);
    }
}
