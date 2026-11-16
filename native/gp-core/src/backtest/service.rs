use crate::backtest::history::{load_backtest_histories, strict_walk_forward_history_symbols};
use crate::backtest::metrics::{
    backtest_metrics, empty_backtest_metrics, equal_weight_benchmark_curve,
    evaluate_walk_forward_folds, selected_symbols_from_latest_rebalance,
    selected_symbols_from_rebalances,
};
use crate::backtest::request::{
    backtest_rebalance_note, backtest_scoring_parameters, backtest_strategy_mode,
    backtest_strategy_mode_note, is_adaptive_backtest, normalize_backtest_benchmark,
    normalize_backtest_source, normalize_rebalance_frequency, sanitize_initial_cash,
    sanitize_transaction_cost_rate, BacktestRequest,
};
use crate::backtest::selection::selected_backtest_items;
use crate::backtest::simulation::{
    simulate_equal_weight_portfolio, simulate_walk_forward_portfolio,
};
use crate::backtest::types::{BacktestMetrics, BacktestResult};
use crate::data_source::{CoreDataSet, MarketDataSource, StaticDataSource};
use crate::dates::parse_date;
use crate::demo_data::MockDataSource;
use crate::error::{CoreError, CoreResult};
use crate::market_data::{normalize_stock_code, StockItem};
use crate::volatility;
use std::collections::{HashMap, HashSet};

pub fn backtest_with_mock(request: &BacktestRequest) -> CoreResult<BacktestResult> {
    backtest_with_source(&MockDataSource, request)
}

pub fn backtest_with_data(
    data: &CoreDataSet,
    request: &BacktestRequest,
) -> CoreResult<BacktestResult> {
    let source = StaticDataSource::new(data);
    backtest_with_source(&source, request)
}

struct BacktestDataRequirements {
    universe: Vec<StockItem>,
    selection_notes: Vec<String>,
    symbols: Vec<String>,
    history_symbols: Vec<String>,
}

fn resolve_backtest_data_requirements(
    source: &impl MarketDataSource,
    request: &BacktestRequest,
) -> CoreResult<BacktestDataRequirements> {
    validate_backtest_date_range(request)?;
    backtest_scoring_parameters(request)?;
    let mut universe = source.stocks()?.to_vec();
    let strategy_mode = backtest_strategy_mode(request);
    let dynamic_mode = strategy_mode != "candidate_snapshot";
    let source_mode = normalize_backtest_source(&request.source);
    if dynamic_mode && source_mode != "watchlist" {
        let mut known_codes = universe
            .iter()
            .map(|stock| stock.code.to_uppercase())
            .collect::<HashSet<_>>();
        for code in source.factor_snapshot_codes()? {
            let normalized = normalize_stock_code(&code);
            if normalized.is_empty() || !known_codes.insert(normalized.to_uppercase()) {
                continue;
            }
            universe.push(StockItem {
                code: normalized,
                ..StockItem::default()
            });
        }
    }
    let (selected, selection_notes) = if dynamic_mode && source_mode != "watchlist" {
        (Vec::new(), Vec::new())
    } else {
        selected_backtest_items(&universe, request)
    };
    let symbols: Vec<String> = if dynamic_mode && source_mode != "watchlist" {
        universe.iter().map(|item| item.code.clone()).collect()
    } else {
        selected
            .iter()
            .map(|item| item.stock.code.clone())
            .collect()
    };
    let history_symbols = if dynamic_mode {
        strict_walk_forward_history_symbols(
            source,
            &symbols,
            &request.start_date,
            &request.end_date,
        )?
    } else {
        symbols.clone()
    };
    Ok(BacktestDataRequirements {
        universe,
        selection_notes,
        symbols,
        history_symbols,
    })
}

pub fn validate_backtest_date_range(request: &BacktestRequest) -> CoreResult<()> {
    let start = parse_date(&request.start_date)?;
    let end = parse_date(&request.end_date)?;
    if start > end {
        return Err(CoreError::new(format!(
            "回测开始日期不能晚于结束日期：{} > {}",
            request.start_date, request.end_date
        )));
    }
    Ok(())
}

pub fn backtest_required_history_symbols(
    source: &impl MarketDataSource,
    request: &BacktestRequest,
) -> CoreResult<Vec<String>> {
    Ok(resolve_backtest_data_requirements(source, request)?.history_symbols)
}

