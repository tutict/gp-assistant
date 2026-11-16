#[test]
fn quarterly_pdf_preserves_real_kerning_and_positioned_table_cells() {
    use lopdf::{
        content::{Content, Operation},
        dictionary, Document, Object,
    };
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {"Type"=>"Font", "Subtype"=>"Type1", "BaseFont"=>"Helvetica", "Encoding"=>"WinAnsiEncoding"});
    let resources_id = document.add_object(dictionary! {"Font"=>dictionary! {"F1"=>font_id}});
    let position = |x: f32, y: f32| {
        Operation::new(
            "Tm",
            vec![1.into(), 0.into(), 0.into(), 1.into(), x.into(), y.into()],
        )
    };
    let mut operations = vec![
        Operation::new("BT", vec![]),
        Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), 10.into()]),
        position(40.0, 740.0),
    ];
    let mut title = Vec::new();
    for word in [
        "FINANCIAL",
        "UPDATE",
        "FOR",
        "THE",
        "PERIOD",
        "ENDED",
        "30",
        "SEPTEMBER",
        "2026",
    ] {
        title.push(Object::string_literal(word));
        title.push(Object::Real(-180.5));
    }
    operations.push(Operation::new("TJ", vec![Object::Array(title)]));
    operations.push(position(40.0, 720.0));
    operations.push(Operation::new(
        "Tj",
        vec![Object::string_literal("Unaudited (RMB Million)")],
    ));
    for (y, label, values) in [
        (
            680.0,
            "Revenue",
            [
                "3,946.2", "3,352.9", "17.7%", "4,196.8", "-6.0%", "11,155.1", "10,210.1", "9.3%",
            ],
        ),
        (
            660.0,
            "Profit for the Period",
            [
                "365.1", "309.4", "18.0%", "316.9", "15.2%", "937.0", "809.1", "15.8%",
            ],
        ),
        (
            640.0,
            "Total comprehensive income",
            [
                "294.3", "142.1", "107.1%", "284.2", "3.6%", "564.3", "785.4", "-28.2%",
            ],
        ),
    ] {
        operations.push(position(40.0, y));
        operations.push(Operation::new("Tj", vec![Object::string_literal(label)]));
        for (column, value) in values.into_iter().enumerate() {
            operations.push(position(220.0 + column as f32 * 45.0, y));
            operations.push(Operation::new("Tj", vec![Object::string_literal(value)]));
        }
    }
    operations.push(Operation::new("ET", vec![]));
    let content_id = document.add_object(lopdf::Stream::new(
        dictionary! {},
        Content { operations }.encode().unwrap(),
    ));
    let page_id = document.add_object(dictionary! {"Type"=>"Page", "Parent"=>pages_id, "Resources"=>resources_id, "Contents"=>content_id, "MediaBox"=>vec![0.into(),0.into(),800.into(),800.into()]});
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {"Type"=>"Pages", "Kids"=>vec![page_id.into()], "Count"=>1}),
    );
    let catalog_id = document.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>pages_id});
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).unwrap();
    let text = super::extract_hkex_pdf_text(&bytes).unwrap();
    let update = super::parse_hkex_quarterly_update_text(
        &text,
        "06969.HK",
        "Financial Update for the Period Ended 30 September 2026",
        "2026-10-12",
        "https://www1.hkexnews.hk/update.pdf",
        "fetched",
    )
    .unwrap();
    assert_eq!(update.period, "2026Q3");
    assert_eq!(update.quarterly_revenue, Some(3946.2));
    assert_eq!(update.nine_month_revenue, Some(11155.1));
    assert_eq!(update.quarterly_net_profit, Some(365.1));
    assert_eq!(update.nine_month_net_profit, Some(937.0));
    assert_eq!(update.nine_month_total_comprehensive_income, Some(564.3));
}

#[test]
fn quarterly_rows_keep_a_separated_minus_and_small_zero_amounts() {
    let text = "Period ended 30 September 2026 (RMB Million)\nRevenue 3,946.2 3,352.9 17.7% 4,196.8 – 6.0% 11,155.1 10,210.1 9.3%\nProfit for the Period\n0.0\n0.0\n0.0%\n0.0\n0.0%\n0.0\n0.0\n0.0%";
    let update = super::parse_hkex_quarterly_update_text(
        text,
        "06969.HK",
        "Financial Update for the Period Ended 30 September 2026",
        "2026-10-12",
        "https://www1.hkexnews.hk/update.pdf",
        "fetched",
    )
    .unwrap();
    assert_eq!(update.quarterly_revenue_yoy_percent, Some(-6.0));
    assert_eq!(update.quarterly_net_profit, Some(0.0));
    assert_eq!(update.nine_month_net_profit, Some(0.0));
}

#[test]
fn quarterly_missing_revenue_does_not_borrow_profit_values() {
    let text="Period ended 30 September 2026 (RMB Million)\nRevenue —\nProfit for the Period 365.1 309.4 18.0% 316.9 15.2% 937.0 809.1 15.8%";
    let update = super::parse_hkex_quarterly_update_text(
        text,
        "06969.HK",
        "Financial Update for the Period Ended 30 September 2026",
        "2026-10-12",
        "https://www1.hkexnews.hk/update.pdf",
        "fetched",
    )
    .unwrap();
    assert_eq!(update.quarterly_revenue, None);
    assert_eq!(update.nine_month_revenue, None);
    assert_eq!(update.quarterly_net_profit, Some(365.1));
}

#[test]
fn quarterly_negative_sign_survives_a_pdf_line_break() {
    let text="Period ended 30 September 2026 (RMB Million)\nRevenue\n3,946.2\n3,352.9\n17.7%\n4,196.8\n–\n6.0%\n11,155.1\n10,210.1\n9.3%";
    let update = super::parse_hkex_quarterly_update_text(
        text,
        "06969.HK",
        "Financial Update for the Period Ended 30 September 2026",
        "2026-10-12",
        "https://www1.hkexnews.hk/update.pdf",
        "fetched",
    )
    .unwrap();
    assert_eq!(update.quarterly_revenue_yoy_percent, Some(-6.0));
    assert_eq!(update.nine_month_revenue, Some(11155.1));
}

#[test]
fn a_quarter_heading_without_financial_values_is_not_a_usable_update() {
    assert!(super::parse_hkex_quarterly_update_text(
        "Period ended 30 September 2026 (RMB Million)",
        "06969.HK",
        "Financial Update for the Period Ended 30 September 2026",
        "2026-10-12",
        "https://www1.hkexnews.hk/update.pdf",
        "fetched"
    )
    .is_err());
}

