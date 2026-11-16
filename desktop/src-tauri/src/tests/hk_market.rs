#[test]
fn hong_kong_stock_codes_normalize_without_changing_mainland_codes() {
    assert_eq!(
        normalize_stock_code("00700.HK").as_deref(),
        Some("00700.HK")
    );
    assert_eq!(normalize_stock_code("HK00700").as_deref(), Some("00700.HK"));
    assert_eq!(normalize_stock_code("700"), None);
    assert_eq!(normalize_stock_code("123456.HK"), None);
    assert_eq!(tencent_symbol("00700.HK").as_deref(), Some("hk00700"));
    assert_eq!(
        normalize_stock_code("600000.SH").as_deref(),
        Some("600000.SH")
    );
    assert_eq!(
        normalize_stock_code("000001.SZ").as_deref(),
        Some("000001.SZ")
    );
    assert_eq!(
        normalize_stock_code("920001.BJ").as_deref(),
        Some("920001.BJ")
    );
}

#[test]
fn hkex_security_master_workbook_keeps_listed_equity_securities() {
    let securities = crate::hk_market::parse_hkex_security_master(include_bytes!(
        "../../testdata/hkex-securities-sample.xlsx"
    ))
    .expect("valid HKEX security workbook should parse");

    assert_eq!(securities.len(), 4);
    assert_eq!(securities[0].code, "00011");
    assert_eq!(securities[0].name, "Hang Seng Bank Limited");
    assert_eq!(securities[1].code, "00700");
    assert_eq!(securities[1].name, "Tencent Holdings Limited");
    assert_eq!(securities[1].currency, "HKD");
    assert_eq!(securities[1].board_lot, Some(100));
    assert_eq!(securities[2].code, "01099");
    assert_eq!(securities[2].name, "CSPC Pharmaceutical Group Limited");
    assert_eq!(securities[3].code, "80000");
    assert_eq!(securities[3].currency, "CNY");
}

#[test]
fn hkex_security_master_rejects_invalid_workbook_bytes() {
    assert!(crate::hk_market::parse_hkex_security_master(b"not an xlsx workbook").is_err());
}

