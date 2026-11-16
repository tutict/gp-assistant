use crate::adaptive_screen::{adaptive_screen_stocks_with_parameters, AdaptiveScreenRequest};
use crate::backtest::history::{latest_factor_snapshot_on_or_before, BacktestHistory};
use crate::backtest::request::{
    adaptive_backtest_requested_mode, backtest_scoring_parameters, is_adaptive_backtest,
    normalize_backtest_source, BacktestRequest,
};
use crate::dates::parse_date;
use crate::error::{CoreError, CoreResult};
use crate::market_data::{normalize_stock_code, HistoryBar, StockFactorSnapshot, StockItem};
use crate::screening::criteria::matches_stock;
use crate::screening::scoring::score_stock;
use crate::screening::service::screen_stocks;
use crate::screening::types::ScreenedStock;
use chrono::NaiveDate;
use std::collections::{BTreeMap, HashMap, HashSet};

pub fn backtest_selected_symbols(universe: &[StockItem], request: &BacktestRequest) -> Vec<String> {
    if is_adaptive_backtest(request) {
        return universe
            .iter()
            .filter(|stock| matches_stock(stock, &request.criteria).is_some())
            .map(|stock| stock.code.clone())
            .collect();
    }
    selected_backtest_items_from_universe(universe, request)
        .0
        .into_iter()
        .map(|item| item.stock.code)
        .collect()
}

#[derive(Clone, Debug)]
pub(super) struct ActiveSelection {
    pub(super) selected_indices: Vec<usize>,
    pub(super) eligible_indices: Vec<usize>,
}

pub(super) fn walk_forward_active_indices(
    universe: &[StockItem],
    request: &BacktestRequest,
    snapshots_by_code: &HashMap<String, BTreeMap<NaiveDate, StockFactorSnapshot>>,
    history_index: &HashMap<String, usize>,
    current_prices: &[Option<f64>],
    date: NaiveDate,
) -> ActiveSelection {
    if normalize_backtest_source(&request.source) == "watchlist" {
        let eligible_indices = dedupe_stock_codes(&request.stock_codes)
            .into_iter()
            .filter_map(|code| {
                let index = history_index.get(&code.to_uppercase()).copied()?;
                let snapshot = latest_factor_snapshot_on_or_before(snapshots_by_code, &code, date)?;
                if snapshot.is_listed != Some(true)
                    || snapshot.is_tradable != Some(true)
                    || (!request.criteria.include_st && snapshot.is_st == Some(true))
                {
                    return None;
                }
                current_prices
                    .get(index)
                    .and_then(|price| *price)
                    .filter(|price| price.is_finite() && *price > 0.0)?;
                Some(index)
            })
            .collect::<Vec<_>>();
        let selected_indices = eligible_indices
            .iter()
            .take(request.top_n.clamp(1, 100))
            .copied()
            .collect();
        return ActiveSelection {
            eligible_indices,
            selected_indices,
        };
    }

    let visible_universe = point_in_time_universe(
        universe,
        snapshots_by_code,
        history_index,
        current_prices,
        date,
    );
    let selected = selected_backtest_items_from_universe(&visible_universe, request).0;
    let eligible_indices = visible_universe
        .iter()
        .filter_map(|stock| history_index.get(&stock.code.to_uppercase()).copied())
        .collect::<Vec<_>>();
    let mut indices = Vec::new();
    for item in selected.into_iter().take(request.top_n.clamp(1, 100)) {
        let code = item.stock.code.to_uppercase();
        if let Some(index) = history_index.get(&code).copied() {
            if current_prices
                .get(index)
                .and_then(|price| *price)
                .filter(|price| *price > 0.0)
                .is_some()
            {
                indices.push(index);
            }
        }
    }
    ActiveSelection {
        selected_indices: indices,
        eligible_indices,
    }
}