#[test]
#[ignore]
fn live_quarter_pdf_probe() {
    let path = std::env::var("GP_HK_QUARTER_PDF").expect("provide the downloaded PDF path");
    let text = super::extract_hkex_pdf_text(&std::fs::read(path).unwrap()).unwrap();
    let update = super::parse_hkex_quarterly_update_text(
        &text,
        "06969.HK",
        "Financial Update for the Period Ended 30 September 2026",
        "2026-10-12",
        "https://www1.hkexnews.hk/listedco/listconews/sehk/2026/1012/2026101201051.pdf",
        "live-probe",
    )
    .unwrap();
    assert_eq!(update.period, "2026Q3");
    assert_eq!(update.quarterly_revenue, Some(3946.2));
    assert_eq!(update.nine_month_revenue, Some(11155.1));
    assert_eq!(update.quarterly_revenue_yoy_percent, Some(-6.0));
    assert_eq!(update.nine_month_net_profit, Some(937.0));
    if let Ok(output) = std::env::var("GP_HK_QUARTER_OUTPUT") {
        std::fs::write(output, serde_json::to_vec_pretty(&update).unwrap()).unwrap();
    }
}
use super::{parse_hkex_security_rows, parse_sina_hk_quote, parse_tencent_hk_quote};
use crate::market_domain::CurrencyCode;

#[test]
fn hkex_quarter_search_url_uses_issuer_scope_and_explicit_date_window() {
    let disclosure = super::HkDisclosureMetadata {
        source_url: Some(
            "https://www1.hkexnews.hk/search/titlesearch.xhtml?lang=EN&market=SEHK&stockId=7609"
                .to_string(),
        ),
        source_name: "HKEXnews".to_string(),
        report_period: None,
        disclosed_at: None,
        checked_at: "now".to_string(),
        status: "metadata_only".to_string(),
        error: None,
    };

    let url = reqwest::Url::parse(
        &super::hkex_quarterly_update_search_url(&disclosure, "20251102", "20261107").unwrap(),
    )
    .unwrap();
    let query = url
        .query_pairs()
        .collect::<std::collections::HashMap<_, _>>();

    assert_eq!(url.path(), "/search/titleSearchServlet.do");
    assert_eq!(query.get("lang").map(|value| value.as_ref()), Some("E"));
    assert_eq!(
        query.get("searchType").map(|value| value.as_ref()),
        Some("0")
    );
    assert_eq!(
        query.get("stockId").map(|value| value.as_ref()),
        Some("7609")
    );
    assert_eq!(
        query.get("fromDate").map(|value| value.as_ref()),
        Some("20251102")
    );
    assert_eq!(
        query.get("toDate").map(|value| value.as_ref()),
        Some("20261107")
    );
    assert_eq!(
        query.get("market").map(|value| value.as_ref()),
        Some("SEHK")
    );
}

#[test]
fn hk_quarter_financial_row_parses_negative_and_accounting_percentages() {
    assert_eq!(
        super::parse_hk_update_numbers("Revenue 3,946.2 (6.0%)"),
        vec![3946.2, -6.0]
    );
}

#[test]
fn hk_proxy_probe_keeps_each_provider_outcome_separate_and_bounded() {
    let (available, history, unavailable) = tauri::async_runtime::block_on(async {
        tokio::join!(
            super::probe_hk_source("tencent", std::time::Duration::from_secs(1), async {
                Ok(())
            }),
            super::probe_hk_source(
                "tencent_history",
                std::time::Duration::from_secs(1),
                async {
                    super::parse_tencent_hk_history(
                        include_str!("../../../../testdata/tencent-hk-kline.json"),
                        "00700.HK",
                    )?;
                    Ok(())
                }
            ),
            super::probe_hk_source(
                "sina_hk_financial",
                std::time::Duration::from_secs(1),
                async { Err("fixture timeout".to_string()) }
            ),
        )
    });

    assert_eq!(available["source_id"], "tencent");
    assert_eq!(available["ok"], true);
    assert_eq!(history["source_id"], "tencent_history");
    assert_eq!(history["ok"], true);
    assert_eq!(unavailable["source_id"], "sina_hk_financial");
    assert_eq!(unavailable["ok"], false);
    assert_eq!(unavailable["error"], "fixture timeout");
    assert!(unavailable["elapsed_ms"].as_u64().is_some());
}

#[test]
fn tencent_hk_history_probe_uses_the_same_daily_query_as_observation_refresh() {
    let url = super::tencent_hk_daily_history_url("01171.HK").unwrap();

    assert!(url.ends_with("&param=hk01171%2Cday%2C%2C%2C2000%2Cqfq"));
    assert!(url.contains("/appstock/app/hkfqkline/get?_var=kline_dayqfq"));
    assert!(super::tencent_hk_daily_history_url("00000.SH").is_err());
}

#[test]
fn tencent_hk_history_parser_accepts_the_hk_endpoint_jsonp_response_for_06929() {
    let response = r#"kline_dayqfq={"code":0,"data":{"hk06929":{"qfqday":[["2023-09-11","10.0","10.5","10.8","9.8","1200000"]]}}};"#;

    let bars = super::parse_tencent_hk_history(response, "06929.HK").unwrap();

    assert_eq!(bars.len(), 1);
    assert_eq!(bars[0].date, "2023-09-11");
    assert_eq!(bars[0].open, 10.0);
    assert_eq!(bars[0].close, 10.5);
}

#[test]
fn tencent_hk_history_parser_accepts_positive_backward_adjusted_dividend_history() {
    let response = r#"kline_dayhfq={"code":0,"data":{"hk01171":{"hfqday":[["2019-01-22","18.570","18.202","18.570","18.106","16420000.000"],["2021-08-25","29.627","30.203","30.267","29.563","16575300.000"]]}}};"#;
    let bars = super::parse_tencent_hk_history(response, "01171.HK").unwrap();
    assert_eq!(bars.len(), 2);
    assert_eq!(bars[0].close, 18.202);
    assert_eq!(bars[1].close, 30.203);
}

#[test]
fn tencent_hk_backward_history_rejects_invalid_rows_and_a_different_adjustment_type() {
    let invalid = r#"{"code":0,"data":{"hk01171":{"hfqday":[["2026-11-05","10","10","11","9","100"],["2026-11-06","-1","-1","-1","-2","100"]]}}}"#;
    assert!(super::parse_tencent_hk_price_page(
        invalid,
        "01171",
        super::HkHistoryPriceAdjustment::Backward
    )
    .unwrap_err()
    .to_string()
    .contains("未补齐"));
    let forward =
        r#"{"code":0,"data":{"hk01171":{"qfqday":[["2026-11-05","10","10","11","9","100"]]}}}"#;
    assert!(super::parse_tencent_hk_price_page(
        forward,
        "01171",
        super::HkHistoryPriceAdjustment::Backward
    )
    .unwrap_err()
    .to_string()
    .contains("hfq"));
}

#[test]
fn tencent_hk_adjusted_history_accepts_unadjusted_rows_returned_for_a_new_listing() {
    let response = r#"{"code":0,"data":{"hk09971":{"day":[["2026-07-08","34.120","33.220","37.380","33.000","11270830.00"]]}}}"#;
    let bars = super::parse_tencent_hk_price_page(
        response,
        "09971",
        super::HkHistoryPriceAdjustment::Backward,
    )
    .unwrap();
    assert_eq!(bars.bars.len(), 1);
    assert_eq!(bars.bars[0].close, 33.220);
}

