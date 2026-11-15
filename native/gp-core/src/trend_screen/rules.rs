use crate::dates::{capital_trade_date, parse_date};
use crate::market_data::StockItem;
use crate::numeric::{ema, round2};
use crate::rates::{percentage_points, ratio_percentage_points};
use crate::screening::scoring::score_contribution;
use crate::screening::types::{ScoreContribution, ScreenedStock, SelectionExplanation};
use crate::trend::types::{TrendIndicatorPoint, TrendIndicatorResult, TrendIndicatorSignal};

pub(super) fn trend_history_behind_quote(
    stock: &StockItem,
    signal: &TrendIndicatorSignal,
    end_date: &str,
) -> bool {
    let quote_date = stock.quote_time.as_deref().and_then(capital_trade_date);
    let Some(quote_date) = quote_date.and_then(|date| parse_date(&date).ok()) else {
        return false;
    };
    match (parse_date(&signal.date), parse_date(end_date)) {
        // A deliberately historical window can precede the current quote.
        (Ok(history_date), Ok(end_date)) => quote_date <= end_date && history_date < quote_date,
        _ => false,
    }
}

#[derive(Clone, Debug)]
pub(super) struct TrendLayerScores {
    pub(super) technical_score: f64,
    pub(super) pattern_score: f64,
    pub(super) quality_score: f64,
    pub(super) trend_score: f64,
    pub(super) final_score: f64,
    pub(super) signal_type: String,
    pub(super) risk_flags: Vec<String>,
    pub(super) reason_tags: Vec<String>,
}

pub(super) fn layered_trend_score(
    stock: &StockItem,
    trend: &TrendIndicatorResult,
) -> TrendLayerScores {
    let signal = &trend.signal;
    let bars = &trend.series;
    let technical_score = technical_layer_score(signal, bars);
    let pattern_score = pattern_layer_score(signal, bars);
    let quality_score = quality_layer_score(stock);
    let mut risk_flags = trend_layer_risk_flags(signal, bars);
    let mut final_score = technical_score * 0.70 + pattern_score * 0.20 + quality_score * 0.10;
    if risk_flags
        .iter()
        .any(|flag| flag == "breakdown_ma20" || flag == "bearish_long_ma_stack")
    {
        final_score -= 18.0;
    }
    if risk_flags
        .iter()
        .any(|flag| flag == "macd_bearish_divergence" || flag == "volume_stall")
    {
        final_score -= 8.0;
    }
    final_score = final_score.clamp(0.0, 100.0);
    let signal_type = classify_trend_signal(signal, bars, final_score, &risk_flags);
    if signal_type == "risk_warning" && !risk_flags.iter().any(|flag| flag == "weak_final_score") {
        risk_flags.push("weak_final_score".to_string());
    }
    risk_flags.sort();
    risk_flags.dedup();
    let reason_tags = layered_reason_tags(
        signal,
        bars,
        &signal_type,
        technical_score,
        pattern_score,
        quality_score,
    );
    TrendLayerScores {
        technical_score: round2(technical_score),
        pattern_score: round2(pattern_score),
        quality_score: round2(quality_score),
        trend_score: round2(final_score),
        final_score: round2(final_score),
        signal_type,
        risk_flags,
        reason_tags,
    }
}

pub(super) fn trend_screen_candidate_allowed(layered: &TrendLayerScores) -> bool {
    !layered
        .risk_flags
        .iter()
        .any(|flag| flag == "bearish_long_ma_stack")
}

