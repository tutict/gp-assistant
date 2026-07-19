use super::*;

#[test]
fn llm_models_endpoint_appends_models_to_provider_base_path() {
    assert_eq!(
        llm_models_endpoint("https://api.openai.com/v1/")
            .unwrap()
            .as_str(),
        "https://api.openai.com/v1/models"
    );
    assert_eq!(
        llm_models_endpoint("https://gateway.example/v1/chat/completions?trace=1")
            .unwrap()
            .as_str(),
        "https://gateway.example/v1/models"
    );
    assert_eq!(
        llm_models_endpoint("http://127.0.0.1:11434/v1/models")
            .unwrap()
            .as_str(),
        "http://127.0.0.1:11434/v1/models"
    );
    assert!(llm_models_endpoint("file:///tmp/models").is_err());
}

#[test]
fn parse_llm_model_options_supports_openai_and_ollama_shapes() {
    let openai = parse_llm_model_options(&json!({
        "data": [
            {"id": "gpt-5.4", "owned_by": "openai"},
            {"id": "gpt-5.4", "owned_by": "duplicate"},
            {"id": "gpt-5.4-mini", "display_name": "GPT 5.4 Mini"}
        ]
    }));
    assert_eq!(openai.len(), 2);
    assert_eq!(openai[0]["id"].as_str(), Some("gpt-5.4"));
    assert_eq!(openai[1]["name"].as_str(), Some("GPT 5.4 Mini"));

    let ollama = parse_llm_model_options(&json!({
        "models": [
            {"name": "qwen2.5:7b", "model": "qwen2.5:7b"},
            "deepseek-r1:8b"
        ]
    }));
    assert_eq!(ollama.len(), 2);
    assert_eq!(ollama[0]["id"].as_str(), Some("qwen2.5:7b"));
    assert_eq!(ollama[1]["id"].as_str(), Some("deepseek-r1:8b"));
}

#[test]
fn llm_models_http_error_explains_auth_and_version_failures() {
    assert!(llm_models_http_error(401, b"{}", "sk-test").contains("API 密钥"));
    assert!(llm_models_http_error(404, b"{}", "sk-test").contains("/v1"));
    assert!(
        llm_models_http_error(429, br#"{"error":{"message":"rate limited"}}"#, "sk-test")
            .contains("rate limited")
    );
    let redacted = llm_models_http_error(
        500,
        br#"{"error":{"message":"upstream echoed sk-secret-token"}}"#,
        "sk-secret-token",
    );
    assert!(!redacted.contains("sk-secret-token"));
    assert!(redacted.contains("[已隐藏密钥]"));
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

    assert!(observe_needs_fundamental_supplement(&data, "000100.SZ"));
    assert!(merge_observe_fundamental_supplement(
        &mut data,
        "000100.SZ",
        fields
    ));
    assert!(!observe_needs_fundamental_supplement(&data, "000100.SZ"));
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
    assert!(observe_needs_fundamental_supplement(&data, "000100.SZ"));
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
fn trend_history_prefetch_codes_normalizes_and_deduplicates_candidates() {
    let result = json!({
        "items": [
            {"stock": {"code": "000001"}},
            {"stock": {"code": "000001.SZ"}},
            {"stock": {"code": "600000.SH"}},
            {"stock": {"code": "invalid"}}
        ]
    });

    assert_eq!(
        trend_history_prefetch_codes(&result, 10),
        vec!["000001.SZ".to_string(), "600000.SH".to_string()]
    );
}

#[test]
fn trend_history_cache_requires_enough_bars_inside_requested_window() {
    let data = json!({
        "histories": {
            "000001.SZ": [
                {"date": "2026-03-04", "close": 9.8},
                {"date": "2026-03-05", "close": 9.9},
                {"date": "2026-03-06", "close": 10.0}
            ]
        }
    });

    assert!(history_cache_has_bars(
        &data,
        "000001.SZ",
        "20260305",
        "20260306",
        2
    ));
    assert!(!history_cache_has_bars(
        &data,
        "000001.SZ",
        "20260305",
        "20260306",
        3
    ));
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
