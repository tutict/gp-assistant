#[test]
fn cn_market_refresh_reads_batch_financial_reports_without_losing_negative_or_zero_values() {
    let reports = json!({"result":{"data":[{
        "SECUCODE":"000002.SZ", "SECURITY_CODE":"000002",
        "REPORTDATE":"2026-09-30 00:00:00", "NOTICE_DATE":"2026-10-30 00:00:00",
        "BASIC_EPS":-2.01, "BPS":7.80289650705, "WEIGHTAVG_ROE":-22.8,
        "TOTAL_OPERATE_INCOME":104998998452.2, "PARENT_NETPROFIT":-23943928057.47,
        "DEDUCT_PARENT_NETPROFIT":-21537523659.32, "DPN_RATIO":0.0
    }]}});
    let report = parse_latest_observe_financial_snapshot(&reports, "000002.SZ").unwrap();
    assert_eq!(report["period"], "2026-09-30");
    assert_eq!(report["latest_eps"], -2.01);
    assert_eq!(report["roe"], -22.8);
    assert_eq!(report["deducted_net_profit_growth_rate"], 0.0);
    assert!((report["operating_revenue_billion"].as_f64().unwrap() - 1049.989984522).abs() < 1e-8);
    assert!(
        (report["deducted_net_profit_billion"].as_f64().unwrap() + 215.3752365932).abs() < 1e-8
    );
    assert_eq!(report["report_notice_date"], "2026-10-30 00:00:00");
}

#[test]
fn cn_market_refresh_does_not_downgrade_reports_during_quote_batch_merges() {
    let seed = json!({
        "stocks":[{"code":"000002.SZ", "period":"2026Q1", "latest_eps":-0.5}],
        "financials":{"000002.SZ":{
            "period":"2026Q3", "latest_eps":-2.01, "roe":-22.8,
            "source":"eastmoney_cn_financial", "report_notice_date":"2026-10-30"
        }}
    });
    let bundled = json!({"financials":{"000002.SZ":{
        "period":"2026Q1", "latest_eps":-0.5, "roe":-5.23, "source":"old-bundled-snapshot"
    }}});
    let merged =
        filtered_financial_snapshot_map(&seed, &bundled, &HashSet::from(["000002.SZ".to_string()]));
    assert_eq!(merged["000002.SZ"]["period"], "2026Q3");
    assert_eq!(merged["000002.SZ"]["latest_eps"], -2.01);
    assert_eq!(merged["000002.SZ"]["roe"], -22.8);
    assert_eq!(merged["000002.SZ"]["report_notice_date"], "2026-10-30");
}

#[test]
fn cn_market_refresh_projects_reported_loss_roe_into_screening_with_ratio_units() {
    let mut data = json!({"stocks":[{
        "code":"000002.SZ", "name":"万科A", "price":6.08, "roe":null, "latest_eps":-0.5
    }]});
    let fields = json!({"period":"2026Q3", "roe":-22.8, "latest_eps":-2.01});
    assert!(merge_observe_fundamental_supplement(
        &mut data,
        "000002.SZ",
        fields.as_object().unwrap().clone()
    ));
    assert_eq!(data["stocks"][0]["latest_eps"], -2.01);
    assert!((data["stocks"][0]["roe"].as_f64().unwrap() + 0.228).abs() < 1e-12);
}

#[test]
fn tencent_quote_refresh_preserves_an_existing_st_flag() {
    let mut fields = vec![""; 80];
    fields[1] = "普通名称";
    fields[2] = "000001";
    fields[3] = "10.0";
    let seed = HashMap::from([(
        "000001.SZ".to_string(),
        json!({"is_st": true}).as_object().unwrap().clone(),
    )]);
    let stocks = parse_tencent_quotes(&format!("v_sz000001=\"{}\";", fields.join("~")), &seed, false);
    assert_eq!(stocks[0]["is_st"], true);
}

#[test]
fn cn_market_refresh_keeps_reported_negative_valuation_ratios() {
    let mut fields = vec![""; 80];
    fields[1] = "万科A";
    fields[2] = "000002";
    fields[3] = "6.08";
    fields[4] = "6.08";
    fields[30] = "20261106150000";
    fields[39] = "-6.32";
    fields[46] = "-0.42";
    fields[52] = "18.0";
    let text = format!("v_sz000002=\"{}\";", fields.join("~"));
    let seed = HashMap::from([(
        "000002.SZ".to_string(),
        json!({"pe":20.0,"pb":0.8,"roe":-0.228})
            .as_object()
            .unwrap()
            .clone(),
    )]);
    let stocks = parse_tencent_quotes(&text, &seed, false);
    assert_eq!(stocks[0]["pe"], -6.32);
    assert_eq!(stocks[0]["pb"], -0.42);
    assert_eq!(stocks[0]["roe"], -0.228);
}

