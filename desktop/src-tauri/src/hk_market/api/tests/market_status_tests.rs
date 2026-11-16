use super::*;

fn cache_fixture() -> HkMarketCache {
    let mut cache = HkMarketCache {
        securities_updated_at: Some("2026-11-07T08:00:00Z".to_owned()),
        ..HkMarketCache::default()
    };
    for (code, currency, classification) in [
        ("00700", "HKD", "exchange_listed_equity"),
        ("02800", "HKD", "unverified"),
        ("09090", "USD", "exchange_listed_equity"),
    ] {
        cache.securities.push(
            serde_json::from_value(serde_json::json!({
                "code":code,"name":code,"currency":currency,"security_type":"Equity",
                "classification_status":classification,
            }))
            .unwrap(),
        );
        cache.quotes.insert(
            code.to_owned(),
            serde_json::from_value(serde_json::json!({
                "code":code,"name":code,"currency":currency,"price":400.0,
                "price_semantics_version":1,"source":"Tencent Finance",
                "quote_time":"2026/11/06 16:08:22","fetched_at":"2026-11-07T08:00:00Z",
            }))
            .unwrap(),
        );
    }
    cache
}

fn status(cache: &HkMarketCache) -> serde_json::Value {
    let now = chrono::DateTime::parse_from_rfc3339("2026-11-07T13:00:00Z")
        .unwrap()
        .timestamp_millis() as u128;
    hk_market_status_value(
        Path::new("hk-market-data.json"),
        cache,
        serde_json::json!({"database_bytes":1048576}),
        now,
    )
    .unwrap()
}

#[test]
fn hk_market_status_counts_the_screenable_universe_and_uses_quote_observation_dates() {
    let result = status(&cache_fixture());
    assert_eq!(result["market"], "hk");
    assert_eq!(result["security_count"], 3);
    assert_eq!(result["quote_count"], 3);
    assert_eq!(result["universe_count"], 1);
    assert_eq!(result["quote_trade_date"], "20261106");
    assert_eq!(result["current_trade_date"], "20261106");
    assert_eq!(result["quote_generated_at"], "2026-11-07T08:00:00Z");
    assert_eq!(result["cache_bytes"], 1048576);
    assert_eq!(result["stale"], false);
}

#[test]
fn hk_market_status_does_not_make_old_quotes_fresh_by_downloading_them_today() {
    let mut cache = cache_fixture();
    cache.quotes.get_mut("00700").unwrap().quote_time = Some("2026/11/05 16:08:22".to_owned());
    let result = status(&cache);
    assert_eq!(result["quote_trade_date"], "20261105");
    assert_eq!(result["stale"], true);
}

#[test]
fn hk_market_status_marks_a_directory_without_quotes_as_empty() {
    let mut cache = cache_fixture();
    cache.quotes.clear();
    let result = status(&cache);
    assert_eq!(result["universe_count"], 0);
    assert!(result["quote_trade_date"].is_null());
    assert!(result["quote_generated_at"].is_null());
    assert_eq!(result["stale"], true);
    assert_eq!(result["universe_updated_at"], "2026-11-07T08:00:00Z");
}

#[test]
fn hk_market_status_preserves_counts_and_reports_cached_refresh_errors() {
    let mut cache = cache_fixture();
    cache
        .quote_errors
        .insert("00700".to_owned(), "provider unavailable".to_owned());
    let result = status(&cache);
    assert_eq!(result["universe_count"], 1);
    assert_eq!(result["quote_trade_date"], "20261106");
    assert_eq!(result["stale"], true);
    assert_eq!(result["quote_errors"]["00700"], "provider unavailable");
}

#[test]
fn hk_market_status_does_not_mark_current_quotes_stale_for_an_individual_history_error() {
    let mut cache = cache_fixture();
    cache.history_errors.insert(
        "00700".to_owned(),
        "history provider unavailable".to_owned(),
    );
    let result = status(&cache);
    assert_eq!(result["stale"], false);
    assert_eq!(
        result["history_errors"]["00700"],
        "history provider unavailable"
    );
}

#[test]
fn hk_market_status_keeps_a_missing_supported_quote_pending() {
    let mut cache = cache_fixture();
    cache.securities.push(
        serde_json::from_value(serde_json::json!({
            "code":"09988","name":"Alibaba","currency":"HKD","security_type":"Equity",
            "classification_status":"exchange_listed_equity",
        }))
        .unwrap(),
    );
    let result = status(&cache);
    assert_eq!(result["universe_count"], 1);
    assert_eq!(result["stale"], true);
}

#[test]
fn hk_market_status_ignores_quote_errors_outside_the_hkd_scope() {
    let mut cache = cache_fixture();
    cache
        .quote_errors
        .insert("09090".to_owned(), "unsupported USD counter".to_owned());
    assert_eq!(status(&cache)["stale"], false);
}

#[test]
fn hk_quote_refresh_excluded_currency_counters_do_not_make_a_complete_supported_batch_stale() {
    let mut summary = HkQuoteRefreshSummary {
        requested_count: 3,
        attempted_count: 2,
        updated_count: 2,
        unsupported_currency_count: 1,
        ..HkQuoteRefreshSummary::default()
    };
    assert!(!summary.is_stale());
    summary.updated_count = 1;
    summary.failed_count = 1;
    assert!(summary.is_stale());
    assert!(HkQuoteRefreshSummary::default().is_stale());
}
