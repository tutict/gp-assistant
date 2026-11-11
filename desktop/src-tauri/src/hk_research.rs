use crate::hk_market::HkMarketCache;
use stock_optimizer_core as gp_core;

const EXPLORATORY_LIMITATIONS: [&str; 3] = [
    "survivorship_bias: current verified universe is applied to historical dates",
    "corporate_actions: daily bars are not verified as corporate-action adjusted",
    "point_in_time_financials: historical financial availability is incomplete",
];

pub(crate) fn attach_cached_hk_financial_metrics(
    result: &mut serde_json::Value,
    cache: &HkMarketCache,
) -> usize {
    let mut enriched = result
        .get_mut("items")
        .and_then(serde_json::Value::as_array_mut)
        .map_or(0, |items| {
            attach_cached_hk_financial_metrics_to_rows(items, cache)
        });
    if let Some(groups) = result
        .get_mut("groups")
        .and_then(serde_json::Value::as_array_mut)
    {
        for group in groups {
            if let Some(items) = group
                .get_mut("items")
                .and_then(serde_json::Value::as_array_mut)
            {
                enriched += attach_cached_hk_financial_metrics_to_rows(items, cache);
            }
        }
    }
    enriched
}

fn attach_cached_hk_financial_metrics_to_rows(
    items: &mut [serde_json::Value],
    cache: &HkMarketCache,
) -> usize {
    let mut enriched = 0;
    for item in items {
        let Some(stock) = item
            .get_mut("stock")
            .and_then(serde_json::Value::as_object_mut)
        else {
            continue;
        };
        let Some(code) = stock
            .get("code")
            .and_then(serde_json::Value::as_str)
            .and_then(|value| value.strip_suffix(".HK"))
            .map(str::to_owned)
        else {
            continue;
        };
        let quote = cache
            .quotes
            .get(&code)
            .filter(|quote| quote.currency.eq_ignore_ascii_case("HKD"));
        if let Some(quote) = quote {
            stock.insert(
                "quote_currency".to_owned(),
                serde_json::json!(quote.currency),
            );
            stock.insert("quote_source".to_owned(), serde_json::json!(quote.source));
            if let Some(name) = crate::hk_market::hk_chinese_display_name(Some(quote)) {
                stock.insert("name".to_owned(), serde_json::json!(name));
            }
        }
        if let Some(error) = cache.financial_errors.get(&code) {
            stock.insert("hk_financial_error".to_owned(), serde_json::json!(error));
        }
        let Some(snapshot) = cache.financials.get(&code) else {
            continue;
        };
        if crate::hk_market::hk_chinese_display_name(quote).is_none() {
            if let Some(name) = crate::hk_market::hk_financial_chinese_name(Some(snapshot)) {
                stock.insert("name".to_owned(), serde_json::json!(name));
            }
        }
        let mut detail = snapshot.clone();
        if let Some(quote) = quote {
            crate::hk_market::enrich_hk_financial_snapshot_from_quote(&mut detail, quote);
        }
        if let Some(error) = cache.financial_errors.get(&code) {
            detail
                .field_errors
                .insert("financial".to_owned(), error.clone());
        }
        stock.insert(
            "hk_financial_snapshot".to_owned(),
            serde_json::json!(detail),
        );
        let mut changed = false;
        if let Some(value) = snapshot.eps.value.filter(|value| value.is_finite()) {
            stock.insert("latest_eps".to_string(), serde_json::json!(value));
            changed = true;
        }
        if let Some(value) = snapshot.roe.value.filter(|value| value.is_finite()) {
            stock.insert("roe".to_string(), serde_json::json!(value / 100.0));
            changed = true;
        }
        if let Some(value) = snapshot
            .dividend_yield
            .value
            .filter(|value| value.is_finite())
        {
            stock
                .entry("dividend_yield".to_string())
                .or_insert_with(|| serde_json::json!(value / 100.0));
            changed = true;
        }
        stock.insert(
            "financial_report_period".to_string(),
            serde_json::json!(snapshot.normalized_period),
        );
        stock.insert(
            "financial_report_date".to_string(),
            serde_json::json!(snapshot.report_date),
        );
        stock.insert(
            "financial_source".to_string(),
            serde_json::json!(snapshot.candidate_source),
        );
        stock.insert(
            "financial_updated_at".to_string(),
            serde_json::json!(snapshot.candidate_fetched_at),
        );
        stock.insert(
            "financial_verification_status".to_string(),
            serde_json::json!(snapshot.verification_status),
        );
        if !snapshot.additional_metrics.is_empty() {
            stock.insert(
                "financial_metrics".to_string(),
                serde_json::to_value(&snapshot.additional_metrics)
                    .unwrap_or(serde_json::Value::Null),
            );
            changed = true;
        }
        if changed {
            enriched += 1;
        }
    }
    enriched
}

