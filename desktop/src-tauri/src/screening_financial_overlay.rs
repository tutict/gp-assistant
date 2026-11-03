use serde_json::{Map, Value};
use std::collections::HashMap;
use stock_optimizer_core as gp_core;

/// Merge already-normalized financial fields without serializing the stock universe.
pub(super) fn merge(data: &gp_core::CoreDataSet, snapshot: &Value) -> Vec<gp_core::StockItem> {
    let mut codes = Vec::with_capacity(data.stocks.len());
    let mut stocks = HashMap::with_capacity(data.stocks.len());
    for stock in &data.stocks {
        let Some(code) = crate::market::normalize_stock_code(&stock.code) else {
            continue;
        };
        let mut stock = stock.clone();
        stock.code = code.clone();
        codes.push(code.clone());
        stocks.insert(code, stock);
    }
    if let Some(financials) = snapshot.get("financials").and_then(Value::as_object) {
        for (code, value) in financials {
            if let (Some(code), Some(row)) =
                (crate::market::normalize_stock_code(code), value.as_object())
            {
                if let Some(stock) = stocks.get_mut(&code) {
                    merge_fields(stock, row);
                }
            }
        }
    }
    if let Some(rows) = snapshot.get("stocks").and_then(Value::as_array) {
        for row in rows {
            let code = row
                .get("code")
                .and_then(Value::as_str)
                .and_then(crate::market::normalize_stock_code);
            if let (Some(code), Some(row)) = (code, row.as_object()) {
                if let Some(stock) = stocks.get_mut(&code) {
                    merge_fields(stock, row);
                }
            }
        }
    }
    if let Some(industries) = snapshot.get("industries").and_then(Value::as_object) {
        for (code, value) in industries {
            let code = crate::market::normalize_stock_code(code);
            let industry = value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty() && *value != "-");
            if let (Some(code), Some(industry)) = (code, industry) {
                if let Some(stock) = stocks.get_mut(&code) {
                    stock.industry = industry.to_string();
                }
            }
        }
    }
    codes
        .into_iter()
        .filter_map(|code| stocks.remove(&code))
        .map(|mut stock| {
            if stock.industry.trim().is_empty() {
                stock.industry = crate::market::board_label(&stock.code).to_string();
            }
            stock
        })
        .collect()
}

fn merge_fields(stock: &mut gp_core::StockItem, row: &Map<String, Value>) {
    for (field, target) in [
        (
            "deducted_net_profit_billion",
            &mut stock.deducted_net_profit_billion,
        ),
        (
            "deducted_net_profit_margin",
            &mut stock.deducted_net_profit_margin,
        ),
        (
            "deducted_net_profit_growth_rate",
            &mut stock.deducted_net_profit_growth_rate,
        ),
        ("latest_eps", &mut stock.latest_eps),
    ] {
        if target.is_none_or(|value| !value.is_finite()) {
            *target = crate::market::finite_object_number(row, field);
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};
    use stock_optimizer_core as gp_core;

    fn legacy_stocks(data: &gp_core::CoreDataSet, snapshot: &Value) -> Vec<gp_core::StockItem> {
        let mut data = json!({"stocks":data.stocks});
        crate::screening::merge_screen_financial_snapshot_into_data(&mut data, snapshot);
        serde_json::from_value(data["stocks"].take()).unwrap()
    }

    #[test]
    fn typed_overlay_matches_legacy_financial_priority_without_replacing_quotes() {
        let data: gp_core::CoreDataSet = serde_json::from_value(json!({"stocks":[
            {"code":"000001.SZ","name":"平安银行","industry":"银行","price":10.0,"quote_time":"20261030","roe":0.12,"latest_eps":0.0,"deducted_net_profit_margin":-2.0},
            {"code":"600519.SH","name":"贵州茅台","industry":"白酒","price":20.0}
        ]})).unwrap();
        let snapshot = json!({
            "financials":{
                "000001.SZ":{"latest_eps":9.0,"deducted_net_profit_margin":18.0,"deducted_net_profit_growth_rate":0.804352},
                "600519.SH":{"latest_eps":2.5,"deducted_net_profit_billion":12.0}
            },
            "stocks":[{"code":"600519.SH","price":999.0,"roe":0.99,"latest_eps":99.0,"deducted_net_profit_margin":20.0}],
            "industries":{"600519.SH":" 白酒制造 "}
        });
        let actual = super::merge(&data, &snapshot);
        assert_eq!(
            serde_json::to_value(&actual).unwrap(),
            serde_json::to_value(legacy_stocks(&data, &snapshot)).unwrap()
        );
        assert_eq!(actual[0].latest_eps, Some(0.0));
        assert_eq!(actual[0].deducted_net_profit_margin, Some(-2.0));
        assert_eq!(actual[0].deducted_net_profit_growth_rate, Some(0.804352));
        assert_eq!(actual[1].latest_eps, Some(2.5));
        assert_eq!(actual[1].price, 20.0);
        assert_eq!(actual[1].roe, None);
        assert_eq!(actual[1].industry, "白酒制造");
    }

    #[test]
    fn typed_overlay_preserves_universe_order_and_last_duplicate_record() {
        let data: gp_core::CoreDataSet = serde_json::from_value(json!({"stocks":[
            {"code":"600519","name":"旧记录","industry":"白酒","price":10.0},
            {"code":"000001.SZ","name":"平安银行","industry":"","price":12.0},
            {"code":"600519.SH","name":"贵州茅台","industry":"白酒","price":20.0}
        ]}))
        .unwrap();
        let snapshot = json!({"financials":{"600519.SH":{"latest_eps":2.5},"688372.SH":{"latest_eps":3.0}},"industries":{"000001.SZ":"-"}});
        let actual = super::merge(&data, &snapshot);
        assert_eq!(
            serde_json::to_value(&actual).unwrap(),
            serde_json::to_value(legacy_stocks(&data, &snapshot)).unwrap()
        );
        assert_eq!(actual.len(), 2);
        assert_eq!(actual[0].code, "600519.SH");
        assert_eq!(actual[0].name, "贵州茅台");
        assert_eq!(actual[1].code, "000001.SZ");
        assert!(!actual[1].industry.is_empty());
    }
}