#[test]
fn tencent_hk_history_preserves_the_suspended_quote_date_separately_from_the_last_trade() {
    let mut quote = vec![serde_json::json!(""); 41];
    quote[30] = serde_json::json!("2026/11/06 16:08:54");
    quote[40] = serde_json::json!("S");
    let text = serde_json::json!({"code":0,"data":{"hk02555":{"qfqday":[["2026-10-12","3.875","3.860","3.900","3.845","280000.000"]],"qt":{"hk02555":quote}}}}).to_string();
    let page = super::parse_tencent_hk_price_page(
        &text,
        "02555",
        super::HkHistoryPriceAdjustment::Forward,
    )
    .unwrap();
    assert_eq!(page.bars.last().unwrap().date, "2026-10-12");
    assert_eq!(page.suspended_as_of.as_deref(), Some("20261106"));
}

#[tokio::test]
async fn hk_history_switches_the_whole_range_when_the_preferred_adjustment_has_invalid_prices() {
    use super::HkHistoryPriceAdjustment as Adjustment;
    let attempts = std::sync::Mutex::new(Vec::new());
    let fetched = super::fetch_valid_hk_adjustment_range(Adjustment::Forward, |adjustment| {
        attempts.lock().unwrap().push(adjustment);
        async move {
            if adjustment == Adjustment::Forward {
                Err(super::HkHistoryFetchError::InvalidPrices(
                    "nonpositive early dividend price".to_owned(),
                ))
            } else {
                Ok(vec![super::HkDailyBar {
                    date: "2020-01-02".to_owned(),
                    open: 20.0,
                    close: 20.0,
                    high: 21.0,
                    low: 19.0,
                    volume_shares: Some(100),
                }]
                .into())
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(
        *attempts.lock().unwrap(),
        vec![Adjustment::Forward, Adjustment::Backward]
    );
    assert_eq!(fetched.price_adjustment, Adjustment::Backward);
    assert_eq!(fetched.bars[0].close, 20.0);
}

#[tokio::test]
async fn hk_history_reports_network_errors_without_trying_a_different_price_basis() {
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let error =
        super::fetch_valid_hk_adjustment_range(super::HkHistoryPriceAdjustment::Forward, |_| {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async {
                Err(super::HkHistoryFetchError::Provider(
                    "provider timeout".to_owned(),
                ))
            }
        })
        .await
        .unwrap_err();
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(error.contains("provider timeout"));
}

#[tokio::test]
async fn tencent_hk_history_pagination_never_claims_coverage_after_its_page_limit() {
    let start = chrono::NaiveDate::from_ymd_opt(1900, 1, 1).unwrap();
    let end = chrono::NaiveDate::from_ymd_opt(2026, 11, 7).unwrap();
    let result =
        super::collect_tencent_hk_history_range("01171", start, end, |cursor| async move {
            Ok(vec![super::HkDailyBar {
                date: (cursor - chrono::Duration::days(1))
                    .format("%Y-%m-%d")
                    .to_string(),
                open: 10.0,
                close: 10.0,
                high: 11.0,
                low: 9.0,
                volume_shares: Some(100),
            }]
            .into())
        })
        .await;
    assert!(result.unwrap_err().to_string().contains("达到上限"));
}

#[tokio::test]
async fn tencent_hk_history_pagination_accepts_a_confirmed_exhausted_new_listing() {
    let start = chrono::NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
    let end = chrono::NaiveDate::from_ymd_opt(2026, 11, 7).unwrap();
    let mut requested = 0;
    let result = super::collect_tencent_hk_history_range("01171", start, end, |cursor| {
        requested += 1;
        let exhausted = requested > 1;
        async move {
            Ok(if exhausted {
                Vec::new()
            } else {
                vec![super::HkDailyBar {
                    date: (cursor - chrono::Duration::days(1))
                        .format("%Y-%m-%d")
                        .to_string(),
                    open: 10.0,
                    close: 10.0,
                    high: 11.0,
                    low: 9.0,
                    volume_shares: Some(100),
                }]
            }
            .into())
        }
    })
    .await
    .unwrap();
    assert_eq!(result.bars.len(), 1);
    assert_eq!(requested, 2);
}

#[test]
fn hk_history_normalization_preserves_returns_and_does_not_mix_adjustments() {
    let backward: super::HkHistoryRecord = serde_json::from_value(serde_json::json!({
        "bars":[{"date":"2026-11-05","open":20.0,"close":20.0,"high":21.0,"low":19.0},
            {"date":"2026-11-06","open":40.0,"close":40.0,"high":41.0,"low":39.0}],
        "source":"fixture","fetched_at":"200","price_adjustment":"hfq"
    }))
    .unwrap();
    let quote: super::HkQuote =
        serde_json::from_value(serde_json::json!({"code":"01171","name":"兖矿能源",
            "currency":"HKD","price":10.0,"source":"fixture","quote_time":"2026/11/06 16:00:00"}))
        .unwrap();
    let normalized = super::hk_history_bars_for_quote(&backward, Some(&quote));
    assert_eq!(normalized[1].close, 10.0);
    assert_eq!(normalized[1].close / normalized[0].close, 2.0);
    assert_eq!(backward.bars[1].close, 40.0);
    let mut old_forward = backward.clone();
    old_forward.price_adjustment = super::HkHistoryPriceAdjustment::Forward;
    old_forward.fetched_at = "100".to_owned();
    old_forward.bars[0].date = "2020-01-02".to_owned();
    let merged = super::merge_hk_history_record(Some(&old_forward), backward);
    assert_eq!(merged.bars.len(), 2);
    assert_eq!(merged.bars[0].date, "2026-11-05");
    assert_eq!(
        merged.price_adjustment,
        super::HkHistoryPriceAdjustment::Backward
    );
}

#[test]
fn hk_history_recent_observation_refresh_keeps_the_prepared_archive() {
    let mut cache = super::HkMarketCache::default();
    let archive: super::HkHistoryRecord = serde_json::from_value(serde_json::json!({
        "bars":[{"date":"2020-01-02","open":5.0,"close":5.0,"high":6.0,"low":4.0},
            {"date":"2026-10-30","open":10.0,"close":10.0,"high":11.0,"low":9.0}],
        "source":"fixture","fetched_at":"100","price_adjustment":"hfq",
        "request_range":{"start_date":"20200101","end_date":"20261030"}
    }))
    .unwrap();
    cache.histories.insert("01171".to_owned(), archive);
    super::merge_hk_observation_update(
        &mut cache,
        "01171",
        Err("fixture quote unavailable".to_owned()),
        Ok(super::HkHistoryRecord {
            bars: vec![
                super::HkDailyBar {
                    date: "2026-10-30".to_owned(),
                    open: 10.0,
                    close: 10.0,
                    high: 11.0,
                    low: 9.0,
                    volume_shares: Some(100),
                },
                super::HkDailyBar {
                    date: "2026-11-06".to_owned(),
                    open: 20.0,
                    close: 20.0,
                    high: 21.0,
                    low: 19.0,
                    volume_shares: Some(100),
                },
            ],
            source: "fixture".to_owned(),
            fetched_at: "200".to_owned(),
            price_adjustment: super::HkHistoryPriceAdjustment::Backward,
            request_range: Some(super::HkHistoryRequestRange {
                start_date: "20261030".to_owned(),
                end_date: "20261107".to_owned(),
            }),
            ..Default::default()
        }),
        "200",
    );
    let history = &cache.histories["01171"];
    assert_eq!(history.bars.len(), 3);
    assert_eq!(history.bars.first().unwrap().date, "2020-01-02");
    assert_eq!(history.bars.last().unwrap().close, 20.0);
    assert_eq!(history.request_ranges[0].start_date, "20200101");
}

#[test]
fn hk_history_observation_retains_the_incoming_price_basis_during_a_concurrent_switch() {
    let mut cache = super::HkMarketCache::default();
    let latest: super::HkHistoryRecord = serde_json::from_value(serde_json::json!({
        "bars":[{"date":"2020-01-02","open":20.0,"close":20.0,"high":21.0,"low":19.0},
            {"date":"2026-11-06","open":40.0,"close":40.0,"high":41.0,"low":39.0}],
        "source":"fixture","fetched_at":"200","price_adjustment":"hfq",
        "request_range":{"start_date":"20200101","end_date":"20261107"}
    }))
    .unwrap();
    cache.histories.insert("01171".to_owned(), latest.clone());
    let incoming: super::HkHistoryRecord = serde_json::from_value(serde_json::json!({
        "bars":[{"date":"2024-04-02","open":10.0,"close":10.0,"high":11.0,"low":9.0}],
        "source":"fixture","fetched_at":"100","price_adjustment":"qfq"
    }))
    .unwrap();
    super::merge_hk_observation_update(
        &mut cache,
        "01171",
        Err("fixture quote unavailable".to_owned()),
        Ok(incoming),
        "100",
    );
    assert_eq!(cache.histories["01171"], latest);
}

#[tokio::test]
async fn hk_history_pagination_rejects_a_price_revision_at_the_page_boundary() {
    let start = chrono::NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
    let end = chrono::NaiveDate::from_ymd_opt(2026, 11, 7).unwrap();
    let mut page = 0;
    let error = super::collect_tencent_hk_history_range("01171", start, end, |cursor| {
        page += 1;
        let number = page;
        async move {
            Ok(if number == 1 {
                vec![super::HkDailyBar {
                    date: "2024-04-02".to_owned(),
                    open: 20.0,
                    close: 20.0,
                    high: 21.0,
                    low: 19.0,
                    volume_shares: Some(100),
                }]
            } else {
                vec![
                    super::HkDailyBar {
                        date: "2019-01-22".to_owned(),
                        open: 10.0,
                        close: 10.0,
                        high: 11.0,
                        low: 9.0,
                        volume_shares: Some(100),
                    },
                    super::HkDailyBar {
                        date: cursor.format("%Y-%m-%d").to_string(),
                        open: 19.0,
                        close: 19.0,
                        high: 20.0,
                        low: 18.0,
                        volume_shares: Some(100),
                    },
                ]
            }
            .into())
        }
    })
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        super::HkHistoryFetchError::InvalidPrices(_)
    ));
}

#[test]
fn hkex_directory_marks_listed_equities_usable_without_manual_confirmation() {
    let rows = vec![
        vec![
            "Stock Code".into(),
            "Name of Securities".into(),
            "Category".into(),
            "Subcategory".into(),
            "Board Lot".into(),
            "Trading Currency".into(),
        ],
        vec![
            "00011".into(),
            "Bank".into(),
            "Equity".into(),
            "Equity Securities (Main Board)".into(),
            "100".into(),
            "HKD".into(),
        ],
        vec![
            "00012".into(),
            "Dual Counter".into(),
            "Equity".into(),
            "Equity Securities (Main Board)".into(),
            "100".into(),
            "RMB".into(),
        ],
        vec![
            "03188".into(),
            "ETF".into(),
            "Equity".into(),
            "Exchange Traded Funds".into(),
            "100".into(),
            "HKD".into(),
        ],
    ];

    let securities = parse_hkex_security_rows(&rows).unwrap();

    assert_eq!(
        securities
            .iter()
            .map(|item| (item.code.as_str(), item.currency.as_str()))
            .collect::<Vec<_>>(),
        [("00011", "HKD"), ("00012", "CNY")]
    );
    assert_eq!(
        serde_json::to_value(&securities[0]).unwrap()["classification_status"],
        "exchange_listed_equity"
    );
    assert_eq!(
        securities[0].security_type,
        "Equity / Equity Securities (Main Board)"
    );
    assert_eq!(
        securities[1].classification_status,
        super::HkClassificationStatus::ExchangeListedEquity
    );
}

#[test]
fn legacy_hkex_equity_cache_is_upgraded_but_non_equity_classification_stays_closed() {
    let mut cached = vec![
        super::HkSecurity {
            code: "00700".to_owned(),
            name: "Tencent".to_owned(),
            currency: "HKD".to_owned(),
            board_lot: Some(100),
            security_type: "Equity / Equity Securities (Main Board)".to_owned(),
            classification_status: super::HkClassificationStatus::Unverified,
        },
        super::HkSecurity {
            code: "03188".to_owned(),
            name: "ETF".to_owned(),
            currency: "HKD".to_owned(),
            board_lot: Some(100),
            security_type: "Equity / Exchange Traded Funds".to_owned(),
            classification_status: super::HkClassificationStatus::Unverified,
        },
    ];

    super::normalize_cached_hk_classifications(&mut cached);

    assert!(cached[0].classification_status.allows_observation());
    assert_eq!(
        cached[0].classification_status,
        super::HkClassificationStatus::ExchangeListedEquity
    );
    assert!(!cached[1].classification_status.allows_observation());
}

#[test]
fn hk_quote_currency_must_match_security_master() {
    let cny = CurrencyCode::new("CNY").unwrap();
    let quote = parse_tencent_hk_quote(
        include_str!("../../../../testdata/tencent-hk-quote.txt"),
        "00700.HK",
        &cny,
    );

    assert!(quote.unwrap_err().contains("currency"));
}

#[test]
fn tencent_screen_quotes_read_pb_from_valuation_instead_of_intraday_range() {
    let currency = CurrencyCode::new("HKD").unwrap();
    let text = include_str!("../../../../testdata/tencent-hk-screen-quotes.txt");
    let carpet = parse_tencent_hk_quote(text, "00146.HK", &currency).unwrap();
    let tea = parse_tencent_hk_quote(text, "02555.HK", &currency).unwrap();
    assert_eq!(carpet.pb, Some(0.66));
    assert_eq!(tea.pb, Some(1.27));
    assert_eq!(carpet.name_zh.as_deref(), Some("太平地毡"));
    assert_eq!(tea.name_zh.as_deref(), Some("茶百道"));
}

#[test]
fn tencent_quote_bytes_preserve_chinese_names_in_both_provider_encodings() {
    let text = include_str!("../../../../testdata/tencent-hk-screen-quotes.txt");
    let (gb18030, _, had_errors) = encoding_rs::GB18030.encode(text);
    assert!(!had_errors);
    for body in [gb18030.as_ref(), text.as_bytes()] {
        let decoded = super::decode_tencent_hk_quote_response(body).unwrap();
        let quote =
            parse_tencent_hk_quote(&decoded, "00146.HK", &CurrencyCode::new("HKD").unwrap())
                .unwrap();
        assert_eq!(quote.name_zh.as_deref(), Some("太平地毡"));
        assert!(!decoded.contains('\u{FFFD}'));
    }
}

#[test]
fn eastmoney_hk_details_keep_diluted_eps_and_operating_cash_flow_per_share() {
    let snapshot = super::parse_hk_financial_snapshot(
        include_str!("../../../../testdata/eastmoney-hk-chabaidao.json"),
        "02555.HK",
        "fetched-at",
    )
    .unwrap();
    let diluted = &snapshot.additional_metrics["diluted_eps"];
    assert_eq!(diluted.value, Some(0.228));
    assert_eq!(diluted.currency, "CNY");
    assert_eq!(diluted.unit, "currency/share");
    let cash = &snapshot.additional_metrics["operating_cash_flow_per_share"];
    assert_eq!(cash.value, Some(0.129829150888));
    assert_eq!(cash.currency, "CNY");
    assert_eq!(cash.unit, "currency/share");
}

#[test]
fn hk_company_profile_preserves_official_english_names_and_chinese_business_context() {
    let profile = super::parse_hk_company_profile(
        include_str!("../../../../testdata/hk-company-profile-fixture.json"),
        "01490.HK",
        "fetched-at",
    )
    .unwrap();
    assert_eq!(profile.code, "01490");
    assert_eq!(profile.name, "AI X Tech Inc.");
    assert_eq!(profile.name_zh, None);
    assert_eq!(profile.industry, "媒体及娱乐");
    assert!(profile.business_summary.contains("汽车资讯"));
    assert!(super::parse_hk_company_profile(
        include_str!("../../../../testdata/hk-company-profile-fixture.json"),
        "02878.HK",
        "fetched-at"
    )
    .is_err());
}

#[tokio::test]
#[ignore = "manual live HK quote and financial probe"]
async fn live_hk_screen_data_probe() {
    let output = std::env::var("GP_HK_SCREEN_OUTPUT")
        .expect("set GP_HK_SCREEN_OUTPUT to a writable diagnostic output path");
    let client = crate::network::http::build_http_client_with_proxy(
        "Mozilla/5.0 GuXuanYou HK screening probe",
        std::time::Duration::from_secs(8),
        None,
    )
    .unwrap();
    let mut cache = super::HkMarketCache::default();
    cache.securities = [
        ("00146", "TAI PING CARPET", 1000),
        ("02555", "CHABAIDAO", 200),
        ("01490", "AI X TECH", 4000),
        ("02878", "SOLOMON SYSTECH", 2000),
        ("08490", "NICHE-TECH SEMI", 5000),
        ("06908", "HG SEMI", 5000),
    ]
    .into_iter()
    .map(|(code, name, lot)| super::HkSecurity {
        code: code.to_owned(),
        name: name.to_owned(),
        currency: "HKD".to_owned(),
        board_lot: Some(lot),
        security_type: "Equity / Equity Securities (Main Board)".to_owned(),
        classification_status: super::HkClassificationStatus::ExchangeListedEquity,
    })
    .collect();
    let fetched_at = super::current_epoch_millis();
    let quotes = super::fetch_hk_quote_refresh(
        &client,
        &cache.securities,
        &fetched_at,
        std::time::Duration::from_secs(8),
    )
    .await;
    assert!(quotes.errors.is_empty(), "{:?}", quotes.errors);
    assert_eq!(quotes.quotes.len(), 6);
    cache.quotes = quotes
        .quotes
        .into_iter()
        .map(|quote| (quote.code.clone(), quote))
        .collect();
    assert_eq!(cache.quotes["00146"].name_zh.as_deref(), Some("太平地毡"));
    assert_eq!(cache.quotes["02555"].name_zh.as_deref(), Some("茶百道"));
    for code in ["00146", "02555", "01490", "02878", "08490", "06908"] {
        let mut snapshot = super::fetch_hk_financial_snapshot(&client, code, &fetched_at)
            .await
            .unwrap();
        super::enrich_hk_financial_snapshot_from_quote(&mut snapshot, &cache.quotes[code]);
        cache.financials.insert(code.to_owned(), snapshot);
    }
    let data = crate::hk_research::project_hk_market_data(&cache).unwrap();
    let screened = stock_optimizer_core::screen_stocks(
        &data.stocks,
        &stock_optimizer_core::ScreenCriteria::default(),
    );
    let mut result = serde_json::to_value(screened).unwrap();
    result["groups"] = serde_json::json!([]);
    result["market"] = serde_json::json!("hk");
    crate::hk_research::attach_cached_hk_financial_metrics(&mut result, &cache);
    for row in result["items"].as_array().unwrap() {
        assert_eq!(row["factor_scores"]["data_quality"], 1.0);
        assert!(!row["risk_tags"].as_array().unwrap().iter().any(|tag| tag
            .as_str()
            .is_some_and(|tag| tag.contains("财务数据缺失") || tag.contains("财务缺项"))));
    }
    let english = result["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["stock"]["code"] == "01490.HK")
        .unwrap();
    assert_eq!(english["stock"]["name"], "AI X TECH");
    assert_eq!(english["stock"]["industry"], "媒体及娱乐");
    assert!(english["theme_category"].is_null());
    assert_ne!(english["concept"], "AI算力与芯片");
    std::fs::write(output, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
}

#[test]
fn cached_sina_quote_provides_a_chinese_name_for_display_and_search() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../../testdata/sina-hk-quote.json")).unwrap();
    let quote = parse_sina_hk_quote(
        fixture["payload"].as_str().unwrap(),
        "00700",
        &CurrencyCode::new("HKD").unwrap(),
    )
    .unwrap();
    let security = super::HkSecurity {
        code: "00700".to_owned(),
        name: "TENCENT".to_owned(),
        currency: "HKD".to_owned(),
        board_lot: Some(100),
        security_type: "Equity / Equity Securities (Main Board)".to_owned(),
        classification_status: super::HkClassificationStatus::ExchangeListedEquity,
    };

    assert_eq!(
        super::hk_chinese_display_name(Some(&quote)),
        Some("腾讯控股")
    );
    assert!(super::hk_security_matches_query(
        &security,
        Some(&quote),
        "腾讯"
    ));
    assert!(!super::hk_security_matches_query(
        &security,
        Some(&quote),
        "阿里"
    ));
}

#[test]
fn english_tencent_quotes_are_eligible_for_sina_name_enrichment() {
    let mut tencent = parse_tencent_hk_quote(
        include_str!("../../../../testdata/tencent-hk-quote.txt"),
        "00700.HK",
        &CurrencyCode::new("HKD").unwrap(),
    )
    .unwrap();
    tencent.name = "CHINA RAIL CONS".to_owned();
    tencent.name_zh = None;
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../../testdata/sina-hk-quote.json")).unwrap();
    let sina = parse_sina_hk_quote(
        fixture["payload"].as_str().unwrap(),
        "00700.HK",
        &CurrencyCode::new("HKD").unwrap(),
    )
    .unwrap();

    assert!(super::hk_quote_needs_sina_name_enrichment(&tencent));
    assert!(super::enrich_hk_quote_chinese_name(&mut tencent, &sina));
    assert_eq!(tencent.name, "CHINA RAIL CONS");
    assert_eq!(tencent.name_zh.as_deref(), Some("腾讯控股"));
    assert_eq!(tencent.price, 417.8);
    assert_eq!(
        super::hk_chinese_display_name(Some(&tencent)),
        Some("腾讯控股")
    );
}

#[test]
fn per_stock_observation_adds_a_sina_name_when_tencent_returns_english_only() {
    let mut tencent = parse_tencent_hk_quote(
        include_str!("../../../../testdata/tencent-hk-quote.txt"),
        "00700.HK",
        &CurrencyCode::new("HKD").unwrap(),
    )
    .unwrap();
    tencent.name = "WEIHAI BANK".to_owned();
    tencent.name_zh = None;
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../../testdata/sina-hk-quote.json")).unwrap();
    let sina = parse_sina_hk_quote(
        fixture["payload"].as_str().unwrap(),
        "00700.HK",
        &CurrencyCode::new("HKD").unwrap(),
    )
    .unwrap();

    let supplemental_bytes = tauri::async_runtime::block_on(
        super::enrich_tencent_hk_quote_name_from_sina(&mut tencent, || async {
            Ok::<_, String>((sina, 128))
        }),
    );

    assert_eq!(supplemental_bytes, 128);
    assert_eq!(tencent.name, "WEIHAI BANK");
    assert_eq!(tencent.name_zh.as_deref(), Some("腾讯控股"));
    assert_eq!(tencent.price, 417.8);
}

#[test]
fn canonical_hk_codes_accept_cache_keys_and_display_tickers() {
    assert_eq!(super::canonical_hk_code("00700"), Some("00700".to_owned()));
    assert_eq!(super::canonical_hk_code("700.HK"), Some("00700".to_owned()));
    assert_eq!(
        super::canonical_hk_code("HK00700"),
        Some("00700".to_owned())
    );
    assert_eq!(super::canonical_hk_code("7"), Some("00007".to_owned()));
    assert_eq!(super::canonical_hk_code("7.HK"), Some("00007".to_owned()));
    assert_eq!(super::canonical_hk_code("7.HKX"), None);
}

#[test]
fn tencent_batch_quote_response_is_separated_by_requested_security_code() {
    let fixture = include_str!("../../../../testdata/tencent-hk-quote.txt");
    let source_row = fixture.trim().trim_end_matches(';');
    let (_, raw_fields) = source_row.split_once('=').unwrap();
    let mut fields = raw_fields
        .trim()
        .trim_matches('"')
        .split('~')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    fields[1] = "HSBC Holdings".to_owned();
    fields[2] = "00005".to_owned();
    let second_row = format!("v_hk00005=\"{}\";", fields.join("~"));
    let response = format!("{source_row};{second_row}");
    let hkd = CurrencyCode::new("HKD").unwrap();

    let tencent = super::parse_tencent_hk_quote(&response, "00700.HK", &hkd).unwrap();
    let hsbc = super::parse_tencent_hk_quote(&response, "00005.HK", &hkd).unwrap();

    assert_eq!(tencent.code, "00700");
    assert_eq!(hsbc.code, "00005");
    assert_eq!(hsbc.name, "HSBC Holdings");
    assert!(super::parse_tencent_hk_quote(&response, "00011.HK", &hkd).is_err());
    assert!(
        super::tencent_hk_quote_batch_url(&["00700".to_owned(), "00005".to_owned()])
            .unwrap()
            .ends_with("q=hk00700,hk00005")
    );
}

#[test]
fn quote_batch_merge_keeps_last_successful_quote_when_refresh_fails() {
    let mut cache = super::HkMarketCache::default();
    cache.securities = vec![super::HkSecurity {
        code: "00700".to_owned(),
        name: "Tencent".to_owned(),
        currency: "HKD".to_owned(),
        board_lot: Some(100),
        security_type: "Equity / Equity Securities (Main Board)".to_owned(),
        classification_status: super::HkClassificationStatus::ExchangeListedEquity,
    }];
    cache.quotes.insert(
        "00700".to_owned(),
        super::HkQuote {
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
            change_pct: None,
            volume_shares: None,
            turnover_hkd: None,
            pe: None,
            pb: None,
            dividend_yield_percent: None,
            market_cap_hkd: None,
            total_shares: None,
            circulating_shares: None,
            currency: "HKD".to_owned(),
            quote_time: None,
            source: "Tencent Finance".to_owned(),
            fetched_at: Some("previous".to_owned()),
            last_error: None,
        },
    );
    let outcome = super::HkQuoteRefreshOutcome {
        requested_count: 1,
        unsupported_currency_count: 0,
        quotes: Vec::new(),
        errors: std::collections::HashMap::from([(
            "00700".to_owned(),
            "both providers failed".to_owned(),
        )]),
        response_bytes: 0,
    };

    let summary = super::merge_hk_quote_refresh(&mut cache, outcome);

    assert_eq!(summary.failed_count, 1);
    assert_eq!(cache.quotes["00700"].price, 400.0);
    assert_eq!(
        cache.quotes["00700"].fetched_at.as_deref(),
        Some("previous")
    );
    assert_eq!(
        cache.quotes["00700"].last_error.as_deref(),
        Some("both providers failed")
    );
}

#[test]
fn unverified_hk_classification_cannot_produce_an_observation_identity() {
    let security = super::HkSecurity {
        code: "00011".to_owned(),
        name: "Bank".to_owned(),
        currency: "HKD".to_owned(),
        board_lot: Some(100),
        security_type: "Equity".to_owned(),
        classification_status: super::HkClassificationStatus::Unverified,
    };

    assert!(super::observation_security_id(&security).is_err());
}

#[test]
fn local_confirmation_marks_only_the_selected_hk_security_as_user_confirmed() {
    let mut cache = super::HkMarketCache {
        securities: vec![super::HkSecurity {
            code: "00700".to_owned(),
            name: "Tencent".to_owned(),
            currency: "HKD".to_owned(),
            board_lot: Some(100),
            security_type: "Equity".to_owned(),
            classification_status: super::HkClassificationStatus::Unverified,
        }],
        ..super::HkMarketCache::default()
    };

    super::confirm_hk_ordinary_share(&mut cache, "700.HK").unwrap();

    assert_eq!(
        cache.securities[0].classification_status,
        super::HkClassificationStatus::UserConfirmedOrdinaryShare
    );
    assert!(super::observation_security_id(&cache.securities[0]).is_ok());
}

#[test]
fn local_confirmation_rejects_securities_missing_from_the_hk_directory() {
    let mut cache = super::HkMarketCache::default();

    assert!(super::confirm_hk_ordinary_share(&mut cache, "00700.HK").is_err());
}

#[test]
fn directory_refresh_preserves_local_user_confirmations_for_matching_codes() {
    let previous = vec![super::HkSecurity {
        code: "00700".to_owned(),
        name: "Tencent old name".to_owned(),
        currency: "HKD".to_owned(),
        board_lot: Some(100),
        security_type: "Equity".to_owned(),
        classification_status: super::HkClassificationStatus::UserConfirmedOrdinaryShare,
    }];
    let mut refreshed = vec![super::HkSecurity {
        code: "00700".to_owned(),
        name: "Tencent".to_owned(),
        currency: "HKD".to_owned(),
        board_lot: Some(100),
        security_type: "Equity".to_owned(),
        classification_status: super::HkClassificationStatus::Unverified,
    }];

    super::preserve_local_hk_classifications(&previous, &mut refreshed);

    assert_eq!(refreshed[0].name, "Tencent");
    assert_eq!(
        refreshed[0].classification_status,
        super::HkClassificationStatus::UserConfirmedOrdinaryShare
    );
}

#[test]
fn supported_hk_providers_are_not_blocked_by_readiness_gates() {
    for provider in ["hkex", "tencent", "eastmoney_hk", "hkexnews"] {
        assert!(
            super::ensure_hk_provider_ready(provider).is_ok(),
            "{provider}"
        );
    }
    assert!(super::ensure_hk_provider_ready("frankfurter_ecb").is_ok());
}

#[test]
fn observation_merge_keeps_provider_observation_time_and_fetch_time() {
    let mut cache = super::HkMarketCache::default();
    let quote = super::HkQuote {
        code: "00700".to_owned(),
        name: "Tencent".to_owned(),
        name_zh: None,
        price: 300.0,
        price_semantics_version: 1,
        previous_close: None,
        open: None,
        high: None,
        low: None,
        change: None,
        change_pct: None,
        volume_shares: None,
        turnover_hkd: None,
        pe: None,
        pb: None,
        dividend_yield_percent: None,
        market_cap_hkd: None,
        total_shares: None,
        circulating_shares: None,
        currency: "HKD".to_owned(),
        quote_time: Some("2026-11-04 10:15:00".to_owned()),
        source: "Alternate Quote Provider".to_owned(),
        fetched_at: None,
        last_error: None,
    };

    let result = super::merge_hk_observation_update(
        &mut cache,
        "00700.HK",
        Ok(quote),
        Err("history unavailable".to_owned()),
        "1793778000000",
    );

    assert!(result.quote_updated);
    let saved = &cache.quotes["00700"];
    assert_eq!(saved.source, "Alternate Quote Provider");
    assert_eq!(saved.quote_time.as_deref(), Some("2026-11-04 10:15:00"));
    assert_eq!(saved.fetched_at.as_deref(), Some("1793778000000"));
}

#[test]
fn failed_observation_refresh_keeps_last_good_quote_and_history() {
    let mut cache = super::HkMarketCache::default();
    let quote = super::HkQuote {
        code: "00700".to_owned(),
        name: "Tencent".to_owned(),
        name_zh: None,
        price: 300.0,
        price_semantics_version: 1,
        previous_close: None,
        open: None,
        high: None,
        low: None,
        change: None,
        change_pct: None,
        volume_shares: None,
        turnover_hkd: None,
        pe: None,
        pb: None,
        dividend_yield_percent: None,
        market_cap_hkd: None,
        total_shares: None,
        circulating_shares: None,
        currency: "HKD".to_owned(),
        quote_time: Some("2026-11-03 10:15:00".to_owned()),
        source: "Tencent Finance".to_owned(),
        fetched_at: Some("1793691600000".to_owned()),
        last_error: None,
    };
    cache.quotes.insert("00700".to_owned(), quote.clone());
    let bar = super::HkDailyBar {
        date: "2026-11-03".to_owned(),
        open: 299.0,
        high: 301.0,
        low: 298.0,
        close: 300.0,
        volume_shares: Some(100),
    };
    cache.histories.insert(
        "00700".to_owned(),
        super::HkHistoryRecord {
            bars: vec![bar.clone()],
            request_range: None,
            request_ranges: Vec::new(),
            price_adjustment: Default::default(),
            bar_fetched_at: Default::default(),
            revision_reason: None,
            suspended_as_of: None,
            source: "Tencent Finance".to_owned(),
            fetched_at: "1793691600000".to_owned(),
            last_error: None,
        },
    );

    let result = super::merge_hk_observation_update(
        &mut cache,
        "00700.HK",
        Err("both quote sources failed".to_owned()),
        Err("history unavailable".to_owned()),
        "1793778000000",
    );

    assert!(!result.quote_updated);
    assert!(!result.history_updated);
    assert_eq!(cache.quotes["00700"].price, quote.price);
    assert_eq!(cache.quotes["00700"].fetched_at, quote.fetched_at);
    assert_eq!(cache.histories["00700"].bars, vec![bar]);
    assert_eq!(cache.histories["00700"].fetched_at, "1793691600000");
    assert!(cache.quote_errors["00700"].contains("both quote sources failed"));
}

#[test]
fn hk_quote_keeps_the_currency_matching_the_security_master() {
    let hkd = CurrencyCode::new("HKD").unwrap();
    let quote = parse_tencent_hk_quote(
        include_str!("../../../../testdata/tencent-hk-quote.txt"),
        "00700.HK",
        &hkd,
    )
    .unwrap();

    assert_eq!(quote.code, "00700");
    assert_eq!(quote.currency, "HKD");
}

#[test]
fn sina_quote_fixture_preserves_market_time_and_provider_identity() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../../testdata/sina-hk-quote.json")).unwrap();
    assert_eq!(fixture["sample"], true);
    let hkd = CurrencyCode::new("HKD").unwrap();

    let quote =
        super::parse_sina_hk_quote(fixture["payload"].as_str().unwrap(), "00700.HK", &hkd).unwrap();

    assert_eq!(quote.name, "腾讯控股");
    assert_eq!(quote.price, 401.2);
    assert_eq!(quote.currency, "HKD");
    assert_eq!(quote.source, "Sina Finance");
    assert_eq!(quote.quote_time.as_deref(), Some("2026-11-04 15:05:46"));
    assert_eq!(quote.fetched_at, None);
    let fields = serde_json::to_value(&quote).unwrap();
    assert_eq!(fields["previous_close"], 397.2);
    assert_eq!(fields["open"], 397.8);
    assert_eq!(fields["high"], 402.2);
    assert_eq!(fields["low"], 391.6);
    assert_eq!(fields["change"], 4.0);
    assert_eq!(fields["price_semantics_version"], 1);
    assert!((quote.price - 397.2 - 4.0).abs() < 1e-9);
}

#[test]
fn sina_quote_rejects_an_invalid_current_price_even_when_open_is_valid() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../../testdata/sina-hk-quote.json")).unwrap();
    let payload = fixture["payload"]
        .as_str()
        .unwrap()
        .replace("391.600,401.200,4.000", "391.600,0.000,4.000");
    let hkd = CurrencyCode::new("HKD").unwrap();

    assert!(super::parse_sina_hk_quote(&payload, "00700.HK", &hkd).is_err());
}

#[test]
fn legacy_sina_open_as_latest_price_is_not_used_for_observation() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../../testdata/sina-hk-quote.json")).unwrap();
    let hkd = CurrencyCode::new("HKD").unwrap();
    let current =
        parse_sina_hk_quote(fixture["payload"].as_str().unwrap(), "00700.HK", &hkd).unwrap();
    let mut payload = serde_json::to_value(current).unwrap();
    payload["price"] = serde_json::json!(397.8);
    for key in [
        "previous_close",
        "open",
        "high",
        "low",
        "change",
        "price_semantics_version",
    ] {
        payload.as_object_mut().unwrap().remove(key);
    }
    let legacy = serde_json::from_value::<super::HkQuote>(payload).unwrap();
    let security = super::HkSecurity {
        code: "00700".to_owned(),
        name: "Tencent".to_owned(),
        currency: "HKD".to_owned(),
        board_lot: Some(100),
        security_type: "Equity".to_owned(),
        classification_status: super::HkClassificationStatus::ExchangeListedEquity,
    };

    assert!(super::cached_quote_for_security(&security, Some(legacy)).is_none());
}

