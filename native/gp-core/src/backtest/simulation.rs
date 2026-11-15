use crate::backtest::history::{
    backtest_calendar, latest_price_on_or_before, load_factor_snapshot_timeline, price_on_date,
    BacktestHistory,
};
use crate::backtest::request::{
    adaptive_backtest_requested_mode, backtest_scoring_parameters, is_adaptive_backtest,
    BacktestRequest,
};
use crate::backtest::selection::{
    adaptive_walk_forward_active_indices, walk_forward_active_indices,
};
use crate::backtest::types::EquityPoint;
use crate::data_source::MarketDataSource;
use crate::dates::parse_date;
use crate::error::{CoreError, CoreResult};
use crate::market_data::{HistoryBar, StockFactorSnapshot, StockItem};
use chrono::{Datelike, NaiveDate};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Debug)]
pub(super) struct PortfolioSimulation {
    pub(super) equity_curve: Vec<EquityPoint>,
    pub(super) rebalance_dates: Vec<NaiveDate>,
    pub(super) total_transaction_cost: f64,
    pub(super) total_turnover: f64,
    pub(super) selections: Vec<RebalanceSelection>,
}

#[derive(Clone, Debug)]
pub(super) struct RebalanceSelection {
    pub(super) signal_date: NaiveDate,
    pub(super) date: NaiveDate,
    pub(super) selected_indices: Vec<usize>,
    pub(super) eligible_indices: Vec<usize>,
}

pub(super) fn simulate_equal_weight_portfolio(
    histories: &[BacktestHistory],
    initial_cash: f64,
    transaction_cost_rate: f64,
    rebalance_frequency: &str,
) -> PortfolioSimulation {
    let dates = backtest_calendar(histories);
    let mut cash = initial_cash;
    let mut holdings = vec![0.0; histories.len()];
    let mut last_rebalance_date: Option<NaiveDate> = None;
    let mut equity_curve = Vec::with_capacity(dates.len());
    let mut rebalance_dates = Vec::new();
    let mut total_transaction_cost = 0.0;
    let mut total_turnover = 0.0;

    for date in dates {
        let current_prices: Vec<Option<f64>> = histories
            .iter()
            .map(|history| latest_price_on_or_before(history, date))
            .collect();
        let equity_before_rebalance = cash
            + holdings
                .iter()
                .zip(current_prices.iter())
                .map(|(shares, price)| shares * price.unwrap_or(0.0))
                .sum::<f64>();

        if should_rebalance(date, last_rebalance_date, rebalance_frequency)
            && equity_before_rebalance > 0.0
        {
            let active_indices: Vec<usize> = current_prices
                .iter()
                .enumerate()
                .filter_map(|(index, price)| price.filter(|value| *value > 0.0).map(|_| index))
                .collect();
            if !active_indices.is_empty() {
                let target_weight = 1.0 / active_indices.len() as f64;
                let mut turnover_value = 0.0;
                for index in 0..histories.len() {
                    let price = current_prices[index].unwrap_or(0.0);
                    let target_value = if price > 0.0 && active_indices.contains(&index) {
                        equity_before_rebalance * target_weight
                    } else {
                        0.0
                    };
                    let current_value = holdings[index] * price;
                    turnover_value += (target_value - current_value).abs();
                }

                let transaction_cost = turnover_value * transaction_cost_rate;
                let investable_equity = (equity_before_rebalance - transaction_cost).max(0.0);
                cash = investable_equity;
                holdings.fill(0.0);
                for index in active_indices {
                    if let Some(price) = current_prices[index].filter(|value| *value > 0.0) {
                        let target_value = investable_equity * target_weight;
                        holdings[index] = target_value / price;
                        cash -= target_value;
                    }
                }
                total_transaction_cost += transaction_cost;
                total_turnover += turnover_value / equity_before_rebalance;
                rebalance_dates.push(date);
                last_rebalance_date = Some(date);
            }
        }

        let equity = cash
            + holdings
                .iter()
                .zip(current_prices.iter())
                .map(|(shares, price)| shares * price.unwrap_or(0.0))
                .sum::<f64>();
        equity_curve.push(EquityPoint {
            date: date.format("%Y-%m-%d").to_string(),
            equity,
        });
    }

    PortfolioSimulation {
        equity_curve,
        rebalance_dates,
        total_transaction_cost,
        total_turnover,
        selections: Vec::new(),
    }
}

