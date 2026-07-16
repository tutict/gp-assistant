//! Point-in-time volatility diagnostics used by the backtest response.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::{parse_date, round4, HistoryBar};

const ATR_PERIOD: usize = 14;
const BOLLINGER_PERIOD: usize = 20;
const BOLLINGER_MULTIPLIER: f64 = 2.0;
const DONCHIAN_PERIOD: usize = 20;
const KELTNER_EMA_PERIOD: usize = 20;
const KELTNER_ATR_PERIOD: usize = 10;
const KELTNER_MULTIPLIER: f64 = 2.0;
const CHAIKIN_EMA_PERIOD: usize = 10;
const CHAIKIN_ROC_PERIOD: usize = 10;
const RVI_PERIOD: usize = 14;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VolatilitySnapshot {
    pub symbol: String,
    pub date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atr: Option<AtrSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bollinger_bands: Option<BollingerBandsSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub donchian_channel: Option<DonchianChannelSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keltner_channel: Option<KeltnerChannelSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chaikin_volatility: Option<ChaikinVolatilitySnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rvi: Option<RviSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unavailable: Vec<IndicatorUnavailable>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndicatorUnavailable {
    pub indicator: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AtrSnapshot {
    pub period: usize,
    pub value: f64,
    pub percent_of_close: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BollingerBandsSnapshot {
    pub period: usize,
    pub multiplier: f64,
    pub upper: f64,
    pub middle: f64,
    pub lower: f64,
    pub bandwidth_percent: Option<f64>,
    pub percent_b: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DonchianChannelSnapshot {
    pub period: usize,
    pub upper: f64,
    pub middle: f64,
    pub lower: f64,
    pub width_percent: Option<f64>,
    pub position_percent: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeltnerChannelSnapshot {
    pub ema_period: usize,
    pub atr_period: usize,
    pub multiplier: f64,
    pub upper: f64,
    pub middle: f64,
    pub lower: f64,
    pub width_percent: Option<f64>,
    pub position_percent: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChaikinVolatilitySnapshot {
    pub ema_period: usize,
    pub roc_period: usize,
    pub value: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RviSnapshot {
    pub period: usize,
    pub value: f64,
}

#[derive(Clone, Debug)]
struct NormalizedBar {
    date: NaiveDate,
    close: Option<f64>,
    high: Option<f64>,
    low: Option<f64>,
}

#[derive(Clone, Debug)]
struct CalculationBar {
    high: Option<f64>,
    low: Option<f64>,
    close: f64,
}

pub(crate) fn calculate_volatility_snapshot(
    symbol: &str,
    history: &[HistoryBar],
) -> Option<VolatilitySnapshot> {
    let normalized = normalize_history(history);
    let latest = normalized.last()?;
    let bars = trailing_valid_bars(&normalized);
    let latest_close = latest.close;

    let atr = calculate_atr(&bars, ATR_PERIOD).map(|value| AtrSnapshot {
        period: ATR_PERIOD,
        value: round4(value),
        percent_of_close: round4(value / latest_close.expect("ATR requires close") * 100.0),
    });
    let bollinger_bands = calculate_bollinger_bands(&bars, BOLLINGER_PERIOD, BOLLINGER_MULTIPLIER);
    let donchian_channel = calculate_donchian_channel(&bars, DONCHIAN_PERIOD);
    let keltner_channel = calculate_keltner_channel(
        &bars,
        KELTNER_EMA_PERIOD,
        KELTNER_ATR_PERIOD,
        KELTNER_MULTIPLIER,
    );
    let chaikin_volatility =
        calculate_chaikin_volatility(&bars, CHAIKIN_EMA_PERIOD, CHAIKIN_ROC_PERIOD);
    let rvi = calculate_rvi(&bars, RVI_PERIOD);
    let unavailable = collect_unavailable(
        latest_close,
        &bars,
        atr.is_some(),
        bollinger_bands.is_some(),
        donchian_channel.is_some(),
        keltner_channel.is_some(),
        chaikin_volatility.is_some(),
        rvi.is_some(),
    );

    Some(VolatilitySnapshot {
        symbol: symbol.to_string(),
        date: latest.date.format("%Y-%m-%d").to_string(),
        close: latest_close.map(round4),
        atr,
        bollinger_bands,
        donchian_channel,
        keltner_channel,
        chaikin_volatility,
        rvi,
        unavailable,
    })
}

fn normalize_history(history: &[HistoryBar]) -> Vec<NormalizedBar> {
    let mut by_date = BTreeMap::new();
    for bar in history {
        let Ok(date) = parse_date(&bar.date) else {
            continue;
        };
        let close = (bar.close.is_finite() && bar.close > 0.0).then_some(bar.close);
        let range = close.and_then(|close| {
            bar.high.zip(bar.low).filter(|(high, low)| {
                let open_is_consistent = bar
                    .open
                    .map(|open| open.is_finite() && open > 0.0 && *high >= open && open >= *low)
                    .unwrap_or(true);
                high.is_finite()
                    && low.is_finite()
                    && *low > 0.0
                    && *high >= close
                    && close >= *low
                    && open_is_consistent
            })
        });
        by_date.insert(
            date,
            NormalizedBar {
                date,
                close,
                high: range.map(|(high, _)| high),
                low: range.map(|(_, low)| low),
            },
        );
    }
    by_date.into_values().collect()
}

fn trailing_valid_bars(history: &[NormalizedBar]) -> Vec<CalculationBar> {
    history
        .iter()
        .rev()
        .take_while(|bar| bar.close.is_some())
        .map(|bar| CalculationBar {
            high: bar.high,
            low: bar.low,
            close: bar.close.expect("validated close"),
        })
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn collect_unavailable(
    latest_close: Option<f64>,
    bars: &[CalculationBar],
    atr_ready: bool,
    bollinger_ready: bool,
    donchian_ready: bool,
    keltner_ready: bool,
    chaikin_ready: bool,
    rvi_ready: bool,
) -> Vec<IndicatorUnavailable> {
    let mut unavailable = Vec::new();
    let trailing_ohlc = bars
        .iter()
        .rev()
        .take_while(|bar| bar.high.is_some() && bar.low.is_some())
        .count();
    let latest_invalid = latest_close.is_none();
    let mut push = |indicator: &str, reason: String| {
        unavailable.push(IndicatorUnavailable {
            indicator: indicator.to_string(),
            reason,
        });
    };
    let history_reason = |period: usize| format!("至少需要连续 {period} 个有效交易日");
    let ohlc_reason = |period: usize| format!("近 {period} 个交易日高低价缺失或与收盘价不一致");

    if !atr_ready {
        push(
            "atr",
            if latest_invalid {
                "区间末收盘价无效".to_string()
            } else if bars.len() < ATR_PERIOD {
                history_reason(ATR_PERIOD)
            } else {
                ohlc_reason(ATR_PERIOD)
            },
        );
    }
    if !bollinger_ready {
        push(
            "bollinger_bands",
            if latest_invalid {
                "区间末收盘价无效".to_string()
            } else {
                history_reason(BOLLINGER_PERIOD)
            },
        );
    }
    if !donchian_ready {
        push(
            "donchian_channel",
            if latest_invalid {
                "区间末收盘价无效".to_string()
            } else if bars.len() < DONCHIAN_PERIOD {
                history_reason(DONCHIAN_PERIOD)
            } else {
                ohlc_reason(DONCHIAN_PERIOD)
            },
        );
    }
    if !keltner_ready {
        push(
            "keltner_channel",
            if latest_invalid {
                "区间末收盘价无效".to_string()
            } else if bars.len() < KELTNER_EMA_PERIOD {
                history_reason(KELTNER_EMA_PERIOD)
            } else if trailing_ohlc < KELTNER_ATR_PERIOD {
                ohlc_reason(KELTNER_ATR_PERIOD)
            } else {
                "凯尔特纳通道无法计算".to_string()
            },
        );
    }
    if !chaikin_ready {
        let required = CHAIKIN_EMA_PERIOD + CHAIKIN_ROC_PERIOD;
        push(
            "chaikin_volatility",
            if latest_invalid {
                "区间末收盘价无效".to_string()
            } else if bars.len() < required {
                history_reason(required)
            } else if trailing_ohlc < required {
                ohlc_reason(required)
            } else {
                "10 个交易日前的波幅为零".to_string()
            },
        );
    }
    if !rvi_ready {
        let required = RVI_PERIOD * 2 - 1;
        push(
            "rvi",
            if latest_invalid {
                "区间末收盘价无效".to_string()
            } else if bars.len() < required {
                history_reason(required)
            } else {
                "区间内没有可计算的方向波动".to_string()
            },
        );
    }
    unavailable
}

fn calculate_bollinger_bands(
    bars: &[CalculationBar],
    period: usize,
    multiplier: f64,
) -> Option<BollingerBandsSnapshot> {
    if period == 0 || bars.len() < period || !multiplier.is_finite() || multiplier <= 0.0 {
        return None;
    }
    let window = &bars[bars.len() - period..];
    let middle = window.iter().map(|bar| bar.close).sum::<f64>() / period as f64;
    let variance = window
        .iter()
        .map(|bar| (bar.close - middle).powi(2))
        .sum::<f64>()
        / period as f64;
    let deviation = variance.sqrt();
    let upper = middle + multiplier * deviation;
    let lower = middle - multiplier * deviation;
    let width = upper - lower;
    let latest_close = window.last()?.close;

    Some(BollingerBandsSnapshot {
        period,
        multiplier: round4(multiplier),
        upper: round4(upper),
        middle: round4(middle),
        lower: round4(lower),
        bandwidth_percent: (middle.abs() > f64::EPSILON).then(|| round4(width / middle * 100.0)),
        percent_b: (width.abs() > f64::EPSILON)
            .then(|| round4((latest_close - lower) / width * 100.0)),
    })
}

fn calculate_donchian_channel(
    bars: &[CalculationBar],
    period: usize,
) -> Option<DonchianChannelSnapshot> {
    if period == 0 || bars.len() < period {
        return None;
    }
    let window = &bars[bars.len() - period..];
    let mut upper = f64::NEG_INFINITY;
    let mut lower = f64::INFINITY;
    for bar in window {
        let (high, low) = bar.high.zip(bar.low)?;
        upper = upper.max(high);
        lower = lower.min(low);
    }
    let middle = (upper + lower) / 2.0;
    let width = upper - lower;
    let latest_close = window.last()?.close;

    Some(DonchianChannelSnapshot {
        period,
        upper: round4(upper),
        middle: round4(middle),
        lower: round4(lower),
        width_percent: (middle.abs() > f64::EPSILON).then(|| round4(width / middle * 100.0)),
        position_percent: (width.abs() > f64::EPSILON)
            .then(|| round4((latest_close - lower) / width * 100.0)),
    })
}

fn calculate_keltner_channel(
    bars: &[CalculationBar],
    ema_period: usize,
    atr_period: usize,
    multiplier: f64,
) -> Option<KeltnerChannelSnapshot> {
    if !multiplier.is_finite() || multiplier <= 0.0 {
        return None;
    }
    let closes = bars.iter().map(|bar| bar.close).collect::<Vec<_>>();
    let middle = ema_latest(&closes, ema_period)?;
    let atr = calculate_atr(bars, atr_period)?;
    let upper = middle + multiplier * atr;
    let lower = middle - multiplier * atr;
    let width = upper - lower;
    let latest_close = bars.last()?.close;

    Some(KeltnerChannelSnapshot {
        ema_period,
        atr_period,
        multiplier: round4(multiplier),
        upper: round4(upper),
        middle: round4(middle),
        lower: round4(lower),
        width_percent: (middle.abs() > f64::EPSILON).then(|| round4(width / middle * 100.0)),
        position_percent: (width.abs() > f64::EPSILON)
            .then(|| round4((latest_close - lower) / width * 100.0)),
    })
}

fn ema_latest(values: &[f64], period: usize) -> Option<f64> {
    ema_series(values, period).last().copied()
}

fn calculate_chaikin_volatility(
    bars: &[CalculationBar],
    ema_period: usize,
    roc_period: usize,
) -> Option<ChaikinVolatilitySnapshot> {
    if ema_period == 0 || roc_period == 0 {
        return None;
    }
    let trailing_ranges = bars
        .iter()
        .rev()
        .take_while(|bar| bar.high.is_some() && bar.low.is_some())
        .filter_map(|bar| bar.high.zip(bar.low).map(|(high, low)| high - low))
        .collect::<Vec<_>>();
    let ranges = trailing_ranges.into_iter().rev().collect::<Vec<_>>();
    let ema_values = ema_series(&ranges, ema_period);
    if ema_values.len() <= roc_period {
        return None;
    }
    let current = *ema_values.last()?;
    let previous = ema_values[ema_values.len() - 1 - roc_period];
    if previous.abs() <= f64::EPSILON {
        return None;
    }
    let value = (current / previous - 1.0) * 100.0;
    value.is_finite().then_some(ChaikinVolatilitySnapshot {
        ema_period,
        roc_period,
        value: round4(value),
    })
}

fn ema_series(values: &[f64], period: usize) -> Vec<f64> {
    if period == 0 || values.len() < period {
        return Vec::new();
    }
    let mut ema = values[..period].iter().sum::<f64>() / period as f64;
    let alpha = 2.0 / (period as f64 + 1.0);
    let mut result = Vec::with_capacity(values.len() - period + 1);
    result.push(ema);
    for value in &values[period..] {
        ema += alpha * (value - ema);
        result.push(ema);
    }
    result
}

fn calculate_rvi(bars: &[CalculationBar], period: usize) -> Option<RviSnapshot> {
    if period == 0 || bars.len() < period * 2 - 1 {
        return None;
    }
    let closes = bars.iter().map(|bar| bar.close).collect::<Vec<_>>();
    let mut upward = Vec::with_capacity(closes.len() - period + 1);
    let mut downward = Vec::with_capacity(closes.len() - period + 1);
    for index in period - 1..closes.len() {
        let window = &closes[index + 1 - period..=index];
        let mean = window.iter().sum::<f64>() / period as f64;
        let deviation = (window
            .iter()
            .map(|value| (value - mean).powi(2))
            .sum::<f64>()
            / period as f64)
            .sqrt();
        match closes[index].partial_cmp(&closes[index - 1]) {
            Some(std::cmp::Ordering::Greater) => {
                upward.push(deviation);
                downward.push(0.0);
            }
            Some(std::cmp::Ordering::Less) => {
                upward.push(0.0);
                downward.push(deviation);
            }
            _ => {
                upward.push(0.0);
                downward.push(0.0);
            }
        }
    }
    let smoothed_upward = wilder_latest(&upward, period)?;
    let smoothed_downward = wilder_latest(&downward, period)?;
    let total = smoothed_upward + smoothed_downward;
    if total <= f64::EPSILON {
        return None;
    }
    Some(RviSnapshot {
        period,
        value: round4(smoothed_upward / total * 100.0),
    })
}

fn calculate_atr(bars: &[CalculationBar], period: usize) -> Option<f64> {
    let mut previous_close = None;
    let mut true_ranges = Vec::with_capacity(bars.len());
    for bar in bars {
        let value = bar.high.zip(bar.low).map(|(high, low)| {
            previous_close
                .map(|close: f64| {
                    (high - low)
                        .max((high - close).abs())
                        .max((low - close).abs())
                })
                .unwrap_or(high - low)
        });
        true_ranges.push(value);
        previous_close = Some(bar.close);
    }
    let trailing = true_ranges
        .iter()
        .rev()
        .take_while(|value| value.is_some())
        .filter_map(|value| *value)
        .collect::<Vec<_>>();
    if trailing.len() < period {
        return None;
    }
    let ordered = trailing.into_iter().rev().collect::<Vec<_>>();
    wilder_latest(&ordered, period)
}

fn wilder_latest(values: &[f64], period: usize) -> Option<f64> {
    if period == 0 || values.len() < period {
        return None;
    }
    let mut average = values[..period].iter().sum::<f64>() / period as f64;
    for value in &values[period..] {
        average = (average * (period - 1) as f64 + value) / period as f64;
    }
    average.is_finite().then_some(average)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HistoryBar;

    fn bar(index: usize, close: f64, range: f64) -> HistoryBar {
        HistoryBar {
            date: format!("2020-01-{:02}", index + 1),
            open: Some(close),
            high: Some(close + range / 2.0),
            low: Some(close - range / 2.0),
            close,
            volume: Some(1_000.0),
            capital: None,
        }
    }

    #[test]
    fn atr_true_range_includes_gap_from_previous_close() {
        let mut bars = (0..14)
            .map(|index| bar(index, 100.0, 2.0))
            .collect::<Vec<_>>();
        bars[13] = bar(13, 110.0, 2.0);

        let snapshot = calculate_volatility_snapshot("TEST", &bars).expect("snapshot");
        let atr = snapshot.atr.expect("ATR14");

        assert_eq!(atr.period, 14);
        assert_eq!(atr.value, 2.6429);
        assert_eq!(atr.percent_of_close, 2.4026);
    }

    #[test]
    fn calculates_bollinger_bands_from_population_deviation() {
        let bars = (0..20)
            .map(|index| bar(index, 2.0 + index as f64, 2.0))
            .collect::<Vec<_>>();

        let snapshot = calculate_volatility_snapshot("TEST", &bars).expect("snapshot");
        let bands = snapshot.bollinger_bands.expect("Bollinger 20/2");

        assert_eq!(bands.period, 20);
        assert_eq!(bands.multiplier, 2.0);
        assert_eq!(bands.middle, 11.5);
        assert_eq!(bands.upper, 23.0326);
        assert_eq!(bands.lower, -0.0326);
        assert_eq!(bands.percent_b, Some(91.1877));
        assert_eq!(bands.bandwidth_percent, Some(200.5663));
    }

    #[test]
    fn calculates_donchian_channel_from_window_extremes() {
        let bars = (0..20)
            .map(|index| bar(index, 2.0 + index as f64, 2.0))
            .collect::<Vec<_>>();

        let snapshot = calculate_volatility_snapshot("TEST", &bars).expect("snapshot");
        let channel = snapshot.donchian_channel.expect("Donchian 20");

        assert_eq!(channel.period, 20);
        assert_eq!(channel.upper, 22.0);
        assert_eq!(channel.middle, 11.5);
        assert_eq!(channel.lower, 1.0);
        assert_eq!(channel.width_percent, Some(182.6087));
        assert_eq!(channel.position_percent, Some(95.2381));
    }

    #[test]
    fn calculates_keltner_channel_from_ema_and_atr() {
        let bars = (0..20)
            .map(|index| bar(index, 2.0 + index as f64, 2.0))
            .collect::<Vec<_>>();

        let snapshot = calculate_volatility_snapshot("TEST", &bars).expect("snapshot");
        let channel = snapshot.keltner_channel.expect("Keltner 20/10/2");

        assert_eq!(channel.ema_period, 20);
        assert_eq!(channel.atr_period, 10);
        assert_eq!(channel.multiplier, 2.0);
        assert_eq!(channel.upper, 15.5);
        assert_eq!(channel.middle, 11.5);
        assert_eq!(channel.lower, 7.5);
        assert_eq!(channel.width_percent, Some(69.5652));
        assert_eq!(channel.position_percent, Some(168.75));
    }

    #[test]
    fn calculates_chaikin_volatility_as_ema_range_rate_of_change() {
        let bars = (0..20)
            .map(|index| {
                let range = if index < 10 { 2.0 } else { 4.0 };
                bar(index, 10.0 + index as f64, range)
            })
            .collect::<Vec<_>>();

        let snapshot = calculate_volatility_snapshot("TEST", &bars).expect("snapshot");
        let chaikin = snapshot.chaikin_volatility.expect("Chaikin 10/10");

        assert_eq!(chaikin.ema_period, 10);
        assert_eq!(chaikin.roc_period, 10);
        assert_eq!(chaikin.value, 86.5569);
    }

    #[test]
    fn calculates_rvi_for_mixed_upward_and_downward_volatility() {
        let bars = (0..28)
            .map(|index| bar(index, 10.0 + (index % 2) as f64, 2.0))
            .collect::<Vec<_>>();

        let snapshot = calculate_volatility_snapshot("TEST", &bars).expect("snapshot");
        let rvi = snapshot.rvi.expect("RVI14");

        assert_eq!(rvi.period, 14);
        assert_eq!(rvi.value, 53.5714);
    }

    #[test]
    fn keeps_indicators_unavailable_when_history_is_too_short() {
        let bars = (0..5)
            .map(|index| bar(index, 10.0 + index as f64, 2.0))
            .collect::<Vec<_>>();

        let snapshot = calculate_volatility_snapshot("TEST", &bars).expect("snapshot");

        assert!(snapshot.atr.is_none());
        assert!(snapshot.bollinger_bands.is_none());
        assert!(snapshot.donchian_channel.is_none());
        assert!(snapshot.keltner_channel.is_none());
        assert!(snapshot.chaikin_volatility.is_none());
        assert!(snapshot.rvi.is_none());
        assert_eq!(snapshot.unavailable.len(), 6);
        assert!(snapshot.unavailable[0].reason.contains("14"));
    }

    #[test]
    fn close_only_history_does_not_fabricate_high_low_indicators() {
        let bars = (0..28)
            .map(|index| {
                let mut item = bar(index, 10.0 + index as f64, 2.0);
                item.high = None;
                item.low = None;
                item
            })
            .collect::<Vec<_>>();

        let snapshot = calculate_volatility_snapshot("TEST", &bars).expect("snapshot");

        assert!(snapshot.atr.is_none());
        assert!(snapshot.bollinger_bands.is_some());
        assert!(snapshot.donchian_channel.is_none());
        assert!(snapshot.keltner_channel.is_none());
        assert!(snapshot.chaikin_volatility.is_none());
        assert_eq!(snapshot.rvi.expect("close-only RVI").value, 100.0);
        assert!(snapshot
            .unavailable
            .iter()
            .any(|item| item.reason.contains("高低价缺失")));
    }

    #[test]
    fn inconsistent_ohlc_is_unavailable_for_range_indicators() {
        let bars = (0..28)
            .map(|index| {
                let close = 10.0 + index as f64;
                let mut item = bar(index, close, 2.0);
                item.high = Some(close - 0.5);
                item.low = Some(close - 1.0);
                item
            })
            .collect::<Vec<_>>();

        let snapshot = calculate_volatility_snapshot("TEST", &bars).expect("snapshot");

        assert!(snapshot.atr.is_none());
        assert!(snapshot.bollinger_bands.is_some());
        assert!(snapshot.donchian_channel.is_none());
        assert!(snapshot.keltner_channel.is_none());
        assert!(snapshot.chaikin_volatility.is_none());
        assert!(snapshot.rvi.is_some());
        assert!(snapshot
            .unavailable
            .iter()
            .any(|item| item.reason.contains("不一致")));
    }

    #[test]
    fn inconsistent_open_is_unavailable_for_range_indicators() {
        let bars = (0..28)
            .map(|index| {
                let close = 10.0 + index as f64;
                let mut item = bar(index, close, 2.0);
                item.open = Some(close + 2.0);
                item
            })
            .collect::<Vec<_>>();

        let snapshot = calculate_volatility_snapshot("TEST", &bars).expect("snapshot");

        assert!(snapshot.atr.is_none());
        assert!(snapshot.bollinger_bands.is_some());
        assert!(snapshot.donchian_channel.is_none());
        assert!(snapshot.keltner_channel.is_none());
        assert!(snapshot.chaikin_volatility.is_none());
        assert!(snapshot.rvi.is_some());
        assert!(snapshot
            .unavailable
            .iter()
            .any(|item| item.reason.contains("不一致")));
    }

    #[test]
    fn invalid_latest_close_does_not_fall_back_to_an_earlier_snapshot() {
        let mut bars = (0..28)
            .map(|index| bar(index, 10.0 + index as f64, 2.0))
            .collect::<Vec<_>>();
        bars.push(bar(28, 0.0, 0.0));

        let snapshot = calculate_volatility_snapshot("TEST", &bars).expect("snapshot");

        assert_eq!(snapshot.date, "2020-01-29");
        assert_eq!(snapshot.close, None);
        assert!(snapshot.atr.is_none());
        assert_eq!(snapshot.unavailable.len(), 6);
        assert!(snapshot
            .unavailable
            .iter()
            .all(|item| item.reason == "区间末收盘价无效"));
    }

    #[test]
    fn flat_history_avoids_non_finite_ratios() {
        let bars = (0..28)
            .map(|index| bar(index, 10.0, 0.0))
            .collect::<Vec<_>>();

        let snapshot = calculate_volatility_snapshot("TEST", &bars).expect("snapshot");
        let serialized = serde_json::to_string(&snapshot).expect("serializable snapshot");

        assert_eq!(snapshot.atr.expect("zero ATR").value, 0.0);
        assert_eq!(
            snapshot.bollinger_bands.expect("flat bands").percent_b,
            None
        );
        assert_eq!(
            snapshot
                .donchian_channel
                .expect("flat Donchian")
                .position_percent,
            None
        );
        assert_eq!(
            snapshot
                .keltner_channel
                .expect("flat Keltner")
                .position_percent,
            None
        );
        assert!(snapshot.chaikin_volatility.is_none());
        assert!(snapshot.rvi.is_none());
        assert!(!serialized.contains("NaN"));
        assert!(!serialized.contains("Infinity"));
    }
}
