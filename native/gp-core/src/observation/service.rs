use crate::data_source::{CoreDataSet, MarketDataSource, StaticDataSource};
use crate::demo_data::MockDataSource;
use crate::error::{CoreError, CoreResult};
use crate::market_data::normalize_stock_code;
use crate::observation::capital::build_observation_capital_evidence;
use crate::observation::financial::build_observation_financial_indicators;
use crate::observation::types::{StockObservation, StockObserveRequest};
use crate::trend::service::trend_with_source;
use crate::trend::types::{clamp_series_limit, TrendIndicatorRequest};

pub fn observe_with_mock(request: &StockObserveRequest) -> CoreResult<StockObservation> {
    observe_with_source(&MockDataSource, request)
}

pub fn observe_with_data(
    data: &CoreDataSet,
    request: &StockObserveRequest,
) -> CoreResult<StockObservation> {
    let source = StaticDataSource::new(data);
    observe_with_source(&source, request)
}

pub fn observe_with_source(
    source: &impl MarketDataSource,
    request: &StockObserveRequest,
) -> CoreResult<StockObservation> {
    let code = normalize_stock_code(&request.code);
    let universe = source.stocks()?;
    let stock = universe
        .iter()
        .find(|stock| normalize_stock_code(&stock.code) == code)
        .cloned()
        .ok_or_else(|| {
            CoreError::new(format!(
                "{}{}",
                "\u{672a}\u{627e}\u{5230}\u{80a1}\u{7968}\u{ff1a}", request.code
            ))
        })?;
    let mut notes = vec!["\u{6570}\u{636e}\u{6e90}\u{ff1a}Tauri/Rust \u{7edf}\u{4e00}\u{672c}\u{5730}\u{884c}\u{60c5}\u{7f13}\u{5b58}\u{3002}".to_string()];
    let financial_snapshot = source.get_financial_snapshot(&stock.code);
    let provided_capital_evidence = source.get_capital_evidence(&stock.code);
    let financial_indicators =
        build_observation_financial_indicators(&stock, financial_snapshot.as_ref());
    let trend_request = TrendIndicatorRequest {
        code: stock.code.clone(),
        start_date: request.start_date.clone(),
        end_date: request.end_date.clone(),
        series_limit: clamp_series_limit(request.series_limit),
    };
    let trend = match trend_with_source(source, &trend_request) {
        Ok(result) => Some(result),
        Err(error) => {
            notes.push(format!(
                "{}{}",
                "\u{8d8b}\u{52bf}\u{6307}\u{6807}\u{4e0d}\u{53ef}\u{7528}\u{ff1a}", error
            ));
            None
        }
    };
    if request.include_order_book {
        notes.push("基准为候选池买入持有等权曲线，用于衡量调仓与成本后的超额收益。".to_string());
    }
    let capital_evidence = build_observation_capital_evidence(
        &stock,
        &financial_indicators,
        trend.as_ref(),
        &request.end_date,
        provided_capital_evidence,
    );
    Ok(StockObservation {
        source: "tdx".to_string(),
        stock,
        financial_indicators: Some(financial_indicators),
        trend,
        capital_evidence: Some(capital_evidence),
        order_book: None,
        notes,
    })
}
