use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    ffi::{CStr, CString},
    fmt,
    os::raw::c_char,
    panic,
};

use chrono::{Datelike, Days, Local, NaiveDate, Weekday};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct CoreError {
    message: String,
}

impl CoreError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CoreError {}

impl From<serde_json::Error> for CoreError {
    fn from(error: serde_json::Error) -> Self {
        Self::new(error.to_string())
    }
}

pub type CoreResult<T> = Result<T, CoreError>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StockItem {
    pub code: String,
    pub name: String,
    pub industry: String,
    #[serde(default)]
    pub is_st: bool,
    pub price: f64,
    #[serde(default)]
    pub pe: Option<f64>,
    #[serde(default)]
    pub pb: Option<f64>,
    #[serde(default)]
    pub roe: Option<f64>,
    #[serde(default)]
    pub market_cap_billion: Option<f64>,
    #[serde(default)]
    pub dividend_yield: Option<f64>,
    #[serde(default)]
    pub deducted_net_profit_billion: Option<f64>,
    #[serde(default)]
    pub deducted_net_profit_margin: Option<f64>,
    #[serde(default)]
    pub deducted_net_profit_growth_rate: Option<f64>,
    #[serde(default)]
    pub change_pct: Option<f64>,
    #[serde(default)]
    pub volume: Option<f64>,
    #[serde(default)]
    pub amount: Option<f64>,
    #[serde(default)]
    pub turnover_rate: Option<f64>,
    #[serde(default)]
    pub volume_ratio: Option<f64>,
    #[serde(default)]
    pub quote_time: Option<String>,
}