pub(crate) fn project_hk_market_data(
    cache: &HkMarketCache,
) -> Result<gp_core::CoreDataSet, String> {
    let mut data = gp_core::CoreDataSet::default();
    let mut seen = std::collections::HashSet::new();

    for security in &cache.securities {
        if !security.supports_hkd_research()
            || security.code.len() != 5
            || !security.code.bytes().all(|byte| byte.is_ascii_digit())
            || !seen.insert(security.code.clone())
        {
            continue;
        }
        let Some(quote) = cache.quotes.get(&security.code) else {
            continue;
        };
        if !security.cached_quote_matches_security_master(quote)
            || !quote.currency.eq_ignore_ascii_case("HKD")
            || !quote.price.is_finite()
            || quote.price <= 0.0
        {
            continue;
        }

        let code = format!("{}.HK", security.code);
        let mut stock = gp_core::StockItem {
            code: code.clone(),
            name: crate::hk_market::hk_chinese_display_name(Some(quote))
                .or_else(|| {
                    crate::hk_market::hk_financial_chinese_name(
                        cache.financials.get(&security.code),
                    )
                })
                .unwrap_or(&security.name)
                .to_owned(),
            industry: cache
                .financials
                .get(&security.code)
                .and_then(|snapshot| snapshot.company_profile.as_ref())
                .filter(|profile| profile.code == security.code)
                .map(|profile| profile.industry.clone())
                .unwrap_or_default(),
            price: quote.price,
            pe: quote.pe.filter(|value| value.is_finite()),
            pb: finite_positive(quote.pb),
            market_cap_billion: finite_positive(quote.market_cap_hkd)
                .map(|value| value / 1_000_000_000.0),
            dividend_yield: quote
                .dividend_yield_percent
                .filter(|value| value.is_finite())
                .map(|value| value / 100.0),
            change_pct: quote.change_pct.filter(|value| value.is_finite()),
            volume: quote.volume_shares.map(|value| value as f64),
            quote_time: quote.quote_time.clone(),
            financial_coverage: Some(hk_financial_coverage(cache.financials.get(&security.code))),
            ..gp_core::StockItem::default()
        };

        // Core uses a generic ROE field. Only carry an explicitly verified ratio; monetary
        // values stay out because CoreDataSet has no currency-bearing financial fields.
        if let Some(snapshot) = cache.financials.get(&security.code) {
            stock.roe = snapshot
                .roe
                .value
                .filter(|value| value.is_finite())
                .filter(|_| snapshot.roe.unit == "percent")
                .map(|value| value / 100.0);
            stock.latest_eps = snapshot
                .eps
                .value
                .filter(|value| value.is_finite())
                .filter(|_| snapshot.eps.unit == "currency/share");
            stock.dividend_yield = stock.dividend_yield.or_else(|| {
                snapshot
                    .dividend_yield
                    .value
                    .filter(|value| value.is_finite())
                    .filter(|_| snapshot.dividend_yield.unit == "percent")
                    .map(|value| value / 100.0)
            });
            stock.market_cap_billion = stock.market_cap_billion.or_else(|| {
                snapshot
                    .market_cap
                    .value
                    .filter(|value| value.is_finite())
                    .map(|value| value / 1_000_000_000.0)
            });
        }
        data.stocks.push(stock);

        if let Some(history) = cache.histories.get(&security.code) {
            let normalized_bars = crate::hk_market::hk_history_bars_for_quote(history, Some(quote));
            let bars = normalized_bars
                .iter()
                .filter(|bar| {
                    bar.open.is_finite()
                        && bar.high.is_finite()
                        && bar.low.is_finite()
                        && bar.close.is_finite()
                        && bar.close > 0.0
                })
                .map(|bar| gp_core::HistoryBar {
                    date: bar.date.clone(),
                    open: Some(bar.open),
                    high: Some(bar.high),
                    low: Some(bar.low),
                    close: bar.close,
                    volume: bar.volume_shares.map(|value| value as f64),
                    capital: None,
                })
                .collect::<Vec<_>>();
            if !bars.is_empty() {
                data.histories.insert(code, bars);
            }
        }
    }

    Ok(data)
}