#[test]
fn legacy_tencent_quote_keeps_its_valid_current_price() {
    let current = parse_tencent_hk_quote(
        include_str!("../../../../testdata/tencent-hk-quote.txt"),
        "00700.HK",
        &CurrencyCode::new("HKD").unwrap(),
    )
    .unwrap();
    let mut payload = serde_json::to_value(current).unwrap();
    for key in [
        "previous_close",
        "open",
        "high",
        "low",
        "change",
        "price_semantics_version",
    ] {
        payload.as_object_mut().unwrap().remove(key);
    }
    let legacy = serde_json::from_value::<super::HkQuote>(payload).unwrap();
    let security = super::HkSecurity {
        code: "00700".to_owned(),
        name: "Tencent".to_owned(),
        currency: "HKD".to_owned(),
        board_lot: Some(100),
        security_type: "Equity".to_owned(),
        classification_status: super::HkClassificationStatus::ExchangeListedEquity,
    };

    assert_eq!(
        super::cached_quote_for_security(&security, Some(legacy))
            .unwrap()
            .price,
        417.8
    );
}

#[test]
fn sina_quote_is_selected_when_tencent_quote_fails() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../../testdata/sina-hk-quote.json")).unwrap();
    let hkd = CurrencyCode::new("HKD").unwrap();
    let sina = super::parse_sina_hk_quote(fixture["payload"].as_str().unwrap(), "00700.HK", &hkd);

    let quote = super::choose_hk_quote(Err("Tencent unavailable".to_owned()), sina, &hkd).unwrap();

    assert_eq!(quote.source, "Sina Finance");
    assert_eq!(quote.currency, "HKD");
}

