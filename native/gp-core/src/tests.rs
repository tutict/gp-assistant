use super::*;

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
        financials: HashMap::from([(
            "111111.SZ".to_string(),
            StockFinancialSnapshot {
                latest_eps: Some(0.42),
                latest_bps: Some(12.5),
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
                    note: Some("cached fund flow".to_string()),
                }],
                notes: vec!["cached note".to_string()],
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
fn default_screen_includes_non_st_stocks() {
    let result = screen_with_mock(&ScreenCriteria::default());
    assert_eq!(result.returned, 10);
}

#[test]
fn graph_screen_adds_relation_signals() {
    let result = graph_screen_with_mock(&GraphScreenRequest {
        criteria: ScreenCriteria {
            max_pe: Some(30.0),
            min_roe: Some(0.1),
            ..ScreenCriteria::default()
        },
        seed_codes: vec!["300750.SZ".to_string()],
        seed_query: String::new(),
        relation_depth: 1,
        relation_weight: 0.4,
        limit: 20,
    });
    assert!(result.relation_count >= 7);
    assert!(!result.items.is_empty());
    assert!(result.items.iter().any(|item| item.relation_score > 0.0));
}

#[test]
fn graph_screen_resolves_seed_query_to_robotics_supply_chain() {
    let stocks = vec![
        chain_stock(
            "002747.SZ",
            "\u{57c3}\u{65af}\u{987f}",
            "\u{5de5}\u{4e1a}\u{673a}\u{5668}\u{4eba}",
            220.0,
        ),
        chain_stock(
            "688017.SH",
            "\u{7eff}\u{7684}\u{8c10}\u{6ce2}",
            "\u{673a}\u{5668}\u{4eba}\u{51cf}\u{901f}\u{5668}",
            180.0,
        ),
        chain_stock(
            "002979.SZ",
            "\u{96f7}\u{8d5b}\u{667a}\u{80fd}",
            "\u{4f3a}\u{670d}\u{63a7}\u{5236}\u{5668}",
            120.0,
        ),
        chain_stock(
            "300750.SZ",
            "\u{5b81}\u{5fb7}\u{65f6}\u{4ee3}",
            "\u{9502}\u{7535}\u{65b0}\u{80fd}\u{6e90}",
            1800.0,
        ),
        chain_stock(
            "600519.SH",
            "\u{8d35}\u{5dde}\u{8305}\u{53f0}",
            "\u{767d}\u{9152}",
            2200.0,
        ),
    ];
    let data = CoreDataSet {
        stocks,
        relations: Vec::new(),
        histories: HashMap::new(),
        financials: HashMap::new(),
        capital_evidence: HashMap::new(),
    };

    let result = graph_screen_with_data(
        &data,
        &GraphScreenRequest {
            seed_query: "\u{6309}\u{57c3}\u{65af}\u{987f}\u{4e0a}\u{4e0b}\u{6e38}\u{9009}\u{80a1}"
                .to_string(),
            relation_depth: 2,
            relation_weight: 0.7,
            limit: 5,
            ..GraphScreenRequest::default()
        },
    )
    .expect("graph screen should run");

    assert_eq!(result.center_context.mode, "seed_stock_center");
    assert!(result
        .center_context
        .codes
        .contains(&"002747.SZ".to_string()));
    assert!(result
        .items
        .iter()
        .any(|item| item.stock.code == "688017.SH"));
    assert!(result
        .items
        .iter()
        .any(|item| item.stock.code == "300750.SZ"));
    assert!(result
        .items
        .iter()
        .any(|item| item.related.iter().any(|relation| {
            relation.relation_type == "supply_chain_upstream"
                || relation.relation_type == "supply_chain_downstream"
        })));
}

#[test]
fn graph_screen_does_not_add_robotics_chain_for_non_robot_seed_query() {
    let stocks = vec![
        chain_stock(
            "002747.SZ",
            "\u{57c3}\u{65af}\u{987f}",
            "\u{5de5}\u{4e1a}\u{673a}\u{5668}\u{4eba}",
            220.0,
        ),
        chain_stock(
            "688017.SH",
            "\u{7eff}\u{7684}\u{8c10}\u{6ce2}",
            "\u{673a}\u{5668}\u{4eba}\u{51cf}\u{901f}\u{5668}",
            180.0,
        ),
        chain_stock(
            "300750.SZ",
            "\u{5b81}\u{5fb7}\u{65f6}\u{4ee3}",
            "\u{9502}\u{7535}\u{65b0}\u{80fd}\u{6e90}",
            1800.0,
        ),
        chain_stock(
            "600519.SH",
            "\u{8d35}\u{5dde}\u{8305}\u{53f0}",
            "\u{767d}\u{9152}",
            2200.0,
        ),
    ];
    let data = CoreDataSet {
        stocks,
        relations: Vec::new(),
        histories: HashMap::new(),
        financials: HashMap::new(),
        capital_evidence: HashMap::new(),
    };

    let result = graph_screen_with_data(
        &data,
        &GraphScreenRequest {
            seed_query: "\u{8d35}\u{5dde}\u{8305}\u{53f0}\u{4e0a}\u{4e0b}\u{6e38}".to_string(),
            relation_depth: 2,
            relation_weight: 0.7,
            limit: 5,
            ..GraphScreenRequest::default()
        },
    )
    .expect("graph screen should run");

    assert_eq!(result.center_context.mode, "seed_stock_center");
    assert!(result
        .center_context
        .codes
        .contains(&"600519.SH".to_string()));
    assert!(result
        .items
        .iter()
        .all(|item| item.related.iter().all(|relation| {
            relation.relation_type != "supply_chain_upstream"
                && relation.relation_type != "supply_chain_downstream"
        })));
    assert!(result
        .notes
        .iter()
        .all(|note| { !note.contains("\u{673a}\u{5668}\u{4eba}\u{4ea7}\u{4e1a}\u{94fe}") }));
}
fn chain_stock(code: &str, name: &str, industry: &str, market_cap_billion: f64) -> StockItem {
    StockItem {
        code: code.to_string(),
        name: name.to_string(),
        industry: industry.to_string(),
        is_st: false,
        price: 20.0,
        pe: Some(20.0),
        pb: Some(3.0),
        roe: Some(0.15),
        market_cap_billion: Some(market_cap_billion),
        dividend_yield: Some(0.01),
        deducted_net_profit_billion: Some(3.0),
        deducted_net_profit_margin: Some(12.0),
        deducted_net_profit_growth_rate: Some(15.0),
        ..StockItem::default()
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
        stock_codes: Vec::new(),
        start_date: "20200101".to_string(),
        end_date: "20200110".to_string(),
        top_n: 2,
        initial_cash: 1000.0,
    })
    .expect("backtest should run");
    assert_eq!(result.metrics.num_stocks, 2);
    assert_eq!(result.equity_curve.first().unwrap().equity, 1000.0);
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
    assert!(technical_item.metrics.contains_key("波段机会"));
    assert!(!capital_evidence
        .notes
        .iter()
        .any(|note| note.contains("待迁移")));
    assert!(result.order_book.is_none());
    assert!(result.notes.iter().any(|note| note.contains("Tauri/Rust")));
}
#[test]
fn observe_with_data_accepts_numeric_capital_metric_values() {
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
fn agent_routes_graph_request() {
    let response = run_agent_with_mock("用关系图分析 300750.SZ 产业链选股").unwrap();
    assert_eq!(response.action, "graph_screen");
    assert!(response.reply.contains("不构成投资建议"));
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
fn score_sort_biases_toward_hot_energy_and_tech_sectors() {
    let base = StockItem {
        code: "000001.SZ".to_string(),
        name: "平安银行".to_string(),
        industry: "银行".to_string(),
        is_st: false,
        price: 10.0,
        pe: Some(10.0),
        pb: Some(1.0),
        roe: Some(0.1),
        market_cap_billion: Some(100.0),
        dividend_yield: None,
        deducted_net_profit_billion: None,
        deducted_net_profit_margin: None,
        deducted_net_profit_growth_rate: None,
        change_pct: None,
        volume: None,
        amount: None,
        turnover_rate: None,
        volume_ratio: None,
        quote_time: None,
    };
    let mut chip = base.clone();
    chip.code = "688001.SH".to_string();
    chip.name = "芯片公司".to_string();
    chip.industry = "半导体".to_string();
    let mut solar = base.clone();
    solar.code = "601012.SH".to_string();
    solar.name = "光伏公司".to_string();
    solar.industry = "光伏".to_string();

    let result = screen_stocks(
        &[base, chip, solar],
        &ScreenCriteria {
            sort_by: "score".to_string(),
            sort_dir: "desc".to_string(),
            ..ScreenCriteria::default()
        },
    );

    assert_eq!(
        result
            .items
            .iter()
            .take(2)
            .map(|item| item.stock.code.as_str())
            .collect::<Vec<_>>(),
        vec!["688001.SH", "601012.SH"]
    );
}

#[test]
fn score_sort_promotes_hot_tech_and_energy_candidates_into_limited_results() {
    let bank = StockItem {
        code: "000001.SZ".to_string(),
        name: "高分银行".to_string(),
        industry: "银行".to_string(),
        is_st: false,
        price: 10.0,
        pe: Some(2.0),
        pb: Some(0.2),
        roe: Some(0.3),
        market_cap_billion: Some(100.0),
        dividend_yield: None,
        deducted_net_profit_billion: None,
        deducted_net_profit_margin: None,
        deducted_net_profit_growth_rate: None,
        change_pct: None,
        volume: None,
        amount: None,
        turnover_rate: None,
        volume_ratio: None,
        quote_time: None,
    };
    let mut ordinary_bank = bank.clone();
    ordinary_bank.code = "600000.SH".to_string();
    ordinary_bank.name = "普通银行".to_string();
    ordinary_bank.pe = Some(3.0);
    ordinary_bank.pb = Some(0.3);
    ordinary_bank.roe = Some(0.2);
    let mut chip = bank.clone();
    chip.code = "688001.SH".to_string();
    chip.name = "芯片公司".to_string();
    chip.industry = "半导体".to_string();
    chip.pe = Some(60.0);
    chip.pb = Some(8.0);
    chip.roe = Some(0.03);
    let mut solar = chip.clone();
    solar.code = "601012.SH".to_string();
    solar.name = "光伏公司".to_string();
    solar.industry = "光伏".to_string();
    solar.pe = Some(50.0);
    solar.pb = Some(7.0);

    let result = screen_stocks(
        &[bank, ordinary_bank, chip, solar],
        &ScreenCriteria {
            sort_by: "score".to_string(),
            sort_dir: "desc".to_string(),
            limit: 2,
            ..ScreenCriteria::default()
        },
    );

    assert_eq!(
        result
            .items
            .iter()
            .map(|item| item.stock.code.as_str())
            .collect::<Vec<_>>(),
        vec!["688001.SH", "601012.SH"]
    );
}

#[test]
fn score_sort_promotes_duofuduo_like_hot_themes_and_deprioritizes_bank_infra() {
    let bank = StockItem {
        code: "000001.SZ".to_string(),
        name: "高分银行".to_string(),
        industry: "银行".to_string(),
        is_st: false,
        price: 10.0,
        pe: Some(2.0),
        pb: Some(0.2),
        roe: Some(0.3),
        market_cap_billion: Some(100.0),
        dividend_yield: None,
        deducted_net_profit_billion: None,
        deducted_net_profit_margin: None,
        deducted_net_profit_growth_rate: None,
        change_pct: None,
        volume: None,
        amount: None,
        turnover_rate: None,
        volume_ratio: None,
        quote_time: None,
    };
    let mut infra = bank.clone();
    infra.code = "601668.SH".to_string();
    infra.name = "中国建筑".to_string();
    infra.industry = "建筑装饰".to_string();
    infra.pe = Some(3.0);
    infra.pb = Some(0.4);
    infra.roe = Some(0.2);
    let mut duofuduo = bank.clone();
    duofuduo.code = "002407.SZ".to_string();
    duofuduo.name = "多氟多".to_string();
    duofuduo.industry = "化工".to_string();
    duofuduo.pe = Some(70.0);
    duofuduo.pb = Some(8.0);
    duofuduo.roe = Some(0.03);
    let mut material = duofuduo.clone();
    material.code = "002408.SZ".to_string();
    material.name = "氟材料公司".to_string();
    material.industry = "锂电材料".to_string();
    let mut chip = duofuduo.clone();
    chip.code = "688001.SH".to_string();
    chip.name = "芯片公司".to_string();
    chip.industry = "半导体".to_string();
    chip.pe = Some(60.0);
    let mut solar = chip.clone();
    solar.code = "601012.SH".to_string();
    solar.name = "光伏公司".to_string();
    solar.industry = "光伏".to_string();
    solar.pe = Some(50.0);
    solar.pb = Some(7.0);

    let result = screen_stocks(
        &[bank, infra, duofuduo, material, chip, solar],
        &ScreenCriteria {
            sort_by: "score".to_string(),
            sort_dir: "desc".to_string(),
            limit: 3,
            ..ScreenCriteria::default()
        },
    );

    let codes = result
        .items
        .iter()
        .map(|item| item.stock.code.as_str())
        .collect::<Vec<_>>();
    assert_eq!(codes, vec!["002408.SZ", "688001.SH", "601012.SH"]);
    assert!(!codes.contains(&"002407.SZ"));
    assert!(!codes.contains(&"000001.SZ"));
    assert!(!codes.contains(&"601668.SH"));
}

#[test]
fn score_sort_promotes_medical_and_game_candidates_with_other_hot_sectors() {
    let bank = StockItem {
        code: "000001.SZ".to_string(),
        name: "高分银行".to_string(),
        industry: "银行".to_string(),
        is_st: false,
        price: 10.0,
        pe: Some(2.0),
        pb: Some(0.2),
        roe: Some(0.3),
        market_cap_billion: Some(100.0),
        dividend_yield: None,
        deducted_net_profit_billion: None,
        deducted_net_profit_margin: None,
        deducted_net_profit_growth_rate: None,
        change_pct: None,
        volume: None,
        amount: None,
        turnover_rate: None,
        volume_ratio: None,
        quote_time: None,
    };
    let mut infra = bank.clone();
    infra.code = "601668.SH".to_string();
    infra.name = "中国建筑".to_string();
    infra.industry = "建筑装饰".to_string();
    infra.pe = Some(3.0);
    infra.pb = Some(0.4);
    infra.roe = Some(0.2);
    let mut material = bank.clone();
    material.code = "002408.SZ".to_string();
    material.name = "氟材料公司".to_string();
    material.industry = "锂电材料".to_string();
    material.pe = Some(70.0);
    material.pb = Some(8.0);
    material.roe = Some(0.03);
    let mut chip = material.clone();
    chip.code = "688001.SH".to_string();
    chip.name = "芯片公司".to_string();
    chip.industry = "半导体".to_string();
    chip.pe = Some(60.0);
    let mut solar = chip.clone();
    solar.code = "601012.SH".to_string();
    solar.name = "光伏公司".to_string();
    solar.industry = "光伏".to_string();
    solar.pe = Some(50.0);
    solar.pb = Some(7.0);
    let mut game = chip.clone();
    game.code = "002555.SZ".to_string();
    game.name = "游戏公司".to_string();
    game.industry = "网络游戏".to_string();
    game.pe = Some(80.0);
    game.pb = Some(9.0);
    game.roe = Some(0.02);

    let mut medical = chip.clone();
    medical.code = "300015.SZ".to_string();
    medical.name = "创新药公司".to_string();
    medical.industry = "医疗器械".to_string();
    medical.pe = Some(75.0);
    medical.pb = Some(8.5);
    medical.roe = Some(0.025);

    let result = screen_stocks(
        &[bank, infra, material, chip, solar, medical, game],
        &ScreenCriteria {
            sort_by: "score".to_string(),
            sort_dir: "desc".to_string(),
            limit: 5,
            ..ScreenCriteria::default()
        },
    );

    let codes = result
        .items
        .iter()
        .map(|item| item.stock.code.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        codes,
        vec![
            "002408.SZ",
            "688001.SH",
            "601012.SH",
            "300015.SZ",
            "002555.SZ"
        ]
    );
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
            stock_codes: Vec::new(),
            start_date: "20200101".to_string(),
            end_date: "20200103".to_string(),
            top_n: 1,
            initial_cash: 1000.0,
        },
    )
    .expect("native data backtest should run");
    assert_eq!(result.equity_curve.len(), 3);
    assert_eq!(result.equity_curve.last().unwrap().equity, 1200.0);
}

#[test]
fn backtests_watchlist_codes_in_saved_order() {
    let result = backtest_with_data(
        &sample_data_set(),
        &BacktestRequest {
            criteria: ScreenCriteria::default(),
            source: "watchlist".to_string(),
            stock_codes: vec!["222222.SZ".to_string(), "111111.SZ".to_string()],
            start_date: "20200101".to_string(),
            end_date: "20200103".to_string(),
            top_n: 10,
            initial_cash: 1000.0,
        },
    )
    .expect("watchlist backtest should run");

    assert_eq!(result.symbols, vec!["222222.SZ", "111111.SZ"]);
    assert!(result.notes.iter().any(|note| note.contains("自选观察池")));
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
    // NaN/Infinity must not propagate (all comparisons are false -> last arm).
    assert!(pe_score(Some(f64::NAN)).is_finite());
    assert!(pb_score(Some(f64::NAN)).is_finite());
    assert!(pe_score(Some(f64::INFINITY)).is_finite());
    assert!(pb_score(Some(f64::INFINITY)).is_finite());
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
    let result = screen_stocks(&[stock], &ScreenCriteria::default());
    assert_eq!(result.returned, 1);
    let scored = &result.items[0];
    assert!(scored.score.is_finite(), "composite score must be finite");
    assert!((0.0..=SCREEN_SCORE_SCALE).contains(&scored.score));
    for (key, value) in &scored.factor_scores {
        assert!(value.is_finite(), "factor score {key} must be finite");
    }
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
