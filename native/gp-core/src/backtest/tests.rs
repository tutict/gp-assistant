use super::history::{
    latest_factor_snapshot_on_or_before, load_factor_snapshot_timeline, price_on_date,
    BacktestHistory,
};
use super::metrics::{evaluate_walk_forward_folds, forward_return};
use super::request::BacktestRequest;
use super::selection::{adaptive_walk_forward_active_indices, point_in_time_universe};
use super::simulation::simulate_walk_forward_portfolio_with_snapshots;
use crate::adaptive_screen::{adaptive_screen_stocks_with_parameters, AdaptiveScreenRequest};
use crate::data_source::StaticDataSource;
use crate::dates::parse_date;
use crate::market_data::{HistoryBar, StockFactorSnapshot};
use crate::scoring_parameters::fixed_scoring_parameters;
use crate::screening::types::ScreenCriteria;
use crate::tests::{adaptive_history, adaptive_stock, sample_data_set};
use chrono::NaiveDate;
use std::collections::{BTreeMap, HashMap};

#[test]
fn adaptive_rebalance_propagates_market_data_errors_instead_of_silently_clearing_positions() {
    let selection_date = NaiveDate::from_ymd_opt(2026, 4, 1).expect("valid date");
    let snapshot_date = NaiveDate::from_ymd_opt(2026, 1, 1).expect("valid date");
    let stocks = (0..12).map(adaptive_stock).collect::<Vec<_>>();
    let histories = stocks
        .iter()
        .map(|stock| {
            let bars = (0..91)
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
            algorithm_version: None,
            parameter_set_id: None,
            scoring_parameters: None,
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
fn adaptive_walk_forward_signals_on_the_prior_session_and_trades_on_the_next_session() {
    let base_date = NaiveDate::from_ymd_opt(2026, 1, 1).expect("valid base date");
    let signal_date = base_date + chrono::Duration::days(70);
    let execution_date = base_date + chrono::Duration::days(71);
    let evaluation_date = base_date + chrono::Duration::days(72);
    let stocks = (0..12).map(adaptive_stock).collect::<Vec<_>>();
    let histories = stocks
        .iter()
        .map(|stock| {
            let bars = (0..90)
                .map(|index| {
                    let date = base_date + chrono::Duration::days(index);
                    let close = if index <= 70 {
                        10.0
                    } else if index == 71 {
                        20.0
                    } else {
                        40.0
                    };
                    HistoryBar {
                        date: date.format("%Y-%m-%d").to_string(),
                        open: Some(close),
                        high: Some(close * 1.01),
                        low: Some(close * 0.99),
                        close,
                        volume: Some(1_000_000.0),
                        capital: None,
                    }
                })
                .collect::<Vec<_>>();
            BacktestHistory {
                code: stock.code.clone(),
                prices: bars
                    .iter()
                    .map(|bar| (parse_date(&bar.date).expect("valid bar date"), bar.close))
                    .collect(),
                bars,
            }
        })
        .collect::<Vec<_>>();
    let snapshots = stocks
        .iter()
        .map(|stock| {
            (
                stock.code.clone(),
                BTreeMap::from([(
                    base_date,
                    StockFactorSnapshot {
                        date: base_date.format("%Y-%m-%d").to_string(),
                        available_date: Some(base_date.format("%Y-%m-%d").to_string()),
                        name: Some(stock.name.clone()),
                        industry: Some(stock.industry.clone()),
                        is_st: Some(false),
                        is_listed: Some(true),
                        is_tradable: Some(true),
                        price: Some(10.0),
                        pe: stock.pe,
                        pb: stock.pb,
                        roe: stock.roe,
                        market_cap_billion: stock.market_cap_billion,
                        dividend_yield: stock.dividend_yield,
                        deducted_net_profit_billion: stock.deducted_net_profit_billion,
                        deducted_net_profit_growth_rate: stock.deducted_net_profit_growth_rate,
                        amount: stock.amount,
                        turnover_rate: stock.turnover_rate,
                        volume_ratio: stock.volume_ratio,
                        ..StockFactorSnapshot::default()
                    },
                )]),
            )
        })
        .collect::<HashMap<_, _>>();
    let benchmark_histories = ["000001.SH", "399001.SZ", "399006.SZ"]
        .into_iter()
        .map(|code| {
            (
                code.to_string(),
                (0..90)
                    .map(|index| {
                        let date = base_date + chrono::Duration::days(index);
                        HistoryBar {
                            date: date.format("%Y-%m-%d").to_string(),
                            open: Some(100.0),
                            high: Some(101.0),
                            low: Some(99.0),
                            close: 100.0,
                            volume: Some(1_000_000.0),
                            capital: None,
                        }
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<HashMap<_, _>>();
    let simulation = simulate_walk_forward_portfolio_with_snapshots(
        &stocks,
        &BacktestRequest {
            algorithm_version: None,
            parameter_set_id: None,
            scoring_parameters: None,
            criteria: ScreenCriteria::default(),
            source: "criteria".to_string(),
            strategy_mode: "adaptive_swing_v1:range".to_string(),
            stock_codes: Vec::new(),
            start_date: signal_date.format("%Y%m%d").to_string(),
            end_date: evaluation_date.format("%Y%m%d").to_string(),
            top_n: 10,
            initial_cash: 1_000.0,
            rebalance_frequency: "monthly".to_string(),
            transaction_cost_bps: 0.0,
            benchmark: "candidate_equal_weight".to_string(),
        },
        &histories,
        &snapshots,
        vec![signal_date, execution_date, evaluation_date],
        1_000.0,
        0.0,
        "monthly",
        &benchmark_histories,
    )
    .expect("adaptive walk-forward should execute with point-in-time data");

    assert_eq!(simulation.selections.len(), 1);
    assert_eq!(simulation.selections[0].signal_date, signal_date);
    assert_eq!(simulation.selections[0].date, execution_date);
    let folds = evaluate_walk_forward_folds(&histories, &simulation.selections);
    assert_eq!(folds[0].signal_date.as_deref(), Some("2026-03-12"));
    assert_eq!(folds[0].selection_date, "2026-03-13");
    assert!((simulation.equity_curve[1].equity - 1_000.0).abs() < 1e-6);
    assert!((simulation.equity_curve[2].equity - 2_000.0).abs() < 1e-6);

    let history_index = histories
        .iter()
        .enumerate()
        .map(|(index, history)| (history.code.to_ascii_uppercase(), index))
        .collect::<HashMap<_, _>>();
    let exact_signal_prices = histories
        .iter()
        .map(|history| price_on_date(history, signal_date))
        .collect::<Vec<_>>();
    let auto_request = BacktestRequest {
        algorithm_version: None,
        parameter_set_id: None,
        scoring_parameters: None,
        criteria: ScreenCriteria::default(),
        source: "criteria".to_string(),
        strategy_mode: "adaptive_swing_v1:auto".to_string(),
        stock_codes: Vec::new(),
        start_date: signal_date.format("%Y%m%d").to_string(),
        end_date: evaluation_date.format("%Y%m%d").to_string(),
        top_n: 10,
        initial_cash: 1_000.0,
        rebalance_frequency: "monthly".to_string(),
        transaction_cost_bps: 0.0,
        benchmark: "candidate_equal_weight".to_string(),
    };
    let mut one_stale_stock = exact_signal_prices.clone();
    one_stale_stock[0] = None;
    let active = adaptive_walk_forward_active_indices(
        &stocks,
        &auto_request,
        &histories,
        &benchmark_histories,
        &snapshots,
        &history_index,
        &one_stale_stock,
        signal_date,
    )
    .expect("a stock without an exact signal-day quote should be excluded, not backfilled");
    assert!(!active.selected_indices.contains(&0));

    let mut stale_benchmarks = benchmark_histories.clone();
    for code in ["000001.SH", "399001.SZ"] {
        stale_benchmarks
            .get_mut(code)
            .expect("benchmark fixture should exist")
            .retain(|bar| parse_date(&bar.date).ok() != Some(signal_date));
    }
    let error = adaptive_walk_forward_active_indices(
        &stocks,
        &auto_request,
        &histories,
        &stale_benchmarks,
        &snapshots,
        &history_index,
        &exact_signal_prices,
        signal_date,
    )
    .expect_err("auto mode must reject benchmarks without an exact signal-day bar");
    assert!(error.to_string().contains("宽基指数"));
}

#[test]
fn calibrated_walk_forward_routes_frozen_weights_with_point_in_time_signals() {
    let base = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
    let signal = base + chrono::Duration::days(89);
    let mut a = adaptive_stock(0);
    a.roe = Some(0.10);
    a.volume_ratio = Some(2.0);
    let mut b = a.clone();
    b.code = "000002.SZ".into();
    b.roe = Some(0.13);
    b.volume_ratio = Some(3.01);
    let stocks = vec![a, b];
    let bars = adaptive_history(10.0, 0.002, 90)
        .into_iter()
        .enumerate()
        .map(|(i, mut bar)| {
            bar.date = (base + chrono::Duration::days(i as i64))
                .format("%Y-%m-%d")
                .to_string();
            bar
        })
        .collect::<Vec<_>>();
    let histories = stocks
        .iter()
        .map(|stock| BacktestHistory {
            code: stock.code.clone(),
            prices: bars
                .iter()
                .map(|bar| (parse_date(&bar.date).unwrap(), bar.close))
                .collect(),
            bars: bars.clone(),
        })
        .collect::<Vec<_>>();
    let index = stocks
        .iter()
        .enumerate()
        .map(|(i, s)| (s.code.clone(), i))
        .collect::<HashMap<_, _>>();
    let prices = vec![Some(bars.last().unwrap().close); 2];
    let snapshots = stocks
        .iter()
        .map(|stock| {
            (
                stock.code.clone(),
                BTreeMap::from([(
                    base,
                    StockFactorSnapshot {
                        date: base.format("%Y-%m-%d").to_string(),
                        available_date: Some(base.format("%Y-%m-%d").to_string()),
                        name: Some(stock.name.clone()),
                        industry: Some(stock.industry.clone()),
                        is_st: Some(false),
                        is_listed: Some(true),
                        is_tradable: Some(true),
                        pe: stock.pe,
                        pb: stock.pb,
                        roe: stock.roe,
                        market_cap_billion: stock.market_cap_billion,
                        dividend_yield: stock.dividend_yield,
                        deducted_net_profit_billion: stock.deducted_net_profit_billion,
                        deducted_net_profit_growth_rate: stock.deducted_net_profit_growth_rate,
                        amount: stock.amount,
                        turnover_rate: stock.turnover_rate,
                        volume_ratio: stock.volume_ratio,
                        ..Default::default()
                    },
                )]),
            )
        })
        .collect::<HashMap<_, _>>();
    let request = BacktestRequest {
        strategy_mode: "adaptive_swing_v1:range".into(),
        top_n: 1,
        ..Default::default()
    };
    let fixed = adaptive_walk_forward_active_indices(
        &stocks,
        &request,
        &histories,
        &HashMap::new(),
        &snapshots,
        &index,
        &prices,
        signal,
    )
    .unwrap();
    let mut parameters = fixed_scoring_parameters();
    parameters.algorithm_version = "adaptive_swing_v2".into();
    parameters.parameter_set_id = "pit-test-v2".into();
    parameters
        .modes
        .get_mut("range")
        .unwrap()
        .weights
        .insert("quality".into(), 0.29);
    parameters
        .modes
        .get_mut("range")
        .unwrap()
        .weights
        .insert("range_setup".into(), 0.17);
    let v2request = BacktestRequest {
        strategy_mode: "adaptive_swing_v2:range".into(),
        algorithm_version: Some("adaptive_swing_v2".into()),
        parameter_set_id: Some(parameters.parameter_set_id.clone()),
        scoring_parameters: Some(parameters.clone()),
        top_n: 1,
        ..Default::default()
    };
    let calibrated = adaptive_walk_forward_active_indices(
        &stocks,
        &v2request,
        &histories,
        &HashMap::new(),
        &snapshots,
        &index,
        &prices,
        signal,
    )
    .unwrap();
    assert_ne!(
        calibrated.selected_indices, fixed.selected_indices,
        "calibrated weights must change this boundary selection"
    );
    let mut visible = point_in_time_universe(&stocks, &snapshots, &index, &prices, signal);
    for stock in &mut visible {
        stock.change_pct = Some(bars.last().unwrap().close / bars[bars.len() - 2].close - 1.0);
        stock.quote_time = Some(signal.format("%Y-%m-%d").to_string());
    }
    let visible_histories = stocks
        .iter()
        .map(|s| (s.code.clone(), bars.clone()))
        .collect();
    let expected = adaptive_screen_stocks_with_parameters(
        &visible,
        &visible_histories,
        &HashMap::new(),
        &[],
        &AdaptiveScreenRequest {
            mode: "range".into(),
            primary_limit: 1,
            exploration_limit: 0,
            as_of_date: Some(signal.format("%Y%m%d").to_string()),
            ..Default::default()
        },
        &parameters,
    )
    .unwrap();
    assert_eq!(
        calibrated.selected_indices,
        vec![index[&expected.items[0].stock.code]]
    );
    let mut future = histories.clone();
    for history in &mut future {
        let mut bar = bars.last().unwrap().clone();
        bar.date = (signal + chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        bar.close *= 10.0;
        bar.high = Some(bar.close);
        bar.low = Some(bar.close);
        history.bars.push(bar);
    }
    let unchanged = adaptive_walk_forward_active_indices(
        &stocks,
        &v2request,
        &future,
        &HashMap::new(),
        &snapshots,
        &index,
        &prices,
        signal,
    )
    .unwrap();
    assert_eq!(unchanged.selected_indices, calibrated.selected_indices);
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