fn technical_layer_score(signal: &TrendIndicatorSignal, bars: &[TrendIndicatorPoint]) -> f64 {
    if bars.len() < 45 {
        return 0.0;
    }
    let closes = closes_from_points(bars);
    let volumes = volumes_from_points(bars);
    let Some(close) = closes
        .last()
        .copied()
        .filter(|value| value.is_finite() && *value > 0.0)
    else {
        return 0.0;
    };
    let ma5 = trailing_mean(&closes, 5);
    let ma10 = trailing_mean(&closes, 10);
    let ma20 = trailing_mean(&closes, 20);
    let ma60 = trailing_mean(&closes, 60);
    let high20 = trailing_max(&closes, 20);
    let high60 = trailing_max(&closes, 60);
    let low60 = trailing_min(&closes, 60);
    let volume5 = trailing_mean(&volumes, 5);
    let volume20 = trailing_mean(&volumes, 20);
    let ret20 = closes
        .len()
        .checked_sub(21)
        .and_then(|idx| pct_change(close, closes[idx]))
        .unwrap_or(0.0);
    let ret60 = closes
        .len()
        .checked_sub(61)
        .and_then(|idx| pct_change(close, closes[idx]))
        .unwrap_or(ret20);
    let range_position60 = if high60 > low60 {
        ((close - low60) / (high60 - low60)).clamp(0.0, 1.0)
    } else {
        0.5
    };
    let drawdown20 = if high20 > 0.0 {
        (close / high20 - 1.0).min(0.0).abs() * 100.0
    } else {
        100.0
    };
    let macd = macd_snapshot(&closes);
    let mut score = 0.0;

    if ma5 > ma10 && ma10 > ma20 && ma20 > ma60 {
        score += 18.0;
    } else if ma5 > ma10 && ma10 > ma20 {
        score += 12.0;
    } else if close > ma20 && ma20 > ma60 {
        score += 8.0;
    }
    if close > ma5 {
        score += 4.0;
    }
    if close > ma20 {
        score += 5.0;
    }
    if close > ma60 {
        score += 5.0;
    }
    if ma20 > ma60 {
        score += 5.0;
    }
    score += (ret20.clamp(-8.0, 18.0) + 8.0) / 26.0 * 10.0;
    score += (ret60.clamp(-12.0, 45.0) + 12.0) / 57.0 * 8.0;
    score += range_position60 * 7.0;
    score += (1.0 - (drawdown20 / 18.0).clamp(0.0, 1.0)) * 7.0;

    if macd.dif > macd.dea && macd.hist > 0.0 {
        score += 8.0;
    }
    if macd.prev_dif <= macd.prev_dea && macd.dif > macd.dea {
        score += 7.0;
    }
    if macd.hist > macd.prev_hist {
        score += 5.0;
    }
    if signal.kdj_golden_cross {
        score += 7.0;
    }
    if signal.kdj_oversold && signal.k.unwrap_or(0.0) > signal.d.unwrap_or(0.0) {
        score += 4.0;
    }
    if volume20 > 0.0 && volume5 > volume20 * 1.15 && close >= high20 * 0.98 {
        score += 7.0;
    }
    if volume20 > 0.0 && volume5 <= volume20 * 0.95 && close >= ma20 * 0.98 && close <= ma20 * 1.08
    {
        score += 5.0;
    }
    if signal.swl_above_sws {
        score += 7.0;
    }
    if signal
        .swl
        .zip(signal.sws)
        .map(|(swl, sws)| swl > sws * 1.01)
        .unwrap_or(false)
    {
        score += 3.0;
    }
    if signal.quant_score_max > 0 {
        score += (signal.quant_score as f64 / signal.quant_score_max as f64).clamp(0.0, 1.0) * 8.0;
    }
    score.clamp(0.0, 100.0)
}

