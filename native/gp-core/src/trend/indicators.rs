use crate::dates::parse_date;
use crate::error::CoreResult;
use crate::market_data::{HistoryBar, StockItem};
use crate::numeric::{ema, finite_round4, round4};
use crate::trend::types::{
    default_pattern_score_max, default_quant_score_max, default_trend_signal_type,
    TrendIndicatorPoint, TrendIndicatorSignal,
};
use chrono::NaiveDate;

#[derive(Clone, Debug)]
pub(super) struct PreparedBar {
    date: NaiveDate,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
    capital: f64,
}

#[derive(Clone, Debug)]
pub(super) struct ComputedTrendBar {
    date: NaiveDate,
    close: f64,
    open: f64,
    high: f64,
    low: f64,
    volume: f64,
    previous_close: Option<f64>,
    close_change: Option<f64>,
    close_change_pct: Option<f64>,
    swl: f64,
    sws: f64,
    k: f64,
    d: f64,
    j: f64,
    accumulation_index: f64,
    accumulation_strength: f64,
    swing_opportunity: f64,
    rebound_signal: f64,
    trend_heat: f64,
    volume_price_heat: f64,
    anomaly_heat: f64,
    popularity_heat: f64,
    star_line: f64,
    bull_line: f64,
    wait_line: f64,
    support: f64,
    resistance: f64,
    breakout: f64,
    reversal: f64,
    swl_above_sws: bool,
    kdj_golden_cross: bool,
    kdj_dead_cross: bool,
    kdj_overbought: bool,
    kdj_oversold: bool,
    red_hold: bool,
    cyan_watch: bool,
    short_buy: bool,
    white_exit: bool,
    oversold: bool,
    quant_score: i32,
}

pub(super) fn prepare_bars(
    history: &[HistoryBar],
    stock: &StockItem,
) -> CoreResult<Vec<PreparedBar>> {
    let fallback_capital = stock
        .market_cap_billion
        .map(|market_cap| market_cap * 1_000_000_000.0 / stock.price.max(0.01))
        .unwrap_or_else(|| {
            let max_volume = history
                .iter()
                .filter_map(|bar| bar.volume)
                .fold(0.0_f64, f64::max);
            (max_volume * 20.0).max(1.0)
        });
    let mut bars = Vec::with_capacity(history.len());
    for bar in history {
        let date = parse_date(&bar.date)?;
        let open = bar.open.unwrap_or(bar.close);
        let high = bar.high.unwrap_or(open.max(bar.close));
        let low = bar.low.unwrap_or(open.min(bar.close));
        bars.push(PreparedBar {
            date,
            open,
            high,
            low,
            close: bar.close,
            volume: bar.volume.unwrap_or(0.0).max(0.0),
            capital: bar.capital.unwrap_or(fallback_capital).max(1.0),
        });
    }
    bars.sort_by(|left, right| left.date.cmp(&right.date));
    Ok(bars)
}

