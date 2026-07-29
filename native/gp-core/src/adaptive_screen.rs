use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::{
    concept_group_for_stock, matches_stock, round6, CoreError, CoreResult, HistoryBar,
    ScoreContribution, ScreenCriteria, ScreenResultGroup, ScreenedStock, StockItem,
};

pub const ADAPTIVE_ALGORITHM_VERSION: &str = "adaptive_swing_v1";
const SCORE_SCALE: f64 = 20.0;
const MIN_HISTORY_BARS: usize = 60;
const MIN_MARKET_BREADTH_COVERAGE: f64 = 0.60;
const MIN_MARKET_BREADTH_OBSERVATIONS: usize = 10;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdaptiveScreenRequest {
    #[serde(default)]
    pub criteria: ScreenCriteria,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default = "default_horizon")]
    pub horizon: String,
    #[serde(default = "default_primary_limit")]
    pub primary_limit: usize,
    #[serde(default = "default_exploration_limit")]
    pub exploration_limit: usize,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub as_of_date: Option<String>,
}

impl Default for AdaptiveScreenRequest {
    fn default() -> Self {
        Self {
            criteria: ScreenCriteria::default(),
            mode: default_mode(),
            horizon: default_horizon(),
            primary_limit: default_primary_limit(),
            exploration_limit: default_exploration_limit(),
            run_id: None,
            as_of_date: None,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AdaptiveRecentExposure {
    pub code: String,
    #[serde(default)]
    pub trade_date: String,
    #[serde(default)]
    pub bucket: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdaptiveRegimeEvidence {
    pub key: String,
    pub label: String,
    pub value: f64,
    pub summary: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AdaptiveDataCoverage {
    pub candidate_requested: usize,
    pub candidate_usable: usize,
    pub candidate_ratio: f64,
    pub benchmark_requested: usize,
    pub benchmark_usable: usize,
    pub breadth_usable: bool,
    #[serde(default)]
    pub breadth_requested: usize,
    #[serde(default)]
    pub breadth_observed: usize,
    #[serde(default)]
    pub breadth_coverage_ratio: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdaptiveMarketRegime {
    pub detected: String,
    pub effective: String,
    pub confidence: f64,
    pub overridden: bool,
    pub as_of_date: Option<String>,
    pub evidence: Vec<AdaptiveRegimeEvidence>,
    pub coverage: AdaptiveDataCoverage,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdaptiveScreenResult {
    pub algorithm_version: String,
    pub total: usize,
    pub returned: usize,
    pub items: Vec<ScreenedStock>,
    pub groups: Vec<ScreenResultGroup>,
    pub market_regime: AdaptiveMarketRegime,
    pub notes: Vec<String>,
}

fn default_mode() -> String {
    "auto".to_string()
}

fn default_horizon() -> String {
    "swing_10_30d".to_string()
}

fn default_primary_limit() -> usize {
    10
}

fn default_exploration_limit() -> usize {
    10
}

pub fn adaptive_candidate_codes(
    universe: &[StockItem],
    criteria: &ScreenCriteria,
    limit: usize,
) -> Vec<String> {
    stage_one_candidates(universe, criteria, limit)
        .into_iter()
        .map(|stock| stock.code.clone())
        .collect()
}

fn stage_one_candidates<'a>(
    universe: &'a [StockItem],
    criteria: &ScreenCriteria,
    limit: usize,
) -> Vec<&'a StockItem> {
    let limit = limit.clamp(1, 200);
    let mut eligible = universe
        .iter()
        .filter(|stock| !stock.is_st)
        .filter(|stock| matches_stock(stock, criteria).is_some())
        .filter(|stock| has_required_slow_data(stock))
        .collect::<Vec<_>>();
    eligible.sort_by(|left, right| {
        slow_score(right)
            .total_cmp(&slow_score(left))
            .then_with(|| left.code.cmp(&right.code))
    });

    let mut selected = Vec::new();
    let mut used = HashSet::new();
    let mut counts = HashMap::<String, usize>::new();
    for stock in &eligible {
        let industry = normalized_industry(stock);
        if counts.get(&industry).copied().unwrap_or(0) >= 2 {
            continue;
        }
        selected.push(*stock);
        used.insert(stock.code.clone());
        *counts.entry(industry).or_default() += 1;
        if selected.len() >= limit {
            return selected;
        }
    }
    for stock in eligible {
        if used.contains(&stock.code) {
            continue;
        }
        let industry = normalized_industry(stock);
        if counts.get(&industry).copied().unwrap_or(0) >= 4 {
            continue;
        }
        selected.push(stock);
        *counts.entry(industry).or_default() += 1;
        if selected.len() >= limit {
            break;
        }
    }
    selected
}

fn has_required_slow_data(stock: &StockItem) -> bool {
    stock.price.is_finite()
        && stock.price > 0.0
        && stock.roe.filter(|value| value.is_finite()).is_some()
        && (stock
            .pe
            .filter(|value| value.is_finite() && *value > 0.0)
            .is_some()
            || stock
                .pb
                .filter(|value| value.is_finite() && *value > 0.0)
                .is_some())
        && stock
            .market_cap_billion
            .filter(|value| value.is_finite() && *value > 0.0)
            .is_some()
}

fn slow_score(stock: &StockItem) -> f64 {
    quality_factor(stock) * 0.45
        + valuation_factor(stock) * 0.30
        + liquidity_factor(stock) * 0.15
        + completeness_factor(stock) * 0.10
}

fn normalized_industry(stock: &StockItem) -> String {
    let value = stock.industry.trim();
    if value.is_empty() {
        "__unknown__".to_string()
    } else {
        value.to_string()
    }
}

fn as_percent(value: f64) -> f64 {
    if value.abs() <= 1.0 {
        value * 100.0
    } else {
        value
    }
}

fn quality_factor(stock: &StockItem) -> f64 {
    let roe = stock.roe.map(as_percent).unwrap_or(-100.0);
    let roe_score = ((roe + 2.0) / 22.0).clamp(0.0, 1.0);
    let profit_score = stock
        .deducted_net_profit_billion
        .filter(|value| value.is_finite())
        .map(|value| ((value + 1.0) / 11.0).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let growth_score = stock
        .deducted_net_profit_growth_rate
        .filter(|value| value.is_finite())
        .map(|value| ((as_percent(value) + 20.0) / 60.0).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    (roe_score * 0.65 + profit_score * 0.20 + growth_score * 0.15).clamp(0.0, 1.0)
}

fn valuation_factor(stock: &StockItem) -> f64 {
    let mut values = Vec::new();
    if let Some(pe) = stock.pe.filter(|value| value.is_finite() && *value > 0.0) {
        values.push(match pe {
            value if value <= 15.0 => 1.0,
            value if value <= 30.0 => 0.75,
            value if value <= 60.0 => 0.45,
            _ => 0.20,
        });
    }
    if let Some(pb) = stock.pb.filter(|value| value.is_finite() && *value > 0.0) {
        values.push(match pb {
            value if value <= 1.5 => 1.0,
            value if value <= 3.0 => 0.75,
            value if value <= 6.0 => 0.45,
            _ => 0.20,
        });
    }
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn liquidity_factor(stock: &StockItem) -> f64 {
    let cap = stock
        .market_cap_billion
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|value| (value.ln() / 7.0).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let amount = stock
        .amount
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|value| (value / 1_000_000_000.0).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let turnover = stock
        .turnover_rate
        .filter(|value| value.is_finite())
        .map(|value| {
            let pct = as_percent(value);
            if (0.5..=6.0).contains(&pct) {
                1.0
            } else if pct <= 12.0 {
                0.6
            } else {
                0.25
            }
        })
        .unwrap_or(0.0);
    (cap * 0.45 + amount * 0.35 + turnover * 0.20).clamp(0.0, 1.0)
}

fn completeness_factor(stock: &StockItem) -> f64 {
    let present = [
        stock.roe,
        stock.pe,
        stock.pb,
        stock.market_cap_billion,
        stock.deducted_net_profit_billion,
        stock.deducted_net_profit_growth_rate,
        stock.dividend_yield,
    ]
    .iter()
    .filter(|value| value.filter(|item| item.is_finite()).is_some())
    .count();
    present as f64 / 7.0
}

#[derive(Clone, Debug)]
struct BenchmarkStats {
    return_20: f64,
    ma_spread: f64,
    efficiency: f64,
    atr_percentile: f64,
}

#[derive(Clone, Copy, Debug)]
struct MarketBreadth {
    advancing_ratio: f64,
    requested: usize,
    observed: usize,
    coverage_ratio: f64,
}

impl MarketBreadth {
    fn is_usable(self) -> bool {
        self.observed >= MIN_MARKET_BREADTH_OBSERVATIONS
            && self.coverage_ratio + f64::EPSILON >= MIN_MARKET_BREADTH_COVERAGE
    }
}

fn detect_regime(
    universe: &[StockItem],
    benchmarks: &HashMap<String, Vec<HistoryBar>>,
    request: &AdaptiveScreenRequest,
    candidate_requested: usize,
    candidate_usable: usize,
) -> CoreResult<AdaptiveMarketRegime> {
    let stats = benchmarks
        .values()
        .filter_map(|bars| benchmark_stats(bars))
        .collect::<Vec<_>>();
    let requested = 3;
    let requested_mode = normalize_mode(&request.mode);
    if requested_mode == "auto" && stats.len() < 2 {
        return Err(CoreError::new(
            "市场状态数据不足：自动模式至少需要两个有效宽基指数。",
        ));
    }
    let breadth = market_breadth(universe, request.as_of_date.as_deref());
    let usable_breadth = breadth.filter(|value| value.is_usable());
    let returns = stats.iter().map(|item| item.return_20).collect::<Vec<_>>();
    let spreads = stats.iter().map(|item| item.ma_spread).collect::<Vec<_>>();
    let efficiencies = stats.iter().map(|item| item.efficiency).collect::<Vec<_>>();
    let atrs = stats
        .iter()
        .map(|item| item.atr_percentile)
        .collect::<Vec<_>>();
    let median_return = median(&returns);
    let median_spread = median(&spreads);
    let median_efficiency = median(&efficiencies);
    let median_atr = median(&atrs);
    let trend_votes = stats
        .iter()
        .filter(|item| item.return_20 > 0.03 && item.ma_spread > 0.02)
        .count();
    let defensive_votes = stats
        .iter()
        .filter(|item| item.return_20 < -0.03 && item.ma_spread < -0.02)
        .count();
    let direction_consistency = if stats.is_empty() {
        None
    } else {
        let positive = returns.iter().filter(|value| **value > 0.0).count();
        let negative = returns.iter().filter(|value| **value < 0.0).count();
        Some(positive.max(negative) as f64 / stats.len() as f64)
    };
    let conflicting =
        returns.iter().any(|value| *value > 0.03) && returns.iter().any(|value| *value < -0.03);
    if requested_mode == "auto" && usable_breadth.is_none() {
        return Err(CoreError::new(
            "市场状态数据不足：自动模式的全市场上涨家数覆盖率不足60%或有效样本少于10只。",
        ));
    }
    let sufficient_for_detection = stats.len() >= 2 && usable_breadth.is_some();
    let detected = if !sufficient_for_detection {
        "insufficient"
    } else {
        let breadth = usable_breadth.expect("checked above").advancing_ratio;
        let median_return = median_return.expect("two valid benchmarks");
        let median_spread = median_spread.expect("two valid benchmarks");
        let median_efficiency = median_efficiency.expect("two valid benchmarks");
        let median_atr = median_atr.expect("two valid benchmarks");
        if median_atr > 0.75 || conflicting {
            "transition"
        } else if trend_votes >= 2 && breadth > 0.50 {
            "trend"
        } else if defensive_votes >= 2 || breadth < 0.40 {
            "defensive"
        } else if median_return.abs() <= 0.05
            && median_spread.abs() <= 0.03
            && median_efficiency <= 0.35
            && (0.35..=0.65).contains(&breadth)
        {
            "range"
        } else {
            "transition"
        }
    };
    let effective = if requested_mode == "auto" {
        detected
    } else {
        requested_mode
    };
    let confidence = match detected {
        "insufficient" => 0.0,
        "transition" => 0.55,
        _ => 0.78,
    };
    let mut evidence = Vec::new();
    if let Some(value) = median_return {
        evidence.push(regime_evidence(
            "return_20",
            "宽基20日中位收益",
            value,
            "判断方向与区间",
        ));
    }
    if let Some(value) = median_spread {
        evidence.push(regime_evidence(
            "ma_spread",
            "MA20/MA60中位偏离",
            value,
            "判断趋势结构",
        ));
    }
    if let Some(value) = median_efficiency {
        evidence.push(regime_evidence(
            "efficiency",
            "趋势效率",
            value,
            "识别震荡噪声",
        ));
    }
    if let Some(value) = direction_consistency {
        evidence.push(regime_evidence(
            "direction_consistency",
            "三指数方向一致度",
            value,
            "识别指数分化",
        ));
    }
    if let Some(value) = median_atr {
        evidence.push(regime_evidence(
            "atr_percentile",
            "ATR历史分位",
            value,
            "识别高波动过渡",
        ));
    }
    if let Some(value) = usable_breadth {
        evidence.push(regime_evidence(
            "breadth",
            "上涨家数比例",
            value.advancing_ratio,
            "衡量全市场宽度",
        ));
    }
    if let Some(value) = breadth {
        evidence.push(regime_evidence(
            "breadth_coverage",
            "市场宽度报价覆盖率",
            value.coverage_ratio,
            "有效涨跌幅样本占全市场快照的比例",
        ));
    }
    Ok(AdaptiveMarketRegime {
        detected: detected.to_string(),
        effective: effective.to_string(),
        confidence,
        overridden: requested_mode != "auto" && requested_mode != detected,
        as_of_date: request.as_of_date.clone(),
        evidence,
        coverage: AdaptiveDataCoverage {
            candidate_requested,
            candidate_usable,
            candidate_ratio: if candidate_requested == 0 {
                0.0
            } else {
                round6(candidate_usable as f64 / candidate_requested as f64)
            },
            benchmark_requested: requested,
            benchmark_usable: stats.len(),
            breadth_usable: usable_breadth.is_some(),
            breadth_requested: breadth
                .map(|value| value.requested)
                .unwrap_or(universe.len()),
            breadth_observed: breadth.map(|value| value.observed).unwrap_or(0),
            breadth_coverage_ratio: breadth.map(|value| value.coverage_ratio).unwrap_or(0.0),
        },
    })
}

fn normalize_mode(value: &str) -> &str {
    match value.trim().to_ascii_lowercase().as_str() {
        "range" => "range",
        "trend" => "trend",
        "defensive" => "defensive",
        _ => "auto",
    }
}

fn regime_evidence(key: &str, label: &str, value: f64, summary: &str) -> AdaptiveRegimeEvidence {
    AdaptiveRegimeEvidence {
        key: key.to_string(),
        label: label.to_string(),
        value: round6(value),
        summary: summary.to_string(),
    }
}

fn benchmark_stats(bars: &[HistoryBar]) -> Option<BenchmarkStats> {
    if !has_valid_ohlc_tail(bars, MIN_HISTORY_BARS) {
        return None;
    }
    let closes = valid_closes(bars);
    if closes.len() < MIN_HISTORY_BARS {
        return None;
    }
    let last = *closes.last()?;
    let earlier = closes.get(closes.len().saturating_sub(21)).copied()?;
    let ma20 = mean_tail(&closes, 20)?;
    let ma60 = mean_tail(&closes, 60)?;
    let path = closes
        .windows(2)
        .rev()
        .take(20)
        .map(|pair| (pair[1] - pair[0]).abs())
        .sum::<f64>();
    let displacement = (last - earlier).abs();
    let efficiency = if path <= f64::EPSILON {
        0.0
    } else {
        (displacement / path).clamp(0.0, 1.0)
    };
    Some(BenchmarkStats {
        return_20: last / earlier - 1.0,
        ma_spread: ma20 / ma60 - 1.0,
        efficiency,
        atr_percentile: atr_percentile(bars)?,
    })
}

fn compact_date_key(value: &str) -> Option<String> {
    let digits = value
        .chars()
        .filter(|character| character.is_ascii_digit())
        .collect::<String>();
    (digits.len() >= 8).then(|| digits[..8].to_string())
}

fn market_breadth(universe: &[StockItem], as_of_date: Option<&str>) -> Option<MarketBreadth> {
    let requested = universe.len();
    if requested == 0 {
        return None;
    }
    let changes = universe
        .iter()
        .filter(|stock| {
            as_of_date.is_none_or(|as_of| {
                compact_date_key(as_of)
                    .zip(stock.quote_time.as_deref().and_then(compact_date_key))
                    .is_some_and(|(expected, observed)| expected == observed)
            })
        })
        .filter_map(|stock| stock.change_pct.filter(|value| value.is_finite()))
        .map(as_percent)
        .collect::<Vec<_>>();
    if changes.is_empty() {
        return None;
    }
    let observed = changes.len();
    Some(MarketBreadth {
        advancing_ratio: changes.iter().filter(|value| **value > 0.0).count() as f64
            / observed as f64,
        requested,
        observed,
        coverage_ratio: observed as f64 / requested as f64,
    })
}

fn valid_closes(bars: &[HistoryBar]) -> Vec<f64> {
    bars.iter()
        .filter_map(|bar| (bar.close.is_finite() && bar.close > 0.0).then_some(bar.close))
        .collect()
}

fn has_valid_ohlc_tail(bars: &[HistoryBar], count: usize) -> bool {
    bars.len() >= count
        && bars[bars.len() - count..].iter().all(|bar| {
            let Some(high) = bar.high.filter(|value| value.is_finite()) else {
                return false;
            };
            let Some(low) = bar.low.filter(|value| value.is_finite()) else {
                return false;
            };
            bar.close.is_finite() && bar.close > 0.0 && high >= low
        })
}

fn mean_tail(values: &[f64], count: usize) -> Option<f64> {
    if values.len() < count || count == 0 {
        return None;
    }
    Some(values[values.len() - count..].iter().sum::<f64>() / count as f64)
}

fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    if sorted.is_empty() {
        return None;
    }
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    Some(if sorted.len() % 2 == 0 {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    })
}

fn atr_percentile(bars: &[HistoryBar]) -> Option<f64> {
    let true_ranges = bars
        .windows(2)
        .filter_map(|pair| {
            let previous_close = pair[0].close;
            let bar = &pair[1];
            let high = bar.high.filter(|value| value.is_finite())?;
            let low = bar.low.filter(|value| value.is_finite())?;
            if !previous_close.is_finite()
                || previous_close <= 0.0
                || bar.close <= 0.0
                || high < low
            {
                return None;
            }
            Some(
                (high - low)
                    .max((high - previous_close).abs())
                    .max((low - previous_close).abs())
                    / bar.close,
            )
        })
        .collect::<Vec<_>>();
    let atr_series = true_ranges
        .windows(14)
        .map(|window| window.iter().sum::<f64>() / window.len() as f64)
        .collect::<Vec<_>>();
    if atr_series.len() < 20 {
        return None;
    }
    let current = *atr_series.last().unwrap_or(&0.0);
    let tolerance = current.abs().max(1.0) * 1e-9;
    let below = atr_series
        .iter()
        .filter(|value| **value < current - tolerance)
        .count() as f64;
    let tied = atr_series
        .iter()
        .filter(|value| (**value - current).abs() <= tolerance)
        .count() as f64;
    Some((below + tied * 0.5) / atr_series.len() as f64)
}

#[derive(Clone)]
struct AdaptiveCandidate {
    stock: StockItem,
    factors: BTreeMap<String, f64>,
    score: f64,
    screened: ScreenedStock,
}

fn technical_factors(stock: &StockItem, bars: &[HistoryBar]) -> Option<BTreeMap<String, f64>> {
    if !has_valid_ohlc_tail(bars, MIN_HISTORY_BARS) {
        return None;
    }
    let closes = valid_closes(bars);
    if closes.len() < MIN_HISTORY_BARS {
        return None;
    }
    let latest = *closes.last()?;
    let ma20 = mean_tail(&closes, 20)?;
    let ma60 = mean_tail(&closes, 60)?;
    let earlier20 = closes[closes.len() - 21];
    let earlier3 = closes[closes.len() - 4];
    let return20 = latest / earlier20 - 1.0;
    let return3 = latest / earlier3 - 1.0;
    let daily_return = latest / closes[closes.len() - 2] - 1.0;
    let high10 = closes[closes.len() - 10..]
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let high60 = closes[closes.len() - 60..]
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let low60 = closes[closes.len() - 60..]
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);
    let drawdown10 = latest / high10 - 1.0;
    let drawdown60 = latest / high60 - 1.0;
    let distance_ma20 = latest / ma20 - 1.0;
    let std20 = standard_deviation(&closes[closes.len() - 20..]);
    let percent_b = if std20 <= f64::EPSILON {
        0.5
    } else {
        ((latest - (ma20 - 2.0 * std20)) / (4.0 * std20)).clamp(0.0, 1.0)
    };
    let bollinger_fit = triangular_fit(percent_b, 0.25, 0.65);
    let ma_fit = triangular_fit(distance_ma20, -0.05, 0.03);
    let abnormal_volume = stock
        .volume_ratio
        .filter(|value| value.is_finite())
        .is_some_and(|value| value > 3.0);
    let repair = if daily_return > 0.07 || abnormal_volume {
        0.0
    } else if (-0.12..=-0.03).contains(&drawdown10) && return3 > 0.0 {
        1.0
    } else {
        (0.5 + return3 * 5.0).clamp(0.0, 0.8)
    };
    let volume_fit = gentle_volume_score(bars);
    let broke_low = latest <= low60;
    let range_setup = if broke_low {
        0.0
    } else {
        bollinger_fit * 0.35 + ma_fit * 0.25 + repair * 0.20 + volume_fit * 0.20
    };
    let trend_efficiency = path_efficiency(&closes, 20);
    let trend = ((return20 + 0.08) / 0.20).clamp(0.0, 1.0) * 0.45
        + ((ma20 / ma60 - 0.98) / 0.08).clamp(0.0, 1.0) * 0.35
        + trend_efficiency * 0.20;
    let atr_risk = (1.0 - atr_percent_of_close(bars)? / 0.06).clamp(0.0, 1.0);
    let drawdown_risk = ((drawdown60 + 0.30) / 0.30).clamp(0.0, 1.0);
    let risk = atr_risk * 0.60 + drawdown_risk * 0.40;
    let relative = ((return20 + 0.10) / 0.20).clamp(0.0, 1.0);
    let overheat = overheat_factor(stock, distance_ma20);
    Some(BTreeMap::from([
        ("quality".to_string(), quality_factor(stock)),
        ("valuation".to_string(), valuation_factor(stock)),
        ("range_setup".to_string(), range_setup.clamp(0.0, 1.0)),
        ("trend".to_string(), trend.clamp(0.0, 1.0)),
        ("relative_strength".to_string(), relative),
        ("liquidity".to_string(), liquidity_factor(stock)),
        ("risk".to_string(), risk.clamp(0.0, 1.0)),
        ("stability".to_string(), stability_factor(stock, risk)),
        ("overheat_penalty".to_string(), overheat),
    ]))
}

fn standard_deviation(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    (values
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / values.len() as f64)
        .sqrt()
}

fn triangular_fit(value: f64, lower: f64, upper: f64) -> f64 {
    if (lower..=upper).contains(&value) {
        return 1.0;
    }
    let width = (upper - lower).abs().max(0.01);
    if value < lower {
        (1.0 - (lower - value) / width).clamp(0.0, 1.0)
    } else {
        (1.0 - (value - upper) / width).clamp(0.0, 1.0)
    }
}

fn gentle_volume_score(bars: &[HistoryBar]) -> f64 {
    let volumes = bars
        .iter()
        .filter_map(|bar| bar.volume.filter(|value| value.is_finite() && *value > 0.0))
        .collect::<Vec<_>>();
    if volumes.len() < 21 {
        return 0.0;
    }
    let latest = volumes[volumes.len() - 1];
    let start = volumes.len() - 21;
    let average = volumes[start..volumes.len() - 1].iter().sum::<f64>() / 20.0;
    if average <= f64::EPSILON {
        return 0.0;
    }
    match latest / average {
        ratio if (0.8..=1.8).contains(&ratio) => 1.0,
        ratio if (0.5..=2.5).contains(&ratio) => 0.6,
        _ => 0.2,
    }
}

fn path_efficiency(closes: &[f64], window: usize) -> f64 {
    if closes.len() <= window || window == 0 {
        return 0.0;
    }
    let slice = &closes[closes.len() - window - 1..];
    let path = slice
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .sum::<f64>();
    if path <= f64::EPSILON {
        0.0
    } else {
        ((slice[slice.len() - 1] - slice[0]).abs() / path).clamp(0.0, 1.0)
    }
}

fn atr_percent_of_close(bars: &[HistoryBar]) -> Option<f64> {
    if bars.len() < 15 {
        return None;
    }
    let mut ranges = Vec::new();
    for pair in bars.windows(2).rev().take(14) {
        let previous_close = pair[0].close;
        let current = &pair[1];
        let Some(high) = current.high.filter(|value| value.is_finite()) else {
            continue;
        };
        let Some(low) = current.low.filter(|value| value.is_finite()) else {
            continue;
        };
        let true_range = (high - low)
            .max((high - previous_close).abs())
            .max((low - previous_close).abs());
        if true_range.is_finite() && true_range >= 0.0 {
            ranges.push(true_range);
        }
    }
    if ranges.len() < 14 {
        return None;
    }
    let average = ranges.iter().sum::<f64>() / ranges.len() as f64;
    let close = bars.last().map(|bar| bar.close).unwrap_or(0.0);
    if close > 0.0 {
        Some(average / close)
    } else {
        None
    }
}

fn overheat_factor(stock: &StockItem, distance_ma20: f64) -> f64 {
    let daily = stock
        .change_pct
        .filter(|value| value.is_finite())
        .map(as_percent)
        .map(|value| ((value - 7.0) / 5.0).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let volume = stock
        .volume_ratio
        .filter(|value| value.is_finite())
        .map(|value| ((value - 2.0) / 3.0).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let turnover = stock
        .turnover_rate
        .filter(|value| value.is_finite())
        .map(as_percent)
        .map(|value| ((value - 6.0) / 12.0).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let distance = ((distance_ma20 - 0.08) / 0.12).clamp(0.0, 1.0);
    (daily * 0.40 + volume * 0.20 + turnover * 0.20 + distance * 0.20).clamp(0.0, 1.0)
}

fn stability_factor(stock: &StockItem, risk: f64) -> f64 {
    let dividend = stock
        .dividend_yield
        .filter(|value| value.is_finite())
        .map(as_percent)
        .map(|value| (value / 5.0).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let profitability = stock
        .deducted_net_profit_billion
        .filter(|value| value.is_finite())
        .map(|value| ((value + 0.2) / 2.2).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let growth = stock
        .deducted_net_profit_growth_rate
        .filter(|value| value.is_finite())
        .map(as_percent)
        .map(|value| ((value + 10.0) / 30.0).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    (risk * 0.45 + dividend * 0.25 + profitability * 0.20 + growth * 0.10).clamp(0.0, 1.0)
}

fn factor(factors: &BTreeMap<String, f64>, key: &str) -> f64 {
    factors.get(key).copied().unwrap_or(0.0).clamp(0.0, 1.0)
}

fn mode_weights(mode: &str) -> Vec<(&'static str, &'static str, f64)> {
    match mode {
        "trend" => vec![
            ("quality", "质量", 0.18),
            ("valuation", "估值", 0.10),
            ("trend", "趋势结构", 0.30),
            ("relative_strength", "相对强度", 0.18),
            ("liquidity", "流动性", 0.10),
            ("risk", "风险控制", 0.14),
        ],
        "defensive" => vec![
            ("quality", "质量", 0.30),
            ("valuation", "估值", 0.22),
            ("risk", "低波动与回撤", 0.24),
            ("liquidity", "流动性", 0.12),
            ("stability", "分红及盈利稳定性", 0.12),
        ],
        "transition" => vec![
            ("quality", "质量", 0.27),
            ("valuation", "估值", 0.19),
            ("range_setup", "区间波段形态", 0.11),
            ("relative_strength", "行业相对强度", 0.07),
            ("liquidity", "流动性", 0.11),
            ("risk", "低波动与回撤", 0.19),
            ("stability", "分红及盈利稳定性", 0.06),
        ],
        _ => vec![
            ("quality", "质量", 0.24),
            ("valuation", "估值", 0.16),
            ("range_setup", "区间波段形态", 0.22),
            ("relative_strength", "行业相对强度", 0.14),
            ("liquidity", "流动性", 0.10),
            ("risk", "下行风险", 0.14),
        ],
    }
}

fn weighted_score(factors: &BTreeMap<String, f64>, mode: &str) -> f64 {
    if mode == "transition" {
        return (weighted_score(factors, "range") + weighted_score(factors, "defensive")) / 2.0;
    }
    let base = mode_weights(mode)
        .iter()
        .map(|(key, _, weight)| factor(factors, key) * weight)
        .sum::<f64>();
    let maximum_penalty = match mode {
        "defensive" => 0.06,
        _ => 0.10,
    };
    ((base - factor(factors, "overheat_penalty") * maximum_penalty).max(0.0) * SCORE_SCALE)
        .clamp(0.0, SCORE_SCALE)
}

fn build_screened(
    stock: &StockItem,
    factors: &BTreeMap<String, f64>,
    score: f64,
    mode: &str,
) -> ScreenedStock {
    let mut score_breakdown = mode_weights(mode)
        .into_iter()
        .map(|(key, label, weight)| ScoreContribution {
            key: key.to_string(),
            label: label.to_string(),
            value: Some(round6(factor(factors, key) * SCORE_SCALE)),
            contribution: Some(round6(factor(factors, key) * weight * SCORE_SCALE)),
            tone: "neutral".to_string(),
        })
        .collect::<Vec<_>>();
    let penalty_scale = match mode {
        "defensive" => 1.2,
        "transition" => 1.6,
        _ => 2.0,
    };
    let penalty = factor(factors, "overheat_penalty") * penalty_scale;
    if penalty > 0.0 {
        score_breakdown.push(ScoreContribution {
            key: "overheat_penalty".to_string(),
            label: "追涨过热扣分".to_string(),
            value: Some(round6(penalty)),
            contribution: Some(round6(-penalty)),
            tone: "negative".to_string(),
        });
    }
    let mut reasons = vec![format!("{}模式综合得分 {:.2}", regime_label(mode), score)];
    let mut strongest = factors
        .iter()
        .filter(|(key, _)| key.as_str() != "overheat_penalty")
        .collect::<Vec<_>>();
    strongest.sort_by(|left, right| right.1.total_cmp(left.1).then_with(|| left.0.cmp(right.0)));
    reasons.extend(
        strongest
            .into_iter()
            .take(2)
            .map(|(key, value)| format!("{} {:.1}/20", factor_label(key), value * SCORE_SCALE)),
    );
    ScreenedStock {
        stock: stock.clone(),
        score: round6(score),
        reasons,
        quality_score: round6(factor(factors, "quality") * SCORE_SCALE),
        trend_score: round6(factor(factors, "trend") * SCORE_SCALE),
        risk_score: round6(factor(factors, "risk") * SCORE_SCALE),
        balanced_score: round6(score),
        factor_scores: factors
            .iter()
            .map(|(key, value)| (key.clone(), round6(value * SCORE_SCALE)))
            .collect(),
        score_breakdown,
        score_explanation: format!("adaptive_swing_v1 · {} · 10–30个交易日", regime_label(mode)),
        reason_tags: vec![regime_label(mode).to_string(), "10–30日波段".to_string()],
        risk_tags: if penalty > 0.5 {
            vec!["短线过热".to_string()]
        } else {
            Vec::new()
        },
        suitable_periods: vec!["10–30个交易日".to_string()],
        concept: Some(concept_group_for_stock(stock)),
        theme_category: None,
    }
}

fn regime_label(mode: &str) -> &str {
    match mode {
        "trend" => "趋势",
        "defensive" => "防守",
        "transition" => "过渡",
        _ => "震荡",
    }
}

fn factor_label(key: &str) -> &str {
    match key {
        "quality" => "质量",
        "valuation" => "估值",
        "range_setup" => "波段形态",
        "trend" => "趋势结构",
        "relative_strength" => "相对强度",
        "liquidity" => "流动性",
        "risk" => "风险控制",
        "stability" => "稳定性",
        _ => "综合因子",
    }
}

pub fn adaptive_screen_stocks(
    universe: &[StockItem],
    histories: &HashMap<String, Vec<HistoryBar>>,
    benchmark_histories: &HashMap<String, Vec<HistoryBar>>,
    recent_exposure: &[AdaptiveRecentExposure],
    request: &AdaptiveScreenRequest,
) -> CoreResult<AdaptiveScreenResult> {
    if request.horizon != default_horizon() {
        return Err(CoreError::new("仅支持 swing_10_30d 选股周期"));
    }
    let primary_limit = request.primary_limit.clamp(1, 50);
    let exploration_limit = request.exploration_limit.min(50);
    let pool_limit = 80;
    let staged = stage_one_candidates(universe, &request.criteria, pool_limit);
    if staged.is_empty() {
        return Err(CoreError::new("没有股票通过基础财务与有效报价过滤"));
    }

    let mut usable = Vec::new();
    for stock in staged.iter().copied() {
        let bars = history_for(histories, &stock.code);
        let Some(factors) = bars.and_then(|bars| technical_factors(stock, bars)) else {
            continue;
        };
        usable.push((stock.clone(), factors));
    }
    apply_industry_relative_strength(&mut usable);
    let required = primary_limit.max(((staged.len() as f64) * 0.60).ceil() as usize);
    if usable.len() < required {
        return Err(CoreError::new(format!(
            "市场状态/历史数据不足：初选池 {} 只中仅 {} 只具备至少60根有效日线，最低需要 {} 只",
            staged.len(),
            usable.len(),
            required
        )));
    }

    let market_regime = detect_regime(
        universe,
        benchmark_histories,
        request,
        staged.len(),
        usable.len(),
    )?;
    let effective = market_regime.effective.as_str();
    let mut candidates = usable
        .into_iter()
        .map(|(stock, factors)| {
            let score = weighted_score(&factors, effective);
            let screened = build_screened(&stock, &factors, score, effective);
            AdaptiveCandidate {
                stock: stock.clone(),
                factors,
                score,
                screened,
            }
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.stock.code.cmp(&right.stock.code))
    });

    let primary_candidates = select_primary(&candidates, primary_limit);
    if primary_candidates.len() < primary_limit {
        return Err(CoreError::new(format!(
            "市场状态/历史数据不足：行业分散上限应用后主榜仅能生成 {} 只，要求 {} 只",
            primary_candidates.len(),
            primary_limit
        )));
    }
    let primary_codes = primary_candidates
        .iter()
        .map(|candidate| candidate.stock.code.to_ascii_uppercase())
        .collect::<HashSet<_>>();
    let primary_floor = primary_candidates
        .last()
        .map(|candidate| candidate.score)
        .unwrap_or(0.0);
    let exploration_candidates = select_exploration(
        &candidates,
        &primary_candidates,
        &primary_codes,
        primary_floor,
        exploration_limit,
        recent_exposure,
    );
    let primary = primary_candidates
        .into_iter()
        .map(|candidate| candidate.screened.clone())
        .collect::<Vec<_>>();
    let exploration = exploration_candidates
        .into_iter()
        .map(|candidate| candidate.screened.clone())
        .collect::<Vec<_>>();
    let groups = vec![
        ScreenResultGroup {
            key: "primary".to_string(),
            title: "主榜".to_string(),
            description: "按市场状态自适应评分，单一行业最多3只".to_string(),
            total: primary.len(),
            returned: primary.len(),
            items: primary.clone(),
        },
        ScreenResultGroup {
            key: "exploration".to_string(),
            title: "探索榜".to_string(),
            description: "兼顾原始分、差异度与近期新颖度".to_string(),
            total: exploration.len(),
            returned: exploration.len(),
            items: exploration,
        },
    ];
    Ok(AdaptiveScreenResult {
        algorithm_version: ADAPTIVE_ALGORITHM_VERSION.to_string(),
        total: candidates.len(),
        returned: primary.len(),
        items: primary,
        groups,
        market_regime,
        notes: vec![
            "概念标签仅用于解释与分散，不参与原始评分".to_string(),
            "仅供研究，不构成投资建议".to_string(),
        ],
    })
}

fn apply_industry_relative_strength(usable: &mut [(StockItem, BTreeMap<String, f64>)]) {
    let mut by_industry = BTreeMap::<String, Vec<(String, f64)>>::new();
    for (stock, factors) in usable.iter() {
        by_industry
            .entry(normalized_industry(stock))
            .or_default()
            .push((stock.code.clone(), factor(factors, "relative_strength")));
    }
    let mut percentile_by_code = HashMap::new();
    for values in by_industry.values_mut() {
        values.sort_by(|left, right| {
            left.1
                .total_cmp(&right.1)
                .then_with(|| left.0.cmp(&right.0))
        });
        let denominator = values.len().saturating_sub(1).max(1) as f64;
        for (rank, (code, _)) in values.iter().enumerate() {
            percentile_by_code.insert(code.clone(), rank as f64 / denominator);
        }
    }
    for (stock, factors) in usable.iter_mut() {
        let absolute = factor(factors, "relative_strength");
        let relative = percentile_by_code.get(&stock.code).copied().unwrap_or(0.0);
        factors.insert(
            "relative_strength".to_string(),
            (absolute * 0.40 + relative * 0.60).clamp(0.0, 1.0),
        );
    }
}

fn history_for<'a>(
    histories: &'a HashMap<String, Vec<HistoryBar>>,
    code: &str,
) -> Option<&'a Vec<HistoryBar>> {
    histories
        .get(code)
        .or_else(|| histories.get(&code.to_ascii_uppercase()))
}

fn select_primary<'a>(
    candidates: &'a [AdaptiveCandidate],
    limit: usize,
) -> Vec<&'a AdaptiveCandidate> {
    let mut selected = Vec::new();
    let mut industries = HashMap::<String, usize>::new();
    for candidate in candidates {
        let industry = normalized_industry(&candidate.stock);
        if industries.get(&industry).copied().unwrap_or(0) >= 3 {
            continue;
        }
        selected.push(candidate);
        *industries.entry(industry).or_default() += 1;
        if selected.len() >= limit {
            break;
        }
    }
    selected
}

fn select_exploration<'a>(
    candidates: &'a [AdaptiveCandidate],
    primary: &[&'a AdaptiveCandidate],
    primary_codes: &HashSet<String>,
    primary_floor: f64,
    limit: usize,
    recent_exposure: &[AdaptiveRecentExposure],
) -> Vec<&'a AdaptiveCandidate> {
    if limit == 0 {
        return Vec::new();
    }
    let recent = recent_exposure
        .iter()
        .map(|exposure| exposure.code.to_ascii_uppercase())
        .collect::<HashSet<_>>();
    let mut remaining = candidates
        .iter()
        .filter(|candidate| {
            !primary_codes.contains(&candidate.stock.code.to_ascii_uppercase())
                && candidate.score >= 10.0
                && candidate.score + 2.0 >= primary_floor
        })
        .collect::<Vec<_>>();
    let mut selected = Vec::new();
    let mut industry_counts = HashMap::<String, usize>::new();
    while selected.len() < limit && !remaining.is_empty() {
        remaining.sort_by(|left, right| {
            let left_rank = exploration_rank(left, primary, &selected, &recent);
            let right_rank = exploration_rank(right, primary, &selected, &recent);
            right_rank
                .total_cmp(&left_rank)
                .then_with(|| left.stock.code.cmp(&right.stock.code))
        });
        let index = remaining.iter().position(|candidate| {
            industry_counts
                .get(&normalized_industry(&candidate.stock))
                .copied()
                .unwrap_or(0)
                < 2
        });
        let Some(index) = index else {
            break;
        };
        let candidate = remaining.remove(index);
        *industry_counts
            .entry(normalized_industry(&candidate.stock))
            .or_default() += 1;
        selected.push(candidate);
    }
    selected
}

fn exploration_rank(
    candidate: &AdaptiveCandidate,
    primary: &[&AdaptiveCandidate],
    selected: &[&AdaptiveCandidate],
    recent: &HashSet<String>,
) -> f64 {
    let maximum_similarity = primary
        .iter()
        .copied()
        .chain(selected.iter().copied())
        .map(|other| candidate_similarity(candidate, other))
        .fold(0.0, f64::max);
    let difference = 1.0 - maximum_similarity;
    let novelty = if recent.contains(&candidate.stock.code.to_ascii_uppercase()) {
        0.0
    } else {
        1.0
    };
    candidate.score / SCORE_SCALE * 0.70 + difference * 0.20 + novelty * 0.10
}

fn candidate_similarity(left: &AdaptiveCandidate, right: &AdaptiveCandidate) -> f64 {
    let industry =
        (normalized_industry(&left.stock) == normalized_industry(&right.stock)) as u8 as f64;
    let concept = (concept_group_for_stock(&left.stock) == concept_group_for_stock(&right.stock))
        as u8 as f64;
    let size = (size_bucket(&left.stock) == size_bucket(&right.stock)) as u8 as f64;
    industry * 0.50
        + concept * 0.20
        + size * 0.15
        + factor_cosine(&left.factors, &right.factors) * 0.15
}

fn size_bucket(stock: &StockItem) -> u8 {
    match stock.market_cap_billion.unwrap_or(0.0) {
        value if value >= 1000.0 => 3,
        value if value >= 300.0 => 2,
        value if value >= 80.0 => 1,
        _ => 0,
    }
}

fn factor_cosine(left: &BTreeMap<String, f64>, right: &BTreeMap<String, f64>) -> f64 {
    const KEYS: [&str; 8] = [
        "quality",
        "valuation",
        "range_setup",
        "trend",
        "relative_strength",
        "liquidity",
        "risk",
        "stability",
    ];
    let dot = KEYS
        .iter()
        .map(|key| factor(left, key) * factor(right, key))
        .sum::<f64>();
    let left_norm = KEYS
        .iter()
        .map(|key| factor(left, key).powi(2))
        .sum::<f64>()
        .sqrt();
    let right_norm = KEYS
        .iter()
        .map(|key| factor(right, key).powi(2))
        .sum::<f64>()
        .sqrt();
    if left_norm <= f64::EPSILON || right_norm <= f64::EPSILON {
        0.0
    } else {
        (dot / (left_norm * right_norm)).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn overheat_thresholds_use_percentage_points_without_early_penalties() {
        let mut stock = StockItem {
            change_pct: Some(0.069),
            turnover_rate: Some(0.05),
            volume_ratio: Some(2.0),
            ..StockItem::default()
        };
        assert!(overheat_factor(&stock, 0.08).abs() < 1e-12);

        stock.change_pct = Some(0.07);
        assert!(overheat_factor(&stock, 0.08).abs() < 1e-12);
        stock.change_pct = Some(0.071);
        assert!(overheat_factor(&stock, 0.08) > 0.0);
        stock.change_pct = Some(0.12);
        assert!((overheat_factor(&stock, 0.08) - 0.40).abs() < 1e-9);
    }

    #[test]
    fn defensive_stability_preserves_dividend_and_growth_gradients() {
        let stock = StockItem {
            dividend_yield: Some(0.02),
            deducted_net_profit_growth_rate: Some(0.08),
            ..StockItem::default()
        };
        assert!((stability_factor(&stock, 0.5) - 0.385).abs() < 1e-9);
    }

    #[test]
    fn missing_ohlc_is_not_treated_as_zero_atr_or_low_risk() {
        let bars = (0..60)
            .map(|index| HistoryBar {
                date: format!("{:08}", 20_260_101 + index),
                open: None,
                high: None,
                low: None,
                close: 10.0 + index as f64 * 0.01,
                volume: Some(1_000.0),
                capital: None,
            })
            .collect::<Vec<_>>();
        assert!(atr_percentile(&bars).is_none());
        assert!(atr_percent_of_close(&bars).is_none());
        assert!(technical_factors(&StockItem::default(), &bars).is_none());
    }

    #[test]
    fn missing_market_breadth_is_explicit() {
        assert!(market_breadth(&[StockItem::default()], None).is_none());
    }

    #[test]
    fn market_breadth_counts_only_quotes_from_the_requested_as_of_date() {
        let stocks = [
            StockItem {
                change_pct: Some(1.0),
                quote_time: Some("2026-07-29 15:00:00".to_string()),
                ..StockItem::default()
            },
            StockItem {
                change_pct: Some(-1.0),
                quote_time: Some("2026-07-28".to_string()),
                ..StockItem::default()
            },
        ];
        let breadth = market_breadth(&stocks, Some("20260729")).expect("one quote is current");
        assert_eq!(breadth.requested, 2);
        assert_eq!(breadth.observed, 1);
        assert_eq!(breadth.advancing_ratio, 1.0);
        assert_eq!(breadth.coverage_ratio, 0.5);
    }
}
