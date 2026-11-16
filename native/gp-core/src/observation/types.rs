use crate::capital_evidence::CapitalEvidenceResult;
use crate::dates::{default_end_date, default_start_date};
use crate::market_data::StockItem;
use crate::screening::types::default_tone;
use crate::trend::types::{default_series_limit, TrendIndicatorResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StockObserveRequest {
    pub code: String,
    #[serde(default = "default_start_date")]
    pub start_date: String,
    #[serde(default = "default_end_date")]
    pub end_date: String,
    #[serde(default = "default_series_limit")]
    pub series_limit: usize,
    #[serde(default)]
    pub include_order_book: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FinancialIndicatorItem {
    pub label: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default = "default_tone")]
    pub tone: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FinancialIndicators {
    pub title: String,
    pub period: String,
    pub source: String,
    pub items: Vec<FinancialIndicatorItem>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StockObservation {
    pub source: String,
    pub stock: StockItem,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub financial_indicators: Option<FinancialIndicators>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trend: Option<TrendIndicatorResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capital_evidence: Option<CapitalEvidenceResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order_book: Option<Value>,
    #[serde(default)]
    pub notes: Vec<String>,
}