#[test]
fn cn_market_refresh_persists_report_fields_period_and_publication_evidence() {
    let root = std::env::temp_dir().join(format!(
        "gp-cn-report-roundtrip-{}-{}",
        std::process::id(),
        epoch_millis()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let data = json!({
        "stocks":[{"code":"000002.SZ","name":"万科A","industry":"房地产","price":6.08,"roe":-0.228,"period":"2026Q3","latest_bps":7.80289650705}],
        "financials":{"000002.SZ":{
            "period":"2026Q3", "source":"eastmoney_cn_financial", "currency":"CNY",
            "latest_eps":-2.01, "roe":-22.8,
            "deducted_net_profit_billion":-215.3752365932, "deducted_net_profit_growth_rate":0.0,
            "report_notice_date":"2026-10-30", "report_fetched_at":"1794105983398"
        }}
    });
    let mut store =
        crate::external_data::DataStore::open(crate::external_data::store_path(&root)).unwrap();
    store_market_dataset(&mut store, &data, "tencent").unwrap();
    let projected = project_market_store(&store).unwrap().unwrap();
    assert_eq!(
        projected["financials"]["000002.SZ"]["report_notice_date"],
        "2026-10-30"
    );
    assert_eq!(
        projected["financials"]["000002.SZ"]["report_fetched_at"],
        "1794105983398"
    );
    assert_eq!(projected["financials"]["000002.SZ"]["currency"], "CNY");
    assert_eq!(
        projected["financials"]["000002.SZ"]["deducted_net_profit_growth_rate"],
        0.0
    );
    assert_eq!(projected["stocks"][0]["period"], "2026Q3");
    assert_eq!(projected["stocks"][0]["latest_bps"], 7.80289650705);
    drop(store);
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cn_data_weekend_refresh_uses_the_last_trading_day_for_quote_coverage() {
    let generated = parse_cache_datetime_epoch_ms("2026-11-08T02:00:00Z").unwrap();
    let data = json!({"generated_at_epoch_ms":generated,"stocks":[{
        "code":"000002.SZ","price":6.08,"change_pct":0.01,"quote_time":"20261106150000"
    }]});
    let cache = market_cache_record(
        Path::new("fixture.json"),
        0,
        None,
        json!({"stock_count":1}),
        &data,
        false,
        "fixture",
    );
    assert_eq!(cache["quote_coverage_trade_date"], "20261106");
    assert_eq!(cache["quote_observed"], 1);
    assert!(!market_quote_cache_stale(
        Some(generated),
        generated,
        Some(1.0)
    ));
}

#[test]
fn cn_data_reported_missing_roe_does_not_turn_into_a_valuation_estimate() {
    let mut fields = vec![""; 80];
    fields[1] = "样本公司";
    fields[2] = "000001";
    fields[3] = "12.0";
    fields[39] = "20.0";
    fields[46] = "2.0";
    let seed = HashMap::from([(
        "000001.SZ".to_string(),
        json!({"roe":null,"financial_source":"eastmoney_cn_financial","latest_eps":0.5})
            .as_object()
            .unwrap()
            .clone(),
    )]);
    let stocks = parse_tencent_quotes(
        &format!("v_sz000001=\"{}\";", fields.join("~")),
        &seed,
        false,
    );
    assert!(stocks[0]["roe"].is_null());
}

#[test]
fn cn_data_loss_report_does_not_reuse_a_missing_quotes_old_positive_pe() {
    let mut fields = vec![""; 80];
    fields[1] = "样本公司";
    fields[2] = "000001";
    fields[3] = "12.0";
    fields[46] = "2.0";
    let seed = HashMap::from([(
        "000001.SZ".to_string(),
        json!({"pe":20.0,"latest_eps":-0.5,"financial_source":"eastmoney_cn_financial"})
            .as_object()
            .unwrap()
            .clone(),
    )]);
    let stocks = parse_tencent_quotes(
        &format!("v_sz000001=\"{}\";", fields.join("~")),
        &seed,
        false,
    );
    assert!(stocks[0]["pe"].is_null());
}

#[test]
fn cn_data_dynamic_pe_is_not_substituted_for_a_missing_ttm_pe() {
    let mut fields = vec![""; 80];
    fields[1] = "光庭信息";
    fields[2] = "301221";
    fields[3] = "26.7";
    fields[52] = "-478837.88";
    let stocks = parse_tencent_quotes(
        &format!("v_sz301221=\"{}\";", fields.join("~")),
        &HashMap::new(),
        false,
    );
    assert!(stocks[0]["pe"].is_null());
}

#[test]
fn cn_data_quote_enrichment_keeps_new_report_holes_empty() {
    let seed=HashMap::from([("002389.SZ".to_string(),json!({"code":"002389.SZ","period":"2026Q3","latest_eps":null,"deducted_net_profit_growth_rate":null}).as_object().unwrap().clone())]);
    for snapshot in [
        json!({"financials":{"002389.SZ":{"period":"2026Q1","latest_eps":0.1026,"deducted_net_profit_growth_rate":25.0}}}),
        json!({"financials":{"002389.SZ":{"latest_eps":0.1026}}}),
    ] {
        let stocks = enriched_stock_maps(&seed, &snapshot);
        assert!(stocks["002389.SZ"]["latest_eps"].is_null());
        assert!(stocks["002389.SZ"]["deducted_net_profit_growth_rate"].is_null());
    }
}

#[test]
fn cn_data_adaptive_screen_reports_short_candidate_history_without_blocking_the_market() {
    let mut coverage = vec![
        json!({"code":"001246.SZ","kind":"history","sufficient":false,"available_bars":23,"required_bars":60,"as_of":"20261106","end_date":"20261106"}),
    ];
    for code in [
        "000002.SZ",
        "600519.SH",
        "000001.SZ",
        "600000.SH",
        "000001.SH",
        "399001.SZ",
        "399006.SZ",
    ] {
        coverage.push(json!({"code":code,"kind":"history","sufficient":true,"available_bars":120,"required_bars":60,"as_of":"20261106"}));
    }
    let gaps = adaptive_screen_candidate_gaps(&coverage).unwrap();
    assert_eq!(gaps.len(), 1);
    assert_eq!(gaps[0]["code"], "001246.SZ");
    assert_eq!(gaps[0]["available_bars"], 23);
    coverage
        .iter_mut()
        .find(|item| item["code"] == "399001.SZ")
        .unwrap()["sufficient"] = json!(false);
    let error: Value =
        serde_json::from_str(&adaptive_screen_candidate_gaps(&coverage).unwrap_err()).unwrap();
    assert_eq!(error["missing_count"], 1);
    assert_eq!(error["missing"][0]["code"], "399001.SZ");
    coverage
        .iter_mut()
        .find(|item| item["code"] == "399001.SZ")
        .unwrap()["sufficient"] = json!(true);
    coverage[0]["as_of"] = Value::Null;
    let error: Value =
        serde_json::from_str(&adaptive_screen_candidate_gaps(&coverage).unwrap_err()).unwrap();
    assert_eq!(error["missing"][0]["code"], "001246.SZ");
}

#[test]
fn cn_data_legacy_projection_retains_current_reference_metadata_and_report_identity() {
    let root = std::env::temp_dir().join(format!(
        "gp-cn-reference-projection-{}-{}",
        std::process::id(),
        epoch_millis()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let reference = json!({"directory":{"checked":true,"count":1,"fetched_at":"1794113089918"},"financial":{"latest_report_period":"2026Q3","fetched_at":"1794113089918"},"calendar":{"checked":true,"coverage_start":"20260930","coverage_end":"20261108","trading_dates":["20260930","20261106"]}});
    let latest = json!({"source":"tencent","generated_at_epoch_ms":1794113111699_u64,"cn_reference":reference,
        "archived_stocks":{"000003.SZ":{"code":"000003.SZ","name":"旧证券"}},
        "stocks":[{"code":"000002.SZ","name":"万科A","industry":"房地产","price":3.24,"quote_time":"20261106161442","period":"2026Q3","latest_bps":7.8,"financial_source":"eastmoney_cn_financial","financial_report_notice_date":"2026-10-30"}],
        "financials":{"000002.SZ":{"period":"2026Q3","latest_eps":-2.01,"source":"eastmoney_cn_financial","report_notice_date":"2026-10-30","report_fetched_at":"1794113089918"}}});
    let legacy = json!({"stocks":[{"code":"000002.SZ","name":"旧名称","industry":"房地产","price":6.0,"period":"2026Q1","latest_bps":12.0}],"cn_reference":{"directory":{"checked":false}},"generated_at":"old"});
    let mut store =
        crate::external_data::DataStore::open(crate::external_data::store_path(&root)).unwrap();
    store_market_dataset(&mut store, &latest, "tencent").unwrap();
    let promoted = external_market_domains().into_iter().collect();
    let projected = project_market_store_with_legacy(&store, Some(&legacy), &promoted)
        .unwrap()
        .unwrap();
    assert_eq!(projected["cn_reference"], reference);
    assert_eq!(projected["archived_stocks"], latest["archived_stocks"]);
    assert_eq!(projected["stocks"][0]["period"], "2026Q3");
    assert_eq!(projected["stocks"][0]["latest_bps"], 7.8);
    assert_eq!(
        projected["stocks"][0]["financial_source"],
        "eastmoney_cn_financial"
    );
    let cache = market_cache_record(
        Path::new("fixture.json"),
        0,
        None,
        json!({"stock_count":1}),
        &projected,
        false,
        "fixture",
    );
    assert_eq!(
        cache["cn_reference"]["financial"]["latest_report_period"],
        "2026Q3"
    );
    assert_eq!(cache["quote_coverage_trade_date"], "20261106");
    drop(store);
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cn_backtest_range_patch_preserves_reference_metadata_and_tracks_each_series() {
    let mut data = json!({"cn_reference":{"directory":{"checked":true,"count":5579},"history_ranges":{"300390.SZ":{"bars_hash":"prior"}}}});
    let patch = json!({"histories":{"300750.SZ":[{"date":"2026-10-16","close":10.0}]},"cn_reference":{"history_ranges":{"300750.SZ":{"bars_hash":"new"}}}});
    let selected = market_data_patch_for_codes(&patch, &["300750.SZ".to_string()]);
    assert_eq!(
        selected["cn_reference"]["history_ranges"]["300750.SZ"]["bars_hash"],
        "new"
    );
    apply_market_data_patch(&mut data, &selected);
    assert_eq!(data["cn_reference"]["directory"]["checked"], true);
    assert_eq!(
        data["cn_reference"]["history_ranges"]["300390.SZ"]["bars_hash"],
        "prior"
    );
    assert_eq!(
        data["cn_reference"]["history_ranges"]["300750.SZ"]["bars_hash"],
        "new"
    );
    let proof_only =
        json!({"cn_reference":{"history_ranges":{"301358.SZ":{"bars_hash":"proof-only"}}}});
    assert_eq!(
        market_data_patch_codes(&proof_only),
        vec!["301358.SZ".to_string()]
    );
    let selected = market_data_patch_for_codes(&data, &["300750.SZ".to_string()]);
    assert!(selected["cn_reference"]["history_ranges"]
        .get("300390.SZ")
        .is_none());
}

#[test]
fn cn_data_calendar_covers_holidays_and_the_first_session_before_open() {
    let calendar = json!({"checked":true,"coverage_start":"20260901","coverage_end":"20261108","trading_dates":["20260929","20260930","20261008","20261106"]});
    for time in ["2026-10-06T04:00:00Z", "2026-10-08T01:00:00Z"] {
        let epoch = parse_cache_datetime_epoch_ms(time).unwrap();
        assert_eq!(
            expected_market_quote_date_with_calendar(epoch, Some(&calendar)).as_deref(),
            Some("20260930")
        );
    }
    let epoch = parse_cache_datetime_epoch_ms("2026-10-08T02:00:00Z").unwrap();
    assert_eq!(
        expected_market_quote_date_with_calendar(epoch, Some(&calendar)).as_deref(),
        Some("20261008")
    );
}

#[test]
fn tencent_daily_history_uses_count_only_param_and_filters_dates_locally() {
    assert_eq!(
        tencent_daily_history_param("sh600266"),
        format!("sh600266,day,,,{OBSERVE_DAILY_HISTORY_LIMIT},")
    );

    let rows = vec![
        json!({ "date": "2026-01-01", "close": 10.0 }),
        json!({ "date": "2026-02-01", "close": 11.0 }),
        json!({ "date": "2026-03-01", "close": 12.0 }),
    ];
    let filtered = filter_daily_history_rows_by_date(rows, "20260201", "20260228");

    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0]["date"].as_str(), Some("2026-02-01"));
}

#[test]
fn market_refresh_preserves_cached_adaptive_benchmark_histories() {
    let filtered = filter_seed_histories(
        &json!({
            "histories": {
                "600000.SH": [{ "date": "20260729", "close": 10.0 }],
                "000001.SH": [{ "date": "20260729", "close": 3800.0 }],
                "399001.SZ": [{ "date": "20260729", "close": 12000.0 }],
                "399006.SZ": [{ "date": "20260729", "close": 2500.0 }],
                "900001.SH": [{ "date": "20260729", "close": 1.0 }]
            }
        }),
        &HashSet::from(["600000.SH".to_string()]),
    );
    assert!(filtered.get("600000.SH").is_some());
    for code in adaptive_benchmark_codes() {
        assert!(filtered.get(code).is_some(), "{code} should remain cached");
    }
    assert!(filtered.get("900001.SH").is_none());
}

#[test]
fn market_refresh_history_warmup_targets_default_candidates_and_benchmarks() {
    let stocks = (0..6)
        .map(|index| {
            json!({
                "code": format!("{:06}.SZ", index + 1),
                "name": format!("样本{index}"),
                "industry": "测试行业",
                "price": 10.0 + index as f64,
                "roe": 0.12,
                "pe": 10.0,
                "pb": 1.2,
                "market_cap_billion": 20.0,
                "is_st": false
            })
        })
        .collect::<Vec<_>>();
    let codes = market_refresh_history_codes(&stocks, 4);
    assert!(codes.len() <= 11);
    assert!(codes.iter().any(|code| code == "000001.SH"));
    assert!(codes.iter().any(|code| code == "399001.SZ"));
    assert!(codes.iter().any(|code| code == "399006.SZ"));
    assert!(codes.iter().any(|code| code.ends_with(".SZ")));
}

#[test]
fn market_refresh_history_warmup_keeps_non_diversified_trend_candidates() {
    let stocks = (0..12)
        .map(|index| {
            json!({
                "code": format!("{:06}.SZ", index + 1),
                "name": format!("半导体样本{index}"),
                "industry": "半导体",
                "price": 10.0 + index as f64,
                "roe": 0.08 + index as f64 / 100.0,
                "pe": 10.0 + index as f64,
                "pb": 1.2,
                "market_cap_billion": 20.0 + index as f64,
                "is_st": false
            })
        })
        .collect::<Vec<_>>();
    let typed = serde_json::from_value::<Vec<gp_core::StockItem>>(json!(stocks.clone()))
        .expect("fixture stocks should parse");
    let mut criteria = gp_core::ScreenCriteria::default();
    criteria.limit = 4;
    let trend_codes = gp_core::screen_stocks(&typed, &criteria)
        .items
        .into_iter()
        .map(|item| item.stock.code)
        .collect::<HashSet<_>>();
    let warmup_codes = market_refresh_history_codes(&stocks, 4)
        .into_iter()
        .collect::<HashSet<_>>();
    assert!(trend_codes.is_subset(&warmup_codes));
}

#[test]
fn daily_history_prefers_the_longer_source_result() {
    let short = vec![json!({"date":"20261001", "close":10.0})];
    let long = vec![
        json!({"date":"20260930", "close":9.0}),
        json!({"date":"20261001", "close":10.0}),
    ];
    assert_eq!(
        choose_longer_history(Some(short), Some(long.clone())),
        Some(long)
    );
}

#[test]
fn append_preserved_seed_stocks_keeps_seed_rows_and_fills_missing_industry() {
    let seed = json!({
        "stocks": [
            {"code": "000001.SZ", "name": "Ping An Bank", "industry": "", "price": 10.0},
            {"code": "600000.SH", "name": "SPD Bank", "industry": "Banking", "price": 8.0}
        ]
    });
    let (seed_stocks, seed_codes) = seed_stock_maps(&seed);
    let mut stocks = Vec::new();
    let mut seen = HashSet::new();

    let preserved = append_preserved_seed_stocks(&seed_codes, &seed_stocks, &mut stocks, &mut seen);

    assert_eq!(preserved, 2);
    assert_eq!(stocks.len(), 2);
    let sz_stock = stocks
        .iter()
        .find(|stock| stock.get("code").and_then(Value::as_str) == Some("000001.SZ"))
        .expect("SZ stock should be preserved");
    assert_eq!(
        sz_stock.get("industry").and_then(Value::as_str),
        Some("\u{6df1}\u{5e02}A\u{80a1}")
    );
    let sh_stock = stocks
        .iter()
        .find(|stock| stock.get("code").and_then(Value::as_str) == Some("600000.SH"))
        .expect("SH stock should be preserved");
    assert_eq!(
        sh_stock.get("industry").and_then(Value::as_str),
        Some("Banking")
    );
}

#[test]
fn append_preserved_seed_stocks_skips_already_fetched_rows() {
    let seed = json!({
        "stocks": [
            {"code": "000001.SZ", "name": "Ping An Bank", "price": 10.0},
            {"code": "600000.SH", "name": "SPD Bank", "price": 8.0}
        ]
    });
    let (seed_stocks, seed_codes) = seed_stock_maps(&seed);
    let mut stocks = vec![json!({"code": "000001.SZ", "name": "Live Quote"})];
    let mut seen = HashSet::from(["000001.SZ".to_string()]);

    let preserved = append_preserved_seed_stocks(&seed_codes, &seed_stocks, &mut stocks, &mut seen);

    assert_eq!(preserved, 1);
    assert_eq!(stocks.len(), 2);
    assert!(stocks
        .iter()
        .any(|stock| stock.get("code").and_then(Value::as_str) == Some("600000.SH")));
}

#[test]
fn tencent_candidates_are_interleaved_across_markets() {
    let mut codes = Vec::new();

    append_tencent_candidate_codes(&mut codes);

    assert_eq!(
        &codes[..5],
        &[
            "000001.SZ".to_string(),
            "600000.SH".to_string(),
            "300000.SZ".to_string(),
            "688000.SH".to_string(),
            "920000.BJ".to_string(),
        ]
    );
    assert!(codes.iter().position(|code| code == "600010.SH").unwrap() < 64);
    assert!(codes.iter().position(|code| code == "300010.SZ").unwrap() < 64);
    assert!(codes.iter().position(|code| code == "688010.SH").unwrap() < 64);
    assert!(codes.iter().position(|code| code == "920010.BJ").unwrap() < 64);
}

#[test]
fn tencent_batch_size_keeps_full_refresh_round_trips_reasonable() {
    assert_eq!(15_000usize.div_ceil(TENCENT_BATCH_SIZE), 125);
}

#[test]
fn tencent_quote_parser_extracts_market_cap_and_share_structure() {
    // Preserve the complete field shape of a public Tencent quote sample so
    // index drift around the market-cap/share-count tail is caught by tests.
    let raw = r#"v_sh600941="1~中国移动~600941~93.01~92.80~92.79~50872~23976~26896~93.01~3~93.00~33~92.99~67~92.98~51~92.97~2~93.02~7~93.03~44~93.04~22~93.05~7~93.06~88~~20260716120558~0.21~0.23~93.39~92.46~93.01/50872/472965240~50872~47297~0.56~14.85~~93.39~92.46~1.00~839.66~20168.01~1.42~102.08~83.52~1.03~-12~92.97~17.18~14.71~~~0.17~47296.5240~0.0000~0~   A~GP-A~-5.91~4.85~5.05~9.55~6.31~108.58~85.38~5.39~0.03~1.91~902767867~21683696323~-3.70~-11.90~902767867~~~-14.08~0.00~~CNY~0~___D__F__N~93.09~-170~";"#;

    let stocks = parse_tencent_quotes(raw, &HashMap::new(), false);
    assert_eq!(stocks.len(), 1);
    let stock = &stocks[0];
    assert_eq!(
        stock
            .get("circulating_market_cap_billion")
            .and_then(Value::as_f64),
        Some(839.66)
    );
    assert_eq!(
        stock.get("market_cap_billion").and_then(Value::as_f64),
        Some(20168.01)
    );
    assert_eq!(
        stock.get("circulating_shares").and_then(Value::as_f64),
        Some(902_767_867.0)
    );
    assert_eq!(
        stock.get("total_shares").and_then(Value::as_f64),
        Some(21_683_696_323.0)
    );
}

#[test]
fn observe_result_preserves_share_structure_from_quote_data() {
    let core_payload = json!({
        "data": {
            "stocks": [{
                "code": "600941.SH",
                "market_cap_billion": 20168.01,
                "circulating_market_cap_billion": 839.66,
                "total_shares": 21683696323.0,
                "circulating_shares": 902767867.0
            }]
        }
    });
    let mut result = json!({"stock": {"code": "600941.SH"}});

    enrich_observe_stock_quote_fields(&mut result, &core_payload);

    assert_eq!(
        result["stock"]["total_shares"].as_f64(),
        Some(21_683_696_323.0)
    );
    assert_eq!(
        result["stock"]["circulating_shares"].as_f64(),
        Some(902_767_867.0)
    );
}

#[test]
fn build_candidate_codes_is_stable_as_seed_grows() {
    // A full rebuild paginates with `batch_start`; the candidate list must
    // not drift as freshly fetched codes accumulate into the in-memory seed.
    let base = build_candidate_codes(&[], true, 100_000);
    assert_eq!(base.first().map(String::as_str), Some("000001.SZ"));

    // Seed codes already inside the scan ranges must not reorder the list.
    let seed: Vec<String> = vec![
        "600000.SH".to_string(),
        "000001.SZ".to_string(),
        "300500.SZ".to_string(),
    ];
    let with_seed = build_candidate_codes(&seed, true, 100_000);
    assert_eq!(
        base, with_seed,
        "pagination windows drifted when the seed grew"
    );

    // Seed-only codes (outside the scan ranges) are appended at the end so
    // the scan prefix — and therefore every batch window over it — is fixed.
    let extra_seed: Vec<String> = vec!["830001.BJ".to_string()];
    let with_extra = build_candidate_codes(&extra_seed, true, 100_000);
    assert_eq!(&with_extra[..base.len()], &base[..]);
    assert_eq!(with_extra.last().map(String::as_str), Some("830001.BJ"));
}

#[test]
fn build_candidate_codes_uses_seed_only_without_scan() {
    let seed: Vec<String> = vec!["600000.SH".to_string(), "000001.SZ".to_string()];
    let codes = build_candidate_codes(&seed, false, 100_000);
    assert_eq!(codes, seed);
}

#[test]
fn paginated_rebuild_covers_every_candidate_without_drift() {
    // Simulate the real full rebuild: the frontend walks `batch_start` 0..N
    // while the in-memory seed grows each invocation. The candidate list is
    // rebuilt from that growing seed every batch, so coverage is only correct
    // if the windows do not drift. (Worst case for drift: 100% hit rate, so
    // the seed grows as fast as possible.)
    use std::collections::BTreeSet;
    let max = 1_000usize;
    let stable = build_candidate_codes(&[], true, max);
    let total_batches = stable.len().div_ceil(TENCENT_BATCH_SIZE);
    let mut fetched: BTreeSet<String> = BTreeSet::new();
    for batch_start in 0..total_batches {
        let seed: Vec<String> = fetched.iter().cloned().collect();
        let candidates = build_candidate_codes(&seed, true, max);
        let (start, end, _) = candidate_batch_window(candidates.len(), batch_start, Some(1));
        for code in candidates
            .chunks(TENCENT_BATCH_SIZE)
            .skip(start)
            .take(end - start)
            .flatten()
        {
            fetched.insert(code.clone());
        }
    }
    let expected: BTreeSet<String> = stable.into_iter().collect();
    assert_eq!(fetched, expected, "rebuild missed or duplicated candidates");
}

#[test]
fn payload_usize_field_accepts_query_string_values() {
    let payload = json!({"limit": "3"});
    assert_eq!(payload_usize_field(&payload, "limit", 8, 1, 20), 3);

    let numeric_payload = json!({"limit": 25});
    assert_eq!(payload_usize_field(&numeric_payload, "limit", 8, 1, 20), 20);

    let invalid_payload = json!({"limit": "bad"});
    assert_eq!(payload_usize_field(&invalid_payload, "limit", 8, 1, 20), 8);
}

#[test]
fn market_data_payload_present_detects_any_cached_section() {
    assert!(market_data_payload_present(
        &json!({"stocks": [{"code": "000001.SZ"}]})
    ));
    assert!(market_data_payload_present(
        &json!({"relations": [{"source_code": "000001.SZ"}]})
    ));
    assert!(market_data_payload_present(
        &json!({"histories": {"000001.SZ": []}})
    ));
    assert!(!market_data_payload_present(&json!({})));
}

#[test]
fn market_cache_record_can_omit_data_payload() {
    let payload = json!({
        "generated_at_epoch_ms": 123,
        "notes": ["cached note"],
        "stocks": [{"code": "000001.SZ"}],
        "relations": [],
        "histories": {}
    });
    let summary = json!({"stock_count": 1, "warnings": []});
    let record = market_cache_record(
        Path::new("cache.json"),
        42,
        Some(456),
        summary,
        &payload,
        false,
        "cache written",
    );

    assert!(record.get("data").is_none());
    assert_eq!(record.get("stock_count").and_then(Value::as_u64), Some(1));
    assert_eq!(
        record.get("generated_at").and_then(Value::as_u64),
        Some(123)
    );
    assert_eq!(
        record
            .get("data_notes")
            .and_then(Value::as_array)
            .map(|items| items.len()),
        Some(1)
    );
}

#[test]
fn a_share_legacy_market_cache_promotes_to_external_store_with_factor_snapshot_projection_parity() {
    let root = std::env::temp_dir().join(format!(
        "gp-assistant-cn-market-store-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let market_dir = root.join("market");
    std::fs::create_dir_all(&market_dir).unwrap();
    let legacy_path = market_dir.join(MOBILE_MARKET_DATA_FILE);
    let fixture = json!({
        "generated_at_epoch_ms": 1792454400000_i64,
        "updated_at_epoch_ms": 1792454400000_i64,
        "stocks": [{
            "code": "600000.SH", "name": "浦发银行", "industry": "银行",
            "is_st": false, "price": 10.25, "pe": 6.2, "pb": 0.55,
            "roe": 0.08, "market_cap_billion": 300.0, "change_pct": 0.01,
            "volume": 12345.0, "amount": 126543.0, "quote_time": "2026-10-20 15:00:00"
        }],
        "relations": [{
            "source_code": "600000.SH", "target_code": "601398.SH",
            "relation_type": "sector", "weight": 0.8, "description": "银行业同业"
        }],
        "histories": {"600000.SH": [
            {"date": "2026-10-19", "open": 10.0, "high": 10.3, "low": 9.9, "close": 10.2, "volume": 1000.0},
            {"date": "2026-10-20", "open": 10.2, "high": 10.4, "low": 10.1, "close": 10.25, "volume": 1200.0}
        ]},
        "financials": {"600000.SH": {"latest_eps": 0.8, "latest_bps": 18.0, "period": "2026Q2", "source": "fixture"}},
        "factor_snapshots": {"600000.SH": [{"date": "2026-10-20", "available_date": "2026-10-21", "price": 10.25, "source": "fixture"}]},
        "capital_evidence": {},
    });
    let legacy_bytes = serde_json::to_vec(&fixture).unwrap();
    std::fs::write(&legacy_path, &legacy_bytes).unwrap();

    let mut store =
        crate::external_data::DataStore::open(crate::external_data::store_path(&root)).unwrap();
    assert_eq!(
        crate::market::store::legacy::import_legacy_market_domains(&mut store, &legacy_path)
            .unwrap()
            .len(),
        7
    );
    let projected = crate::market::project_market_store(&store)
        .unwrap()
        .expect("promoted market data should project into the legacy consumer shape");
    let original_core = parse_mobile_market_data_snapshot(&fixture, "fixture")
        .unwrap()
        .0;
    let projected_core = parse_mobile_market_data_snapshot(&projected, "external projection")
        .unwrap()
        .0;
    assert_eq!(
        serde_json::to_value(original_core).unwrap(),
        serde_json::to_value(projected_core).unwrap()
    );
    assert_eq!(std::fs::read(&legacy_path).unwrap(), legacy_bytes);
    assert_eq!(
        store
            .read_records(
                crate::external_data::DataDomain::Quote,
                crate::external_data::Market::China,
                "600000.SH",
            )
            .unwrap()
            .len(),
        1
    );

    for (domain, market) in crate::market::store::projection::external_market_domains() {
        store.clear_domain(domain, market).unwrap();
    }
    let promoted =
        crate::market::store::legacy::import_legacy_market_domains(&mut store, &legacy_path)
            .unwrap();
    let cleared_projection = crate::market::store::projection::project_market_store_with_legacy(
        &store,
        Some(&fixture),
        &promoted,
    )
    .unwrap()
    .unwrap();
    assert_eq!(cleared_projection["stocks"], json!([]));
    assert_eq!(cleared_projection["histories"], json!({}));
    assert_eq!(std::fs::read(&legacy_path).unwrap(), legacy_bytes);

    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_share_legacy_market_cache_promotes_to_external_store_with_empty_factor_projection_parity() {
    let root = std::env::temp_dir().join(format!(
        "gp-assistant-cn-market-store-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let market_dir = root.join("market");
    std::fs::create_dir_all(&market_dir).unwrap();
    let legacy_path = market_dir.join(MOBILE_MARKET_DATA_FILE);
    let fixture = json!({
        "generated_at_epoch_ms": 1792454400000_i64,
        "updated_at_epoch_ms": 1792454400000_i64,
        "stocks": [{
            "code": "600000.SH", "name": "浦发银行", "industry": "银行",
            "is_st": false, "price": 10.25, "pe": 6.2, "pb": 0.55,
            "roe": 0.08, "market_cap_billion": 300.0, "change_pct": 0.01,
            "volume": 12345.0, "amount": 126543.0, "quote_time": "2026-10-20 15:00:00"
        }],
        "relations": [{
            "source_code": "600000.SH", "target_code": "601398.SH",
            "relation_type": "sector", "weight": 0.8, "description": "银行业同业"
        }],
        "histories": {"600000.SH": [
            {"date": "2026-10-19", "open": 10.0, "high": 10.3, "low": 9.9, "close": 10.2, "volume": 1000.0},
            {"date": "2026-10-20", "open": 10.2, "high": 10.4, "low": 10.1, "close": 10.25, "volume": 1200.0}
        ]},
        "financials": {"600000.SH": {"latest_eps": 0.8, "latest_bps": 18.0, "period": "2026Q2", "source": "fixture"}},
        "factor_snapshots": {},
        "capital_evidence": {},
    });
    let legacy_bytes = serde_json::to_vec(&fixture).unwrap();
    std::fs::write(&legacy_path, &legacy_bytes).unwrap();

    let mut store =
        crate::external_data::DataStore::open(crate::external_data::store_path(&root)).unwrap();
    assert_eq!(
        crate::market::store::legacy::import_legacy_market_domains(&mut store, &legacy_path)
            .unwrap()
            .len(),
        7
    );
    let projected = crate::market::project_market_store(&store)
        .unwrap()
        .expect("promoted market data should project into the legacy consumer shape");
    let original_core = parse_mobile_market_data_snapshot(&fixture, "fixture")
        .unwrap()
        .0;
    let projected_core = parse_mobile_market_data_snapshot(&projected, "external projection")
        .unwrap()
        .0;
    assert_eq!(
        serde_json::to_value(original_core).unwrap(),
        serde_json::to_value(projected_core).unwrap()
    );
    assert_eq!(std::fs::read(&legacy_path).unwrap(), legacy_bytes);
    assert_eq!(
        store
            .read_records(
                crate::external_data::DataDomain::Quote,
                crate::external_data::Market::China,
                "600000.SH",
            )
            .unwrap()
            .len(),
        1
    );

    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_share_domain_migration_failure_keeps_legacy_projection_for_only_that_domain() {
    let root = std::env::temp_dir().join(format!(
        "gp-assistant-cn-market-partial-migration-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let market_dir = root.join("market");
    std::fs::create_dir_all(&market_dir).unwrap();
    let legacy_path = market_dir.join(MOBILE_MARKET_DATA_FILE);
    let fixture = json!({
        "generated_at_epoch_ms": 1792454400000_i64,
        "stocks": [{"code":"600000.SH","name":"浦发银行","industry":"银行","price":10.0,"quote_time":"2026-10-20 15:00:00"}],
        "relations": [],
        "histories": {"600000.SH":[{"date":"2026-10-20","close":10.0}]},
        "financials": {}, "factor_snapshots": {}, "capital_evidence": {}
    });
    let legacy_bytes = serde_json::to_vec(&fixture).unwrap();
    std::fs::write(&legacy_path, &legacy_bytes).unwrap();
    let database_path = crate::external_data::store_path(&root);
    let mut store = crate::external_data::DataStore::open(&database_path).unwrap();
    let connection = rusqlite::Connection::open(&database_path).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_quote_migration BEFORE INSERT ON records
         WHEN NEW.domain='quote'
         BEGIN SELECT RAISE(ABORT, 'fixture quote write rejected'); END;",
        )
        .unwrap();

    let promoted =
        crate::market::store::legacy::import_legacy_market_domains(&mut store, &legacy_path)
            .unwrap();
    assert!(promoted.contains(&(
        crate::external_data::DataDomain::DailyHistory,
        crate::external_data::Market::China
    )));
    assert!(!promoted.contains(&(
        crate::external_data::DataDomain::Quote,
        crate::external_data::Market::China
    )));
    let projection = crate::market::store::projection::project_market_store_with_legacy(
        &store,
        Some(&fixture),
        &promoted,
    )
    .unwrap()
    .unwrap();
    let legacy_core = parse_mobile_market_data_snapshot(&fixture, "legacy")
        .unwrap()
        .0;
    let mixed_core = parse_mobile_market_data_snapshot(&projection, "mixed")
        .unwrap()
        .0;
    assert_eq!(
        serde_json::to_value(legacy_core).unwrap(),
        serde_json::to_value(mixed_core).unwrap()
    );
    assert_eq!(std::fs::read(&legacy_path).unwrap(), legacy_bytes);

    drop(connection);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn market_cache_record_reports_same_day_quote_coverage() {
    let generated_at = cache_epoch_ms(Some(&json!("2026-06-26T10:00:00+08:00")))
        .expect("fixture timestamp should parse");
    let payload = json!({
        "generated_at_epoch_ms": generated_at,
        "stocks": [
            {"code": "000001.SZ", "quote_time": "20260626100000", "change_pct": 0.01},
            {"code": "000002.SZ", "quote_time": "2026-06-26 10:00:00", "change_pct": -0.02},
            {"code": "000003.SZ", "quote_time": "20260626100000", "change_pct": null},
            {"code": "000004.SZ", "quote_time": "20260625150000", "change_pct": 0.03}
        ],
        "relations": [],
        "histories": {}
    });
    let record = market_cache_record(
        Path::new("cache.json"),
        42,
        Some(generated_at),
        json!({"stock_count": 4, "warnings": []}),
        &payload,
        false,
        "cache written",
    );

    assert_eq!(
        record
            .get("quote_coverage_trade_date")
            .and_then(Value::as_str),
        Some("20260626")
    );
    assert_eq!(
        record.get("quote_requested").and_then(Value::as_u64),
        Some(4)
    );
    assert_eq!(
        record.get("quote_observed").and_then(Value::as_u64),
        Some(2)
    );
    assert_eq!(
        record.get("quote_coverage_ratio").and_then(Value::as_f64),
        Some(0.5)
    );
}

#[test]
fn retry_with_attempts_retries_transient_failures() {
    let mut attempts = 0usize;
    let result = retry_with_attempts(3, 0, |_| {
        attempts += 1;
        if attempts < 2 {
            Err("transient write failure".to_string())
        } else {
            Ok("ok")
        }
    })
    .expect("retry helper should recover after a transient failure");

    assert_eq!(result, "ok");
    assert_eq!(attempts, 2);
}

#[test]
fn cache_epoch_ms_accepts_iso_quote_dates() {
    let epoch = cache_epoch_ms(Some(&json!("2026-06-26T09:35:00+08:00")))
        .expect("ISO quote time should parse");
    assert_eq!(
        local_yyyymmdd_from_epoch_ms(epoch).as_deref(),
        Some("20260626")
    );
    let utc_epoch =
        cache_epoch_ms(Some(&json!("2026-06-26T01:35:00Z"))).expect("UTC quote time should parse");
    assert_eq!(
        local_yyyymmdd_from_epoch_ms(utc_epoch).as_deref(),
        Some("20260626")
    );
}

#[test]
fn expected_market_quote_date_uses_previous_session_before_open_and_weekends() {
    let friday_morning = cache_epoch_ms(Some(&json!("2026-06-26T08:00:00+08:00")))
        .expect("Friday morning should parse");
    assert_eq!(
        expected_market_quote_date_from_epoch_ms(friday_morning).as_deref(),
        Some("20260625")
    );
    let saturday =
        cache_epoch_ms(Some(&json!("2026-06-27T12:00:00+08:00"))).expect("Saturday should parse");
    assert_eq!(
        expected_market_quote_date_from_epoch_ms(saturday).as_deref(),
        Some("20260626")
    );
}

#[test]
fn market_quote_cache_stale_uses_quote_generation_date() {
    let june_25_quote = 1_782_381_094_692u128;
    let june_26_now = june_25_quote + 86_400_000;

    assert!(market_quote_cache_stale(
        Some(june_25_quote),
        june_26_now,
        Some(1.0)
    ));
    assert!(!market_quote_cache_stale(
        Some(june_26_now),
        june_26_now,
        Some(1.0)
    ));
    assert!(market_quote_cache_stale(None, june_26_now, Some(1.0)));
    assert_eq!(
        local_yyyymmdd_from_epoch_ms(june_25_quote).as_deref(),
        Some("20260625")
    );
}

#[test]
fn market_quote_cache_stale_rejects_partial_current_day_snapshot() {
    let current_quote = cache_epoch_ms(Some(&json!("2026-06-26T10:00:00+08:00")))
        .expect("fixture timestamp should parse");

    assert!(market_quote_cache_stale(
        Some(current_quote),
        current_quote,
        Some(gp_core::MIN_MARKET_BREADTH_COVERAGE - 0.001),
    ));
    assert!(!market_quote_cache_stale(
        Some(current_quote),
        current_quote,
        Some(gp_core::MIN_MARKET_BREADTH_COVERAGE),
    ));
}

#[test]
fn financial_snapshot_enriches_seed_without_preserving_snapshot_only_rows() {
    let seed = json!({
        "stocks": [
            {"code": "000001.SZ", "name": "Ping An Bank", "price": 10.0}
        ]
    });
    let snapshot = json!({
        "stocks": [
            {
                "code": "000001.SZ",
                "deducted_net_profit_billion": 1.2,
                "deducted_net_profit_growth_rate": 12.0,
                "latest_eps": 0.45,
                "latest_bps": 12.3,
                "period": "2026Q1",
                "source": "data/cache/tdx_fundamentals.csv",
                "notes": ["季度 EPS 明细来自缓存"],
                "quarterly_eps": [{"period": "2026Q1", "value": 0.45}]
            },
            {"code": "300001.SZ", "deducted_net_profit_billion": 0.8, "deducted_net_profit_growth_rate": 15.0, "latest_eps": 0.2}
        ]
    });
    let (seed_stocks, _) = seed_stock_maps(&seed);
    let enriched = enriched_stock_maps(&seed_stocks, &snapshot);

    assert_eq!(
        enriched
            .get("000001.SZ")
            .and_then(|stock| object_f64(stock, "deducted_net_profit_billion")),
        Some(1.2)
    );
    assert!(enriched.contains_key("300001.SZ"));

    let mut stocks = Vec::new();
    let mut seen = HashSet::new();
    let preserved =
        append_all_preserved_seed_stocks(&seed_stocks, &enriched, &mut stocks, &mut seen);

    assert_eq!(preserved, 1);
    assert_eq!(stocks.len(), 1);
    assert_eq!(
        stocks[0].get("code").and_then(Value::as_str),
        Some("000001.SZ")
    );
    assert_eq!(
        stocks[0]
            .get("deducted_net_profit_growth_rate")
            .and_then(Value::as_f64),
        Some(12.0)
    );
    assert_eq!(
        stocks[0].get("latest_eps").and_then(Value::as_f64),
        Some(0.45)
    );

    let valid_codes = HashSet::from(["000001.SZ".to_string()]);
    let financials = filtered_financial_snapshot_map(&seed, &snapshot, &valid_codes);
    let item = financials
        .get("000001.SZ")
        .and_then(Value::as_object)
        .expect("financial snapshot should be preserved for stock universe rows");
    assert_eq!(item.get("latest_eps").and_then(Value::as_f64), Some(0.45));
    assert_eq!(item.get("latest_bps").and_then(Value::as_f64), Some(12.3));
    assert_eq!(item.get("period").and_then(Value::as_str), Some("2026Q1"));
    assert_eq!(
        item.get("notes")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(Value::as_str),
        Some("季度 EPS 明细来自缓存")
    );
    assert_eq!(
        item.get("quarterly_eps")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("value"))
            .and_then(Value::as_f64),
        Some(0.45)
    );
    assert!(financials.get("300001.SZ").is_none());
}

#[test]
fn bundled_industry_snapshot_replaces_legacy_market_labels() {
    let seed = json!({
        "stocks": [
            {"code": "000001.SZ", "name": "平安银行", "industry": "深市A股", "price": 10.0}
        ]
    });
    let snapshot = json!({
        "industries": {"000001.SZ": "银行Ⅱ"}
    });
    let (seed_stocks, _) = seed_stock_maps(&seed);
    let enriched = enriched_stock_maps(&seed_stocks, &snapshot);

    assert_eq!(
        enriched
            .get("000001.SZ")
            .and_then(|stock| stock.get("industry"))
            .and_then(Value::as_str),
        Some("银行Ⅱ")
    );
}

#[test]
fn observe_financial_snapshot_merges_inferred_prior_year_eps() {
    let mut data = json!({
        "financials": {
            "000100.SZ": {
                "period": "2026Q1",
                "latest_eps": 0.0692,
                "quarterly_eps": [{"period": "2026Q1", "value": 0.0692, "source": "cache"}]
            }
        }
    });
    let snapshot = json!({
        "financials": {
            "000100.SZ": {
                "period": "2026Q1",
                "latest_eps": 0.0692,
                "quarterly_eps": [
                    {"period": "2026Q1", "value": 0.0692, "source": "snapshot"},
                    {"period": "2025Q1", "value": 0.0545, "source": "snapshot / inferred EPS YoY", "inferred": true, "note": "inferred_from_eps_yoy"}
                ]
            }
        }
    });

    assert!(merge_observe_financial_snapshot(
        &mut data,
        "000100.SZ",
        &snapshot
    ));
    assert_eq!(financial_quarterly_eps_count(&data, "000100.SZ"), 2);
    let rows = data
        .get("financials")
        .and_then(Value::as_object)
        .and_then(|financials| financials.get("000100.SZ"))
        .and_then(|entry| entry.get("quarterly_eps"))
        .and_then(Value::as_array)
        .expect("quarterly EPS rows should exist");
    let prior = rows
        .iter()
        .find(|row| row.get("period").and_then(Value::as_str) == Some("2025Q1"))
        .expect("inferred 2025Q1 EPS should be preserved");
    assert_eq!(prior.get("value").and_then(Value::as_f64), Some(0.0545));
    assert_eq!(prior.get("inferred").and_then(Value::as_bool), Some(true));
}

#[test]
fn observe_financial_snapshot_merges_detailed_fundamental_metrics() {
    let mut data = json!({"financials": {}});
    let snapshot = json!({
        "financials": {
            "000100.SZ": {
                "period": "2026Q1",
                "operating_revenue_billion": 434.778212,
                "operating_revenue_yoy": 8.370839,
                "parent_net_profit_billion": 15.564526,
                "parent_net_profit_yoy": 53.712048,
                "gross_margin": 12.497484,
                "net_margin": 1.399071,
                "roe": 2.47,
                "asset_liability_ratio": 65.027185,
                "goodwill_period": "2026一季报",
                "pledged_share_period": "2026-07-10",
                "dividend_period": "2025-12-31"
            }
        }
    });

    assert!(merge_observe_financial_snapshot(
        &mut data,
        "000100.SZ",
        &snapshot
    ));
    let entry = data["financials"]["000100.SZ"]
        .as_object()
        .expect("financial entry should be merged");
    assert_eq!(
        finite_object_number(entry, "operating_revenue_billion"),
        Some(434.778212)
    );
    assert_eq!(finite_object_number(entry, "gross_margin"), Some(12.497484));
    assert_eq!(finite_object_number(entry, "roe"), Some(2.47));
    assert_eq!(
        finite_object_number(entry, "asset_liability_ratio"),
        Some(65.027185)
    );
    assert_eq!(
        entry.get("goodwill_period").and_then(Value::as_str),
        Some("2026一季报")
    );
    assert_eq!(
        entry.get("pledged_share_period").and_then(Value::as_str),
        Some("2026-07-10")
    );
    assert_eq!(
        entry.get("dividend_period").and_then(Value::as_str),
        Some("2025-12-31")
    );
}

#[test]
fn observe_financial_snapshot_does_not_replace_a_newer_report_with_bundled_cache() {
    let mut data = json!({
        "financials": {
            "688372.SH": {
                "period": "2026中报",
                "latest_eps": 1.01,
                "operating_revenue_billion": 10.7185978642
            }
        }
    });
    let stale_snapshot = json!({
        "financials": {
            "688372.SH": {
                "period": "2026Q1",
                "latest_eps": 0.4754,
                "operating_revenue_billion": 4.8997562472
            }
        }
    });

    assert!(!merge_observe_financial_snapshot(
        &mut data,
        "688372.SH",
        &stale_snapshot
    ));
    assert_eq!(
        data["financials"]["688372.SH"]["period"].as_str(),
        Some("2026中报")
    );
    assert_eq!(
        data["financials"]["688372.SH"]["latest_eps"].as_f64(),
        Some(1.01)
    );
}

#[test]
fn eastmoney_fundamental_parsers_calculate_specialized_metrics() {
    let balance = json!({
        "result": {"data": [{
            "REPORT_DATE_NAME": "2026一季报",
            "GOODWILL": 11_436_177_181.0,
            "TOTAL_PARENT_EQUITY": 63_684_356_235.0
        }]}
    });
    let pledge = json!({
        "result": {"data": [{"TRADE_DATE": "2026-07-10 00:00:00", "PLEDGE_RATIO": 0.74}]}
    });
    let dividend = json!({
        "result": {"data": [{
            "REPORT_DATE": "2025-12-31 00:00:00",
            "PRETAX_BONUS_RMB": 0.9,
            "BASIC_EPS": 0.2333,
            "DIVIDENT_RATIO": 0.019271948608,
            "EX_DIVIDEND_DATE": "2026-06-11 00:00:00"
        }]}
    });

    let goodwill = parse_goodwill_to_net_assets(&balance).expect("goodwill ratio");
    assert!((goodwill - 17.9575925).abs() < 0.00001);
    assert_eq!(parse_latest_pledged_share_ratio(&pledge), Some(0.74));
    let (dividend_yield, payout_ratio) = parse_latest_dividend_metrics(&dividend, Some(4.95));
    assert!((dividend_yield.expect("dividend yield") - 1.8181818).abs() < 0.00001);
    assert!((payout_ratio.expect("payout ratio") - 38.5769396).abs() < 0.00001);
    assert_eq!(
        eastmoney_metric_period(
            eastmoney_result_rows(&balance)
                .first()
                .and_then(Value::as_object),
            &["REPORT_DATE_NAME", "REPORT_DATE"]
        ),
        Some("2026一季报".to_string())
    );
    assert_eq!(
        eastmoney_metric_period(
            eastmoney_result_rows(&pledge)
                .first()
                .and_then(Value::as_object),
            &["TRADE_DATE"]
        ),
        Some("2026-07-10".to_string())
    );
    assert_eq!(
        eastmoney_metric_period(latest_dividend_row(&dividend), &["REPORT_DATE"]),
        Some("2025-12-31".to_string())
    );
}

#[test]
fn eastmoney_empty_result_is_only_normalized_for_optional_datasets() {
    let empty = json!({
        "success": false,
        "code": 9201,
        "message": "返回数据为空",
        "result": null
    });

    let normalized = normalize_eastmoney_public_json(empty.clone(), "optional", true)
        .expect("optional empty response should be accepted");
    assert!(eastmoney_result_rows(&normalized).is_empty());
    assert_eq!(
        normalized.get("success").and_then(Value::as_bool),
        Some(true)
    );
    assert!(normalize_eastmoney_public_json(empty, "required", false).is_err());
}

#[test]
fn eastmoney_fetch_retries_direct_after_preferred_proxy_failure() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local Eastmoney fixture");
    let address = listener.local_addr().expect("local fixture address");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept direct retry");
        let mut buffer = [0_u8; 1024];
        let _ = stream.read(&mut buffer).expect("read fixture request");
        let body = r#"{"result":{"data":[{"PLEDGE_RATIO":0.69}]},"success":true,"code":0}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .expect("write fixture response");
    });
    let preferred_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .proxy(reqwest::Proxy::all("http://127.0.0.1:9").expect("bad proxy URL"))
        .build()
        .expect("preferred client");
    let direct_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .no_proxy()
        .build()
        .expect("direct client");

    let result = tauri::async_runtime::block_on(fetch_eastmoney_public_json_with_direct_retry(
        &preferred_client,
        &direct_client,
        &format!("http://{address}"),
        "Eastmoney fixture",
        false,
    ))
    .expect("direct retry must recover the public metric");
    assert_eq!(parse_latest_pledged_share_ratio(&result), Some(0.69));
    server.join().expect("local Eastmoney fixture");
}

#[test]
fn eastmoney_malformed_metric_rows_do_not_become_false_zeroes() {
    let malformed_pledge = json!({"result": {"data": [{}]}});
    let malformed_dividend = json!({"result": {"data": ["invalid"]}});

    assert_eq!(parse_latest_pledged_share_ratio(&malformed_pledge), None);
    assert_eq!(
        parse_latest_dividend_metrics(&malformed_dividend, Some(4.95)),
        (None, None)
    );
}

#[test]
fn observe_fundamental_supplement_records_freshness_and_refreshes_stale_values() {
    let mut data = json!({"financials": {"000100.SZ": {}}});
    let fields = json!({
        "goodwill_to_net_assets": 17.96,
        "goodwill_period": "2026一季报",
        "pledged_share_ratio": 0.74,
        "pledged_share_period": "2026-07-10",
        "dividend_yield": 1.82,
        "dividend_payout_ratio": 38.58,
        "dividend_period": "2025-12-31"
    })
    .as_object()
    .expect("supplement fields")
    .clone();

    assert!(observe_needs_fundamental_supplement_at(
        &data,
        "000100.SZ",
        "2026-10-12"
    ));
    assert!(merge_observe_fundamental_supplement(
        &mut data,
        "000100.SZ",
        fields
    ));
    assert!(!observe_needs_fundamental_supplement_at(
        &data,
        "000100.SZ",
        "2026-10-12"
    ));
    assert_eq!(
        data["financials"]["000100.SZ"]["goodwill_period"].as_str(),
        Some("2026一季报")
    );
    assert_eq!(
        data["financials"]["000100.SZ"]["pledged_share_period"].as_str(),
        Some("2026-07-10")
    );
    assert_eq!(
        data["financials"]["000100.SZ"]["dividend_period"].as_str(),
        Some("2025-12-31")
    );
    assert!(cache_epoch_ms(Some(
        &data["financials"]["000100.SZ"]["supplement_updated_at_epoch_ms"]
    ))
    .is_some());

    data["financials"]["000100.SZ"]["supplement_updated_at_epoch_ms"] = json!("1");
    assert!(observe_needs_fundamental_supplement_at(
        &data,
        "000100.SZ",
        "2026-10-12"
    ));
}

#[test]
fn eastmoney_latest_financial_snapshot_maps_the_latest_published_report() {
    let raw = json!({
        "result": { "data": [
            {
                "SECUCODE": "688372.SH", "REPORT_DATE": "2026-06-30 00:00:00",
                "REPORT_DATE_NAME": "2026中报", "NOTICE_DATE": "2026-08-29 00:00:00",
                "EPSJB": 1.01, "BPS": 25.152938005446,
                "TOTALOPERATEREVE": 1071859786.42, "TOTALOPERATEREVETZ": 68.995718380253,
                "PARENTNETPROFIT": 159311093.39, "PARENTNETPROFITTZ": 57.611429309995,
                "KCFJCXSYJLR": 145751927.16, "KCFJCXSYJLRTZ": 171.33152614654,
                "XSMLL": 33.590206029, "XSJLL": 14.8630534897,
                "ROEJQ": 4.41, "ZCFZL": 55.1854719758
            },
            {
                "SECUCODE": "600519.SH", "SECURITY_CODE": "600519",
                "REPORT_DATE": "2027-03-31 00:00:00", "REPORT_DATE_NAME": "2027一季报",
                "EPSJB": 99.0
            },
            {
                "SECUCODE": "688372.SH", "REPORT_DATE": "2026-03-31 00:00:00",
                "REPORT_DATE_NAME": "2026一季报", "EPSJB": 0.4754,
                "BPS": 24.926206301057, "TOTALOPERATEREVE": 489975624.72
            }
        ]}
    });

    let fields =
        parse_latest_observe_financial_snapshot(&raw, "688372.SH").expect("latest financial row");

    assert_eq!(
        fields.get("period").and_then(Value::as_str),
        Some("2026中报")
    );
    assert_eq!(fields.get("latest_eps").and_then(Value::as_f64), Some(1.01));
    assert_eq!(
        fields.get("latest_bps").and_then(Value::as_f64),
        Some(25.152938005446)
    );
    assert_eq!(
        fields
            .get("operating_revenue_billion")
            .and_then(Value::as_f64),
        Some(10.7185978642)
    );
    assert_eq!(
        fields
            .get("deducted_net_profit_billion")
            .and_then(Value::as_f64),
        Some(1.4575192716)
    );

    let mut cache = json!({
        "financials": { "688372.SH": { "period": "2026Q1", "latest_eps": 0.4754 } },
        "stocks": [{ "code": "688372.SH", "period": "2026Q1", "latest_eps": 0.4754 }]
    });
    assert!(merge_observe_fundamental_supplement(
        &mut cache,
        "688372.SH",
        fields
    ));
    assert_eq!(
        cache["financials"]["688372.SH"]["period"].as_str(),
        Some("2026中报")
    );
    assert_eq!(
        cache["financials"]["688372.SH"]["latest_eps"].as_f64(),
        Some(1.01)
    );
    assert_eq!(cache["stocks"][0]["period"].as_str(), Some("2026中报"));
    assert_eq!(
        cache["stocks"][0]["deducted_net_profit_billion"].as_f64(),
        Some(1.4575192716)
    );
}

#[test]
fn fundamental_snapshot_period_staleness_tracks_report_publication_cutoffs() {
    assert!(financial_report_period_is_stale("2026Q1", "2026-10-12"));
    assert!(!financial_report_period_is_stale("2026中报", "2026-10-12"));
    assert!(!financial_report_period_is_stale("2026Q1", "2026-07-15"));

    let fresh = crate::market_domain::dates::epoch_millis().to_string();
    let cached = json!({
        "financials": { "688372.SH": {
            "period": "2026Q1", "goodwill_to_net_assets": 0.0,
            "pledged_share_ratio": 0.0, "dividend_yield": 0.0,
            "dividend_payout_ratio": 0.0,
            "supplement_updated_at_epoch_ms": fresh
        }}
    });
    assert!(observe_needs_fundamental_supplement_at(
        &cached,
        "688372.SH",
        "2026-10-12"
    ));
}

#[test]
fn observe_quote_snapshot_fills_exact_share_structure() {
    let mut data = json!({
        "stocks": [{"code": "000100.SZ", "name": "TCL", "price": 4.95}]
    });
    assert!(observe_needs_exact_share_refresh(&data, "000100.SZ"));
    let quote = json!({
        "total_shares": 20_800_862_447.0,
        "circulating_shares": 20_118_326_408.0,
        "market_cap_billion": 1029.64,
        "circulating_market_cap_billion": 995.86,
        "quote_time": "20260716150000"
    });
    assert!(merge_observe_quote_snapshot(
        &mut data,
        "000100.SZ",
        quote.as_object().expect("quote object")
    ));
    assert!(!observe_needs_exact_share_refresh(&data, "000100.SZ"));
    assert_eq!(
        data["stocks"][0]["total_shares"].as_f64(),
        Some(20_800_862_447.0)
    );
    assert_eq!(
        data["stocks"][0]["circulating_shares"].as_f64(),
        Some(20_118_326_408.0)
    );
}

#[test]
fn eastmoney_guba_parser_extracts_hottest_sentiment_items() {
    let rows = (0..12)
            .map(|idx| {
                format!(
                    r#"{{"post_id":{},"post_title":"利好突破 看多上涨 {}","stockbar_code":"000725","post_click_count":{},"post_comment_count":{},"post_publish_time":"2026-06-25 12:{:02}:50","bullish_bearish":1}}"#,
                    100 + idx,
                    idx,
                    idx * 10,
                    idx,
                    idx
                )
            })
            .collect::<Vec<_>>()
            .join(",");
    let html = format!(r#"<script>var article_list={{"re":[{}]}};</script>"#, rows);
    let items = parse_eastmoney_guba_items(
        &html,
        "000725.SZ",
        "https://guba.eastmoney.com/list,000725.html",
    );

    assert_eq!(items.len(), 10);
    let first = items.first().unwrap();
    assert_eq!(
        first.get("category").and_then(Value::as_str),
        Some("community_sentiment")
    );
    assert_eq!(
        first.get("source").and_then(Value::as_str),
        Some("东方财富股吧")
    );
    assert!(first
        .get("title")
        .and_then(Value::as_str)
        .unwrap()
        .contains("11"));
    assert_eq!(
        first
            .get("metrics")
            .and_then(Value::as_object)
            .and_then(|metrics| metrics.get("评论数"))
            .and_then(Value::as_str),
        Some("11.00")
    );
    assert!(first.get("score").and_then(Value::as_f64).unwrap() > 60.0);
}

#[test]
fn eastmoney_lhb_parser_extracts_matching_stock() {
    let raw = r#"{"result":{"data":[{"SECURITY_CODE":"000725","SECURITY_NAME_ABBR":"京东方A","TRADE_DATE":"2026-06-17 00:00:00","BUY_AMT":147874970.48,"SELL_AMT":274892234.61,"NET_BUY_AMT":-127017264.13,"RATIO":-1.0467,"BUY_TIMES":0,"SELL_TIMES":1,"EXPLANATION":"日涨幅偏离值达到7%的前5只证券"}]}}"#;
    let item = parse_eastmoney_lhb_item(raw, "000725.SZ", "2026-06-01", "2026-06-25").unwrap();
    assert_eq!(
        item.get("category").and_then(Value::as_str),
        Some("institution_lhb")
    );
    assert_eq!(
        item.get("source").and_then(Value::as_str),
        Some("东方财富龙虎榜机构统计")
    );
    assert!(item.get("score").and_then(Value::as_f64).unwrap() < 50.0);
    let metrics = item.get("metrics").and_then(Value::as_object).unwrap();
    assert!(metrics
        .get("机构净买额")
        .and_then(Value::as_str)
        .unwrap()
        .contains("亿"));
    assert_eq!(
        metrics.get("机构买卖比").and_then(Value::as_str),
        Some("0.538")
    );
}

#[test]
fn eastmoney_lhb_uses_the_latest_report_date_not_the_largest_historical_amount() {
    let raw = r#"{"result":{"data":[
        {"SECURITY_CODE":"688372","TRADE_DATE":"2024-03-29 00:00:00","BUY_AMT":71992500,"SELL_AMT":105705000,"NET_BUY_AMT":-33712500},
        {"SECURITY_CODE":"688372","TRADE_DATE":"2026-10-09 00:00:00","BUY_AMT":3000000,"SELL_AMT":2000000,"NET_BUY_AMT":1000000}
    ]}}"#;

    let item = parse_eastmoney_lhb_item(raw, "688372.SH", "19900101", "20261012")
        .expect("latest public LHB row");

    assert_eq!(item.get("date").and_then(Value::as_str), Some("2026-10-09"));
    assert_eq!(item["metrics"]["机构净买额"].as_str(), Some("100.00 万"));
}

#[test]
fn eastmoney_market_active_brokerage_parser_selects_latest_day_and_sorts_by_activity() {
    let rows = json!({
        "result": { "count": 3, "data": [
            {"ONLIST_DATE":"2026-10-09","OPERATEDEPT_CODE":"A","OPERATEDEPT_NAME":"活跃席位A","BUYER_APPEAR_NUM":2,"SELLER_APPEAR_NUM":2,"TOTAL_BUYAMT":50000000.0,"TOTAL_SELLAMT":10000000.0,"TOTAL_NETAMT":40000000.0,"BUY_STOCK":"600001.SH 600002.SH","SECURITY_NAME_ABBR":"甲 乙"},
            {"ONLIST_DATE":"2026-10-09","OPERATEDEPT_CODE":"B","OPERATEDEPT_NAME":"活跃席位B","BUYER_APPEAR_NUM":4,"SELLER_APPEAR_NUM":3,"TOTAL_BUYAMT":80000000.0,"TOTAL_SELLAMT":30000000.0,"TOTAL_NETAMT":50000000.0,"BUY_STOCK":"600001.SH","SECURITY_NAME_ABBR":"甲"},
            {"ONLIST_DATE":"2026-10-08","OPERATEDEPT_CODE":"C","OPERATEDEPT_NAME":"旧日期席位","BUYER_APPEAR_NUM":20,"SELLER_APPEAR_NUM":20,"TOTAL_BUYAMT":100000000.0,"TOTAL_SELLAMT":100000000.0,"TOTAL_NETAMT":0.0}
        ]}
    });

    let board = parse_eastmoney_market_active_brokerage_board(&rows, "2026-10-12")
        .expect("daily active brokerages");

    assert_eq!(board["trade_date"].as_str(), Some("2026-10-09"));
    assert_eq!(board["total_departments"].as_u64(), Some(2));
    assert_eq!(board["items"][0]["name"].as_str(), Some("活跃席位B"));
    assert_eq!(board["items"][0]["total_appearances"].as_u64(), Some(7));
}

#[test]
fn eastmoney_institutional_holdings_keep_report_and_disclosure_dates() {
    let rows = json!({
        "result": { "data": [
            {"SECURITY_CODE":"688372","END_DATE":"2026-06-30","REPORT_DATE_NAME":"2026中报","NOTICE_DATE":"2026-08-29","HOLDER_RANK":3,"HOLDER_NAME":"某公募基金","HOLDER_TYPE":"证券投资基金","HOLD_NUM":4022078,"FREE_HOLDNUM_RATIO":2.3928,"HOLDER_STATEE":"加仓","CHANGE_RATIO":74.59},
            {"SECURITY_CODE":"688372","END_DATE":"2026-06-30","REPORT_DATE_NAME":"2026中报","NOTICE_DATE":"2026-08-29","HOLDER_RANK":4,"HOLDER_NAME":"某外资机构","HOLDER_TYPE":"QFII","HOLD_NUM":3500311,"FREE_HOLDNUM_RATIO":2.0824,"HOLDER_STATEE":"加仓","CHANGE_RATIO":129.8},
            {"SECURITY_CODE":"688372","END_DATE":"2026-06-30","REPORT_DATE_NAME":"2026中报","NOTICE_DATE":"2026-08-29","HOLDER_RANK":5,"HOLDER_NAME":"个人股东","HOLDER_TYPE":"个人","HOLD_NUM":1200000,"FREE_HOLDNUM_RATIO":0.72},
            {"SECURITY_CODE":"688372","END_DATE":"2025-12-31","REPORT_DATE_NAME":"2025年报","NOTICE_DATE":"2026-04-22","HOLDER_RANK":3,"HOLDER_NAME":"旧期基金","HOLDER_TYPE":"证券投资基金","HOLD_NUM":1000000,"FREE_HOLDNUM_RATIO":0.5}
        ]}
    });

    let holdings = parse_eastmoney_institutional_holdings(&rows, "688372.SH", "2026-10-12")
        .expect("institutional holdings snapshot");

    assert_eq!(holdings["report_period"].as_str(), Some("2026中报"));
    assert_eq!(holdings["report_date"].as_str(), Some("2026-06-30"));
    assert_eq!(holdings["disclosed_at"].as_str(), Some("2026-08-29"));
    assert_eq!(holdings["items"].as_array().unwrap().len(), 2);
    assert_eq!(holdings["items"][0]["name"].as_str(), Some("某公募基金"));
}

#[test]
fn capital_refresh_preserves_the_split_public_market_intelligence_fields() {
    let mut data = json!({"capital_evidence": {"688372.SH": {
        "market_active_brokerages": {"checked_on": "2026-10-12", "trade_date": "2026-10-12"},
        "institutional_holdings": {"checked_on": "2026-10-12", "report_period": "2026中报"}
    }}});
    assert!(merge_capital_evidence_items(
        &mut data,
        "688372.SH",
        vec![json!({"category":"fund_flow", "date":"2026-10-12"})],
        "20261012"
    ));
    assert_eq!(
        data["capital_evidence"]["688372.SH"]["market_active_brokerages"]["trade_date"].as_str(),
        Some("2026-10-12")
    );
    assert_eq!(
        data["capital_evidence"]["688372.SH"]["institutional_holdings"]["report_period"].as_str(),
        Some("2026中报")
    );

    assert!(store_observe_market_intelligence(
        &mut data,
        "688372.SH",
        json!({"status":"available", "checked_on":"2026-10-13", "trade_date":"2026-10-12"}),
        json!({"status":"available", "checked_on":"2026-10-13", "report_period":"2026中报"})
    ));
    assert_eq!(
        data["capital_evidence"]["688372.SH"]["market_active_brokerages"]["checked_on"].as_str(),
        Some("2026-10-13")
    );
    assert_eq!(
        data["capital_evidence"]["688372.SH"]["institutional_holdings"]["report_period"].as_str(),
        Some("2026中报")
    );
}

#[test]
fn institution_buy_sell_ratio_handles_zero_sell_amount() {
    assert_eq!(institution_buy_sell_ratio(100.0, 0.0), "∞");
    assert_eq!(institution_buy_sell_ratio(0.0, 0.0), "-");
    assert_eq!(institution_buy_sell_ratio(150.0, 100.0), "1.500");
}

#[test]
fn eastmoney_lhb_parser_returns_no_hit_status() {
    let raw =
        r#"{"result":{"data":[{"SECURITY_CODE":"600000","TRADE_DATE":"2026-06-17 00:00:00"}]}}"#;
    let item = parse_eastmoney_lhb_item(raw, "000725.SZ", "2026-06-01", "2026-06-25").unwrap();
    assert_eq!(
        item.get("category").and_then(Value::as_str),
        Some("institution_lhb_status")
    );
}

#[test]
fn eastmoney_lhb_seats_merge_buy_and_sell_without_inventing_one_sided_net() {
    let buy_raw = r#"{"result":{"data":[
        {"SECURITY_CODE":"600360","TRADE_DATE":"2026-07-14","OPERATEDEPT_CODE":"A","OPERATEDEPT_NAME":"机构专用","BUY":100000000.0,"TOTAL_BUYRIO":12.5,"CHANGE_RATE":9.91,"EXPLANATION":"日涨幅偏离值达到7%"},
        {"SECURITY_CODE":"600360","TRADE_DATE":"2026-07-14","OPERATEDEPT_CODE":"B","OPERATEDEPT_NAME":"某证券上海营业部","BUY":50000000.0,"CHANGE_RATE":9.91}
    ]}}"#;
    let sell_raw = r#"{"result":{"data":[
        {"SECURITY_CODE":"600360","TRADE_DATE":"2026-07-14","OPERATEDEPT_CODE":"A","OPERATEDEPT_NAME":"机构专用","SELL":30000000.0,"TOTAL_SELLRIO":4.2},
        {"SECURITY_CODE":"600360","TRADE_DATE":"2026-07-14","OPERATEDEPT_CODE":"C","OPERATEDEPT_NAME":"某证券深圳营业部","SELL":80000000.0,"CHANGE_RATE":9.91}
    ]}}"#;

    let buy = parse_eastmoney_lhb_seat_side(buy_raw, "600360", true).unwrap();
    let sell = parse_eastmoney_lhb_seat_side(sell_raw, "600360", false).unwrap();
    let seats = merge_eastmoney_lhb_seats(buy, sell);

    assert_eq!(seats.len(), 3);
    let institution = seats
        .iter()
        .find(|seat| seat.get("seat_code").and_then(Value::as_str) == Some("A"))
        .expect("institution seat");
    assert_eq!(
        institution.get("direction").and_then(Value::as_str),
        Some("both")
    );
    assert_eq!(
        institution.get("net_amount").and_then(Value::as_f64),
        Some(70_000_000.0)
    );

    let buy_only = seats
        .iter()
        .find(|seat| seat.get("seat_code").and_then(Value::as_str) == Some("B"))
        .expect("buy-only seat");
    assert_eq!(
        buy_only.get("direction").and_then(Value::as_str),
        Some("buy")
    );
    assert!(buy_only.get("net_amount").is_some_and(Value::is_null));
}

