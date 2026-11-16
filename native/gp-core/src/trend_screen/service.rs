use crate::data_source::{CoreDataSet, MarketDataSource, StaticDataSource};
use crate::dates::capital_trade_date;
use crate::demo_data::MockDataSource;
use crate::error::CoreResult;
use crate::market_data::StockItem;
use crate::numeric::round6;
use crate::screening::criteria::deducted_profit_rule_notes;
use crate::screening::service::screen_candidate_pool;
use crate::trend::indicators::trend_notes;
use crate::trend::service::trend_with_source;
use crate::trend::types::TrendIndicatorRequest;
use crate::trend_screen::rules::{
    layered_trend_score, trend_history_behind_quote, trend_screen_candidate_allowed,
    trend_screen_explanation,
};
use crate::trend_screen::types::{TrendScreenRequest, TrendScreenResult, TrendStockSignal};

pub fn trend_screen_with_mock(request: &TrendScreenRequest) -> CoreResult<TrendScreenResult> {
    trend_screen_with_source(&MockDataSource, request)
}

pub fn trend_screen_with_data(
    data: &CoreDataSet,
    request: &TrendScreenRequest,
) -> CoreResult<TrendScreenResult> {
    let source = StaticDataSource::new(data);
    trend_screen_with_source(&source, request)
}

pub fn trend_screen_candidate_codes(
    universe: &[StockItem],
    request: &TrendScreenRequest,
) -> Vec<String> {
    screen_candidate_pool(universe, &request.criteria, request.limit)
        .into_iter()
        .map(|candidate| candidate.stock.code)
        .collect()
}

pub fn trend_screen_with_source(
    source: &impl MarketDataSource,
    request: &TrendScreenRequest,
) -> CoreResult<TrendScreenResult> {
    let universe = source.stocks()?;
    let candidate_pool = screen_candidate_pool(&universe, &request.criteria, request.limit);
    let mut items = Vec::new();
    let mut skipped = 0_usize;
    let mut outdated = Vec::new();

    for candidate in &candidate_pool {
        let trend_request = TrendIndicatorRequest {
            code: candidate.stock.code.clone(),
            start_date: request.start_date.clone(),
            end_date: request.end_date.clone(),
            series_limit: 60,
        };
        let Ok(analysis) = trend_with_source(source, &trend_request) else {
            skipped += 1;
            continue;
        };
        if trend_history_behind_quote(&candidate.stock, &analysis.signal, &request.end_date) {
            outdated.push(format!(
                "{}：日线截至 {}，落后于 {} 的行情，需补齐日线后重新计算依据。",
                candidate.stock.code,
                analysis.signal.date,
                capital_trade_date(candidate.stock.quote_time.as_deref().unwrap_or_default())
                    .unwrap_or_default(),
            ));
            continue;
        }
        let layered = layered_trend_score(&candidate.stock, &analysis);
        if !trend_screen_candidate_allowed(&layered) {
            skipped += 1;
            continue;
        }
        let mut signal = analysis.signal.clone();
        signal.signal_type = layered.signal_type.clone();
        signal.risk_flags = layered.risk_flags.clone();
        signal.technical_score = Some(round6(layered.technical_score));
        signal.pattern_layer_score = Some(round6(layered.pattern_score));
        signal.quality_score = Some(round6(layered.quality_score));
        let mut reasons = candidate.reasons.clone();
        reasons.extend(
            signal
                .reasons
                .iter()
                .filter(|reason| {
                    !signal.risk_flags.contains(reason)
                        && reason.as_str() != "cyan_watch"
                        && !(reason.as_str() == "kdj_oversold" && signal.kdj_overbought)
                })
                .cloned(),
        );
        reasons.extend(layered.reason_tags.clone());
        reasons.sort();
        reasons.dedup();
        items.push(TrendStockSignal {
            stock: candidate.stock.clone(),
            base_score: round6(candidate.score),
            trend_score: round6(layered.trend_score),
            final_score: round6(layered.final_score),
            signal: signal.clone(),
            reasons,
            explanation: trend_screen_explanation(candidate, &signal, &layered),
        });
    }
    items.sort_by(|left, right| right.final_score.total_cmp(&left.final_score));
    items.truncate(request.limit.clamp(1, 100));
    let mut notes = trend_notes();
    notes.extend(deducted_profit_rule_notes(&universe, &request.criteria));
    if !outdated.is_empty() {
        notes.push(format!(
            "{} 只候选的日线落后行情，未使用旧指标参与当前趋势排名。",
            outdated.len()
        ));
        notes.extend(outdated.into_iter().take(20));
    }
    if skipped > 0 {
        notes.push(format!(
            "Skipped {skipped} stocks without usable OHLC history."
        ));
    }
    Ok(TrendScreenResult {
        total: candidate_pool.len(),
        returned: items.len(),
        items,
        screen_style: "balanced_swing".to_string(),
        notes,
    })
}
