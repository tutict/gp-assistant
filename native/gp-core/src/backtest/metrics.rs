use crate::backtest::history::{
    backtest_calendar, latest_price_on_or_before, price_on_date, BacktestHistory,
};
use crate::backtest::simulation::RebalanceSelection;
use crate::backtest::types::{BacktestMetrics, EquityPoint, WalkForwardFold};
use chrono::NaiveDate;
use std::collections::HashSet;

pub(super) fn empty_backtest_metrics(num_stocks: usize, strategy_mode: &str) -> BacktestMetrics {
    BacktestMetrics {
        total_return: 0.0,
        annualized_return: None,
        max_drawdown: None,
        num_stocks,
        benchmark_total_return: None,
        benchmark_annualized_return: None,
        benchmark_max_drawdown: None,
        excess_return: None,
        total_transaction_cost: 0.0,
        total_turnover: 0.0,
        rebalance_count: 0,
        oos_fold_count: 0,
        evaluated_selection_count: 0,
        selection_hit_count: 0,
        precision_at_n: None,
        strategy_mode: strategy_mode.to_string(),
    }
}

pub(super) fn equal_weight_benchmark_curve(
    histories: &[BacktestHistory],
    initial_cash: f64,
    start_date: Option<NaiveDate>,
) -> Vec<EquityPoint> {
    backtest_calendar(histories)
        .into_iter()
        .filter(|date| start_date.is_none_or(|start| *date >= start))
        .filter_map(|date| {
            let values: Vec<f64> = histories
                .iter()
                .filter_map(|history| {
                    let first = match start_date {
                        Some(start) => history
                            .prices
                            .range(start..)
                            .next()
                            .map(|(_, price)| *price)?,
                        None => history.prices.values().next().copied()?,
                    };
                    let price = latest_price_on_or_before(history, date)?;
                    if first > 0.0 && price > 0.0 {
                        Some(price / first)
                    } else {
                        None
                    }
                })
                .collect();
            if values.is_empty() {
                None
            } else {
                Some(EquityPoint {
                    date: date.format("%Y-%m-%d").to_string(),
                    equity: initial_cash * values.iter().sum::<f64>() / values.len() as f64,
                })
            }
        })
        .collect()
}

pub(super) fn selected_symbols_from_latest_rebalance(
    histories: &[BacktestHistory],
    selections: &[RebalanceSelection],
) -> Vec<String> {
    selections
        .last()
        .map(|selection| {
            selection
                .selected_indices
                .iter()
                .filter_map(|index| histories.get(*index).map(|history| history.code.clone()))
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn selected_symbols_from_rebalances(
    histories: &[BacktestHistory],
    selections: &[RebalanceSelection],
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut symbols = Vec::new();
    for selection in selections {
        for index in &selection.selected_indices {
            let Some(history) = histories.get(*index) else {
                continue;
            };
            if seen.insert(history.code.clone()) {
                symbols.push(history.code.clone());
            }
        }
    }
    symbols
}

pub(super) fn evaluate_walk_forward_folds(
    histories: &[BacktestHistory],
    selections: &[RebalanceSelection],
) -> Vec<WalkForwardFold> {
    let final_date = backtest_calendar(histories).last().copied();
    selections
        .iter()
        .enumerate()
        .map(|(position, selection)| {
            let evaluation_end = selections
                .get(position + 1)
                .map(|next| next.date)
                .or(final_date)
                .filter(|end| *end > selection.date);
            let selected_symbols = selection
                .selected_indices
                .iter()
                .filter_map(|index| histories.get(*index).map(|history| history.code.clone()))
                .collect::<Vec<_>>();
            let selected_returns = evaluation_end
                .map(|end| {
                    selection
                        .selected_indices
                        .iter()
                        .map(|index| {
                            histories
                                .get(*index)
                                .and_then(|history| forward_return(history, selection.date, end))
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let benchmark_returns = evaluation_end
                .map(|end| {
                    selection
                        .eligible_indices
                        .iter()
                        .filter_map(|index| {
                            forward_return(histories.get(*index)?, selection.date, end)
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let benchmark_forward_return = mean(&benchmark_returns);
            let realized_selected_returns = selected_returns
                .iter()
                .filter_map(|value| *value)
                .collect::<Vec<_>>();
            let average_forward_return = mean(&realized_selected_returns);
            let (evaluated_selection_count, hit_count, precision_at_n) =
                if let Some(benchmark) = benchmark_forward_return {
                    let hit_count = selected_returns
                        .iter()
                        .filter_map(|value| *value)
                        .filter(|value| *value > benchmark)
                        .count();
                    let evaluated = selected_returns.len();
                    let precision = (evaluated > 0).then_some(hit_count as f64 / evaluated as f64);
                    (evaluated, hit_count, precision)
                } else {
                    (0, 0, None)
                };
            WalkForwardFold {
                signal_date: Some(selection.signal_date.format("%Y-%m-%d").to_string()),
                selection_date: selection.date.format("%Y-%m-%d").to_string(),
                evaluation_end_date: evaluation_end.map(|date| date.format("%Y-%m-%d").to_string()),
                selected_symbols,
                eligible_symbol_count: selection.eligible_indices.len(),
                evaluated_selection_count,
                hit_count,
                precision_at_n,
                average_forward_return,
                benchmark_forward_return,
                average_excess_return: average_forward_return
                    .zip(benchmark_forward_return)
                    .map(|(selected, benchmark)| selected - benchmark),
            }
        })
        .collect()
}

pub(super) fn forward_return(
    history: &BacktestHistory,
    start: NaiveDate,
    end: NaiveDate,
) -> Option<f64> {
    let start_price = price_on_date(history, start)?;
    let end_price = price_on_date(history, end)?;
    (start_price > 0.0 && end_price > 0.0).then_some(end_price / start_price - 1.0)
}

fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then_some(values.iter().sum::<f64>() / values.len() as f64)
}

pub(super) fn backtest_metrics(
    equity_curve: &[EquityPoint],
    initial_cash: f64,
) -> (f64, Option<f64>, Option<f64>) {
    let Some(first) = equity_curve.first() else {
        return (0.0, None, None);
    };
    let Some(last) = equity_curve.last() else {
        return (0.0, None, None);
    };

    let denominator = if initial_cash.is_finite() && initial_cash > 0.0 {
        initial_cash
    } else {
        first.equity
    };
    let total_return = last.equity / denominator - 1.0;
    let first_date = match NaiveDate::parse_from_str(&first.date, "%Y-%m-%d") {
        Ok(date) => date,
        Err(_) => return (total_return, None, None),
    };
    let last_date = match NaiveDate::parse_from_str(&last.date, "%Y-%m-%d") {
        Ok(date) => date,
        Err(_) => return (total_return, None, None),
    };
    let days = (last_date - first_date).num_days().max(1) as f64;
    let years = days / 365.0;
    let annualized_return = if years > 0.0 {
        Some((1.0 + total_return).powf(1.0 / years) - 1.0)
    } else {
        None
    };

    let mut rolling_max = first.equity;
    let mut max_drawdown = 0.0_f64;
    for point in equity_curve {
        rolling_max = rolling_max.max(point.equity);
        if rolling_max > 0.0 {
            max_drawdown = max_drawdown.min(point.equity / rolling_max - 1.0);
        }
    }

    (total_return, annualized_return, Some(max_drawdown))
}