#[test]
fn sina_quote_fallback_rejects_currency_mismatch() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../../testdata/sina-hk-quote.json")).unwrap();
    let cny = CurrencyCode::new("CNY").unwrap();

    let quote = super::parse_sina_hk_quote(fixture["payload"].as_str().unwrap(), "00700.HK", &cny);

    assert!(quote.unwrap_err().contains("currency"));
}

#[test]
fn cached_quote_is_not_exposed_when_its_currency_conflicts_with_the_security_master() {
    let security = super::HkSecurity {
        code: "00012".to_owned(),
        name: "Dual Counter".to_owned(),
        currency: "CNY".to_owned(),
        board_lot: Some(100),
        security_type: "Equity".to_owned(),
        classification_status: super::HkClassificationStatus::Unverified,
    };
    let quote = super::HkQuote {
        code: "00012".to_owned(),
        name: "Dual Counter".to_owned(),
        name_zh: None,
        price: 10.0,
        price_semantics_version: 1,
        previous_close: None,
        open: None,
        high: None,
        low: None,
        change: None,
        change_pct: None,
        volume_shares: None,
        turnover_hkd: None,
        pe: None,
        pb: None,
        dividend_yield_percent: None,
        market_cap_hkd: None,
        total_shares: None,
        circulating_shares: None,
        currency: "HKD".to_owned(),
        quote_time: None,
        source: "Tencent Finance".to_owned(),
        fetched_at: None,
        last_error: None,
    };

    assert!(!security.cached_quote_matches_security_master(&quote));
    assert!(super::cached_quote_for_security(&security, Some(quote)).is_none());
}

