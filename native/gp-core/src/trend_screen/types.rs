use crate::dates::{default_end_date, default_start_date};
use crate::market_data::StockItem;
use crate::screening::types::{ScreenCriteria, SelectionExplanation};
use crate::trend::types::TrendIndicatorSignal;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrendScreenRequest {
    #[serde(default)]
    pub criteria: ScreenCriteria,
    #[serde(default = "default_start_date")]
    pub start_date: String,
    #[serde(default = "default_end_date")]
    pub end_date: String,
    #[serde(default = "default_trend_limit")]
    pub limit: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrendStockSignal {
    pub stock: StockItem,
    pub base_score: f64,
    pub trend_score: f64,
    pub final_score: f64,
    pub signal: TrendIndicatorSignal,
    #[serde(default)]
    pub reasons: Vec<String>,
    #[serde(default)]
    pub explanation: SelectionExplanation,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrendScreenResult {
    pub total: usize,
    pub returned: usize,
    pub items: Vec<TrendStockSignal>,
    #[serde(default = "default_trend_screen_style")]
    pub screen_style: String,
    #[serde(default)]
    pub notes: Vec<String>,
}

fn default_trend_screen_style() -> String {
    "balanced_swing".to_string()
}

fn default_trend_limit() -> usize {
    10
}
