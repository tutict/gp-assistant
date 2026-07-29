use super::*;

fn adaptive_history(base: f64, slope: f64, bars: usize) -> Vec<HistoryBar> {
    (0..bars)
        .map(|index| {
            let close = base + slope * index as f64 + ((index % 7) as f64 - 3.0) * 0.03;
            HistoryBar {
                date: format!("2026-{index:03}"),
                open: Some(close - 0.05),
                high: Some(close + 0.18),
                low: Some(close - 0.18),
                close,
                volume: Some(1_000_000.0 + index as f64 * 1_000.0),
                capital: None,
            }
        })
        .collect()
}

fn adaptive_stock(index: usize) -> StockItem {
    StockItem {
        code: format!("{:06}.SZ", index + 1),
        name: format!("波段样本{}", index + 1),
        industry: format!("行业{}", index % 6),
        price: 10.0 + index as f64,
        pe: Some(10.0 + index as f64),
        pb: Some(1.0 + index as f64 * 0.05),
        roe: Some(0.10 + index as f64 * 0.005),
        market_cap_billion: Some(80.0 + index as f64 * 10.0),
        dividend_yield: Some(0.02),
        deducted_net_profit_billion: Some(2.0),
        deducted_net_profit_growth_rate: Some(0.08),
        change_pct: Some(if index % 2 == 0 { 0.01 } else { -0.01 }),
        amount: Some(600_000_000.0),
        turnover_rate: Some(0.025),
        volume_ratio: Some(1.1),
        ..StockItem::default()
    }
}

#[test]
fn adaptive_screen_detects_range_and_returns_distinct_primary_and_exploration_lists() {
    let stocks = (0..18).map(adaptive_stock).collect::<Vec<_>>();
    let histories = stocks
        .iter()
        .map(|stock| {
            (
                stock.code.clone(),
                adaptive_history(stock.price - 0.2, 0.002, 90),
            )
        })
        .collect::<HashMap<_, _>>();
    let benchmarks = ["000001.SH", "399001.SZ", "399006.SZ"]
        .into_iter()
        .map(|code| (code.to_string(), adaptive_history(100.0, 0.0, 90)))
        .collect::<HashMap<_, _>>();
    let request = AdaptiveScreenRequest {
        primary_limit: 5,
        exploration_limit: 5,
        ..AdaptiveScreenRequest::default()
    };
    let recent = Vec::<AdaptiveRecentExposure>::new();
    let result = adaptive_screen_stocks(&stocks, &histories, &benchmarks, &recent, &request)
        .expect("adaptive screen should run");
    assert_eq!(result.algorithm_version, "adaptive_swing_v1");
    assert_eq!(result.market_regime.detected, "range");
    assert_eq!(result.market_regime.effective, "range");
    assert_eq!(result.groups[0].key, "primary");
    assert_eq!(result.groups[1].key, "exploration");
    assert_eq!(result.items.len(), 5);
    let primary = result
        .items
        .iter()
        .map(|item| &item.stock.code)
        .collect::<HashSet<_>>();
    assert!(result.groups[1]
        .items
        .iter()
        .all(|item| !primary.contains(&item.stock.code)));
}

fn adaptive_fixture(
    benchmark_slope: f64,
) -> (
    Vec<StockItem>,
    HashMap<String, Vec<HistoryBar>>,
    HashMap<String, Vec<HistoryBar>>,
) {
    let stocks = (0..18).map(adaptive_stock).collect::<Vec<_>>();
    let histories = stocks
        .iter()
        .map(|stock| {
            (
                stock.code.clone(),
                adaptive_history(stock.price - 0.2, 0.002, 90),
            )
        })
        .collect::<HashMap<_, _>>();
    let benchmarks = ["000001.SH", "399001.SZ", "399006.SZ"]
        .into_iter()
        .map(|code| {
            (
                code.to_string(),
                adaptive_history(100.0, benchmark_slope, 90),
            )
        })
        .collect::<HashMap<_, _>>();
    (stocks, histories, benchmarks)
}

#[test]
fn adaptive_screen_detects_trend_defensive_and_manual_override() {
    let (mut trend_stocks, histories, benchmarks) = adaptive_fixture(0.22);
    for stock in &mut trend_stocks {
        stock.change_pct = Some(1.0);
    }
    let trend = adaptive_screen_stocks(
        &trend_stocks,
        &histories,
        &benchmarks,
        &[],
        &AdaptiveScreenRequest::default(),
    )
    .expect("trend fixture should run");
    assert_eq!(trend.market_regime.detected, "trend");

    let (mut defensive_stocks, defensive_histories, mut defensive_benchmarks) =
        adaptive_fixture(-0.22);
    for bars in defensive_benchmarks.values_mut() {
        for (index, bar) in bars.iter_mut().enumerate() {
            bar.close = 100.0 * 0.998_f64.powi(index as i32);
            bar.open = Some(bar.close * 1.001);
            bar.high = Some(bar.close * 1.002);
            bar.low = Some(bar.close * 0.998);
        }
    }
    for stock in &mut defensive_stocks {
        stock.change_pct = Some(-1.0);
    }
    let defensive = adaptive_screen_stocks(
        &defensive_stocks,
        &defensive_histories,
        &defensive_benchmarks,
        &[],
        &AdaptiveScreenRequest::default(),
    )
    .expect("defensive fixture should run");
    assert_eq!(defensive.market_regime.detected, "defensive");

    let overridden = adaptive_screen_stocks(
        &defensive_stocks,
        &defensive_histories,
        &defensive_benchmarks,
        &[],
        &AdaptiveScreenRequest {
            mode: "trend".to_string(),
            ..AdaptiveScreenRequest::default()
        },
    )
    .expect("manual override should run");
    assert_eq!(overridden.market_regime.detected, "defensive");
    assert_eq!(overridden.market_regime.effective, "trend");
    assert!(overridden.market_regime.overridden);
}

#[test]
fn adaptive_screen_rejects_low_history_coverage_and_caps_primary_industry() {
    let (mut stocks, mut histories, benchmarks) = adaptive_fixture(0.0);
    for stock in stocks.iter().skip(8) {
        histories.remove(&stock.code);
    }
    let error = adaptive_screen_stocks(
        &stocks,
        &histories,
        &benchmarks,
        &[],
        &AdaptiveScreenRequest::default(),
    )
    .expect_err("coverage below 60 percent must fail");
    assert!(error.to_string().contains("历史数据不足"));

    let (_, complete_histories, _) = adaptive_fixture(0.0);
    for stock in &mut stocks {
        stock.industry = "单一行业".to_string();
    }
    let error = adaptive_screen_stocks(
        &stocks,
        &complete_histories,
        &benchmarks,
        &[],
        &AdaptiveScreenRequest {
            primary_limit: 10,
            exploration_limit: 0,
            ..AdaptiveScreenRequest::default()
        },
    )
    .expect_err("a single-industry pool cannot satisfy a ten-stock primary list");
    assert!(error.to_string().contains("10"));
}

#[test]
fn adaptive_candidate_prefetch_pool_is_capped_at_eighty_and_four_per_industry() {
    let mut stocks = (0..150).map(adaptive_stock).collect::<Vec<_>>();
    for (index, stock) in stocks.iter_mut().enumerate() {
        stock.code = format!("{:06}.SZ", index + 1);
        stock.industry = format!("行业{}", index % 30);
    }
    let codes = adaptive_candidate_codes(&stocks, &ScreenCriteria::default(), 80);
    assert_eq!(codes.len(), 80);
    let by_code = stocks
        .iter()
        .map(|stock| (stock.code.as_str(), stock.industry.as_str()))
        .collect::<HashMap<_, _>>();
    let mut counts = HashMap::<&str, usize>::new();
    for code in &codes {
        *counts.entry(by_code[code.as_str()]).or_default() += 1;
    }
    assert!(counts.values().all(|count| *count <= 4));
}

#[test]
fn adaptive_candidate_pool_always_rejects_st_even_when_legacy_filter_allows_it() {
    let mut stocks = (0..12).map(adaptive_stock).collect::<Vec<_>>();
    stocks[0].is_st = true;
    let criteria = ScreenCriteria {
        include_st: true,
        ..ScreenCriteria::default()
    };
    let codes = adaptive_candidate_codes(&stocks, &criteria, 80);
    assert!(!codes.contains(&stocks[0].code));
}

#[test]
fn adaptive_screen_rejects_a_pool_smaller_than_the_requested_primary_list() {
    let (stocks, histories, benchmarks) = adaptive_fixture(0.0);
    let stocks = stocks.into_iter().take(4).collect::<Vec<_>>();
    let error = adaptive_screen_stocks(
        &stocks,
        &histories,
        &benchmarks,
        &[],
        &AdaptiveScreenRequest::default(),
    )
    .expect_err("a partial list must not be returned when fewer than primary_limit are usable");
    assert!(error.to_string().contains("10"));
}

#[test]
fn adaptive_backtest_contract_keeps_requested_mode_and_prefetches_the_point_in_time_universe() {
    assert_eq!(
        normalize_backtest_strategy_mode("adaptive_swing_v1:defensive"),
        "adaptive_swing_v1"
    );
    assert_eq!(
        adaptive_backtest_requested_mode("adaptive_swing_v1:defensive"),
        "defensive"
    );
    let stocks = (0..12).map(adaptive_stock).collect::<Vec<_>>();
    let symbols = backtest_selected_symbols(
        &stocks,
        &BacktestRequest {
            criteria: ScreenCriteria::default(),
            source: "criteria".to_string(),
            strategy_mode: "adaptive_swing_v1:auto".to_string(),
            stock_codes: Vec::new(),
            start_date: "20200101".to_string(),
            end_date: "20260729".to_string(),
            top_n: 10,
            initial_cash: 1_000_000.0,
            rebalance_frequency: "monthly".to_string(),
            transaction_cost_bps: 10.0,
            benchmark: "candidate_equal_weight".to_string(),
        },
    );
    assert_eq!(symbols.len(), stocks.len());
}

#[test]
fn adaptive_rebalance_propagates_market_data_errors_instead_of_silently_clearing_positions() {
    let selection_date = NaiveDate::from_ymd_opt(2026, 4, 1).expect("valid date");
    let snapshot_date = NaiveDate::from_ymd_opt(2026, 1, 1).expect("valid date");
    let stocks = (0..12).map(adaptive_stock).collect::<Vec<_>>();
    let histories = stocks
        .iter()
        .map(|stock| {
            let bars = (0..90)
                .map(|index| {
                    let date = snapshot_date + chrono::Duration::days(index);
                    HistoryBar {
                        date: date.format("%Y-%m-%d").to_string(),
                        open: Some(stock.price),
                        high: Some(stock.price * 1.01),
                        low: Some(stock.price * 0.99),
                        close: stock.price,
                        volume: Some(1_000_000.0),
                        capital: None,
                    }
                })
                .collect::<Vec<_>>();
            BacktestHistory {
                code: stock.code.clone(),
                prices: BTreeMap::new(),
                bars,
            }
        })
        .collect::<Vec<_>>();
    let history_index = histories
        .iter()
        .enumerate()
        .map(|(index, history)| (history.code.to_ascii_uppercase(), index))
        .collect::<HashMap<_, _>>();
    let snapshots = stocks
        .iter()
        .map(|stock| {
            (
                stock.code.clone(),
                BTreeMap::from([(
                    snapshot_date,
                    StockFactorSnapshot {
                        date: "2026-01-01".to_string(),
                        available_date: Some("2026-01-01".to_string()),
                        name: Some(stock.name.clone()),
                        industry: Some(stock.industry.clone()),
                        is_st: Some(false),
                        is_listed: Some(true),
                        is_tradable: Some(true),
                        price: Some(stock.price),
                        pe: stock.pe,
                        pb: stock.pb,
                        roe: stock.roe,
                        market_cap_billion: stock.market_cap_billion,
                        change_pct: stock.change_pct,
                        volume: stock.volume,
                        amount: stock.amount,
                        turnover_rate: stock.turnover_rate,
                        volume_ratio: stock.volume_ratio,
                        ..StockFactorSnapshot::default()
                    },
                )]),
            )
        })
        .collect::<HashMap<_, _>>();
    let error = adaptive_walk_forward_active_indices(
        &stocks,
        &BacktestRequest {
            criteria: ScreenCriteria::default(),
            source: "criteria".to_string(),
            strategy_mode: "adaptive_swing_v1:auto".to_string(),
            stock_codes: Vec::new(),
            start_date: "20260101".to_string(),
            end_date: "20260401".to_string(),
            top_n: 10,
            initial_cash: 1_000_000.0,
            rebalance_frequency: "monthly".to_string(),
            transaction_cost_bps: 10.0,
            benchmark: "candidate_equal_weight".to_string(),
        },
        &histories,
        &HashMap::new(),
        &snapshots,
        &history_index,
        &vec![Some(10.0); histories.len()],
        selection_date,
    )
    .expect_err("missing benchmark histories must invalidate the rebalance");
    assert!(error.to_string().contains("调仓日 2026-04-01 数据不足"));
    assert!(error.to_string().contains("宽基指数"));
}