fn pattern_layer_score(signal: &TrendIndicatorSignal, bars: &[TrendIndicatorPoint]) -> f64 {
    let closes = closes_from_points(bars);
    let volumes = volumes_from_points(bars);
    let close = closes.last().copied().unwrap_or(signal.close);
    let ma20 = trailing_mean(&closes, 20);
    let ma60 = trailing_mean(&closes, 60);
    let high20 = trailing_max(&closes, 20);
    let volume5 = trailing_mean(&volumes, 5);
    let volume20 = trailing_mean(&volumes, 20);
    let mut score = (signal.pattern_score as f64 / signal.pattern_score_max.max(1) as f64 * 45.0)
        .clamp(0.0, 45.0);
    if close >= high20 * 0.985 && volume20 > 0.0 && volume5 > volume20 * 1.1 {
        score += 18.0;
    }
    if close >= ma20 * 0.98 && close <= ma20 * 1.06 && volume20 > 0.0 && volume5 <= volume20 * 1.05
    {
        score += 14.0;
    }
    if close >= ma60 * 0.98 && close <= ma60 * 1.08 {
        score += 8.0;
    }
    if signal
        .support
        .map(|support| close >= support * 0.98 && close <= support * 1.08)
        .unwrap_or(false)
    {
        score += 7.0;
    }
    if signal
        .pattern_signals
        .iter()
        .any(|value| value == "dragon_trend_volume")
    {
        score += 8.0;
    }
    if signal
        .pattern_signals
        .iter()
        .any(|value| value == "swing_opportunity")
    {
        score += 6.0;
    }
    if signal.white_exit {
        score -= 16.0;
    }
    if signal.kdj_overbought {
        score -= 8.0;
    }
    if high_upper_shadow_ratio(bars).unwrap_or(0.0) >= 0.45 {
        score -= 8.0;
    }
    score.clamp(0.0, 100.0)
}

fn quality_layer_score(stock: &StockItem) -> f64 {
    let mut score: f64 = 45.0;
    if let Some(roe) = stock.roe.and_then(ratio_percentage_points) {
        score += if roe >= 15.0 {
            18.0
        } else if roe >= 8.0 {
            10.0
        } else if roe >= 0.0 {
            2.0
        } else {
            -14.0
        };
    }
    if stock
        .deducted_net_profit_billion
        .map(|value| value > 0.0)
        .unwrap_or(false)
    {
        score += 14.0;
    } else {
        score -= 8.0;
    }
    if let Some(growth) = stock
        .deducted_net_profit_growth_rate
        .and_then(percentage_points)
    {
        score += if growth >= 20.0 {
            10.0
        } else if growth >= 10.0 {
            6.0
        } else if growth >= 0.0 {
            2.0
        } else {
            -10.0
        };
    }
    if let Some(margin) = stock.deducted_net_profit_margin.and_then(percentage_points) {
        score += if margin >= 15.0 {
            6.0
        } else if margin >= 8.0 {
            3.0
        } else {
            0.0
        };
    }
    if let Some(pe) = stock.pe.filter(|value| value.is_finite() && *value > 0.0) {
        score += if pe <= 35.0 {
            4.0
        } else if pe <= 60.0 {
            0.0
        } else {
            -6.0
        };
    }
    if let Some(pb) = stock.pb.filter(|value| value.is_finite() && *value > 0.0) {
        score += if pb <= 6.0 { 4.0 } else { -4.0 };
    }
    score.clamp(0.0, 100.0)
}

fn classify_trend_signal(
    signal: &TrendIndicatorSignal,
    bars: &[TrendIndicatorPoint],
    final_score: f64,
    risk_flags: &[String],
) -> String {
    if final_score < 65.0
        || risk_flags
            .iter()
            .any(|flag| flag == "breakdown_ma20" || flag == "bearish_long_ma_stack")
    {
        return "risk_warning".to_string();
    }
    let closes = closes_from_points(bars);
    let volumes = volumes_from_points(bars);
    let close = closes.last().copied().unwrap_or(signal.close);
    let high20 = trailing_max(&closes, 20);
    let ma20 = trailing_mean(&closes, 20);
    let volume5 = trailing_mean(&volumes, 5);
    let volume20 = trailing_mean(&volumes, 20);
    if close >= high20 * 0.985 && volume20 > 0.0 && volume5 >= volume20 * 1.1 {
        return "breakout_chase".to_string();
    }
    if close >= ma20 * 0.98
        && close <= ma20 * 1.06
        && (signal.kdj_golden_cross || signal.swl_above_sws)
    {
        return "pullback_buy".to_string();
    }
    "trend_continuation".to_string()
}