fn finite_positive(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite() && *value > 0.0)
}

fn hk_financial_coverage(
    snapshot: Option<&crate::hk_market::HkFinancialSnapshot>,
) -> gp_core::FinancialCoverage {
    let Some(snapshot) = snapshot else {
        return gp_core::FinancialCoverage::default();
    };
    let monetary_field = |metric: &crate::hk_market::HkFinancialMetric, unit: &str| {
        metric.value.is_some_and(|value| value.is_finite())
            && metric.unit == unit
            && metric.currency == snapshot.currency
    };
    gp_core::FinancialCoverage {
        eps: monetary_field(&snapshot.eps, "currency/share"),
        roe: snapshot.roe.value.is_some_and(|value| value.is_finite())
            && snapshot.roe.unit == "percent",
        revenue: monetary_field(&snapshot.revenue, "currency_total"),
        net_profit: monetary_field(&snapshot.net_profit, "currency_total"),
    }
}

pub(crate) fn hk_exploratory_limitations() -> &'static [&'static str] {
    &EXPLORATORY_LIMITATIONS
}

pub(crate) fn quote_provenance(
    cache: &HkMarketCache,
    data: &gp_core::CoreDataSet,
) -> serde_json::Value {
    serde_json::Value::Object(
        data.stocks
            .iter()
            .filter_map(|stock| {
                let canonical = stock.code.strip_suffix(".HK")?;
                let quote = cache.quotes.get(canonical)?;
                Some((
                    stock.code.clone(),
                    serde_json::json!({
                        "source_id": quote.source,
                        "currency": quote.currency,
                        "fetched_at": quote.fetched_at,
                        "observed_at": quote.quote_time,
                        "last_error": quote.last_error,
                    }),
                ))
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::gp_core;
    use crate::hk_market::{
        HkClassificationStatus, HkDailyBar, HkHistoryRecord, HkMarketCache, HkQuote, HkSecurity,
    };

    fn fixture_cache() -> HkMarketCache {
        let mut cache = HkMarketCache::default();
        cache.securities = vec![HkSecurity {
            code: "00700".to_owned(),
            name: "Tencent".to_owned(),
            currency: "HKD".to_owned(),
            board_lot: Some(100),
            security_type: "ordinary share".to_owned(),
            classification_status: HkClassificationStatus::VerifiedOrdinaryShare,
        }];
        cache.quotes.insert(
            "00700".to_owned(),
            HkQuote {
                code: "00700".to_owned(),
                name: "Tencent".to_owned(),
                name_zh: None,
                price: 400.0,
                price_semantics_version: 1,
                previous_close: None,
                open: None,
                high: None,
                low: None,
                change: None,
                change_pct: Some(1.5),
                volume_shares: Some(100_000),
                turnover_hkd: Some(40_000_000.0),
                pe: Some(18.0),
                pb: Some(3.0),
                dividend_yield_percent: Some(1.0),
                market_cap_hkd: Some(3_800_000_000_000.0),
                total_shares: Some(9_500_000_000),
                circulating_shares: Some(9_000_000_000),
                currency: "HKD".to_owned(),
                quote_time: Some("2026-11-03T08:00:00Z".to_owned()),
                source: "tencent".to_owned(),
                fetched_at: Some("2026-11-03T08:01:00Z".to_owned()),
                last_error: None,
            },
        );
        cache.histories.insert(
            "00700".to_owned(),
            HkHistoryRecord {
                request_range: None,
                request_ranges: Vec::new(),
                price_adjustment: Default::default(),
                bar_fetched_at: Default::default(),
                revision_reason: None,
                suspended_as_of: None,
                bars: vec![
                    HkDailyBar {
                        date: "20261102".to_owned(),
                        open: 397.0,
                        high: 399.0,
                        low: 396.0,
                        close: 398.0,
                        volume_shares: Some(90_000),
                    },
                    HkDailyBar {
                        date: "20261103".to_owned(),
                        open: 398.0,
                        high: 402.0,
                        low: 397.0,
                        close: 400.0,
                        volume_shares: Some(100_000),
                    },
                ],
                source: "tencent".to_owned(),
                fetched_at: "2026-11-03T08:02:00Z".to_owned(),
                last_error: None,
            },
        );
        cache
    }

    #[test]
    fn projection_uses_hkex_listed_equities_and_preserves_hk_namespace() {
        let data = super::project_hk_market_data(&fixture_cache()).unwrap();

        assert_eq!(data.stocks.len(), 1);
        assert_eq!(data.stocks[0].code, "00700.HK");
        assert_eq!(data.stocks[0].price, 400.0);
        assert_eq!(data.histories["00700.HK"][1].date, "20261103");
        assert!(data.stocks.iter().all(|stock| stock.code.ends_with(".HK")));
    }

    #[test]
    fn projection_uses_tencent_chinese_quote_name_for_display_when_available() {
        let mut cache = fixture_cache();
        cache.quotes.get_mut("00700").unwrap().name = "腾讯控股".to_owned();

        let data = super::project_hk_market_data(&cache).unwrap();

        assert_eq!(data.stocks[0].name, "腾讯控股");
    }

    #[test]
    fn screening_projection_keeps_quote_market_cap_without_a_financial_snapshot() {
        let data = super::project_hk_market_data(&fixture_cache()).unwrap();
        assert_eq!(data.stocks[0].market_cap_billion, Some(3800.0));
    }

    #[test]
    fn screening_projection_normalizes_dividend_yield_without_a_financial_snapshot() {
        let data = super::project_hk_market_data(&fixture_cache()).unwrap();
        assert_eq!(data.stocks[0].dividend_yield, Some(0.01));
    }

    #[test]
    fn hk_screen_keeps_a_loss_making_issuer_complete_when_report_fields_exist() {
        let mut cache = fixture_cache();
        cache.securities[0].code = "08490".to_owned();
        cache.securities[0].name = "NICHE-TECH SEMI".to_owned();
        let mut quote = cache.quotes.remove("00700").unwrap();
        quote.code = "08490".to_owned();
        quote.pe = Some(-1.29);
        quote.dividend_yield_percent = Some(0.0);
        cache.quotes.insert("08490".to_owned(), quote);
        cache.financials.insert(
            "08490".to_owned(),
            crate::hk_market::parse_hk_financial_snapshot(
                include_str!("../testdata/eastmoney-hk-niche-tech.json"),
                "08490.HK",
                "fetched-at",
            )
            .unwrap(),
        );
        let data = super::project_hk_market_data(&cache).unwrap();
        let result = gp_core::screen_stocks(&data.stocks, &gp_core::ScreenCriteria::default());
        assert_eq!(result.items.len(), 1);
        let row = &result.items[0];
        assert_eq!(row.factor_scores["data_quality"], 1.0);
        assert!(!row.risk_tags.iter().any(|tag| tag.contains("财务数据缺失")));
        assert!(row.risk_tags.iter().any(|tag| tag.contains("亏损")));
        assert!(row.stock.deducted_net_profit_billion.is_none());
        assert!(row.stock.deducted_net_profit_growth_rate.is_none());
    }

    #[test]
    fn hk_screen_preserves_a_negative_pe_as_known_loss_information() {
        let mut cache = fixture_cache();
        cache.quotes.get_mut("00700").unwrap().pe = Some(-1.29);
        let data = super::project_hk_market_data(&cache).unwrap();
        assert_eq!(data.stocks[0].pe, Some(-1.29));
    }

    #[test]
    fn hk_screen_names_the_financial_field_that_is_actually_missing() {
        let mut cache = fixture_cache();
        let mut snapshot = crate::hk_market::parse_hk_financial_snapshot(
            include_str!("../testdata/eastmoney-hk-financial.json"),
            "00700.HK",
            "fetched-at",
        )
        .unwrap();
        snapshot.revenue.value = None;
        cache.financials.insert("00700".to_owned(), snapshot);
        let data = super::project_hk_market_data(&cache).unwrap();
        let result = gp_core::screen_stocks(&data.stocks, &gp_core::ScreenCriteria::default());
        assert!(result.items[0]
            .risk_tags
            .iter()
            .any(|tag| tag == "财务缺项：营业总收入"));
    }

    #[test]
    fn hk_report_zero_values_are_present_and_do_not_trigger_missing_or_loss_warnings() {
        let mut cache = fixture_cache();
        let mut snapshot = crate::hk_market::parse_hk_financial_snapshot(
            include_str!("../testdata/eastmoney-hk-financial.json"),
            "00700.HK",
            "fetched-at",
        )
        .unwrap();
        for metric in [
            &mut snapshot.eps,
            &mut snapshot.roe,
            &mut snapshot.revenue,
            &mut snapshot.net_profit,
        ] {
            metric.value = Some(0.0);
        }
        cache.financials.insert("00700".to_owned(), snapshot);
        let data = super::project_hk_market_data(&cache).unwrap();
        let result = gp_core::screen_stocks(&data.stocks, &gp_core::ScreenCriteria::default());
        assert_eq!(result.items[0].factor_scores["data_quality"], 1.0);
        assert!(!result.items[0]
            .risk_tags
            .iter()
            .any(|tag| tag.contains("缺项") || tag.contains("缺失") || tag.contains("亏损")));
    }

    #[test]
    fn projection_carries_cached_financial_eps_and_roe_into_screening_stock() {
        let mut cache = fixture_cache();
        let financial = crate::hk_market::parse_hk_financial_snapshot(
            include_str!("../testdata/eastmoney-hk-financial.json"),
            "00700.HK",
            "financial-at",
        )
        .unwrap();
        cache.financials.insert("00700".to_owned(), financial);

        let data = super::project_hk_market_data(&cache).unwrap();

        assert_eq!(data.stocks[0].latest_eps, Some(12.639));
        assert!((data.stocks[0].roe.unwrap() - 0.09966353785254).abs() < 1e-12);
        assert!((data.stocks[0].dividend_yield.unwrap() - 0.01).abs() < 1e-12);
    }

    #[test]
    fn screening_result_displays_cached_hk_eps_and_financial_data_without_changing_scores() {
        let mut cache = fixture_cache();
        cache.financials.insert(
            "00700".to_owned(),
            crate::hk_market::parse_hk_financial_snapshot(
                include_str!("../testdata/eastmoney-hk-financial.json"),
                "00700.HK",
                "financial-at",
            )
            .unwrap(),
        );
        let mut result = serde_json::json!({
            "items": [{
                "stock": {"code":"00700.HK", "name":"腾讯控股", "price":400.0},
                "score": 15.3,
                "reasons": []
            }]
        });

        let enriched = super::attach_cached_hk_financial_metrics(&mut result, &cache);

        assert_eq!(enriched, 1);
        assert_eq!(result["items"][0]["score"], 15.3);
        assert_eq!(result["items"][0]["stock"]["latest_eps"], 12.639);
        assert!(
            (result["items"][0]["stock"]["roe"].as_f64().unwrap() - 0.09966353785254).abs() < 1e-12
        );
        assert_eq!(
            result["items"][0]["stock"]["financial_report_period"],
            "2026H1"
        );
        assert_eq!(
            result["items"][0]["stock"]["financial_source"],
            "Eastmoney HK financial indicators"
        );
        assert_eq!(result["items"][0]["stock"]["quote_currency"], "HKD");
        assert_eq!(
            result["items"][0]["stock"]["hk_financial_snapshot"]["currency"],
            "CNY"
        );
        assert_eq!(
            result["items"][0]["stock"]["hk_financial_snapshot"]["revenue"]["value"],
            401243000000.0
        );
        assert_eq!(
            result["items"][0]["stock"]["hk_financial_snapshot"]["additional_metrics"]
                ["net_profit_yoy"]["value"],
            10.9672966559
        );
    }

    #[test]
    fn screening_result_attaches_cached_financial_data_to_visible_group_candidates() {
        let mut cache = fixture_cache();
        cache.financials.insert(
            "00700".to_owned(),
            crate::hk_market::parse_hk_financial_snapshot(
                include_str!("../testdata/eastmoney-hk-financial.json"),
                "00700.HK",
                "financial-at",
            )
            .unwrap(),
        );
        let mut result = serde_json::json!({
            "items": [],
            "groups": [{
                "key": "potential",
                "items": [{
                    "stock": {"code":"00700.HK", "name":"腾讯控股", "price":400.0},
                    "score": 15.3,
                    "reasons": []
                }]
            }]
        });

        let enriched = super::attach_cached_hk_financial_metrics(&mut result, &cache);

        assert_eq!(enriched, 1);
        assert_eq!(result["groups"][0]["items"][0]["score"], 15.3);
        assert_eq!(
            result["groups"][0]["items"][0]["stock"]["latest_eps"],
            12.639
        );
        assert_eq!(
            result["groups"][0]["items"][0]["stock"]["financial_report_period"],
            "2026H1"
        );
        assert_eq!(
            result["groups"][0]["items"][0]["stock"]["financial_source"],
            "Eastmoney HK financial indicators"
        );
    }

    #[test]
    fn grouped_result_keeps_financial_preparation_failures_without_fabricating_a_snapshot() {
        let mut cache = fixture_cache();
        cache.financial_errors.insert("00700".to_string(), "financial source timeout".to_string());
        let mut result = serde_json::json!({"groups":[{"sector":"消费零售","items":[{"stock":{"code":"00700.HK"},"score":15.3}]}]});
        assert_eq!(super::attach_cached_hk_financial_metrics(&mut result, &cache), 0);
        let stock = &result["groups"][0]["items"][0]["stock"];
        assert_eq!(stock["hk_financial_error"], "financial source timeout");
        assert!(stock.get("hk_financial_snapshot").is_none());
        assert_eq!(result["groups"][0]["items"][0]["score"], 15.3);
    }

    #[test]
    fn projection_prefers_sina_name_when_tencent_quote_name_is_english() {
        let mut cache = fixture_cache();
        let quote = cache.quotes.get_mut("00700").unwrap();
        quote.name = "CHINA RAIL CONS".to_owned();
        quote.name_zh = Some("中国中铁".to_owned());

        let data = super::project_hk_market_data(&cache).unwrap();

        assert_eq!(data.stocks[0].name, "中国中铁");
    }

    #[test]
    fn screening_details_preserve_cached_values_and_include_the_last_financial_refresh_error() {
        let mut cache = fixture_cache();
        cache.financials.insert(
            "00700".to_owned(),
            crate::hk_market::parse_hk_financial_snapshot(
                include_str!("../testdata/eastmoney-hk-financial.json"),
                "00700.HK",
                "fetched-at",
            )
            .unwrap(),
        );
        cache
            .financial_errors
            .insert("00700".to_owned(), "financial refresh failed".to_owned());
        let mut result =
            serde_json::json!({ "items": [{ "stock": { "code": "00700.HK" }, "score": 15.3 }] });
        super::attach_cached_hk_financial_metrics(&mut result, &cache);
        assert_eq!(
            result["items"][0]["stock"]["hk_financial_snapshot"]["field_errors"]["financial"],
            "financial refresh failed"
        );
        assert_eq!(
            result["items"][0]["stock"]["hk_financial_snapshot"]["revenue"]["value"],
            401243000000.0
        );
        assert_eq!(result["items"][0]["score"], 15.3);
    }

    #[test]
    fn projection_includes_an_hkex_listed_equity_without_calling_it_an_ordinary_share() {
        let mut cache = fixture_cache();
        let mut hsbc_quote = cache.quotes["00700"].clone();
        hsbc_quote.code = "00005".to_owned();
        hsbc_quote.name = "HSBC".to_owned();
        hsbc_quote.price = 102.0;
        cache.securities.push(HkSecurity {
            code: "00005".to_owned(),
            name: "HSBC".to_owned(),
            currency: "HKD".to_owned(),
            board_lot: Some(400),
            security_type: "Equity / Equity Securities (Main Board)".to_owned(),
            classification_status: HkClassificationStatus::ExchangeListedEquity,
        });
        cache.quotes.insert("00005".to_owned(), hsbc_quote);

        let data = super::project_hk_market_data(&cache).unwrap();

        assert_eq!(data.stocks.len(), 2);
        assert_eq!(data.stocks[1].code, "00005.HK");
        assert_eq!(data.stocks[1].price, 102.0);
    }

    #[test]
    fn projection_rejects_quote_currency_mismatch_and_does_not_convert_currency() {
        let mut cache = fixture_cache();
        cache.quotes.get_mut("00700").unwrap().currency = "CNY".to_owned();

        let data = super::project_hk_market_data(&cache).unwrap();

        assert!(data.stocks.is_empty());
        assert!(data.histories.is_empty());
    }

    #[test]
    fn projection_rejects_non_hkd_quotes_when_core_cannot_represent_currency() {
        let mut cache = fixture_cache();
        cache.securities[0].currency = "USD".to_owned();
        cache.quotes.get_mut("00700").unwrap().currency = "USD".to_owned();

        let data = super::project_hk_market_data(&cache).unwrap();

        assert!(data.stocks.is_empty());
    }

    #[test]
    fn projection_keeps_quoted_security_when_history_is_empty() {
        let mut cache = fixture_cache();
        cache.histories.clear();

        let data = super::project_hk_market_data(&cache).unwrap();

        assert_eq!(data.stocks.len(), 1);
        assert!(data.histories.is_empty());
    }

    #[test]
    fn exploratory_backtest_limitations_name_all_evidence_gaps() {
        let limitations = super::hk_exploratory_limitations().join(" ");
        assert!(limitations.contains("survivorship"));
        assert!(limitations.contains("corporate_actions"));
        assert!(limitations.contains("point_in_time_financials"));
    }

    #[test]
    fn candidate_snapshot_core_backtest_accepts_hk_namespace_without_cn_benchmarks() {
        let data = super::project_hk_market_data(&fixture_cache()).unwrap();
        let request: gp_core::BacktestRequest = serde_json::from_value(serde_json::json!({
            "strategy_mode":"candidate_snapshot",
            "stock_codes":["00700.HK"],
            "start_date":"20261102",
            "end_date":"20261103",
            "benchmark":"none"
        }))
        .unwrap();

        let result = gp_core::backtest_with_data(&data, &request).unwrap();

        assert_eq!(result.strategy_mode, "candidate_snapshot");
        assert_eq!(result.symbols, vec!["00700.HK"]);
    }

    #[test]
    fn candidate_snapshot_backtest_reports_missing_cached_history() {
        let mut cache = fixture_cache();
        cache.histories.clear();
        let data = super::project_hk_market_data(&cache).unwrap();
        let request: gp_core::BacktestRequest = serde_json::from_value(serde_json::json!({
            "strategy_mode":"candidate_snapshot",
            "stock_codes":["00700.HK"],
            "start_date":"20261102",
            "end_date":"20261103",
            "benchmark":"none"
        }))
        .unwrap();

        let error = gp_core::backtest_with_data(&data, &request).unwrap_err();

        assert!(error.to_string().contains("历史日线"));
    }
}