pub(super) fn compute_trend_bars(bars: &[PreparedBar]) -> Vec<ComputedTrendBar> {
    let close: Vec<f64> = bars.iter().map(|bar| bar.close).collect();
    let open: Vec<f64> = bars.iter().map(|bar| bar.open).collect();
    let high: Vec<f64> = bars.iter().map(|bar| bar.high).collect();
    let low: Vec<f64> = bars.iter().map(|bar| bar.low).collect();
    let volume: Vec<f64> = bars.iter().map(|bar| bar.volume).collect();
    let capital: Vec<f64> = bars.iter().map(|bar| bar.capital).collect();

    let ema10 = ema(&close, 10);
    let ema20 = ema(&close, 20);
    let swl: Vec<f64> = ema10
        .iter()
        .zip(&ema20)
        .map(|(left, right)| (*left * 7.0 + *right * 3.0) / 10.0)
        .collect();
    let vol_sum_5 = rolling_sum(&volume, 5);
    let alpha: Vec<f64> = vol_sum_5
        .iter()
        .zip(&capital)
        .map(|(vol_sum, capital)| {
            ((100.0 * *vol_sum / (3.0 * *capital)).max(1.0) / 100.0).clamp(0.01, 1.0)
        })
        .collect();
    let sws = dma(&ema20, &alpha);

    let var1 = primary_state(&close, true);
    let vard = primary_state(&close, false);
    let red_states = alternating_states(&var1, &close, true);
    let cyan_states = alternating_states(&vard, &close, false);
    let red_hold = or_states(&red_states);
    let cyan_watch = or_states(&cyan_states);

    let ma34 = ma(&close, 34);
    let ma3 = ma(&close, 3);
    let bull_line = ma(&close, 26);
    let star_line = star_line(&close, &open, &high, &low);
    let (k, d, j) = kdj(&close, &high, &low);
    let quant_score = quant_scores(&close, &high, &low, &volume);
    let capital_behavior = capital_behavior_metrics(&close, &open, &high, &low, &volume);

    let mut computed = Vec::with_capacity(bars.len());
    for index in 0..bars.len() {
        let e_value = (high[index] + low[index] + open[index] + 2.0 * close[index]) / 5.0;
        let short_buy = index > 0 && cyan_watch[index - 1] && var1[index];
        let white_exit = index > 0 && red_hold[index - 1] && vard[index];
        let oversold =
            ma34[index].is_finite() && ((close[index] - ma34[index]) / ma34[index] * 100.0) < -14.0;
        let wait_line = if ma3[index].is_finite()
            && star_line[index].is_finite()
            && ma3[index] > star_line[index]
        {
            star_line[index]
        } else {
            ma3[index]
        };
        let previous_close = if index > 0 && close[index - 1].is_finite() {
            Some(close[index - 1])
        } else {
            None
        };
        let close_change = previous_close.map(|previous| close[index] - previous);
        let close_change_pct = previous_close
            .filter(|previous| previous.abs() > f64::EPSILON)
            .map(|previous| (close[index] - previous) / previous);
        computed.push(ComputedTrendBar {
            date: bars[index].date,
            close: close[index],
            open: open[index],
            high: high[index],
            low: low[index],
            volume: volume[index],
            previous_close,
            close_change,
            close_change_pct,
            swl: swl[index],
            sws: sws[index],
            k: k[index],
            d: d[index],
            j: j[index],
            accumulation_index: capital_behavior[index].accumulation_index,
            accumulation_strength: capital_behavior[index].accumulation_strength,
            swing_opportunity: capital_behavior[index].swing_opportunity,
            rebound_signal: capital_behavior[index].rebound_signal,
            trend_heat: capital_behavior[index].trend_heat,
            volume_price_heat: capital_behavior[index].volume_price_heat,
            anomaly_heat: capital_behavior[index].anomaly_heat,
            popularity_heat: capital_behavior[index].popularity_heat,
            star_line: star_line[index],
            bull_line: bull_line[index],
            wait_line,
            support: 2.0 * e_value - high[index],
            resistance: 2.0 * e_value - low[index],
            breakout: e_value + (high[index] - low[index]),
            reversal: e_value - (high[index] - low[index]),
            swl_above_sws: swl[index] > sws[index],
            kdj_golden_cross: index > 0 && k[index - 1] <= d[index - 1] && k[index] > d[index],
            kdj_dead_cross: index > 0 && k[index - 1] >= d[index - 1] && k[index] < d[index],
            kdj_overbought: k[index] >= 80.0 || d[index] >= 80.0 || j[index] >= 100.0,
            kdj_oversold: k[index] <= 20.0 || d[index] <= 20.0 || j[index] <= 0.0,
            red_hold: red_hold[index],
            cyan_watch: cyan_watch[index],
            short_buy,
            white_exit,
            oversold,
            quant_score: quant_score[index],
        });
    }
    computed
}

fn primary_state(close: &[f64], up: bool) -> Vec<bool> {
    let mut state = vec![false; close.len()];
    for index in 2..close.len() {
        state[index] = if up {
            close[index] > close[index - 1] && close[index] > close[index - 2]
        } else {
            close[index] < close[index - 1] && close[index] < close[index - 2]
        };
    }
    state
}

fn alternating_states(primary: &[bool], close: &[f64], starts_with_le_ge: bool) -> Vec<Vec<bool>> {
    let mut states = vec![primary.to_vec()];
    for state_index in 1..12 {
        let previous = states.last().expect("state exists");
        let le_ge = if state_index % 2 == 1 {
            starts_with_le_ge
        } else {
            !starts_with_le_ge
        };
        let mut next = vec![false; close.len()];
        for index in 2..close.len() {
            let condition = if le_ge {
                close[index] <= close[index - 1] && close[index] >= close[index - 2]
            } else {
                close[index] >= close[index - 1] && close[index] <= close[index - 2]
            };
            next[index] = previous[index - 1] && condition;
        }
        states.push(next);
    }
    states
}