#[test]
fn eastmoney_main_fund_flow_parser_uses_latest_requested_trade_date() {
    let raw = r#"{"data":{"klines":["2026-07-15,762761376.0,-551448.0,-762209936.0,582441216.0,180320160.0,8.55,-0.01,-8.54,6.53,2.02,1251.06,2.98","2026-07-16,-79273120.0,-267954.0,79541072.0,81928864.0,-161201984.0,-1.32,-0.00,1.33,1.37,-2.69,1258.99,0.63","2026-07-17,-854126672.0,-610634.0,854737312.0,-71324912.0,-782801760.0,-11.66,-0.01,11.67,-0.97,-10.69,1253.00,-0.48"]}}"#;

    let item = parse_eastmoney_main_fund_flow_item(raw, "600519.SH", "2026-07-16").unwrap();

    assert_eq!(
        item.get("category").and_then(Value::as_str),
        Some("fund_flow")
    );
    assert_eq!(item.get("date").and_then(Value::as_str), Some("2026-07-16"));
    assert_eq!(
        item.get("sentiment").and_then(Value::as_str),
        Some("uncertain")
    );
    let metrics = item.get("metrics").and_then(Value::as_object).unwrap();
    assert_eq!(
        metrics.get("主力净占比").and_then(Value::as_str),
        Some("-1.32%")
    );
    assert_eq!(
        metrics.get("主力介入度").and_then(Value::as_str),
        Some("低（1.32%）")
    );
    assert_eq!(
        metrics.get("超大单净流入额").and_then(Value::as_str),
        Some("-1.61 亿")
    );
    assert_eq!(
        metrics.get("大单净流入额").and_then(Value::as_str),
        Some("8192.89 万")
    );
    assert_eq!(
        metrics.get("中小单净流入额").and_then(Value::as_str),
        Some("7927.31 万")
    );
    assert!(metrics
        .get("通俗结论")
        .and_then(Value::as_str)
        .unwrap()
        .contains("净卖出"));
}

