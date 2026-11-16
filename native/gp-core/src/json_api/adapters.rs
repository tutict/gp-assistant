use crate::agent::{
    run_agent_stream_with_data_events_with_context, run_agent_with_data_and_context,
    run_agent_with_mock, AgentRequest, AgentStreamEvent,
};
use crate::backtest::{backtest_with_data, backtest_with_mock, BacktestRequest};
use crate::data_source::{validate_data_set, CoreDataSet};
use crate::error::CoreResult;
use crate::graph_compat::{graph_screen_with_data, graph_screen_with_mock, GraphScreenRequest};
use crate::json_api::requests::{
    AgentStreamWithDataRequest, AgentWithDataRequest, BacktestWithDataRequest,
    GraphScreenWithDataRequest, ObserveWithDataRequest, ScreenWithDataRequest,
    SectorScreenWithDataRequest, TrendScreenWithDataRequest, TrendWithDataRequest,
};
use crate::mobile_skill::{run_mobile_stock_skill, MobileStockSkillRequest};
use crate::observation::{observe_with_data, observe_with_mock, StockObserveRequest};
use crate::screening::{
    screen_with_data, screen_with_mock, sector_screen_with_data, sector_screen_with_mock,
    ScreenCriteria, SectorScreenRequest,
};
use crate::trend::{trend_with_data, trend_with_mock, TrendIndicatorRequest};
use crate::trend_screen::{trend_screen_with_data, trend_screen_with_mock, TrendScreenRequest};
use serde_json::Value;

pub fn screen_value(payload: Value) -> CoreResult<Value> {
    let criteria: ScreenCriteria = serde_json::from_value(payload)?;
    serde_json::to_value(screen_with_mock(&criteria)).map_err(Into::into)
}

pub fn sector_screen_value(payload: Value) -> CoreResult<Value> {
    let request: SectorScreenRequest = serde_json::from_value(payload)?;
    serde_json::to_value(sector_screen_with_mock(&request)).map_err(Into::into)
}

pub fn screen_with_data_value(payload: Value) -> CoreResult<Value> {
    let request: ScreenWithDataRequest = serde_json::from_value(payload)?;
    serde_json::to_value(screen_with_data(&request.data, &request.criteria)?).map_err(Into::into)
}

pub fn sector_screen_with_data_value(payload: Value) -> CoreResult<Value> {
    let request: SectorScreenWithDataRequest = serde_json::from_value(payload)?;
    serde_json::to_value(sector_screen_with_data(&request.data, &request.request)?)
        .map_err(Into::into)
}

pub fn graph_screen_value(payload: Value) -> CoreResult<Value> {
    let request: GraphScreenRequest = serde_json::from_value(payload)?;
    serde_json::to_value(graph_screen_with_mock(&request)).map_err(Into::into)
}

pub fn graph_screen_with_data_value(payload: Value) -> CoreResult<Value> {
    let request: GraphScreenWithDataRequest = serde_json::from_value(payload)?;
    serde_json::to_value(graph_screen_with_data(&request.data, &request.request)?)
        .map_err(Into::into)
}

pub fn backtest_value(payload: Value) -> CoreResult<Value> {
    let request: BacktestRequest = serde_json::from_value(payload)?;
    serde_json::to_value(backtest_with_mock(&request)?).map_err(Into::into)
}

pub fn backtest_with_data_value(payload: Value) -> CoreResult<Value> {
    let request: BacktestWithDataRequest = serde_json::from_value(payload)?;
    serde_json::to_value(backtest_with_data(&request.data, &request.request)?).map_err(Into::into)
}

pub fn trend_value(payload: Value) -> CoreResult<Value> {
    let request: TrendIndicatorRequest = serde_json::from_value(payload)?;
    serde_json::to_value(trend_with_mock(&request)?).map_err(Into::into)
}

pub fn observe_value(payload: Value) -> CoreResult<Value> {
    let request: StockObserveRequest = serde_json::from_value(payload)?;
    serde_json::to_value(observe_with_mock(&request)?).map_err(Into::into)
}

pub fn trend_with_data_value(payload: Value) -> CoreResult<Value> {
    let request: TrendWithDataRequest = serde_json::from_value(payload)?;
    serde_json::to_value(trend_with_data(&request.data, &request.request)?).map_err(Into::into)
}

pub fn observe_with_data_value(payload: Value) -> CoreResult<Value> {
    let request: ObserveWithDataRequest = serde_json::from_value(payload)?;
    serde_json::to_value(observe_with_data(&request.data, &request.request)?).map_err(Into::into)
}

pub fn trend_screen_value(payload: Value) -> CoreResult<Value> {
    let request: TrendScreenRequest = serde_json::from_value(payload)?;
    serde_json::to_value(trend_screen_with_mock(&request)?).map_err(Into::into)
}

pub fn trend_screen_with_data_value(payload: Value) -> CoreResult<Value> {
    let request: TrendScreenWithDataRequest = serde_json::from_value(payload)?;
    serde_json::to_value(trend_screen_with_data(&request.data, &request.request)?)
        .map_err(Into::into)
}

pub fn agent_value(payload: Value) -> CoreResult<Value> {
    let request: AgentRequest = serde_json::from_value(payload)?;
    serde_json::to_value(run_agent_with_mock(&request.message)?).map_err(Into::into)
}

pub fn agent_with_data_value(payload: Value) -> CoreResult<Value> {
    let request: AgentWithDataRequest = serde_json::from_value(payload)?;
    serde_json::to_value(run_agent_with_data_and_context(
        &request.data,
        &request.message,
        &request.context,
    )?)
    .map_err(Into::into)
}

pub fn agent_stream_with_data_events_value(payload: Value) -> CoreResult<Vec<AgentStreamEvent>> {
    let request: AgentStreamWithDataRequest = serde_json::from_value(payload)?;
    Ok(run_agent_stream_with_data_events_with_context(
        &request.data,
        &request.message,
        request.run_id.as_deref(),
        request.mode.as_deref(),
        &request.context,
    ))
}

pub fn mobile_stock_skill_value(payload: Value) -> CoreResult<Value> {
    let request: MobileStockSkillRequest = serde_json::from_value(payload)?;
    serde_json::to_value(run_mobile_stock_skill(&request)).map_err(Into::into)
}

pub fn validate_data_source_value(payload: Value) -> CoreResult<Value> {
    let data: CoreDataSet = serde_json::from_value(payload)?;
    serde_json::to_value(validate_data_set(&data)?).map_err(Into::into)
}