#[test]
fn adaptive_exploration_exposure_is_soft_and_same_day_results_are_deterministic() {
    let (mut stocks, histories, benchmarks) = adaptive_fixture(0.0);
    for (index, stock) in stocks.iter_mut().enumerate() {
        stock.industry = format!("独立行业{index}");
    }
    let request = AdaptiveScreenRequest {
        primary_limit: 5,
        exploration_limit: 50,
        ..AdaptiveScreenRequest::default()
    };
    let first = adaptive_screen_stocks(&stocks, &histories, &benchmarks, &[], &request)
        .expect("baseline should run");
    let repeated = adaptive_screen_stocks(&stocks, &histories, &benchmarks, &[], &request)
        .expect("repeat should run");
    let codes = |result: &AdaptiveScreenResult| {
        result
            .groups
            .iter()
            .flat_map(|group| group.items.iter().map(|item| item.stock.code.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(codes(&first), codes(&repeated));

    let exposed_code = first.groups[1].items[0].stock.code.clone();
    let exposed = vec![AdaptiveRecentExposure {
        code: exposed_code.clone(),
        trade_date: "20260729".to_string(),
        bucket: "exploration".to_string(),
    }];
    let reranked = adaptive_screen_stocks(&stocks, &histories, &benchmarks, &exposed, &request)
        .expect("exposure rerank should run");
    assert!(reranked.groups[1]
        .items
        .iter()
        .any(|item| item.stock.code == exposed_code));
    assert_ne!(reranked.groups[1].items[0].stock.code, exposed_code);
    assert!(reranked
        .groups
        .iter()
        .flat_map(|group| &group.items)
        .all(|item| item.score.is_finite() && (0.0..=20.0).contains(&item.score)));
}

#[test]
fn adaptive_auto_rejects_missing_breadth_while_manual_mode_marks_detection_insufficient() {
    let (mut stocks, histories, benchmarks) = adaptive_fixture(0.0);
    for stock in &mut stocks {
        stock.change_pct = None;
    }
    let automatic = adaptive_screen_stocks(
        &stocks,
        &histories,
        &benchmarks,
        &[],
        &AdaptiveScreenRequest::default(),
    )
    .expect_err("auto mode must not invent neutral market breadth");
    assert!(automatic.to_string().contains("覆盖率不足"));

    let manual = adaptive_screen_stocks(
        &stocks,
        &histories,
        &benchmarks,
        &[],
        &AdaptiveScreenRequest {
            mode: "range".to_string(),
            ..AdaptiveScreenRequest::default()
        },
    )
    .expect("manual override can score while detection reports insufficient evidence");
    assert_eq!(manual.market_regime.detected, "insufficient");
    assert_eq!(manual.market_regime.effective, "range");
    assert_eq!(manual.market_regime.confidence, 0.0);
    assert!(!manual.market_regime.coverage.breadth_usable);
}

#[test]
fn adaptive_auto_rejects_partial_breadth_and_reports_observed_coverage() {
    let (mut stocks, histories, benchmarks) = adaptive_fixture(0.0);
    for stock in stocks.iter_mut().skip(5) {
        stock.change_pct = None;
    }
    let automatic = adaptive_screen_stocks(
        &stocks,
        &histories,
        &benchmarks,
        &[],
        &AdaptiveScreenRequest::default(),
    )
    .expect_err("a handful of quotes must not represent full-market breadth");
    assert!(automatic.to_string().contains("60%"));

    let manual = adaptive_screen_stocks(
        &stocks,
        &histories,
        &benchmarks,
        &[],
        &AdaptiveScreenRequest {
            mode: "range".to_string(),
            ..AdaptiveScreenRequest::default()
        },
    )
    .expect("manual mode should expose insufficient detection evidence");
    assert_eq!(manual.market_regime.detected, "insufficient");
    assert_eq!(manual.market_regime.coverage.breadth_requested, 18);
    assert_eq!(manual.market_regime.coverage.breadth_observed, 5);
    assert!((manual.market_regime.coverage.breadth_coverage_ratio - 5.0 / 18.0).abs() < 1e-6);
    assert!(!manual.market_regime.coverage.breadth_usable);
}

#[test]
fn adaptive_release_gate_requires_every_performance_diversity_and_latency_check() {
    let passing = evaluate_adaptive_release_gate(&AdaptiveReleaseGateInput {
        legacy_annualized_return: Some(0.10),
        adaptive_annualized_return: Some(0.09),
        legacy_max_drawdown: Some(-0.12),
        adaptive_max_drawdown: Some(-0.13),
        legacy_precision_at_10: Some(0.55),
        adaptive_precision_at_10: Some(0.52),
        max_primary_industry_count: Some(3),
        average_adjacent_jaccard: Some(0.70),
        five_run_unique_coverage: Some(20),
        first_run_millis: Some(20_000),
        cached_run_millis: Some(2_000),
    });
    assert!(passing.passed);
    assert!(passing.checks.iter().all(|check| check.passed));

    let incomplete = evaluate_adaptive_release_gate(&AdaptiveReleaseGateInput {
        legacy_annualized_return: Some(0.10),
        adaptive_annualized_return: Some(0.12),
        ..AdaptiveReleaseGateInput::default()
    });
    assert!(!incomplete.passed);
    assert!(incomplete
        .checks
        .iter()
        .any(|check| check.key == "cached_run_millis" && !check.passed));
}

fn sample_data_set() -> CoreDataSet {
    let stocks = vec![
        StockItem {
            code: "111111.SZ".to_string(),
            name: "样本银行".to_string(),
            industry: "银行".to_string(),
            is_st: false,
            price: 10.0,
            pe: Some(5.0),
            pb: Some(0.8),
            roe: Some(0.15),
            market_cap_billion: Some(100.0),
            dividend_yield: Some(0.04),
            deducted_net_profit_billion: Some(8.0),
            deducted_net_profit_margin: Some(12.0),
            deducted_net_profit_growth_rate: Some(12.0),
            ..StockItem::default()
        },
        StockItem {
            code: "222222.SZ".to_string(),
            name: "样本汽车".to_string(),
            industry: "汽车".to_string(),
            is_st: false,
            price: 20.0,
            pe: Some(30.0),
            pb: Some(3.0),
            roe: Some(0.08),
            market_cap_billion: Some(60.0),
            dividend_yield: Some(0.01),
            deducted_net_profit_billion: Some(3.0),
            deducted_net_profit_margin: Some(8.0),
            deducted_net_profit_growth_rate: Some(8.0),
            ..StockItem::default()
        },
    ];
    let relations = vec![StockRelation {
        source_code: "111111.SZ".to_string(),
        target_code: "222222.SZ".to_string(),
        relation_type: "custom_peer".to_string(),
        weight: 0.5,
        description: Some("\u{672c}\u{5730}\u{6837}\u{672c}\u{5173}\u{7cfb}".to_string()),
    }];
    let histories = HashMap::from([(
        "111111.SZ".to_string(),
        vec![
            HistoryBar {
                date: "2020-01-01".to_string(),
                open: Some(9.8),
                high: Some(10.2),
                low: Some(9.7),
                close: 10.0,
                volume: Some(1_000_000.0),
                capital: Some(1_000_000_000.0),
            },
            HistoryBar {
                date: "2020-01-02".to_string(),
                open: Some(10.2),
                high: Some(11.2),
                low: Some(10.1),
                close: 11.0,
                volume: Some(1_200_000.0),
                capital: Some(1_000_000_000.0),
            },
            HistoryBar {
                date: "2020-01-03".to_string(),
                open: Some(11.1),
                high: Some(12.2),
                low: Some(11.0),
                close: 12.0,
                volume: Some(1_400_000.0),
                capital: Some(1_000_000_000.0),
            },
        ],
    )]);
    CoreDataSet {
        stocks,
        relations,
        histories,
        factor_snapshots: HashMap::new(),
        financials: HashMap::from([(
            "111111.SZ".to_string(),
            StockFinancialSnapshot {
                latest_eps: Some(0.42),
                latest_bps: Some(12.5),
                operating_revenue_billion: None,
                operating_revenue_yoy: None,
                parent_net_profit_billion: None,
                parent_net_profit_yoy: None,
                gross_margin: None,
                net_margin: None,
                roe: None,
                asset_liability_ratio: None,
                goodwill_to_net_assets: None,
                pledged_share_ratio: None,
                dividend_yield: None,
                dividend_payout_ratio: None,
                goodwill_period: None,
                pledged_share_period: None,
                dividend_period: None,
                period: Some("2026Q1".to_string()),
                source: Some("data/cache/tdx_fundamentals.csv".to_string()),
                quarterly_eps: vec![
                    QuarterlyEpsPoint {
                        period: "2026Q1".to_string(),
                        value: 0.42,
                        source: None,
                    },
                    QuarterlyEpsPoint {
                        period: "2025Q1".to_string(),
                        value: 0.36,
                        source: None,
                    },
                ],
                notes: vec!["季度 EPS 明细来自缓存".to_string()],
            },
        )]),
        capital_evidence: HashMap::from([(
            "111111.SZ".to_string(),
            CapitalEvidenceResult {
                stock_code: "111111.SZ".to_string(),
                generated_at: "2026-06-25T10:00:00".to_string(),
                composite_score: Some(68.0),
                confidence: "中".to_string(),
                model_used: false,
                as_of_trade_date: Some("2026-06-25".to_string()),
                freshness: "fresh-cache".to_string(),
                contributions: BTreeMap::new(),
                summary: Some("cached capital evidence".to_string()),
                sections: Vec::new(),
                items: vec![CapitalEvidenceItem {
                    category: "fund_flow".to_string(),
                    source: "test cache".to_string(),
                    title: "fund flow sample".to_string(),
                    date: Some("2026-06-25".to_string()),
                    metrics: BTreeMap::from([("净额".to_string(), "1200万".to_string())]),
                    sentiment: "positive".to_string(),
                    weight: 0.35,
                    confidence: "中".to_string(),
                    url: None,
                    score: Some(72.0),
                    note: Some("缓存资金流样例".to_string()),
                }],
                notes: vec!["缓存说明".to_string()],
            },
        )]),
    }
}

#[test]
fn screens_by_pe_and_roe() {
    let result = screen_with_mock(&ScreenCriteria {
        max_pe: Some(10.0),
        min_roe: Some(0.1),
        ..ScreenCriteria::default()
    });
    assert_eq!(result.returned, 3);
    assert_eq!(result.total, 3);
    assert!(result
        .items
        .iter()
        .all(|item| item.stock.pe.map(|pe| pe <= 10.0).unwrap_or(false)));
    assert!(result.items.iter().all(|item| item
        .stock
        .roe
        .and_then(as_percent)
        .map(|roe| roe >= 10.0)
        .unwrap_or(false)));
    assert!(result
        .items
        .iter()
        .all(|item| item.reasons.contains(&"pe_ok".to_string())));
    assert!(result
        .items
        .windows(2)
        .all(|pair| pair[0].score >= pair[1].score));
}

#[test]
fn roe_filter_accepts_ratio_and_percent_thresholds() {
    let universe = vec![
        custom_test_stock("000001.SZ", "high-roe", "bank", 100.0, 10.0, 0.15),
        custom_test_stock("000002.SZ", "low-roe", "bank", 100.0, 10.0, 0.08),
    ];

    for minimum in [0.10, 10.0] {
        let result = screen_stocks(
            &universe,
            &ScreenCriteria {
                min_roe: Some(minimum),
                ..ScreenCriteria::default()
            },
        );
        assert_eq!(result.returned, 1);
        assert_eq!(result.items[0].stock.code, "000001.SZ");
    }
}

#[test]
fn default_screen_includes_non_st_stocks() {
    let result = screen_with_mock(&ScreenCriteria::default());
    assert_eq!(result.returned, 10);
}

#[test]
fn custom_screen_uses_top_bar_criteria() {
    let data = CoreDataSet {
        stocks: vec![
            custom_test_stock("300750.SZ", "catl", "battery", 1800.0, 20.0, 0.18),
            custom_test_stock("600519.SH", "moutai", "liquor", 2200.0, 32.0, 0.32),
            custom_test_stock("002594.SZ", "byd", "auto", 900.0, 45.0, 0.09),
        ],
        relations: vec![StockRelation {
            source_code: "300750.SZ".to_string(),
            target_code: "002594.SZ".to_string(),
            relation_type: "supply_chain".to_string(),
            weight: 1.0,
            description: Some("ignored relation".to_string()),
        }],
        histories: HashMap::new(),
        financials: HashMap::new(),
        factor_snapshots: HashMap::new(),
        capital_evidence: HashMap::new(),
    };

    let result = graph_screen_with_data(
        &data,
        &GraphScreenRequest {
            criteria: ScreenCriteria {
                max_pe: Some(35.0),
                min_roe: Some(0.15),
                ..ScreenCriteria::default()
            },
            seed_codes: vec!["002594.SZ".to_string()],
            seed_query: "byd".to_string(),
            relation_depth: 3,
            relation_weight: 1.0,
            limit: 10,
        },
    )
    .expect("custom criteria screen should run");

    let codes = result
        .items
        .iter()
        .map(|item| item.stock.code.as_str())
        .collect::<Vec<_>>();
    assert_eq!(result.center_context.mode, "custom_criteria");
    assert_eq!(result.relation_count, 0);
    assert_eq!(result.returned, 2);
    assert!(codes.contains(&"300750.SZ"));
    assert!(codes.contains(&"600519.SH"));
    assert!(!codes.contains(&"002594.SZ"));
    assert!(result.items.iter().all(|item| item.related.is_empty()));
    assert!(result.items.iter().all(|item| item.relation_score == 0.0));
}

#[test]
fn custom_screen_empty_criteria_returns_top_ranked_candidates() {
    let result = graph_screen_with_mock(&GraphScreenRequest {
        criteria: ScreenCriteria::default(),
        seed_codes: Vec::new(),
        seed_query: String::new(),
        limit: 5,
        ..GraphScreenRequest::default()
    });

    assert_eq!(result.center_context.mode, "custom_criteria");
    assert_eq!(result.returned, 5);
    assert_eq!(result.relation_count, 0);
    assert!(result.items.iter().all(|item| item.related.is_empty()));
    assert!(!result.notes.is_empty());
}

fn custom_test_stock(
    code: &str,
    name: &str,
    industry: &str,
    market_cap_billion: f64,
    pe: f64,
    roe: f64,
) -> StockItem {
    StockItem {
        code: code.to_string(),
        name: name.to_string(),
        industry: industry.to_string(),
        is_st: false,
        price: 20.0,
        pe: Some(pe),
        pb: Some(3.0),
        roe: Some(roe),
        market_cap_billion: Some(market_cap_billion),
        dividend_yield: Some(0.01),
        deducted_net_profit_billion: Some(3.0),
        deducted_net_profit_margin: Some(12.0),
        deducted_net_profit_growth_rate: Some(15.0),
        ..StockItem::default()
    }
}

fn pit_factor_snapshot(date: &str, pe: Option<f64>) -> StockFactorSnapshot {
    StockFactorSnapshot {
        date: date.to_string(),
        available_date: Some(date.to_string()),
        is_st: Some(false),
        is_listed: Some(true),
        is_tradable: Some(true),
        pe,
        ..StockFactorSnapshot::default()
    }
}
#[test]
fn backtest_uses_mock_history() {
    let result = backtest_with_mock(&BacktestRequest {
        criteria: ScreenCriteria {
            max_pe: Some(10.0),
            ..ScreenCriteria::default()
        },
        source: default_backtest_source(),
        strategy_mode: "candidate_snapshot".to_string(),
        stock_codes: Vec::new(),
        start_date: "20200101".to_string(),
        end_date: "20200110".to_string(),
        top_n: 2,
        initial_cash: 1000.0,
        rebalance_frequency: default_rebalance_frequency(),
        transaction_cost_bps: default_transaction_cost_bps(),
        benchmark: default_backtest_benchmark(),
    })
    .expect("backtest should run");
    assert_eq!(result.metrics.num_stocks, 2);
    assert!(result.equity_curve.first().unwrap().equity < 1000.0);
    assert!(result.metrics.total_transaction_cost > 0.0);
    assert!(result.metrics.total_return > 0.0);
}

#[test]
fn trend_indicator_runs_with_mock_history() {
    let result = trend_with_mock(&TrendIndicatorRequest {
        code: "300750.SZ".to_string(),
        start_date: "20200101".to_string(),
        end_date: default_end_date(),
        series_limit: 80,
    })
    .expect("trend indicator should run");
    assert_eq!(result.stock.code, "300750.SZ");
    assert!(!result.series.is_empty());
    assert!(result.signal.quant_score <= 90);
    assert!(result.signal.pattern_score <= 100);
    let latest = result.series.last().unwrap();
    assert!(latest.accumulation_index.is_some());
    assert!(latest.accumulation_strength.is_some());
    assert!(latest.swing_opportunity.is_some());
    assert!(latest.rebound_signal.is_some());
    assert!(latest.trend_heat.is_some());
}

#[test]
fn trend_with_data_allows_full_observe_history_series() {
    let mut data = sample_data_set();
    let start = NaiveDate::from_ymd_opt(2020, 1, 1).expect("valid start date");
    let history: Vec<HistoryBar> = (0..620)
        .map(|idx| {
            let close = 10.0 + idx as f64 * 0.01;
            HistoryBar {
                date: start
                    .checked_add_days(Days::new(idx))
                    .expect("valid history date")
                    .format("%Y-%m-%d")
                    .to_string(),
                open: Some(close - 0.03),
                high: Some(close + 0.08),
                low: Some(close - 0.08),
                close,
                volume: Some(1_000_000.0 + idx as f64),
                capital: Some(1_000_000_000.0),
            }
        })
        .collect();
    data.histories.insert("111111.SZ".to_string(), history);

    let result = trend_with_data(
        &data,
        &TrendIndicatorRequest {
            code: "111111.SZ".to_string(),
            start_date: "20200101".to_string(),
            end_date: "20211231".to_string(),
            series_limit: 620,
        },
    )
    .expect("full history trend should run");

    assert_eq!(result.series.len(), 620);
    let latest = result.series.last().expect("latest point");
    assert!(latest.open.is_some());
    assert!(latest.high.is_some());
    assert!(latest.low.is_some());
    assert!(latest.volume.is_some());
}

#[test]
fn trend_screen_ranks_candidates() {
    let result = trend_screen_with_mock(&TrendScreenRequest {
        criteria: ScreenCriteria::default(),
        start_date: "20200101".to_string(),
        end_date: default_end_date(),
        limit: 5,
    })
    .expect("trend screen should run");
    assert_eq!(result.returned, 5);
    assert!(result.items[0].final_score >= result.items[4].final_score);
}

#[test]
fn sector_screen_uses_full_filtered_universe_before_grouping() {
    let stocks: Vec<StockItem> = (0..260)
        .map(|idx| StockItem {
            code: format!("300{:03}.SZ", idx),
            name: format!("{}{:03}", "\u{7b97}\u{529b}\u{82af}\u{7247}", idx),
            industry: "\u{534a}\u{5bfc}\u{4f53}".to_string(),
            is_st: false,
            price: 20.0 + idx as f64 / 100.0,
            pe: Some(18.0),
            pb: Some(2.2),
            roe: Some(0.16),
            market_cap_billion: Some(80.0),
            dividend_yield: Some(0.02),
            deducted_net_profit_billion: Some(3.0),
            deducted_net_profit_margin: Some(12.0),
            deducted_net_profit_growth_rate: Some(18.0),
            ..StockItem::default()
        })
        .collect();
    let result = sector_screen_stocks(
        &stocks,
        &SectorScreenRequest {
            criteria: ScreenCriteria {
                min_roe: Some(0.1),
                limit: 10,
                ..ScreenCriteria::default()
            },
            max_sectors: 1,
            per_sector_limit: 7,
            min_sector_candidates: 1,
            group_by: default_sector_group_by(),
        },
    );

    assert_eq!(result.total, 260);
    assert_eq!(result.sector_count, 1);
    assert_eq!(result.groups[0].total, 260);
    assert_eq!(result.groups[0].returned, 7);
}

#[test]
fn sector_screen_can_group_by_market_board() {
    let stocks = vec![
        StockItem {
            code: "688001.SH".to_string(),
            name: "star-a".to_string(),
            industry: "chip".to_string(),
            is_st: false,
            price: 20.0,
            pe: Some(18.0),
            pb: Some(2.0),
            roe: Some(0.16),
            market_cap_billion: Some(80.0),
            dividend_yield: Some(0.01),
            deducted_net_profit_billion: Some(1.0),
            deducted_net_profit_margin: Some(12.0),
            deducted_net_profit_growth_rate: Some(18.0),
            ..StockItem::default()
        },
        StockItem {
            code: "300001.SZ".to_string(),
            name: "growth-a".to_string(),
            industry: "device".to_string(),
            is_st: false,
            price: 21.0,
            pe: Some(17.0),
            pb: Some(2.1),
            roe: Some(0.17),
            market_cap_billion: Some(90.0),
            dividend_yield: Some(0.01),
            deducted_net_profit_billion: Some(1.1),
            deducted_net_profit_margin: Some(13.0),
            deducted_net_profit_growth_rate: Some(19.0),
            ..StockItem::default()
        },
        StockItem {
            code: "600000.SH".to_string(),
            name: "main-a".to_string(),
            industry: "bank".to_string(),
            is_st: false,
            price: 10.0,
            pe: Some(8.0),
            pb: Some(0.8),
            roe: Some(0.12),
            market_cap_billion: Some(200.0),
            dividend_yield: Some(0.03),
            deducted_net_profit_billion: Some(2.0),
            deducted_net_profit_margin: Some(20.0),
            deducted_net_profit_growth_rate: Some(8.0),
            ..StockItem::default()
        },
    ];
    let result = sector_screen_stocks(
        &stocks,
        &SectorScreenRequest {
            group_by: "board".to_string(),
            max_sectors: 10,
            per_sector_limit: 5,
            min_sector_candidates: 1,
            ..SectorScreenRequest::default()
        },
    );

    let groups: Vec<&str> = result
        .groups
        .iter()
        .map(|group| group.sector.as_str())
        .collect();
    assert_eq!(
        groups,
        vec![
            "\u{79d1}\u{521b}\u{677f}",
            "\u{521b}\u{4e1a}\u{677f}",
            "\u{6caa}\u{4e3b}\u{677f}"
        ]
    );
}

#[test]
fn observe_with_data_returns_financial_and_trend_payloads() {
    let mut data = sample_data_set();
    let history: Vec<HistoryBar> = (0..45)
        .map(|idx| {
            let close = 10.0 + idx as f64 * 0.12;
            HistoryBar {
                date: format!("2020-{:02}-{:02}", 2 + idx / 28, idx % 28 + 1),
                open: Some(close - 0.05),
                high: Some(close + 0.2),
                low: Some(close - 0.2),
                close,
                volume: Some(1_000_000.0 + idx as f64 * 10_000.0),
                capital: Some(1_000_000_000.0),
            }
        })
        .collect();
    data.histories.insert("111111.SZ".to_string(), history);

    let result = observe_with_data(
        &data,
        &StockObserveRequest {
            code: "111111".to_string(),
            start_date: "20200201".to_string(),
            end_date: "20200331".to_string(),
            series_limit: 40,
            include_order_book: true,
        },
    )
    .expect("observation should run from the shared data set");

    assert_eq!(result.stock.code, "111111.SZ");
    let financial = result.financial_indicators.as_ref().unwrap();
    assert!(financial
        .notes
        .iter()
        .any(|note| note.contains("季度 EPS 明细来自缓存")));
    let financial_items = &financial.items;
    assert!(financial_items.len() >= 6);
    assert!(financial_items
        .iter()
        .any(|item| item.metric_key.as_deref() == Some("quarterly_eps")
            && item.period.as_deref() == Some("2026Q1")));
    let trend = result.trend.as_ref().unwrap();
    assert!(trend.series.len() <= 40);
    assert!(trend
        .series
        .iter()
        .any(|point| point.k.is_some() && point.d.is_some() && point.j.is_some()));
    assert!(trend.signal.k.is_some());
    assert!(trend.signal.d.is_some());
    assert!(trend.signal.j.is_some());
    let capital_evidence = result.capital_evidence.as_ref().unwrap();
    assert!(capital_evidence.composite_score.is_some());
    assert!(capital_evidence
        .sections
        .iter()
        .any(|section| section.key == "technical_behavior" && section.available));
    assert!(capital_evidence
        .items
        .iter()
        .any(|item| item.category == "technical_behavior"));
    assert!(capital_evidence
        .items
        .iter()
        .any(|item| item.category == "fund_flow" && item.score == Some(72.0)));
    assert!(capital_evidence
        .contributions
        .get("资金流")
        .and_then(|value| value.get("available"))
        .and_then(Value::as_bool)
        .unwrap_or(false));
    let technical_item = capital_evidence
        .items
        .iter()
        .find(|item| item.category == "technical_behavior")
        .unwrap();
    assert!(technical_item.metrics.contains_key("吸筹强度"));
    assert!(technical_item.metrics.contains_key("趋势热度"));
    assert!(!capital_evidence
        .notes
        .iter()
        .any(|note| note.contains("待迁移")));
    assert!(result.order_book.is_none());
    assert!(result.notes.iter().any(|note| note.contains("Tauri/Rust")));
}

#[test]
fn observe_with_data_preserves_detailed_fundamental_snapshot_metrics() {
    let data = sample_data_set();
    let mut payload = serde_json::to_value(ObserveWithDataRequest {
        data,
        request: StockObserveRequest {
            code: "111111.SZ".to_string(),
            start_date: "2020-01-01".to_string(),
            end_date: "2020-01-03".to_string(),
            series_limit: 40,
            include_order_book: false,
        },
    })
    .expect("observe payload should serialize");
    payload["data"]["financials"]["111111.SZ"]["operating_revenue_billion"] = json!(43.477821);
    payload["data"]["financials"]["111111.SZ"]["operating_revenue_yoy"] = json!(8.370839);
    payload["data"]["financials"]["111111.SZ"]["parent_net_profit_billion"] = json!(1.556453);
    payload["data"]["financials"]["111111.SZ"]["parent_net_profit_yoy"] = json!(53.712048);
    payload["data"]["financials"]["111111.SZ"]["gross_margin"] = json!(12.497484);
    payload["data"]["financials"]["111111.SZ"]["net_margin"] = json!(1.399071);
    payload["data"]["financials"]["111111.SZ"]["roe"] = json!(0.07);
    payload["data"]["financials"]["111111.SZ"]["asset_liability_ratio"] = json!(65.027185);
    payload["data"]["financials"]["111111.SZ"]["goodwill_to_net_assets"] = json!(17.957322);
    payload["data"]["financials"]["111111.SZ"]["pledged_share_ratio"] = json!(0.74);
    payload["data"]["financials"]["111111.SZ"]["dividend_yield"] = json!(0.95);
    payload["data"]["financials"]["111111.SZ"]["dividend_payout_ratio"] = json!(38.577797);
    payload["data"]["financials"]["111111.SZ"]["goodwill_period"] = json!("2026Q1");
    payload["data"]["financials"]["111111.SZ"]["pledged_share_period"] = json!("2026-07-10");
    payload["data"]["financials"]["111111.SZ"]["dividend_period"] = json!("2025-12-31");

    let result = observe_with_data_value(payload)
        .expect("detailed financial snapshot should remain observable");
    let items = result["financial_indicators"]["items"]
        .as_array()
        .expect("financial indicator items should exist");
    for metric_key in [
        "operating_revenue",
        "operating_revenue_yoy",
        "parent_net_profit",
        "parent_net_profit_yoy",
        "gross_margin",
        "net_margin",
        "roe",
        "asset_liability_ratio",
        "goodwill_to_net_assets",
        "pledged_share_ratio",
        "dividend_yield",
        "dividend_payout_ratio",
    ] {
        assert!(
            items
                .iter()
                .any(|item| item.get("metric_key").and_then(Value::as_str) == Some(metric_key)),
            "missing detailed financial metric: {metric_key}"
        );
    }
    let metric = |key: &str| {
        items
            .iter()
            .find(|item| item.get("metric_key").and_then(Value::as_str) == Some(key))
            .expect("financial metric should exist")
    };
    assert_eq!(metric("roe")["value"].as_str(), Some("0.070%"));
    assert_eq!(
        metric("pledged_share_ratio")["value"].as_str(),
        Some("0.740%")
    );
    assert_eq!(
        metric("pledged_share_ratio")["period"].as_str(),
        Some("2026-07-10")
    );
    assert_eq!(metric("dividend_yield")["value"].as_str(), Some("0.950%"));
    assert_eq!(
        metric("dividend_yield")["period"].as_str(),
        Some("2025-12-31")
    );
}

#[test]
fn observe_with_data_accepts_numeric_capital_metric_values() {
    let mut data = sample_data_set();
    let history: Vec<HistoryBar> = (0..45)
        .map(|idx| {
            let close = 10.0 + idx as f64 * 0.1;
            HistoryBar {
                date: format!("2020-{:02}-{:02}", 2 + idx / 28, idx % 28 + 1),
                open: Some(close - 0.05),
                high: Some(close + 0.2),
                low: Some(close - 0.2),
                close,
                volume: Some(1_000_000.0 + idx as f64 * 10_000.0),
                capital: Some(1_000_000_000.0),
            }
        })
        .collect();
    data.histories.insert("111111.SZ".to_string(), history);

    let mut payload = serde_json::to_value(ObserveWithDataRequest {
        data,
        request: StockObserveRequest {
            code: "111111.SZ".to_string(),
            start_date: "20200201".to_string(),
            end_date: "20200331".to_string(),
            series_limit: 40,
            include_order_book: false,
        },
    })
    .expect("payload should serialize");
    payload["data"]["capital_evidence"]["111111.SZ"]["items"] = json!([
        {
            "category": "community_sentiment",
            "source": "东方财富股吧",
            "title": "numeric metric cache sample",
            "date": "2026-06-25",
            "metrics": {
                "阅读数": 6.0,
                "评论数": 2,
                "标题": "numeric metric cache sample"
            },
            "sentiment": "uncertain",
            "weight": 0.15,
            "confidence": "低",
            "score": 50.0
        }
    ]);

    let result = observe_with_data_value(payload)
        .expect("numeric metric values in cached capital evidence should not block observe");
    assert!(result.get("trend").is_some());
    let items = result["capital_evidence"]["items"]
        .as_array()
        .expect("capital evidence items");
    let community = items
        .iter()
        .find(|item| item.get("category").and_then(Value::as_str) == Some("community_sentiment"))
        .expect("community sentiment item should remain");
    assert_eq!(
        community["metrics"]["阅读数"],
        Value::String("6.0".to_string())
    );
    assert!(items
        .iter()
        .any(|item| item.get("category").and_then(Value::as_str) == Some("technical_behavior")));
}
#[test]
fn observe_without_cached_capital_evidence_uses_local_proxy_items() {
    let mut data = sample_data_set();
    data.capital_evidence.clear();
    let result = observe_with_data(
        &data,
        &StockObserveRequest {
            code: "111111.SZ".to_string(),
            start_date: "2020-01-01".to_string(),
            end_date: "2020-01-03".to_string(),
            series_limit: 40,
            include_order_book: false,
        },
    )
    .expect("observation should still run without cached capital evidence");

    let capital_evidence = result.capital_evidence.as_ref().unwrap();
    assert!(capital_evidence.items.iter().any(|item| {
        item.category == "fund_flow" && item.title == "本地量价资金代理" && item.score.is_some()
    }));
    let proxy = capital_evidence
        .items
        .iter()
        .find(|item| item.category == "fund_flow" && item.title == "本地量价资金代理")
        .expect("local fund-flow proxy should be present");
    assert!(proxy.metrics.contains_key("隐性资金代理分"));
    assert!(proxy.metrics.contains_key("推断方向"));
    assert!(proxy
        .note
        .as_deref()
        .unwrap_or_default()
        .contains("不是外部主力净流入数据"));
    assert!(capital_evidence
        .items
        .iter()
        .any(|item| item.category == "message_sentiment_status"));
    assert!(capital_evidence.sections.iter().any(|section| {
        section.key == "message_sentiment"
            && section
                .items
                .iter()
                .any(|item| item.category == "message_sentiment_status")
    }));
}
#[test]
fn agent_routes_sector_request_to_custom_screen() {
    let response = run_agent_with_mock("筛选半导体趋势股").unwrap();
    assert_eq!(response.action, "trend_screen");
    assert_eq!(response.intent.as_ref().unwrap().kind, "trend_analysis");
    assert!(response.reply.contains("仅供选股研究"));
    assert!(response.data.is_some());
}
#[test]
fn agent_routes_trend_request() {
    let response = run_agent_with_mock("用趋势指标筛选上升趋势股票").unwrap();
    assert_eq!(response.action, "trend_screen");
    assert!(response.reply.contains("不构成投资建议"));
    assert!(response.data.is_some());
}
#[test]
fn agent_stream_with_data_emits_status_and_result() {
    let events = run_agent_stream_with_data_events(
        &sample_data_set(),
        "screen bank stocks",
        Some("native-run"),
        None,
    );
    assert!(events.iter().any(|event| event.event_type == "status"));
    let result = events
        .iter()
        .find(|event| event.event_type == "result")
        .expect("agent stream should return result event");
    assert_eq!(result.run_id, "native-run");
    assert_eq!(result.response.as_ref().unwrap().action, "screen");
}

#[test]
fn agent_reply_reflects_mode() {
    fn reply_for(events: &[AgentStreamEvent]) -> &str {
        events
            .iter()
            .find(|event| event.event_type == "result")
            .and_then(|event| event.response.as_ref())
            .map(|response| response.reply.as_str())
            .expect("agent stream should return a result with a reply")
    }

    let research = run_agent_stream_with_data_events(
        &sample_data_set(),
        "screen bank stocks",
        Some("mode-run"),
        None,
    );
    let research_reply = reply_for(&research);
    assert!(research_reply.contains("不构成投资建议"));

    let quick = run_agent_stream_with_data_events(
        &sample_data_set(),
        "screen bank stocks",
        Some("mode-run"),
        Some("quick"),
    );
    let quick_reply = reply_for(&quick);
    assert!(!quick_reply.contains("不构成投资建议"));
    assert!(quick_reply.len() < research_reply.len());

    let expert = run_agent_stream_with_data_events(
        &sample_data_set(),
        "screen bank stocks",
        Some("mode-run"),
        Some("expert"),
    );
    let expert_reply = reply_for(&expert);
    assert!(expert_reply.contains("风险提示"));
    assert!(expert_reply.contains("下一步"));
}

#[test]
fn agent_observe_reply_is_beginner_readable() {
    let events = run_agent_stream_with_data_events(
        &sample_data_set(),
        "observe 111111.SZ",
        Some("beginner-run"),
        Some("expert"),
    );
    let response = events
        .iter()
        .find(|event| event.event_type == "result")
        .and_then(|event| event.response.as_ref())
        .expect("agent stream should return an observe result");

    assert_eq!(response.action, "observe_stock");
    let joined = response
        .answer_sections
        .iter()
        .flat_map(|section| {
            std::iter::once(section.title.as_str())
                .chain(section.bullets.iter().map(String::as_str))
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(joined.contains("一句话结论"));
    assert!(joined.contains("新手"));
    assert!(joined.contains("支撑位"));
    assert!(joined.contains("压力位"));
    assert!(joined.contains("KDJ"));
    assert!(joined.contains("资金证据综合分"));
    assert!(joined.contains("不等于马上买入"));
    assert!(joined.contains("不是投资建议") || joined.contains("不构成投资建议"));
}
#[test]
fn mobile_stock_skill_classifies_sources_with_guardrails() {
    let result = run_mobile_stock_skill(&MobileStockSkillRequest {
        stock_code: "300750.SZ".to_string(),
        stock_name: "宁德时代".to_string(),
        question: "分析近期利好利空".to_string(),
        sources: vec![
            MobileStockSourceItem {
                title: "宁德时代签订储能订单公告".to_string(),
                summary: "公司披露新签订单增长，交付节奏改善。".to_string(),
                source_tier: "filing".to_string(),
                source_name: "巨潮资讯".to_string(),
                published_at: Some("2026-06-08".to_string()),
                source_url: Some("https://example.test/notice".to_string()),
                evidence: "公司披露新签订单增长，预计对经营产生积极影响。".to_string(),
            },
            MobileStockSourceItem {
                title: "股吧讨论短线必涨".to_string(),
                summary: "社区讨论称短线必涨。".to_string(),
                source_tier: "community".to_string(),
                source_name: "股吧".to_string(),
                published_at: Some("2026-06-08".to_string()),
                source_url: None,
                evidence: "社区讨论称短线必涨。".to_string(),
            },
        ],
    });

    assert_eq!(result.overview.positive_count, 1);
    assert_eq!(result.overview.unverified_count, 1);
    assert_eq!(result.positive_factors[0].source_tier, "filing");
    assert_eq!(result.unverified_leads[0].label, "unverified");
    assert!(result.positive_factors[0]
        .risk_note
        .contains("不构成投资建议"));
    assert!(result
        .notes
        .iter()
        .any(|note| note.contains("社区来源只作为待验证线索")));
}
#[test]
fn mobile_stock_skill_does_not_invent_without_sources() {
    let result = run_mobile_stock_skill(&MobileStockSkillRequest {
        stock_code: "300750.SZ".to_string(),
        stock_name: "宁德时代".to_string(),
        question: "分析近期利好利空".to_string(),
        sources: Vec::new(),
    });

    assert_eq!(result.overview.overall_label, "neutral");
    assert_eq!(result.overview.positive_count, 0);
    assert!(result.overview.summary.contains("未找到可靠信源"));
    assert!(result
        .notes
        .iter()
        .any(|note| note.contains("未找到可靠信源")));
}
#[test]
fn validates_native_data_set() {
    let summary = validate_data_set(&sample_data_set()).expect("data set should be valid");
    assert_eq!(summary.stock_count, 2);
    assert_eq!(summary.relation_count, 1);
    assert_eq!(summary.history_bar_count, 3);
}

#[test]
fn screens_with_native_data_set() {
    let result = screen_with_data(
        &sample_data_set(),
        &ScreenCriteria {
            max_pe: Some(10.0),
            ..ScreenCriteria::default()
        },
    )
    .expect("native data screen should run");
    assert_eq!(result.returned, 1);
    assert_eq!(result.items[0].stock.code, "111111.SZ");
}

#[test]
fn screens_with_deducted_profit_quality_rule() {
    let result = screen_with_data(
        &sample_data_set(),
        &ScreenCriteria {
            min_deducted_net_profit_billion: Some(0.0),
            min_deducted_net_profit_growth_rate: Some(10.0),
            ..ScreenCriteria::default()
        },
    )
    .expect("native data screen should run");

    assert_eq!(result.returned, 1);
    assert_eq!(result.items[0].stock.code, "111111.SZ");
    assert!(result.items[0]
        .reasons
        .contains(&"deducted_net_profit_ok".to_string()));
    assert!(result.items[0]
        .reasons
        .contains(&"deducted_net_profit_growth_rate_ok".to_string()));
}

#[test]
fn screens_native_data_with_partial_industry_match() {
    let mut data = sample_data_set();
    data.stocks[0].industry = "银行服务".to_string();

    let result = screen_with_data(
        &data,
        &ScreenCriteria {
            industry: Some("银行".to_string()),
            ..ScreenCriteria::default()
        },
    )
    .expect("native data screen should run");

    assert_eq!(result.returned, 1);
    assert_eq!(result.items[0].stock.code, "111111.SZ");
}

#[test]
fn selected_industry_does_not_match_empty_stock_industry() {
    let universe = vec![StockItem {
        code: "000001.SZ".to_string(),
        name: "平安银行".to_string(),
        industry: "".to_string(),
        is_st: false,
        price: 11.0,
        pe: Some(5.0),
        pb: Some(0.8),
        roe: Some(0.16),
        market_cap_billion: Some(240.0),
        dividend_yield: None,
        latest_eps: None,
        deducted_net_profit_billion: None,
        deducted_net_profit_margin: None,
        deducted_net_profit_growth_rate: None,
        change_pct: None,
        volume: None,
        amount: None,
        turnover_rate: None,
        volume_ratio: None,
        quote_time: None,
    }];

    let result = screen_stocks(
        &universe,
        &ScreenCriteria {
            industry: Some("银行".to_string()),
            ..ScreenCriteria::default()
        },
    );

    assert_eq!(result.returned, 0);
}

#[test]
fn optional_metric_sort_keeps_missing_values_last() {
    let universe = vec![
        StockItem {
            code: "000001.SZ".to_string(),
            name: "平安银行".to_string(),
            industry: "银行".to_string(),
            is_st: false,
            price: 11.0,
            pe: None,
            pb: None,
            roe: None,
            market_cap_billion: None,
            dividend_yield: None,
            latest_eps: None,
            deducted_net_profit_billion: None,
            deducted_net_profit_margin: None,
            deducted_net_profit_growth_rate: None,
            change_pct: None,
            volume: None,
            amount: None,
            turnover_rate: None,
            volume_ratio: None,
            quote_time: None,
        },
        StockItem {
            code: "600000.SH".to_string(),
            name: "浦发银行".to_string(),
            industry: "银行".to_string(),
            is_st: false,
            price: 9.0,
            pe: Some(5.0),
            pb: Some(0.6),
            roe: Some(0.11),
            market_cap_billion: Some(170.0),
            dividend_yield: None,
            latest_eps: None,
            deducted_net_profit_billion: None,
            deducted_net_profit_margin: None,
            deducted_net_profit_growth_rate: None,
            change_pct: None,
            volume: None,
            amount: None,
            turnover_rate: None,
            volume_ratio: None,
            quote_time: None,
        },
        StockItem {
            code: "600036.SH".to_string(),
            name: "招商银行".to_string(),
            industry: "银行".to_string(),
            is_st: false,
            price: 31.0,
            pe: Some(7.0),
            pb: Some(0.9),
            roe: Some(0.14),
            market_cap_billion: Some(900.0),
            dividend_yield: None,
            latest_eps: None,
            deducted_net_profit_billion: None,
            deducted_net_profit_margin: None,
            deducted_net_profit_growth_rate: None,
            change_pct: None,
            volume: None,
            amount: None,
            turnover_rate: None,
            volume_ratio: None,
            quote_time: None,
        },
    ];

    let ascending = screen_stocks(
        &universe,
        &ScreenCriteria {
            sort_by: "pe".to_string(),
            sort_dir: "asc".to_string(),
            ..ScreenCriteria::default()
        },
    );
    let descending = screen_stocks(
        &universe,
        &ScreenCriteria {
            sort_by: "pe".to_string(),
            sort_dir: "desc".to_string(),
            ..ScreenCriteria::default()
        },
    );

    assert_eq!(
        ascending
            .items
            .iter()
            .map(|item| item.stock.code.as_str())
            .collect::<Vec<_>>(),
        vec!["600000.SH", "600036.SH", "000001.SZ"]
    );
    assert_eq!(
        descending
            .items
            .iter()
            .map(|item| item.stock.code.as_str())
            .collect::<Vec<_>>(),
        vec!["600036.SH", "600000.SH", "000001.SZ"]
    );
}

#[test]
fn ui_metric_sorts_use_requested_fields_without_diversification() {
    let mut highest_cap = custom_test_stock(
        "000001.SZ",
        "highest-cap",
        "same-industry",
        500.0,
        20.0,
        0.08,
    );
    highest_cap.change_pct = Some(0.01);
    let mut highest_roe = custom_test_stock(
        "000002.SZ",
        "highest-roe",
        "same-industry",
        400.0,
        20.0,
        0.20,
    );
    highest_roe.change_pct = Some(-0.02);
    let mut highest_change = custom_test_stock(
        "000003.SZ",
        "highest-change",
        "same-industry",
        300.0,
        20.0,
        0.12,
    );
    highest_change.change_pct = Some(0.05);
    let mut other_industry = custom_test_stock(
        "000004.SZ",
        "other-industry",
        "other-industry",
        50.0,
        20.0,
        0.05,
    );
    other_industry.change_pct = Some(0.0);
    let universe = vec![highest_cap, highest_roe, highest_change, other_industry];

    let market_cap = screen_stocks(
        &universe,
        &ScreenCriteria {
            limit: 3,
            sort_by: "market_cap".to_string(),
            sort_dir: "desc".to_string(),
            ..ScreenCriteria::default()
        },
    );
    assert_eq!(
        market_cap
            .items
            .iter()
            .map(|item| item.stock.code.as_str())
            .collect::<Vec<_>>(),
        vec!["000001.SZ", "000002.SZ", "000003.SZ"]
    );

    let roe = screen_stocks(
        &universe,
        &ScreenCriteria {
            limit: 4,
            sort_by: "roe".to_string(),
            sort_dir: "desc".to_string(),
            ..ScreenCriteria::default()
        },
    );
    assert_eq!(roe.items[0].stock.code, "000002.SZ");

    let change = screen_stocks(
        &universe,
        &ScreenCriteria {
            limit: 4,
            sort_by: "change_pct".to_string(),
            sort_dir: "desc".to_string(),
            ..ScreenCriteria::default()
        },
    );
    assert_eq!(change.items[0].stock.code, "000003.SZ");
}

#[test]
fn rotation_score_profile_uses_intraday_market_heat() {
    let base = StockItem {
        code: "300001.SZ".to_string(),
        name: "quality-one".to_string(),
        industry: "consumer".to_string(),
        price: 10.0,
        pe: Some(10.0),
        pb: Some(1.0),
        roe: Some(0.12),
        market_cap_billion: Some(120.0),
        ..StockItem::default()
    };
    let mut quiet = base.clone();
    quiet.code = "300001.SZ".to_string();
    quiet.change_pct = Some(0.001);
    quiet.volume_ratio = Some(0.8);
    quiet.turnover_rate = Some(0.005);
    quiet.amount = Some(80_000_000.0);

    let mut active = base.clone();
    active.code = "300002.SZ".to_string();
    active.change_pct = Some(0.075);
    active.volume_ratio = Some(2.6);
    active.turnover_rate = Some(0.07);
    active.amount = Some(1_500_000_000.0);

    let rotation = screen_stocks(
        &[quiet.clone(), active.clone()],
        &ScreenCriteria {
            sort_by: "score".to_string(),
            sort_dir: "desc".to_string(),
            score_profile: "rotation".to_string(),
            ..ScreenCriteria::default()
        },
    );
    let quality = screen_stocks(
        &[quiet, active],
        &ScreenCriteria {
            sort_by: "score".to_string(),
            sort_dir: "desc".to_string(),
            score_profile: "quality".to_string(),
            ..ScreenCriteria::default()
        },
    );

    assert_eq!(rotation.items[0].stock.code, "300002.SZ");
    assert!(rotation.items[0].factor_scores.contains_key("market_heat"));
    assert_eq!(quality.items[0].stock.code, "300001.SZ");
    assert!(!quality.items[0].factor_scores.contains_key("market_heat"));
}

#[test]
fn quality_profile_uses_quality_ranking_without_hot_sector_promotion() {
    let bank = StockItem {
        code: "000001.SZ".to_string(),
        name: "高质量银行".to_string(),
        industry: "银行".to_string(),
        price: 10.0,
        pe: Some(8.0),
        pb: Some(1.0),
        roe: Some(0.25),
        market_cap_billion: Some(300.0),
        dividend_yield: Some(0.03),
        deducted_net_profit_billion: Some(20.0),
        deducted_net_profit_growth_rate: Some(0.15),
        ..StockItem::default()
    };
    let mut chip = bank.clone();
    chip.code = "688001.SH".to_string();
    chip.name = "弱基本面芯片".to_string();
    chip.industry = "半导体".to_string();
    chip.pe = Some(60.0);
    chip.pb = Some(8.0);
    chip.roe = Some(0.03);
    chip.dividend_yield = None;
    chip.deducted_net_profit_billion = Some(-1.0);
    chip.deducted_net_profit_growth_rate = Some(-0.20);

    let result = screen_stocks(
        &[chip, bank],
        &ScreenCriteria {
            sort_by: "score".to_string(),
            sort_dir: "desc".to_string(),
            score_profile: "quality".to_string(),
            limit: 1,
            ..ScreenCriteria::default()
        },
    );

    assert_eq!(result.items[0].stock.code, "000001.SZ");
    assert_eq!(result.items[0].risk_score, SCREEN_SCORE_SCALE);
    assert!(result.items[0]
        .score_breakdown
        .iter()
        .all(|factor| factor.key != "theme"));
    assert!(result.items[0]
        .risk_tags
        .iter()
        .all(|tag| !tag.contains("低热度板块")));
}

#[test]
fn quality_profile_penalizes_incomplete_core_financial_data() {
    let complete = custom_test_stock("000002.SZ", "complete", "consumer", 120.0, 12.0, 0.18);
    let mut incomplete = complete.clone();
    incomplete.code = "000001.SZ".to_string();
    incomplete.market_cap_billion = None;
    incomplete.dividend_yield = None;
    incomplete.deducted_net_profit_billion = None;
    incomplete.deducted_net_profit_growth_rate = None;

    assert_eq!(data_quality_score(&complete), 1.0);
    assert_eq!(data_quality_score(&incomplete), 0.65);

    let result = screen_stocks(
        &[incomplete, complete],
        &ScreenCriteria {
            sort_by: "score".to_string(),
            sort_dir: "desc".to_string(),
            score_profile: "quality".to_string(),
            limit: 2,
            ..ScreenCriteria::default()
        },
    );

    assert_eq!(result.items[0].stock.code, "000002.SZ");
    assert_eq!(result.items[0].factor_scores["data_quality"], 1.0);
    assert_eq!(result.items[1].factor_scores["data_quality"], 0.65);
    assert!(result.items[0]
        .score_breakdown
        .iter()
        .any(|factor| factor.key == "data_quality"));
}

#[test]
fn backtests_with_native_history() {
    let result = backtest_with_data(
        &sample_data_set(),
        &BacktestRequest {
            criteria: ScreenCriteria {
                max_pe: Some(10.0),
                ..ScreenCriteria::default()
            },
            source: default_backtest_source(),
            strategy_mode: "candidate_snapshot".to_string(),
            stock_codes: Vec::new(),
            start_date: "20200101".to_string(),
            end_date: "20200103".to_string(),
            top_n: 1,
            initial_cash: 1000.0,
            rebalance_frequency: default_rebalance_frequency(),
            transaction_cost_bps: default_transaction_cost_bps(),
            benchmark: default_backtest_benchmark(),
        },
    )
    .expect("native data backtest should run");
    assert_eq!(result.equity_curve.len(), 3);
    assert!((result.equity_curve.last().unwrap().equity - 1198.8).abs() < 1e-6);
    assert!((result.metrics.total_return - 0.1988).abs() < 1e-6);
    assert_eq!(result.metrics.strategy_mode, "candidate_snapshot");
    assert_eq!(result.strategy_mode, "candidate_snapshot");
    assert_eq!(result.metrics.rebalance_count, 1);
    assert_eq!(result.volatility_snapshots.len(), 1);
    assert_eq!(result.volatility_snapshots[0].symbol, "111111.SZ");
    assert!(result.volatility_snapshots[0].atr.is_none());
}

#[test]
fn candidate_snapshot_rejects_selected_stocks_without_history() {
    let mut data = sample_data_set();
    data.histories.clear();
    let error = backtest_with_data(
        &data,
        &BacktestRequest {
            criteria: ScreenCriteria {
                max_pe: Some(10.0),
                ..ScreenCriteria::default()
            },
            source: default_backtest_source(),
            strategy_mode: "candidate_snapshot".to_string(),
            stock_codes: Vec::new(),
            start_date: "20200101".to_string(),
            end_date: "20200103".to_string(),
            top_n: 1,
            initial_cash: 1000.0,
            rebalance_frequency: default_rebalance_frequency(),
            transaction_cost_bps: default_transaction_cost_bps(),
            benchmark: default_backtest_benchmark(),
        },
    )
    .expect_err("selected stocks without history must not produce a zero-value result");

    assert!(error.to_string().contains("历史日线"));
}

#[test]
fn backtests_watchlist_codes_in_saved_order() {
    let result = backtest_with_data(
        &sample_data_set(),
        &BacktestRequest {
            criteria: ScreenCriteria::default(),
            source: "watchlist".to_string(),
            strategy_mode: "candidate_snapshot".to_string(),
            stock_codes: vec!["222222.SZ".to_string(), "111111.SZ".to_string()],
            start_date: "20200101".to_string(),
            end_date: "20200103".to_string(),
            top_n: 10,
            initial_cash: 1000.0,
            rebalance_frequency: default_rebalance_frequency(),
            transaction_cost_bps: default_transaction_cost_bps(),
            benchmark: default_backtest_benchmark(),
        },
    )
    .expect("watchlist backtest should run");

    assert_eq!(result.symbols, vec!["111111.SZ"]);
    assert!(result.notes.iter().any(|note| note.contains("自选观察池")));
}

#[test]
fn walk_forward_requires_point_in_time_factor_snapshots() {
    let error = backtest_with_data(
        &sample_data_set(),
        &BacktestRequest {
            criteria: ScreenCriteria {
                max_pe: Some(10.0),
                ..ScreenCriteria::default()
            },
            source: default_backtest_source(),
            strategy_mode: "walk_forward".to_string(),
            stock_codes: Vec::new(),
            start_date: "20200101".to_string(),
            end_date: "20200103".to_string(),
            top_n: 1,
            initial_cash: 1000.0,
            rebalance_frequency: default_rebalance_frequency(),
            transaction_cost_bps: default_transaction_cost_bps(),
            benchmark: default_backtest_benchmark(),
        },
    )
    .expect_err("criteria walk-forward must reject missing factor snapshots");

    assert!(error.to_string().contains("历史因子快照"));
}

#[test]
fn walk_forward_rejects_partial_factor_snapshot_coverage() {
    let mut data = sample_data_set();
    let second_history = data
        .histories
        .get("111111.SZ")
        .cloned()
        .expect("sample history");
    data.histories
        .insert("222222.SZ".to_string(), second_history);
    data.factor_snapshots.insert(
        "111111.SZ".to_string(),
        vec![pit_factor_snapshot("2020-01-01", Some(5.0))],
    );

    let error = backtest_with_data(
        &data,
        &BacktestRequest {
            criteria: ScreenCriteria::default(),
            source: default_backtest_source(),
            strategy_mode: "walk_forward".to_string(),
            stock_codes: Vec::new(),
            start_date: "20200101".to_string(),
            end_date: "20200103".to_string(),
            top_n: 1,
            initial_cash: 1000.0,
            rebalance_frequency: default_rebalance_frequency(),
            transaction_cost_bps: 0.0,
            benchmark: "none".to_string(),
        },
    )
    .expect_err("strict walk-forward must reject partial PIT coverage");

    assert!(error.to_string().contains("222222.SZ"));
}

#[test]
fn walk_forward_rejects_partial_history_coverage() {
    let mut data = sample_data_set();
    data.factor_snapshots = HashMap::from([
        (
            "111111.SZ".to_string(),
            vec![pit_factor_snapshot("2020-01-01", Some(5.0))],
        ),
        (
            "222222.SZ".to_string(),
            vec![pit_factor_snapshot("2020-01-01", Some(8.0))],
        ),
    ]);

    let error = backtest_with_data(
        &data,
        &BacktestRequest {
            criteria: ScreenCriteria::default(),
            source: default_backtest_source(),
            strategy_mode: "walk_forward".to_string(),
            stock_codes: Vec::new(),
            start_date: "20200101".to_string(),
            end_date: "20200103".to_string(),
            top_n: 1,
            initial_cash: 1000.0,
            rebalance_frequency: default_rebalance_frequency(),
            transaction_cost_bps: 0.0,
            benchmark: "none".to_string(),
        },
    )
    .expect_err("strict walk-forward must reject partial history coverage");

    assert!(error.to_string().contains("222222.SZ"));
    assert!(error.to_string().contains("历史行情"));
}

#[test]
fn walk_forward_watchlist_requires_point_in_time_status() {
    let error = backtest_with_data(
        &sample_data_set(),
        &BacktestRequest {
            criteria: ScreenCriteria::default(),
            source: "watchlist".to_string(),
            strategy_mode: "walk_forward".to_string(),
            stock_codes: vec!["111111.SZ".to_string()],
            start_date: "20200101".to_string(),
            end_date: "20200103".to_string(),
            top_n: 1,
            initial_cash: 1000.0,
            rebalance_frequency: default_rebalance_frequency(),
            transaction_cost_bps: 0.0,
            benchmark: "none".to_string(),
        },
    )
    .expect_err("walk-forward watchlist must require historical status snapshots");

    assert!(error.to_string().contains("111111.SZ"));
}

#[test]
fn walk_forward_watchlist_respects_historical_st_status() {
    let mut data = sample_data_set();
    let mut st_snapshot = pit_factor_snapshot("2020-01-01", Some(5.0));
    st_snapshot.is_st = Some(true);
    data.factor_snapshots
        .insert("111111.SZ".to_string(), vec![st_snapshot]);

    let result = backtest_with_data(
        &data,
        &BacktestRequest {
            criteria: ScreenCriteria {
                include_st: false,
                ..ScreenCriteria::default()
            },
            source: "watchlist".to_string(),
            strategy_mode: "walk_forward".to_string(),
            stock_codes: vec!["111111.SZ".to_string()],
            start_date: "20200101".to_string(),
            end_date: "20200103".to_string(),
            top_n: 1,
            initial_cash: 1000.0,
            rebalance_frequency: default_rebalance_frequency(),
            transaction_cost_bps: 0.0,
            benchmark: "none".to_string(),
        },
    )
    .expect("historical ST watchlist stock should be excluded without breaking the fold");

    assert!(result.symbols.is_empty());
    assert!((result.equity_curve.last().unwrap().equity - 1000.0).abs() < 1e-6);
}

#[test]
fn walk_forward_without_history_fails_instead_of_skipping_pit_validation() {
    let mut data = sample_data_set();
    data.histories.clear();
    data.factor_snapshots = HashMap::from([
        (
            "111111.SZ".to_string(),
            vec![pit_factor_snapshot("2020-01-01", Some(5.0))],
        ),
        (
            "222222.SZ".to_string(),
            vec![pit_factor_snapshot("2020-01-01", Some(8.0))],
        ),
    ]);

    let error = backtest_with_data(
        &data,
        &BacktestRequest {
            criteria: ScreenCriteria::default(),
            source: default_backtest_source(),
            strategy_mode: "walk_forward".to_string(),
            stock_codes: Vec::new(),
            start_date: "20200101".to_string(),
            end_date: "20200103".to_string(),
            top_n: 1,
            initial_cash: 1000.0,
            rebalance_frequency: default_rebalance_frequency(),
            transaction_cost_bps: 0.0,
            benchmark: "none".to_string(),
        },
    )
    .expect_err("strict walk-forward cannot validate PIT data without history");

    assert!(error.to_string().contains("历史行情"));
}

#[test]
fn factor_snapshot_is_not_visible_before_its_public_availability_date() {
    let mut data = sample_data_set();
    data.factor_snapshots.insert(
        "111111.SZ".to_string(),
        vec![StockFactorSnapshot {
            date: "2019-12-31".to_string(),
            available_date: Some("2020-02-01".to_string()),
            is_st: Some(false),
            is_listed: Some(true),
            is_tradable: Some(true),
            pe: Some(5.0),
            ..StockFactorSnapshot::default()
        }],
    );
    let source = StaticDataSource::new(&data);
    let timeline =
        load_factor_snapshot_timeline(&source, "111111.SZ").expect("valid snapshot timeline");

    assert!(latest_factor_snapshot_on_or_before(
        &HashMap::from([("111111.SZ".to_string(), timeline.clone())]),
        "111111.SZ",
        NaiveDate::from_ymd_opt(2020, 1, 31).unwrap(),
    )
    .is_none());
    assert!(latest_factor_snapshot_on_or_before(
        &HashMap::from([("111111.SZ".to_string(), timeline)]),
        "111111.SZ",
        NaiveDate::from_ymd_opt(2020, 2, 1).unwrap(),
    )
    .is_some());
}

#[test]
fn factor_snapshot_rejects_availability_before_report_date() {
    let mut data = sample_data_set();
    data.factor_snapshots.insert(
        "111111.SZ".to_string(),
        vec![StockFactorSnapshot {
            date: "2020-03-31".to_string(),
            available_date: Some("2020-03-01".to_string()),
            is_st: Some(false),
            is_listed: Some(true),
            is_tradable: Some(true),
            ..StockFactorSnapshot::default()
        }],
    );
    let source = StaticDataSource::new(&data);

    let error = load_factor_snapshot_timeline(&source, "111111.SZ")
        .expect_err("availability cannot precede the report date");

    assert!(error.to_string().contains("早于报告期"));
}

#[test]
fn walk_forward_fold_return_requires_an_exact_end_date_price() {
    let history = BacktestHistory {
        code: "111111.SZ".to_string(),
        prices: BTreeMap::from([
            (NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(), 10.0),
            (NaiveDate::from_ymd_opt(2020, 3, 1).unwrap(), 5.0),
        ]),
        bars: Vec::new(),
    };

    assert_eq!(
        forward_return(
            &history,
            NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
            NaiveDate::from_ymd_opt(2020, 2, 1).unwrap(),
        ),
        None
    );
}

#[test]
fn walk_forward_uses_point_in_time_factor_snapshots() {
    let mut data = sample_data_set();
    data.histories.insert(
        "222222.SZ".to_string(),
        vec![
            HistoryBar {
                date: "2020-01-01".to_string(),
                open: Some(20.0),
                high: Some(20.5),
                low: Some(19.8),
                close: 20.0,
                volume: Some(1_000_000.0),
                capital: Some(1_000_000_000.0),
            },
            HistoryBar {
                date: "2020-02-01".to_string(),
                open: Some(20.0),
                high: Some(20.5),
                low: Some(19.5),
                close: 20.0,
                volume: Some(1_000_000.0),
                capital: Some(1_000_000_000.0),
            },
            HistoryBar {
                date: "2020-03-01".to_string(),
                open: Some(18.0),
                high: Some(18.5),
                low: Some(17.5),
                close: 18.0,
                volume: Some(1_000_000.0),
                capital: Some(1_000_000_000.0),
            },
        ],
    );
    data.histories.insert(
        "111111.SZ".to_string(),
        vec![
            HistoryBar {
                date: "2020-01-01".to_string(),
                open: Some(10.0),
                high: Some(10.5),
                low: Some(9.8),
                close: 10.0,
                volume: Some(1_000_000.0),
                capital: Some(1_000_000_000.0),
            },
            HistoryBar {
                date: "2020-02-01".to_string(),
                open: Some(15.0),
                high: Some(15.5),
                low: Some(14.8),
                close: 15.0,
                volume: Some(1_000_000.0),
                capital: Some(1_000_000_000.0),
            },
            HistoryBar {
                date: "2020-03-01".to_string(),
                open: Some(20.0),
                high: Some(20.5),
                low: Some(19.8),
                close: 20.0,
                volume: Some(1_000_000.0),
                capital: Some(1_000_000_000.0),
            },
        ],
    );
    data.factor_snapshots = HashMap::from([
        (
            "111111.SZ".to_string(),
            vec![
                StockFactorSnapshot {
                    date: "2020-01-01".to_string(),
                    available_date: Some("2020-01-01".to_string()),
                    is_st: Some(false),
                    is_listed: Some(true),
                    is_tradable: Some(true),
                    pe: Some(5.0),
                    ..StockFactorSnapshot::default()
                },
                StockFactorSnapshot {
                    date: "2020-02-01".to_string(),
                    available_date: Some("2020-02-01".to_string()),
                    is_st: Some(false),
                    is_listed: Some(true),
                    is_tradable: Some(true),
                    pe: Some(50.0),
                    ..StockFactorSnapshot::default()
                },
            ],
        ),
        (
            "222222.SZ".to_string(),
            vec![
                StockFactorSnapshot {
                    date: "2020-01-01".to_string(),
                    available_date: Some("2020-01-01".to_string()),
                    is_st: Some(false),
                    is_listed: Some(true),
                    is_tradable: Some(true),
                    pe: Some(50.0),
                    ..StockFactorSnapshot::default()
                },
                StockFactorSnapshot {
                    date: "2020-02-01".to_string(),
                    available_date: Some("2020-02-01".to_string()),
                    is_st: Some(false),
                    is_listed: Some(true),
                    is_tradable: Some(true),
                    pe: Some(5.0),
                    ..StockFactorSnapshot::default()
                },
            ],
        ),
    ]);
    data.stocks.retain(|stock| stock.code != "222222.SZ");

    let result = backtest_with_data(
        &data,
        &BacktestRequest {
            criteria: ScreenCriteria {
                max_pe: Some(10.0),
                ..ScreenCriteria::default()
            },
            source: default_backtest_source(),
            strategy_mode: "walk_forward".to_string(),
            stock_codes: Vec::new(),
            start_date: "20200101".to_string(),
            end_date: "20200301".to_string(),
            top_n: 1,
            initial_cash: 1000.0,
            rebalance_frequency: "monthly".to_string(),
            transaction_cost_bps: 0.0,
            benchmark: "none".to_string(),
        },
    )
    .expect("walk-forward should use factor snapshots");

    assert_eq!(result.strategy_mode, "walk_forward");
    assert_eq!(result.rebalance_dates, vec!["2020-02-01", "2020-03-01"]);
    assert_eq!(result.symbols, vec!["111111.SZ", "222222.SZ"]);
    assert!((result.equity_curve.last().unwrap().equity - 1333.3333333333333).abs() < 1e-6);
    assert_eq!(result.metrics.oos_fold_count, 1);
    assert_eq!(result.metrics.evaluated_selection_count, 1);
    assert_eq!(result.metrics.selection_hit_count, 1);
    assert_eq!(result.metrics.precision_at_n, Some(1.0));
    assert_eq!(result.walk_forward_folds.len(), 2);
}

#[test]
fn walk_forward_rejects_snapshot_without_public_availability_date() {
    let mut data = sample_data_set();
    data.factor_snapshots.insert(
        "111111.SZ".to_string(),
        vec![StockFactorSnapshot {
            date: "2019-12-31".to_string(),
            available_date: None,
            is_st: Some(false),
            is_listed: Some(true),
            is_tradable: Some(true),
            pe: Some(5.0),
            ..StockFactorSnapshot::default()
        }],
    );

    let error = backtest_with_data(
        &data,
        &BacktestRequest {
            criteria: ScreenCriteria::default(),
            source: default_backtest_source(),
            strategy_mode: "walk_forward".to_string(),
            stock_codes: Vec::new(),
            start_date: "20200101".to_string(),
            end_date: "20200103".to_string(),
            top_n: 1,
            initial_cash: 1000.0,
            rebalance_frequency: "monthly".to_string(),
            transaction_cost_bps: 0.0,
            benchmark: "none".to_string(),
        },
    )
    .expect_err("strict PIT backtest must require a public availability date");

    assert!(error.to_string().contains("available_date"));
}

#[test]
fn walk_forward_does_not_fill_missing_snapshot_fields_from_current_data() {
    let mut data = sample_data_set();
    data.stocks.retain(|stock| stock.code == "111111.SZ");
    data.factor_snapshots.insert(
        "111111.SZ".to_string(),
        vec![StockFactorSnapshot {
            date: "2020-01-01".to_string(),
            available_date: Some("2020-01-01".to_string()),
            is_st: Some(false),
            is_listed: Some(true),
            is_tradable: Some(true),
            pe: None,
            ..StockFactorSnapshot::default()
        }],
    );

    let result = backtest_with_data(
        &data,
        &BacktestRequest {
            criteria: ScreenCriteria {
                max_pe: Some(10.0),
                ..ScreenCriteria::default()
            },
            source: default_backtest_source(),
            strategy_mode: "walk_forward".to_string(),
            stock_codes: Vec::new(),
            start_date: "20200101".to_string(),
            end_date: "20200103".to_string(),
            top_n: 1,
            initial_cash: 1000.0,
            rebalance_frequency: "monthly".to_string(),
            transaction_cost_bps: 0.0,
            benchmark: "none".to_string(),
        },
    )
    .expect("strict walk-forward should hold cash when factors are missing");

    assert_eq!(result.rebalance_dates, vec!["2020-01-02"]);
    assert!((result.equity_curve.last().unwrap().equity - 1000.0).abs() < 1e-6);
    assert!(result.volatility_snapshots.is_empty());
    assert_eq!(
        result.volatility_message.as_deref(),
        Some("Walk-forward 末次调仓没有符合条件的标的。")
    );
}

#[test]
fn walk_forward_moves_to_cash_when_no_stock_matches_at_rebalance() {
    let mut data = sample_data_set();
    data.stocks.retain(|stock| stock.code == "111111.SZ");
    data.histories.clear();
    data.histories.insert(
        "111111.SZ".to_string(),
        vec![
            HistoryBar {
                date: "2020-01-01".to_string(),
                open: None,
                high: None,
                low: None,
                close: 10.0,
                volume: None,
                capital: None,
            },
            HistoryBar {
                date: "2020-02-01".to_string(),
                open: None,
                high: None,
                low: None,
                close: 20.0,
                volume: None,
                capital: None,
            },
            HistoryBar {
                date: "2020-03-01".to_string(),
                open: None,
                high: None,
                low: None,
                close: 10.0,
                volume: None,
                capital: None,
            },
        ],
    );
    data.factor_snapshots.insert(
        "111111.SZ".to_string(),
        vec![
            StockFactorSnapshot {
                date: "2020-01-01".to_string(),
                available_date: Some("2020-01-01".to_string()),
                is_st: Some(false),
                is_listed: Some(true),
                is_tradable: Some(true),
                pe: Some(5.0),
                ..StockFactorSnapshot::default()
            },
            StockFactorSnapshot {
                date: "2020-02-01".to_string(),
                available_date: Some("2020-02-01".to_string()),
                is_st: Some(false),
                is_listed: Some(true),
                is_tradable: Some(true),
                pe: Some(50.0),
                ..StockFactorSnapshot::default()
            },
        ],
    );

    let result = backtest_with_data(
        &data,
        &BacktestRequest {
            criteria: ScreenCriteria {
                max_pe: Some(10.0),
                ..ScreenCriteria::default()
            },
            source: default_backtest_source(),
            strategy_mode: "walk_forward".to_string(),
            stock_codes: Vec::new(),
            start_date: "20200101".to_string(),
            end_date: "20200301".to_string(),
            top_n: 1,
            initial_cash: 1000.0,
            rebalance_frequency: "monthly".to_string(),
            transaction_cost_bps: 0.0,
            benchmark: "none".to_string(),
        },
    )
    .expect("walk-forward should rebalance an empty selection to cash");

    assert_eq!(result.rebalance_dates, vec!["2020-02-01", "2020-03-01"]);
    assert!((result.equity_curve.last().unwrap().equity - 500.0).abs() < 1e-6);
}

#[test]
fn walk_forward_does_not_trade_a_suspended_holding_at_a_stale_price() {
    let mut data = sample_data_set();
    data.histories = HashMap::from([
        (
            "111111.SZ".to_string(),
            vec![
                HistoryBar {
                    date: "2020-01-01".to_string(),
                    open: None,
                    high: None,
                    low: None,
                    close: 10.0,
                    volume: None,
                    capital: None,
                },
                HistoryBar {
                    date: "2020-03-01".to_string(),
                    open: None,
                    high: None,
                    low: None,
                    close: 5.0,
                    volume: None,
                    capital: None,
                },
            ],
        ),
        (
            "222222.SZ".to_string(),
            vec![
                HistoryBar {
                    date: "2020-01-01".to_string(),
                    open: None,
                    high: None,
                    low: None,
                    close: 20.0,
                    volume: None,
                    capital: None,
                },
                HistoryBar {
                    date: "2020-02-01".to_string(),
                    open: None,
                    high: None,
                    low: None,
                    close: 20.0,
                    volume: None,
                    capital: None,
                },
                HistoryBar {
                    date: "2020-03-01".to_string(),
                    open: None,
                    high: None,
                    low: None,
                    close: 20.0,
                    volume: None,
                    capital: None,
                },
            ],
        ),
    ]);
    let mut suspended = pit_factor_snapshot("2020-02-01", Some(50.0));
    suspended.is_tradable = Some(false);
    data.factor_snapshots = HashMap::from([
        (
            "111111.SZ".to_string(),
            vec![pit_factor_snapshot("2020-01-01", Some(5.0)), suspended],
        ),
        (
            "222222.SZ".to_string(),
            vec![
                pit_factor_snapshot("2020-01-01", Some(50.0)),
                pit_factor_snapshot("2020-02-01", Some(5.0)),
            ],
        ),
    ]);

    let result = backtest_with_data(
        &data,
        &BacktestRequest {
            criteria: ScreenCriteria {
                max_pe: Some(10.0),
                ..ScreenCriteria::default()
            },
            source: default_backtest_source(),
            strategy_mode: "walk_forward".to_string(),
            stock_codes: Vec::new(),
            start_date: "20200101".to_string(),
            end_date: "20200301".to_string(),
            top_n: 1,
            initial_cash: 1000.0,
            rebalance_frequency: "monthly".to_string(),
            transaction_cost_bps: 0.0,
            benchmark: "none".to_string(),
        },
    )
    .expect("suspended holdings should remain locked until an exact trade price exists");

    assert!((result.equity_curve.last().unwrap().equity - 1000.0).abs() < 1e-6);
    assert_eq!(result.symbols, vec!["111111.SZ", "222222.SZ"]);
    assert_eq!(result.volatility_snapshots.len(), 1);
    assert_eq!(result.volatility_snapshots[0].symbol, "222222.SZ");
    assert_eq!(result.walk_forward_folds[0].evaluated_selection_count, 1);
    assert_eq!(result.walk_forward_folds[0].hit_count, 0);
    assert_eq!(result.walk_forward_folds[0].precision_at_n, Some(0.0));
}

#[test]
fn screen_stocks_empty_universe_returns_empty_without_panic() {
    let result = screen_stocks(&[], &ScreenCriteria::default());
    assert_eq!(result.returned, 0);
    assert_eq!(result.total, 0);
    assert!(result.items.is_empty());
}

#[test]
fn pe_and_pb_scores_handle_nonpositive_and_nonfinite() {
    // Non-positive PE/PB are penalized via match guards, never used as a divisor.
    assert_eq!(pe_score(Some(0.0)), 0.25);
    assert_eq!(pe_score(Some(-12.0)), 0.25);
    assert_eq!(pb_score(Some(0.0)), 0.25);
    assert_eq!(pb_score(Some(-3.0)), 0.25);
    // Missing metrics fall back to a neutral score.
    assert_eq!(pe_score(None), 0.52);
    assert_eq!(pb_score(None), 0.52);
    // Non-finite values use the same neutral fallback as missing metrics.
    assert_eq!(pe_score(Some(f64::NAN)), pe_score(None));
    assert_eq!(pb_score(Some(f64::NAN)), pb_score(None));
    assert_eq!(pe_score(Some(f64::INFINITY)), pe_score(None));
    assert_eq!(pb_score(Some(f64::INFINITY)), pb_score(None));
}

#[test]
fn fundamental_score_with_nonfinite_inputs_stays_bounded() {
    let stock = StockItem {
        code: "000001.SZ".to_string(),
        roe: Some(f64::NAN),
        dividend_yield: Some(f64::INFINITY),
        deducted_net_profit_billion: Some(f64::NAN),
        deducted_net_profit_growth_rate: Some(f64::NAN),
        ..StockItem::default()
    };
    let score = fundamental_score(&stock);
    assert!(score.is_finite());
    assert!((0.0..=1.0).contains(&score));
}

#[test]
fn score_stock_with_degenerate_metrics_stays_finite_and_bounded() {
    let stock = StockItem {
        code: "000002.SZ".to_string(),
        name: "degenerate".to_string(),
        industry: "银行".to_string(),
        price: 0.0,
        pe: Some(0.0),
        pb: Some(0.0),
        roe: Some(f64::NAN),
        market_cap_billion: Some(0.0),
        dividend_yield: Some(f64::INFINITY),
        ..StockItem::default()
    };
    let scored = score_stock(&stock, &[], "balanced");
    assert!(scored.score.is_finite(), "composite score must be finite");
    assert!((0.0..=SCREEN_SCORE_SCALE).contains(&scored.score));
    for (key, value) in &scored.factor_scores {
        assert!(value.is_finite(), "factor score {key} must be finite");
    }
}

#[test]
fn hard_filters_reject_invalid_quotes_and_valuation_metrics() {
    let valid = custom_test_stock("000001.SZ", "valid", "bank", 100.0, 10.0, 0.15);
    let mut zero_price = valid.clone();
    zero_price.code = "000002.SZ".to_string();
    zero_price.price = 0.0;
    let mut negative_pe = valid.clone();
    negative_pe.code = "000003.SZ".to_string();
    negative_pe.pe = Some(-10.0);
    let mut nonfinite_pb = valid.clone();
    nonfinite_pb.code = "000004.SZ".to_string();
    nonfinite_pb.pb = Some(f64::NAN);

    let result = screen_stocks(
        &[valid, zero_price, negative_pe, nonfinite_pb],
        &ScreenCriteria {
            max_pe: Some(20.0),
            max_pb: Some(5.0),
            ..ScreenCriteria::default()
        },
    );

    assert_eq!(result.returned, 1);
    assert_eq!(result.items[0].stock.code, "000001.SZ");
}

#[test]
fn industry_percentiles_are_tie_aware_and_shrink_small_groups() {
    let tied_a = custom_test_stock("000001.SZ", "tied-a", "small-group", 100.0, 20.0, 0.10);
    let tied_b = custom_test_stock("000002.SZ", "tied-b", "small-group", 100.0, 20.0, 0.10);
    let high = custom_test_stock("000003.SZ", "high", "small-group", 100.0, 20.0, 0.20);
    let singleton = custom_test_stock("000004.SZ", "singleton", "single-group", 100.0, 20.0, 0.15);
    let result = screen_stocks(
        &[tied_a, tied_b, high, singleton],
        &ScreenCriteria {
            score_profile: "quality".to_string(),
            ..ScreenCriteria::default()
        },
    );
    let by_code = result
        .items
        .iter()
        .map(|item| (item.stock.code.as_str(), item))
        .collect::<HashMap<_, _>>();

    let tied_a_pct = by_code["000001.SZ"].factor_scores["industry_quality_pct"];
    let tied_b_pct = by_code["000002.SZ"].factor_scores["industry_quality_pct"];
    let high_pct = by_code["000003.SZ"].factor_scores["industry_quality_pct"];
    let singleton_pct = by_code["000004.SZ"].factor_scores["industry_quality_pct"];
    assert_eq!(tied_a_pct, tied_b_pct);
    assert!(high_pct > tied_a_pct);
    assert!((0.35..0.65).contains(&high_pct));
    assert_eq!(singleton_pct, 0.5);
}

#[test]
fn balanced_profile_stays_bounded() {
    let factors = BTreeMap::from([
        ("theme".to_string(), 1.0),
        ("fundamental".to_string(), 1.0),
        ("valuation".to_string(), 1.0),
        ("market_heat".to_string(), 1.0),
        ("size".to_string(), 1.0),
        ("risk".to_string(), 1.0),
        ("overheat_penalty".to_string(), 0.0),
    ]);
    assert!((profile_score(&factors, "balanced") - 1.0).abs() < f64::EPSILON);
}

#[test]
fn balanced_profile_preserves_legacy_theme_weight() {
    let factors = BTreeMap::from([
        ("theme".to_string(), 1.0),
        ("fundamental".to_string(), 0.0),
        ("valuation".to_string(), 0.0),
        ("market_heat".to_string(), 0.0),
        ("size".to_string(), 0.0),
        ("risk".to_string(), 0.0),
        ("overheat_penalty".to_string(), 0.0),
    ]);
    assert!((profile_score(&factors, "balanced") - 0.16).abs() < f64::EPSILON);
}

#[test]
fn screen_stocks_with_tied_scores_is_deterministic() {
    let make = |code: &str| StockItem {
        code: code.to_string(),
        name: code.to_string(),
        industry: "银行".to_string(),
        price: 10.0,
        pe: Some(8.0),
        pb: Some(1.0),
        roe: Some(0.15),
        market_cap_billion: Some(120.0),
        ..StockItem::default()
    };
    let universe = vec![make("000001.SZ"), make("000002.SZ"), make("000003.SZ")];
    let criteria = ScreenCriteria {
        sort_by: "score".to_string(),
        sort_dir: "desc".to_string(),
        ..ScreenCriteria::default()
    };
    let first = screen_stocks(&universe, &criteria);
    let second = screen_stocks(&universe, &criteria);
    let codes_first: Vec<_> = first
        .items
        .iter()
        .map(|item| item.stock.code.clone())
        .collect();
    let codes_second: Vec<_> = second
        .items
        .iter()
        .map(|item| item.stock.code.clone())
        .collect();
    assert_eq!(first.returned, 3);
    assert_eq!(
        codes_first, codes_second,
        "tie-break ordering must be deterministic"
    );
}
#[test]
fn static_data_source_borrows_original_stock_slice() {
    let data = sample_data_set();
    let source = StaticDataSource::new(&data);
    let stocks = source.stocks().expect("borrow stocks");

    assert_eq!(stocks.as_ptr(), data.stocks.as_ptr());
    assert_eq!(stocks.len(), data.stocks.len());
}

#[test]
#[ignore = "manual release performance guard"]
fn benchmark_screen_stocks_8000_universe() {
    let universe = (0..8_000)
        .map(|index| StockItem {
            code: format!("{:06}.SZ", index),
            name: format!("stock-{index}"),
            industry: format!("industry-{}", index % 24),
            price: 10.0 + (index % 100) as f64,
            pe: Some(8.0 + (index % 30) as f64),
            pb: Some(0.8 + (index % 10) as f64 / 10.0),
            roe: Some(0.08 + (index % 15) as f64 / 100.0),
            market_cap_billion: Some(20.0 + (index % 500) as f64),
            change_pct: Some((index % 20) as f64 / 10.0),
            turnover_rate: Some(1.0 + (index % 50) as f64 / 10.0),
            volume_ratio: Some(0.8 + (index % 20) as f64 / 10.0),
            ..StockItem::default()
        })
        .collect::<Vec<_>>();
    let criteria = ScreenCriteria {
        limit: 200,
        sort_by: "score".to_string(),
        sort_dir: "desc".to_string(),
        ..ScreenCriteria::default()
    };
    let started = std::time::Instant::now();

    for _ in 0..10 {
        let result = screen_stocks(&universe, &criteria);
        assert_eq!(result.total, universe.len());
        std::hint::black_box(result);
    }

    let elapsed = started.elapsed();
    let average = elapsed / 10;
    eprintln!("8,000-stock screening x10: {elapsed:?}, average: {average:?}");
    assert!(
        average < std::time::Duration::from_millis(500),
        "8,000-stock screening regression: average {average:?}"
    );
}
