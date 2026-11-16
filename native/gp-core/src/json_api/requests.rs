use crate::agent::AgentContext;
use crate::backtest::BacktestRequest;
use crate::data_source::CoreDataSet;
use crate::graph_compat::GraphScreenRequest;
use crate::observation::StockObserveRequest;
use crate::screening::{ScreenCriteria, SectorScreenRequest};
use crate::trend::TrendIndicatorRequest;
use crate::trend_screen::TrendScreenRequest;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScreenWithDataRequest {
    pub data: CoreDataSet,
    #[serde(default)]
    pub criteria: ScreenCriteria,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SectorScreenWithDataRequest {
    pub data: CoreDataSet,
    #[serde(default)]
    pub request: SectorScreenRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphScreenWithDataRequest {
    pub data: CoreDataSet,
    #[serde(default)]
    pub request: GraphScreenRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BacktestWithDataRequest {
    pub data: CoreDataSet,
    pub request: BacktestRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrendWithDataRequest {
    pub data: CoreDataSet,
    pub request: TrendIndicatorRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObserveWithDataRequest {
    pub data: CoreDataSet,
    pub request: StockObserveRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrendScreenWithDataRequest {
    pub data: CoreDataSet,
    pub request: TrendScreenRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentWithDataRequest {
    pub data: CoreDataSet,
    pub message: String,
    #[serde(default)]
    pub context: AgentContext,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentStreamWithDataRequest {
    pub data: CoreDataSet,
    pub message: String,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub context: AgentContext,
}