pub(super) fn simulate_walk_forward_portfolio(
    source: &impl MarketDataSource,
    universe: &[StockItem],
    request: &BacktestRequest,
    histories: &[BacktestHistory],
    initial_cash: f64,
    transaction_cost_rate: f64,
    rebalance_frequency: &str,
) -> CoreResult<PortfolioSimulation> {
    let start_date = parse_date(&request.start_date)?;
    let end_date = parse_date(&request.end_date)?;
    let dates = backtest_calendar(histories)
        .into_iter()
        .filter(|date| *date >= start_date && *date <= end_date)
        .collect::<Vec<_>>();
    let mut snapshots_by_code = HashMap::new();
    let mut missing_snapshot_codes = Vec::new();
    for history in histories {
        let timeline = load_factor_snapshot_timeline(source, &history.code)?;
        if timeline.is_empty() {
            missing_snapshot_codes.push(history.code.clone());
        }
        snapshots_by_code.insert(history.code.clone(), timeline);
    }
    if !missing_snapshot_codes.is_empty() {
        let preview = missing_snapshot_codes
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join("、");
        return Err(CoreError::new(format!(
            "walk_forward 严格回测要求每个有历史行情的标的都有历史因子快照 factor_snapshots；缺少 {} 个：{}",
            missing_snapshot_codes.len(),
            preview
        )));
    }
    let benchmark_histories = if is_adaptive_backtest(request) {
        let histories = ["000001.SH", "399001.SZ", "399006.SZ"]
            .into_iter()
            .filter_map(|code| {
                source
                    .get_history(code, "19900101", &request.end_date)
                    .ok()
                    .filter(|bars| bars.len() >= MIN_HISTORY_BARS_FOR_ADAPTIVE_BACKTEST)
                    .map(|bars| (code.to_string(), bars))
            })
            .collect::<HashMap<_, _>>();
        if adaptive_backtest_requested_mode(&request.strategy_mode) == "auto" && histories.len() < 2
        {
            return Err(CoreError::new(
                "adaptive_swing_v1 回测至少需要2个有效宽基指数的历史日线",
            ));
        }
        histories
    } else {
        HashMap::new()
    };
    simulate_walk_forward_portfolio_with_snapshots(
        universe,
        request,
        histories,
        &snapshots_by_code,
        dates,
        initial_cash,
        transaction_cost_rate,
        rebalance_frequency,
        &benchmark_histories,
    )
}

const MIN_HISTORY_BARS_FOR_ADAPTIVE_BACKTEST: usize = 60;