impl Default for StockItem {
    fn default() -> Self {
        Self {
            code: String::new(),
            name: String::new(),
            industry: String::new(),
            is_st: false,
            price: 0.0,
            pe: None,
            pb: None,
            roe: None,
            market_cap_billion: None,
            dividend_yield: None,
            deducted_net_profit_billion: None,
            deducted_net_profit_margin: None,
            deducted_net_profit_growth_rate: None,
            change_pct: None,
            volume: None,
            amount: None,
            turnover_rate: None,
            volume_ratio: None,
            quote_time: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScreenCriteria {
    #[serde(default)]
    pub min_roe: Option<f64>,
    #[serde(default)]
    pub max_pe: Option<f64>,
    #[serde(default)]
    pub max_pb: Option<f64>,
    #[serde(default)]
    pub min_market_cap_billion: Option<f64>,
    #[serde(default)]
    pub min_deducted_net_profit_billion: Option<f64>,
    #[serde(default)]
    pub min_deducted_net_profit_margin: Option<f64>,
    #[serde(default)]
    pub min_deducted_net_profit_growth_rate: Option<f64>,
    #[serde(default)]
    pub industry: Option<String>,
    #[serde(default)]
    pub include_st: bool,
    #[serde(default = "default_screen_limit")]
    pub limit: usize,
    #[serde(default = "default_sort_by")]
    pub sort_by: String,
    #[serde(default = "default_sort_dir")]
    pub sort_dir: String,
    #[serde(default = "default_score_profile")]
    pub score_profile: String,
}

impl Default for ScreenCriteria {
    fn default() -> Self {
        Self {
            min_roe: None,
            max_pe: None,
            max_pb: None,
            min_market_cap_billion: None,
            min_deducted_net_profit_billion: None,
            min_deducted_net_profit_margin: None,
            min_deducted_net_profit_growth_rate: None,
            industry: None,
            include_st: false,
            limit: default_screen_limit(),
            sort_by: default_sort_by(),
            sort_dir: default_sort_dir(),
            score_profile: default_score_profile(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScreenedStock {
    pub stock: StockItem,
    pub score: f64,
    pub reasons: Vec<String>,
    #[serde(default)]
    pub factor_scores: BTreeMap<String, f64>,
    #[serde(default)]
    pub score_explanation: String,
    #[serde(default)]
    pub concept: Option<String>,
    #[serde(default)]
    pub theme_category: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScreenResultGroup {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub total: usize,
    #[serde(default)]
    pub returned: usize,
    #[serde(default)]
    pub items: Vec<ScreenedStock>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScreenResult {
    pub total: usize,
    pub returned: usize,
    pub items: Vec<ScreenedStock>,
    #[serde(default)]
    pub groups: Vec<ScreenResultGroup>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SectorScreenRequest {
    #[serde(default)]
    pub criteria: ScreenCriteria,
    #[serde(default = "default_sector_group_limit")]
    pub max_sectors: usize,
    #[serde(default = "default_per_sector_limit")]
    pub per_sector_limit: usize,
    #[serde(default = "default_min_sector_candidates")]
    pub min_sector_candidates: usize,
    #[serde(default = "default_sector_group_by")]
    pub group_by: String,
}

impl Default for SectorScreenRequest {
    fn default() -> Self {
        Self {
            criteria: ScreenCriteria::default(),
            max_sectors: default_sector_group_limit(),
            per_sector_limit: default_per_sector_limit(),
            min_sector_candidates: default_min_sector_candidates(),
            group_by: default_sector_group_by(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SectorScreenGroup {
    pub sector: String,
    pub total: usize,
    pub returned: usize,
    pub average_score: f64,
    pub items: Vec<ScreenedStock>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SectorScreenResult {
    pub total: usize,
    pub returned: usize,
    pub sector_count: usize,
    pub groups: Vec<SectorScreenGroup>,
    #[serde(default)]
    pub notes: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StockRelation {
    pub source_code: String,
    pub target_code: String,
    pub relation_type: String,
    #[serde(default = "default_relation_weight")]
    pub weight: f64,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScoreContribution {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub value: Option<f64>,
    #[serde(default)]
    pub contribution: Option<f64>,
    #[serde(default = "default_tone")]
    pub tone: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SelectionExplanation {
    #[serde(default)]
    pub basis: Vec<String>,
    #[serde(default)]
    pub score_breakdown: Vec<ScoreContribution>,
    #[serde(default)]
    pub risk_checks: Vec<String>,
    #[serde(default)]
    pub verification: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GraphCenterContext {
    #[serde(default = "default_graph_center_mode")]
    pub mode: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub codes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphScreenRequest {
    #[serde(default)]
    pub criteria: ScreenCriteria,
    #[serde(default)]
    pub seed_codes: Vec<String>,
    #[serde(default = "default_relation_depth")]
    pub relation_depth: usize,
    #[serde(default = "default_relation_weight_param")]
    pub relation_weight: f64,
    #[serde(default = "default_graph_limit")]
    pub limit: usize,
}

impl Default for GraphScreenRequest {
    fn default() -> Self {
        Self {
            criteria: ScreenCriteria::default(),
            seed_codes: Vec::new(),
            relation_depth: default_relation_depth(),
            relation_weight: default_relation_weight_param(),
            limit: default_graph_limit(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphStockSignal {
    pub stock: StockItem,
    pub base_score: f64,
    pub relation_score: f64,
    pub final_score: f64,
    pub suggested_weight: f64,
    pub reasons: Vec<String>,
    #[serde(default)]
    pub related: Vec<StockRelation>,
    #[serde(default)]
    pub explanation: SelectionExplanation,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphScreenResult {
    pub total: usize,
    pub returned: usize,
    pub relation_count: usize,
    pub items: Vec<GraphStockSignal>,
    #[serde(default)]
    pub center_context: GraphCenterContext,
    #[serde(default)]
    pub notes: Vec<String>,
}

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
pub struct CapitalEvidenceItem {
    pub category: String,
    pub source: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_metrics")]
    pub metrics: BTreeMap<String, String>,
    #[serde(default = "default_uncertain_sentiment")]
    pub sentiment: String,
    #[serde(default)]
    pub weight: f64,
    #[serde(default = "default_low_confidence")]
    pub confidence: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

fn deserialize_string_metrics<'de, D>(deserializer: D) -> Result<BTreeMap<String, String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<BTreeMap<String, Value>>::deserialize(deserializer)?;
    let Some(map) = value else {
        return Ok(BTreeMap::new());
    };
    let mut metrics = BTreeMap::new();
    for (key, value) in map {
        let text = match value {
            Value::Null => String::new(),
            Value::String(value) => value,
            Value::Bool(value) => value.to_string(),
            Value::Number(value) => value.to_string(),
            other => other.to_string(),
        };
        metrics.insert(key, text);
    }
    Ok(metrics)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapitalEvidenceSection {
    pub key: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(default)]
    pub weight: f64,
    #[serde(default)]
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default)]
    pub items: Vec<CapitalEvidenceItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapitalEvidenceResult {
    pub stock_code: String,
    pub generated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composite_score: Option<f64>,
    #[serde(default = "default_low_confidence")]
    pub confidence: String,
    #[serde(default)]
    pub model_used: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_of_trade_date: Option<String>,
    #[serde(default = "default_unknown_freshness")]
    pub freshness: String,
    #[serde(default)]
    pub contributions: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default)]
    pub sections: Vec<CapitalEvidenceSection>,
    #[serde(default)]
    pub items: Vec<CapitalEvidenceItem>,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BacktestRequest {
    #[serde(default)]
    pub criteria: ScreenCriteria,
    #[serde(default = "default_backtest_source")]
    pub source: String,
    #[serde(default)]
    pub stock_codes: Vec<String>,
    pub start_date: String,
    pub end_date: String,
    #[serde(default = "default_top_n")]
    pub top_n: usize,
    #[serde(default = "default_initial_cash")]
    pub initial_cash: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EquityPoint {
    pub date: String,
    pub equity: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BacktestMetrics {
    pub total_return: f64,
    pub annualized_return: Option<f64>,
    pub max_drawdown: Option<f64>,
    pub num_stocks: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BacktestResult {
    pub metrics: BacktestMetrics,
    pub equity_curve: Vec<EquityPoint>,
    pub symbols: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentRequest {
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentResponse {
    pub reply: String,
    pub action: String,
    #[serde(default)]
    pub criteria: Option<ScreenCriteria>,
    #[serde(default)]
    pub backtest: Option<BacktestRequest>,
    #[serde(default)]
    pub graph_screen: Option<GraphScreenRequest>,
    #[serde(default)]
    pub trend_screen: Option<TrendScreenRequest>,
    #[serde(default)]
    pub data: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryBar {
    pub date: String,
    #[serde(default)]
    pub open: Option<f64>,
    #[serde(default)]
    pub high: Option<f64>,
    #[serde(default)]
    pub low: Option<f64>,
    pub close: f64,
    #[serde(default)]
    pub volume: Option<f64>,
    #[serde(default)]
    pub capital: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct QuarterlyEpsPoint {
    pub period: String,
    pub value: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StockFinancialSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_eps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_bps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default)]
    pub quarterly_eps: Vec<QuarterlyEpsPoint>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CoreDataSet {
    #[serde(default)]
    pub stocks: Vec<StockItem>,
    #[serde(default)]
    pub relations: Vec<StockRelation>,
    #[serde(default)]
    pub histories: HashMap<String, Vec<HistoryBar>>,
    #[serde(default)]
    pub financials: HashMap<String, StockFinancialSnapshot>,
    #[serde(default)]
    pub capital_evidence: HashMap<String, CapitalEvidenceResult>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataSourceSummary {
    pub stock_count: usize,
    pub relation_count: usize,
    pub history_symbol_count: usize,
    pub history_bar_count: usize,
    #[serde(default)]
    pub warnings: Vec<String>,
}

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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentStreamWithDataRequest {
    pub data: CoreDataSet,
    pub message: String,
    #[serde(default)]
    pub run_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentStreamEvent {
    pub run_id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<AgentResponse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MobileStockSourceItem {
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub source_tier: String,
    #[serde(default)]
    pub source_name: String,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub evidence: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MobileStockSkillRequest {
    pub stock_code: String,
    #[serde(default)]
    pub stock_name: String,
    #[serde(default)]
    pub question: String,
    #[serde(default)]
    pub sources: Vec<MobileStockSourceItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MobileStockSkillFinding {
    pub label: String,
    pub title: String,
    pub summary: String,
    pub source_tier: String,
    pub source_name: String,
    #[serde(default)]
    pub published_at: Option<String>,
    pub evidence: String,
    pub confidence: f64,
    pub risk_note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MobileStockSkillOverview {
    pub stock_code: String,
    pub stock_name: String,
    pub overall_label: String,
    pub summary: String,
    pub positive_count: usize,
    pub negative_count: usize,
    pub neutral_count: usize,
    pub unverified_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MobileStockSkillResult {
    pub overview: MobileStockSkillOverview,
    #[serde(default)]
    pub positive_factors: Vec<MobileStockSkillFinding>,
    #[serde(default)]
    pub negative_factors: Vec<MobileStockSkillFinding>,
    #[serde(default)]
    pub neutral_information: Vec<MobileStockSkillFinding>,
    #[serde(default)]
    pub unverified_leads: Vec<MobileStockSkillFinding>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Clone, Debug)]
struct HistoryPoint {
    date: NaiveDate,
    close: f64,
}

#[derive(Clone, Debug)]
struct PreparedBar {
    date: NaiveDate,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
    capital: f64,
}

#[derive(Clone, Debug)]
struct ComputedTrendBar {
    date: NaiveDate,
    close: f64,
    previous_close: Option<f64>,
    close_change: Option<f64>,
    close_change_pct: Option<f64>,
    swl: f64,
    sws: f64,
    k: f64,
    d: f64,
    j: f64,
    accumulation_index: f64,
    accumulation_strength: f64,
    swing_opportunity: f64,
    rebound_signal: f64,
    trend_heat: f64,
    volume_price_heat: f64,
    anomaly_heat: f64,
    popularity_heat: f64,
    star_line: f64,
    bull_line: f64,
    wait_line: f64,
    support: f64,
    resistance: f64,
    breakout: f64,
    reversal: f64,
    swl_above_sws: bool,
    kdj_golden_cross: bool,
    kdj_dead_cross: bool,
    kdj_overbought: bool,
    kdj_oversold: bool,
    red_hold: bool,
    cyan_watch: bool,
    short_buy: bool,
    white_exit: bool,
    oversold: bool,
    quant_score: i32,
}

pub trait MarketDataSource {
    fn list_stocks(&self) -> CoreResult<Vec<StockItem>>;

    fn list_relations(&self) -> CoreResult<Vec<StockRelation>> {
        Ok(Vec::new())
    }

    fn get_history(
        &self,
        code: &str,
        start_date: &str,
        end_date: &str,
    ) -> CoreResult<Vec<HistoryBar>>;

    fn get_financial_snapshot(&self, _code: &str) -> Option<StockFinancialSnapshot> {
        None
    }

    fn get_capital_evidence(&self, _code: &str) -> Option<CapitalEvidenceResult> {
        None
    }
}

pub struct MockDataSource;

impl MarketDataSource for MockDataSource {
    fn list_stocks(&self) -> CoreResult<Vec<StockItem>> {
        Ok(mock_stocks())
    }

    fn list_relations(&self) -> CoreResult<Vec<StockRelation>> {
        Ok(mock_relations())
    }

    fn get_history(
        &self,
        code: &str,
        start_date: &str,
        end_date: &str,
    ) -> CoreResult<Vec<HistoryBar>> {
        mock_history(&mock_stocks(), code, start_date, end_date)
    }
}

pub struct StaticDataSource {
    data: CoreDataSet,
}

impl StaticDataSource {
    pub fn new(data: CoreDataSet) -> Self {
        Self { data }
    }
}

impl MarketDataSource for StaticDataSource {
    fn list_stocks(&self) -> CoreResult<Vec<StockItem>> {
        Ok(self.data.stocks.clone())
    }

    fn list_relations(&self) -> CoreResult<Vec<StockRelation>> {
        Ok(self.data.relations.clone())
    }

    fn get_history(
        &self,
        code: &str,
        start_date: &str,
        end_date: &str,
    ) -> CoreResult<Vec<HistoryBar>> {
        let Some(history) = self.data.histories.get(code) else {
            return Ok(Vec::new());
        };
        history_bars_in_range(history, start_date, end_date)
    }

    fn get_financial_snapshot(&self, code: &str) -> Option<StockFinancialSnapshot> {
        let normalized = normalize_stock_code(code);
        self.data
            .financials
            .get(code)
            .cloned()
            .or_else(|| self.data.financials.get(&normalized).cloned())
    }

    fn get_capital_evidence(&self, code: &str) -> Option<CapitalEvidenceResult> {
        let normalized = normalize_stock_code(code);
        self.data
            .capital_evidence
            .get(code)
            .cloned()
            .or_else(|| self.data.capital_evidence.get(&normalized).cloned())
    }
}

fn default_screen_limit() -> usize {
    10
}

fn default_sort_by() -> String {
    "score".to_string()
}

fn default_sort_dir() -> String {
    "desc".to_string()
}

fn default_score_profile() -> String {
    "quality".to_string()
}

fn default_relation_weight() -> f64 {
    1.0
}

fn default_relation_depth() -> usize {
    1
}

fn default_relation_weight_param() -> f64 {
    0.35
}

fn default_tone() -> String {
    "neutral".to_string()
}

fn default_uncertain_sentiment() -> String {
    "uncertain".to_string()
}

fn default_low_confidence() -> String {
    "\u{4f4e}".to_string()
}

fn default_unknown_freshness() -> String {
    "unknown".to_string()
}

fn default_graph_center_mode() -> String {
    "seed_codes".to_string()
}

fn default_trend_screen_style() -> String {
    "short_buy".to_string()
}

fn default_graph_limit() -> usize {
    10
}

fn default_sector_group_limit() -> usize {
    12
}

fn default_per_sector_limit() -> usize {
    5
}

fn default_min_sector_candidates() -> usize {
    1
}

fn default_sector_group_by() -> String {
    "concept".to_string()
}

fn default_start_date() -> String {
    "20200101".to_string()
}

fn default_end_date() -> String {
    current_system_date_yyyymmdd()
}

fn current_system_date_yyyymmdd() -> String {
    Local::now().date_naive().format("%Y%m%d").to_string()
}

fn default_series_limit() -> usize {
    120
}

fn default_trend_limit() -> usize {
    10
}

fn default_quant_score_max() -> i32 {
    90
}

fn default_pattern_score_max() -> i32 {
    100
}

fn default_top_n() -> usize {
    10
}

fn default_initial_cash() -> f64 {
    1_000_000.0
}

fn default_backtest_source() -> String {
    "criteria".to_string()
}

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
    serde_json::to_value(run_agent_with_data(&request.data, &request.message)?).map_err(Into::into)
}

pub fn agent_stream_with_data_events_value(payload: Value) -> CoreResult<Vec<AgentStreamEvent>> {
    let request: AgentStreamWithDataRequest = serde_json::from_value(payload)?;
    Ok(run_agent_stream_with_data_events(
        &request.data,
        &request.message,
        request.run_id.as_deref(),
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

pub fn screen_with_mock(criteria: &ScreenCriteria) -> ScreenResult {
    screen_with_source(&MockDataSource, criteria).expect("mock data source should be valid")
}

pub fn sector_screen_with_mock(request: &SectorScreenRequest) -> SectorScreenResult {
    let universe = mock_stocks();
    sector_screen_stocks(&universe, request)
}
pub fn screen_with_data(data: &CoreDataSet, criteria: &ScreenCriteria) -> CoreResult<ScreenResult> {
    let source = StaticDataSource::new(data.clone());
    screen_with_source(&source, criteria)
}

pub fn screen_with_source(
    source: &impl MarketDataSource,
    criteria: &ScreenCriteria,
) -> CoreResult<ScreenResult> {
    let universe = source.list_stocks()?;
    Ok(screen_stocks(&universe, criteria))
}

pub fn sector_screen_with_data(
    data: &CoreDataSet,
    request: &SectorScreenRequest,
) -> CoreResult<SectorScreenResult> {
    let source = StaticDataSource::new(data.clone());
    let universe = source.list_stocks()?;
    Ok(sector_screen_stocks(&universe, request))
}

pub fn sector_screen_stocks(
    universe: &[StockItem],
    request: &SectorScreenRequest,
) -> SectorScreenResult {
    let max_sectors = request.max_sectors.clamp(1, 50);
    let per_sector_limit = request.per_sector_limit.clamp(1, 50);
    let min_sector_candidates = request.min_sector_candidates.clamp(1, 500);
    let group_by = request.group_by.trim().to_ascii_lowercase();
    let board_mode = matches!(group_by.as_str(), "board" | "market" | "market_board");
    let mut screened = Vec::new();
    let mut notes = deducted_profit_rule_notes(universe, &request.criteria);

    for stock in universe {
        let Some(reasons) = matches_stock(stock, &request.criteria) else {
            continue;
        };
        screened.push(score_stock(
            stock,
            &reasons,
            &request.criteria.score_profile,
        ));
    }
    sort_screened(&mut screened, &request.criteria);

    let mut by_sector: HashMap<String, Vec<ScreenedStock>> = HashMap::new();
    for item in &screened {
        let sector = if board_mode {
            board_group_for_stock(&item.stock).to_string()
        } else {
            item.concept
                .clone()
                .unwrap_or_else(|| concept_group_for_stock(&item.stock))
        };
        by_sector.entry(sector).or_default().push(item.clone());
    }

    let mut groups: Vec<SectorScreenGroup> = by_sector
        .into_iter()
        .filter(|(_, items)| items.len() >= min_sector_candidates)
        .map(|(sector, mut items)| {
            let total = items.len();
            items.truncate(per_sector_limit);
            let returned = items.len();
            let average_score = if returned == 0 {
                0.0
            } else {
                items.iter().map(|item| item.score).sum::<f64>() / returned as f64
            };
            SectorScreenGroup {
                sector,
                total,
                returned,
                average_score: (average_score * 1_000_000.0).round() / 1_000_000.0,
                items,
            }
        })
        .collect();
    groups.sort_by(|left, right| {
        let left_rank = if board_mode {
            board_rank(&left.sector)
        } else {
            concept_rank(&left.sector)
        };
        let right_rank = if board_mode {
            board_rank(&right.sector)
        } else {
            concept_rank(&right.sector)
        };
        left_rank
            .cmp(&right_rank)
            .then_with(|| right.average_score.total_cmp(&left.average_score))
            .then_with(|| right.total.cmp(&left.total))
            .then_with(|| left.sector.cmp(&right.sector))
    });
    groups.truncate(max_sectors);
    let returned = groups.iter().map(|group| group.returned).sum();
    if request
        .criteria
        .industry
        .as_deref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        notes.push(format!(
            "{}{}",
            "\u{5df2}\u{9650}\u{5b9a}\u{884c}\u{4e1a}\u{ff1a}",
            request.criteria.industry.as_deref().unwrap_or_default()
        ));
    }
    if board_mode {
        notes.push("\u{5206}\u{677f}\u{5757}\u{7b5b}\u{9009}\u{5df2}\u{6309} A \u{80a1}\u{4ea4}\u{6613}\u{677f}\u{5757}\u{5206}\u{7ec4}\u{ff1a}\u{79d1}\u{521b}\u{677f}\u{3001}\u{521b}\u{4e1a}\u{677f}\u{3001}\u{5317}\u{4ea4}\u{6240}\u{3001}\u{6caa}\u{4e3b}\u{677f}\u{548c}\u{6df1}\u{4e3b}\u{677f}\u{3002}".to_string());
    } else {
        notes.push("\u{5206}\u{6982}\u{5ff5}\u{7b5b}\u{9009}\u{5df2}\u{5728} Rust gp-core \u{4e2d}\u{57fa}\u{4e8e}\u{5b8c}\u{6574}\u{5019}\u{9009}\u{6c60}\u{5206}\u{7ec4}\u{ff0c}\u{4e0d}\u{518d}\u{4f9d}\u{8d56}\u{524d}\u{7aef}\u{79fb}\u{52a8}\u{7aef}\u{4e8c}\u{6b21}\u{5206}\u{7ec4}\u{3002}".to_string());
    }
    SectorScreenResult {
        total: screened.len(),
        returned,
        sector_count: groups.len(),
        groups,
        notes,
    }
}
pub fn graph_screen_with_mock(request: &GraphScreenRequest) -> GraphScreenResult {
    graph_screen_with_source(&MockDataSource, request).expect("mock data source should be valid")
}

pub fn graph_screen_with_data(
    data: &CoreDataSet,
    request: &GraphScreenRequest,
) -> CoreResult<GraphScreenResult> {
    let source = StaticDataSource::new(data.clone());
    graph_screen_with_source(&source, request)
}

pub fn graph_screen_with_source(
    source: &impl MarketDataSource,
    request: &GraphScreenRequest,
) -> CoreResult<GraphScreenResult> {
    let universe = source.list_stocks()?;
    let relations = source.list_relations()?;
    Ok(graph_screen_stocks(&universe, &relations, request))
}

pub fn backtest_with_mock(request: &BacktestRequest) -> CoreResult<BacktestResult> {
    backtest_with_source(&MockDataSource, request)
}

pub fn backtest_with_data(
    data: &CoreDataSet,
    request: &BacktestRequest,
) -> CoreResult<BacktestResult> {
    let source = StaticDataSource::new(data.clone());
    backtest_with_source(&source, request)
}

pub fn backtest_with_source(
    source: &impl MarketDataSource,
    request: &BacktestRequest,
) -> CoreResult<BacktestResult> {
    let universe = source.list_stocks()?;
    let (selected, selection_notes) = selected_backtest_items(&universe, request);
    let symbols: Vec<String> = selected
        .iter()
        .map(|item| item.stock.code.clone())
        .collect();

    let mut portfolio_points: BTreeMap<NaiveDate, Vec<f64>> = BTreeMap::new();
    for item in &selected {
        let bars = source.get_history(&item.stock.code, &request.start_date, &request.end_date)?;
        let history = history_points_from_bars(&bars)?;
        if let Some(base) = history
            .first()
            .map(|point| point.close)
            .filter(|value| *value != 0.0)
        {
            for point in history {
                portfolio_points
                    .entry(point.date)
                    .or_default()
                    .push(point.close / base);
            }
        }
    }

    if portfolio_points.is_empty() {
        return Ok(BacktestResult {
            metrics: BacktestMetrics {
                total_return: 0.0,
                annualized_return: None,
                max_drawdown: None,
                num_stocks: symbols.len(),
            },
            equity_curve: Vec::new(),
            symbols,
            notes: selection_notes,
        });
    }

    let equity_curve: Vec<EquityPoint> = portfolio_points
        .iter()
        .map(|(date, values)| {
            let average = values.iter().sum::<f64>() / values.len() as f64;
            EquityPoint {
                date: date.format("%Y-%m-%d").to_string(),
                equity: average * request.initial_cash,
            }
        })
        .collect();

    let (total_return, annualized_return, max_drawdown) = backtest_metrics(&equity_curve);
    Ok(BacktestResult {
        metrics: BacktestMetrics {
            total_return,
            annualized_return,
            max_drawdown,
            num_stocks: symbols.len(),
        },
        equity_curve,
        symbols,
        notes: selection_notes,
    })
}

fn selected_backtest_items(
    universe: &[StockItem],
    request: &BacktestRequest,
) -> (Vec<ScreenedStock>, Vec<String>) {
    if normalize_backtest_source(&request.source) != "watchlist" {
        let screened = screen_stocks(universe, &request.criteria);
        return (
            screened
                .items
                .into_iter()
                .take(request.top_n.clamp(1, 100))
                .collect(),
            Vec::new(),
        );
    }

    let codes = dedupe_stock_codes(&request.stock_codes);
    if codes.is_empty() {
        return (
            Vec::new(),
            vec!["自选观察池为空，未执行固定标的回测。".to_string()],
        );
    }

    let by_code: HashMap<String, StockItem> = universe
        .iter()
        .map(|stock| (stock.code.to_uppercase(), stock.clone()))
        .collect();
    let mut selected = Vec::new();
    let mut missing = Vec::new();
    for code in codes.iter().take(request.top_n.clamp(1, 100)) {
        if let Some(stock) = by_code.get(code) {
            selected.push(score_stock(stock, &["watchlist".to_string()], "quality"));
        } else {
            missing.push(code.clone());
        }
    }

    let mut notes = vec![format!(
        "回测标的来源：自选观察池，已选择 {} / {} 只。",
        selected.len(),
        codes.len()
    )];
    if !missing.is_empty() {
        let sample = missing
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let suffix = if missing.len() > 5 { " 等" } else { "" };
        notes.push(format!(
            "自选观察池中 {} 只股票不在当前股票池：{}{}。",
            missing.len(),
            sample,
            suffix
        ));
    }
    (selected, notes)
}

fn normalize_backtest_source(source: &str) -> String {
    if source.trim().eq_ignore_ascii_case("watchlist") {
        "watchlist".to_string()
    } else {
        "criteria".to_string()
    }
}

fn dedupe_stock_codes(codes: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for code in codes {
        let normalized = normalize_stock_code(code);
        if normalized.is_empty() || !seen.insert(normalized.clone()) {
            continue;
        }
        result.push(normalized);
    }
    result
}

fn normalize_stock_code(code: &str) -> String {
    let text = code.trim().to_uppercase();
    if text.is_empty() {
        return String::new();
    }
    if let Some((digits, suffix)) = text.split_once('.') {
        if digits.chars().all(|ch| ch.is_ascii_digit()) && matches!(suffix, "SH" | "SZ" | "BJ") {
            return format!("{:0>6}.{}", digits, suffix);
        }
        return text;
    }
    let digits: String = text.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if digits.len() != 6 {
        return text;
    }
    let suffix = if digits.starts_with('5') || digits.starts_with('6') || digits.starts_with('9') {
        "SH"
    } else if digits.starts_with('4') || digits.starts_with('8') {
        "BJ"
    } else {
        "SZ"
    };
    format!("{}.{}", digits, suffix)
}

pub fn trend_with_mock(request: &TrendIndicatorRequest) -> CoreResult<TrendIndicatorResult> {
    trend_with_source(&MockDataSource, request)
}

pub fn trend_with_data(
    data: &CoreDataSet,
    request: &TrendIndicatorRequest,
) -> CoreResult<TrendIndicatorResult> {
    let source = StaticDataSource::new(data.clone());
    trend_with_source(&source, request)
}

pub fn trend_with_source(
    source: &impl MarketDataSource,
    request: &TrendIndicatorRequest,
) -> CoreResult<TrendIndicatorResult> {
    let universe = source.list_stocks()?;
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
    let limit = request.series_limit.clamp(20, 500);
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

pub fn observe_with_mock(request: &StockObserveRequest) -> CoreResult<StockObservation> {
    observe_with_source(&MockDataSource, request)
}

pub fn observe_with_data(
    data: &CoreDataSet,
    request: &StockObserveRequest,
) -> CoreResult<StockObservation> {
    let source = StaticDataSource::new(data.clone());
    observe_with_source(&source, request)
}

pub fn observe_with_source(
    source: &impl MarketDataSource,
    request: &StockObserveRequest,
) -> CoreResult<StockObservation> {
    let code = normalize_stock_code(&request.code);
    let universe = source.list_stocks()?;
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
        series_limit: request.series_limit.clamp(20, 500),
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
        notes.push("\u{76d8}\u{53e3}\u{6570}\u{636e}\u{5c1a}\u{672a}\u{8fc1}\u{79fb}\u{5230} Rust \u{884c}\u{60c5}\u{6e90}\u{ff0c}\u{5df2}\u{8fd4}\u{56de}\u{7a7a}\u{76d8}\u{53e3}\u{3002}".to_string());
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

fn build_observation_financial_indicators(
    stock: &StockItem,
    financial: Option<&StockFinancialSnapshot>,
) -> FinancialIndicators {
    let mut items = Vec::new();
    let mut notes = Vec::new();
    let mut source_parts = vec!["Tauri/Rust".to_string()];
    let mut period = "\u{672c}\u{5730}\u{884c}\u{60c5}\u{5feb}\u{7167}".to_string();

    if let Some(snapshot) = financial {
        if let Some(source) = snapshot
            .source
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            source_parts.push(source.clone());
        }
        if let Some(snapshot_period) = snapshot
            .period
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            period = snapshot_period.clone();
        }
    }

    push_indicator(
        &mut items,
        "\u{5e02}\u{76c8}\u{7387}(TTM)",
        stock.pe,
        |value| format_number(value),
        "neutral",
    );
    push_indicator(
        &mut items,
        "\u{5e02}\u{51c0}\u{7387}",
        stock.pb,
        |value| format_number(value),
        "neutral",
    );
    push_indicator(
        &mut items,
        "ROE",
        stock.roe,
        |value| format_percent(value),
        indicator_tone(stock.roe),
    );
    push_indicator(
        &mut items,
        "\u{5e02}\u{503c}",
        stock.market_cap_billion,
        |value| format!("{}\u{4ebf}", format_number(value)),
        "neutral",
    );
    push_indicator(
        &mut items,
        "\u{80a1}\u{606f}\u{7387}",
        stock.dividend_yield,
        |value| format_percent(value),
        "neutral",
    );
    push_indicator(
        &mut items,
        "\u{6263}\u{975e}\u{51c0}\u{5229}\u{6da6}",
        stock.deducted_net_profit_billion,
        |value| format!("{}\u{4ebf}", format_number(value)),
        indicator_tone(stock.deducted_net_profit_billion),
    );
    push_indicator(
        &mut items,
        "\u{6263}\u{975e}\u{51c0}\u{5229}\u{7387}",
        stock.deducted_net_profit_margin,
        |value| format_percent(value),
        "neutral",
    );
    push_indicator(
        &mut items,
        "\u{6263}\u{975e}\u{51c0}\u{5229}\u{6da6}\u{589e}\u{957f}\u{7387}",
        stock.deducted_net_profit_growth_rate,
        |value| format_percent(value),
        indicator_tone(stock.deducted_net_profit_growth_rate),
    );

    let snapshot_period = financial.and_then(|snapshot| snapshot.period.as_deref());
    let latest_eps = financial.and_then(|snapshot| snapshot.latest_eps);
    if latest_eps.is_some() {
        push_indicator_with_meta(
            &mut items,
            "\u{6700}\u{65b0}\u{6bcf}\u{80a1}\u{6536}\u{76ca}",
            latest_eps,
            |value| format!("{}\u{5143}", format_number(value)),
            indicator_tone(latest_eps),
            Some("\u{5143}"),
            Some("latest_eps"),
            snapshot_period,
        );
    } else if let Some(pe) = stock.pe.filter(|value| value.abs() > f64::EPSILON) {
        let eps = stock.price / pe;
        push_indicator_with_meta(
            &mut items,
            "\u{6bcf}\u{80a1}\u{6536}\u{76ca}(\u{4f30}\u{7b97})",
            Some(eps),
            |value| format!("{}\u{5143}", format_number(value)),
            indicator_tone(Some(eps)),
            Some("\u{5143}"),
            Some("estimated_eps"),
            None,
        );
    }

    let latest_bps = financial.and_then(|snapshot| snapshot.latest_bps);
    if latest_bps.is_some() {
        push_indicator_with_meta(
            &mut items,
            "\u{6bcf}\u{80a1}\u{51c0}\u{8d44}\u{4ea7}",
            latest_bps,
            |value| format!("{}\u{5143}", format_number(value)),
            "neutral",
            Some("\u{5143}"),
            Some("latest_bps"),
            snapshot_period,
        );
    } else if let Some(pb) = stock.pb.filter(|value| value.abs() > f64::EPSILON) {
        let bps = stock.price / pb;
        push_indicator_with_meta(
            &mut items,
            "\u{6bcf}\u{80a1}\u{51c0}\u{8d44}\u{4ea7}(\u{4f30}\u{7b97})",
            Some(bps),
            |value| format!("{}\u{5143}", format_number(value)),
            "neutral",
            Some("\u{5143}"),
            Some("estimated_bps"),
            None,
        );
    }

    if let Some(snapshot) = financial {
        notes.extend(snapshot.notes.iter().filter_map(|note| {
            let trimmed = note.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }));
        let mut points = snapshot
            .quarterly_eps
            .iter()
            .filter(|point| {
                point.value.is_finite() && normalize_period_key(&point.period).is_some()
            })
            .cloned()
            .collect::<Vec<_>>();
        points.sort_by(|left, right| right.period.cmp(&left.period));
        points.dedup_by(|left, right| left.period == right.period);
        let values_by_period: HashMap<String, f64> = points
            .iter()
            .map(|point| (point.period.clone(), point.value))
            .collect();
        for point in points.iter().take(12) {
            let tone = previous_year_period(&point.period)
                .and_then(|previous| values_by_period.get(&previous).copied())
                .map(|previous| indicator_tone(Some(point.value - previous)))
                .unwrap_or("neutral");
            push_indicator_with_meta(
                &mut items,
                &format!("{} \u{6bcf}\u{80a1}\u{6536}\u{76ca}", point.period),
                Some(point.value),
                |value| format!("{}\u{5143}", format_number(value)),
                tone,
                Some("\u{5143}"),
                Some("quarterly_eps"),
                Some(point.period.as_str()),
            );
        }
        if points.is_empty() {
            notes.push("\u{5f53}\u{524d}\u{8d22}\u{62a5}\u{5feb}\u{7167}\u{6ca1}\u{6709}\u{5b63}\u{5ea6} EPS \u{660e}\u{7ec6}\u{ff0c}\u{5148}\u{5c55}\u{793a}\u{6700}\u{65b0} EPS \u{6216}\u{4f30}\u{7b97}\u{503c}\u{3002}".to_string());
        }
    } else {
        notes.push("\u{5f53}\u{524d}\u{6570}\u{636e}\u{96c6}\u{672a}\u{63a5}\u{5165}\u{8d22}\u{62a5} EPS \u{5feb}\u{7167}\u{ff0c}\u{6bcf}\u{80a1}\u{6536}\u{76ca}\u{6309}\u{5e02}\u{76c8}\u{7387}\u{4f30}\u{7b97}\u{3002}".to_string());
    }

    FinancialIndicators {
        title: "\u{6700}\u{65b0}\u{6307}\u{6807}".to_string(),
        period,
        source: source_parts.join(" / "),
        items,
        notes,
    }
}

fn previous_year_period(period: &str) -> Option<String> {
    let normalized = normalize_period_key(period)?;
    let year = normalized.get(0..4)?.parse::<i32>().ok()?;
    let quarter = normalized.get(5..6)?;
    Some(format!("{:04}Q{}", year - 1, quarter))
}

fn normalize_period_key(period: &str) -> Option<String> {
    let raw = period.trim().to_ascii_uppercase();
    let bytes = raw.as_bytes();
    if bytes.len() == 6
        && bytes[0..4].iter().all(|byte| byte.is_ascii_digit())
        && bytes[4] == b'Q'
        && matches!(bytes[5], b'1'..=b'4')
    {
        return Some(raw);
    }
    None
}
fn push_indicator(
    items: &mut Vec<FinancialIndicatorItem>,
    label: &str,
    raw_value: Option<f64>,
    formatter: impl Fn(f64) -> String,
    tone: &str,
) {
    push_indicator_with_meta(items, label, raw_value, formatter, tone, None, None, None);
}

fn push_indicator_with_meta(
    items: &mut Vec<FinancialIndicatorItem>,
    label: &str,
    raw_value: Option<f64>,
    formatter: impl Fn(f64) -> String,
    tone: &str,
    unit: Option<&str>,
    metric_key: Option<&str>,
    period: Option<&str>,
) {
    let Some(value) = raw_value.filter(|value| value.is_finite()) else {
        return;
    };
    items.push(FinancialIndicatorItem {
        label: label.to_string(),
        value: formatter(value),
        raw_value: Some(value),
        unit: unit.map(str::to_string),
        tone: tone.to_string(),
        metric_key: metric_key.map(str::to_string),
        period: period.map(str::to_string),
    });
}

fn format_number(value: f64) -> String {
    if value.abs() >= 100.0 {
        format!("{value:.0}")
    } else if value.abs() >= 10.0 {
        format!("{value:.2}")
    } else {
        format!("{value:.3}")
    }
}

fn format_percent(value: f64) -> String {
    let percent = as_percent(value).unwrap_or(value);
    format!("{}%", format_number(percent))
}

fn indicator_tone(value: Option<f64>) -> &'static str {
    match value {
        Some(value) if value.is_finite() && value >= 0.0 => "rise",
        Some(value) if value.is_finite() => "fall",
        _ => "neutral",
    }
}

fn build_observation_capital_evidence(
    stock: &StockItem,
    _financial_indicators: &FinancialIndicators,
    trend: Option<&TrendIndicatorResult>,
    end_date: &str,
    provided: Option<CapitalEvidenceResult>,
) -> CapitalEvidenceResult {
    const FUND_FLOW_WEIGHT: f64 = 0.35;
    const INSTITUTION_WEIGHT: f64 = 0.25;
    const NEWS_WEIGHT: f64 = 0.15;
    const TECHNICAL_WEIGHT: f64 = 0.25;

    let as_of_trade_date = provided
        .as_ref()
        .and_then(|evidence| evidence.as_of_trade_date.as_deref())
        .filter(|value| !value.trim().is_empty())
        .and_then(|value| capital_trade_date(value).or_else(|| Some(value.trim().to_string())))
        .or_else(|| capital_trade_date(end_date))
        .or_else(|| trend.map(|trend| trend.signal.date.clone()))
        .unwrap_or_else(|| Local::now().date_naive().format("%Y-%m-%d").to_string());

    let mut items = provided
        .as_ref()
        .map(|evidence| evidence.items.clone())
        .unwrap_or_default();
    items
        .retain(|item| item.category != "external_status" && item.category != "technical_behavior");

    let technical_score = trend.map(technical_capital_score);
    if let Some(trend) = trend {
        if !capital_bucket_has_evidence(&items, ["fund_flow", "", ""]) {
            if let Some(item) = local_fund_flow_proxy_evidence_item(trend) {
                items.push(item);
            }
        }
        items.push(technical_capital_evidence_item(
            trend,
            technical_score.unwrap_or(50.0),
        ));
    }
    if !capital_bucket_has_evidence(&items, ["institution_lhb", "institution_lhb_status", ""]) {
        items.push(institution_status_evidence_item(&as_of_trade_date));
    }
    if !capital_bucket_has_evidence(&items, ["news_rag", "community_sentiment", ""]) {
        items.push(message_sentiment_status_evidence_item(&as_of_trade_date));
    }
    dedup_capital_items(&mut items);

    let buckets = [
        (
            "资金流",
            FUND_FLOW_WEIGHT,
            ["fund_flow", "", ""],
            ["fund_flow", "", ""],
        ),
        (
            "机构席位",
            INSTITUTION_WEIGHT,
            ["institution_lhb", "institution_lhb_status", ""],
            ["institution_lhb", "", ""],
        ),
        (
            "消息情绪",
            NEWS_WEIGHT,
            [
                "news_rag",
                "community_sentiment",
                "message_sentiment_status",
            ],
            ["news_rag", "community_sentiment", ""],
        ),
        (
            "技术推断",
            TECHNICAL_WEIGHT,
            ["technical_behavior", "", ""],
            ["technical_behavior", "", ""],
        ),
    ];
    let mut weighted_sum = 0.0;
    let mut total_weight: f64 = 0.0;
    let mut contributions = BTreeMap::new();
    let mut scored_labels = Vec::new();
    let mut evidence_labels = Vec::new();
    for (label, weight, score_categories, evidence_categories) in buckets {
        let score = capital_bucket_score(&items, score_categories);
        let available = capital_bucket_has_evidence(&items, evidence_categories);
        if score.is_some() {
            scored_labels.push(label.to_string());
        }
        if available {
            evidence_labels.push(label.to_string());
        }
        let bucket_score = score.unwrap_or(50.0);
        weighted_sum += bucket_score * weight;
        total_weight += weight;
        contributions.insert(
            label.to_string(),
            json!({
                "score": score.map(round2),
                "weight": weight,
                "available": available,
            }),
        );
    }
    let composite_score = round2(weighted_sum / total_weight.max(0.01));
    let model_used = provided
        .as_ref()
        .map(|evidence| evidence.model_used)
        .unwrap_or(false);
    let confidence = capital_evidence_confidence(&items, technical_score.is_some());
    let sections = build_capital_evidence_sections(&items, &contributions);
    let freshness = provided
        .as_ref()
        .map(|evidence| evidence.freshness.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("refreshed")
        .to_string();
    let mut notes = Vec::new();
    if let Some(provided) = provided.as_ref() {
        for note in &provided.notes {
            push_capital_note(&mut notes, note);
        }
    }
    if !model_used {
        push_capital_note(
            &mut notes,
            "未调用模型，综合资金证据分由本地规则按资金流、机构席位、消息情绪、技术推断四类权重生成。",
        );
    }
    if !capital_bucket_has_evidence(&items, ["fund_flow", "", ""]) {
        push_capital_note(
            &mut notes,
            "资金流外部证据暂缺，本次用中性权重保留该桶，不把缺失数据当作流入或流出结论。",
        );
    }
    if !items.iter().any(|item| item.category == "institution_lhb") {
        push_capital_note(
            &mut notes,
            "龙虎榜机构席位未命中或未提供，仅表示当前证据源没有可展示记录，不代表机构没有交易。",
        );
    }
    if !capital_bucket_has_evidence(&items, ["news_rag", "community_sentiment", ""]) {
        push_capital_note(
            &mut notes,
            "消息情绪证据暂缺，当前不对新闻或社区情绪加减分。",
        );
    }

    CapitalEvidenceResult {
        stock_code: normalize_stock_code(&stock.code),
        generated_at: Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
        composite_score: Some(composite_score),
        confidence,
        model_used,
        as_of_trade_date: Some(as_of_trade_date),
        freshness,
        contributions,
        summary: Some(capital_evidence_summary(
            composite_score,
            &scored_labels,
            &evidence_labels,
        )),
        sections,
        items,
        notes,
    }
}

fn dedup_capital_items(items: &mut Vec<CapitalEvidenceItem>) {
    let mut seen = HashSet::new();
    items.retain(|item| {
        let key = format!(
            "{}|{}|{}|{}",
            item.category,
            item.source,
            item.title,
            item.date.as_deref().unwrap_or("")
        );
        seen.insert(key)
    });
}

fn capital_item_matches<const N: usize>(item: &CapitalEvidenceItem, categories: [&str; N]) -> bool {
    categories
        .iter()
        .any(|category| !category.is_empty() && *category == item.category.as_str())
}

fn capital_bucket_has_evidence<const N: usize>(
    items: &[CapitalEvidenceItem],
    categories: [&str; N],
) -> bool {
    items
        .iter()
        .any(|item| capital_item_matches(item, categories))
}

fn capital_bucket_score<const N: usize>(
    items: &[CapitalEvidenceItem],
    categories: [&str; N],
) -> Option<f64> {
    let mut total = 0.0;
    let mut count = 0.0;
    for item in items
        .iter()
        .filter(|item| capital_item_matches(item, categories))
    {
        if let Some(score) = item.score.filter(|value| value.is_finite()) {
            total += score;
            count += 1.0;
        }
    }
    if count > 0.0 {
        Some(round2(total / count))
    } else {
        None
    }
}

fn capital_evidence_confidence(items: &[CapitalEvidenceItem], has_technical: bool) -> String {
    let scored_external_count = [
        ["fund_flow", "", ""],
        ["institution_lhb", "", ""],
        [
            "news_rag",
            "community_sentiment",
            "message_sentiment_status",
        ],
    ]
    .into_iter()
    .filter(|categories| capital_bucket_score(items, *categories).is_some())
    .count();
    if scored_external_count >= 3 {
        "高".to_string()
    } else if scored_external_count >= 1 || has_technical {
        "中".to_string()
    } else {
        "低".to_string()
    }
}

fn capital_evidence_summary(
    composite_score: f64,
    scored_labels: &[String],
    evidence_labels: &[String],
) -> String {
    if !scored_labels.is_empty() {
        return format!(
            "综合资金证据分 {}，已纳入{}；缺失分数的证据桶按中性权重处理。",
            format_number(composite_score),
            scored_labels.join("、")
        );
    }
    if !evidence_labels.is_empty() {
        return format!(
            "综合资金证据分 {}，当前只有{}状态证据，缺失分数的证据桶按中性权重处理。",
            format_number(composite_score),
            evidence_labels.join("、")
        );
    }
    format!(
        "综合资金证据分 {}，外部资金证据暂缺，当前仅保留中性权重。",
        format_number(composite_score)
    )
}

fn push_capital_note(notes: &mut Vec<String>, note: impl AsRef<str>) {
    let clean = note.as_ref().trim();
    if clean.is_empty() || notes.iter().any(|existing| existing == clean) {
        return;
    }
    notes.push(clean.to_string());
}

fn local_fund_flow_proxy_evidence_item(
    trend: &TrendIndicatorResult,
) -> Option<CapitalEvidenceItem> {
    let signal = &trend.signal;
    let latest = trend.series.last()?;
    let score = local_fund_flow_proxy_score(latest);
    let mut metrics = BTreeMap::new();
    metrics.insert("收盘价".to_string(), format_number(signal.close));
    if let Some(change_pct) = signal.close_change_pct {
        metrics.insert("涨跌幅".to_string(), format_percent(change_pct));
    }
    let numeric_metrics = [
        ("量价热度", latest.volume_price_heat),
        ("吸筹强度", latest.accumulation_strength),
        ("吸筹指标", latest.accumulation_index),
        ("趋势热度", latest.trend_heat),
        ("异动热度", latest.anomaly_heat),
        ("人气热度", latest.popularity_heat),
    ];
    for (label, value) in numeric_metrics {
        if let Some(value) = value.filter(|value| value.is_finite()) {
            metrics.insert(label.to_string(), format_number(value));
        }
    }
    metrics.insert("证据类型".to_string(), "本地日线量价代理".to_string());
    Some(CapitalEvidenceItem {
        category: "fund_flow".to_string(),
        source: "Tauri/Rust 日线量价".to_string(),
        title: "本地量价资金代理".to_string(),
        date: Some(signal.date.clone()),
        metrics,
        sentiment: score_sentiment(score).to_string(),
        weight: 0.35,
        confidence: "中".to_string(),
        url: None,
        score: Some(score),
        note: Some(format!(
            "资金流代理分 {}：由量价热度、吸筹强度、趋势热度和承接指标合成；它不是外部主力净流入数据。",
            format_number(score)
        )),
    })
}

fn local_fund_flow_proxy_score(point: &TrendIndicatorPoint) -> f64 {
    let volume_price = point.volume_price_heat.unwrap_or(50.0).clamp(0.0, 100.0);
    let accumulation_strength = point
        .accumulation_strength
        .unwrap_or(50.0)
        .clamp(0.0, 100.0);
    let trend_heat = point.trend_heat.unwrap_or(50.0).clamp(0.0, 100.0);
    let rebound = point.rebound_signal.unwrap_or(50.0).clamp(0.0, 100.0);
    let accumulation_index = point
        .accumulation_index
        .map(|value| (50.0 + value * 2.0).clamp(0.0, 100.0))
        .unwrap_or(50.0);
    round2(
        volume_price * 0.35
            + accumulation_strength * 0.25
            + trend_heat * 0.20
            + accumulation_index * 0.15
            + rebound * 0.05,
    )
}

fn technical_capital_score(trend: &TrendIndicatorResult) -> f64 {
    let signal = &trend.signal;
    let quant_max = signal.quant_score_max.max(1) as f64;
    let pattern_max = signal.pattern_score_max.max(1) as f64;
    let quant = (signal.quant_score as f64 / quant_max * 100.0).clamp(0.0, 100.0);
    let pattern = (signal.pattern_score as f64 / pattern_max * 100.0).clamp(0.0, 100.0);
    let mut score = quant * 0.55 + pattern * 0.45;
    if signal.short_buy {
        score += 8.0;
    }
    if signal.kdj_golden_cross {
        score += 6.0;
    }
    if signal.kdj_oversold {
        score += 5.0;
    }
    if signal.red_hold {
        score += 4.0;
    }
    if signal.swl_above_sws {
        score += 3.0;
    }
    if signal.white_exit {
        score -= 12.0;
    }
    if signal.kdj_dead_cross {
        score -= 10.0;
    }
    if signal.kdj_overbought {
        score -= 8.0;
    }
    if signal.cyan_watch {
        score -= 4.0;
    }
    round2(score.clamp(0.0, 100.0))
}

fn technical_capital_evidence_item(
    trend: &TrendIndicatorResult,
    score: f64,
) -> CapitalEvidenceItem {
    let signal = &trend.signal;
    let mut metrics = BTreeMap::new();
    metrics.insert(
        "\u{6536}\u{76d8}\u{4ef7}".to_string(),
        format_number(signal.close),
    );
    if let Some(change_pct) = signal.close_change_pct {
        metrics.insert(
            "\u{6da8}\u{8dcc}\u{5e45}".to_string(),
            format_percent(change_pct),
        );
    }
    metrics.insert(
        "\u{91cf}\u{5316}\u{5206}".to_string(),
        format!("{}/{}", signal.quant_score, signal.quant_score_max),
    );
    metrics.insert(
        "\u{5f62}\u{6001}\u{5206}".to_string(),
        format!("{}/{}", signal.pattern_score, signal.pattern_score_max),
    );
    if let (Some(k), Some(d), Some(j)) = (signal.k, signal.d, signal.j) {
        metrics.insert(
            "KDJ".to_string(),
            format!(
                "K {} / D {} / J {}",
                format_number(k),
                format_number(d),
                format_number(j)
            ),
        );
    }
    if let (Some(swl), Some(sws)) = (signal.swl, signal.sws) {
        metrics.insert(
            "SWL/SWS".to_string(),
            format!("{} / {}", format_number(swl), format_number(sws)),
        );
    }
    metrics.insert("\u{72b6}\u{6001}".to_string(), signal.status.clone());
    if let Some(point) = trend.series.last() {
        let point_metrics = [
            ("吸筹指标", point.accumulation_index),
            ("吸筹强度", point.accumulation_strength),
            ("波段机会", point.swing_opportunity),
            ("绝地反击", point.rebound_signal),
            ("趋势热度", point.trend_heat),
            ("量价热度", point.volume_price_heat),
            ("异动热度", point.anomaly_heat),
            ("人气热度", point.popularity_heat),
        ];
        for (label, value) in point_metrics {
            if let Some(value) = value.filter(|value| value.is_finite()) {
                metrics.insert(label.to_string(), format_number(value));
            }
        }
    }
    metrics.insert(
        "\u{77ed}\u{7ebf}\u{4e70}\u{70b9}".to_string(),
        if signal.short_buy {
            "\u{662f}"
        } else {
            "\u{5426}"
        }
        .to_string(),
    );
    let risk_labels = capital_technical_risk_labels(signal);
    if !risk_labels.is_empty() {
        metrics.insert(
            "\u{98ce}\u{9669}\u{4fe1}\u{53f7}".to_string(),
            risk_labels.join("\u{3001}"),
        );
    }
    CapitalEvidenceItem {
        category: "technical_behavior".to_string(),
        source: "Tauri/Rust \u{6280}\u{672f}\u{6307}\u{6807}".to_string(),
        title: "\u{6280}\u{672f}\u{63a8}\u{65ad}\u{8d44}\u{91d1}\u{627f}\u{63a5}".to_string(),
        date: Some(signal.date.clone()),
        metrics,
        sentiment: score_sentiment(score).to_string(),
        weight: 0.25,
        confidence: "\u{4e2d}".to_string(),
        url: None,
        score: Some(score),
        note: Some(technical_capital_note(signal, score)),
    }
}

fn institution_status_evidence_item(as_of_trade_date: &str) -> CapitalEvidenceItem {
    let mut metrics = BTreeMap::new();
    metrics.insert(
        "状态".to_string(),
        "当前没有可展示的龙虎榜机构席位记录".to_string(),
    );
    metrics.insert("查询窗口".to_string(), as_of_trade_date.to_string());
    metrics.insert(
        "已尝试信源".to_string(),
        "本地缓存 / 已提供外部证据".to_string(),
    );
    CapitalEvidenceItem {
        category: "institution_lhb_status".to_string(),
        source: "Tauri/Rust".to_string(),
        title: "机构席位暂无外部证据".to_string(),
        date: Some(as_of_trade_date.to_string()),
        metrics,
        sentiment: "uncertain".to_string(),
        weight: 0.25,
        confidence: "低".to_string(),
        url: None,
        score: None,
        note: Some(
            "当前窗口没有可展示的龙虎榜机构席位证据；这只是证据缺口，不代表机构没有买卖。"
                .to_string(),
        ),
    }
}

fn message_sentiment_status_evidence_item(as_of_trade_date: &str) -> CapitalEvidenceItem {
    let mut metrics = BTreeMap::new();
    metrics.insert("状态".to_string(), "本地消息缓存暂无可用证据".to_string());
    metrics.insert("查询窗口".to_string(), as_of_trade_date.to_string());
    metrics.insert(
        "已尝试信源".to_string(),
        "本地新闻缓存 / 已提供外部证据".to_string(),
    );
    CapitalEvidenceItem {
        category: "message_sentiment_status".to_string(),
        source: "Tauri/Rust".to_string(),
        title: "消息情绪暂无本地证据".to_string(),
        date: Some(as_of_trade_date.to_string()),
        metrics,
        sentiment: "uncertain".to_string(),
        weight: 0.15,
        confidence: "低".to_string(),
        url: None,
        score: None,
        note: Some("当前没有可纳入评分的新闻或社区情绪证据；该桶保留中性权重。".to_string()),
    }
}

fn build_capital_evidence_sections(
    items: &[CapitalEvidenceItem],
    contributions: &BTreeMap<String, Value>,
) -> Vec<CapitalEvidenceSection> {
    let definitions = [
        (
            "fund_flow",
            "资金流",
            "资金流",
            ["fund_flow", "", ""],
            ["fund_flow", "", ""],
        ),
        (
            "institution_lhb",
            "机构席位",
            "机构席位",
            ["institution_lhb", "institution_lhb_status", ""],
            ["institution_lhb", "", ""],
        ),
        (
            "message_sentiment",
            "消息情绪",
            "消息情绪",
            [
                "news_rag",
                "community_sentiment",
                "message_sentiment_status",
            ],
            ["news_rag", "community_sentiment", ""],
        ),
        (
            "technical_behavior",
            "技术推断",
            "技术推断",
            ["technical_behavior", "", ""],
            ["technical_behavior", "", ""],
        ),
    ];
    definitions
        .into_iter()
        .map(
            |(key, title, contribution_key, categories, evidence_categories)| {
                let section_items = items
                    .iter()
                    .filter(|item| {
                        categories
                            .iter()
                            .any(|category| !category.is_empty() && *category == item.category)
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                let contribution = contributions.get(contribution_key);
                let score = contribution
                    .and_then(|value| value.get("score"))
                    .and_then(Value::as_f64);
                let weight = contribution
                    .and_then(|value| value.get("weight"))
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                let contribution_available = contribution
                    .and_then(|value| value.get("available"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let has_real_evidence = section_items.iter().any(|item| {
                    evidence_categories
                        .iter()
                        .any(|category| !category.is_empty() && *category == item.category)
                });
                let available = contribution_available || has_real_evidence;
                CapitalEvidenceSection {
                    key: key.to_string(),
                    title: title.to_string(),
                    score,
                    weight,
                    available,
                    summary: Some(capital_section_summary(
                        title,
                        score,
                        available,
                        section_items.len(),
                    )),
                    items: section_items,
                }
            },
        )
        .collect()
}

fn capital_section_summary(
    title: &str,
    score: Option<f64>,
    available: bool,
    item_count: usize,
) -> String {
    if available {
        if let Some(score) = score {
            return format!("{}\u{8bc1}\u{636e}\u{5206} {}\u{ff0c}\u{547d}\u{4e2d} {} \u{6761}\u{8bc1}\u{636e}\u{3002}", title, format_number(score), item_count);
        }
        return format!("{}\u{6709} {} \u{6761}\u{72b6}\u{6001}\u{6216}\u{8f85}\u{52a9}\u{8bc1}\u{636e}\u{3002}", title, item_count);
    }
    format!(
        "{}\u{6682}\u{65e0}\u{53ef}\u{7528}\u{8bc1}\u{636e}\u{3002}",
        title
    )
}

fn capital_technical_risk_labels(signal: &TrendIndicatorSignal) -> Vec<String> {
    let mut labels = Vec::new();
    if signal.white_exit {
        labels.push("\u{767d}\u{7ebf}\u{5356}\u{51fa}".to_string());
    }
    if signal.kdj_dead_cross {
        labels.push("KDJ \u{6b7b}\u{53c9}".to_string());
    }
    if signal.kdj_overbought {
        labels.push("KDJ \u{9ad8}\u{4f4d}\u{8d85}\u{4e70}".to_string());
    }
    if signal.cyan_watch {
        labels.push("\u{9752}\u{8272}\u{89c2}\u{671b}".to_string());
    }
    labels
}

fn technical_capital_note(signal: &TrendIndicatorSignal, score: f64) -> String {
    let mut parts = Vec::new();
    if signal.short_buy {
        parts.push("\u{77ed}\u{7ebf}\u{4e70}\u{70b9}\u{6210}\u{7acb}".to_string());
    }
    if signal.kdj_golden_cross {
        parts.push("KDJ \u{91d1}\u{53c9}".to_string());
    }
    if signal.swl_above_sws {
        parts.push("SWL \u{5728} SWS \u{4e0a}\u{65b9}".to_string());
    }
    if signal.white_exit || signal.kdj_dead_cross || signal.kdj_overbought {
        parts.push("\u{5b58}\u{5728}\u{6280}\u{672f}\u{98ce}\u{9669}\u{4fe1}\u{53f7}".to_string());
    }
    if parts.is_empty() {
        parts.push("\u{6280}\u{672f}\u{6307}\u{6807}\u{672a}\u{51fa}\u{73b0}\u{5f3a}\u{52bf}\u{627f}\u{63a5}".to_string());
    }
    format!(
        "\u{6280}\u{672f}\u{63a8}\u{65ad}\u{5206} {}\u{ff1a}{}\u{3002}\u{8be5}\u{9879}\u{53ea}\u{4f5c}\u{8d44}\u{91d1}\u{884c}\u{4e3a}\u{7684}\u{8f85}\u{52a9}\u{89c2}\u{5bdf}\u{ff0c}\u{4e0d}\u{7b49}\u{540c}\u{4e8e}\u{771f}\u{5b9e}\u{8d44}\u{91d1}\u{6d41}\u{3002}",
        format_number(score),
        parts.join("\u{3001}")
    )
}

fn score_sentiment(score: f64) -> &'static str {
    if score >= 60.0 {
        "positive"
    } else if score <= 40.0 {
        "negative"
    } else {
        "neutral"
    }
}

fn capital_trade_date(value: &str) -> Option<String> {
    let raw = value.trim();
    if raw.len() >= 10
        && raw.as_bytes().get(4) == Some(&b'-')
        && raw.as_bytes().get(7) == Some(&b'-')
    {
        return Some(raw[..10].to_string());
    }
    let digits = raw
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.len() < 8 {
        return None;
    }
    NaiveDate::parse_from_str(&digits[..8], "%Y%m%d")
        .ok()
        .map(|date| date.format("%Y-%m-%d").to_string())
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

pub fn trend_screen_with_mock(request: &TrendScreenRequest) -> CoreResult<TrendScreenResult> {
    trend_screen_with_source(&MockDataSource, request)
}

pub fn trend_screen_with_data(
    data: &CoreDataSet,
    request: &TrendScreenRequest,
) -> CoreResult<TrendScreenResult> {
    let source = StaticDataSource::new(data.clone());
    trend_screen_with_source(&source, request)
}

pub fn trend_screen_with_source(
    source: &impl MarketDataSource,
    request: &TrendScreenRequest,
) -> CoreResult<TrendScreenResult> {
    let universe = source.list_stocks()?;
    let candidate_pool = screen_candidate_pool(&universe, &request.criteria, request.limit);
    let mut items = Vec::new();
    let mut skipped = 0_usize;

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
        let trend_score = trend_score(&analysis.signal);
        let final_score = combined_trend_score(candidate.score, trend_score);
        let mut reasons = candidate.reasons.clone();
        reasons.extend(analysis.signal.reasons.clone());
        items.push(TrendStockSignal {
            stock: candidate.stock.clone(),
            base_score: round6(candidate.score),
            trend_score: round6(trend_score),
            final_score: round6(final_score),
            signal: analysis.signal.clone(),
            reasons,
            explanation: trend_screen_explanation(
                candidate,
                &analysis.signal,
                trend_score,
                final_score,
            ),
        });
    }

    items.sort_by(|left, right| right.final_score.total_cmp(&left.final_score));
    items.truncate(request.limit.clamp(1, 100));
    let mut notes = trend_notes();
    notes.extend(deducted_profit_rule_notes(&universe, &request.criteria));
    if skipped > 0 {
        notes.push(format!(
            "Skipped {skipped} stocks without usable OHLC history."
        ));
    }
    Ok(TrendScreenResult {
        total: candidate_pool.len(),
        returned: items.len(),
        items,
        screen_style: "short_buy".to_string(),
        notes,
    })
}

pub fn run_agent_with_mock(message: &str) -> CoreResult<AgentResponse> {
    run_agent_with_source(&MockDataSource, message)
}

pub fn run_agent_with_data(data: &CoreDataSet, message: &str) -> CoreResult<AgentResponse> {
    let source = StaticDataSource::new(data.clone());
    run_agent_with_source(&source, message)
}

pub fn run_agent_stream_with_data_events(
    data: &CoreDataSet,
    message: &str,
    run_id: Option<&str>,
) -> Vec<AgentStreamEvent> {
    let source = StaticDataSource::new(data.clone());
    run_agent_stream_with_source_events(&source, message, run_id)
}

pub fn run_agent_with_source(
    source: &impl MarketDataSource,
    message: &str,
) -> CoreResult<AgentResponse> {
    let lower = message.to_lowercase();
    let criteria = heuristic_criteria(message);

    if contains_any(
        &lower,
        &[
            "趋势",
            "上升趋势",
            "趋势指标",
            "短买",
            "主力吸筹",
            "红色持股",
            "青色观望",
            "swl",
            "sws",
            "量化评分",
            "支撑",
            "阻力",
        ],
    ) {
        let default_end_date = current_system_date_yyyymmdd();
        let trend_request = TrendScreenRequest {
            criteria,
            start_date: extract_date(message, "20200101", true),
            end_date: extract_date(message, &default_end_date, false),
            limit: 10,
        };
        let data = serde_json::to_value(trend_screen_with_source(source, &trend_request)?)?;
        return Ok(AgentResponse {
            reply: research_reply("已按趋势指标做选股排序。"),
            action: "trend_screen".to_string(),
            criteria: None,
            backtest: None,
            graph_screen: None,
            trend_screen: Some(trend_request),
            data: Some(data),
        });
    }

    if contains_any(
        &lower,
        &[
            "关系",
            "产业链",
            "上下游",
            "供应链",
            "关联",
            "联动",
            "图学习",
            "知识图谱",
            "graph",
            "gnn",
            "langgraph",
        ],
    ) {
        let graph_request = GraphScreenRequest {
            criteria,
            seed_codes: extract_codes(message),
            relation_depth: if contains_any(&lower, &["二级", "2层", "2-hop", "two hop"]) {
                2
            } else {
                1
            },
            relation_weight: 0.4,
            limit: 10,
        };
        let data = serde_json::to_value(graph_screen_with_source(source, &graph_request)?)?;
        return Ok(AgentResponse {
            reply: research_reply("已按股票关系图做关系传播选股。"),
            action: "graph_screen".to_string(),
            criteria: None,
            backtest: None,
            graph_screen: Some(graph_request),
            trend_screen: None,
            data: Some(data),
        });
    }

    if message.contains("回测") || lower.contains("backtest") {
        let default_end_date = current_system_date_yyyymmdd();
        let backtest = BacktestRequest {
            criteria,
            source: default_backtest_source(),
            stock_codes: Vec::new(),
            start_date: extract_date(message, "20200101", true),
            end_date: extract_date(message, &default_end_date, false),
            top_n: default_top_n(),
            initial_cash: default_initial_cash(),
        };
        let data = serde_json::to_value(backtest_with_source(source, &backtest)?)?;
        return Ok(AgentResponse {
            reply: research_reply("已按描述执行本地回测。"),
            action: "backtest".to_string(),
            criteria: None,
            backtest: Some(backtest),
            graph_screen: None,
            trend_screen: None,
            data: Some(data),
        });
    }

    if contains_any(&lower, &["选股", "筛选", "screen", "挑股票"]) {
        let data = serde_json::to_value(screen_with_source(source, &criteria)?)?;
        return Ok(AgentResponse {
            reply: research_reply("已按描述筛选股票。"),
            action: "screen".to_string(),
            criteria: Some(criteria),
            backtest: None,
            graph_screen: None,
            trend_screen: None,
            data: Some(data),
        });
    }

    Ok(AgentResponse {
        reply: research_reply("请说明要普通选股、关系图选股，还是回测。"),
        action: "clarify".to_string(),
        criteria: None,
        backtest: None,
        graph_screen: None,
        trend_screen: None,
        data: None,
    })
}

pub fn run_agent_stream_with_source_events(
    source: &impl MarketDataSource,
    message: &str,
    run_id: Option<&str>,
) -> Vec<AgentStreamEvent> {
    let run_id = run_id.unwrap_or("gp-agent-run").to_string();
    let mut events = vec![
        agent_status_event(&run_id, "understand", "理解意图", 8, None),
        agent_status_event(&run_id, "intent", "识别动作", 24, None),
        agent_status_event(&run_id, "execute", "执行本地智能体", 64, None),
    ];

    match run_agent_with_source(source, message) {
        Ok(response) => {
            let action = response.action.clone();
            events.push(agent_status_event(
                &run_id,
                "format",
                "整理结果",
                88,
                Some(action.as_str()),
            ));
            events.push(agent_status_event(
                &run_id,
                "complete",
                "完成",
                100,
                Some(action.as_str()),
            ));
            events.push(AgentStreamEvent {
                run_id,
                event_type: "result".to_string(),
                stage: None,
                label: None,
                percent: None,
                action: Some(action),
                response: Some(response),
                message: None,
            });
        }
        Err(error) => {
            events.push(AgentStreamEvent {
                run_id,
                event_type: "error".to_string(),
                stage: None,
                label: None,
                percent: None,
                action: None,
                response: None,
                message: Some(error.to_string()),
            });
        }
    }

    events
}

fn agent_status_event(
    run_id: &str,
    stage: &str,
    label: &str,
    percent: u8,
    action: Option<&str>,
) -> AgentStreamEvent {
    AgentStreamEvent {
        run_id: run_id.to_string(),
        event_type: "status".to_string(),
        stage: Some(stage.to_string()),
        label: Some(label.to_string()),
        percent: Some(percent),
        action: action.map(ToOwned::to_owned),
        response: None,
        message: None,
    }
}

pub fn run_mobile_stock_skill(request: &MobileStockSkillRequest) -> MobileStockSkillResult {
    let mut positive_factors = Vec::new();
    let mut negative_factors = Vec::new();
    let mut neutral_information = Vec::new();
    let mut unverified_leads = Vec::new();
    let mut notes = vec![
        "手机端股票分析 Skill 已按结构化信源条目生成结论。".to_string(),
        "仅供选股研究，不构成投资建议。".to_string(),
    ];

    for source in request.sources.iter().take(80) {
        let finding = mobile_source_to_finding(source);
        match finding.label.as_str() {
            "positive" => positive_factors.push(finding),
            "negative" => negative_factors.push(finding),
            "neutral" => neutral_information.push(finding),
            _ => unverified_leads.push(finding),
        }
    }

    sort_findings(&mut positive_factors);
    sort_findings(&mut negative_factors);
    sort_findings(&mut neutral_information);
    sort_findings(&mut unverified_leads);

    if request.sources.is_empty() {
        notes.push("未找到可靠信源；请先接入 CNINFO、通达信 F10 或公开新闻 URL。".to_string());
    }
    if request
        .sources
        .iter()
        .any(|item| normalize_source_tier(&item.source_tier) == "community")
    {
        notes.push("社区来源只作为待验证线索，不能单独形成利好或利空事实。".to_string());
    }

    let overview = MobileStockSkillOverview {
        stock_code: request.stock_code.trim().to_uppercase(),
        stock_name: request.stock_name.trim().to_string(),
        overall_label: mobile_overall_label(
            positive_factors.len(),
            negative_factors.len(),
            unverified_leads.len(),
        ),
        summary: mobile_overview_summary(
            request,
            positive_factors.len(),
            negative_factors.len(),
            neutral_information.len(),
            unverified_leads.len(),
        ),
        positive_count: positive_factors.len(),
        negative_count: negative_factors.len(),
        neutral_count: neutral_information.len(),
        unverified_count: unverified_leads.len(),
    };

    MobileStockSkillResult {
        overview,
        positive_factors,
        negative_factors,
        neutral_information,
        unverified_leads,
        notes,
    }
}

fn research_reply(text: &str) -> String {
    format!("{text} 仅供选股研究，不构成投资建议。")
}

fn mobile_source_to_finding(source: &MobileStockSourceItem) -> MobileStockSkillFinding {
    let source_tier = normalize_source_tier(&source.source_tier);
    let evidence = first_non_empty(&[
        source.evidence.as_str(),
        source.summary.as_str(),
        source.title.as_str(),
    ]);
    let label = classify_mobile_source(&source_tier, &source.title, &source.summary, &evidence);
    let confidence = mobile_confidence(&source_tier, &label, !evidence.is_empty());

    MobileStockSkillFinding {
        label: label.clone(),
        title: truncate_chars(
            &first_non_empty(&[source.title.as_str(), source.summary.as_str(), "未命名信源"]),
            80,
        ),
        summary: mobile_finding_summary(&label, source),
        source_tier,
        source_name: first_non_empty(&[source.source_name.as_str(), "未标注来源"]),
        published_at: source.published_at.clone(),
        evidence: truncate_chars(&evidence, 220),
        confidence,
        risk_note: mobile_risk_note(&label),
    }
}

fn classify_mobile_source(source_tier: &str, title: &str, summary: &str, evidence: &str) -> String {
    if source_tier == "community" || evidence.trim().is_empty() {
        return "unverified".to_string();
    }
    let text = format!("{title} {summary} {evidence}").to_lowercase();
    let positive = contains_any(
        &text,
        &[
            "增长", "预增", "扭亏", "订单", "中标", "签订", "扩产", "回购", "增持", "分红", "盈利",
            "改善", "突破", "获批", "投产", "景气", "涨价",
        ],
    );
    let negative = contains_any(
        &text,
        &[
            "下滑", "下降", "亏损", "预亏", "减持", "处罚", "调查", "诉讼", "仲裁", "违约", "终止",
            "取消", "风险", "计提", "减值", "停产", "限产", "退市",
        ],
    );

    match (positive, negative) {
        (true, false) => "positive".to_string(),
        (false, true) => "negative".to_string(),
        (true, true) => "neutral".to_string(),
        (false, false) => "neutral".to_string(),
    }
}

fn mobile_confidence(source_tier: &str, label: &str, has_evidence: bool) -> f64 {
    if !has_evidence || label == "unverified" {
        return 0.2;
    }
    let base = match source_tier {
        "filing" => 0.9,
        "financial_snapshot" => 0.75,
        "news" => 0.62,
        "research" => 0.55,
        "manual_url" => 0.5,
        "community" => 0.2,
        _ => 0.4,
    };
    round4(base)
}

fn mobile_finding_summary(label: &str, source: &MobileStockSourceItem) -> String {
    let body = first_non_empty(&[
        source.summary.as_str(),
        source.evidence.as_str(),
        source.title.as_str(),
    ]);
    let prefix = match label {
        "positive" => "选股研究视角下偏利好",
        "negative" => "选股研究视角下偏利空",
        "neutral" => "中性信息",
        _ => "待验证线索",
    };
    format!("{prefix}：{}", truncate_chars(&body, 120))
}

fn mobile_risk_note(label: &str) -> String {
    let note = match label {
        "positive" => "需要用后续公告、财报和成交数据验证，不能据此作交易决定。",
        "negative" => "需要确认事项影响范围和持续性，不能单独作为交易依据。",
        "neutral" => "属于信息披露或背景材料，需结合财务和行情继续验证。",
        _ => "来源不足或未被官方披露确认，只能作为待验证线索。",
    };
    format!("{note} 仅供选股研究，不构成投资建议。")
}

fn mobile_overall_label(positive: usize, negative: usize, unverified: usize) -> String {
    if positive == 0 && negative == 0 && unverified > 0 {
        return "unverified".to_string();
    }
    if positive > negative {
        "positive".to_string()
    } else if negative > positive {
        "negative".to_string()
    } else {
        "neutral".to_string()
    }
}

fn mobile_overview_summary(
    request: &MobileStockSkillRequest,
    positive: usize,
    negative: usize,
    neutral: usize,
    unverified: usize,
) -> String {
    let target = if request.stock_name.trim().is_empty() {
        request.stock_code.trim()
    } else {
        request.stock_name.trim()
    };
    if positive + negative + neutral + unverified == 0 {
        return format!(
            "{target} 未找到可靠信源，暂不能形成利好利空判断。仅供选股研究，不构成投资建议。"
        );
    }
    format!(
        "{target} 当前命中利好 {positive} 条、利空 {negative} 条、中性 {neutral} 条、待验证 {unverified} 条。仅供选股研究，不构成投资建议。"
    )
}

fn sort_findings(findings: &mut [MobileStockSkillFinding]) {
    findings.sort_by(|left, right| {
        right
            .confidence
            .total_cmp(&left.confidence)
            .then_with(|| right.published_at.cmp(&left.published_at))
    });
}

fn normalize_source_tier(value: &str) -> String {
    let normalized = value.trim().to_lowercase();
    match normalized.as_str() {
        "filing" | "financial_snapshot" | "news" | "research" | "community" | "manual_url" => {
            normalized
        }
        "tdx" | "f10" => "financial_snapshot".to_string(),
        "cninfo" | "notice" | "announcement" => "filing".to_string(),
        "" => "manual_url".to_string(),
        _ => normalized,
    }
}

fn first_non_empty(values: &[&str]) -> String {
    values
        .iter()
        .find_map(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .unwrap_or_default()
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut result = String::new();
    for (index, ch) in value.chars().enumerate() {
        if index >= max_chars {
            result.push_str("...");
            break;
        }
        result.push(ch);
    }
    result
}

pub fn screen_stocks(universe: &[StockItem], criteria: &ScreenCriteria) -> ScreenResult {
    let mut screened = Vec::new();
    let notes = deducted_profit_rule_notes(universe, criteria);

    for stock in universe {
        let Some(reasons) = matches_stock(stock, criteria) else {
            continue;
        };
        screened.push(score_stock(stock, &reasons, &criteria.score_profile));
    }

    sort_screened(&mut screened, criteria);

    let limit = criteria.limit.clamp(1, 200);
    let (items, promoted) = primary_screen_items(&screened, criteria, limit);
    let groups = screen_result_groups(&screened);
    let mut notes = notes;
    if promoted {
        notes.push("\u{5df2}\u{6309}\u{5171}\u{4eab}\u{7b5b}\u{9009}\u{89c4}\u{5219}\u{5747}\u{8861}\u{8bc4}\u{4f30}\u{4e3b}\u{9898}\u{3001}\u{57fa}\u{672c}\u{9762}\u{3001}\u{4f30}\u{503c}\u{3001}\u{89c4}\u{6a21}\u{548c}\u{98ce}\u{9669}\u{ff0c}\u{5e76}\u{4f18}\u{5148}\u{5c55}\u{793a}\u{70ed}\u{95e8}\u{4e3b}\u{9898}\u{5019}\u{9009}\u{3002}".to_string());
    }
    notes.push(format!(
        "{}{}{}",
        "\u{666e}\u{901a}\u{7b5b}\u{9009}\u{5df2}\u{62c6}\u{6210}\u{70ed}\u{95e8}\u{80a1}\u{548c}\u{7efc}\u{5408}\u{5206}\u{5927}\u{4e8e} ",
        POTENTIAL_SCORE_THRESHOLD,
        " \u{7684}\u{6f5c}\u{529b}\u{80a1}\u{ff0c}\u{6bcf}\u{7c7b}\u{6700}\u{591a} 10 \u{53ea}\u{3002}",
    ));
    ScreenResult {
        total: screened.len(),
        returned: items.len(),
        items,
        groups,
        notes,
    }
}

fn deducted_profit_rule_notes(universe: &[StockItem], criteria: &ScreenCriteria) -> Vec<String> {
    if criteria.min_deducted_net_profit_billion.is_none()
        && criteria.min_deducted_net_profit_margin.is_none()
        && criteria.min_deducted_net_profit_growth_rate.is_none()
    {
        return Vec::new();
    }
    let with_metrics = universe
        .iter()
        .filter(|stock| has_deducted_profit_rule_metrics(stock, criteria))
        .count();
    if with_metrics < universe.len() {
        vec![format!(
            "扣非净利润规则已启用；当前股票池 {with_metrics}/{} 只股票带扣非财务字段，缺字段股票按不达标处理。",
            universe.len()
        )]
    } else {
        Vec::new()
    }
}

fn has_deducted_profit_rule_metrics(stock: &StockItem, criteria: &ScreenCriteria) -> bool {
    if criteria.min_deducted_net_profit_billion.is_some()
        && stock.deducted_net_profit_billion.is_none()
    {
        return false;
    }
    if criteria.min_deducted_net_profit_margin.is_some()
        && stock.deducted_net_profit_margin.is_none()
    {
        return false;
    }
    if criteria.min_deducted_net_profit_growth_rate.is_some()
        && stock.deducted_net_profit_growth_rate.is_none()
    {
        return false;
    }
    true
}

pub fn graph_screen_stocks(
    universe: &[StockItem],
    provider_relations: &[StockRelation],
    request: &GraphScreenRequest,
) -> GraphScreenResult {
    let candidate_pool = screen_candidate_pool(universe, &request.criteria, request.limit);
    let center_context = resolve_center_context(&candidate_pool, &request.seed_codes);
    let candidate_by_code: HashMap<String, ScreenedStock> = candidate_pool
        .iter()
        .map(|item| (item.stock.code.clone(), item.clone()))
        .collect();
    let all_stocks: HashMap<String, StockItem> = universe
        .iter()
        .map(|stock| (stock.code.clone(), stock.clone()))
        .collect();

    let mut raw_relations = provider_relations.to_vec();
    raw_relations.extend(infer_industry_relations(universe));
    let relations = merge_relations(&raw_relations);
    let adjacency = build_adjacency(&relations);
    let base_scores = normalize_scores(
        &candidate_by_code
            .iter()
            .map(|(code, item)| (code.clone(), item.score))
            .collect(),
    );
    let seed_codes: HashSet<String> = center_context.codes.iter().cloned().collect();
    let max_depth = request.relation_depth.clamp(1, 3);
    let relation_weight = request.relation_weight.clamp(0.0, 1.0);

    let mut signals = Vec::new();
    for (code, screened) in candidate_by_code {
        let relation_score =
            relation_score(&code, &base_scores, &adjacency, &seed_codes, max_depth);
        let base_score = *base_scores.get(&code).unwrap_or(&0.0);
        let final_score = (1.0 - relation_weight) * base_score + relation_weight * relation_score;
        let related = top_related(&code, &relations, &all_stocks, 5);
        signals.push(GraphStockSignal {
            stock: screened.stock.clone(),
            base_score: round6(base_score),
            relation_score: round6(relation_score),
            final_score: round6(final_score),
            suggested_weight: 0.0,
            reasons: graph_reasons(&screened, relation_score),
            related: related.clone(),
            explanation: graph_explanation(
                &screened,
                base_score,
                relation_score,
                final_score,
                relation_weight,
                &center_context,
                &related,
            ),
        });
    }

    signals.sort_by(|left, right| right.final_score.total_cmp(&left.final_score));
    signals.truncate(request.limit.clamp(1, 100));
    assign_weights(&mut signals);

    let mut notes = vec![
        "Graph screening uses lightweight relation propagation in gp-core.".to_string(),
        "LangGraph is only agent orchestration; stock relations are modeled by relation scoring."
            .to_string(),
    ];
    notes.extend(deducted_profit_rule_notes(universe, &request.criteria));
    if center_context.mode == "theme_center" {
        notes.push(format!(
            "\u{672a}\u{63d0}\u{4f9b}\u{79cd}\u{5b50}\u{80a1}\u{ff0c}\u{56fe}\u{8c31}\u{9009}\u{80a1}\u{4f7f}\u{7528}{}\u{3002}",
            center_context.label
        ));
    }
    if relations.is_empty() {
        notes.push("\u{672a}\u{627e}\u{5230}\u{80a1}\u{7968}\u{5173}\u{7cfb}\u{6570}\u{636e}\u{ff0c}\u{7ed3}\u{679c}\u{5df2}\u{56de}\u{9000}\u{4e3a}\u{57fa}\u{7840}\u{7b5b}\u{9009}\u{5206}\u{3002}".to_string());
    }

    GraphScreenResult {
        total: candidate_pool.len(),
        returned: signals.len(),
        relation_count: relations.len(),
        items: signals,
        center_context,
        notes,
    }
}

pub fn mock_stocks() -> Vec<StockItem> {
    vec![
        StockItem {
            code: "600519.SH".to_string(),
            name: "贵州茅台".to_string(),
            industry: "白酒".to_string(),
            is_st: false,
            price: 1700.0,
            pe: Some(32.1),
            pb: Some(10.5),
            roe: Some(0.32),
            market_cap_billion: Some(2200.0),
            dividend_yield: Some(0.02),
            deducted_net_profit_billion: Some(680.0),
            deducted_net_profit_margin: Some(48.0),
            deducted_net_profit_growth_rate: Some(12.0),
            ..StockItem::default()
        },
        StockItem {
            code: "000001.SZ".to_string(),
            name: "平安银行".to_string(),
            industry: "银行".to_string(),
            is_st: false,
            price: 12.3,
            pe: Some(5.8),
            pb: Some(0.6),
            roe: Some(0.12),
            market_cap_billion: Some(240.0),
            dividend_yield: Some(0.05),
            deducted_net_profit_billion: Some(450.0),
            deducted_net_profit_margin: Some(38.0),
            deducted_net_profit_growth_rate: Some(6.0),
            ..StockItem::default()
        },
        StockItem {
            code: "300750.SZ".to_string(),
            name: "宁德时代".to_string(),
            industry: "动力电池".to_string(),
            is_st: false,
            price: 195.0,
            pe: Some(25.5),
            pb: Some(6.5),
            roe: Some(0.18),
            market_cap_billion: Some(900.0),
            dividend_yield: Some(0.01),
            deducted_net_profit_billion: Some(380.0),
            deducted_net_profit_margin: Some(16.0),
            deducted_net_profit_growth_rate: Some(18.0),
            ..StockItem::default()
        },
        StockItem {
            code: "002594.SZ".to_string(),
            name: "比亚迪".to_string(),
            industry: "汽车".to_string(),
            is_st: false,
            price: 246.0,
            pe: Some(22.4),
            pb: Some(4.8),
            roe: Some(0.22),
            market_cap_billion: Some(720.0),
            dividend_yield: Some(0.006),
            deducted_net_profit_billion: Some(250.0),
            deducted_net_profit_margin: Some(12.0),
            deducted_net_profit_growth_rate: Some(15.0),
            ..StockItem::default()
        },
        StockItem {
            code: "002475.SZ".to_string(),
            name: "立讯精密".to_string(),
            industry: "电子制造".to_string(),
            is_st: false,
            price: 36.8,
            pe: Some(24.0),
            pb: Some(4.1),
            roe: Some(0.17),
            market_cap_billion: Some(260.0),
            dividend_yield: Some(0.008),
            deducted_net_profit_billion: Some(95.0),
            deducted_net_profit_margin: Some(11.0),
            deducted_net_profit_growth_rate: Some(11.0),
            ..StockItem::default()
        },
        StockItem {
            code: "600036.SH".to_string(),
            name: "招商银行".to_string(),
            industry: "银行".to_string(),
            is_st: false,
            price: 31.2,
            pe: Some(6.9),
            pb: Some(0.9),
            roe: Some(0.14),
            market_cap_billion: Some(900.0),
            dividend_yield: Some(0.04),
            deducted_net_profit_billion: Some(1300.0),
            deducted_net_profit_margin: Some(42.0),
            deducted_net_profit_growth_rate: Some(5.0),
            ..StockItem::default()
        },
        StockItem {
            code: "600000.SH".to_string(),
            name: "浦发银行".to_string(),
            industry: "银行".to_string(),
            is_st: false,
            price: 9.1,
            pe: Some(4.8),
            pb: Some(0.5),
            roe: Some(0.11),
            market_cap_billion: Some(170.0),
            dividend_yield: Some(0.06),
            deducted_net_profit_billion: Some(320.0),
            deducted_net_profit_margin: Some(36.0),
            deducted_net_profit_growth_rate: Some(4.0),
            ..StockItem::default()
        },
        StockItem {
            code: "601012.SH".to_string(),
            name: "隆基绿能".to_string(),
            industry: "光伏".to_string(),
            is_st: false,
            price: 18.6,
            pe: Some(14.2),
            pb: Some(1.8),
            roe: Some(0.13),
            market_cap_billion: Some(140.0),
            dividend_yield: Some(0.012),
            deducted_net_profit_billion: Some(90.0),
            deducted_net_profit_margin: Some(10.5),
            deducted_net_profit_growth_rate: Some(12.0),
            ..StockItem::default()
        },
        StockItem {
            code: "600309.SH".to_string(),
            name: "万华化学".to_string(),
            industry: "化工".to_string(),
            is_st: false,
            price: 78.4,
            pe: Some(15.6),
            pb: Some(2.6),
            roe: Some(0.19),
            market_cap_billion: Some(245.0),
            dividend_yield: Some(0.025),
            deducted_net_profit_billion: Some(160.0),
            deducted_net_profit_margin: Some(13.0),
            deducted_net_profit_growth_rate: Some(14.0),
            ..StockItem::default()
        },
        StockItem {
            code: "600887.SH".to_string(),
            name: "伊利股份".to_string(),
            industry: "食品饮料".to_string(),
            is_st: false,
            price: 28.5,
            pe: Some(18.2),
            pb: Some(3.1),
            roe: Some(0.16),
            market_cap_billion: Some(185.0),
            dividend_yield: Some(0.035),
            deducted_net_profit_billion: Some(100.0),
            deducted_net_profit_margin: Some(9.0),
            deducted_net_profit_growth_rate: Some(8.0),
            ..StockItem::default()
        },
    ]
}

pub fn mock_relations() -> Vec<StockRelation> {
    vec![
        StockRelation {
            source_code: "600036.SH".to_string(),
            target_code: "000001.SZ".to_string(),
            relation_type: "industry_peer".to_string(),
            weight: 0.75,
            description: Some("同受银行估值与信用周期影响。".to_string()),
        },
        StockRelation {
            source_code: "600036.SH".to_string(),
            target_code: "600000.SH".to_string(),
            relation_type: "industry_peer".to_string(),
            weight: 0.8,
            description: Some("A 股大型商业银行同业对比。".to_string()),
        },
        StockRelation {
            source_code: "000001.SZ".to_string(),
            target_code: "600000.SH".to_string(),
            relation_type: "industry_peer".to_string(),
            weight: 0.7,
            description: Some("区域银行与信用周期同业信号。".to_string()),
        },
        StockRelation {
            source_code: "300750.SZ".to_string(),
            target_code: "002594.SZ".to_string(),
            relation_type: "supply_chain".to_string(),
            weight: 0.65,
            description: Some("动力电池与新能源汽车需求链联动。".to_string()),
        },
        StockRelation {
            source_code: "300750.SZ".to_string(),
            target_code: "601012.SH".to_string(),
            relation_type: "thematic".to_string(),
            weight: 0.45,
            description: Some("清洁能源资本开支周期相关。".to_string()),
        },
        StockRelation {
            source_code: "002475.SZ".to_string(),
            target_code: "002594.SZ".to_string(),
            relation_type: "manufacturing_chain".to_string(),
            weight: 0.35,
            description: Some("先进制造与出口需求链联动。".to_string()),
        },
        StockRelation {
            source_code: "600309.SH".to_string(),
            target_code: "300750.SZ".to_string(),
            relation_type: "upstream_material".to_string(),
            weight: 0.3,
            description: Some("化工材料与新能源供应链上游相关。".to_string()),
        },
    ]
}

fn as_percent(value: f64) -> Option<f64> {
    if !value.is_finite() {
        return None;
    }
    Some(if (-1.0..=1.0).contains(&value) {
        value * 100.0
    } else {
        value
    })
}

fn matches_stock(stock: &StockItem, criteria: &ScreenCriteria) -> Option<Vec<String>> {
    let mut reasons = Vec::new();
    if !criteria.include_st && stock.is_st {
        return None;
    }

    if let Some(industry) = criteria.industry.as_ref() {
        let stock_value = stock.industry.trim().to_lowercase();
        let selected_value = industry.trim().to_lowercase();
        if !selected_value.is_empty()
            && (stock_value.is_empty()
                || (stock_value != selected_value
                    && !stock_value.contains(&selected_value)
                    && !selected_value.contains(&stock_value)))
        {
            return None;
        }
    }

    if let Some(min_roe) = criteria.min_roe {
        if stock.roe.map(|roe| roe < min_roe).unwrap_or(true) {
            return None;
        }
        reasons.push("roe_ok".to_string());
    }

    if let Some(max_pe) = criteria.max_pe {
        if stock.pe.map(|pe| pe > max_pe).unwrap_or(true) {
            return None;
        }
        reasons.push("pe_ok".to_string());
    }

    if let Some(max_pb) = criteria.max_pb {
        if stock.pb.map(|pb| pb > max_pb).unwrap_or(true) {
            return None;
        }
        reasons.push("pb_ok".to_string());
    }

    if let Some(min_market_cap) = criteria.min_market_cap_billion {
        if stock
            .market_cap_billion
            .map(|market_cap| market_cap < min_market_cap)
            .unwrap_or(true)
        {
            return None;
        }
        reasons.push("mcap_ok".to_string());
    }

    if let Some(min_profit) = criteria.min_deducted_net_profit_billion {
        if stock
            .deducted_net_profit_billion
            .map(|profit| profit <= min_profit)
            .unwrap_or(true)
        {
            return None;
        }
        reasons.push("deducted_net_profit_ok".to_string());
    }

    if let Some(min_margin) = criteria.min_deducted_net_profit_margin {
        if stock
            .deducted_net_profit_margin
            .and_then(as_percent)
            .map(|margin| margin <= min_margin)
            .unwrap_or(true)
        {
            return None;
        }
        reasons.push("deducted_net_profit_margin_ok".to_string());
    }

    if let Some(min_growth) = criteria.min_deducted_net_profit_growth_rate {
        if stock
            .deducted_net_profit_growth_rate
            .and_then(as_percent)
            .map(|growth| growth <= min_growth)
            .unwrap_or(true)
        {
            return None;
        }
        reasons.push("deducted_net_profit_growth_rate_ok".to_string());
    }

    Some(reasons)
}

const SCREEN_GROUP_LIMIT: usize = 10;
const POTENTIAL_SCORE_THRESHOLD: f64 = 10.0;
const SCREEN_SCORE_SCALE: f64 = 20.0;
const THEME_PROMOTION_ORDER: [&str; 7] = [
    "materials",
    "ai_chain",
    "semiconductor_wafer",
    "tech",
    "energy",
    "medical",
    "game",
];
const THEME_FILL_ORDER: [&str; 7] = [
    "ai_chain",
    "semiconductor_wafer",
    "materials",
    "tech",
    "energy",
    "medical",
    "game",
];
const THEME_RULES: [(&str, &str, f64, &[&str]); 7] = [
    (
        "materials",
        "\u{65b0}\u{6750}\u{6599}",
        0.96,
        &[
            "\u{6c1f}\u{5316}\u{5de5}",
            "\u{6c1f}\u{6750}\u{6599}",
            "\u{9502}\u{7535}\u{6750}\u{6599}",
            "\u{7535}\u{89e3}\u{6db2}",
            "\u{516d}\u{6c1f}\u{78f7}\u{9178}\u{9502}",
            "\u{65b0}\u{80fd}\u{6750}",
            "\u{65b0}\u{6750}\u{6599}",
            "\u{56fa}\u{6001}\u{7535}\u{6c60}",
            "\u{78c1}\u{6750}",
        ],
    ),
    (
        "semiconductor_wafer",
        "\u{534a}\u{5bfc}\u{4f53}\u{6676}\u{5706}",
        0.9,
        &[
            "\u{534a}\u{5bfc}\u{4f53}\u{6676}\u{5706}",
            "\u{6676}\u{5706}",
            "\u{6676}\u{5706}\u{4ee3}\u{5de5}",
            "\u{6676}\u{5706}\u{5236}\u{9020}",
            "\u{6676}\u{5706}\u{5382}",
            "\u{7845}\u{6676}\u{5706}",
            "\u{7845}\u{7247}",
            "\u{5916}\u{5ef6}\u{7247}",
            "\u{5916}\u{5ef6}\u{7845}\u{7247}",
            "\u{534a}\u{5bfc}\u{4f53}\u{886c}\u{5e95}",
            "\u{886c}\u{5e95}",
            "\u{78b3}\u{5316}\u{7845}\u{886c}\u{5e95}",
            "sic\u{886c}\u{5e95}",
            "\u{629b}\u{5149}\u{7247}",
            "8\u{82f1}\u{5bf8}",
            "12\u{82f1}\u{5bf8}",
        ],
    ),
    (
        "ai_chain",
        "AI\u{7b97}\u{529b}\u{4e0e}\u{82af}\u{7247}",
        0.95,
        &[
            "\u{534a}\u{5bfc}\u{4f53}",
            "\u{82af}\u{7247}",
            "\u{7b97}\u{529b}",
            "\u{4eba}\u{5de5}\u{667a}\u{80fd}",
            "ai",
            "\u{5149}\u{6a21}\u{5757}",
            "cpo",
            "\u{670d}\u{52a1}\u{5668}",
            "\u{6db2}\u{51b7}",
            "gpu",
            "hbm",
            "\u{5b58}\u{50a8}",
            "\u{6570}\u{636e}\u{4e2d}\u{5fc3}",
            "\u{4e91}\u{8ba1}\u{7b97}",
            "\u{5927}\u{6a21}\u{578b}",
            "aigc",
            "\u{8fb9}\u{7f18}\u{8ba1}\u{7b97}",
            "pcb",
            "\u{5c01}\u{88c5}",
            "\u{5c01}\u{6d4b}",
            "eda",
            "soc",
        ],
    ),
    (
        "tech",
        "\u{79d1}\u{6280}\u{5236}\u{9020}",
        0.84,
        &[
            "\u{673a}\u{5668}\u{4eba}",
            "\u{8f6f}\u{4ef6}",
            "\u{901a}\u{4fe1}",
            "\u{79d1}\u{6280}",
            "\u{7535}\u{5b50}",
            "\u{81ea}\u{52a8}\u{5316}",
            "\u{9ad8}\u{7aef}\u{5236}\u{9020}",
            "\u{667a}\u{80fd}\u{5236}\u{9020}",
        ],
    ),
    (
        "energy",
        "\u{65b0}\u{80fd}\u{6e90}",
        0.82,
        &[
            "\u{65b0}\u{80fd}\u{6e90}",
            "\u{7535}\u{6c60}",
            "\u{50a8}\u{80fd}",
            "\u{5149}\u{4f0f}",
            "\u{7535}\u{529b}",
            "\u{80fd}\u{6e90}",
            "\u{6cb9}\u{6c14}",
            "\u{7164}\u{70ad}",
            "\u{98ce}\u{7535}",
            "\u{5145}\u{7535}\u{6869}",
        ],
    ),
    (
        "medical",
        "\u{533b}\u{836f}\u{533b}\u{7597}",
        0.8,
        &[
            "\u{533b}\u{836f}",
            "\u{533b}\u{7597}",
            "\u{751f}\u{7269}\u{5236}\u{54c1}",
            "\u{521b}\u{65b0}\u{836f}",
            "\u{4e2d}\u{836f}",
            "\u{5316}\u{5b66}\u{5236}\u{836f}",
            "\u{533b}\u{7597}\u{5668}\u{68b0}",
            "cro",
            "cxo",
            "\u{75ab}\u{82d7}",
            "\u{533b}\u{7597}\u{670d}\u{52a1}",
            "\u{4eff}\u{5236}\u{836f}",
        ],
    ),
    (
        "game",
        "\u{6e38}\u{620f}\u{4f20}\u{5a92}",
        0.78,
        &[
            "\u{6e38}\u{620f}",
            "\u{7f51}\u{7edc}\u{6e38}\u{620f}",
            "\u{624b}\u{6e38}",
            "\u{7535}\u{7ade}",
            "\u{4e91}\u{6e38}\u{620f}",
            "\u{4e92}\u{52a8}\u{5a31}\u{4e50}",
            "\u{6587}\u{5316}\u{4f20}\u{5a92}",
            "\u{4f20}\u{5a92}",
        ],
    ),
];
const CONCEPT_GROUP_RULES: [(&str, &[&str]); 15] = [
    (
        "\u{534a}\u{5bfc}\u{4f53}\u{6676}\u{5706}",
        &[
            "\u{534a}\u{5bfc}\u{4f53}\u{6676}\u{5706}",
            "\u{6676}\u{5706}",
            "\u{6676}\u{5706}\u{4ee3}\u{5de5}",
            "\u{6676}\u{5706}\u{5236}\u{9020}",
            "\u{6676}\u{5706}\u{5382}",
            "\u{7845}\u{6676}\u{5706}",
            "\u{7845}\u{7247}",
            "\u{5916}\u{5ef6}\u{7247}",
            "\u{5916}\u{5ef6}\u{7845}\u{7247}",
            "\u{534a}\u{5bfc}\u{4f53}\u{886c}\u{5e95}",
            "\u{886c}\u{5e95}",
            "\u{78b3}\u{5316}\u{7845}\u{886c}\u{5e95}",
            "sic\u{886c}\u{5e95}",
            "\u{629b}\u{5149}\u{7247}",
            "8\u{82f1}\u{5bf8}",
            "12\u{82f1}\u{5bf8}",
        ],
    ),
    (
        "AI\u{7b97}\u{529b}\u{4e0e}\u{82af}\u{7247}",
        &[
            "\u{534a}\u{5bfc}\u{4f53}",
            "\u{82af}\u{7247}",
            "\u{7b97}\u{529b}",
            "\u{4eba}\u{5de5}\u{667a}\u{80fd}",
            "ai",
            "\u{5149}\u{6a21}\u{5757}",
            "cpo",
            "\u{670d}\u{52a1}\u{5668}",
            "\u{6db2}\u{51b7}",
            "gpu",
            "hbm",
            "\u{5b58}\u{50a8}",
            "\u{6570}\u{636e}\u{4e2d}\u{5fc3}",
            "\u{4e91}\u{8ba1}\u{7b97}",
            "\u{5927}\u{6a21}\u{578b}",
            "aigc",
            "\u{8fb9}\u{7f18}\u{8ba1}\u{7b97}",
            "pcb",
            "\u{5c01}\u{88c5}",
            "\u{5c01}\u{6d4b}",
            "eda",
            "soc",
        ],
    ),
    (
        "\u{65b0}\u{6750}\u{6599}",
        &[
            "\u{6c1f}\u{5316}\u{5de5}",
            "\u{6c1f}\u{6750}\u{6599}",
            "\u{9502}\u{7535}\u{6750}\u{6599}",
            "\u{7535}\u{89e3}\u{6db2}",
            "\u{516d}\u{6c1f}\u{78f7}\u{9178}\u{9502}",
            "\u{65b0}\u{80fd}\u{6750}",
            "\u{65b0}\u{6750}\u{6599}",
            "\u{56fa}\u{6001}\u{7535}\u{6c60}",
            "\u{78c1}\u{6750}",
        ],
    ),
    (
        "\u{65b0}\u{80fd}\u{6e90}\u{4e0e}\u{50a8}\u{80fd}",
        &[
            "\u{65b0}\u{80fd}\u{6e90}",
            "\u{7535}\u{6c60}",
            "\u{50a8}\u{80fd}",
            "\u{5149}\u{4f0f}",
            "\u{7535}\u{529b}",
            "\u{80fd}\u{6e90}",
            "\u{98ce}\u{7535}",
            "\u{5145}\u{7535}\u{6869}",
        ],
    ),
    (
        "\u{6e38}\u{620f}\u{4f20}\u{5a92}",
        &[
            "\u{6e38}\u{620f}",
            "\u{7f51}\u{7edc}\u{6e38}\u{620f}",
            "\u{624b}\u{6e38}",
            "\u{7535}\u{7ade}",
            "\u{4e91}\u{6e38}\u{620f}",
            "\u{4e92}\u{52a8}\u{5a31}\u{4e50}",
            "\u{4f20}\u{5a92}",
            "\u{5e7f}\u{544a}\u{8425}\u{9500}",
        ],
    ),
    (
        "\u{673a}\u{5668}\u{4eba}\u{4e0e}\u{9ad8}\u{7aef}\u{5236}\u{9020}",
        &[
            "\u{673a}\u{5668}\u{4eba}",
            "\u{5de5}\u{4e1a}\u{6bcd}\u{673a}",
            "\u{81ea}\u{52a8}\u{5316}",
            "\u{9ad8}\u{7aef}\u{5236}\u{9020}",
            "\u{667a}\u{80fd}\u{5236}\u{9020}",
            "\u{673a}\u{68b0}\u{8bbe}\u{5907}",
        ],
    ),
    (
        "\u{6d88}\u{8d39}\u{96f6}\u{552e}",
        &[
            "\u{98df}\u{54c1}",
            "\u{996e}\u{6599}",
            "\u{767d}\u{9152}",
            "\u{4f11}\u{95f2}\u{98df}\u{54c1}",
            "\u{4e00}\u{822c}\u{96f6}\u{552e}",
            "\u{5546}\u{8d38}\u{96f6}\u{552e}",
            "\u{5bb6}\u{7535}",
            "\u{65c5}\u{6e38}",
            "\u{9152}\u{5e97}",
            "\u{9910}\u{996e}",
        ],
    ),
    (
        "\u{533b}\u{836f}\u{533b}\u{7597}",
        &[
            "\u{533b}\u{836f}",
            "\u{533b}\u{7597}",
            "\u{751f}\u{7269}\u{5236}\u{54c1}",
            "\u{521b}\u{65b0}\u{836f}",
            "\u{4e2d}\u{836f}",
            "\u{5316}\u{5b66}\u{5236}\u{836f}",
            "\u{533b}\u{7597}\u{5668}\u{68b0}",
            "cro",
        ],
    ),
    (
        "\u{91d1}\u{878d}\u{5730}\u{4ea7}",
        &[
            "\u{94f6}\u{884c}",
            "\u{8bc1}\u{5238}",
            "\u{4fdd}\u{9669}",
            "\u{623f}\u{5730}\u{4ea7}",
            "\u{5730}\u{4ea7}",
            "\u{7269}\u{4e1a}",
        ],
    ),
    (
        "\u{57fa}\u{5efa}\u{5efa}\u{7b51}",
        &[
            "\u{5efa}\u{7b51}",
            "\u{623f}\u{5c4b}\u{5efa}\u{8bbe}",
            "\u{5de5}\u{7a0b}\u{5efa}\u{8bbe}",
            "\u{57fa}\u{7840}\u{5efa}\u{8bbe}",
            "\u{6c34}\u{6ce5}",
            "\u{94c1}\u{8def}",
            "\u{516c}\u{8def}",
            "\u{88c5}\u{4fee}\u{88c5}\u{9970}",
        ],
    ),
    (
        "\u{5468}\u{671f}\u{8d44}\u{6e90}",
        &[
            "\u{7164}\u{70ad}",
            "\u{94a2}\u{94c1}",
            "\u{666e}\u{94a2}",
            "\u{6709}\u{8272}",
            "\u{91d1}\u{5c5e}",
            "\u{5316}\u{5de5}",
            "\u{77f3}\u{6cb9}",
            "\u{6cb9}\u{6c14}",
            "\u{77ff}\u{4e1a}",
        ],
    ),
    (
        "\u{6c7d}\u{8f66}\u{4ea7}\u{4e1a}\u{94fe}",
        &[
            "\u{6c7d}\u{8f66}",
            "\u{6574}\u{8f66}",
            "\u{96f6}\u{90e8}\u{4ef6}",
            "\u{8f6e}\u{80ce}",
            "\u{667a}\u{80fd}\u{9a7e}\u{9a76}",
            "\u{65e0}\u{4eba}\u{9a7e}\u{9a76}",
            "\u{6c7d}\u{8f66}\u{670d}\u{52a1}",
        ],
    ),
    (
        "\u{519b}\u{5de5}\u{822a}\u{5929}",
        &[
            "\u{519b}\u{5de5}",
            "\u{822a}\u{5929}",
            "\u{822a}\u{7a7a}",
            "\u{536b}\u{661f}",
            "\u{8239}\u{8236}",
            "\u{65e0}\u{4eba}\u{673a}",
            "\u{56fd}\u{9632}",
        ],
    ),
    (
        "\u{4ea4}\u{8fd0}\u{7269}\u{6d41}",
        &[
            "\u{7269}\u{6d41}",
            "\u{822a}\u{8fd0}",
            "\u{6e2f}\u{53e3}",
            "\u{673a}\u{573a}",
            "\u{822a}\u{7a7a}\u{8fd0}\u{8f93}",
            "\u{94c1}\u{8def}\u{8fd0}\u{8f93}",
            "\u{5feb}\u{9012}",
        ],
    ),
    (
        "\u{516c}\u{7528}\u{73af}\u{4fdd}",
        &[
            "\u{73af}\u{4fdd}",
            "\u{6c34}\u{52a1}",
            "\u{71c3}\u{6c14}",
            "\u{4f9b}\u{70ed}",
            "\u{516c}\u{7528}\u{4e8b}\u{4e1a}",
        ],
    ),
];
const COLD_SECTOR_KEYWORDS: [&str; 9] = [
    "\u{94f6}\u{884c}",
    "\u{57fa}\u{5efa}",
    "\u{5efa}\u{7b51}",
    "\u{5efa}\u{7b51}\u{88c5}\u{9970}",
    "\u{5de5}\u{7a0b}\u{5efa}\u{8bbe}",
    "\u{57fa}\u{7840}\u{5efa}\u{8bbe}",
    "\u{6c34}\u{6ce5}",
    "\u{94c1}\u{8def}",
    "\u{516c}\u{8def}",
];

fn score_stock(stock: &StockItem, reasons: &[String], score_profile: &str) -> ScreenedStock {
    let theme = theme_match_for_stock(stock);
    let cold = is_cold_sector(&stock.industry);
    let mut factor_scores = BTreeMap::from([
        (
            "theme".to_string(),
            theme.as_ref().map(|(_, _, score)| *score).unwrap_or(0.35),
        ),
        ("fundamental".to_string(), fundamental_score(stock)),
        ("valuation".to_string(), valuation_score(stock)),
        ("size".to_string(), size_score(stock)),
        ("risk".to_string(), risk_score(stock, cold)),
    ]);
    if is_rotation_score_profile(score_profile) {
        factor_scores.insert("market_heat".to_string(), market_heat_score(stock));
    }
    let weighted = weighted_score(&factor_scores, score_profile);
    let score = (weighted * SCREEN_SCORE_SCALE).clamp(0.0, SCREEN_SCORE_SCALE);
    let mut all_reasons = reasons.to_vec();
    all_reasons.extend(factor_reasons(theme.as_ref(), cold, &factor_scores));
    let score_explanation = explain_score(stock, theme.as_ref(), cold, &factor_scores);
    let rounded_scores = factor_scores
        .into_iter()
        .map(|(key, value)| (key, (value * 10_000.0).round() / 10_000.0))
        .collect();
    ScreenedStock {
        stock: stock.clone(),
        score: (score * 1_000_000.0).round() / 1_000_000.0,
        reasons: all_reasons,
        factor_scores: rounded_scores,
        score_explanation,
        concept: Some(concept_group_for_stock(stock)),
        theme_category: theme.map(|(key, _, _)| key.to_string()),
    }
}
fn fundamental_score(stock: &StockItem) -> f64 {
    let roe_percent = stock.roe.and_then(as_percent);
    let roe_score = match roe_percent {
        None => 0.5,
        Some(value) if value >= 15.0 => 1.0,
        Some(value) if value >= 8.0 => 0.72 + (value - 8.0) / 7.0 * 0.18,
        Some(value) if value >= 0.0 => 0.42 + value / 8.0 * 0.25,
        Some(_) => 0.2,
    };
    let dividend = stock.dividend_yield.and_then(as_percent);
    let dividend_bonus = dividend
        .map(|value| value.max(0.0).min(6.0) / 6.0 * 0.12)
        .unwrap_or(0.0);
    let mut quality_bonus = 0.0;
    if stock
        .deducted_net_profit_billion
        .map(|value| value > 0.0)
        .unwrap_or(false)
    {
        quality_bonus += 0.08;
    }
    if stock
        .deducted_net_profit_growth_rate
        .and_then(as_percent)
        .map(|value| value >= 10.0)
        .unwrap_or(false)
    {
        quality_bonus += 0.08;
    }
    (roe_score + dividend_bonus + quality_bonus).clamp(0.0, 1.0)
}

fn valuation_score(stock: &StockItem) -> f64 {
    ((pe_score(stock.pe) + pb_score(stock.pb)) / 2.0).clamp(0.0, 1.0)
}

fn pe_score(value: Option<f64>) -> f64 {
    match value {
        None => 0.52,
        Some(value) if value <= 0.0 => 0.25,
        Some(value) if value <= 15.0 => 1.0,
        Some(value) if value <= 30.0 => 0.72,
        Some(value) if value <= 60.0 => 0.45,
        Some(_) => 0.28,
    }
}

fn pb_score(value: Option<f64>) -> f64 {
    match value {
        None => 0.52,
        Some(value) if value <= 0.0 => 0.25,
        Some(value) if value <= 1.5 => 1.0,
        Some(value) if value <= 3.0 => 0.74,
        Some(value) if value <= 6.0 => 0.5,
        Some(value) if value <= 10.0 => 0.34,
        Some(_) => 0.22,
    }
}

fn size_score(stock: &StockItem) -> f64 {
    match stock.market_cap_billion {
        None => 0.55,
        Some(value) if value < 20.0 => 0.36,
        Some(value) if value < 100.0 => 0.68,
        Some(value) if value < 500.0 => 0.88,
        Some(value) if value < 2000.0 => 0.78,
        Some(_) => 0.62,
    }
}

fn risk_score(stock: &StockItem, cold: bool) -> f64 {
    if stock.is_st {
        0.05
    } else if cold {
        0.36
    } else {
        1.0
    }
}

fn is_rotation_score_profile(score_profile: &str) -> bool {
    score_profile.trim().eq_ignore_ascii_case("rotation")
}

fn weighted_score(factor_scores: &BTreeMap<String, f64>, score_profile: &str) -> f64 {
    if is_rotation_score_profile(score_profile) {
        factor_scores.get("theme").copied().unwrap_or(0.35) * 0.20
            + factor_scores.get("fundamental").copied().unwrap_or(0.5) * 0.20
            + factor_scores.get("valuation").copied().unwrap_or(0.5) * 0.20
            + factor_scores.get("market_heat").copied().unwrap_or(0.5) * 0.22
            + factor_scores.get("size").copied().unwrap_or(0.55) * 0.08
            + factor_scores.get("risk").copied().unwrap_or(1.0) * 0.10
    } else {
        factor_scores.get("theme").copied().unwrap_or(0.35) * 0.24
            + factor_scores.get("fundamental").copied().unwrap_or(0.5) * 0.24
            + factor_scores.get("valuation").copied().unwrap_or(0.5) * 0.24
            + factor_scores.get("size").copied().unwrap_or(0.55) * 0.14
            + factor_scores.get("risk").copied().unwrap_or(1.0) * 0.14
    }
}

fn market_heat_score(stock: &StockItem) -> f64 {
    let change_pct = stock.change_pct.and_then(as_percent).unwrap_or(0.0);
    let positive_momentum = if change_pct >= 0.0 {
        (change_pct.min(10.0) / 10.0).clamp(0.0, 1.0)
    } else {
        (0.45 + change_pct.max(-10.0) / 20.0).clamp(0.0, 0.45)
    };
    let volume_ratio = stock
        .volume_ratio
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|value| ((value.min(3.0) - 1.0).max(0.0) / 2.0).clamp(0.0, 1.0))
        .unwrap_or(0.45);
    let turnover = stock
        .turnover_rate
        .and_then(as_percent)
        .map(|value| (value.max(0.0).min(8.0) / 8.0).clamp(0.0, 1.0))
        .unwrap_or(0.45);
    let amount = stock
        .amount
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|value| ((value / 1_000_000_000.0).min(1.0)).clamp(0.0, 1.0))
        .unwrap_or(0.45);
    (positive_momentum * 0.46 + volume_ratio * 0.24 + turnover * 0.18 + amount * 0.12)
        .clamp(0.0, 1.0)
}

fn factor_reasons(
    theme: Option<&(&'static str, &'static str, f64)>,
    cold: bool,
    factor_scores: &BTreeMap<String, f64>,
) -> Vec<String> {
    let mut reasons = Vec::new();
    if let Some((_, label, _)) = theme {
        reasons.push(format!("{}{label}", "\u{4e3b}\u{9898}:"));
    }
    if factor_scores.get("valuation").copied().unwrap_or(0.0) >= 0.74 {
        reasons.push("\u{4f30}\u{503c}\u{8f83}\u{4f4e}".to_string());
    } else if factor_scores.get("valuation").copied().unwrap_or(0.0) <= 0.38 {
        reasons.push("\u{4f30}\u{503c}\u{504f}\u{9ad8}".to_string());
    }
    if factor_scores.get("fundamental").copied().unwrap_or(0.0) >= 0.72 {
        reasons.push("\u{57fa}\u{672c}\u{9762}\u{8f83}\u{5f3a}".to_string());
    }
    if factor_scores.get("market_heat").copied().unwrap_or(0.0) >= 0.72 {
        reasons.push("\u{8f6e}\u{52a8}\u{70ed}\u{5ea6}\u{8f83}\u{9ad8}".to_string());
    }
    if cold {
        reasons.push("\u{4f4e}\u{70ed}\u{5ea6}\u{964d}\u{6743}".to_string());
    }
    reasons
}

fn explain_score(
    stock: &StockItem,
    theme: Option<&(&'static str, &'static str, f64)>,
    cold: bool,
    factor_scores: &BTreeMap<String, f64>,
) -> String {
    let mut parts = Vec::new();
    if let Some((_, label, _)) = theme {
        parts.push(format!("{}{label}", "\u{4e3b}\u{9898}\u{547d}\u{4e2d}"));
    } else {
        parts.push("\u{4e3b}\u{9898}\u{70ed}\u{5ea6}\u{4e00}\u{822c}".to_string());
    }
    parts.push(format!(
        "{}{}",
        "\u{4f30}\u{503c}",
        tier_word(factor_scores.get("valuation").copied().unwrap_or(0.0))
    ));
    parts.push(format!(
        "{}{}",
        "\u{57fa}\u{672c}\u{9762}",
        tier_word(factor_scores.get("fundamental").copied().unwrap_or(0.0))
    ));
    if stock.market_cap_billion.is_none() {
        parts.push(
            "\u{5e02}\u{503c}\u{7f3a}\u{5931}\u{6309}\u{4e2d}\u{6027}\u{5904}\u{7406}".to_string(),
        );
    } else {
        parts.push(format!(
            "{}{}",
            "\u{5e02}\u{503c}\u{89c4}\u{6a21}",
            tier_word(factor_scores.get("size").copied().unwrap_or(0.0))
        ));
    }
    if let Some(market_heat) = factor_scores.get("market_heat") {
        parts.push(format!(
            "{}{}",
            "\u{8f6e}\u{52a8}\u{70ed}\u{5ea6}",
            tier_word(*market_heat)
        ));
    }
    parts.push(if cold { "\u{94f6}\u{884c}/\u{57fa}\u{5efa}\u{7b49}\u{4f4e}\u{70ed}\u{5ea6}\u{65b9}\u{5411}\u{5df2}\u{964d}\u{6743}".to_string() } else { "\u{98ce}\u{9669}\u{60e9}\u{7f5a}\u{4f4e}".to_string() });
    format!("{}{}", parts.join("\u{ff1b}"), "\u{3002}")
}

fn tier_word(value: f64) -> &'static str {
    if value >= 0.72 {
        "\u{5f3a}"
    } else if value >= 0.5 {
        "\u{4e2d}\u{6027}"
    } else {
        "\u{504f}\u{5f31}"
    }
}

fn concept_group_for_stock(stock: &StockItem) -> String {
    let text = stock_text(stock);
    for (label, keywords) in CONCEPT_GROUP_RULES {
        if contains_any(&text, keywords) {
            return label.to_string();
        }
    }
    "\u{5176}\u{4ed6}\u{6982}\u{5ff5}".to_string()
}

fn concept_rank(concept: &str) -> usize {
    CONCEPT_GROUP_RULES
        .iter()
        .position(|(label, _)| *label == concept)
        .unwrap_or(CONCEPT_GROUP_RULES.len())
}

fn board_group_for_stock(stock: &StockItem) -> &'static str {
    let code = stock.code.trim().to_ascii_uppercase();
    let digits = code.split('.').next().unwrap_or("");
    if code.ends_with(".BJ")
        || digits.starts_with('8')
        || digits.starts_with('4')
        || digits.starts_with('9')
    {
        "\u{5317}\u{4ea4}\u{6240}"
    } else if digits.starts_with("688") {
        "\u{79d1}\u{521b}\u{677f}"
    } else if digits.starts_with("300") || digits.starts_with("301") {
        "\u{521b}\u{4e1a}\u{677f}"
    } else if code.ends_with(".SH") || digits.starts_with('6') {
        "\u{6caa}\u{4e3b}\u{677f}"
    } else {
        "\u{6df1}\u{4e3b}\u{677f}"
    }
}

fn board_rank(board: &str) -> usize {
    match board {
        "\u{79d1}\u{521b}\u{677f}" => 0,
        "\u{521b}\u{4e1a}\u{677f}" => 1,
        "\u{5317}\u{4ea4}\u{6240}" => 2,
        "\u{6caa}\u{4e3b}\u{677f}" => 3,
        "\u{6df1}\u{4e3b}\u{677f}" => 4,
        _ => 99,
    }
}

fn theme_match_for_stock(stock: &StockItem) -> Option<(&'static str, &'static str, f64)> {
    theme_match_for_text(&stock_text(stock))
}

fn theme_match_for_text(text: &str) -> Option<(&'static str, &'static str, f64)> {
    for (key, label, score, keywords) in THEME_RULES {
        if contains_any(text, keywords) {
            return Some((key, label, score));
        }
    }
    None
}

fn stock_text(stock: &StockItem) -> String {
    format!("{} {}", stock.name, stock.industry).to_lowercase()
}

fn hot_sector_category(industry: &str) -> Option<&'static str> {
    theme_match_for_text(&industry.to_lowercase()).map(|(key, _, _)| key)
}

fn is_cold_sector(industry: &str) -> bool {
    let normalized = industry.trim().to_lowercase();
    if normalized.is_empty() {
        return false;
    }
    COLD_SECTOR_KEYWORDS
        .iter()
        .any(|keyword| normalized.contains(keyword))
}

fn hot_pick_category(stock: &StockItem) -> Option<&'static str> {
    hot_sector_category(&format!("{} {}", stock.name, stock.industry))
}

fn should_promote_hot_sectors(criteria: &ScreenCriteria) -> bool {
    criteria.industry.as_deref().unwrap_or("").trim().is_empty()
        && criteria.sort_by.trim().eq_ignore_ascii_case("score")
        && !criteria.sort_dir.trim().eq_ignore_ascii_case("asc")
}

fn promote_hot_sector_items(
    screened: &[ScreenedStock],
    criteria: &ScreenCriteria,
    limit: usize,
) -> Vec<ScreenedStock> {
    if !should_promote_hot_sectors(criteria) || limit == 0 {
        return screened.iter().take(limit).cloned().collect();
    }

    let mut promoted = Vec::new();
    let mut used_codes = HashSet::new();
    for category in THEME_PROMOTION_ORDER {
        if let Some(candidate) = screened.iter().find(|item| {
            !used_codes.contains(&item.stock.code)
                && item.theme_category.as_deref() == Some(category)
        }) {
            promoted.push(candidate.clone());
            used_codes.insert(candidate.stock.code.clone());
            if promoted.len() >= limit {
                return promoted;
            }
        }
    }

    for category in THEME_FILL_ORDER {
        for item in screened {
            if used_codes.contains(&item.stock.code)
                || item.theme_category.as_deref() != Some(category)
            {
                continue;
            }
            promoted.push(item.clone());
            used_codes.insert(item.stock.code.clone());
            if promoted.len() >= limit {
                return promoted;
            }
        }
    }

    for item in screened {
        if used_codes.contains(&item.stock.code) || is_cold_sector(&item.stock.industry) {
            continue;
        }
        promoted.push(item.clone());
        used_codes.insert(item.stock.code.clone());
        if promoted.len() >= limit {
            return promoted;
        }
    }

    for item in screened {
        if used_codes.contains(&item.stock.code) {
            continue;
        }
        promoted.push(item.clone());
        used_codes.insert(item.stock.code.clone());
        if promoted.len() >= limit {
            break;
        }
    }
    promoted
}

fn sort_screened(screened: &mut [ScreenedStock], criteria: &ScreenCriteria) {
    let sort_by = criteria.sort_by.trim().to_ascii_lowercase();
    let ascending = criteria.sort_dir.trim().eq_ignore_ascii_case("asc");
    screened.sort_by(|left, right| {
        let ordering = match (sort_value(left, &sort_by), sort_value(right, &sort_by)) {
            (Some(left_value), Some(right_value)) => {
                if ascending {
                    left_value.total_cmp(&right_value)
                } else {
                    right_value.total_cmp(&left_value)
                }
            }
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        };
        ordering
            .then_with(|| right.score.total_cmp(&left.score))
            .then_with(|| left.stock.code.cmp(&right.stock.code))
    });
}

fn primary_screen_items(
    screened: &[ScreenedStock],
    criteria: &ScreenCriteria,
    limit: usize,
) -> (Vec<ScreenedStock>, bool) {
    if !should_promote_hot_sectors(criteria) || limit == 0 {
        let items = screened.iter().take(limit).cloned().collect::<Vec<_>>();
        return (items, false);
    }
    let promoted = promote_hot_sector_items(screened, criteria, limit);
    let promoted_flag = promoted
        .iter()
        .map(|item| item.stock.code.as_str())
        .collect::<Vec<_>>()
        != screened
            .iter()
            .take(limit)
            .map(|item| item.stock.code.as_str())
            .collect::<Vec<_>>();
    (promoted, promoted_flag)
}

fn screen_result_groups(screened: &[ScreenedStock]) -> Vec<ScreenResultGroup> {
    let hot_items = hot_group_items(screened, SCREEN_GROUP_LIMIT);
    let hot_codes: HashSet<String> = hot_items
        .iter()
        .map(|item| item.stock.code.clone())
        .collect();
    let potential_items = potential_group_items(
        screened,
        SCREEN_GROUP_LIMIT,
        &hot_codes,
        POTENTIAL_SCORE_THRESHOLD,
    );
    vec![
        ScreenResultGroup {
            key: "hot".to_string(),
            title: "\u{70ed}\u{95e8}\u{80a1}".to_string(),
            description: "AI\u{4e0a}\u{4e0b}\u{6e38}\u{3001}\u{82af}\u{7247}\u{3001}\u{7b97}\u{529b}\u{3001}\u{80fd}\u{6e90}\u{3001}\u{65b0}\u{6750}\u{6599}\u{3001}\u{533b}\u{836f}\u{3001}\u{6e38}\u{620f}\u{7b49}\u{70ed}\u{95e8}\u{65b9}\u{5411}\u{ff0c}\u{6309}\u{7efc}\u{5408}\u{5206}\u{548c}\u{4e3b}\u{9898}\u{4f18}\u{5148}\u{7ea7}\u{5c55}\u{793a}\u{3002}".to_string(),
            total: screened.iter().filter(|item| item.theme_category.is_some()).count(),
            returned: hot_items.len(),
            items: hot_items,
        },
        ScreenResultGroup {
            key: "potential".to_string(),
            title: "\u{6f5c}\u{529b}\u{80a1}".to_string(),
            description: format!("{}{}{}", "\u{7efc}\u{5408}\u{5206}\u{5927}\u{4e8e} ", POTENTIAL_SCORE_THRESHOLD, " \u{7684}\u{5019}\u{9009}\u{ff0c}\u{5df2}\u{5c3d}\u{91cf}\u{907f}\u{514d}\u{4e0e}\u{70ed}\u{95e8}\u{80a1}\u{91cd}\u{590d}\u{3002}"),
            total: screened.iter().filter(|item| item.score > POTENTIAL_SCORE_THRESHOLD).count(),
            returned: potential_items.len(),
            items: potential_items,
        },
    ]
}

fn hot_group_items(screened: &[ScreenedStock], limit: usize) -> Vec<ScreenedStock> {
    let mut selected = Vec::new();
    let mut used_codes = HashSet::new();
    let mut by_score = screened.to_vec();
    by_score.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.stock.code.cmp(&right.stock.code))
    });
    for category in THEME_PROMOTION_ORDER {
        if let Some(candidate) = first_by_theme(&by_score, category, &used_codes) {
            append_selected(&mut selected, &mut used_codes, candidate);
            if selected.len() >= limit {
                return selected;
            }
        }
    }
    for category in THEME_FILL_ORDER {
        for item in &by_score {
            if used_codes.contains(&item.stock.code)
                || item.theme_category.as_deref() != Some(category)
            {
                continue;
            }
            append_selected(&mut selected, &mut used_codes, item.clone());
            if selected.len() >= limit {
                return selected;
            }
        }
    }
    selected
}

fn potential_group_items(
    screened: &[ScreenedStock],
    limit: usize,
    exclude_codes: &HashSet<String>,
    threshold: f64,
) -> Vec<ScreenedStock> {
    let mut ranked = screened.to_vec();
    ranked.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.stock.code.cmp(&right.stock.code))
    });
    let mut selected = Vec::new();
    for item in ranked {
        if exclude_codes.contains(&item.stock.code) || item.score <= threshold {
            continue;
        }
        selected.push(item);
        if selected.len() >= limit {
            break;
        }
    }
    selected
}

fn first_by_theme(
    screened: &[ScreenedStock],
    category: &str,
    used_codes: &HashSet<String>,
) -> Option<ScreenedStock> {
    screened
        .iter()
        .find(|item| {
            !used_codes.contains(&item.stock.code)
                && item.theme_category.as_deref() == Some(category)
        })
        .cloned()
}

fn append_selected(
    selected: &mut Vec<ScreenedStock>,
    used_codes: &mut HashSet<String>,
    item: ScreenedStock,
) {
    used_codes.insert(item.stock.code.clone());
    selected.push(item);
}

fn sort_value(item: &ScreenedStock, sort_by: &str) -> Option<f64> {
    match sort_by {
        "price" => Some(item.stock.price),
        "pe" => item.stock.pe,
        "pb" => item.stock.pb,
        _ => Some(item.score),
    }
}

fn screen_candidate_pool(
    universe: &[StockItem],
    criteria: &ScreenCriteria,
    requested_limit: usize,
) -> Vec<ScreenedStock> {
    let pool_size = 200.min((requested_limit * 5).max(criteria.limit).max(50));
    let mut expanded_criteria = criteria.clone();
    expanded_criteria.limit = pool_size;
    screen_stocks(universe, &expanded_criteria).items
}

fn resolve_center_context(
    candidate_pool: &[ScreenedStock],
    seed_codes: &[String],
) -> GraphCenterContext {
    let normalized_seed_codes: Vec<String> = seed_codes
        .iter()
        .map(|code| normalize_stock_code(code))
        .filter(|code| !code.is_empty())
        .take(50)
        .collect();
    if !normalized_seed_codes.is_empty() {
        return GraphCenterContext {
            mode: "seed_codes".to_string(),
            label: "\u{79cd}\u{5b50}\u{80a1}\u{4e2d}\u{5fc3}".to_string(),
            codes: normalized_seed_codes,
        };
    }

    if candidate_pool.is_empty() {
        return GraphCenterContext {
            mode: "theme_center".to_string(),
            label: "\u{4e3b}\u{9898}\u{4e2d}\u{5fc3}\u{4e3a}\u{7a7a}".to_string(),
            codes: Vec::new(),
        };
    }

    let mut groups: HashMap<String, Vec<ScreenedStock>> = HashMap::new();
    let mut labels: HashMap<String, String> = HashMap::new();
    for item in candidate_pool {
        let (key, label) = center_group_key(item);
        groups.entry(key.clone()).or_default().push(item.clone());
        labels.insert(key, label);
    }

    let mut ranked: Vec<(String, Vec<ScreenedStock>)> = groups.into_iter().collect();
    ranked.sort_by(|(left_key, left_items), (right_key, right_items)| {
        let left_avg = average_screen_score(left_items);
        let right_avg = average_screen_score(right_items);
        right_items
            .len()
            .cmp(&left_items.len())
            .then_with(|| right_avg.total_cmp(&left_avg))
            .then_with(|| labels.get(right_key).cmp(&labels.get(left_key)))
    });
    let Some((selected_key, mut selected_items)) = ranked.into_iter().next() else {
        return GraphCenterContext::default();
    };
    selected_items.sort_by(|left, right| right.score.total_cmp(&left.score));
    selected_items.truncate(5);
    GraphCenterContext {
        mode: "theme_center".to_string(),
        label: labels.get(&selected_key).cloned().unwrap_or(selected_key),
        codes: selected_items
            .into_iter()
            .map(|item| item.stock.code)
            .collect(),
    }
}

fn average_screen_score(items: &[ScreenedStock]) -> f64 {
    if items.is_empty() {
        0.0
    } else {
        items.iter().map(|item| item.score).sum::<f64>() / items.len() as f64
    }
}

fn center_group_key(item: &ScreenedStock) -> (String, String) {
    if let Some(category) = hot_pick_category(&item.stock) {
        return (
            format!("theme:{category}"),
            format!("\u{4e3b}\u{9898}\u{4e2d}\u{5fc3}\u{ff1a}{category}"),
        );
    }
    if !item.stock.industry.trim().is_empty() {
        return (
            format!("industry:{}", item.stock.industry),
            format!("industry center: {}", item.stock.industry),
        );
    }
    (
        "industry:unknown".to_string(),
        "industry center: unknown".to_string(),
    )
}
fn merge_relations(relations: &[StockRelation]) -> Vec<StockRelation> {
    let mut merged: HashMap<(String, String, String), StockRelation> = HashMap::new();
    for relation in relations {
        if relation.source_code == relation.target_code {
            continue;
        }
        let (source, target) = sorted_pair(&relation.source_code, &relation.target_code);
        let key = (
            source.clone(),
            target.clone(),
            relation.relation_type.clone(),
        );
        let should_replace = merged
            .get(&key)
            .map(|existing| relation.weight > existing.weight)
            .unwrap_or(true);
        if should_replace {
            let mut normalized = relation.clone();
            normalized.source_code = source;
            normalized.target_code = target;
            merged.insert(key, normalized);
        }
    }
    merged.into_values().collect()
}

fn infer_industry_relations(universe: &[StockItem]) -> Vec<StockRelation> {
    let mut by_industry: HashMap<String, Vec<StockItem>> = HashMap::new();
    for stock in universe {
        if !stock.industry.is_empty() && stock.industry != "未知行业" {
            by_industry
                .entry(stock.industry.clone())
                .or_default()
                .push(stock.clone());
        }
    }

    let mut relations = Vec::new();
    for (industry, mut members) in by_industry {
        members.sort_by(|left, right| {
            right
                .market_cap_billion
                .unwrap_or(0.0)
                .total_cmp(&left.market_cap_billion.unwrap_or(0.0))
        });
        members.truncate(12);
        for index in 0..members.len() {
            let end = (index + 4).min(members.len());
            for target in &members[index + 1..end] {
                relations.push(StockRelation {
                    source_code: members[index].code.clone(),
                    target_code: target.code.clone(),
                    relation_type: "industry_peer".to_string(),
                    weight: 0.45,
                    description: Some(format!("同行业：{industry}")),
                });
            }
        }
    }
    relations
}

fn build_adjacency(
    relations: &[StockRelation],
) -> HashMap<String, Vec<(String, f64, StockRelation)>> {
    let mut adjacency: HashMap<String, Vec<(String, f64, StockRelation)>> = HashMap::new();
    for relation in relations {
        adjacency
            .entry(relation.source_code.clone())
            .or_default()
            .push((
                relation.target_code.clone(),
                relation.weight,
                relation.clone(),
            ));
        adjacency
            .entry(relation.target_code.clone())
            .or_default()
            .push((
                relation.source_code.clone(),
                relation.weight,
                relation.clone(),
            ));
    }
    adjacency
}

fn normalize_scores(scores: &HashMap<String, f64>) -> HashMap<String, f64> {
    if scores.is_empty() {
        return HashMap::new();
    }
    let min_score = scores.values().copied().fold(f64::INFINITY, f64::min);
    let max_score = scores.values().copied().fold(f64::NEG_INFINITY, f64::max);
    if max_score == min_score {
        return scores.keys().map(|code| (code.clone(), 1.0)).collect();
    }
    scores
        .iter()
        .map(|(code, score)| (code.clone(), (score - min_score) / (max_score - min_score)))
        .collect()
}

fn relation_score(
    code: &str,
    base_scores: &HashMap<String, f64>,
    adjacency: &HashMap<String, Vec<(String, f64, StockRelation)>>,
    seed_codes: &HashSet<String>,
    max_depth: usize,
) -> f64 {
    if !adjacency.contains_key(code) {
        return 0.0;
    }

    let mut weighted_sum = 0.0;
    let mut total_weight = 0.0;
    let mut queue = VecDeque::from([(code.to_string(), 0_usize, 1.0_f64)]);
    let mut visited = HashSet::from([code.to_string()]);

    while let Some((current, depth, path_weight)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }
        for (neighbor, edge_weight, _relation) in adjacency.get(&current).into_iter().flatten() {
            if visited.contains(neighbor) {
                continue;
            }
            visited.insert(neighbor.clone());
            let propagated_weight = path_weight * edge_weight;
            if let Some(base_score) = base_scores.get(neighbor) {
                let distance_discount = 1.0 / (depth + 1) as f64;
                weighted_sum += base_score * propagated_weight * distance_discount;
                total_weight += propagated_weight * distance_discount;
            }
            queue.push_back((neighbor.clone(), depth + 1, propagated_weight));
        }
    }

    let mut score = if total_weight > 0.0 {
        weighted_sum / total_weight
    } else {
        0.0
    };
    if !seed_codes.is_empty() {
        let proximity = seed_proximity(code, seed_codes, adjacency, max_depth);
        score = 1.0_f64.min(score + proximity * 0.25);
    }
    score
}

fn seed_proximity(
    code: &str,
    seed_codes: &HashSet<String>,
    adjacency: &HashMap<String, Vec<(String, f64, StockRelation)>>,
    max_depth: usize,
) -> f64 {
    if seed_codes.contains(code) {
        return 1.0;
    }

    let mut queue = VecDeque::from([(code.to_string(), 0_usize)]);
    let mut visited = HashSet::from([code.to_string()]);
    while let Some((current, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }
        for (neighbor, _edge_weight, _relation) in adjacency.get(&current).into_iter().flatten() {
            if visited.contains(neighbor) {
                continue;
            }
            if seed_codes.contains(neighbor) {
                return 0.0_f64.max((max_depth - depth) as f64 / max_depth as f64);
            }
            visited.insert(neighbor.clone());
            queue.push_back((neighbor.clone(), depth + 1));
        }
    }
    0.0
}

fn top_related(
    code: &str,
    relations: &[StockRelation],
    all_stocks: &HashMap<String, StockItem>,
    limit: usize,
) -> Vec<StockRelation> {
    let mut connected: Vec<StockRelation> = relations
        .iter()
        .filter(|relation| relation.source_code == code || relation.target_code == code)
        .cloned()
        .collect();
    connected.sort_by(|left, right| right.weight.total_cmp(&left.weight));
    connected
        .into_iter()
        .take(limit)
        .map(|relation| relation_with_names(relation, all_stocks))
        .collect()
}

fn relation_with_names(
    mut relation: StockRelation,
    all_stocks: &HashMap<String, StockItem>,
) -> StockRelation {
    let Some(source) = all_stocks.get(&relation.source_code) else {
        return relation;
    };
    let Some(target) = all_stocks.get(&relation.target_code) else {
        return relation;
    };
    let description = relation.description.clone().unwrap_or_default();
    relation.description = Some(
        format!("{} <-> {}. {}", source.name, target.name, description)
            .trim()
            .to_string(),
    );
    relation
}

fn graph_reasons(screened: &ScreenedStock, relation_score: f64) -> Vec<String> {
    let mut reasons = screened.reasons.clone();
    if relation_score >= 0.65 {
        reasons.push("strong_relation_signal".to_string());
    } else if relation_score >= 0.35 {
        reasons.push("moderate_relation_signal".to_string());
    }
    reasons
}

fn graph_explanation(
    screened: &ScreenedStock,
    base_score: f64,
    relation_score: f64,
    final_score: f64,
    relation_weight: f64,
    center_context: &GraphCenterContext,
    related: &[StockRelation],
) -> SelectionExplanation {
    let center_codes = if center_context.codes.is_empty() {
        "none".to_string()
    } else {
        center_context.codes.join(", ")
    };
    let mut basis = vec![
        format!(
            "Passed the current base screen with raw score {:.2}.",
            screened.score
        ),
        format!(
            "Relation propagation center: {}; center codes: {}.",
            center_context.label, center_codes
        ),
    ];
    if let Some(strongest) = related.first() {
        basis.push(format!(
            "Strongest displayed edge is {} with weight {:.2}.",
            strongest.relation_type, strongest.weight
        ));
    } else {
        basis.push("No direct relation edge is displayed; ranking relies on base score and graph propagation score.".to_string());
    }

    let base_component = (1.0 - relation_weight) * base_score;
    let relation_component = relation_weight * relation_score;
    let mut risk_checks = Vec::new();
    if relation_score >= 0.65 {
        risk_checks.push("Relation signal is strong; verify that the relation is still supported by recent business evidence.".to_string());
    } else if relation_score >= 0.35 {
        risk_checks.push("Relation signal is moderate; use it as candidate expansion rather than a standalone buy reason.".to_string());
    } else {
        risk_checks.push(
            "Relation signal is weak; selection is driven more by the base screen score."
                .to_string(),
        );
    }
    if related.is_empty() {
        risk_checks.push("No direct relation edge is available; refresh or enrich relation data before relying on graph evidence.".to_string());
    }

    SelectionExplanation {
        basis,
        score_breakdown: vec![
            score_contribution(
                "base_score",
                "Base score",
                base_score,
                base_component,
                0.7,
                0.35,
            ),
            score_contribution(
                "relation_score",
                "Relation score",
                relation_score,
                relation_component,
                0.65,
                0.35,
            ),
            score_contribution(
                "final_score",
                "Final score",
                final_score,
                final_score,
                0.65,
                0.35,
            ),
        ],
        risk_checks,
        verification: vec![
            "Cross-check with trend timing for short-buy or exit signals.".to_string(),
            "Open the related edges and verify peer, supply-chain, or theme evidence.".to_string(),
        ],
    }
}

fn score_contribution(
    key: &str,
    label: &str,
    value: f64,
    contribution: f64,
    strong_threshold: f64,
    watch_threshold: f64,
) -> ScoreContribution {
    ScoreContribution {
        key: key.to_string(),
        label: label.to_string(),
        value: Some(round6(value)),
        contribution: Some(round6(contribution)),
        tone: if value >= strong_threshold {
            "strong"
        } else if value >= watch_threshold {
            "watch"
        } else {
            "weak"
        }
        .to_string(),
    }
}
fn assign_weights(signals: &mut [GraphStockSignal]) {
    if signals.is_empty() {
        return;
    }

    let positive_sum: f64 = signals.iter().map(|item| item.final_score.max(0.0)).sum();
    if positive_sum <= 0.0 {
        let equal_weight = round6(1.0 / signals.len() as f64);
        for item in signals {
            item.suggested_weight = equal_weight;
        }
        return;
    }

    for item in signals {
        item.suggested_weight = round6(item.final_score.max(0.0) / positive_sum);
    }
}

fn prepare_bars(history: &[HistoryBar], stock: &StockItem) -> CoreResult<Vec<PreparedBar>> {
    let fallback_capital = stock
        .market_cap_billion
        .map(|market_cap| market_cap * 1_000_000_000.0 / stock.price.max(0.01))
        .unwrap_or_else(|| {
            let max_volume = history
                .iter()
                .filter_map(|bar| bar.volume)
                .fold(0.0_f64, f64::max);
            (max_volume * 20.0).max(1.0)
        });
    let mut bars = Vec::with_capacity(history.len());
    for bar in history {
        let date = parse_date(&bar.date)?;
        let open = bar.open.unwrap_or(bar.close);
        let high = bar.high.unwrap_or(open.max(bar.close));
        let low = bar.low.unwrap_or(open.min(bar.close));
        bars.push(PreparedBar {
            date,
            open,
            high,
            low,
            close: bar.close,
            volume: bar.volume.unwrap_or(0.0).max(0.0),
            capital: bar.capital.unwrap_or(fallback_capital).max(1.0),
        });
    }
    bars.sort_by(|left, right| left.date.cmp(&right.date));
    Ok(bars)
}

fn compute_trend_bars(bars: &[PreparedBar]) -> Vec<ComputedTrendBar> {
    let close: Vec<f64> = bars.iter().map(|bar| bar.close).collect();
    let open: Vec<f64> = bars.iter().map(|bar| bar.open).collect();
    let high: Vec<f64> = bars.iter().map(|bar| bar.high).collect();
    let low: Vec<f64> = bars.iter().map(|bar| bar.low).collect();
    let volume: Vec<f64> = bars.iter().map(|bar| bar.volume).collect();
    let capital: Vec<f64> = bars.iter().map(|bar| bar.capital).collect();

    let ema10 = ema(&close, 10);
    let ema20 = ema(&close, 20);
    let swl: Vec<f64> = ema10
        .iter()
        .zip(&ema20)
        .map(|(left, right)| (*left * 7.0 + *right * 3.0) / 10.0)
        .collect();
    let vol_sum_5 = rolling_sum(&volume, 5);
    let alpha: Vec<f64> = vol_sum_5
        .iter()
        .zip(&capital)
        .map(|(vol_sum, capital)| {
            ((100.0 * *vol_sum / (3.0 * *capital)).max(1.0) / 100.0).clamp(0.01, 1.0)
        })
        .collect();
    let sws = dma(&ema20, &alpha);

    let var1 = primary_state(&close, true);
    let vard = primary_state(&close, false);
    let red_states = alternating_states(&var1, &close, true);
    let cyan_states = alternating_states(&vard, &close, false);
    let red_hold = or_states(&red_states);
    let cyan_watch = or_states(&cyan_states);

    let ma34 = ma(&close, 34);
    let ma3 = ma(&close, 3);
    let bull_line = ma(&close, 26);
    let star_line = star_line(&close, &open, &high, &low);
    let (k, d, j) = kdj(&close, &high, &low);
    let quant_score = quant_scores(&close, &high, &low, &volume);
    let capital_behavior = capital_behavior_metrics(&close, &open, &high, &low, &volume);

    let mut computed = Vec::with_capacity(bars.len());
    for index in 0..bars.len() {
        let e_value = (high[index] + low[index] + open[index] + 2.0 * close[index]) / 5.0;
        let short_buy = index > 0 && cyan_watch[index - 1] && var1[index];
        let white_exit = index > 0 && red_hold[index - 1] && vard[index];
        let oversold =
            ma34[index].is_finite() && ((close[index] - ma34[index]) / ma34[index] * 100.0) < -14.0;
        let wait_line = if ma3[index].is_finite()
            && star_line[index].is_finite()
            && ma3[index] > star_line[index]
        {
            star_line[index]
        } else {
            ma3[index]
        };
        let previous_close = if index > 0 && close[index - 1].is_finite() {
            Some(close[index - 1])
        } else {
            None
        };
        let close_change = previous_close.map(|previous| close[index] - previous);
        let close_change_pct = previous_close
            .filter(|previous| previous.abs() > f64::EPSILON)
            .map(|previous| (close[index] - previous) / previous);
        computed.push(ComputedTrendBar {
            date: bars[index].date,
            close: close[index],
            previous_close,
            close_change,
            close_change_pct,
            swl: swl[index],
            sws: sws[index],
            k: k[index],
            d: d[index],
            j: j[index],
            accumulation_index: capital_behavior[index].accumulation_index,
            accumulation_strength: capital_behavior[index].accumulation_strength,
            swing_opportunity: capital_behavior[index].swing_opportunity,
            rebound_signal: capital_behavior[index].rebound_signal,
            trend_heat: capital_behavior[index].trend_heat,
            volume_price_heat: capital_behavior[index].volume_price_heat,
            anomaly_heat: capital_behavior[index].anomaly_heat,
            popularity_heat: capital_behavior[index].popularity_heat,
            star_line: star_line[index],
            bull_line: bull_line[index],
            wait_line,
            support: 2.0 * e_value - high[index],
            resistance: 2.0 * e_value - low[index],
            breakout: e_value + (high[index] - low[index]),
            reversal: e_value - (high[index] - low[index]),
            swl_above_sws: swl[index] > sws[index],
            kdj_golden_cross: index > 0 && k[index - 1] <= d[index - 1] && k[index] > d[index],
            kdj_dead_cross: index > 0 && k[index - 1] >= d[index - 1] && k[index] < d[index],
            kdj_overbought: k[index] >= 80.0 || d[index] >= 80.0 || j[index] >= 100.0,
            kdj_oversold: k[index] <= 20.0 || d[index] <= 20.0 || j[index] <= 0.0,
            red_hold: red_hold[index],
            cyan_watch: cyan_watch[index],
            short_buy,
            white_exit,
            oversold,
            quant_score: quant_score[index],
        });
    }
    computed
}

fn primary_state(close: &[f64], up: bool) -> Vec<bool> {
    let mut state = vec![false; close.len()];
    for index in 2..close.len() {
        state[index] = if up {
            close[index] > close[index - 1] && close[index] > close[index - 2]
        } else {
            close[index] < close[index - 1] && close[index] < close[index - 2]
        };
    }
    state
}

fn alternating_states(primary: &[bool], close: &[f64], starts_with_le_ge: bool) -> Vec<Vec<bool>> {
    let mut states = vec![primary.to_vec()];
    for state_index in 1..12 {
        let previous = states.last().expect("state exists");
        let le_ge = if state_index % 2 == 1 {
            starts_with_le_ge
        } else {
            !starts_with_le_ge
        };
        let mut next = vec![false; close.len()];
        for index in 2..close.len() {
            let condition = if le_ge {
                close[index] <= close[index - 1] && close[index] >= close[index - 2]
            } else {
                close[index] >= close[index - 1] && close[index] <= close[index - 2]
            };
            next[index] = previous[index - 1] && condition;
        }
        states.push(next);
    }
    states
}

fn or_states(states: &[Vec<bool>]) -> Vec<bool> {
    if states.is_empty() {
        return Vec::new();
    }
    let mut combined = vec![false; states[0].len()];
    for state in states {
        for (index, value) in state.iter().enumerate() {
            combined[index] |= *value;
        }
    }
    combined
}

fn ema(values: &[f64], span: usize) -> Vec<f64> {
    let alpha = 2.0 / (span as f64 + 1.0);
    let mut output = Vec::with_capacity(values.len());
    let mut previous = None;
    for value in values {
        let current = match previous {
            Some(previous_value) => alpha * *value + (1.0 - alpha) * previous_value,
            None => *value,
        };
        previous = Some(current);
        output.push(current);
    }
    output
}

fn ma(values: &[f64], window: usize) -> Vec<f64> {
    let mut output = vec![f64::NAN; values.len()];
    let mut sum = 0.0;
    for index in 0..values.len() {
        sum += values[index];
        if index >= window {
            sum -= values[index - window];
        }
        if index + 1 >= window {
            output[index] = sum / window as f64;
        }
    }
    output
}

fn rolling_sum(values: &[f64], window: usize) -> Vec<f64> {
    let mut output = Vec::with_capacity(values.len());
    let mut sum = 0.0;
    for index in 0..values.len() {
        sum += values[index];
        if index >= window {
            sum -= values[index - window];
        }
        output.push(sum);
    }
    output
}

fn rolling_mean_partial(values: &[f64], window: usize) -> Vec<f64> {
    let mut output = Vec::with_capacity(values.len());
    let mut sum = 0.0;
    for index in 0..values.len() {
        sum += values[index];
        if index >= window {
            sum -= values[index - window];
        }
        let count = (index + 1).min(window).max(1);
        output.push(sum / count as f64);
    }
    output
}

fn rolling_min(values: &[f64], window: usize) -> Vec<f64> {
    (0..values.len())
        .map(|index| {
            let start = index.saturating_sub(window.saturating_sub(1));
            values[start..=index]
                .iter()
                .copied()
                .fold(f64::INFINITY, f64::min)
        })
        .collect()
}

fn rolling_max(values: &[f64], window: usize) -> Vec<f64> {
    (0..values.len())
        .map(|index| {
            let start = index.saturating_sub(window.saturating_sub(1));
            values[start..=index]
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max)
        })
        .collect()
}

fn dma(values: &[f64], alpha: &[f64]) -> Vec<f64> {
    let mut output = Vec::with_capacity(values.len());
    let mut previous = None;
    for (value, coefficient) in values.iter().zip(alpha) {
        let coefficient = (*coefficient).clamp(0.0, 1.0);
        let current = match previous {
            Some(previous_value) => coefficient * *value + (1.0 - coefficient) * previous_value,
            None => *value,
        };
        previous = Some(current);
        output.push(current);
    }
    output
}

fn tdx_sma(values: &[f64], window: f64, weight: f64) -> Vec<f64> {
    let mut output = Vec::with_capacity(values.len());
    let mut previous = None;
    for value in values {
        let value = if value.is_finite() { *value } else { 50.0 };
        let current = match previous {
            Some(previous_value) => (weight * value + (window - weight) * previous_value) / window,
            None => value,
        };
        previous = Some(current);
        output.push(current);
    }
    output
}

fn kdj(close: &[f64], high: &[f64], low: &[f64]) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let low_min = rolling_min(low, 9);
    let high_max = rolling_max(high, 9);
    let rsv: Vec<f64> = close
        .iter()
        .enumerate()
        .map(|(index, close)| {
            let spread = high_max[index] - low_min[index];
            if spread == 0.0 {
                f64::NAN
            } else {
                ((*close - low_min[index]) / spread * 100.0).clamp(0.0, 100.0)
            }
        })
        .collect();
    let k = tdx_sma(&rsv, 3.0, 1.0);
    let d = tdx_sma(&k, 3.0, 1.0);
    let j = k.iter().zip(&d).map(|(k, d)| 3.0 * k - 2.0 * d).collect();
    (k, d, j)
}

fn quant_scores(close: &[f64], high: &[f64], low: &[f64], volume: &[f64]) -> Vec<i32> {
    let ma5 = ma(close, 5);
    let ma10 = ma(close, 10);
    let ma20 = ma(close, 20);
    let ma60 = ma(close, 60);
    let volume_ma60 = ma(volume, 60);
    let (k, _d, j) = kdj(close, high, low);
    let ema12 = ema(close, 12);
    let ema26 = ema(close, 26);
    let dif: Vec<f64> = ema12
        .iter()
        .zip(ema26)
        .map(|(left, right)| *left - right)
        .collect();
    let dea = ema(&dif, 9);
    let macd: Vec<f64> = dif
        .iter()
        .zip(&dea)
        .map(|(dif, dea)| 2.0 * (*dif - *dea))
        .collect();

    let mut scores = vec![0; close.len()];
    for index in 0..close.len() {
        if ma5[index] > ma10[index] {
            scores[index] += 20;
        }
        if ma20[index] > ma60[index] {
            scores[index] += 10;
        }
        if j[index] > k[index] {
            scores[index] += 10;
        }
        if dif[index] > dea[index] {
            scores[index] += 10;
        }
        if macd[index] > 0.0 {
            scores[index] += 10;
        }
        if volume[index] > volume_ma60[index] {
            scores[index] += 10;
        }
        if index > 0 && close[index] / close[index - 1] > 1.03 {
            scores[index] += 10;
        }
    }
    scores
}

#[derive(Clone, Copy, Debug, Default)]
struct CapitalBehaviorBar {
    accumulation_index: f64,
    accumulation_strength: f64,
    swing_opportunity: f64,
    rebound_signal: f64,
    trend_heat: f64,
    volume_price_heat: f64,
    anomaly_heat: f64,
    popularity_heat: f64,
}

fn capital_behavior_metrics(
    close: &[f64],
    open: &[f64],
    high: &[f64],
    low: &[f64],
    volume: &[f64],
) -> Vec<CapitalBehaviorBar> {
    let len = close.len();
    if len == 0 {
        return Vec::new();
    }

    let volume_ma20 = rolling_mean_partial(volume, 20);
    let low_60 = rolling_min(low, 60);
    let high_60 = rolling_max(high, 60);
    let ma5 = ma(close, 5);
    let ma10 = ma(close, 10);
    let ma20 = ma(close, 20);
    let ma60 = ma(close, 60);

    let mut raw = Vec::with_capacity(len);
    let mut low_zone_values = Vec::with_capacity(len);
    let mut lower_shadow_values = Vec::with_capacity(len);
    let mut close_recovery_values = Vec::with_capacity(len);
    let mut volume_expansion_values = Vec::with_capacity(len);
    let mut price_position_values = Vec::with_capacity(len);
    let mut pct_change_values = Vec::with_capacity(len);
    let mut amplitude_values = Vec::with_capacity(len);
    let mut volume_ratio_values = Vec::with_capacity(len);

    for index in 0..len {
        let previous_close = if index > 0 && close[index - 1].abs() > f64::EPSILON {
            close[index - 1]
        } else {
            close[index].max(0.01)
        };
        let price_range = (high[index] - low[index]).max(0.0);
        let pct_change = close[index] / previous_close - 1.0;
        let amplitude = if previous_close.abs() > f64::EPSILON {
            price_range / previous_close
        } else {
            0.0
        };
        let volume_ratio = if volume_ma20[index].abs() > f64::EPSILON {
            (volume[index] / volume_ma20[index]).clamp(0.0, 4.0)
        } else {
            1.0
        };
        let rolling_range = high_60[index] - low_60[index];
        let price_position = if rolling_range.abs() > f64::EPSILON {
            ((close[index] - low_60[index]) / rolling_range).clamp(0.0, 1.0)
        } else {
            0.5
        };
        let low_zone = 1.0 - price_position;
        let lower_shadow = if price_range.abs() > f64::EPSILON {
            ((open[index].min(close[index]) - low[index]) / price_range).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let close_recovery = if price_range.abs() > f64::EPSILON {
            ((close[index] - low[index]) / price_range).clamp(0.0, 1.0)
        } else {
            0.5
        };
        let volume_expansion = ((volume_ratio - 1.0).clamp(0.0, 2.0) / 2.0).clamp(0.0, 1.0);
        let positive_close = if close[index] >= open[index] {
            1.0
        } else {
            0.0
        };
        let negative_close = if close[index] < open[index] { 1.0 } else { 0.0 };

        raw.push(
            low_zone * 44.0
                + volume_expansion * 28.0
                + lower_shadow * 18.0
                + close_recovery * 8.0
                + positive_close * 4.0
                - price_position * 22.0
                - negative_close * pct_change.abs().min(0.08) * 260.0
                - if volume_ratio > 1.8 && close[index] < open[index] {
                    12.0
                } else {
                    0.0
                },
        );
        low_zone_values.push(low_zone);
        lower_shadow_values.push(lower_shadow);
        close_recovery_values.push(close_recovery);
        volume_expansion_values.push(volume_expansion);
        price_position_values.push(price_position);
        pct_change_values.push(pct_change);
        amplitude_values.push(amplitude);
        volume_ratio_values.push(volume_ratio);
    }

    let accumulation_index: Vec<f64> = ema(&raw, 5)
        .into_iter()
        .map(|value| (value - 34.0).clamp(-100.0, 100.0))
        .collect();

    let mut strength_raw = Vec::with_capacity(len);
    let mut swing_raw = Vec::with_capacity(len);
    let mut rebound_raw = Vec::with_capacity(len);
    let mut trend_heat = Vec::with_capacity(len);
    let mut volume_price_heat = Vec::with_capacity(len);
    let mut anomaly_heat = Vec::with_capacity(len);
    let mut popularity_heat = Vec::with_capacity(len);

    for index in 0..len {
        let low_zone = low_zone_values[index];
        let lower_shadow = lower_shadow_values[index];
        let close_recovery = close_recovery_values[index];
        let volume_expansion = volume_expansion_values[index];
        let price_position = price_position_values[index];
        let pct_change = pct_change_values[index];
        let amplitude = amplitude_values[index];
        let volume_ratio = volume_ratio_values[index];

        strength_raw.push(
            (accumulation_index[index].max(0.0) * 0.72
                + low_zone * 18.0
                + lower_shadow * 12.0
                + volume_expansion * 18.0)
                .clamp(0.0, 100.0),
        );

        let trend_turn = if ma5[index] > ma10[index] { 22.0 } else { 0.0 }
            + if ma10[index] > ma20[index] { 18.0 } else { 0.0 }
            + if close[index] > ma20[index] {
                18.0
            } else {
                0.0
            }
            + if index >= 5 && (ma20[index] - ma20[index - 5]) > 0.0 {
                14.0
            } else {
                0.0
            };
        swing_raw.push(
            (strength_raw[index] * 0.48 + trend_turn * 0.46 + low_zone * 16.0
                - price_position * 18.0)
                .clamp(0.0, 100.0),
        );
        rebound_raw.push(
            (low_zone * 32.0
                + lower_shadow * 28.0
                + volume_expansion * 20.0
                + if pct_change > 0.0 { 12.0 } else { 0.0 }
                + if index > 0 && pct_change_values[index - 1] < -0.025 {
                    8.0
                } else {
                    0.0
                })
            .clamp(0.0, 100.0),
        );

        let trend_value = if close[index] > ma5[index] { 20.0 } else { 0.0 }
            + if ma5[index] > ma10[index] { 25.0 } else { 0.0 }
            + if ma10[index] > ma20[index] { 25.0 } else { 0.0 }
            + if ma20[index] > ma60[index] { 15.0 } else { 0.0 }
            + if index >= 5 && (ma20[index] - ma20[index - 5]) > 0.0 {
                15.0
            } else {
                0.0
            };
        let volume_price_value = (volume_expansion * 42.0
            + pct_change.clamp(0.0, 0.06) * 520.0
            + close_recovery * 18.0
            + if close[index] > open[index] { 8.0 } else { 0.0 })
        .clamp(0.0, 100.0);
        let anomaly_value = (amplitude.min(0.12) * 360.0
            + (volume_ratio - 1.0).clamp(0.0, 3.0) * 20.0
            + pct_change.abs().min(0.08) * 260.0)
            .clamp(0.0, 100.0);
        let popularity_value = ((volume_ratio.min(3.0) / 3.0) * 36.0
            + amplitude.min(0.1) * 260.0
            + pct_change.clamp(0.0, 0.06) * 360.0
            + trend_value * 0.18)
            .clamp(0.0, 100.0);
        trend_heat.push(trend_value.clamp(0.0, 100.0));
        volume_price_heat.push(volume_price_value);
        anomaly_heat.push(anomaly_value);
        popularity_heat.push(popularity_value);
    }

    let accumulation_strength = ema(&strength_raw, 3);
    let swing_opportunity = ema(&swing_raw, 3);
    let rebound_signal = ema(&rebound_raw, 3);

    (0..len)
        .map(|index| CapitalBehaviorBar {
            accumulation_index: accumulation_index[index],
            accumulation_strength: accumulation_strength[index].clamp(0.0, 100.0),
            swing_opportunity: swing_opportunity[index].clamp(0.0, 100.0),
            rebound_signal: rebound_signal[index].clamp(0.0, 100.0),
            trend_heat: trend_heat[index],
            volume_price_heat: volume_price_heat[index],
            anomaly_heat: anomaly_heat[index],
            popularity_heat: popularity_heat[index],
        })
        .collect()
}

fn star_line(close: &[f64], open: &[f64], high: &[f64], low: &[f64]) -> Vec<f64> {
    let ytsl: Vec<f64> = close
        .iter()
        .enumerate()
        .map(|(index, close)| (3.0 * close + low[index] + open[index] + high[index]) / 6.0)
        .collect();
    let mut output = vec![f64::NAN; close.len()];
    for index in 20..close.len() {
        let mut total = 0.0;
        for (offset, weight) in (0..19).zip((2..=20).rev()) {
            total += ytsl[index - offset] * weight as f64;
        }
        total += ytsl[index - 20];
        output[index] = total / 211.0;
    }
    output
}

fn trend_signal_from_bar(code: &str, bar: &ComputedTrendBar) -> TrendIndicatorSignal {
    let reasons = trend_reasons(bar);
    TrendIndicatorSignal {
        code: code.to_string(),
        date: bar.date.format("%Y-%m-%d").to_string(),
        close: round4(bar.close),
        previous_close: bar.previous_close.and_then(finite_round4),
        close_change: bar.close_change.and_then(finite_round4),
        close_change_pct: bar.close_change_pct.and_then(finite_round4),
        swl: finite_round4(bar.swl),
        sws: finite_round4(bar.sws),
        k: finite_round4(bar.k),
        d: finite_round4(bar.d),
        j: finite_round4(bar.j),
        star_line: finite_round4(bar.star_line),
        bull_line: finite_round4(bar.bull_line),
        wait_line: finite_round4(bar.wait_line),
        support: finite_round4(bar.support),
        resistance: finite_round4(bar.resistance),
        breakout: finite_round4(bar.breakout),
        reversal: finite_round4(bar.reversal),
        swl_above_sws: bar.swl_above_sws,
        kdj_golden_cross: bar.kdj_golden_cross,
        kdj_dead_cross: bar.kdj_dead_cross,
        kdj_overbought: bar.kdj_overbought,
        kdj_oversold: bar.kdj_oversold,
        red_hold: bar.red_hold,
        cyan_watch: bar.cyan_watch,
        short_buy: bar.short_buy,
        white_exit: bar.white_exit,
        oversold: bar.oversold,
        quant_score: bar.quant_score,
        quant_score_max: default_quant_score_max(),
        pattern_score: pattern_score(bar),
        pattern_score_max: default_pattern_score_max(),
        pattern_signals: pattern_signals(bar),
        status: trend_status(bar),
        reasons,
        notes: trend_notes(),
    }
}

fn trend_point_from_bar(bar: &ComputedTrendBar) -> TrendIndicatorPoint {
    TrendIndicatorPoint {
        date: bar.date.format("%Y-%m-%d").to_string(),
        close: round4(bar.close),
        swl: finite_round4(bar.swl),
        sws: finite_round4(bar.sws),
        k: finite_round4(bar.k),
        d: finite_round4(bar.d),
        j: finite_round4(bar.j),
        accumulation_index: finite_round4(bar.accumulation_index),
        accumulation_strength: finite_round4(bar.accumulation_strength),
        swing_opportunity: finite_round4(bar.swing_opportunity),
        rebound_signal: finite_round4(bar.rebound_signal),
        trend_heat: finite_round4(bar.trend_heat),
        volume_price_heat: finite_round4(bar.volume_price_heat),
        anomaly_heat: finite_round4(bar.anomaly_heat),
        popularity_heat: finite_round4(bar.popularity_heat),
        red_hold: bar.red_hold,
        cyan_watch: bar.cyan_watch,
        short_buy: bar.short_buy,
        white_exit: bar.white_exit,
    }
}

fn trend_reasons(bar: &ComputedTrendBar) -> Vec<String> {
    let mut reasons = Vec::new();
    if bar.short_buy {
        reasons.push("short_buy_signal".to_string());
    }
    if bar.red_hold {
        reasons.push("red_hold".to_string());
    }
    if bar.swl_above_sws {
        reasons.push("swl_above_sws".to_string());
    }
    if bar.kdj_golden_cross {
        reasons.push("kdj_golden_cross".to_string());
    }
    if bar.kdj_dead_cross {
        reasons.push("kdj_dead_cross".to_string());
    }
    if bar.kdj_oversold {
        reasons.push("kdj_oversold".to_string());
    }
    if bar.kdj_overbought {
        reasons.push("kdj_overbought".to_string());
    }
    if bar.quant_score >= 60 {
        reasons.push("high_quant_score".to_string());
    }
    if bar.white_exit {
        reasons.push("white_exit".to_string());
    }
    if bar.cyan_watch {
        reasons.push("cyan_watch".to_string());
    }
    if bar.oversold {
        reasons.push("oversold".to_string());
    }
    if bar.accumulation_strength >= 55.0 {
        reasons.push("accumulation_strength".to_string());
    }
    if bar.swing_opportunity >= 60.0 {
        reasons.push("swing_opportunity".to_string());
    }
    reasons
}

fn pattern_score(bar: &ComputedTrendBar) -> i32 {
    let values = [
        bar.accumulation_strength,
        bar.swing_opportunity,
        bar.trend_heat,
        bar.volume_price_heat,
        bar.anomaly_heat,
        bar.popularity_heat,
    ];
    (values.iter().sum::<f64>() / values.len().max(1) as f64).round() as i32
}

fn pattern_signals(bar: &ComputedTrendBar) -> Vec<String> {
    let mut signals = Vec::new();
    if bar.accumulation_index > 8.0 && bar.accumulation_strength >= 45.0 {
        signals.push("bottom_accumulation".to_string());
    }
    if bar.swing_opportunity >= 60.0 {
        signals.push("swing_opportunity".to_string());
    }
    if bar.rebound_signal >= 62.0 {
        signals.push("rebound_signal".to_string());
    }
    if bar.trend_heat >= 65.0 && bar.volume_price_heat >= 55.0 {
        signals.push("dragon_trend_volume".to_string());
    }
    signals
}

fn trend_status(bar: &ComputedTrendBar) -> String {
    if bar.short_buy {
        "buy_setup"
    } else if bar.white_exit {
        "exit"
    } else if bar.red_hold && bar.swl_above_sws {
        "uptrend"
    } else if bar.red_hold {
        "hold"
    } else if bar.cyan_watch {
        "watch"
    } else if bar.oversold {
        "oversold"
    } else {
        "neutral"
    }
    .to_string()
}

fn trend_score(signal: &TrendIndicatorSignal) -> f64 {
    short_buy_score(signal)
}

fn short_buy_score(signal: &TrendIndicatorSignal) -> f64 {
    let mut score = (signal.quant_score as f64 / signal.quant_score_max.max(1) as f64) * 28.0;
    if signal.short_buy {
        score += 24.0;
    }
    if signal
        .pattern_signals
        .iter()
        .any(|value| value == "bottom_accumulation")
    {
        score += 10.0;
    }
    if signal
        .pattern_signals
        .iter()
        .any(|value| value == "swing_opportunity")
    {
        score += 12.0;
    }
    if signal
        .pattern_signals
        .iter()
        .any(|value| value == "rebound_signal")
    {
        score += 10.0;
    }
    if signal.swl_above_sws {
        score += 6.0;
    }
    if signal.red_hold {
        score += 4.0;
    }
    if signal
        .star_line
        .map(|star_line| signal.close > star_line)
        .unwrap_or(false)
    {
        score += 3.0;
    }
    if signal.white_exit {
        score -= 30.0;
    }
    if signal.cyan_watch {
        score -= 12.0;
    }
    score.clamp(0.0, 100.0)
}

fn combined_trend_score(base_score: f64, trend_score: f64) -> f64 {
    combined_short_buy_score(base_score, trend_score)
}

fn combined_short_buy_score(base_score: f64, trend_score: f64) -> f64 {
    let base_component = base_score.clamp(0.0, 20.0) / 20.0 * 25.0;
    (base_component + trend_score * 0.75).min(100.0)
}

fn trend_screen_explanation(
    candidate: &ScreenedStock,
    signal: &TrendIndicatorSignal,
    trend_score: f64,
    final_score: f64,
) -> SelectionExplanation {
    let mut basis = vec![
        format!(
            "Passed the current base screen with raw score {:.2}.",
            candidate.score
        ),
        short_buy_basis_text(signal),
    ];
    if let Some(pattern_text) = pattern_basis_text(signal) {
        basis.push(pattern_text);
    }

    let base_component = candidate.score.clamp(0.0, 20.0) / 20.0 * 25.0;
    let trend_component = trend_score * 0.75;
    SelectionExplanation {
        basis,
        score_breakdown: vec![
            score_contribution("base_score", "Base screen", candidate.score, base_component, 14.0, 9.0),
            score_contribution("short_buy_score", "Short-buy setup", trend_score, trend_component, 70.0, 45.0),
            score_contribution("final_score", "Final score", final_score, final_score, 70.0, 45.0),
        ],
        risk_checks: short_buy_risk_checks(signal),
        verification: vec![
            "\u{5efa}\u{8bae}\u{7528}\u{56fe}\u{8c31}\u{9009}\u{80a1}\u{4ea4}\u{53c9}\u{9a8c}\u{8bc1}\u{ff0c}\u{786e}\u{8ba4}\u{8be5}\u{80a1}\u{662f}\u{5426}\u{5904}\u{5728}\u{66f4}\u{5f3a}\u{7684}\u{4e3b}\u{9898}\u{4e2d}\u{5fc3}\u{3002}".to_string(),
            "Watch the next trading day direction, volume, and close for setup confirmation.".to_string(),
        ],
    }
}

fn short_buy_basis_text(signal: &TrendIndicatorSignal) -> String {
    if signal.short_buy {
        return "Short-buy signal is active; this is the highest-priority timing trigger."
            .to_string();
    }
    if signal
        .pattern_signals
        .iter()
        .any(|value| value == "swing_opportunity")
    {
        return "Swing-opportunity signal is strong enough for short-term watchlist inclusion."
            .to_string();
    }
    if signal
        .pattern_signals
        .iter()
        .any(|value| value == "rebound_signal")
    {
        return "Rebound signal is active and supports a short-term repair watch.".to_string();
    }
    "No explicit short-buy trigger is active; selection relies on quant score and base conditions."
        .to_string()
}

fn pattern_basis_text(signal: &TrendIndicatorSignal) -> Option<String> {
    let mut labels = Vec::new();
    if signal
        .pattern_signals
        .iter()
        .any(|value| value == "bottom_accumulation")
    {
        labels.push("bottom accumulation");
    }
    if signal
        .pattern_signals
        .iter()
        .any(|value| value == "swing_opportunity")
    {
        labels.push("swing opportunity");
    }
    if signal
        .pattern_signals
        .iter()
        .any(|value| value == "rebound_signal")
    {
        labels.push("rebound signal");
    }
    if signal
        .pattern_signals
        .iter()
        .any(|value| value == "dragon_trend_volume")
    {
        labels.push("trend-volume resonance");
    }
    if labels.is_empty() {
        None
    } else {
        Some(format!("Pattern signals: {}.", labels.join(", ")))
    }
}

fn short_buy_risk_checks(signal: &TrendIndicatorSignal) -> Vec<String> {
    let mut risks = Vec::new();
    if signal.white_exit {
        risks.push(
            "White-exit signal is active; short-buy setup has high invalidation risk.".to_string(),
        );
    }
    if signal.cyan_watch {
        risks.push("Cyan-watch state means trend confirmation is insufficient.".to_string());
    }
    if risks.is_empty() {
        risks.push("No major exit/high-zone risk is active, but next-day price-volume confirmation is still required.".to_string());
    }
    risks
}

fn trend_notes() -> Vec<String> {
    vec![
        "Accumulation analysis is inferred from daily OHLCV only; real chip distribution, seat-level LHB, and main-fund flow are not included."
            .to_string(),
        "WINNER(C) depends on chip-distribution data and is omitted; quant_score is scored out of 90."
            .to_string(),
        "SWS uses the formula's volume/capital term as a percent DMA coefficient and clips it to 1%-100%."
            .to_string(),
    ]
}

pub fn validate_data_set(data: &CoreDataSet) -> CoreResult<DataSourceSummary> {
    let mut warnings = Vec::new();
    let mut stock_codes = HashSet::new();
    for stock in &data.stocks {
        if stock.code.trim().is_empty() {
            return Err(CoreError::new("Stock code cannot be empty"));
        }
        if !stock_codes.insert(stock.code.clone()) {
            return Err(CoreError::new(format!(
                "Duplicate stock code: {}",
                stock.code
            )));
        }
    }

    for relation in &data.relations {
        if !stock_codes.contains(&relation.source_code) {
            warnings.push(format!(
                "Relation source code is not in stock universe: {}",
                relation.source_code
            ));
        }
        if !stock_codes.contains(&relation.target_code) {
            warnings.push(format!(
                "Relation target code is not in stock universe: {}",
                relation.target_code
            ));
        }
        if !(0.0..=1.0).contains(&relation.weight) {
            return Err(CoreError::new(format!(
                "Relation weight must be within [0, 1]: {} -> {}",
                relation.source_code, relation.target_code
            )));
        }
    }

    let mut history_bar_count = 0;
    for (code, bars) in &data.histories {
        if !stock_codes.contains(code) {
            warnings.push(format!("History code is not in stock universe: {code}"));
        }
        let mut previous_date = None;
        for bar in bars {
            let date = parse_date(&bar.date)?;
            if bar.close < 0.0 {
                return Err(CoreError::new(format!(
                    "History close cannot be negative: {code} {}",
                    bar.date
                )));
            }
            for (label, value) in [
                ("open", bar.open),
                ("high", bar.high),
                ("low", bar.low),
                ("volume", bar.volume),
                ("capital", bar.capital),
            ] {
                if value.map(|value| value < 0.0).unwrap_or(false) {
                    return Err(CoreError::new(format!(
                        "History {label} cannot be negative: {code} {}",
                        bar.date
                    )));
                }
            }
            if previous_date
                .map(|previous| date < previous)
                .unwrap_or(false)
            {
                warnings.push(format!("History is not sorted by date: {code}"));
            }
            previous_date = Some(date);
            history_bar_count += 1;
        }
    }

    Ok(DataSourceSummary {
        stock_count: data.stocks.len(),
        relation_count: data.relations.len(),
        history_symbol_count: data.histories.len(),
        history_bar_count,
        warnings,
    })
}

fn history_bars_in_range(
    history: &[HistoryBar],
    start_date: &str,
    end_date: &str,
) -> CoreResult<Vec<HistoryBar>> {
    let start = parse_date(start_date)?;
    let end = parse_date(end_date)?;
    if start > end {
        return Ok(Vec::new());
    }

    let mut filtered = Vec::new();
    for bar in history {
        let date = parse_date(&bar.date)?;
        if date >= start && date <= end {
            filtered.push(bar.clone());
        }
    }
    filtered.sort_by(|left, right| {
        parse_date(&left.date)
            .unwrap_or(NaiveDate::MIN)
            .cmp(&parse_date(&right.date).unwrap_or(NaiveDate::MIN))
    });
    Ok(filtered)
}

fn history_points_from_bars(history: &[HistoryBar]) -> CoreResult<Vec<HistoryPoint>> {
    let mut points = Vec::with_capacity(history.len());
    for bar in history {
        points.push(HistoryPoint {
            date: parse_date(&bar.date)?,
            close: bar.close,
        });
    }
    points.sort_by(|left, right| left.date.cmp(&right.date));
    Ok(points)
}

fn mock_history(
    universe: &[StockItem],
    code: &str,
    start_date: &str,
    end_date: &str,
) -> CoreResult<Vec<HistoryBar>> {
    let stock = universe
        .iter()
        .find(|stock| stock.code == code)
        .ok_or_else(|| CoreError::new(format!("Stock {code} not found")))?;
    let start = parse_date(start_date)?;
    let end = parse_date(end_date)?;
    if start > end {
        return Ok(Vec::new());
    }

    let mut points = Vec::new();
    let mut current = start;
    let seed: u32 = code.bytes().map(u32::from).sum();
    let drift = 0.00028 + (seed % 7) as f64 * 0.00006;
    let phase = (seed % 17) as f64 / 3.0;
    let mut previous_close = stock.price;
    let capital = stock
        .market_cap_billion
        .map(|market_cap| market_cap * 1_000_000_000.0 / stock.price.max(0.01));
    while current <= end {
        if !matches!(current.weekday(), Weekday::Sat | Weekday::Sun) {
            let index = points.len() as f64;
            let wave = (index / 8.0 + phase).sin() * 0.018;
            let pullback = (index / 23.0 + phase).sin() * 0.008;
            let close = (stock.price * (1.0 + drift * index + wave + pullback)).max(0.01);
            let open = previous_close * (1.0 + (index / 9.0 + phase).cos() * 0.004);
            let high = open.max(close) * (1.006 + (index / 11.0 + phase).sin().abs() * 0.006);
            let low = open.min(close) * (0.994 - (index / 13.0 + phase).cos().abs() * 0.004);
            let volume = (2_000_000.0 + (seed % 31) as f64 * 55_000.0 + index * 2_500.0)
                * (1.0 + (index / 10.0 + phase).sin().abs() * 0.35);
            points.push(HistoryBar {
                date: current.format("%Y-%m-%d").to_string(),
                open: Some(open),
                high: Some(high),
                low: Some(low),
                close,
                volume: Some(volume),
                capital,
            });
            previous_close = close;
        }
        current = current
            .checked_add_days(Days::new(1))
            .ok_or_else(|| CoreError::new("Date overflow"))?;
    }
    Ok(points)
}

fn parse_date(input: &str) -> CoreResult<NaiveDate> {
    NaiveDate::parse_from_str(input, "%Y%m%d")
        .or_else(|_| NaiveDate::parse_from_str(input, "%Y-%m-%d"))
        .map_err(|_| CoreError::new(format!("Invalid date: {input}")))
}

fn backtest_metrics(equity_curve: &[EquityPoint]) -> (f64, Option<f64>, Option<f64>) {
    let Some(first) = equity_curve.first() else {
        return (0.0, None, None);
    };
    let Some(last) = equity_curve.last() else {
        return (0.0, None, None);
    };

    let total_return = last.equity / first.equity - 1.0;
    let first_date = match NaiveDate::parse_from_str(&first.date, "%Y-%m-%d") {
        Ok(date) => date,
        Err(_) => return (total_return, None, None),
    };
    let last_date = match NaiveDate::parse_from_str(&last.date, "%Y-%m-%d") {
        Ok(date) => date,
        Err(_) => return (total_return, None, None),
    };
    let days = (last_date - first_date).num_days().max(1) as f64;
    let years = days / 365.0;
    let annualized_return = if years > 0.0 {
        Some((1.0 + total_return).powf(1.0 / years) - 1.0)
    } else {
        None
    };

    let mut rolling_max = first.equity;
    let mut max_drawdown = 0.0_f64;
    for point in equity_curve {
        rolling_max = rolling_max.max(point.equity);
        if rolling_max > 0.0 {
            max_drawdown = max_drawdown.min(point.equity / rolling_max - 1.0);
        }
    }

    (total_return, annualized_return, Some(max_drawdown))
}

fn heuristic_criteria(message: &str) -> ScreenCriteria {
    let mut criteria = ScreenCriteria::default();
    criteria.max_pe = extract_number_after(message, &["PE", "pe", "市盈率"]);
    criteria.max_pb = extract_number_after(message, &["PB", "pb", "市净率"]);
    criteria.min_roe = extract_percent_after(message, &["ROE", "roe", "净资产收益率"]);
    criteria.industry = extract_industry(message);
    criteria
}

fn extract_number_after(message: &str, labels: &[&str]) -> Option<f64> {
    for label in labels {
        let pattern = format!(
            r"{}\s*(?:低于|小于|<=|<|不高于|少于|在)?\s*(\d+(?:\.\d+)?)",
            regex::escape(label)
        );
        if let Ok(regex) = Regex::new(&pattern) {
            if let Some(match_result) = regex.captures(message) {
                return match_result.get(1)?.as_str().parse().ok();
            }
        }
    }
    None
}

fn extract_percent_after(message: &str, labels: &[&str]) -> Option<f64> {
    for label in labels {
        let pattern = format!(
            r"{}\s*(?:高于|大于|>=|>|不低于|超过|在)?\s*(\d+(?:\.\d+)?)\s*(%)?",
            regex::escape(label)
        );
        if let Ok(regex) = Regex::new(&pattern) {
            if let Some(match_result) = regex.captures(message) {
                let value: f64 = match_result.get(1)?.as_str().parse().ok()?;
                return Some(if match_result.get(2).is_some() || value > 1.0 {
                    value / 100.0
                } else {
                    value
                });
            }
        }
    }
    None
}

fn extract_industry(message: &str) -> Option<String> {
    let known = [
        ("银行", "银行"),
        ("白酒", "白酒"),
        ("饮料", "白酒"),
        ("电池", "动力电池"),
        ("动力电池", "动力电池"),
        ("新能源", "动力电池"),
        ("汽车", "汽车"),
        ("电子", "电子制造"),
        ("电子制造", "电子制造"),
        ("光伏", "光伏"),
        ("化工", "化工"),
    ];
    known
        .iter()
        .find(|(keyword, _industry)| message.contains(keyword))
        .map(|(_keyword, industry)| (*industry).to_string())
}

fn extract_codes(message: &str) -> Vec<String> {
    let Ok(regex) = Regex::new(r"\b\d{6}\.(?:SH|SZ|BJ)\b") else {
        return Vec::new();
    };
    regex
        .find_iter(&message.to_uppercase())
        .map(|matched| matched.as_str().to_string())
        .collect()
}

fn extract_date(message: &str, default: &str, first: bool) -> String {
    let Ok(date_regex) = Regex::new(r"\b(20\d{2})(?:[-/.年]?)(\d{1,2})(?:[-/.月]?)(\d{1,2})")
    else {
        return default.to_string();
    };
    let dates: Vec<(String, String, String)> = date_regex
        .captures_iter(message)
        .filter_map(|captures| {
            Some((
                captures.get(1)?.as_str().to_string(),
                captures.get(2)?.as_str().to_string(),
                captures.get(3)?.as_str().to_string(),
            ))
        })
        .collect();
    if let Some((year, month, day)) = if first { dates.first() } else { dates.last() } {
        return format!("{year}{:0>2}{:0>2}", month, day);
    }

    let Ok(year_regex) = Regex::new(r"\b(20\d{2})\b") else {
        return default.to_string();
    };
    let years: Vec<String> = year_regex
        .captures_iter(message)
        .filter_map(|captures| captures.get(1).map(|matched| matched.as_str().to_string()))
        .collect();
    if let Some(year) = if first { years.first() } else { years.last() } {
        if first {
            format!("{year}0101")
        } else {
            format!("{year}1231")
        }
    } else {
        default.to_string()
    }
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles
        .iter()
        .any(|needle| text.contains(&needle.to_lowercase()))
}

fn sorted_pair(left: &str, right: &str) -> (String, String) {
    if left <= right {
        (left.to_string(), right.to_string())
    } else {
        (right.to_string(), left.to_string())
    }
}

fn round6(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

fn round4(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

fn finite_round4(value: f64) -> Option<f64> {
    if value.is_finite() {
        Some(round4(value))
    } else {
        None
    }
}

fn c_string_to_str<'a>(input: *const c_char) -> CoreResult<&'a str> {
    if input.is_null() {
        return Err(CoreError::new("Input pointer is null"));
    }
    unsafe { CStr::from_ptr(input) }
        .to_str()
        .map_err(|error| CoreError::new(error.to_string()))
}

fn ffi_response<F>(input: *const c_char, handler: F) -> *mut c_char
where
    F: FnOnce(&str) -> CoreResult<Value> + panic::UnwindSafe,
{
    let result = panic::catch_unwind(|| {
        let input = c_string_to_str(input)?;
        handler(input)
    });

    let envelope = match result {
        Ok(Ok(data)) => json!({ "ok": true, "data": data }),
        Ok(Err(error)) => json!({ "ok": false, "error": error.to_string() }),
        Err(_) => json!({ "ok": false, "error": "Rust core panicked" }),
    };

    let output = envelope.to_string().replace('\0', "");
    CString::new(output)
        .expect("JSON output should not contain null bytes")
        .into_raw()
}

#[no_mangle]
pub extern "C" fn gp_core_screen_json(criteria_json: *const c_char) -> *mut c_char {
    ffi_response(criteria_json, |input| {
        let criteria: ScreenCriteria = serde_json::from_str(input)?;
        serde_json::to_value(screen_with_mock(&criteria)).map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_screen_with_data_json(request_json: *const c_char) -> *mut c_char {
    ffi_response(request_json, |input| {
        let request: ScreenWithDataRequest = serde_json::from_str(input)?;
        serde_json::to_value(screen_with_data(&request.data, &request.criteria)?)
            .map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_graph_screen_json(request_json: *const c_char) -> *mut c_char {
    ffi_response(request_json, |input| {
        let request: GraphScreenRequest = serde_json::from_str(input)?;
        serde_json::to_value(graph_screen_with_mock(&request)).map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_graph_screen_with_data_json(request_json: *const c_char) -> *mut c_char {
    ffi_response(request_json, |input| {
        let request: GraphScreenWithDataRequest = serde_json::from_str(input)?;
        serde_json::to_value(graph_screen_with_data(&request.data, &request.request)?)
            .map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_backtest_json(request_json: *const c_char) -> *mut c_char {
    ffi_response(request_json, |input| {
        let request: BacktestRequest = serde_json::from_str(input)?;
        serde_json::to_value(backtest_with_mock(&request)?).map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_backtest_with_data_json(request_json: *const c_char) -> *mut c_char {
    ffi_response(request_json, |input| {
        let request: BacktestWithDataRequest = serde_json::from_str(input)?;
        serde_json::to_value(backtest_with_data(&request.data, &request.request)?)
            .map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_trend_json(request_json: *const c_char) -> *mut c_char {
    ffi_response(request_json, |input| {
        let request: TrendIndicatorRequest = serde_json::from_str(input)?;
        serde_json::to_value(trend_with_mock(&request)?).map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_trend_with_data_json(request_json: *const c_char) -> *mut c_char {
    ffi_response(request_json, |input| {
        let request: TrendWithDataRequest = serde_json::from_str(input)?;
        serde_json::to_value(trend_with_data(&request.data, &request.request)?).map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_trend_screen_json(request_json: *const c_char) -> *mut c_char {
    ffi_response(request_json, |input| {
        let request: TrendScreenRequest = serde_json::from_str(input)?;
        serde_json::to_value(trend_screen_with_mock(&request)?).map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_trend_screen_with_data_json(request_json: *const c_char) -> *mut c_char {
    ffi_response(request_json, |input| {
        let request: TrendScreenWithDataRequest = serde_json::from_str(input)?;
        serde_json::to_value(trend_screen_with_data(&request.data, &request.request)?)
            .map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_agent_json(request_json: *const c_char) -> *mut c_char {
    ffi_response(request_json, |input| {
        let request: AgentRequest = serde_json::from_str(input)?;
        serde_json::to_value(run_agent_with_mock(&request.message)?).map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_agent_with_data_json(request_json: *const c_char) -> *mut c_char {
    ffi_response(request_json, |input| {
        let request: AgentWithDataRequest = serde_json::from_str(input)?;
        serde_json::to_value(run_agent_with_data(&request.data, &request.message)?)
            .map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_mobile_stock_skill_json(request_json: *const c_char) -> *mut c_char {
    ffi_response(request_json, |input| {
        let request: MobileStockSkillRequest = serde_json::from_str(input)?;
        serde_json::to_value(run_mobile_stock_skill(&request)).map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_validate_data_source_json(data_json: *const c_char) -> *mut c_char {
    ffi_response(data_json, |input| {
        let data: CoreDataSet = serde_json::from_str(input)?;
        serde_json::to_value(validate_data_set(&data)?).map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_free_string(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        let _ = CString::from_raw(ptr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_data_set() -> CoreDataSet {
        let stocks = vec![
            StockItem {
                code: "111111.SZ".to_string(),
                name: "样本银行".to_string(),
                industry: "银行".to_string(),
                is_st: false,
                price: 10.0,
                pe: Some(5.0),
                pb: Some(0.8),
                roe: Some(0.15),
                market_cap_billion: Some(100.0),
                dividend_yield: Some(0.04),
                deducted_net_profit_billion: Some(8.0),
                deducted_net_profit_margin: Some(12.0),
                deducted_net_profit_growth_rate: Some(12.0),
                ..StockItem::default()
            },
            StockItem {
                code: "222222.SZ".to_string(),
                name: "样本汽车".to_string(),
                industry: "汽车".to_string(),
                is_st: false,
                price: 20.0,
                pe: Some(30.0),
                pb: Some(3.0),
                roe: Some(0.08),
                market_cap_billion: Some(60.0),
                dividend_yield: Some(0.01),
                deducted_net_profit_billion: Some(3.0),
                deducted_net_profit_margin: Some(8.0),
                deducted_net_profit_growth_rate: Some(8.0),
                ..StockItem::default()
            },
        ];
        let relations = vec![StockRelation {
            source_code: "111111.SZ".to_string(),
            target_code: "222222.SZ".to_string(),
            relation_type: "custom_peer".to_string(),
            weight: 0.5,
            description: Some("\u{672c}\u{5730}\u{6837}\u{672c}\u{5173}\u{7cfb}".to_string()),
        }];
        let histories = HashMap::from([(
            "111111.SZ".to_string(),
            vec![
                HistoryBar {
                    date: "2020-01-01".to_string(),
                    open: Some(9.8),
                    high: Some(10.2),
                    low: Some(9.7),
                    close: 10.0,
                    volume: Some(1_000_000.0),
                    capital: Some(1_000_000_000.0),
                },
                HistoryBar {
                    date: "2020-01-02".to_string(),
                    open: Some(10.2),
                    high: Some(11.2),
                    low: Some(10.1),
                    close: 11.0,
                    volume: Some(1_200_000.0),
                    capital: Some(1_000_000_000.0),
                },
                HistoryBar {
                    date: "2020-01-03".to_string(),
                    open: Some(11.1),
                    high: Some(12.2),
                    low: Some(11.0),
                    close: 12.0,
                    volume: Some(1_400_000.0),
                    capital: Some(1_000_000_000.0),
                },
            ],
        )]);
        CoreDataSet {
            stocks,
            relations,
            histories,
            financials: HashMap::from([(
                "111111.SZ".to_string(),
                StockFinancialSnapshot {
                    latest_eps: Some(0.42),
                    latest_bps: Some(12.5),
                    period: Some("2026Q1".to_string()),
                    source: Some("data/cache/tdx_fundamentals.csv".to_string()),
                    quarterly_eps: vec![
                        QuarterlyEpsPoint {
                            period: "2026Q1".to_string(),
                            value: 0.42,
                            source: None,
                        },
                        QuarterlyEpsPoint {
                            period: "2025Q1".to_string(),
                            value: 0.36,
                            source: None,
                        },
                    ],
                    notes: vec!["季度 EPS 明细来自缓存".to_string()],
                },
            )]),
            capital_evidence: HashMap::from([(
                "111111.SZ".to_string(),
                CapitalEvidenceResult {
                    stock_code: "111111.SZ".to_string(),
                    generated_at: "2026-06-25T10:00:00".to_string(),
                    composite_score: Some(68.0),
                    confidence: "中".to_string(),
                    model_used: false,
                    as_of_trade_date: Some("2026-06-25".to_string()),
                    freshness: "fresh-cache".to_string(),
                    contributions: BTreeMap::new(),
                    summary: Some("cached capital evidence".to_string()),
                    sections: Vec::new(),
                    items: vec![CapitalEvidenceItem {
                        category: "fund_flow".to_string(),
                        source: "test cache".to_string(),
                        title: "fund flow sample".to_string(),
                        date: Some("2026-06-25".to_string()),
                        metrics: BTreeMap::from([("净额".to_string(), "1200万".to_string())]),
                        sentiment: "positive".to_string(),
                        weight: 0.35,
                        confidence: "中".to_string(),
                        url: None,
                        score: Some(72.0),
                        note: Some("cached fund flow".to_string()),
                    }],
                    notes: vec!["cached note".to_string()],
                },
            )]),
        }
    }

    #[test]
    fn screens_by_pe_and_roe() {
        let result = screen_with_mock(&ScreenCriteria {
            max_pe: Some(10.0),
            min_roe: Some(0.1),
            ..ScreenCriteria::default()
        });
        assert_eq!(result.returned, 3);
        assert_eq!(result.total, 3);
        assert!(result.items.iter().all(|item| item
            .stock
            .pe
            .map(|pe| pe <= 10.0)
            .unwrap_or(false)));
        assert!(result.items.iter().all(|item| item
            .stock
            .roe
            .and_then(as_percent)
            .map(|roe| roe >= 10.0)
            .unwrap_or(false)));
        assert!(result
            .items
            .iter()
            .all(|item| item.reasons.contains(&"pe_ok".to_string())));
        assert!(result
            .items
            .windows(2)
            .all(|pair| pair[0].score >= pair[1].score));
    }

    #[test]
    fn default_screen_includes_non_st_stocks() {
        let result = screen_with_mock(&ScreenCriteria::default());
        assert_eq!(result.returned, 10);
    }

    #[test]
    fn graph_screen_adds_relation_signals() {
        let result = graph_screen_with_mock(&GraphScreenRequest {
            criteria: ScreenCriteria {
                max_pe: Some(30.0),
                min_roe: Some(0.1),
                ..ScreenCriteria::default()
            },
            seed_codes: vec!["300750.SZ".to_string()],
            relation_depth: 1,
            relation_weight: 0.4,
            limit: 20,
        });
        assert!(result.relation_count >= 7);
        assert!(!result.items.is_empty());
        assert!(result.items.iter().any(|item| item.relation_score > 0.0));
    }

    #[test]
    fn backtest_uses_mock_history() {
        let result = backtest_with_mock(&BacktestRequest {
            criteria: ScreenCriteria {
                max_pe: Some(10.0),
                ..ScreenCriteria::default()
            },
            source: default_backtest_source(),
            stock_codes: Vec::new(),
            start_date: "20200101".to_string(),
            end_date: "20200110".to_string(),
            top_n: 2,
            initial_cash: 1000.0,
        })
        .expect("backtest should run");
        assert_eq!(result.metrics.num_stocks, 2);
        assert_eq!(result.equity_curve.first().unwrap().equity, 1000.0);
        assert!(result.metrics.total_return > 0.0);
    }

    #[test]
    fn trend_indicator_runs_with_mock_history() {
        let result = trend_with_mock(&TrendIndicatorRequest {
            code: "300750.SZ".to_string(),
            start_date: "20200101".to_string(),
            end_date: default_end_date(),
            series_limit: 80,
        })
        .expect("trend indicator should run");
        assert_eq!(result.stock.code, "300750.SZ");
        assert!(!result.series.is_empty());
        assert!(result.signal.quant_score <= 90);
        assert!(result.signal.pattern_score <= 100);
        let latest = result.series.last().unwrap();
        assert!(latest.accumulation_index.is_some());
        assert!(latest.accumulation_strength.is_some());
        assert!(latest.swing_opportunity.is_some());
        assert!(latest.rebound_signal.is_some());
        assert!(latest.trend_heat.is_some());
    }

    #[test]
    fn trend_screen_ranks_candidates() {
        let result = trend_screen_with_mock(&TrendScreenRequest {
            criteria: ScreenCriteria::default(),
            start_date: "20200101".to_string(),
            end_date: default_end_date(),
            limit: 5,
        })
        .expect("trend screen should run");
        assert_eq!(result.returned, 5);
        assert!(result.items[0].final_score >= result.items[4].final_score);
    }

    #[test]
    fn sector_screen_uses_full_filtered_universe_before_grouping() {
        let stocks: Vec<StockItem> = (0..260)
            .map(|idx| StockItem {
                code: format!("300{:03}.SZ", idx),
                name: format!("{}{:03}", "\u{7b97}\u{529b}\u{82af}\u{7247}", idx),
                industry: "\u{534a}\u{5bfc}\u{4f53}".to_string(),
                is_st: false,
                price: 20.0 + idx as f64 / 100.0,
                pe: Some(18.0),
                pb: Some(2.2),
                roe: Some(0.16),
                market_cap_billion: Some(80.0),
                dividend_yield: Some(0.02),
                deducted_net_profit_billion: Some(3.0),
                deducted_net_profit_margin: Some(12.0),
                deducted_net_profit_growth_rate: Some(18.0),
                ..StockItem::default()
            })
            .collect();
        let result = sector_screen_stocks(
            &stocks,
            &SectorScreenRequest {
                criteria: ScreenCriteria {
                    min_roe: Some(0.1),
                    limit: 10,
                    ..ScreenCriteria::default()
                },
                max_sectors: 1,
                per_sector_limit: 7,
                min_sector_candidates: 1,
                group_by: default_sector_group_by(),
            },
        );

        assert_eq!(result.total, 260);
        assert_eq!(result.sector_count, 1);
        assert_eq!(result.groups[0].total, 260);
        assert_eq!(result.groups[0].returned, 7);
    }

    #[test]
    fn sector_screen_can_group_by_market_board() {
        let stocks = vec![
            StockItem {
                code: "688001.SH".to_string(),
                name: "star-a".to_string(),
                industry: "chip".to_string(),
                is_st: false,
                price: 20.0,
                pe: Some(18.0),
                pb: Some(2.0),
                roe: Some(0.16),
                market_cap_billion: Some(80.0),
                dividend_yield: Some(0.01),
                deducted_net_profit_billion: Some(1.0),
                deducted_net_profit_margin: Some(12.0),
                deducted_net_profit_growth_rate: Some(18.0),
                ..StockItem::default()
            },
            StockItem {
                code: "300001.SZ".to_string(),
                name: "growth-a".to_string(),
                industry: "device".to_string(),
                is_st: false,
                price: 21.0,
                pe: Some(17.0),
                pb: Some(2.1),
                roe: Some(0.17),
                market_cap_billion: Some(90.0),
                dividend_yield: Some(0.01),
                deducted_net_profit_billion: Some(1.1),
                deducted_net_profit_margin: Some(13.0),
                deducted_net_profit_growth_rate: Some(19.0),
                ..StockItem::default()
            },
            StockItem {
                code: "600000.SH".to_string(),
                name: "main-a".to_string(),
                industry: "bank".to_string(),
                is_st: false,
                price: 10.0,
                pe: Some(8.0),
                pb: Some(0.8),
                roe: Some(0.12),
                market_cap_billion: Some(200.0),
                dividend_yield: Some(0.03),
                deducted_net_profit_billion: Some(2.0),
                deducted_net_profit_margin: Some(20.0),
                deducted_net_profit_growth_rate: Some(8.0),
                ..StockItem::default()
            },
        ];
        let result = sector_screen_stocks(
            &stocks,
            &SectorScreenRequest {
                group_by: "board".to_string(),
                max_sectors: 10,
                per_sector_limit: 5,
                min_sector_candidates: 1,
                ..SectorScreenRequest::default()
            },
        );

        let groups: Vec<&str> = result
            .groups
            .iter()
            .map(|group| group.sector.as_str())
            .collect();
        assert_eq!(
            groups,
            vec![
                "\u{79d1}\u{521b}\u{677f}",
                "\u{521b}\u{4e1a}\u{677f}",
                "\u{6caa}\u{4e3b}\u{677f}"
            ]
        );
    }

    #[test]
    fn observe_with_data_returns_financial_and_trend_payloads() {
        let mut data = sample_data_set();
        let history: Vec<HistoryBar> = (0..45)
            .map(|idx| {
                let close = 10.0 + idx as f64 * 0.12;
                HistoryBar {
                    date: format!("2020-{:02}-{:02}", 2 + idx / 28, idx % 28 + 1),
                    open: Some(close - 0.05),
                    high: Some(close + 0.2),
                    low: Some(close - 0.2),
                    close,
                    volume: Some(1_000_000.0 + idx as f64 * 10_000.0),
                    capital: Some(1_000_000_000.0),
                }
            })
            .collect();
        data.histories.insert("111111.SZ".to_string(), history);

        let result = observe_with_data(
            &data,
            &StockObserveRequest {
                code: "111111".to_string(),
                start_date: "20200201".to_string(),
                end_date: "20200331".to_string(),
                series_limit: 40,
                include_order_book: true,
            },
        )
        .expect("observation should run from the shared data set");

        assert_eq!(result.stock.code, "111111.SZ");
        let financial = result.financial_indicators.as_ref().unwrap();
        assert!(financial
            .notes
            .iter()
            .any(|note| note.contains("季度 EPS 明细来自缓存")));
        let financial_items = &financial.items;
        assert!(financial_items.len() >= 6);
        assert!(financial_items
            .iter()
            .any(|item| item.metric_key.as_deref() == Some("quarterly_eps")
                && item.period.as_deref() == Some("2026Q1")));
        let trend = result.trend.as_ref().unwrap();
        assert!(trend.series.len() <= 40);
        assert!(trend
            .series
            .iter()
            .any(|point| point.k.is_some() && point.d.is_some() && point.j.is_some()));
        assert!(trend.signal.k.is_some());
        assert!(trend.signal.d.is_some());
        assert!(trend.signal.j.is_some());
        let capital_evidence = result.capital_evidence.as_ref().unwrap();
        assert!(capital_evidence.composite_score.is_some());
        assert!(capital_evidence
            .sections
            .iter()
            .any(|section| section.key == "technical_behavior" && section.available));
        assert!(capital_evidence
            .items
            .iter()
            .any(|item| item.category == "technical_behavior"));
        assert!(capital_evidence
            .items
            .iter()
            .any(|item| item.category == "fund_flow" && item.score == Some(72.0)));
        assert!(capital_evidence
            .contributions
            .get("资金流")
            .and_then(|value| value.get("available"))
            .and_then(Value::as_bool)
            .unwrap_or(false));
        let technical_item = capital_evidence
            .items
            .iter()
            .find(|item| item.category == "technical_behavior")
            .unwrap();
        assert!(technical_item.metrics.contains_key("吸筹强度"));
        assert!(technical_item.metrics.contains_key("波段机会"));
        assert!(!capital_evidence
            .notes
            .iter()
            .any(|note| note.contains("待迁移")));
        assert!(result.order_book.is_none());
        assert!(result.notes.iter().any(|note| note.contains("Tauri/Rust")));
    }
    #[test]
    fn observe_with_data_accepts_numeric_capital_metric_values() {
        let mut data = sample_data_set();
        let history: Vec<HistoryBar> = (0..45)
            .map(|idx| {
                let close = 10.0 + idx as f64 * 0.12;
                HistoryBar {
                    date: format!("2020-{:02}-{:02}", 2 + idx / 28, idx % 28 + 1),
                    open: Some(close - 0.05),
                    high: Some(close + 0.2),
                    low: Some(close - 0.2),
                    close,
                    volume: Some(1_000_000.0 + idx as f64 * 10_000.0),
                    capital: Some(1_000_000_000.0),
                }
            })
            .collect();
        data.histories.insert("111111.SZ".to_string(), history);

        let mut payload = serde_json::to_value(ObserveWithDataRequest {
            data,
            request: StockObserveRequest {
                code: "111111.SZ".to_string(),
                start_date: "20200201".to_string(),
                end_date: "20200331".to_string(),
                series_limit: 40,
                include_order_book: false,
            },
        })
        .expect("payload should serialize");
        payload["data"]["capital_evidence"]["111111.SZ"]["items"] = json!([
            {
                "category": "community_sentiment",
                "source": "东方财富股吧",
                "title": "numeric metric cache sample",
                "date": "2026-06-25",
                "metrics": {
                    "阅读数": 6.0,
                    "评论数": 2,
                    "标题": "numeric metric cache sample"
                },
                "sentiment": "uncertain",
                "weight": 0.15,
                "confidence": "低",
                "score": 50.0
            }
        ]);

        let result = observe_with_data_value(payload)
            .expect("numeric metric values in cached capital evidence should not block observe");
        assert!(result.get("trend").is_some());
        let items = result["capital_evidence"]["items"]
            .as_array()
            .expect("capital evidence items");
        let community = items
            .iter()
            .find(|item| {
                item.get("category").and_then(Value::as_str) == Some("community_sentiment")
            })
            .expect("community sentiment item should remain");
        assert_eq!(
            community["metrics"]["阅读数"],
            Value::String("6.0".to_string())
        );
        assert!(
            items
                .iter()
                .any(|item| item.get("category").and_then(Value::as_str)
                    == Some("technical_behavior"))
        );
    }

    #[test]
    fn observe_without_cached_capital_evidence_uses_local_proxy_items() {
        let mut data = sample_data_set();
        data.capital_evidence.clear();
        let result = observe_with_data(
            &data,
            &StockObserveRequest {
                code: "111111.SZ".to_string(),
                start_date: "2020-01-01".to_string(),
                end_date: "2020-01-03".to_string(),
                series_limit: 40,
                include_order_book: false,
            },
        )
        .expect("observation should still run without cached capital evidence");

        let capital_evidence = result.capital_evidence.as_ref().unwrap();
        assert!(capital_evidence.items.iter().any(|item| {
            item.category == "fund_flow" && item.title == "本地量价资金代理" && item.score.is_some()
        }));
        assert!(capital_evidence
            .items
            .iter()
            .any(|item| item.category == "message_sentiment_status"));
        assert!(capital_evidence.sections.iter().any(|section| {
            section.key == "message_sentiment"
                && section
                    .items
                    .iter()
                    .any(|item| item.category == "message_sentiment_status")
        }));
    }

    #[test]
    fn agent_routes_graph_request() {
        let response = run_agent_with_mock("用关系图分析 300750.SZ 产业链选股").unwrap();
        assert_eq!(response.action, "graph_screen");
        assert!(response.reply.contains("不构成投资建议"));
        assert!(response.data.is_some());
    }

    #[test]
    fn agent_routes_trend_request() {
        let response = run_agent_with_mock("用趋势指标筛选上升趋势股票").unwrap();
        assert_eq!(response.action, "trend_screen");
        assert!(response.reply.contains("不构成投资建议"));
        assert!(response.data.is_some());
    }

    #[test]
    fn agent_stream_with_data_emits_status_and_result() {
        let events = run_agent_stream_with_data_events(
            &sample_data_set(),
            "screen bank stocks",
            Some("native-run"),
        );
        assert!(events.iter().any(|event| event.event_type == "status"));
        let result = events
            .iter()
            .find(|event| event.event_type == "result")
            .expect("agent stream should return result event");
        assert_eq!(result.run_id, "native-run");
        assert_eq!(result.response.as_ref().unwrap().action, "screen");
    }

    #[test]
    fn mobile_stock_skill_classifies_sources_with_guardrails() {
        let result = run_mobile_stock_skill(&MobileStockSkillRequest {
            stock_code: "300750.SZ".to_string(),
            stock_name: "宁德时代".to_string(),
            question: "分析近期利好利空".to_string(),
            sources: vec![
                MobileStockSourceItem {
                    title: "宁德时代签订储能订单公告".to_string(),
                    summary: "公司披露新签订单增长，交付节奏改善。".to_string(),
                    source_tier: "filing".to_string(),
                    source_name: "巨潮资讯".to_string(),
                    published_at: Some("2026-06-08".to_string()),
                    source_url: Some("https://example.test/notice".to_string()),
                    evidence: "公司披露新签订单增长，预计对经营产生积极影响。".to_string(),
                },
                MobileStockSourceItem {
                    title: "股吧讨论短线必涨".to_string(),
                    summary: "社区讨论称短线必涨。".to_string(),
                    source_tier: "community".to_string(),
                    source_name: "股吧".to_string(),
                    published_at: Some("2026-06-08".to_string()),
                    source_url: None,
                    evidence: "社区讨论称短线必涨。".to_string(),
                },
            ],
        });

        assert_eq!(result.overview.positive_count, 1);
        assert_eq!(result.overview.unverified_count, 1);
        assert_eq!(result.positive_factors[0].source_tier, "filing");
        assert_eq!(result.unverified_leads[0].label, "unverified");
        assert!(result.positive_factors[0]
            .risk_note
            .contains("不构成投资建议"));
        assert!(result
            .notes
            .iter()
            .any(|note| note.contains("社区来源只作为待验证线索")));
    }

    #[test]
    fn mobile_stock_skill_does_not_invent_without_sources() {
        let result = run_mobile_stock_skill(&MobileStockSkillRequest {
            stock_code: "300750.SZ".to_string(),
            stock_name: "宁德时代".to_string(),
            question: "分析近期利好利空".to_string(),
            sources: Vec::new(),
        });

        assert_eq!(result.overview.overall_label, "neutral");
        assert_eq!(result.overview.positive_count, 0);
        assert!(result.overview.summary.contains("未找到可靠信源"));
        assert!(result
            .notes
            .iter()
            .any(|note| note.contains("未找到可靠信源")));
    }

    #[test]
    fn validates_native_data_set() {
        let summary = validate_data_set(&sample_data_set()).expect("data set should be valid");
        assert_eq!(summary.stock_count, 2);
        assert_eq!(summary.relation_count, 1);
        assert_eq!(summary.history_bar_count, 3);
    }

    #[test]
    fn screens_with_native_data_set() {
        let result = screen_with_data(
            &sample_data_set(),
            &ScreenCriteria {
                max_pe: Some(10.0),
                ..ScreenCriteria::default()
            },
        )
        .expect("native data screen should run");
        assert_eq!(result.returned, 1);
        assert_eq!(result.items[0].stock.code, "111111.SZ");
    }

    #[test]
    fn screens_with_deducted_profit_quality_rule() {
        let result = screen_with_data(
            &sample_data_set(),
            &ScreenCriteria {
                min_deducted_net_profit_billion: Some(0.0),
                min_deducted_net_profit_growth_rate: Some(10.0),
                ..ScreenCriteria::default()
            },
        )
        .expect("native data screen should run");

        assert_eq!(result.returned, 1);
        assert_eq!(result.items[0].stock.code, "111111.SZ");
        assert!(result.items[0]
            .reasons
            .contains(&"deducted_net_profit_ok".to_string()));
        assert!(result.items[0]
            .reasons
            .contains(&"deducted_net_profit_growth_rate_ok".to_string()));
    }

    #[test]
    fn screens_native_data_with_partial_industry_match() {
        let mut data = sample_data_set();
        data.stocks[0].industry = "银行服务".to_string();

        let result = screen_with_data(
            &data,
            &ScreenCriteria {
                industry: Some("银行".to_string()),
                ..ScreenCriteria::default()
            },
        )
        .expect("native data screen should run");

        assert_eq!(result.returned, 1);
        assert_eq!(result.items[0].stock.code, "111111.SZ");
    }

    #[test]
    fn selected_industry_does_not_match_empty_stock_industry() {
        let universe = vec![StockItem {
            code: "000001.SZ".to_string(),
            name: "平安银行".to_string(),
            industry: "".to_string(),
            is_st: false,
            price: 11.0,
            pe: Some(5.0),
            pb: Some(0.8),
            roe: Some(0.16),
            market_cap_billion: Some(240.0),
            dividend_yield: None,
            deducted_net_profit_billion: None,
            deducted_net_profit_margin: None,
            deducted_net_profit_growth_rate: None,
            change_pct: None,
            volume: None,
            amount: None,
            turnover_rate: None,
            volume_ratio: None,
            quote_time: None,
        }];

        let result = screen_stocks(
            &universe,
            &ScreenCriteria {
                industry: Some("银行".to_string()),
                ..ScreenCriteria::default()
            },
        );

        assert_eq!(result.returned, 0);
    }

    #[test]
    fn optional_metric_sort_keeps_missing_values_last() {
        let universe = vec![
            StockItem {
                code: "000001.SZ".to_string(),
                name: "平安银行".to_string(),
                industry: "银行".to_string(),
                is_st: false,
                price: 11.0,
                pe: None,
                pb: None,
                roe: None,
                market_cap_billion: None,
                dividend_yield: None,
                deducted_net_profit_billion: None,
                deducted_net_profit_margin: None,
                deducted_net_profit_growth_rate: None,
                change_pct: None,
                volume: None,
                amount: None,
                turnover_rate: None,
                volume_ratio: None,
                quote_time: None,
            },
            StockItem {
                code: "600000.SH".to_string(),
                name: "浦发银行".to_string(),
                industry: "银行".to_string(),
                is_st: false,
                price: 9.0,
                pe: Some(5.0),
                pb: Some(0.6),
                roe: Some(0.11),
                market_cap_billion: Some(170.0),
                dividend_yield: None,
                deducted_net_profit_billion: None,
                deducted_net_profit_margin: None,
                deducted_net_profit_growth_rate: None,
                change_pct: None,
                volume: None,
                amount: None,
                turnover_rate: None,
                volume_ratio: None,
                quote_time: None,
            },
            StockItem {
                code: "600036.SH".to_string(),
                name: "招商银行".to_string(),
                industry: "银行".to_string(),
                is_st: false,
                price: 31.0,
                pe: Some(7.0),
                pb: Some(0.9),
                roe: Some(0.14),
                market_cap_billion: Some(900.0),
                dividend_yield: None,
                deducted_net_profit_billion: None,
                deducted_net_profit_margin: None,
                deducted_net_profit_growth_rate: None,
                change_pct: None,
                volume: None,
                amount: None,
                turnover_rate: None,
                volume_ratio: None,
                quote_time: None,
            },
        ];

        let ascending = screen_stocks(
            &universe,
            &ScreenCriteria {
                sort_by: "pe".to_string(),
                sort_dir: "asc".to_string(),
                ..ScreenCriteria::default()
            },
        );
        let descending = screen_stocks(
            &universe,
            &ScreenCriteria {
                sort_by: "pe".to_string(),
                sort_dir: "desc".to_string(),
                ..ScreenCriteria::default()
            },
        );

        assert_eq!(
            ascending
                .items
                .iter()
                .map(|item| item.stock.code.as_str())
                .collect::<Vec<_>>(),
            vec!["600000.SH", "600036.SH", "000001.SZ"]
        );
        assert_eq!(
            descending
                .items
                .iter()
                .map(|item| item.stock.code.as_str())
                .collect::<Vec<_>>(),
            vec!["600036.SH", "600000.SH", "000001.SZ"]
        );
    }

    #[test]
    fn rotation_score_profile_uses_intraday_market_heat() {
        let base = StockItem {
            code: "300001.SZ".to_string(),
            name: "quality-one".to_string(),
            industry: "consumer".to_string(),
            price: 10.0,
            pe: Some(10.0),
            pb: Some(1.0),
            roe: Some(0.12),
            market_cap_billion: Some(120.0),
            ..StockItem::default()
        };
        let mut quiet = base.clone();
        quiet.code = "300001.SZ".to_string();
        quiet.change_pct = Some(0.001);
        quiet.volume_ratio = Some(0.8);
        quiet.turnover_rate = Some(0.005);
        quiet.amount = Some(80_000_000.0);

        let mut active = base.clone();
        active.code = "300002.SZ".to_string();
        active.change_pct = Some(0.075);
        active.volume_ratio = Some(2.6);
        active.turnover_rate = Some(0.07);
        active.amount = Some(1_500_000_000.0);

        let rotation = screen_stocks(
            &[quiet.clone(), active.clone()],
            &ScreenCriteria {
                sort_by: "score".to_string(),
                sort_dir: "desc".to_string(),
                score_profile: "rotation".to_string(),
                ..ScreenCriteria::default()
            },
        );
        let quality = screen_stocks(
            &[quiet, active],
            &ScreenCriteria {
                sort_by: "score".to_string(),
                sort_dir: "desc".to_string(),
                score_profile: "quality".to_string(),
                ..ScreenCriteria::default()
            },
        );

        assert_eq!(rotation.items[0].stock.code, "300002.SZ");
        assert!(rotation.items[0].factor_scores.contains_key("market_heat"));
        assert_eq!(quality.items[0].stock.code, "300001.SZ");
        assert!(!quality.items[0].factor_scores.contains_key("market_heat"));
    }

    #[test]
    fn score_sort_biases_toward_hot_energy_and_tech_sectors() {
        let base = StockItem {
            code: "000001.SZ".to_string(),
            name: "平安银行".to_string(),
            industry: "银行".to_string(),
            is_st: false,
            price: 10.0,
            pe: Some(10.0),
            pb: Some(1.0),
            roe: Some(0.1),
            market_cap_billion: Some(100.0),
            dividend_yield: None,
            deducted_net_profit_billion: None,
            deducted_net_profit_margin: None,
            deducted_net_profit_growth_rate: None,
            change_pct: None,
            volume: None,
            amount: None,
            turnover_rate: None,
            volume_ratio: None,
            quote_time: None,
        };
        let mut chip = base.clone();
        chip.code = "688001.SH".to_string();
        chip.name = "芯片公司".to_string();
        chip.industry = "半导体".to_string();
        let mut solar = base.clone();
        solar.code = "601012.SH".to_string();
        solar.name = "光伏公司".to_string();
        solar.industry = "光伏".to_string();

        let result = screen_stocks(
            &[base, chip, solar],
            &ScreenCriteria {
                sort_by: "score".to_string(),
                sort_dir: "desc".to_string(),
                ..ScreenCriteria::default()
            },
        );

        assert_eq!(
            result
                .items
                .iter()
                .take(2)
                .map(|item| item.stock.code.as_str())
                .collect::<Vec<_>>(),
            vec!["688001.SH", "601012.SH"]
        );
    }

    #[test]
    fn score_sort_promotes_hot_tech_and_energy_candidates_into_limited_results() {
        let bank = StockItem {
            code: "000001.SZ".to_string(),
            name: "高分银行".to_string(),
            industry: "银行".to_string(),
            is_st: false,
            price: 10.0,
            pe: Some(2.0),
            pb: Some(0.2),
            roe: Some(0.3),
            market_cap_billion: Some(100.0),
            dividend_yield: None,
            deducted_net_profit_billion: None,
            deducted_net_profit_margin: None,
            deducted_net_profit_growth_rate: None,
            change_pct: None,
            volume: None,
            amount: None,
            turnover_rate: None,
            volume_ratio: None,
            quote_time: None,
        };
        let mut ordinary_bank = bank.clone();
        ordinary_bank.code = "600000.SH".to_string();
        ordinary_bank.name = "普通银行".to_string();
        ordinary_bank.pe = Some(3.0);
        ordinary_bank.pb = Some(0.3);
        ordinary_bank.roe = Some(0.2);
        let mut chip = bank.clone();
        chip.code = "688001.SH".to_string();
        chip.name = "芯片公司".to_string();
        chip.industry = "半导体".to_string();
        chip.pe = Some(60.0);
        chip.pb = Some(8.0);
        chip.roe = Some(0.03);
        let mut solar = chip.clone();
        solar.code = "601012.SH".to_string();
        solar.name = "光伏公司".to_string();
        solar.industry = "光伏".to_string();
        solar.pe = Some(50.0);
        solar.pb = Some(7.0);

        let result = screen_stocks(
            &[bank, ordinary_bank, chip, solar],
            &ScreenCriteria {
                sort_by: "score".to_string(),
                sort_dir: "desc".to_string(),
                limit: 2,
                ..ScreenCriteria::default()
            },
        );

        assert_eq!(
            result
                .items
                .iter()
                .map(|item| item.stock.code.as_str())
                .collect::<Vec<_>>(),
            vec!["688001.SH", "601012.SH"]
        );
    }

    #[test]
    fn score_sort_promotes_duofuduo_like_hot_themes_and_deprioritizes_bank_infra() {
        let bank = StockItem {
            code: "000001.SZ".to_string(),
            name: "高分银行".to_string(),
            industry: "银行".to_string(),
            is_st: false,
            price: 10.0,
            pe: Some(2.0),
            pb: Some(0.2),
            roe: Some(0.3),
            market_cap_billion: Some(100.0),
            dividend_yield: None,
            deducted_net_profit_billion: None,
            deducted_net_profit_margin: None,
            deducted_net_profit_growth_rate: None,
            change_pct: None,
            volume: None,
            amount: None,
            turnover_rate: None,
            volume_ratio: None,
            quote_time: None,
        };
        let mut infra = bank.clone();
        infra.code = "601668.SH".to_string();
        infra.name = "中国建筑".to_string();
        infra.industry = "建筑装饰".to_string();
        infra.pe = Some(3.0);
        infra.pb = Some(0.4);
        infra.roe = Some(0.2);
        let mut duofuduo = bank.clone();
        duofuduo.code = "002407.SZ".to_string();
        duofuduo.name = "多氟多".to_string();
        duofuduo.industry = "化工".to_string();
        duofuduo.pe = Some(70.0);
        duofuduo.pb = Some(8.0);
        duofuduo.roe = Some(0.03);
        let mut material = duofuduo.clone();
        material.code = "002408.SZ".to_string();
        material.name = "氟材料公司".to_string();
        material.industry = "锂电材料".to_string();
        let mut chip = duofuduo.clone();
        chip.code = "688001.SH".to_string();
        chip.name = "芯片公司".to_string();
        chip.industry = "半导体".to_string();
        chip.pe = Some(60.0);
        let mut solar = chip.clone();
        solar.code = "601012.SH".to_string();
        solar.name = "光伏公司".to_string();
        solar.industry = "光伏".to_string();
        solar.pe = Some(50.0);
        solar.pb = Some(7.0);

        let result = screen_stocks(
            &[bank, infra, duofuduo, material, chip, solar],
            &ScreenCriteria {
                sort_by: "score".to_string(),
                sort_dir: "desc".to_string(),
                limit: 3,
                ..ScreenCriteria::default()
            },
        );

        let codes = result
            .items
            .iter()
            .map(|item| item.stock.code.as_str())
            .collect::<Vec<_>>();
        assert_eq!(codes, vec!["002408.SZ", "688001.SH", "601012.SH"]);
        assert!(!codes.contains(&"002407.SZ"));
        assert!(!codes.contains(&"000001.SZ"));
        assert!(!codes.contains(&"601668.SH"));
    }

    #[test]
    fn score_sort_promotes_medical_and_game_candidates_with_other_hot_sectors() {
        let bank = StockItem {
            code: "000001.SZ".to_string(),
            name: "高分银行".to_string(),
            industry: "银行".to_string(),
            is_st: false,
            price: 10.0,
            pe: Some(2.0),
            pb: Some(0.2),
            roe: Some(0.3),
            market_cap_billion: Some(100.0),
            dividend_yield: None,
            deducted_net_profit_billion: None,
            deducted_net_profit_margin: None,
            deducted_net_profit_growth_rate: None,
            change_pct: None,
            volume: None,
            amount: None,
            turnover_rate: None,
            volume_ratio: None,
            quote_time: None,
        };
        let mut infra = bank.clone();
        infra.code = "601668.SH".to_string();
        infra.name = "中国建筑".to_string();
        infra.industry = "建筑装饰".to_string();
        infra.pe = Some(3.0);
        infra.pb = Some(0.4);
        infra.roe = Some(0.2);
        let mut material = bank.clone();
        material.code = "002408.SZ".to_string();
        material.name = "氟材料公司".to_string();
        material.industry = "锂电材料".to_string();
        material.pe = Some(70.0);
        material.pb = Some(8.0);
        material.roe = Some(0.03);
        let mut chip = material.clone();
        chip.code = "688001.SH".to_string();
        chip.name = "芯片公司".to_string();
        chip.industry = "半导体".to_string();
        chip.pe = Some(60.0);
        let mut solar = chip.clone();
        solar.code = "601012.SH".to_string();
        solar.name = "光伏公司".to_string();
        solar.industry = "光伏".to_string();
        solar.pe = Some(50.0);
        solar.pb = Some(7.0);
        let mut game = chip.clone();
        game.code = "002555.SZ".to_string();
        game.name = "游戏公司".to_string();
        game.industry = "网络游戏".to_string();
        game.pe = Some(80.0);
        game.pb = Some(9.0);
        game.roe = Some(0.02);

        let mut medical = chip.clone();
        medical.code = "300015.SZ".to_string();
        medical.name = "创新药公司".to_string();
        medical.industry = "医疗器械".to_string();
        medical.pe = Some(75.0);
        medical.pb = Some(8.5);
        medical.roe = Some(0.025);

        let result = screen_stocks(
            &[bank, infra, material, chip, solar, medical, game],
            &ScreenCriteria {
                sort_by: "score".to_string(),
                sort_dir: "desc".to_string(),
                limit: 5,
                ..ScreenCriteria::default()
            },
        );

        let codes = result
            .items
            .iter()
            .map(|item| item.stock.code.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            codes,
            vec![
                "002408.SZ",
                "688001.SH",
                "601012.SH",
                "300015.SZ",
                "002555.SZ"
            ]
        );
    }

    #[test]
    fn backtests_with_native_history() {
        let result = backtest_with_data(
            &sample_data_set(),
            &BacktestRequest {
                criteria: ScreenCriteria {
                    max_pe: Some(10.0),
                    ..ScreenCriteria::default()
                },
                source: default_backtest_source(),
                stock_codes: Vec::new(),
                start_date: "20200101".to_string(),
                end_date: "20200103".to_string(),
                top_n: 1,
                initial_cash: 1000.0,
            },
        )
        .expect("native data backtest should run");
        assert_eq!(result.equity_curve.len(), 3);
        assert_eq!(result.equity_curve.last().unwrap().equity, 1200.0);
    }

    #[test]
    fn backtests_watchlist_codes_in_saved_order() {
        let result = backtest_with_data(
            &sample_data_set(),
            &BacktestRequest {
                criteria: ScreenCriteria::default(),
                source: "watchlist".to_string(),
                stock_codes: vec!["222222.SZ".to_string(), "111111.SZ".to_string()],
                start_date: "20200101".to_string(),
                end_date: "20200103".to_string(),
                top_n: 10,
                initial_cash: 1000.0,
            },
        )
        .expect("watchlist backtest should run");

        assert_eq!(result.symbols, vec!["222222.SZ", "111111.SZ"]);
        assert!(result.notes.iter().any(|note| note.contains("自选观察池")));
    }
}