fn layered_reason_tags(
    signal: &TrendIndicatorSignal,
    bars: &[TrendIndicatorPoint],
    signal_type: &str,
    technical: f64,
    pattern: f64,
    quality: f64,
) -> Vec<String> {
    let closes = closes_from_points(bars);
    let ma5 = trailing_mean(&closes, 5);
    let ma10 = trailing_mean(&closes, 10);
    let ma20 = trailing_mean(&closes, 20);
    let ma60 = trailing_mean(&closes, 60);
    let close = closes.last().copied().unwrap_or(signal.close);
    let mut tags = vec![format!("signal_type:{signal_type}")];
    if ma5 > ma10 && ma10 > ma20 && ma20 > ma60 {
        tags.push("ma_bull_stack".to_string());
    }
    if close > ma20 {
        tags.push("price_above_ma20".to_string());
    }
    if signal.swl_above_sws {
        tags.push("swl_strength".to_string());
    }
    if signal.kdj_golden_cross {
        tags.push("kdj_golden_cross".to_string());
    }
    if technical >= 75.0 {
        tags.push("technical_score_strong".to_string());
    }
    if pattern >= 70.0 {
        tags.push("pattern_score_strong".to_string());
    }
    if quality >= 70.0 {
        tags.push("quality_soft_bonus".to_string());
    }
    tags
}

fn trend_layer_risk_flags(
    signal: &TrendIndicatorSignal,
    bars: &[TrendIndicatorPoint],
) -> Vec<String> {
    let closes = closes_from_points(bars);
    let volumes = volumes_from_points(bars);
    let close = closes.last().copied().unwrap_or(signal.close);
    let ma20 = trailing_mean(&closes, 20);
    let ma60 = trailing_mean(&closes, 60);
    let ma120 = trailing_mean(&closes, 120);
    let volume5 = trailing_mean(&volumes, 5);
    let volume20 = trailing_mean(&volumes, 20);
    let macd = macd_snapshot(&closes);
    let mut flags = signal.risk_flags.clone();
    if bars.len() < 45 {
        flags.push("insufficient_history".to_string());
    }
    if volume20 <= 0.0 || !volume20.is_finite() {
        flags.push("low_volume".to_string());
    }
    if ma20.is_finite() && close < ma20 * 0.97 {
        flags.push("breakdown_ma20".to_string());
    }
    if ma120.is_finite() && ma20 < ma60 && ma60 < ma120 {
        flags.push("bearish_long_ma_stack".to_string());
    }
    if signal.kdj_overbought {
        flags.push("kdj_overbought".to_string());
    }
    if signal.white_exit {
        flags.push("white_exit".to_string());
    }
    if high_upper_shadow_ratio(bars).unwrap_or(0.0) >= 0.45
        && volume20 > 0.0
        && volume5 > volume20 * 1.1
    {
        flags.push("volume_stall".to_string());
    }
    if macd.dif > 0.0 && macd.hist < macd.prev_hist && close >= trailing_max(&closes, 20) * 0.98 {
        flags.push("macd_bearish_divergence".to_string());
    }
    flags.sort();
    flags.dedup();
    flags
}

fn closes_from_points(bars: &[TrendIndicatorPoint]) -> Vec<f64> {
    bars.iter().map(|bar| bar.close).collect()
}

fn volumes_from_points(bars: &[TrendIndicatorPoint]) -> Vec<f64> {
    bars.iter()
        .map(|bar| bar.volume.unwrap_or(0.0).max(0.0))
        .collect()
}

fn trailing_mean(values: &[f64], window: usize) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    let start = values.len().saturating_sub(window.max(1));
    let mut sum = 0.0;
    let mut count = 0.0;
    for value in values[start..]
        .iter()
        .copied()
        .filter(|value| value.is_finite())
    {
        sum += value;
        count += 1.0;
    }
    if count > 0.0 {
        sum / count
    } else {
        f64::NAN
    }
}

fn trailing_max(values: &[f64], window: usize) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    let start = values.len().saturating_sub(window.max(1));
    values[start..]
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .fold(f64::NEG_INFINITY, f64::max)
}