#[test]
fn main_fund_flow_conclusion_explains_high_outflow_is_not_positive() {
    assert_eq!(main_fund_involvement(-11.66), "高");
    let conclusion = main_fund_flow_plain_conclusion(-11.66);
    assert!(conclusion.contains("每 100 元成交约有 11.66 元"));
    assert!(conclusion.contains("净卖出"));
    assert!(conclusion.contains("影响较大"));
}

#[test]
fn merging_real_main_fund_flow_preserves_local_proxy() {
    let code = "000001.SZ";
    let mut data = json!({
        "capital_evidence": {
            "000001.SZ": {
                "items": [
                    {
                        "category": "fund_flow",
                        "title": "本地量价资金代理",
                        "source": "Tauri/Rust 日线量价",
                        "metrics": {"证据类型": "本地日线量价代理"}
                    },
                    {
                        "category": "fund_flow",
                        "title": "旧主力资金流",
                        "date": "2026-07-16",
                        "metrics": {"主力净占比": "1.00%"}
                    }
                ]
            }
        }
    });
    let refreshed = json!({
        "category": "fund_flow",
        "title": "当日主力资金流",
        "date": "2026-07-17",
        "metrics": {"主力净占比": "-2.00%"}
    });

    assert!(merge_capital_evidence_items(
        &mut data,
        code,
        vec![refreshed],
        "2026-07-17",
    ));

    let items = data["capital_evidence"][code]["items"]
        .as_array()
        .expect("merged capital evidence items");
    let fund_flow_items = items
        .iter()
        .filter(|item| item.get("category").and_then(Value::as_str) == Some("fund_flow"))
        .collect::<Vec<_>>();
    assert_eq!(fund_flow_items.len(), 2);
    assert_eq!(
        fund_flow_items
            .iter()
            .filter(|item| is_local_fund_flow_proxy_value(item))
            .count(),
        1
    );
    let real_item = fund_flow_items
        .iter()
        .find(|item| !is_local_fund_flow_proxy_value(item))
        .expect("real fund flow item");
    assert_eq!(
        real_item.get("date").and_then(Value::as_str),
        Some("2026-07-17")
    );
}

