use crate::dates::{default_end_date, default_start_date};
use crate::market_data::StockItem;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrendIndicatorRequest {
    pub code: String,
    #[serde(default = "default_start_date")]
    pub start_date: String,
    #[serde(default = "default_end_date")]
    pub end_date: String,
    #[serde(default = "default_series_limit")]
    pub series_limit: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrendIndicatorPoint {
    pub date: String,
    pub close: f64,
    #[serde(default)]
    pub open: Option<f64>,
    #[serde(default)]
    pub high: Option<f64>,
    #[serde(default)]
    pub low: Option<f64>,
    #[serde(default)]
    pub volume: Option<f64>,
    #[serde(default)]
    pub swl: Option<f64>,
    #[serde(default)]
    pub sws: Option<f64>,
    #[serde(default)]
    pub k: Option<f64>,
    #[serde(default)]
    pub d: Option<f64>,
    #[serde(default)]
    pub j: Option<f64>,
    #[serde(default)]
    pub accumulation_index: Option<f64>,
    #[serde(default)]
    pub accumulation_strength: Option<f64>,
    #[serde(default)]
    pub swing_opportunity: Option<f64>,
    #[serde(default)]
    pub rebound_signal: Option<f64>,
    #[serde(default)]
    pub trend_heat: Option<f64>,
    #[serde(default)]
    pub volume_price_heat: Option<f64>,
    #[serde(default)]
    pub anomaly_heat: Option<f64>,
    #[serde(default)]
    pub popularity_heat: Option<f64>,
    #[serde(default)]
    pub red_hold: bool,
    #[serde(default)]
    pub cyan_watch: bool,
    #[serde(default)]
    pub short_buy: bool,
    #[serde(default)]
    pub white_exit: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrendIndicatorSignal {
    pub code: String,
    pub date: String,
    pub close: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_close: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_change: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_change_pct: Option<f64>,
    #[serde(default)]
    pub swl: Option<f64>,
    #[serde(default)]
    pub sws: Option<f64>,
    #[serde(default)]
    pub k: Option<f64>,
    #[serde(default)]
    pub d: Option<f64>,
    #[serde(default)]
    pub j: Option<f64>,
    #[serde(default)]
    pub star_line: Option<f64>,
    #[serde(default)]
    pub bull_line: Option<f64>,
    #[serde(default)]
    pub wait_line: Option<f64>,
    #[serde(default)]
    pub support: Option<f64>,
    #[serde(default)]
    pub resistance: Option<f64>,
    #[serde(default)]
    pub breakout: Option<f64>,
    #[serde(default)]
    pub reversal: Option<f64>,
    #[serde(default)]
    pub swl_above_sws: bool,
    #[serde(default)]
    pub kdj_golden_cross: bool,
    #[serde(default)]
    pub kdj_dead_cross: bool,
    #[serde(default)]
    pub kdj_overbought: bool,
    #[serde(default)]
    pub kdj_oversold: bool,
    #[serde(default)]
    pub red_hold: bool,
    #[serde(default)]
    pub cyan_watch: bool,
    #[serde(default)]
    pub short_buy: bool,
    #[serde(default)]
    pub white_exit: bool,
    #[serde(default)]
    pub oversold: bool,
    #[serde(default = "default_trend_signal_type")]
    pub signal_type: String,
    #[serde(default)]
    pub risk_flags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub technical_score: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern_layer_score: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_score: Option<f64>,
    #[serde(default)]
    pub quant_score: i32,
    #[serde(default = "default_quant_score_max")]
    pub quant_score_max: i32,
    #[serde(default)]
    pub pattern_score: i32,
    #[serde(default = "default_pattern_score_max")]
    pub pattern_score_max: i32,
    #[serde(default)]
    pub pattern_signals: Vec<String>,
    pub status: String,
    #[serde(default)]
    pub reasons: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrendIndicatorResult {
    pub stock: StockItem,
    pub signal: TrendIndicatorSignal,
    #[serde(default)]
    pub series: Vec<TrendIndicatorPoint>,
}

pub(super) fn default_trend_signal_type() -> String {
    "trend_continuation".to_string()
}

pub(crate) fn default_series_limit() -> usize {
    120
}

pub(crate) fn clamp_series_limit(limit: usize) -> usize {
    limit.clamp(20, 10_000)
}

pub(super) fn default_quant_score_max() -> i32 {
    90
}

pub(super) fn default_pattern_score_max() -> i32 {
    100
}