pub(super) fn adaptive_walk_forward_active_indices(
    universe: &[StockItem],
    request: &BacktestRequest,
    histories: &[BacktestHistory],
    benchmark_histories: &HashMap<String, Vec<HistoryBar>>,
    snapshots_by_code: &HashMap<String, BTreeMap<NaiveDate, StockFactorSnapshot>>,
    history_index: &HashMap<String, usize>,
    current_prices: &[Option<f64>],
    date: NaiveDate,
) -> CoreResult<ActiveSelection> {
    let mut visible_universe = point_in_time_universe(
        universe,
        snapshots_by_code,
        history_index,
        current_prices,
        date,
    );
    let eligible_indices = visible_universe
        .iter()
        .filter_map(|stock| history_index.get(&stock.code.to_uppercase()).copied())
        .collect::<Vec<_>>();
    if visible_universe.is_empty() {
        return Ok(ActiveSelection {
            eligible_indices,
            selected_indices: Vec::new(),
        });
    }

    let mut point_in_time_histories = HashMap::new();
    for stock in &mut visible_universe {
        let Some(index) = history_index.get(&stock.code.to_uppercase()).copied() else {
            continue;
        };
        let Some(history) = histories.get(index) else {
            continue;
        };
        let bars = history
            .bars
            .iter()
            .filter(|bar| {
                parse_date(&bar.date)
                    .map(|bar_date| bar_date <= date)
                    .unwrap_or(false)
            })
            .rev()
            .take(120)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>();
        if bars.last().and_then(|bar| parse_date(&bar.date).ok()) != Some(date) {
            return Err(CoreError::new(format!(
                "adaptive_swing_v1 调仓信号日缺少股票日线：{} {}",
                stock.code,
                date.format("%Y-%m-%d")
            )));
        }
        if bars.len() >= 2 {
            let latest = bars[bars.len() - 1].close;
            let previous = bars[bars.len() - 2].close;
            stock.change_pct = (previous > 0.0).then_some(latest / previous - 1.0);
        }
        stock.quote_time = Some(date.format("%Y-%m-%d").to_string());
        point_in_time_histories.insert(stock.code.clone(), bars);
    }
    let point_in_time_benchmarks = benchmark_histories
        .iter()
        .filter_map(|(code, bars)| {
            let visible = bars
                .iter()
                .filter(|bar| {
                    parse_date(&bar.date)
                        .map(|bar_date| bar_date <= date)
                        .unwrap_or(false)
                })
                .rev()
                .take(120)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>();
            (visible.last().and_then(|bar| parse_date(&bar.date).ok()) == Some(date))
                .then(|| (code.clone(), visible))
        })
        .collect::<HashMap<_, _>>();
    let adaptive_request = AdaptiveScreenRequest {
        criteria: request.criteria.clone(),
        mode: adaptive_backtest_requested_mode(&request.strategy_mode),
        horizon: "swing_10_30d".to_string(),
        primary_limit: request.top_n.clamp(1, 100),
        exploration_limit: 0,
        run_id: None,
        as_of_date: Some(date.format("%Y%m%d").to_string()),
    };
    let parameters = backtest_scoring_parameters(request)?
        .ok_or_else(|| CoreError::new("adaptive backtest requires adaptive scoring parameters"))?;
    let selected_indices = adaptive_screen_stocks_with_parameters(
        &visible_universe,
        &point_in_time_histories,
        &point_in_time_benchmarks,
        &[],
        &adaptive_request,
        &parameters,
    )
    .map_err(|error| {
        CoreError::new(format!(
            "adaptive_swing_v1 调仓日 {} 数据不足：{}",
            date.format("%Y-%m-%d"),
            error
        ))
    })?
    .items
    .into_iter()
    .filter_map(|item| history_index.get(&item.stock.code.to_uppercase()).copied())
    .collect::<Vec<_>>();
    Ok(ActiveSelection {
        selected_indices,
        eligible_indices,
    })
}