pub fn backtest_with_source(
    source: &impl MarketDataSource,
    request: &BacktestRequest,
) -> CoreResult<BacktestResult> {
    backtest_scoring_parameters(request)?;
    let strategy_mode = backtest_strategy_mode(request);
    let dynamic_mode = strategy_mode != "candidate_snapshot";
    let BacktestDataRequirements {
        universe,
        selection_notes,
        symbols,
        history_symbols,
    } = resolve_backtest_data_requirements(source, request)?;
    let history_load_start = if is_adaptive_backtest(request) {
        "19900101"
    } else {
        &request.start_date
    };
    let histories = load_backtest_histories(
        source,
        &history_symbols,
        history_load_start,
        &request.end_date,
    )?;
    if dynamic_mode {
        let available_codes = histories
            .iter()
            .map(|history| history.code.to_uppercase())
            .collect::<HashSet<_>>();
        let missing_history_codes = history_symbols
            .iter()
            .filter(|code| !available_codes.contains(&code.to_uppercase()))
            .take(8)
            .cloned()
            .collect::<Vec<_>>();
        if !missing_history_codes.is_empty() {
            return Err(CoreError::new(format!(
                "walk_forward 严格回测缺少当期上市候选的历史行情：{}",
                missing_history_codes.join("、")
            )));
        }
    }
    if histories.is_empty() {
        if !symbols.is_empty() {
            return Err(CoreError::new(format!(
                "回测区间缺少已选 {} 只股票的历史日线，无法计算净值曲线。请联网重试或先刷新行情缓存。",
                symbols.len()
            )));
        }
        let reported_symbols = if dynamic_mode { Vec::new() } else { symbols };
        return Ok(BacktestResult {
            metrics: empty_backtest_metrics(reported_symbols.len(), &strategy_mode),
            equity_curve: Vec::new(),
            benchmark_curve: Vec::new(),
            symbols: reported_symbols,
            benchmark_symbols: Vec::new(),
            rebalance_dates: Vec::new(),
            walk_forward_folds: Vec::new(),
            volatility_snapshots: Vec::new(),
            volatility_message: Some("回测区间没有可用于波动率计算的标的日线。".to_string()),
            strategy_mode,
            notes: selection_notes,
        });
    }

    let initial_cash = sanitize_initial_cash(request.initial_cash);
    let cost_rate = sanitize_transaction_cost_rate(request.transaction_cost_bps);
    let rebalance_frequency = normalize_rebalance_frequency(&request.rebalance_frequency);
    let simulation = match strategy_mode.as_str() {
        "walk_forward" | "adaptive_swing_v1" | "adaptive_swing_v2" => {
            simulate_walk_forward_portfolio(
                source,
                &universe,
                request,
                &histories,
                initial_cash,
                cost_rate,
                &rebalance_frequency,
            )?
        }
        _ => simulate_equal_weight_portfolio(
            &histories,
            initial_cash,
            cost_rate,
            &rebalance_frequency,
        ),
    };
    let benchmark_enabled = normalize_backtest_benchmark(&request.benchmark) != "none";
    let benchmark_curve = if benchmark_enabled {
        equal_weight_benchmark_curve(
            &histories,
            initial_cash,
            parse_date(&request.start_date).ok(),
        )
    } else {
        Vec::new()
    };

    let (total_return, annualized_return, max_drawdown) =
        backtest_metrics(&simulation.equity_curve, initial_cash);
    let (benchmark_total_return, benchmark_annualized_return, benchmark_max_drawdown) =
        if benchmark_curve.is_empty() {
            (None, None, None)
        } else {
            let (total, annualized, drawdown) = backtest_metrics(&benchmark_curve, initial_cash);
            (Some(total), annualized, drawdown)
        };
    let excess_return =
        benchmark_total_return.map(|benchmark_return| total_return - benchmark_return);
    let available_symbols: Vec<String> = histories
        .iter()
        .map(|history| history.code.clone())
        .collect();
    let walk_forward_folds = if dynamic_mode {
        evaluate_walk_forward_folds(&histories, &simulation.selections)
    } else {
        Vec::new()
    };
    let reported_symbols = if dynamic_mode {
        selected_symbols_from_rebalances(&histories, &simulation.selections)
    } else {
        available_symbols.clone()
    };
    let volatility_symbols = if dynamic_mode {
        selected_symbols_from_latest_rebalance(&histories, &simulation.selections)
    } else {
        reported_symbols.clone()
    };
    let evaluated_selection_count = walk_forward_folds
        .iter()
        .map(|fold| fold.evaluated_selection_count)
        .sum::<usize>();
    let selection_hit_count = walk_forward_folds
        .iter()
        .map(|fold| fold.hit_count)
        .sum::<usize>();
    let precision_at_n = (evaluated_selection_count > 0)
        .then_some(selection_hit_count as f64 / evaluated_selection_count as f64);
    let oos_fold_count = walk_forward_folds
        .iter()
        .filter(|fold| fold.precision_at_n.is_some())
        .count();

    let mut notes = selection_notes;
    let missing_history_count = symbols.len().saturating_sub(available_symbols.len());
    if missing_history_count > 0 {
        notes.push(format!(
            "{} 只标的缺少有效历史行情，已从回测组合中剔除。",
            missing_history_count
        ));
    }
    notes.push(format!(
        "回测采用{}等权组合，交易成本为{:.1}bps。",
        backtest_rebalance_note(&rebalance_frequency),
        request.transaction_cost_bps.clamp(0.0, 500.0)
    ));
    notes.push(backtest_strategy_mode_note(&strategy_mode).to_string());
    if dynamic_mode {
        notes.push(format!(
            "滚动样本外评估共 {} 折，{} 个入选样本可计算相对候选池基准的命中率。",
            oos_fold_count, evaluated_selection_count
        ));
    }
    if benchmark_enabled {
        notes.push("基准为候选池买入持有等权曲线，用于衡量调仓与成本后的超额收益。".to_string());
    }
    let history_by_code = histories
        .iter()
        .map(|history| (history.code.to_uppercase(), history))
        .collect::<HashMap<_, _>>();
    let stock_name_by_code = universe
        .iter()
        .filter_map(|stock| {
            let name = stock.name.trim();
            (!name.is_empty()).then(|| (stock.code.to_uppercase(), name.to_string()))
        })
        .collect::<HashMap<_, _>>();
    let volatility_snapshots = volatility_symbols
        .iter()
        .filter_map(|symbol| {
            history_by_code
                .get(&symbol.to_uppercase())
                .and_then(|history| {
                    volatility::calculate_volatility_snapshot(symbol, &history.bars).map(
                        |mut snapshot| {
                            snapshot.name = stock_name_by_code.get(&symbol.to_uppercase()).cloned();
                            snapshot
                        },
                    )
                })
        })
        .collect::<Vec<_>>();
    let volatility_message = volatility_snapshots.is_empty().then(|| {
        if dynamic_mode && volatility_symbols.is_empty() {
            "Walk-forward 末次调仓没有符合条件的标的。".to_string()
        } else if volatility_symbols.is_empty() {
            "候选快照没有符合条件的标的。".to_string()
        } else {
            "区间末标的没有可解析的日线日期。".to_string()
        }
    });
    notes.push(
        "波动率快照按区间末可见日线计算：ATR14、布林20/2、唐奇安20、凯尔特纳EMA20+2×ATR10、Chaikin 10/10、RVI14；历史不足的指标保持缺失。"
            .to_string(),
    );

    Ok(BacktestResult {
        metrics: BacktestMetrics {
            total_return,
            annualized_return,
            max_drawdown,
            num_stocks: reported_symbols.len(),
            benchmark_total_return,
            benchmark_annualized_return,
            benchmark_max_drawdown,
            excess_return,
            total_transaction_cost: simulation.total_transaction_cost,
            total_turnover: simulation.total_turnover,
            rebalance_count: simulation.rebalance_dates.len(),
            oos_fold_count,
            evaluated_selection_count,
            selection_hit_count,
            precision_at_n,
            strategy_mode: strategy_mode.clone(),
        },
        equity_curve: simulation.equity_curve,
        benchmark_curve,
        symbols: reported_symbols,
        benchmark_symbols: if benchmark_enabled {
            available_symbols
        } else {
            Vec::new()
        },
        rebalance_dates: simulation
            .rebalance_dates
            .iter()
            .map(|date| date.format("%Y-%m-%d").to_string())
            .collect(),
        walk_forward_folds,
        volatility_snapshots,
        volatility_message,
        strategy_mode,
        notes,
    })
}