fn or_states(states: &[Vec<bool>]) -> Vec<bool> {
    if states.is_empty() {
        return Vec::new();
    }
    let mut combined = vec![false; states[0].len()];
    for state in states {
        for (index, value) in state.iter().enumerate() {
            combined[index] |= *value;
        }
    }
    combined
}

fn ma(values: &[f64], window: usize) -> Vec<f64> {
    let mut output = vec![f64::NAN; values.len()];
    let mut sum = 0.0;
    for index in 0..values.len() {
        sum += values[index];
        if index >= window {
            sum -= values[index - window];
        }
        if index + 1 >= window {
            output[index] = sum / window as f64;
        }
    }
    output
}

fn rolling_sum(values: &[f64], window: usize) -> Vec<f64> {
    let mut output = Vec::with_capacity(values.len());
    let mut sum = 0.0;
    for index in 0..values.len() {
        sum += values[index];
        if index >= window {
            sum -= values[index - window];
        }
        output.push(sum);
    }
    output
}

fn rolling_mean_partial(values: &[f64], window: usize) -> Vec<f64> {
    let mut output = Vec::with_capacity(values.len());
    let mut sum = 0.0;
    for index in 0..values.len() {
        sum += values[index];
        if index >= window {
            sum -= values[index - window];
        }
        let count = (index + 1).min(window).max(1);
        output.push(sum / count as f64);
    }
    output
}

fn rolling_min(values: &[f64], window: usize) -> Vec<f64> {
    (0..values.len())
        .map(|index| {
            let start = index.saturating_sub(window.saturating_sub(1));
            values[start..=index]
                .iter()
                .copied()
                .fold(f64::INFINITY, f64::min)
        })
        .collect()
}

fn rolling_max(values: &[f64], window: usize) -> Vec<f64> {
    (0..values.len())
        .map(|index| {
            let start = index.saturating_sub(window.saturating_sub(1));
            values[start..=index]
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max)
        })
        .collect()
}

fn dma(values: &[f64], alpha: &[f64]) -> Vec<f64> {
    let mut output = Vec::with_capacity(values.len());
    let mut previous = None;
    for (value, coefficient) in values.iter().zip(alpha) {
        let coefficient = (*coefficient).clamp(0.0, 1.0);
        let current = match previous {
            Some(previous_value) => coefficient * *value + (1.0 - coefficient) * previous_value,
            None => *value,
        };
        previous = Some(current);
        output.push(current);
    }
    output
}

fn tdx_sma(values: &[f64], window: f64, weight: f64) -> Vec<f64> {
    let mut output = Vec::with_capacity(values.len());
    let mut previous = None;
    for value in values {
        let value = if value.is_finite() { *value } else { 50.0 };
        let current = match previous {
            Some(previous_value) => (weight * value + (window - weight) * previous_value) / window,
            None => value,
        };
        previous = Some(current);
        output.push(current);
    }
    output
}

fn kdj(close: &[f64], high: &[f64], low: &[f64]) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let low_min = rolling_min(low, 9);
    let high_max = rolling_max(high, 9);
    let rsv: Vec<f64> = close
        .iter()
        .enumerate()
        .map(|(index, close)| {
            let spread = high_max[index] - low_min[index];
            if spread == 0.0 {
                f64::NAN
            } else {
                ((*close - low_min[index]) / spread * 100.0).clamp(0.0, 100.0)
            }
        })
        .collect();
    let k = tdx_sma(&rsv, 3.0, 1.0);
    let d = tdx_sma(&k, 3.0, 1.0);
    let j = k.iter().zip(&d).map(|(k, d)| 3.0 * k - 2.0 * d).collect();
    (k, d, j)
}

