use super::*;

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
fn candidate_batch_window_clamps_to_available_batches() {
    assert_eq!(candidate_batch_window(0, 0, Some(4)), (0, 0, 0));
    assert_eq!(candidate_batch_window(600, 0, Some(2)), (0, 2, 5));
    assert_eq!(candidate_batch_window(600, 4, Some(4)), (4, 5, 5));
    assert_eq!(candidate_batch_window(600, 9, Some(1)), (5, 5, 5));
}

#[test]
fn tencent_batch_size_keeps_full_refresh_round_trips_reasonable() {
    assert_eq!(15_000usize.div_ceil(TENCENT_BATCH_SIZE), 125);
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

    assert!(market_quote_cache_stale(Some(june_25_quote), june_26_now));
    assert!(!market_quote_cache_stale(Some(june_26_now), june_26_now));
    assert!(market_quote_cache_stale(None, june_26_now));
    assert_eq!(
        local_yyyymmdd_from_epoch_ms(june_25_quote).as_deref(),
        Some("20260625")
    );
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
    assert_eq!(stocks[0].get("latest_eps").and_then(Value::as_f64), Some(0.45));

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
