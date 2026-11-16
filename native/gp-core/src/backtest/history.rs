use crate::data_source::{history_points_from_bars, MarketDataSource};
use crate::dates::parse_date;
use crate::error::{CoreError, CoreResult};
use crate::market_data::{HistoryBar, StockFactorSnapshot};
use chrono::NaiveDate;
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug)]
pub(super) struct BacktestHistory {
    pub(super) code: String,
    pub(super) prices: BTreeMap<NaiveDate, f64>,
    pub(super) bars: Vec<HistoryBar>,
}

pub(super) fn strict_walk_forward_history_symbols(
    source: &impl MarketDataSource,
    symbols: &[String],
    start_date: &str,
    end_date: &str,
) -> CoreResult<Vec<String>> {
    let start = parse_date(start_date)?;
    let end = parse_date(end_date)?;
    let mut required = Vec::new();
    let mut missing_snapshots = Vec::new();

    for code in symbols {
        let timeline = load_factor_snapshot_timeline(source, code)?;
        if timeline.is_empty() {
            missing_snapshots.push(code.clone());
            continue;
        }
        let tradable_at_start = timeline
            .range(..=start)
            .next_back()
            .map(|(_, snapshot)| {
                snapshot.is_listed == Some(true) && snapshot.is_tradable == Some(true)
            })
            .unwrap_or(false);
        let becomes_tradable_in_range = timeline.range(start..=end).any(|(_, snapshot)| {
            snapshot.is_listed == Some(true) && snapshot.is_tradable == Some(true)
        });
        if tradable_at_start || becomes_tradable_in_range {
            required.push(code.clone());
        }
    }

    if !missing_snapshots.is_empty() {
        let preview = missing_snapshots
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join("、");
        return Err(CoreError::new(format!(
            "walk_forward 严格回测要求完整的历史因子快照 factor_snapshots；缺少 {} 个：{}",
            missing_snapshots.len(),
            preview
        )));
    }

    Ok(required)
}

pub(super) fn load_backtest_histories(
    source: &impl MarketDataSource,
    symbols: &[String],
    start_date: &str,
    end_date: &str,
) -> CoreResult<Vec<BacktestHistory>> {
    let mut histories = Vec::new();
    for code in symbols {
        let bars = source.get_history(code, start_date, end_date)?;
        let prices: BTreeMap<NaiveDate, f64> = history_points_from_bars(&bars)?
            .into_iter()
            .filter(|point| point.close.is_finite() && point.close > 0.0)
            .map(|point| (point.date, point.close))
            .collect();
        if !prices.is_empty() {
            histories.push(BacktestHistory {
                code: code.clone(),
                prices,
                bars,
            });
        }
    }
    Ok(histories)
}

pub(super) fn load_factor_snapshot_timeline(
    source: &impl MarketDataSource,
    code: &str,
) -> CoreResult<BTreeMap<NaiveDate, StockFactorSnapshot>> {
    let mut timeline = BTreeMap::new();
    for snapshot in source.get_factor_snapshots(code)? {
        let available_date = snapshot.available_date.as_deref().ok_or_else(|| {
            CoreError::new(format!(
                "历史因子快照缺少 available_date，无法判断数据何时可用：{code} {}",
                snapshot.date
            ))
        })?;
        let report_date = parse_date(&snapshot.date).map_err(|_| {
            CoreError::new(format!(
                "历史因子快照报告期 date 无效：{code} {}",
                snapshot.date
            ))
        })?;
        if snapshot.is_st.is_none()
            || snapshot.is_listed.is_none()
            || snapshot.is_tradable.is_none()
        {
            return Err(CoreError::new(format!(
                "历史因子快照缺少 is_st/is_listed/is_tradable 状态：{code} {available_date}"
            )));
        }
        let availability_date = parse_date(available_date).map_err(|_| {
            CoreError::new(format!(
                "历史因子快照 available_date 无效：{code} {available_date}"
            ))
        })?;
        if availability_date < report_date {
            return Err(CoreError::new(format!(
                "历史因子快照 available_date 早于报告期 date：{code} {} 早于 {}",
                available_date, snapshot.date
            )));
        }
        timeline.insert(availability_date, snapshot);
    }
    Ok(timeline)
}

pub(super) fn latest_factor_snapshot_on_or_before<'a>(
    snapshots_by_code: &'a HashMap<String, BTreeMap<NaiveDate, StockFactorSnapshot>>,
    code: &str,
    date: NaiveDate,
) -> Option<&'a StockFactorSnapshot> {
    snapshots_by_code
        .get(code)
        .or_else(|| snapshots_by_code.get(&code.to_uppercase()))
        .and_then(|timeline| {
            timeline
                .range(..=date)
                .next_back()
                .map(|(_, snapshot)| snapshot)
        })
}

pub(super) fn backtest_calendar(histories: &[BacktestHistory]) -> Vec<NaiveDate> {
    let mut dates = BTreeMap::new();
    for history in histories {
        for date in history.prices.keys() {
            dates.insert(*date, ());
        }
    }
    dates.into_keys().collect()
}

pub(super) fn latest_price_on_or_before(history: &BacktestHistory, date: NaiveDate) -> Option<f64> {
    history
        .prices
        .range(..=date)
        .next_back()
        .map(|(_, price)| *price)
}

pub(super) fn price_on_date(history: &BacktestHistory, date: NaiveDate) -> Option<f64> {
    history.prices.get(&date).copied()
}