#[test]
fn quote_capability_is_unavailable_for_an_unsupported_listing_currency() {
    let security = super::HkSecurity {
        code: "00012".to_owned(),
        name: "Dual Counter".to_owned(),
        currency: "CNY".to_owned(),
        board_lot: Some(100),
        security_type: "Equity".to_owned(),
        classification_status: super::HkClassificationStatus::Unverified,
    };

    let capability = super::quote_capability_for_security(&security, None, None, None);

    assert_eq!(
        capability.status,
        crate::market_domain::CapabilityStatus::Unavailable
    );
    assert!(capability.reason.as_deref().unwrap().contains("CNY"));
}

#[test]
fn quote_capability_is_available_when_the_provider_matches_the_listing_currency() {
    let security = super::HkSecurity {
        code: "00700".to_owned(),
        name: "Tencent".to_owned(),
        currency: "HKD".to_owned(),
        board_lot: Some(100),
        security_type: "Equity".to_owned(),
        classification_status: super::HkClassificationStatus::VerifiedOrdinaryShare,
    };

    let capability = super::quote_capability_for_security(&security, Some("HKD"), None, None);

    assert_eq!(
        capability.status,
        crate::market_domain::CapabilityStatus::Available
    );
    assert_eq!(capability.coverage.as_ref().unwrap().available_count, 1);
}

#[test]
fn cached_quote_remains_available_when_provider_refresh_is_enabled() {
    let security = super::HkSecurity {
        code: "00700".to_owned(),
        name: "Tencent".to_owned(),
        currency: "HKD".to_owned(),
        board_lot: Some(100),
        security_type: "Equity".to_owned(),
        classification_status: super::HkClassificationStatus::VerifiedOrdinaryShare,
    };

    let capability = super::quote_capability_for_security(&security, Some("HKD"), None, None);

    assert_eq!(
        capability.status,
        crate::market_domain::CapabilityStatus::Available
    );
    assert_eq!(capability.reason.as_deref(), None);
}