#[test]
fn ths_quarterly_eps_parser_extracts_metric_value() {
    let raw = json!({
        "data": {
            "data": [
                {
                    "date": "2026-06-30",
                    "index_list": {
                        "基本每股收益": {"value": "0.88"},
                        "每股净资产": {"value": "5.2"}
                    }
                }
            ]
        }
    })
    .to_string();

    let rows = parse_ths_quarterly_eps_json(&raw).expect("ths parser");

    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].get("period").and_then(Value::as_str),
        Some("2026Q2")
    );
    assert_eq!(rows[0].get("value").and_then(Value::as_f64), Some(0.88));
}

#[test]
fn sina_quarterly_eps_parser_extracts_table_cells() {
    let html = r#"
            <table>
              <tr><th>指标</th><th>2026-03-31</th><th>2025-03-31</th></tr>
              <tr><td>基本每股收益</td><td>0.42</td><td>0.36</td></tr>
            </table>
        "#;

    let rows = parse_sina_quarterly_eps_html(html);

    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].get("period").and_then(Value::as_str),
        Some("2026Q1")
    );
    assert_eq!(
        rows[1].get("period").and_then(Value::as_str),
        Some("2025Q1")
    );
}

#[test]
fn basic_financial_merge_adds_tdx_latest_eps_as_quarter() {
    let mut data = json!({
        "stocks": [
            {
                "code": "000001.SZ",
                "latest_eps": 0.42,
                "latest_bps": 12.5,
                "period": "2026Q1"
            }
        ],
        "financials": {}
    });

    assert!(merge_basic_financial_from_stock(&mut data, "000001.SZ"));
    assert_eq!(financial_quarterly_eps_count(&data, "000001.SZ"), 1);
    let entry = data
        .get("financials")
        .and_then(Value::as_object)
        .and_then(|items| items.get("000001.SZ"))
        .and_then(Value::as_object)
        .expect("financial entry");
    assert_eq!(entry.get("latest_eps").and_then(Value::as_f64), Some(0.42));
    assert!(entry
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or("")
        .contains("通达信基础财务"));
}