fn quant_scores(close: &[f64], high: &[f64], low: &[f64], volume: &[f64]) -> Vec<i32> {
    let ma5 = ma(close, 5);
    let ma10 = ma(close, 10);
    let ma20 = ma(close, 20);
    let ma60 = ma(close, 60);
    let volume_ma60 = ma(volume, 60);
    let (k, _d, j) = kdj(close, high, low);
    let ema12 = ema(close, 12);
    let ema26 = ema(close, 26);
    let dif: Vec<f64> = ema12
        .iter()
        .zip(ema26)
        .map(|(left, right)| *left - right)
        .collect();
    let dea = ema(&dif, 9);
    let macd: Vec<f64> = dif
        .iter()
        .zip(&dea)
        .map(|(dif, dea)| 2.0 * (*dif - *dea))
        .collect();

    let mut scores = vec![0; close.len()];
    for index in 0..close.len() {
        if ma5[index] > ma10[index] {
            scores[index] += 20;
        }
        if ma20[index] > ma60[index] {
            scores[index] += 10;
        }
        if j[index] > k[index] {
            scores[index] += 10;
        }
        if dif[index] > dea[index] {
            scores[index] += 10;
        }
        if macd[index] > 0.0 {
            scores[index] += 10;
        }
        if volume[index] > volume_ma60[index] {
            scores[index] += 10;
        }
        if index > 0 && close[index] / close[index - 1] > 1.03 {
            scores[index] += 10;
        }
    }
    scores
}

#[derive(Clone, Copy, Debug, Default)]
struct CapitalBehaviorBar {
    accumulation_index: f64,
    accumulation_strength: f64,
    swing_opportunity: f64,
    rebound_signal: f64,
    trend_heat: f64,
    volume_price_heat: f64,
    anomaly_heat: f64,
    popularity_heat: f64,
}