fn trailing_min(values: &[f64], window: usize) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    let start = values.len().saturating_sub(window.max(1));
    values[start..]
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .fold(f64::INFINITY, f64::min)
}

#[derive(Clone, Copy, Debug, Default)]
struct MacdSnapshot {
    dif: f64,
    dea: f64,
    hist: f64,
    prev_dif: f64,
    prev_dea: f64,
    prev_hist: f64,
}

fn macd_snapshot(closes: &[f64]) -> MacdSnapshot {
    if closes.len() < 2 {
        return MacdSnapshot::default();
    }
    let ema12 = ema(closes, 12);
    let ema26 = ema(closes, 26);
    let dif = ema12
        .iter()
        .zip(ema26.iter())
        .map(|(left, right)| *left - *right)
        .collect::<Vec<_>>();
    let dea = ema(&dif, 9);
    let hist = dif
        .iter()
        .zip(dea.iter())
        .map(|(dif, dea)| 2.0 * (*dif - *dea))
        .collect::<Vec<_>>();
    let last = dif.len() - 1;
    let prev = last.saturating_sub(1);
    MacdSnapshot {
        dif: dif[last],
        dea: dea[last],
        hist: hist[last],
        prev_dif: dif[prev],
        prev_dea: dea[prev],
        prev_hist: hist[prev],
    }
}

fn high_upper_shadow_ratio(bars: &[TrendIndicatorPoint]) -> Option<f64> {
    let bar = bars.last()?;
    let high = bar.high?;
    let open = bar.open.unwrap_or(bar.close);
    let low = bar.low?;
    if !high.is_finite() || !open.is_finite() || !low.is_finite() || high <= low {
        return None;
    }
    Some(((high - open.max(bar.close)) / (high - low)).clamp(0.0, 1.0))
}

pub(super) fn trend_screen_explanation(
    candidate: &ScreenedStock,
    signal: &TrendIndicatorSignal,
    layered: &TrendLayerScores,
) -> SelectionExplanation {
    let mut basis = vec![
        format!("通过基础筛选，基础评分 {:.2}。", candidate.score),
        format!(
            "{}：技术分 {:.2}、形态分 {:.2}、质量分 {:.2}。",
            trend_screen_signal_label(&signal.signal_type),
            layered.technical_score,
            layered.pattern_score,
            layered.quality_score
        ),
        format!(
            "日线截至 {}，收盘价 {:.2} 元；K={}、D={}、J={}。",
            signal.date,
            signal.close,
            trend_indicator_value(signal.k),
            trend_indicator_value(signal.d),
            trend_indicator_value(signal.j),
        ),
    ];
    if let Some(pattern_text) = pattern_basis_text(signal) {
        basis.push(pattern_text);
    }

    let mut score_breakdown = vec![
        score_contribution(
            "technical_score",
            "技术层",
            layered.technical_score,
            layered.technical_score * 0.70,
            75.0,
            60.0,
        ),
        score_contribution(
            "pattern_score",
            "形态层",
            layered.pattern_score,
            layered.pattern_score * 0.20,
            70.0,
            55.0,
        ),
        score_contribution(
            "quality_score",
            "质量层",
            layered.quality_score,
            layered.quality_score * 0.10,
            70.0,
            50.0,
        ),
    ];
    let adjustment = layered.final_score
        - score_breakdown
            .iter()
            .filter_map(|part| part.contribution)
            .sum::<f64>();
    if adjustment.abs() >= 0.01 {
        score_breakdown.push(ScoreContribution {
            key: "risk_adjustment".into(),
            label: "风险调整".into(),
            value: Some(round2(adjustment)),
            contribution: Some(round2(adjustment)),
            tone: "negative".into(),
        });
    }
    SelectionExplanation {
        basis,
        score_breakdown,
        risk_checks: trend_risk_checks(signal),
        verification: vec![
            "观察下一交易日的价格与成交量，确认信号是否延续。".to_string(),
            "结合突破、回踩、趋势延续和风险预警状态继续核验。".to_string(),
        ],
    }
}