#[test]
fn append_all_preserved_seed_stocks_keeps_rows_outside_current_batch() {
    let seed = json!({
        "stocks": [
            {"code": "000001.SZ", "name": "Ping An Bank", "price": 10.0},
            {"code": "600000.SH", "name": "SPD Bank", "price": 8.0},
            {"code": "688001.SH", "name": "STAR", "price": 20.0}
        ]
    });
    let (seed_stocks, _) = seed_stock_maps(&seed);
    let mut stocks = vec![json!({"code": "000001.SZ", "name": "Live Quote"})];
    let mut seen = HashSet::from(["000001.SZ".to_string()]);

    let preserved =
        append_all_preserved_seed_stocks(&seed_stocks, &seed_stocks, &mut stocks, &mut seen);

    assert_eq!(preserved, 2);
    assert_eq!(stocks.len(), 3);
    assert!(stocks
        .iter()
        .any(|stock| stock.get("code").and_then(Value::as_str) == Some("600000.SH")));
    assert!(stocks
        .iter()
        .any(|stock| stock.get("code").and_then(Value::as_str) == Some("688001.SH")));
}

#[test]
fn financial_snapshot_financials_object_enriches_screen_stock_rows() {
    let seed = json!({
        "stocks": [
            {"code": "000001.SZ", "name": "Ping An Bank", "price": 10.0}
        ]
    });
    let snapshot = json!({
        "financials": {
            "000001.SZ": {"latest_eps": 0.45, "latest_bps": 12.3}
        }
    });
    let (seed_stocks, seed_codes) = seed_stock_maps(&seed);
    let enriched = enriched_stock_maps(&seed_stocks, &snapshot);
    let mut stocks = Vec::new();
    let mut seen = HashSet::new();
    append_preserved_seed_stocks(&seed_codes, &enriched, &mut stocks, &mut seen);

    assert_eq!(stocks.len(), 1);
    assert_eq!(
        stocks[0].get("latest_eps").and_then(Value::as_f64),
        Some(0.45)
    );
    assert_eq!(
        stocks[0].get("latest_bps").and_then(Value::as_f64),
        Some(12.3)
    );
}

