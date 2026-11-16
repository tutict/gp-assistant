use crate::data_source::{CoreDataSet, MarketDataSource, StaticDataSource};
use crate::demo_data::MockDataSource;
use crate::error::{CoreError, CoreResult};
use crate::trend::indicators::{
    compute_trend_bars, prepare_bars, trend_point_from_bar, trend_signal_from_bar,
};
use crate::trend::types::{clamp_series_limit, TrendIndicatorRequest, TrendIndicatorResult};

pub fn trend_with_mock(request: &TrendIndicatorRequest) -> CoreResult<TrendIndicatorResult> {
    trend_with_source(&MockDataSource, request)
}

pub fn trend_with_data(
    data: &CoreDataSet,
    request: &TrendIndicatorRequest,
) -> CoreResult<TrendIndicatorResult> {
    let source = StaticDataSource::new(data);
    trend_with_source(&source, request)
}

pub fn trend_with_source(
    source: &impl MarketDataSource,
    request: &TrendIndicatorRequest,
) -> CoreResult<TrendIndicatorResult> {
    let universe = source.stocks()?;
    let stock = universe
        .iter()
        .find(|stock| stock.code.as_str() == request.code.as_str())
        .cloned()
        .ok_or_else(|| CoreError::new(format!("未找到股票 {}", request.code)))?;
    let history = source.get_history(&stock.code, &request.start_date, &request.end_date)?;
    let bars = prepare_bars(&history, &stock)?;
    if bars.is_empty() {
        return Err(CoreError::new(format!("{} 没有可用历史行情", stock.code)));
    }
    let computed = compute_trend_bars(&bars);
    let signal = trend_signal_from_bar(&stock.code, computed.last().expect("non-empty trend bars"));
    let limit = clamp_series_limit(request.series_limit);
    let skip = computed.len().saturating_sub(limit);
    let series = computed
        .iter()
        .skip(skip)
        .map(trend_point_from_bar)
        .collect();
    Ok(TrendIndicatorResult {
        stock,
        signal,
        series,
    })
}