fn capital_behavior_metrics(
    close: &[f64],
    open: &[f64],
    high: &[f64],
    low: &[f64],
    volume: &[f64],
) -> Vec<CapitalBehaviorBar> {
    let len = close.len();
    if len == 0 {
        return Vec::new();
    }

    let volume_ma20 = rolling_mean_partial(volume, 20);
    let low_60 = rolling_min(low, 60);
    let high_60 = rolling_max(high, 60);
    let ma5 = ma(close, 5);
    let ma10 = ma(close, 10);
    let ma20 = ma(close, 20);
    let ma60 = ma(close, 60);

    let mut raw = Vec::with_capacity(len);
    let mut low_zone_values = Vec::with_capacity(len);
    let mut lower_shadow_values = Vec::with_capacity(len);
    let mut close_recovery_values = Vec::with_capacity(len);
    let mut volume_expansion_values = Vec::with_capacity(len);
    let mut price_position_values = Vec::with_capacity(len);
    let mut pct_change_values = Vec::with_capacity(len);
    let mut amplitude_values = Vec::with_capacity(len);
    let mut volume_ratio_values = Vec::with_capacity(len);

    for index in 0..len {
        let previous_close = if index > 0 && close[index - 1].abs() > f64::EPSILON {
            close[index - 1]
        } else {
            close[index].max(0.01)
        };
        let price_range = (high[index] - low[index]).max(0.0);
        let pct_change = close[index] / previous_close - 1.0;
        let amplitude = if previous_close.abs() > f64::EPSILON {
            price_range / previous_close
        } else {
            0.0
        };
        let volume_ratio = if volume_ma20[index].abs() > f64::EPSILON {
            (volume[index] / volume_ma20[index]).clamp(0.0, 4.0)
        } else {
            1.0
        };
        let rolling_range = high_60[index] - low_60[index];
        let price_position = if rolling_range.abs() > f64::EPSILON {
            ((close[index] - low_60[index]) / rolling_range).clamp(0.0, 1.0)
        } else {
            0.5
        };
        let low_zone = 1.0 - price_position;
        let lower_shadow = if price_range.abs() > f64::EPSILON {
            ((open[index].min(close[index]) - low[index]) / price_range).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let close_recovery = if price_range.abs() > f64::EPSILON {
            ((close[index] - low[index]) / price_range).clamp(0.0, 1.0)
        } else {
            0.5
        };
        let volume_expansion = ((volume_ratio - 1.0).clamp(0.0, 2.0) / 2.0).clamp(0.0, 1.0);
        let positive_close = if close[index] >= open[index] {
            1.0
        } else {
            0.0
        };
        let negative_close = if close[index] < open[index] { 1.0 } else { 0.0 };

        raw.push(
            low_zone * 44.0
                + volume_expansion * 28.0
                + lower_shadow * 18.0
                + close_recovery * 8.0
                + positive_close * 4.0
                - price_position * 22.0
                - negative_close * pct_change.abs().min(0.08) * 260.0
                - if volume_ratio > 1.8 && close[index] < open[index] {
                    12.0
                } else {
                    0.0
                },
        );
        low_zone_values.push(low_zone);
        lower_shadow_values.push(lower_shadow);
        close_recovery_values.push(close_recovery);
        volume_expansion_values.push(volume_expansion);
        price_position_values.push(price_position);
        pct_change_values.push(pct_change);
        amplitude_values.push(amplitude);
        volume_ratio_values.push(volume_ratio);
    }

    let accumulation_index: Vec<f64> = ema(&raw, 5)
        .into_iter()
        .map(|value| (value - 34.0).clamp(-100.0, 100.0))
        .collect();

    let mut strength_raw = Vec::with_capacity(len);
    let mut swing_raw = Vec::with_capacity(len);
    let mut rebound_raw = Vec::with_capacity(len);
    let mut trend_heat = Vec::with_capacity(len);
    let mut volume_price_heat = Vec::with_capacity(len);
    let mut anomaly_heat = Vec::with_capacity(len);
    let mut popularity_heat = Vec::with_capacity(len);

    for index in 0..len {
        let low_zone = low_zone_values[index];
        let lower_shadow = lower_shadow_values[index];
        let close_recovery = close_recovery_values[index];
        let volume_expansion = volume_expansion_values[index];
        let price_position = price_position_values[index];
        let pct_change = pct_change_values[index];
        let amplitude = amplitude_values[index];
        let volume_ratio = volume_ratio_values[index];

        strength_raw.push(
            (accumulation_index[index].max(0.0) * 0.72
                + low_zone * 18.0
                + lower_shadow * 12.0
                + volume_expansion * 18.0)
                .clamp(0.0, 100.0),
        );

        let trend_turn = if ma5[index] > ma10[index] { 22.0 } else { 0.0 }
            + if ma10[index] > ma20[index] { 18.0 } else { 0.0 }
            + if close[index] > ma20[index] {
                18.0
            } else {
                0.0
            }
            + if index >= 5 && (ma20[index] - ma20[index - 5]) > 0.0 {
                14.0
            } else {
                0.0
            };
        swing_raw.push(
            (strength_raw[index] * 0.48 + trend_turn * 0.46 + low_zone * 16.0
                - price_position * 18.0)
                .clamp(0.0, 100.0),
        );
        rebound_raw.push(
            (low_zone * 32.0
                + lower_shadow * 28.0
                + volume_expansion * 20.0
                + if pct_change > 0.0 { 12.0 } else { 0.0 }
                + if index > 0 && pct_change_values[index - 1] < -0.025 {
                    8.0
                } else {
                    0.0
                })
            .clamp(0.0, 100.0),
        );

        let trend_value = if close[index] > ma5[index] { 20.0 } else { 0.0 }
            + if ma5[index] > ma10[index] { 25.0 } else { 0.0 }
            + if ma10[index] > ma20[index] { 25.0 } else { 0.0 }
            + if ma20[index] > ma60[index] { 15.0 } else { 0.0 }
            + if index >= 5 && (ma20[index] - ma20[index - 5]) > 0.0 {
                15.0
            } else {
                0.0
            };
        let volume_price_value = (volume_expansion * 42.0
            + pct_change.clamp(0.0, 0.06) * 520.0
            + close_recovery * 18.0
            + if close[index] > open[index] { 8.0 } else { 0.0 })
        .clamp(0.0, 100.0);
        let anomaly_value = (amplitude.min(0.12) * 360.0
            + (volume_ratio - 1.0).clamp(0.0, 3.0) * 20.0
            + pct_change.abs().min(0.08) * 260.0)
            .clamp(0.0, 100.0);
        let popularity_value = ((volume_ratio.min(3.0) / 3.0) * 36.0
            + amplitude.min(0.1) * 260.0
            + pct_change.clamp(0.0, 0.06) * 360.0
            + trend_value * 0.18)
            .clamp(0.0, 100.0);
        trend_heat.push(trend_value.clamp(0.0, 100.0));
        volume_price_heat.push(volume_price_value);
        anomaly_heat.push(anomaly_value);
        popularity_heat.push(popularity_value);
    }

    let accumulation_strength = ema(&strength_raw, 3);
    let swing_opportunity = ema(&swing_raw, 3);
    let rebound_signal = ema(&rebound_raw, 3);

    (0..len)
        .map(|index| CapitalBehaviorBar {
            accumulation_index: accumulation_index[index],
            accumulation_strength: accumulation_strength[index].clamp(0.0, 100.0),
            swing_opportunity: swing_opportunity[index].clamp(0.0, 100.0),
            rebound_signal: rebound_signal[index].clamp(0.0, 100.0),
            trend_heat: trend_heat[index],
            volume_price_heat: volume_price_heat[index],
            anomaly_heat: anomaly_heat[index],
            popularity_heat: popularity_heat[index],
        })
        .collect()
}

fn star_line(close: &[f64], open: &[f64], high: &[f64], low: &[f64]) -> Vec<f64> {
    let ytsl: Vec<f64> = close
        .iter()
        .enumerate()
        .map(|(index, close)| (3.0 * close + low[index] + open[index] + high[index]) / 6.0)
        .collect();
    let mut output = vec![f64::NAN; close.len()];
    for index in 20..close.len() {
        let mut total = 0.0;
        for (offset, weight) in (0..19).zip((2..=20).rev()) {
            total += ytsl[index - offset] * weight as f64;
        }
        total += ytsl[index - 20];
        output[index] = total / 211.0;
    }
    output
}

pub(super) fn trend_signal_from_bar(code: &str, bar: &ComputedTrendBar) -> TrendIndicatorSignal {
    let reasons = trend_reasons(bar);
    TrendIndicatorSignal {
        code: code.to_string(),
        date: bar.date.format("%Y-%m-%d").to_string(),
        close: round4(bar.close),
        previous_close: bar.previous_close.and_then(finite_round4),
        close_change: bar.close_change.and_then(finite_round4),
        close_change_pct: bar.close_change_pct.and_then(finite_round4),
        swl: finite_round4(bar.swl),
        sws: finite_round4(bar.sws),
        k: finite_round4(bar.k),
        d: finite_round4(bar.d),
        j: finite_round4(bar.j),
        star_line: finite_round4(bar.star_line),
        bull_line: finite_round4(bar.bull_line),
        wait_line: finite_round4(bar.wait_line),
        support: finite_round4(bar.support),
        resistance: finite_round4(bar.resistance),
        breakout: finite_round4(bar.breakout),
        reversal: finite_round4(bar.reversal),
        swl_above_sws: bar.swl_above_sws,
        kdj_golden_cross: bar.kdj_golden_cross,
        kdj_dead_cross: bar.kdj_dead_cross,
        kdj_overbought: bar.kdj_overbought,
        kdj_oversold: bar.kdj_oversold,
        red_hold: bar.red_hold,
        cyan_watch: bar.cyan_watch,
        short_buy: bar.short_buy,
        white_exit: bar.white_exit,
        oversold: bar.oversold,
        signal_type: default_trend_signal_type(),
        risk_flags: trend_risk_flags_from_bar(bar),
        technical_score: None,
        pattern_layer_score: None,
        quality_score: None,
        quant_score: bar.quant_score,
        quant_score_max: default_quant_score_max(),
        pattern_score: pattern_score(bar),
        pattern_score_max: default_pattern_score_max(),
        pattern_signals: pattern_signals(bar),
        status: trend_status(bar),
        reasons,
        notes: trend_notes(),
    }
}

pub(super) fn trend_point_from_bar(bar: &ComputedTrendBar) -> TrendIndicatorPoint {
    TrendIndicatorPoint {
        date: bar.date.format("%Y-%m-%d").to_string(),
        close: round4(bar.close),
        open: finite_round4(bar.open),
        high: finite_round4(bar.high),
        low: finite_round4(bar.low),
        volume: finite_round4(bar.volume),
        swl: finite_round4(bar.swl),
        sws: finite_round4(bar.sws),
        k: finite_round4(bar.k),
        d: finite_round4(bar.d),
        j: finite_round4(bar.j),
        accumulation_index: finite_round4(bar.accumulation_index),
        accumulation_strength: finite_round4(bar.accumulation_strength),
        swing_opportunity: finite_round4(bar.swing_opportunity),
        rebound_signal: finite_round4(bar.rebound_signal),
        trend_heat: finite_round4(bar.trend_heat),
        volume_price_heat: finite_round4(bar.volume_price_heat),
        anomaly_heat: finite_round4(bar.anomaly_heat),
        popularity_heat: finite_round4(bar.popularity_heat),
        red_hold: bar.red_hold,
        cyan_watch: bar.cyan_watch,
        short_buy: bar.short_buy,
        white_exit: bar.white_exit,
    }
}

fn trend_reasons(bar: &ComputedTrendBar) -> Vec<String> {
    let mut reasons = Vec::new();
    if bar.short_buy {
        reasons.push("short_buy_signal".to_string());
    }
    if bar.red_hold {
        reasons.push("red_hold".to_string());
    }
    if bar.swl_above_sws {
        reasons.push("swl_above_sws".to_string());
    }
    if bar.kdj_golden_cross {
        reasons.push("kdj_golden_cross".to_string());
    }
    if bar.kdj_dead_cross {
        reasons.push("kdj_dead_cross".to_string());
    }
    if bar.kdj_oversold {
        reasons.push("kdj_oversold".to_string());
    }
    if bar.kdj_overbought {
        reasons.push("kdj_overbought".to_string());
    }
    if bar.quant_score >= 60 {
        reasons.push("high_quant_score".to_string());
    }
    if bar.white_exit {
        reasons.push("white_exit".to_string());
    }
    if bar.cyan_watch {
        reasons.push("cyan_watch".to_string());
    }
    if bar.oversold {
        reasons.push("oversold".to_string());
    }
    if bar.accumulation_strength >= 55.0 {
        reasons.push("accumulation_strength".to_string());
    }
    if bar.swing_opportunity >= 60.0 {
        reasons.push("swing_opportunity".to_string());
    }
    reasons
}

fn pattern_score(bar: &ComputedTrendBar) -> i32 {
    let values = [
        bar.accumulation_strength,
        bar.swing_opportunity,
        bar.trend_heat,
        bar.volume_price_heat,
        bar.anomaly_heat,
        bar.popularity_heat,
    ];
    (values.iter().sum::<f64>() / values.len().max(1) as f64).round() as i32
}

fn pattern_signals(bar: &ComputedTrendBar) -> Vec<String> {
    let mut signals = Vec::new();
    if bar.accumulation_index > 8.0 && bar.accumulation_strength >= 45.0 {
        signals.push("bottom_accumulation".to_string());
    }
    if bar.swing_opportunity >= 60.0 {
        signals.push("swing_opportunity".to_string());
    }
    if bar.rebound_signal >= 62.0 {
        signals.push("rebound_signal".to_string());
    }
    if bar.trend_heat >= 65.0 && bar.volume_price_heat >= 55.0 {
        signals.push("dragon_trend_volume".to_string());
    }
    signals
}

fn trend_status(bar: &ComputedTrendBar) -> String {
    if bar.short_buy {
        "buy_setup"
    } else if bar.white_exit {
        "exit"
    } else if bar.red_hold && bar.swl_above_sws {
        "uptrend"
    } else if bar.red_hold {
        "hold"
    } else if bar.cyan_watch {
        "watch"
    } else if bar.oversold {
        "oversold"
    } else {
        "neutral"
    }
    .to_string()
}

fn trend_risk_flags_from_bar(bar: &ComputedTrendBar) -> Vec<String> {
    let mut flags = Vec::new();
    if bar.white_exit {
        flags.push("white_exit".to_string());
    }
    if bar.kdj_overbought {
        flags.push("kdj_overbought".to_string());
    }
    if bar.kdj_dead_cross {
        flags.push("kdj_dead_cross".to_string());
    }
    if !bar.swl_above_sws {
        flags.push("swl_below_sws".to_string());
    }
    flags
}

pub(crate) fn trend_notes() -> Vec<String> {
    vec![
        "吸筹分析根据日线价量推断，暂未纳入真实筹码分布、龙虎榜席位和主力资金流。".to_string(),
        "量化评分满分为90分，筹码分布相关指标尚未纳入。".to_string(),
        "长期均线根据量能动态平滑，用于趋势参考。".to_string(),
    ]
}