#[test]
fn core_payload_strips_financial_snapshot_after_merging_screen_rows() {
    let mut data = json!({
        "stocks": [
            {"code": "000001.SZ", "name": "Ping An Bank", "price": 10.0}
        ]
    });
    let snapshot = json!({
        "financials": {
            "000001.SZ": {"latest_eps": 0.45}
        }
    });
    merge_screen_financial_snapshot_into_data(&mut data, &snapshot);
    let stripped = strip_core_side_payload_fields(json!({
        "financial_snapshot": snapshot,
        "limit": 10
    }));

    assert_eq!(
        data["stocks"][0].get("latest_eps").and_then(Value::as_f64),
        Some(0.45)
    );
    assert!(stripped.get("financial_snapshot").is_none());
    assert_eq!(stripped.get("limit").and_then(Value::as_i64), Some(10));
}

#[test]
fn mobile_market_cache_reuses_matching_metadata_and_invalidates_on_size() {
    let path = PathBuf::from("test-mobile-market-cache.json");
    let data = json!({
        "stocks": [{"code": "000001.SZ", "name": "Ping An Bank", "industry": "Banking", "price": 10.0}],
        "relations": []
    });
    let (typed, summary) =
        parse_mobile_market_data_snapshot(&data, "test market data").expect("valid summary");

    forget_mobile_market_data_cache(&path);
    remember_mobile_market_data_cache(&path, 100, Some(1), &data, typed, &summary);

    let reused = cached_mobile_market_data_entry(&path, 100, Some(1)).expect("cache hit");
    assert_eq!(reused.bytes, 100);
    assert_eq!(reused.data["stocks"][0]["code"].as_str(), Some("000001.SZ"));
    assert!(cached_mobile_market_data_entry(&path, 101, Some(1)).is_none());
    assert!(cached_mobile_market_data_entry(&path, 100, Some(2)).is_none());

    forget_mobile_market_data_cache(&path);
    assert!(cached_mobile_market_data_entry(&path, 100, Some(1)).is_none());
}