pub(super) fn point_in_time_universe(
    universe: &[StockItem],
    snapshots_by_code: &HashMap<String, BTreeMap<NaiveDate, StockFactorSnapshot>>,
    history_index: &HashMap<String, usize>,
    current_prices: &[Option<f64>],
    date: NaiveDate,
) -> Vec<StockItem> {
    universe
        .iter()
        .filter_map(|stock| {
            let snapshot =
                latest_factor_snapshot_on_or_before(snapshots_by_code, &stock.code, date)?;
            if snapshot.is_listed != Some(true) || snapshot.is_tradable != Some(true) {
                return None;
            }
            snapshot.is_st?;
            let index = history_index.get(&stock.code.to_uppercase()).copied()?;
            let price = current_prices
                .get(index)
                .and_then(|price| *price)
                .filter(|price| price.is_finite() && *price > 0.0)?;
            Some(apply_factor_snapshot(stock, snapshot, price))
        })
        .collect()
}

fn apply_factor_snapshot(
    stock: &StockItem,
    snapshot: &StockFactorSnapshot,
    historical_price: f64,
) -> StockItem {
    let finite = |value: Option<f64>| value.filter(|value| value.is_finite());
    StockItem {
        code: stock.code.clone(),
        name: snapshot.name.clone().unwrap_or_default(),
        industry: snapshot.industry.clone().unwrap_or_default(),
        is_st: snapshot.is_st.unwrap_or(true),
        price: historical_price,
        pe: finite(snapshot.pe),
        pb: finite(snapshot.pb),
        roe: finite(snapshot.roe),
        market_cap_billion: finite(snapshot.market_cap_billion),
        dividend_yield: finite(snapshot.dividend_yield),
        latest_eps: None,
        financial_coverage: None,
        deducted_net_profit_billion: finite(snapshot.deducted_net_profit_billion),
        deducted_net_profit_margin: finite(snapshot.deducted_net_profit_margin),
        deducted_net_profit_growth_rate: finite(snapshot.deducted_net_profit_growth_rate),
        change_pct: finite(snapshot.change_pct),
        volume: finite(snapshot.volume),
        amount: finite(snapshot.amount),
        turnover_rate: finite(snapshot.turnover_rate),
        volume_ratio: finite(snapshot.volume_ratio),
        quote_time: snapshot.available_date.clone(),
    }
}

pub(super) fn selected_backtest_items(
    universe: &[StockItem],
    request: &BacktestRequest,
) -> (Vec<ScreenedStock>, Vec<String>) {
    selected_backtest_items_from_universe(universe, request)
}

fn selected_backtest_items_from_universe(
    universe: &[StockItem],
    request: &BacktestRequest,
) -> (Vec<ScreenedStock>, Vec<String>) {
    if normalize_backtest_source(&request.source) != "watchlist" {
        let screened = screen_stocks(universe, &request.criteria);
        return (
            screened
                .items
                .into_iter()
                .take(request.top_n.clamp(1, 100))
                .collect(),
            Vec::new(),
        );
    }

    let codes = dedupe_stock_codes(&request.stock_codes);
    if codes.is_empty() {
        return (
            Vec::new(),
            vec!["自选观察池为空，未执行固定标的回测。".to_string()],
        );
    }

    let by_code: HashMap<String, StockItem> = universe
        .iter()
        .map(|stock| (stock.code.to_uppercase(), stock.clone()))
        .collect();
    let mut selected = Vec::new();
    let mut missing = Vec::new();
    for code in codes.iter().take(request.top_n.clamp(1, 100)) {
        if let Some(stock) = by_code.get(code) {
            selected.push(score_stock(stock, &["watchlist".to_string()], "quality"));
        } else {
            missing.push(code.clone());
        }
    }

    let mut notes = vec![format!(
        "回测标的来源：自选观察池，已选择 {} / {} 只。",
        selected.len(),
        codes.len()
    )];
    if !missing.is_empty() {
        let sample = missing
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let suffix = if missing.len() > 5 { " 等" } else { "" };
        notes.push(format!(
            "自选观察池中 {} 只股票不在当前股票池：{}{}。",
            missing.len(),
            sample,
            suffix
        ));
    }
    (selected, notes)
}

fn dedupe_stock_codes(codes: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for code in codes {
        let normalized = normalize_stock_code(code);
        if normalized.is_empty() || !seen.insert(normalized.clone()) {
            continue;
        }
        result.push(normalized);
    }
    result
}