pub(super) fn simulate_walk_forward_portfolio_with_snapshots(
    universe: &[StockItem],
    request: &BacktestRequest,
    histories: &[BacktestHistory],
    snapshots_by_code: &HashMap<String, BTreeMap<NaiveDate, StockFactorSnapshot>>,
    dates: Vec<NaiveDate>,
    initial_cash: f64,
    transaction_cost_rate: f64,
    rebalance_frequency: &str,
    benchmark_histories: &HashMap<String, Vec<HistoryBar>>,
) -> CoreResult<PortfolioSimulation> {
    backtest_scoring_parameters(request)?;
    let mut cash = initial_cash;
    let mut holdings = vec![0.0; histories.len()];
    let mut last_rebalance_date: Option<NaiveDate> = None;
    let mut previous_trade_date: Option<NaiveDate> = None;
    let mut equity_curve = Vec::with_capacity(dates.len());
    let mut rebalance_dates = Vec::new();
    let mut total_transaction_cost = 0.0;
    let mut total_turnover = 0.0;
    let mut selections = Vec::new();
    let history_index: HashMap<String, usize> = histories
        .iter()
        .enumerate()
        .map(|(index, history)| (history.code.to_uppercase(), index))
        .collect();

    for date in dates {
        let valuation_prices: Vec<Option<f64>> = histories
            .iter()
            .map(|history| latest_price_on_or_before(history, date))
            .collect();
        let trade_prices: Vec<Option<f64>> = histories
            .iter()
            .map(|history| price_on_date(history, date))
            .collect();
        let equity_before_rebalance = cash
            + holdings
                .iter()
                .zip(valuation_prices.iter())
                .map(|(shares, price)| shares * price.unwrap_or(0.0))
                .sum::<f64>();

        if should_rebalance(date, last_rebalance_date, rebalance_frequency)
            && equity_before_rebalance > 0.0
            && previous_trade_date.is_some()
        {
            let signal_date = previous_trade_date.expect("checked above");
            let signal_prices = histories
                .iter()
                .map(|history| price_on_date(history, signal_date))
                .collect::<Vec<_>>();
            let active = if is_adaptive_backtest(request) {
                adaptive_walk_forward_active_indices(
                    universe,
                    request,
                    histories,
                    benchmark_histories,
                    snapshots_by_code,
                    &history_index,
                    &signal_prices,
                    signal_date,
                )?
            } else {
                walk_forward_active_indices(
                    universe,
                    request,
                    snapshots_by_code,
                    &history_index,
                    &signal_prices,
                    signal_date,
                )
            };
            let target_weight = if active.selected_indices.is_empty() {
                0.0
            } else {
                1.0 / active.selected_indices.len() as f64
            };
            let active_set: HashSet<usize> = active.selected_indices.iter().copied().collect();
            let tradable_equity_before = cash
                + holdings
                    .iter()
                    .zip(trade_prices.iter())
                    .map(|(shares, price)| shares * price.unwrap_or(0.0))
                    .sum::<f64>();
            let mut turnover_value = 0.0;
            for index in 0..histories.len() {
                let price = trade_prices[index].unwrap_or(0.0);
                let target_value = if price > 0.0 && active_set.contains(&index) {
                    tradable_equity_before * target_weight
                } else {
                    0.0
                };
                if price > 0.0 {
                    let current_value = holdings[index] * price;
                    turnover_value += (target_value - current_value).abs();
                }
            }

            let transaction_cost = turnover_value * transaction_cost_rate;
            let investable_equity = (tradable_equity_before - transaction_cost).max(0.0);
            cash = investable_equity;
            for (index, price) in trade_prices.iter().enumerate() {
                if price.is_some() {
                    holdings[index] = 0.0;
                }
            }
            for index in active.selected_indices.iter().copied() {
                if let Some(price) = trade_prices[index].filter(|value| *value > 0.0) {
                    let target_value = investable_equity * target_weight;
                    holdings[index] = target_value / price;
                    cash -= target_value;
                }
            }
            total_transaction_cost += transaction_cost;
            total_turnover += turnover_value / equity_before_rebalance;
            rebalance_dates.push(date);
            selections.push(RebalanceSelection {
                signal_date,
                date,
                selected_indices: active.selected_indices,
                eligible_indices: active.eligible_indices,
            });
            last_rebalance_date = Some(date);
        }

        let equity = cash
            + holdings
                .iter()
                .zip(valuation_prices.iter())
                .map(|(shares, price)| shares * price.unwrap_or(0.0))
                .sum::<f64>();
        equity_curve.push(EquityPoint {
            date: date.format("%Y-%m-%d").to_string(),
            equity,
        });
        previous_trade_date = Some(date);
    }

    Ok(PortfolioSimulation {
        equity_curve,
        rebalance_dates,
        total_transaction_cost,
        total_turnover,
        selections,
    })
}

fn should_rebalance(
    date: NaiveDate,
    last_rebalance_date: Option<NaiveDate>,
    frequency: &str,
) -> bool {
    let Some(previous) = last_rebalance_date else {
        return true;
    };
    match frequency {
        "none" => false,
        "quarterly" => date.year() != previous.year() || quarter(date) != quarter(previous),
        _ => date.year() != previous.year() || date.month() != previous.month(),
    }
}

fn quarter(date: NaiveDate) -> u32 {
    (date.month0() / 3) + 1
}