#[test]
fn backtest_prefetch_matches_core_watchlist_top_n_slots() {
    let universe = ["000001.SZ", "600000.SH", "002432.SZ"]
        .into_iter()
        .map(|code| gp_core::StockItem {
            code: code.to_string(),
            ..gp_core::StockItem::default()
        })
        .collect::<Vec<_>>();
    let request = gp_core::BacktestRequest {
        criteria: gp_core::ScreenCriteria::default(),
        source: "watchlist".to_string(),
        strategy_mode: "candidate_snapshot".to_string(),
        stock_codes: vec![
            "600000".to_string(),
            "unknown".to_string(),
            "600000.SH".to_string(),
            "000001.SZ".to_string(),
            "002432.SZ".to_string(),
        ],
        start_date: "20200101".to_string(),
        end_date: "20260717".to_string(),
        top_n: 2,
        initial_cash: 1000.0,
        rebalance_frequency: "monthly".to_string(),
        transaction_cost_bps: 10.0,
        benchmark: "none".to_string(),
        ..gp_core::BacktestRequest::default()
    };

    assert_eq!(
        backtest_history_prefetch_codes(&universe, &request),
        vec!["600000.SH".to_string()]
    );
}

#[test]
fn backtest_prefetch_rejects_single_history_row() {
    assert!(!backtest_history_rows_are_usable(&[json!({
        "date": "2026-07-17",
        "close": 10.0
    })]));
    assert!(backtest_history_rows_are_usable(&[
        json!({ "date": "2026-07-16", "close": 9.8 }),
        json!({ "date": "2026-07-17", "close": 10.0 }),
    ]));
    assert!(!backtest_history_rows_are_usable(&[
        json!({ "date": "2026-07-17", "close": 9.8 }),
        json!({ "date": "2026-07-17", "close": 10.0 }),
    ]));
    assert!(!backtest_history_rows_are_usable(&[
        json!({ "date": "2026-07-16", "close": 0.0 }),
        json!({ "date": "2026-07-17", "close": 10.0 }),
    ]));
}

#[test]
fn market_data_patch_updates_only_requested_stock() {
    let source = json!({
        "stocks": [
            {"code": "000001.SZ", "name": "A", "price": 10.5},
            {"code": "600000.SH", "name": "B", "price": 8.0}
        ],
        "histories": {
            "000001.SZ": [{"date": "2026-07-09", "close": 10.5}],
            "600000.SH": [{"date": "2026-07-09", "close": 8.0}]
        },
        "financials": {
            "000001.SZ": {"latest_eps": 1.2},
            "600000.SH": {"latest_eps": 0.8}
        }
    });
    let patch = market_data_patch_for_codes(&source, &["000001.SZ".to_string()]);
    assert!(patch["histories"].get("000001.SZ").is_some());
    assert!(patch["histories"].get("600000.SH").is_none());

    let mut target = json!({
        "stocks": [
            {"code": "000001.SZ", "name": "A", "price": 9.5},
            {"code": "600000.SH", "name": "B", "price": 8.0}
        ],
        "histories": {
            "600000.SH": [{"date": "2026-07-09", "close": 8.0}]
        },
        "financials": {
            "600000.SH": {"latest_eps": 0.8}
        }
    });
    apply_market_data_patch(&mut target, &patch);

    assert_eq!(target["stocks"][0]["price"].as_f64(), Some(10.5));
    assert_eq!(target["stocks"][1]["price"].as_f64(), Some(8.0));
    assert_eq!(
        target["financials"]["000001.SZ"]["latest_eps"].as_f64(),
        Some(1.2)
    );
    assert_eq!(
        target["financials"]["600000.SH"]["latest_eps"].as_f64(),
        Some(0.8)
    );
}

#[test]
#[ignore = "requires live Tencent daily history"]
fn real_tencent_daily_history_smoke_returns_rows() {
    let rows = tauri::async_runtime::block_on(fetch_observe_daily_history(
        "600989.SH",
        "20260106",
        "20501231",
    ))
    .unwrap_or_else(|error| panic!("real Tencent daily history smoke failed: {error}"));
    println!(
        "real Tencent daily history smoke: rows={}, first={:?}, last={:?}",
        rows.len(),
        rows.first().and_then(|row| row.get("date")).cloned(),
        rows.last().and_then(|row| row.get("date")).cloned()
    );
    assert!(
        rows.len() >= MIN_ADAPTIVE_SCREEN_HISTORY_BARS,
        "real Tencent daily history should provide enough rows for adaptive screening"
    );
}