fn trend_screen_signal_label(signal_type: &str) -> &str {
    match signal_type {
        "trend_continuation" => "趋势延续",
        "trend_reversal" => "趋势反转",
        "range_bound" => "区间震荡",
        "breakout_attempt" => "突破尝试",
        "breakout_chase" => "突破追踪",
        "pullback_buy" => "回踩观察",
        "risk_warning" => "风险预警",
        _ => "趋势观察",
    }
}

fn trend_indicator_value(value: Option<f64>) -> String {
    value
        .filter(|value| value.is_finite())
        .map(|value| format!("{value:.2}"))
        .unwrap_or_else(|| "—".into())
}

fn pattern_basis_text(signal: &TrendIndicatorSignal) -> Option<String> {
    let mut labels = Vec::new();
    if signal
        .pattern_signals
        .iter()
        .any(|value| value == "bottom_accumulation")
    {
        labels.push("底部吸筹");
    }
    if signal
        .pattern_signals
        .iter()
        .any(|value| value == "swing_opportunity")
    {
        labels.push("波段机会");
    }
    if signal
        .pattern_signals
        .iter()
        .any(|value| value == "rebound_signal")
    {
        labels.push("反弹信号");
    }
    if signal
        .pattern_signals
        .iter()
        .any(|value| value == "dragon_trend_volume")
    {
        labels.push("趋势量能共振");
    }
    if labels.is_empty() {
        None
    } else {
        Some(format!("形态信号：{}。", labels.join("、")))
    }
}

fn trend_risk_checks(signal: &TrendIndicatorSignal) -> Vec<String> {
    let mut risks = Vec::new();
    if signal.kdj_overbought {
        let zone = if signal.kdj_oversold {
            "KDJ高低位信号分歧"
        } else {
            "KDJ超买"
        };
        let triggers = [
            ("K", signal.k, 80.0),
            ("D", signal.d, 80.0),
            ("J", signal.j, 100.0),
        ]
        .into_iter()
        .filter_map(|(label, value, threshold)| {
            value
                .filter(|value| value.is_finite() && *value >= threshold)
                .map(|value| format!("{label}={value:.2}≥{threshold:.0}"))
        })
        .collect::<Vec<_>>();
        risks.push(format!(
            "{zone}：{} 日线，{}；需观察高位回落风险。",
            signal.date,
            triggers.join("、")
        ));
    }
    if signal.kdj_dead_cross {
        risks.push("KDJ死叉，短期动量转弱。".into());
    }
    if signal.white_exit {
        risks.push("白线离场信号已触发，需重新核验短线趋势。".into());
    }
    if signal.cyan_watch {
        risks.push("青线观察状态，趋势确认尚不充分。".into());
    }
    for flag in &signal.risk_flags {
        let text = match flag.as_str() {
            "swl_below_sws" => "短期均线低于长期均线。",
            "breakdown_ma20" => "跌破20日均线：收盘价低于均线3%以上。",
            "bearish_long_ma_stack" => "长期均线空头排列。",
            "volume_stall" => "放量滞涨，需观察上方抛压。",
            "macd_bearish_divergence" => "MACD动量减弱，存在顶背离风险。",
            "weak_final_score" => "趋势强度不足：综合技术评分或趋势结构未通过强势确认。",
            "insufficient_history" => "日线样本不足，技术评分参考性有限。",
            "low_volume" => "成交量不足，信号确认受限。",
            _ => continue,
        };
        risks.push(text.into());
    }
    if risks.is_empty() {
        risks.push("暂未触发已配置的技术风险规则，仍需后续价格与成交量确认。".into());
    }
    risks
}

fn pct_change(current: f64, previous: f64) -> Option<f64> {
    if current.is_finite() && previous.is_finite() && previous.abs() > f64::EPSILON {
        Some((current / previous - 1.0) * 100.0)
    } else {
        None
    }
}