#[test]
fn hk_market_cache_uses_a_separate_file_and_preserves_cn_cache_bytes() {
    let root = std::env::temp_dir().join(format!(
        "gp-assistant-hk-cache-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).expect("temporary cache directory should be created");
    let cn_path = root.join("mobile-market-data.json");
    std::fs::write(&cn_path, b"cn cache sentinel").expect("CN cache should be writable");
    let hk_path = crate::hk_market::hk_market_cache_path(&root);
    let cache = crate::hk_market::HkMarketCache {
        schema_version: 1,
        securities: vec![crate::hk_market::HkSecurity {
            code: "00700".to_string(),
            name: "Tencent Holdings Limited".to_string(),
            currency: "HKD".to_string(),
            board_lot: Some(100),
            security_type: "Equity / Equity Securities (Main Board)".to_string(),
            classification_status: crate::hk_market::HkClassificationStatus::Unverified,
        }],
        securities_source: "HKEX".to_string(),
        securities_updated_at: Some("2026-10-19T00:00:00Z".to_string()),
        securities_last_attempt_at: None,
        securities_last_error: None,
        quotes: HashMap::new(),
        histories: HashMap::new(),
        quote_errors: HashMap::new(),
        history_errors: HashMap::new(),
        financials: HashMap::new(),
        fx_rate: None,
        fx_error: None,
        disclosures: HashMap::new(),
        quarterly_updates: HashMap::new(),
        financial_errors: HashMap::new(),
        disclosure_errors: HashMap::new(),
    };

    crate::hk_market::write_hk_market_cache(&hk_path, &cache).expect("HK cache should be written");

    assert_eq!(
        hk_path.file_name().and_then(|name| name.to_str()),
        Some("hk-market-data.json")
    );
    let mut expected_cache = cache.clone();
    expected_cache.securities[0].classification_status =
        crate::hk_market::HkClassificationStatus::ExchangeListedEquity;
    assert_eq!(
        crate::hk_market::read_hk_market_cache(&hk_path).unwrap(),
        expected_cache
    );
    assert_eq!(std::fs::read(&cn_path).unwrap(), b"cn cache sentinel");
    std::fs::remove_dir_all(root).expect("temporary cache directory should be removed");
}

#[test]
fn stale_hk_cache_writes_preserve_other_securities_updates() {
    let root = std::env::temp_dir().join(format!(
        "gp-assistant-hk-concurrent-cache-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let path = hk_market_cache_path(&root);
    write_hk_market_cache(&path, &hk_test_cache()).unwrap();

    let first_quote = parse_tencent_hk_quote(
        include_str!("../../testdata/tencent-hk-quote.txt"),
        "00700.HK",
        &crate::market_domain::CurrencyCode::new("HKD").unwrap(),
    )
    .unwrap();
    let first_path = path.clone();
    let first_update = std::thread::spawn(move || {
        update_hk_market_cache(&first_path, |cache| {
            cache.quotes.insert("00700".into(), first_quote);
            Ok(())
        })
        .unwrap();
    });
    let second_path = path.clone();
    let second_update = std::thread::spawn(move || {
        let mut quote = parse_tencent_hk_quote(
            include_str!("../../testdata/tencent-hk-quote.txt"),
            "00700.HK",
            &crate::market_domain::CurrencyCode::new("HKD").unwrap(),
        )
        .unwrap();
        quote.code = "00011".into();
        update_hk_market_cache(&second_path, |cache| {
            cache.quotes.insert("00011".into(), quote);
            Ok(())
        })
        .unwrap();
    });
    first_update.join().unwrap();
    second_update.join().unwrap();

    let persisted = read_hk_market_cache(&path).unwrap();
    assert_eq!(persisted.quotes["00700"].price, 417.8);
    assert_eq!(persisted.quotes["00011"].code, "00011");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn hk_legacy_cache_promotes_to_external_sqlite_and_stays_as_read_only_recovery_copy() {
    let root = std::env::temp_dir().join(format!(
        "gp-assistant-hk-external-store-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let path = hk_market_cache_path(&root);
    let mut legacy = hk_test_cache();
    legacy.securities = vec![HkSecurity {
        code: "00700".into(),
        name: "Tencent Holdings Limited".into(),
        currency: "HKD".into(),
        board_lot: Some(100),
        security_type: "Equity".into(),
        classification_status: crate::hk_market::HkClassificationStatus::Unverified,
    }];
    legacy.securities_source = "HKEX".into();
    legacy.securities_updated_at = Some("legacy-directory-at".into());
    legacy.quotes.insert(
        "00700".into(),
        HkQuote {
            code: "00700".into(),
            name: "Tencent".into(),
            name_zh: None,
            price: 417.8,
            price_semantics_version: 1,
            previous_close: None,
            open: None,
            high: None,
            low: None,
            change: None,
            change_pct: Some(1.2),
            volume_shares: Some(100),
            turnover_hkd: Some(41780.0),
            pe: Some(15.2),
            pb: Some(3.1),
            dividend_yield_percent: Some(1.0),
            market_cap_hkd: Some(3_700_000_000_000.0),
            total_shares: Some(9_000_000_000),
            circulating_shares: Some(8_900_000_000),
            currency: "HKD".into(),
            quote_time: Some("2026/10/19 16:00:00".into()),
            source: "Tencent Finance".into(),
            fetched_at: Some("quote-at".into()),
            last_error: None,
        },
    );
    std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let source_bytes = std::fs::read(&path).unwrap();

    let imported = read_hk_market_cache(&path).unwrap();
    assert_eq!(imported.securities, legacy.securities);
    assert_eq!(imported.quotes, legacy.quotes);
    assert!(root.join("external-data/external-data.sqlite").exists());
    assert_eq!(std::fs::read(&path).unwrap(), source_bytes);

    let mut updated = imported;
    updated.quotes.get_mut("00700").unwrap().price = 420.0;
    updated.quotes.get_mut("00700").unwrap().fetched_at = Some("quote-new".into());
    write_hk_market_cache(&path, &updated).unwrap();
    let reloaded = read_hk_market_cache(&path).unwrap();
    assert_eq!(reloaded.quotes["00700"].price, 420.0);
    assert_eq!(
        reloaded.quotes["00700"].fetched_at.as_deref(),
        Some("quote-new")
    );
    assert_eq!(std::fs::read(&path).unwrap(), source_bytes);

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn hk_legacy_migration_falls_back_per_domain_when_one_domain_fails() {
    let root = std::env::temp_dir().join(format!(
        "gp-assistant-hk-partial-migration-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let path = hk_market_cache_path(&root);
    let mut legacy = hk_test_cache();
    legacy.securities = vec![HkSecurity {
        code: "00700".into(),
        name: "Stored Directory".into(),
        currency: "HKD".into(),
        board_lot: Some(100),
        security_type: "Equity / Equity Securities (Main Board)".into(),
        classification_status: crate::hk_market::HkClassificationStatus::Unverified,
    }];
    legacy.securities_source = "legacy-directory".into();
    legacy.securities_updated_at = Some("legacy-at".into());
    legacy.quotes.insert(
        "00700".into(),
        HkQuote {
            code: "00700".into(),
            name: "Tencent".into(),
            name_zh: None,
            price: 417.8,
            price_semantics_version: 1,
            previous_close: None,
            open: None,
            high: None,
            low: None,
            change: None,
            change_pct: Some(0.0),
            volume_shares: Some(100),
            turnover_hkd: Some(1000.0),
            pe: None,
            pb: None,
            dividend_yield_percent: None,
            market_cap_hkd: None,
            total_shares: None,
            circulating_shares: None,
            currency: "HKD".into(),
            quote_time: None,
            source: "legacy-quote".into(),
            fetched_at: Some("legacy-quote-at".into()),
            last_error: None,
        },
    );
    std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();

    let database_path = crate::external_data::store_path(&root);
    let _store = crate::external_data::DataStore::open(&database_path).unwrap();
    let connection = rusqlite::Connection::open(&database_path).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_hk_quote_migration BEFORE INSERT ON records
             WHEN NEW.domain='quote' AND NEW.market='hk'
             BEGIN SELECT RAISE(ABORT, 'fixture HK quote migration rejected'); END;",
        )
        .unwrap();

    let first_projection = read_hk_market_cache(&path).unwrap();
    assert_eq!(first_projection.securities[0].name, "Stored Directory");
    assert_eq!(first_projection.quotes["00700"].price, 417.8);

    let mut changed_legacy = legacy.clone();
    changed_legacy.securities[0].name = "Changed Legacy Directory".into();
    changed_legacy.quotes.get_mut("00700").unwrap().price = 420.0;
    std::fs::write(&path, serde_json::to_vec(&changed_legacy).unwrap()).unwrap();

    let reloaded = read_hk_market_cache(&path).unwrap();
    assert_eq!(reloaded.securities[0].name, "Stored Directory");
    assert_eq!(reloaded.quotes["00700"].price, 420.0);

    drop(connection);
    drop(_store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_hkex_update_does_not_replace_the_last_valid_cache() {
    let root = std::env::temp_dir().join(format!(
        "gp-assistant-hk-cache-invalid-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).expect("temporary cache directory should be created");
    let path = crate::hk_market::hk_market_cache_path(&root);
    let cache = crate::hk_market::HkMarketCache {
        schema_version: 1,
        securities: vec![crate::hk_market::HkSecurity {
            code: "00700".to_string(),
            name: "Tencent Holdings Limited".to_string(),
            currency: "HKD".to_string(),
            board_lot: Some(100),
            security_type: "Equity / Equity Securities (Main Board)".to_string(),
            classification_status: crate::hk_market::HkClassificationStatus::Unverified,
        }],
        securities_source: "HKEX".to_string(),
        securities_updated_at: Some("previous".to_string()),
        securities_last_attempt_at: None,
        securities_last_error: None,
        quotes: HashMap::new(),
        histories: HashMap::new(),
        quote_errors: HashMap::new(),
        history_errors: HashMap::new(),
        financials: HashMap::new(),
        fx_rate: None,
        fx_error: None,
        disclosures: HashMap::new(),
        quarterly_updates: HashMap::new(),
        financial_errors: HashMap::new(),
        disclosure_errors: HashMap::new(),
    };
    crate::hk_market::write_hk_market_cache(&path, &cache).unwrap();
    let mut expected_cache = cache.clone();
    expected_cache.securities[0].classification_status =
        crate::hk_market::HkClassificationStatus::ExchangeListedEquity;

    assert!(
        crate::hk_market::update_hkex_security_cache(&path, b"invalid workbook", "new").is_err()
    );
    assert_eq!(
        crate::hk_market::read_hk_market_cache(&path).unwrap(),
        expected_cache
    );
    crate::hk_market::record_hkex_refresh_error(&path, "attempt", "HKEX unavailable").unwrap();
    let recorded = crate::hk_market::read_hk_market_cache(&path).unwrap();
    assert_eq!(recorded.securities, expected_cache.securities);
    assert_eq!(
        recorded.securities_last_attempt_at.as_deref(),
        Some("attempt")
    );
    assert_eq!(
        recorded.securities_last_error.as_deref(),
        Some("HKEX unavailable")
    );
    std::fs::remove_dir_all(root).expect("temporary cache directory should be removed");
}

#[test]
fn tencent_hk_quote_keeps_hkd_and_share_units_without_a_share_multipliers() {
    let quote = crate::hk_market::parse_tencent_hk_quote(
        include_str!("../../testdata/tencent-hk-quote.txt"),
        "00700.HK",
        &crate::market_domain::CurrencyCode::new("HKD").unwrap(),
    )
    .expect("Tencent HK quote fixture should parse");
    assert_eq!(quote.code, "00700");
    assert_eq!(quote.currency, "HKD");
    assert_eq!(quote.price, 417.8);
    assert_eq!(quote.change_pct, Some(0.0));
    assert_eq!(quote.volume_shares, Some(18_752_869));
    assert_eq!(quote.turnover_hkd, Some(7_901_198_756.124));
    assert_eq!(quote.quote_time.as_deref(), Some("2026/10/16 16:08:22"));
    assert_eq!(quote.pe, Some(15.26));
    assert_eq!(quote.pb, Some(2.92));
    assert_eq!(quote.dividend_yield_percent, Some(1.27));
    assert_eq!(quote.total_shares, Some(9_092_644_829));
    let fields = serde_json::to_value(&quote).unwrap();
    assert_eq!(fields["previous_close"], 417.8);
    assert_eq!(fields["open"], 424.0);
    assert_eq!(fields["high"], 428.8);
    assert_eq!(fields["low"], 415.8);
    assert_eq!(fields["change"], 0.0);
}

#[test]
fn tencent_hk_history_maps_daily_ohlc_and_share_volume() {
    let history = crate::hk_market::parse_tencent_hk_history(
        include_str!("../../testdata/tencent-hk-kline.json"),
        "00700.HK",
    )
    .expect("Tencent HK history fixture should parse");
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].date, "2026-10-15");
    assert_eq!(history[0].open, 415.0);
    assert_eq!(history[0].high, 422.6);
    assert_eq!(history[0].low, 414.0);
    assert_eq!(history[0].close, 417.8);
    assert_eq!(history[0].volume_shares, Some(20_676_017));
}

#[test]
fn tencent_hk_parsers_reject_incomplete_payloads() {
    assert!(crate::hk_market::parse_tencent_hk_quote(
        "v_hk00700=\"\";",
        "00700.HK",
        &crate::market_domain::CurrencyCode::new("HKD").unwrap(),
    )
    .is_err());
    assert!(crate::hk_market::parse_tencent_hk_history("{}", "00700.HK").is_err());
}

#[test]
fn hk_observation_merge_keeps_history_when_quote_refresh_fails() {
    let mut cache = hk_test_cache();
    cache.histories.insert(
        "00700".to_string(),
        HkHistoryRecord {
            request_range: None,
            request_ranges: Vec::new(),
            price_adjustment: Default::default(),
            bar_fetched_at: Default::default(),
            revision_reason: None,
            suspended_as_of: None,
            bars: vec![HkDailyBar {
                date: "2026-10-16".to_string(),
                open: 424.0,
                high: 428.8,
                low: 415.8,
                close: 417.8,
                volume_shares: Some(18_752_869),
            }],
            source: "Tencent Finance".to_string(),
            fetched_at: "old-history".to_string(),
            last_error: None,
        },
    );
    let result = crate::hk_market::merge_hk_observation_update(
        &mut cache,
        "00700.HK",
        Err("quote offline".to_string()),
        Ok(HkHistoryRecord {
            bars: vec![HkDailyBar {
                date: "2026-10-17".to_string(),
                open: 418.0,
                high: 420.0,
                low: 416.0,
                close: 419.0,
                volume_shares: Some(100),
            }],
            source: "Tencent Finance".to_owned(),
            fetched_at: "new-fetch".to_owned(),
            ..Default::default()
        }),
        "new-fetch",
    );
    assert!(!result.quote_updated);
    assert!(result.history_updated);
    assert_eq!(cache.histories["00700"].fetched_at, "new-fetch");
    assert_eq!(
        cache.quote_errors.get("00700").map(String::as_str),
        Some("quote offline")
    );
}

#[test]
fn hk_observation_merge_keeps_quote_when_history_refresh_fails() {
    let mut cache = hk_test_cache();
    let result = crate::hk_market::merge_hk_observation_update(
        &mut cache,
        "00700.HK",
        Ok(HkQuote {
            code: "00700".to_string(),
            name: "Tencent".to_string(),
            name_zh: None,
            price: 419.0,
            price_semantics_version: 1,
            previous_close: None,
            open: None,
            high: None,
            low: None,
            change: None,
            change_pct: None,
            volume_shares: Some(100),
            turnover_hkd: Some(1000.0),
            pe: Some(15.0),
            pb: Some(3.0),
            dividend_yield_percent: Some(1.2),
            market_cap_hkd: Some(1000.0),
            total_shares: Some(100),
            circulating_shares: Some(90),
            currency: "HKD".to_string(),
            quote_time: Some("2026/10/17 16:00:00".to_string()),
            source: "Tencent Finance".to_string(),
            fetched_at: None,
            last_error: None,
        }),
        Err("history offline".to_string()),
        "new-fetch",
    );
    assert!(result.quote_updated);
    assert!(!result.history_updated);
    assert_eq!(
        cache.quotes["00700"].fetched_at.as_deref(),
        Some("new-fetch")
    );
    assert!(cache.histories.get("00700").is_none());
    assert_eq!(
        cache.history_errors.get("00700").map(String::as_str),
        Some("history offline")
    );
    assert!(!cache.quote_errors.contains_key("00700"));
}

#[test]
fn hk_quote_refresh_updates_cached_valuation_metrics_and_preserves_report_values() {
    let mut cache = hk_test_cache();
    let mut snapshot = parse_hk_financial_snapshot(
        include_str!("../../testdata/eastmoney-hk-financial.json"),
        "00700.HK",
        "candidate-at",
    )
    .unwrap();
    snapshot.revenue.value = Some(123.0);
    snapshot.pe.value = Some(99.0);
    snapshot.pb.value = Some(99.0);
    snapshot.candidate_fetched_at = "candidate-at".to_string();
    cache.financials.insert("00700".to_string(), snapshot);
    let quote = parse_tencent_hk_quote(
        include_str!("../../testdata/tencent-hk-quote.txt"),
        "00700.HK",
        &crate::market_domain::CurrencyCode::new("HKD").unwrap(),
    )
    .unwrap();

    let result = merge_hk_observation_update(
        &mut cache,
        "00700.HK",
        Ok(quote),
        Err("history offline".to_string()),
        "quote-at",
    );

    assert!(result.quote_updated);
    let snapshot = &cache.financials["00700"];
    assert_eq!(snapshot.revenue.value, Some(123.0));
    assert_eq!(snapshot.candidate_fetched_at, "candidate-at");
    assert_eq!(snapshot.pe.value, Some(15.26));
    assert_eq!(snapshot.pb.value, Some(2.92));
    assert_eq!(snapshot.dividend_yield.value, Some(1.27));
    assert_eq!(snapshot.market_cap.source, "Tencent Finance");
}

#[test]
fn hk_observation_merge_reports_both_failures_without_creating_data() {
    let mut cache = hk_test_cache();
    let result = crate::hk_market::merge_hk_observation_update(
        &mut cache,
        "00700.HK",
        Err("quote offline".to_string()),
        Err("history offline".to_string()),
        "new-fetch",
    );
    assert!(!result.quote_updated);
    assert!(!result.history_updated);
    assert_eq!(result.notes.len(), 2);
    assert!(cache.quotes.is_empty());
    assert!(cache.histories.is_empty());
}

#[test]
fn hk_cache_only_observation_policy_never_requests_missing_quote_or_history() {
    assert!(!hk_observation_should_refresh("cache_only"));
    assert!(hk_observation_should_refresh("refresh"));
}

#[test]
fn hk_financial_fixture_preserves_period_disclosure_currency_and_metrics() {
    let snapshot = parse_hk_financial_snapshot(
        include_str!("../../testdata/eastmoney-hk-financial.json"),
        "00700.HK",
        "2026-10-19T12:00:00Z",
    )
    .expect("HK financial fixture should parse");
    assert_eq!(snapshot.raw_report_period, "2026年 中报");
    assert_eq!(snapshot.normalized_period, "2026H1");
    assert_eq!(snapshot.report_date.as_deref(), Some("2026-06-30"));
    assert_eq!(snapshot.disclosed_at.as_deref(), Some("2026-08-25"));
    assert_eq!(snapshot.currency, "CNY");
    assert_eq!(snapshot.revenue.value, Some(401243000000.0));
    assert!((snapshot.eps.value.unwrap() - 12.639).abs() < 1e-9);
    assert_eq!(snapshot.roe.value, Some(9.966353785254));
    assert_eq!(
        snapshot.additional_metrics["revenue_yoy"].value,
        Some(10.0725325491)
    );
    assert_eq!(
        snapshot.additional_metrics["net_profit_yoy"].value,
        Some(10.9672966559)
    );
    assert_eq!(
        snapshot.additional_metrics["bps"].value,
        Some(124.92561178447)
    );
}

#[test]
fn sina_hk_financial_fixture_parses_statement_values_and_preserves_units() {
    let snapshot = parse_sina_hk_financial_snapshot(
        include_str!("../../testdata/sina-hk-financial.html"),
        "00700.HK",
        "sina-fetched-at",
    )
    .expect("Sina HK financial fixture should parse");

    assert_eq!(snapshot.normalized_period, "2026H1");
    assert_eq!(snapshot.report_date.as_deref(), Some("2026-06-30"));
    assert_eq!(snapshot.disclosed_at.as_deref(), Some("2026-08-25"));
    assert_eq!(snapshot.revenue.value, Some(401_243_000_000.0));
    assert_eq!(snapshot.revenue.currency, "HKD");
    assert_eq!(snapshot.net_profit.value, Some(117_369_000_000.0));
    assert!((snapshot.eps.value.unwrap() - 12.639).abs() < 1e-9);
    assert_eq!(snapshot.eps.unit, "currency/share");
    assert_eq!(
        snapshot.additional_metrics["revenue_yoy"].value,
        Some(10.07)
    );
    assert_eq!(snapshot.additional_metrics["bps"].value, Some(124.925));
    assert_eq!(snapshot.candidate_source, "Sina Finance HK statements");
}

#[test]
fn hkex_quarterly_update_fixture_is_separate_from_the_h1_financial_snapshot() {
    let update = parse_hkex_quarterly_update_text(
        include_str!("../../testdata/smoore-hkex-q3-update.txt"),
        "06969.HK",
        "Financial Update for the Period Ended 30 September 2026",
        "2026-10-12",
        "https://www1.hkexnews.hk/listedco/listconews/sehk/2026/1012/2026101201051.pdf",
        "2026-11-07T10:00:00Z",
    )
    .expect("HKEX Q3 update should parse");

    assert_eq!(update.code, "06969");
    assert_eq!(update.period, "2026Q3");
    assert_eq!(update.period_end, "2026-09-30");
    assert_eq!(update.disclosed_at, "2026-10-12");
    assert_eq!(update.currency, "CNY");
    assert_eq!(update.unit, "million");
    assert_eq!(update.audit_status, "unaudited");
    assert_eq!(update.quarterly_revenue, Some(3946.2));
    assert_eq!(update.quarterly_revenue_yoy_percent, Some(-6.0));
    assert_eq!(update.quarterly_revenue_qoq_percent, Some(17.7));
    assert_eq!(update.nine_month_revenue, Some(11155.1));
    assert_eq!(update.nine_month_revenue_yoy_percent, Some(9.3));
    assert_eq!(update.quarterly_net_profit, Some(365.1));
    assert_eq!(update.quarterly_net_profit_yoy_percent, Some(15.2));
    assert_eq!(update.quarterly_net_profit_qoq_percent, Some(18.0));
    assert_eq!(update.nine_month_net_profit, Some(937.0));
    assert_eq!(update.nine_month_net_profit_yoy_percent, Some(15.8));
    assert_eq!(update.quarterly_total_comprehensive_income, Some(294.3));
    assert_eq!(
        update.quarterly_total_comprehensive_income_yoy_percent,
        Some(3.6)
    );
    assert_eq!(
        update.quarterly_total_comprehensive_income_qoq_percent,
        Some(107.1)
    );
    assert_eq!(update.nine_month_total_comprehensive_income, Some(564.3));
    assert_eq!(
        update.nine_month_total_comprehensive_income_yoy_percent,
        Some(-28.2)
    );
    assert!(update.source_url.ends_with("2026101201051.pdf"));
}

#[test]
fn hkex_quarterly_search_selects_the_newest_q3_update_and_ignores_interim_results() {
    let announcements = parse_hkex_quarterly_announcements(
        include_str!("../../testdata/hkex-quarterly-search.json"),
        "06969.HK",
    );

    assert_eq!(announcements.len(), 1);
    assert_eq!(
        announcements[0].title,
        "Financial Update for the Period Ended 30 September 2026"
    );
    assert_eq!(announcements[0].disclosed_at, "2026-10-12");
    assert!(announcements[0].source_url.ends_with("2026101201051.pdf"));
}

#[test]
fn hk_quarterly_update_refresh_preserves_the_last_good_update_on_provider_failure() {
    let update = parse_hkex_quarterly_update_text(
        include_str!("../../testdata/smoore-hkex-q3-update.txt"),
        "06969.HK",
        "Financial Update for the Period Ended 30 September 2026",
        "2026-10-12",
        "https://www1.hkexnews.hk/listedco/listconews/sehk/2026/1012/2026101201051.pdf",
        "initial-fetch",
    )
    .unwrap();
    let mut cache = hk_test_cache();
    let mut financial_snapshot = parse_hk_financial_snapshot(
        include_str!("../../testdata/eastmoney-hk-financial.json"),
        "00700.HK",
        "h1-fetch",
    )
    .unwrap();
    financial_snapshot.code = "06969".to_string();
    cache
        .financials
        .insert("06969".to_string(), financial_snapshot);
    let financial_before_quarterly_refresh = build_hk_financial_result(&cache, "06969");

    merge_hk_quarterly_update_refresh(
        &mut cache,
        "06969.HK",
        Ok(Some(update.clone())),
        "first-check",
    );
    merge_hk_quarterly_update_refresh(
        &mut cache,
        "06969.HK",
        Err("HKEX request timed out".to_string()),
        "second-check",
    );

    assert_eq!(cache.quarterly_updates["06969"].update, Some(update));
    assert_eq!(cache.quarterly_updates["06969"].checked_at, "second-check");
    assert_eq!(
        cache.quarterly_updates["06969"].error.as_deref(),
        Some("HKEX request timed out")
    );
    let result = build_hk_financial_result(&cache, "06969");
    assert_eq!(result.quarterly_update.as_ref().unwrap().period, "2026Q3");
    assert_eq!(result.stale, financial_before_quarterly_refresh.stale);
    assert_eq!(
        result.field_errors,
        financial_before_quarterly_refresh.field_errors
    );
    assert!(result.quarterly_update_error.is_some());
    assert!(!result.field_errors.contains_key("quarterly_update"));
}

#[test]
fn hk_quarterly_update_refresh_never_replaces_a_newer_cached_period() {
    let current = parse_hkex_quarterly_update_text(
        include_str!("../../testdata/smoore-hkex-q3-update.txt"),
        "06969.HK",
        "Financial Update for the Period Ended 30 September 2026",
        "2026-10-12",
        "https://www1.hkexnews.hk/listedco/listconews/sehk/2026/1012/2026101201051.pdf",
        "current-fetch",
    )
    .unwrap();
    let mut older = current.clone();
    older.period = "2025Q3".to_string();
    older.period_end = "2025-09-30".to_string();
    older.disclosed_at = "2025-10-12".to_string();
    let mut cache = hk_test_cache();
    merge_hk_quarterly_update_refresh(
        &mut cache,
        "06969.HK",
        Ok(Some(current.clone())),
        "current-check",
    );

    merge_hk_quarterly_update_refresh(&mut cache, "06969.HK", Ok(Some(older)), "stale-check");

    assert_eq!(cache.quarterly_updates["06969"].update, Some(current));
}

#[test]
fn hk_financial_sources_keep_eastmoney_values_and_fill_missing_same_period_fields_from_sina() {
    let mut eastmoney = parse_hk_financial_snapshot(
        include_str!("../../testdata/eastmoney-hk-financial.json"),
        "00700.HK",
        "eastmoney-at",
    )
    .unwrap();
    eastmoney.net_profit.value = None;
    eastmoney.additional_metrics.remove("gross_margin");
    let sina = parse_sina_hk_financial_snapshot(
        include_str!("../../testdata/sina-hk-financial.html"),
        "00700.HK",
        "sina-at",
    )
    .unwrap();

    let merged = merge_hk_financial_candidates(Ok(eastmoney), Ok(sina)).unwrap();

    assert_eq!(merged.revenue.value, Some(401_243_000_000.0));
    assert!(merged.revenue.source.contains("Eastmoney"));
    assert_eq!(merged.net_profit.value, Some(117_369_000_000.0));
    assert!(merged.net_profit.source.contains("Sina"));
    assert_eq!(merged.additional_metrics["gross_margin"].value, Some(57.24));
    assert!(merged.additional_metrics["gross_margin"]
        .source
        .contains("Sina"));
    assert!(merged.candidate_source.contains("Eastmoney"));
    assert!(merged.candidate_source.contains("Sina"));
    assert_eq!(merged.candidate_fetched_at, "eastmoney-at");
    assert_eq!(
        merged.supplemental_source.as_deref(),
        Some("Sina Finance HK statements")
    );
    assert_eq!(merged.supplemental_fetched_at.as_deref(), Some("sina-at"));
}

#[test]
fn hk_financial_providers_build_code_scoped_endpoints_and_reject_invalid_codes() {
    let financial_url = eastmoney_hk_financial_endpoint("HK00700").unwrap();
    assert!(financial_url.contains("RPT_HKF10_FN_GMAININDICATOR"));
    assert!(financial_url.contains("00700.HK"));
    let disclosure_url = hkex_disclosure_endpoint("00700.HK").unwrap();
    assert!(disclosure_url.contains("name=00700"));
    assert!(disclosure_url.contains("market=SEHK"));
    assert!(eastmoney_hk_financial_endpoint("not-a-hk-code").is_err());
    assert!(hkex_disclosure_endpoint("600000.SH").is_err());
}

#[test]
fn hk_financial_parser_rejects_unknown_currency_instead_of_assuming_cny() {
    let payload = include_str!("../../testdata/eastmoney-hk-financial.json")
        .replace("\"CURRENCY\":\"人民币\",", "");
    assert!(parse_hk_financial_snapshot(&payload, "00700.HK", "now").is_err());
}

#[test]
fn hk_financial_refresh_enriches_valuation_fields_from_the_cached_hk_quote() {
    let mut cache = hk_test_cache();
    let quote = parse_tencent_hk_quote(
        include_str!("../../testdata/tencent-hk-quote.txt"),
        "00700.HK",
        &crate::market_domain::CurrencyCode::new("HKD").unwrap(),
    )
    .unwrap();
    cache.quotes.insert("00700".to_string(), quote);
    let snapshot = parse_hk_financial_snapshot(
        include_str!("../../testdata/eastmoney-hk-financial.json"),
        "00700.HK",
        "new",
    )
    .unwrap();
    merge_hk_financial_refresh(
        &mut cache,
        "00700.HK",
        Ok(snapshot),
        Err("offline".to_string()),
        Err("offline".to_string()),
    );
    let snapshot = &cache.financials["00700"];
    assert_eq!(snapshot.pe.value, Some(15.26));
    assert_eq!(snapshot.pb.value, Some(2.92));
    assert_eq!(snapshot.dividend_yield.value, Some(1.27));
    assert_eq!(snapshot.dividend_yield.unit, "percent");
    assert_eq!(snapshot.market_cap.currency, "HKD");
    assert_eq!(snapshot.total_shares.value, Some(9_092_644_829.0));
}

#[test]
fn hk_financial_result_reports_independent_update_times_and_resource_errors() {
    let mut cache = hk_test_cache();
    let snapshot = parse_hk_financial_snapshot(
        include_str!("../../testdata/eastmoney-hk-financial.json"),
        "00700.HK",
        "candidate-at",
    )
    .unwrap();
    cache.financials.insert("00700".to_string(), snapshot);
    cache.fx_rate = Some(HkFxRate {
        pair: "HKD/CNY".to_string(),
        rate: 0.85,
        source: "Frankfurter".to_string(),
        fetched_at: "fx-at".to_string(),
        observed_at: "2026-10-19".to_string(),
        last_error: None,
    });
    cache.disclosures.insert(
        "00700".to_string(),
        HkDisclosureMetadata {
            source_url: Some(
                "https://www1.hkexnews.hk/search/titlesearch.xhtml?stockId=7609".to_string(),
            ),
            source_name: "HKEXnews".to_string(),
            report_period: None,
            disclosed_at: None,
            checked_at: "official-at".to_string(),
            status: "metadata_only".to_string(),
            error: None,
        },
    );
    cache
        .financial_errors
        .insert("00700".to_string(), "candidate request failed".to_string());
    cache.fx_error = Some("rate request failed".to_string());
    cache
        .disclosure_errors
        .insert("00700".to_string(), "disclosure request failed".to_string());

    let result = build_hk_financial_result(&cache, "00700");
    assert_eq!(result.candidate_updated_at.as_deref(), Some("candidate-at"));
    assert_eq!(result.fx_updated_at.as_deref(), Some("fx-at"));
    assert_eq!(result.official_checked_at.as_deref(), Some("official-at"));
    assert_eq!(result.field_errors["financial"], "candidate request failed");
    assert_eq!(result.field_errors["fx_rate"], "rate request failed");
    assert_eq!(
        result.field_errors["official_disclosure"],
        "disclosure request failed"
    );
    assert!(result.stale);
}

#[test]
fn hk_financial_refresh_failure_preserves_all_prior_sections() {
    let mut cache = hk_test_cache();
    let mut snapshot = parse_hk_financial_snapshot(
        include_str!("../../testdata/eastmoney-hk-financial.json"),
        "00700.HK",
        "old-candidate",
    )
    .unwrap();
    snapshot.revenue.value = Some(321.0);
    cache.financials.insert("00700".to_string(), snapshot);
    cache.fx_rate = Some(HkFxRate {
        pair: "HKD/CNY".to_string(),
        rate: 0.84,
        source: "Frankfurter".to_string(),
        fetched_at: "old-fx".to_string(),
        observed_at: "2026-10-18".to_string(),
        last_error: None,
    });
    let old_disclosure = HkDisclosureMetadata {
        source_url: Some("https://example.test/old".to_string()),
        source_name: "HKEXnews".to_string(),
        report_period: None,
        disclosed_at: None,
        checked_at: "old-official".to_string(),
        status: "metadata_only".to_string(),
        error: None,
    };
    cache
        .disclosures
        .insert("00700".to_string(), old_disclosure.clone());

    let merged = merge_hk_financial_refresh(
        &mut cache,
        "00700.HK",
        Err("candidate down".to_string()),
        Err("fx down".to_string()),
        Err("official down".to_string()),
    );

    assert_eq!(cache.financials["00700"].revenue.value, Some(321.0));
    assert_eq!(
        cache.financials["00700"].candidate_fetched_at,
        "old-candidate"
    );
    assert_eq!(cache.fx_rate.as_ref().unwrap().fetched_at, "old-fx");
    assert_eq!(cache.disclosures["00700"], old_disclosure);
    assert_eq!(merged.field_errors.len(), 3);
    assert_eq!(merged.notes.len(), 3);
}

#[test]
fn hk_fx_rate_parser_rejects_missing_rate_and_keeps_observed_date() {
    let rate = parse_hk_fx_rate(
        r#"{"date":"2026-10-19","base":"HKD","quote":"CNY","rate":0.85078}"#,
        "2026-10-19T12:00:00Z",
    )
    .unwrap();
    assert_eq!(rate.pair, "HKD/CNY");
    assert_eq!(rate.rate, 0.85078);
    assert_eq!(rate.observed_at, "2026-10-19");
    assert_eq!(rate.source, "European Central Bank (via Frankfurter)");
    assert!(parse_hk_fx_rate(r#"{"base":"HKD","quote":"CNY"}"#, "now").is_err());
}

#[test]
fn hk_fx_rate_parser_rejects_unexpected_currency_pairs() {
    let response = r#"{"date":"2026-10-19","base":"USD","quote":"CNY","rate":7.12}"#;
    assert!(parse_hk_fx_rate(response, "2026-10-19T12:00:00Z")
        .unwrap_err()
        .contains("currency pair"));
}

#[test]
fn hk_financial_snapshot_marks_conflicts_without_dropping_candidate_value() {
    let mut snapshot = parse_hk_financial_snapshot(
        include_str!("../../testdata/eastmoney-hk-financial.json"),
        "00700.HK",
        "now",
    )
    .unwrap();
    snapshot.official_source_url =
        Some("https://www1.hkexnews.hk/search/titlesearch.xhtml?stockId=00700".to_string());
    snapshot.official_source_name = Some("HKEXnews".to_string());
    snapshot.verification_status = "conflict".to_string();
    assert_eq!(snapshot.verification_status, "conflict");
    assert!(snapshot.revenue.value.is_some());
}

#[test]
fn hk_financial_verifier_keeps_candidate_and_official_values_on_conflict() {
    let mut snapshot = parse_hk_financial_snapshot(
        include_str!("../../testdata/eastmoney-hk-financial.json"),
        "00700.HK",
        "now",
    )
    .unwrap();
    let official = HashMap::from([(
        "revenue".to_string(),
        HkFinancialMetric {
            value: Some(410_000_000_000.0),
            currency: "CNY".to_string(),
            unit: "currency_total".to_string(),
            source: "Issuer annual report".to_string(),
            reason: None,
        },
    )]);
    verify_hk_financial_snapshot(&mut snapshot, official);
    assert_eq!(snapshot.verification_status, "conflict");
    assert_eq!(snapshot.revenue.value, Some(401_243_000_000.0));
    assert_eq!(
        snapshot.official_values["revenue"].value,
        Some(410_000_000_000.0)
    );
    assert_eq!(snapshot.verification_conflicts, vec!["revenue"]);
}

#[test]
fn hk_financial_currency_pair_uses_fx_only_for_available_pair() {
    let metric = HkFinancialMetric {
        value: Some(100.0),
        currency: "CNY".to_string(),
        unit: "currency_total".to_string(),
        source: "fixture".to_string(),
        reason: None,
    };
    let rate = HkFxRate {
        pair: "HKD/CNY".to_string(),
        rate: 0.85,
        source: "Frankfurter".to_string(),
        fetched_at: "now".to_string(),
        observed_at: "2026-10-19".to_string(),
        last_error: None,
    };
    let converted = hk_financial_currency_pair(&metric, Some(&rate));
    assert_eq!(converted.hkd, Some(100.0 / 0.85));
    assert_eq!(converted.cny, Some(100.0));
    let unavailable = hk_financial_currency_pair(&metric, None);
    assert_eq!(unavailable.hkd, None);
    assert_eq!(unavailable.cny, Some(100.0));
}

#[test]
fn hk_disclosure_lookup_maps_code_to_official_hkex_stock_id() {
    let disclosure = parse_hk_disclosure_lookup(
        "callback({\"stockInfo\":[{\"stockId\":7609,\"code\":\"00700\",\"name\":\"TENCENT\"}]});",
        "00700.HK",
        "now",
    )
    .unwrap();
    assert_eq!(disclosure.status, "metadata_only");
    assert!(disclosure
        .source_url
        .as_deref()
        .unwrap()
        .contains("stockId=7609"));
    assert_eq!(disclosure.source_name, "HKEXnews");
    let unavailable =
        parse_hk_disclosure_lookup("callback({\"stockInfo\":[]});", "00700.HK", "now").unwrap();
    assert_eq!(unavailable.status, "unavailable");
    assert!(unavailable.source_url.is_none());
}

#[test]
fn hk_financial_refresh_merge_keeps_each_success_independently() {
    let mut cache = hk_test_cache();
    let old_rate = HkFxRate {
        pair: "HKD/CNY".to_string(),
        rate: 0.84,
        source: "Frankfurter".to_string(),
        fetched_at: "old".to_string(),
        observed_at: "2026-10-18".to_string(),
        last_error: None,
    };
    cache.fx_rate = Some(old_rate);
    let snapshot = parse_hk_financial_snapshot(
        include_str!("../../testdata/eastmoney-hk-financial.json"),
        "00700.HK",
        "new",
    )
    .unwrap();
    let disclosure = HkDisclosureMetadata {
        source_url: Some("https://example.test/hkex".to_string()),
        source_name: "HKEXnews".to_string(),
        report_period: None,
        disclosed_at: None,
        checked_at: "new".to_string(),
        status: "metadata_only".to_string(),
        error: None,
    };
    let result = merge_hk_financial_refresh(
        &mut cache,
        "00700.HK",
        Ok(snapshot),
        Err("FX unavailable".to_string()),
        Ok(disclosure),
    );
    assert!(result.snapshot_updated);
    assert!(!result.fx_updated);
    assert!(result.disclosure_updated);
    assert!(cache.financials.contains_key("00700"));
    assert_eq!(cache.fx_rate.as_ref().unwrap().rate, 0.84);
    assert_eq!(
        cache.fx_rate.as_ref().unwrap().last_error.as_deref(),
        Some("FX unavailable")
    );
    assert!(cache.disclosures.contains_key("00700"));
    assert_eq!(
        cache.financials["00700"].verification_status,
        "metadata_only"
    );
}

#[test]
fn hk_manual_financial_refresh_keeps_same_period_company_metadata_after_profile_failure() {
    let mut cache = hk_test_cache();
    let mut previous = parse_hk_financial_snapshot(
        include_str!("../../testdata/eastmoney-hk-financial.json"),
        "00700.HK",
        "old-at",
    )
    .unwrap();
    previous.company_profile = Some(crate::hk_market::HkCompanyProfile {
        code: "00700".to_owned(),
        name: "腾讯控股有限公司".to_owned(),
        name_zh: Some("腾讯控股".to_owned()),
        industry: "软件服务".to_owned(),
        business_summary: "提供互联网服务。".to_owned(),
        source: "Eastmoney HK company profile".to_owned(),
        fetched_at: "old-at".to_owned(),
    });
    let expected_profile = previous.company_profile.clone();
    cache.financials.insert("00700".to_owned(), previous);
    let mut refreshed = parse_hk_financial_snapshot(
        include_str!("../../testdata/eastmoney-hk-financial.json"),
        "00700.HK",
        "new-at",
    )
    .unwrap();
    refreshed.field_errors.insert(
        "company_profile".to_owned(),
        "profile request failed".to_owned(),
    );
    let merged = merge_hk_financial_refresh(
        &mut cache,
        "00700.HK",
        Ok(refreshed),
        Err("fx unavailable".to_owned()),
        Err("disclosure unavailable".to_owned()),
    );
    assert!(merged.snapshot_updated);
    assert_eq!(cache.financials["00700"].company_profile, expected_profile);
    assert_eq!(cache.financials["00700"].candidate_fetched_at, "new-at");
    assert_eq!(
        cache.financials["00700"].field_errors["company_profile"],
        "profile request failed"
    );
}

#[test]
fn hk_financial_cache_sections_survive_atomic_write_and_reload() {
    let root = std::env::temp_dir().join(format!(
        "gp-assistant-hk-financial-cache-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let path = hk_market_cache_path(&root);
    let mut cache = hk_test_cache();
    cache.financials.insert(
        "00700".to_string(),
        parse_hk_financial_snapshot(
            include_str!("../../testdata/eastmoney-hk-financial.json"),
            "00700.HK",
            "candidate-at",
        )
        .unwrap(),
    );
    cache.fx_rate = Some(HkFxRate {
        pair: "HKD/CNY".to_string(),
        rate: 0.85,
        source: "Frankfurter".to_string(),
        fetched_at: "fx-at".to_string(),
        observed_at: "2026-10-19".to_string(),
        last_error: None,
    });
    cache.disclosures.insert(
        "00700".to_string(),
        HkDisclosureMetadata {
            source_url: Some("https://example.test/hkex".to_string()),
            source_name: "HKEXnews".to_string(),
            report_period: None,
            disclosed_at: None,
            checked_at: "official-at".to_string(),
            status: "metadata_only".to_string(),
            error: None,
        },
    );
    let update = parse_hkex_quarterly_update_text(
        include_str!("../../testdata/smoore-hkex-q3-update.txt"),
        "06969.HK",
        "Financial Update for the Period Ended 30 September 2026",
        "2026-10-12",
        "https://www1.hkexnews.hk/listedco/listconews/sehk/2026/1012/2026101201051.pdf",
        "update-fetch",
    )
    .unwrap();
    cache.quarterly_updates.insert(
        "06969".to_string(),
        HkQuarterlyUpdateRecord {
            update: Some(update),
            checked_at: "update-check".to_string(),
            error: None,
        },
    );

    write_hk_market_cache(&path, &cache).unwrap();

    assert_eq!(read_hk_market_cache(&path).unwrap(), cache);
    std::fs::remove_dir_all(root).unwrap();
}
