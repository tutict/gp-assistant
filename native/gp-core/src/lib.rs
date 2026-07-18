use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt,
    sync::OnceLock,
};

use chrono::{Datelike, Days, Local, NaiveDate, Weekday};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

mod volatility;
pub use volatility::{
    AtrSnapshot, BollingerBandsSnapshot, ChaikinVolatilitySnapshot, DonchianChannelSnapshot,
    IndicatorUnavailable, KeltnerChannelSnapshot, RviSnapshot, VolatilitySnapshot,
};

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
    pub latest_eps: Option<f64>,
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
            latest_eps: None,
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
    pub quality_score: f64,
    #[serde(default)]
    pub trend_score: f64,
    #[serde(default)]
    pub risk_score: f64,
    #[serde(default)]
    pub balanced_score: f64,
    #[serde(default)]
    pub factor_scores: BTreeMap<String, f64>,
    #[serde(default)]
    pub score_breakdown: Vec<ScoreContribution>,
    #[serde(default)]
    pub score_explanation: String,
    #[serde(default)]
    pub reason_tags: Vec<String>,
    #[serde(default)]
    pub risk_tags: Vec<String>,
    #[serde(default)]
    pub suitable_periods: Vec<String>,
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
    #[serde(default)]
    pub seed_query: String,
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
            seed_query: String::new(),
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
    #[serde(default = "default_backtest_strategy_mode")]
    pub strategy_mode: String,
    #[serde(default)]
    pub stock_codes: Vec<String>,
    pub start_date: String,
    pub end_date: String,
    #[serde(default = "default_top_n")]
    pub top_n: usize,
    #[serde(default = "default_initial_cash")]
    pub initial_cash: f64,
    #[serde(default = "default_rebalance_frequency")]
    pub rebalance_frequency: String,
    #[serde(default = "default_transaction_cost_bps")]
    pub transaction_cost_bps: f64,
    #[serde(default = "default_backtest_benchmark")]
    pub benchmark: String,
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
    pub benchmark_total_return: Option<f64>,
    pub benchmark_annualized_return: Option<f64>,
    pub benchmark_max_drawdown: Option<f64>,
    pub excess_return: Option<f64>,
    pub total_transaction_cost: f64,
    pub total_turnover: f64,
    pub rebalance_count: usize,
    #[serde(default)]
    pub oos_fold_count: usize,
    #[serde(default)]
    pub evaluated_selection_count: usize,
    #[serde(default)]
    pub selection_hit_count: usize,
    #[serde(default)]
    pub precision_at_n: Option<f64>,
    #[serde(default = "default_backtest_strategy_mode")]
    pub strategy_mode: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WalkForwardFold {
    pub selection_date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluation_end_date: Option<String>,
    #[serde(default)]
    pub selected_symbols: Vec<String>,
    #[serde(default)]
    pub eligible_symbol_count: usize,
    #[serde(default)]
    pub evaluated_selection_count: usize,
    #[serde(default)]
    pub hit_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precision_at_n: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub average_forward_return: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub benchmark_forward_return: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub average_excess_return: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BacktestResult {
    pub metrics: BacktestMetrics,
    pub equity_curve: Vec<EquityPoint>,
    #[serde(default)]
    pub benchmark_curve: Vec<EquityPoint>,
    pub symbols: Vec<String>,
    #[serde(default)]
    pub benchmark_symbols: Vec<String>,
    #[serde(default)]
    pub rebalance_dates: Vec<String>,
    #[serde(default)]
    pub walk_forward_folds: Vec<WalkForwardFold>,
    #[serde(default)]
    pub volatility_snapshots: Vec<VolatilitySnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volatility_message: Option<String>,
    #[serde(default = "default_backtest_strategy_mode")]
    pub strategy_mode: String,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentRequest {
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentSkill {
    pub key: String,
    pub title: String,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Value,
    pub platforms: Vec<String>,
    pub evidence_level: String,
    pub fallback: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentIntent {
    pub kind: String,
    pub query: String,
    #[serde(default)]
    pub symbols: Vec<String>,
    #[serde(default)]
    pub window: Option<String>,
    pub depth: String,
    pub mode: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentToolCall {
    pub id: String,
    pub tool: String,
    pub label: String,
    pub status: String,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub output_summary: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentEvidenceItem {
    pub title: String,
    pub source: String,
    pub level: String,
    pub summary: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentAnswerSection {
    pub title: String,
    #[serde(default)]
    pub bullets: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AgentContext {
    #[serde(default)]
    pub watchlist: Vec<AgentWatchlistItem>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AgentWatchlistItem {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub industry: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentResponse {
    pub reply: String,
    pub action: String,
    #[serde(default)]
    pub intent: Option<AgentIntent>,
    #[serde(default)]
    pub tool_calls: Vec<AgentToolCall>,
    #[serde(default)]
    pub evidence_summary: Vec<AgentEvidenceItem>,
    #[serde(default)]
    pub answer_sections: Vec<AgentAnswerSection>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<String>,
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
    pub operating_revenue_billion: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operating_revenue_yoy: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_net_profit_billion: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_net_profit_yoy: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gross_margin: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_margin: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roe: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_liability_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goodwill_to_net_assets: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pledged_share_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dividend_yield: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dividend_payout_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goodwill_period: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pledged_share_period: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dividend_period: Option<String>,
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
pub struct StockFactorSnapshot {
    pub date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub industry: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_st: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_listed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_tradable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pe: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pb: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roe: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_cap_billion: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dividend_yield: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deducted_net_profit_billion: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deducted_net_profit_margin: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deducted_net_profit_growth_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_pct: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turnover_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
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
    pub factor_snapshots: HashMap<String, Vec<StockFactorSnapshot>>,
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
    pub factor_snapshot_symbol_count: usize,
    #[serde(default)]
    pub factor_snapshot_count: usize,
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
    pub payload: Option<Value>,
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
    open: f64,
    high: f64,
    low: f64,
    volume: f64,
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
    fn stocks(&self) -> CoreResult<&[StockItem]>;

    fn relations(&self) -> CoreResult<&[StockRelation]> {
        Ok(&[])
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

    fn get_factor_snapshots(&self, _code: &str) -> CoreResult<Vec<StockFactorSnapshot>> {
        Ok(Vec::new())
    }

    fn factor_snapshot_codes(&self) -> CoreResult<Vec<String>> {
        Ok(Vec::new())
    }

    fn get_capital_evidence(&self, _code: &str) -> Option<CapitalEvidenceResult> {
        None
    }
}

pub struct MockDataSource;

impl MarketDataSource for MockDataSource {
    fn stocks(&self) -> CoreResult<&[StockItem]> {
        static STOCKS: OnceLock<Vec<StockItem>> = OnceLock::new();
        Ok(STOCKS.get_or_init(mock_stocks).as_slice())
    }

    fn relations(&self) -> CoreResult<&[StockRelation]> {
        static RELATIONS: OnceLock<Vec<StockRelation>> = OnceLock::new();
        Ok(RELATIONS.get_or_init(mock_relations).as_slice())
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

pub struct StaticDataSource<'a> {
    data: &'a CoreDataSet,
    stock_override: Option<&'a [StockItem]>,
    history_override: Option<&'a HashMap<String, Vec<HistoryBar>>>,
}

impl<'a> StaticDataSource<'a> {
    pub fn new(data: &'a CoreDataSet) -> Self {
        Self {
            data,
            stock_override: None,
            history_override: None,
        }
    }

    pub fn with_stocks(data: &'a CoreDataSet, stocks: &'a [StockItem]) -> Self {
        Self {
            data,
            stock_override: Some(stocks),
            history_override: None,
        }
    }

    pub fn with_overrides(
        data: &'a CoreDataSet,
        stocks: Option<&'a [StockItem]>,
        histories: Option<&'a HashMap<String, Vec<HistoryBar>>>,
    ) -> Self {
        Self {
            data,
            stock_override: stocks,
            history_override: histories,
        }
    }
}

impl MarketDataSource for StaticDataSource<'_> {
    fn stocks(&self) -> CoreResult<&[StockItem]> {
        Ok(self.stock_override.unwrap_or(&self.data.stocks))
    }

    fn relations(&self) -> CoreResult<&[StockRelation]> {
        Ok(&self.data.relations)
    }

    fn get_history(
        &self,
        code: &str,
        start_date: &str,
        end_date: &str,
    ) -> CoreResult<Vec<HistoryBar>> {
        let normalized = normalize_stock_code(code);
        if let Some(history) = self
            .history_override
            .and_then(|histories| histories.get(code).or_else(|| histories.get(&normalized)))
        {
            return history_bars_in_range(history, start_date, end_date);
        }
        let Some(history) = self
            .data
            .histories
            .get(code)
            .or_else(|| self.data.histories.get(&normalized))
        else {
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

    fn get_factor_snapshots(&self, code: &str) -> CoreResult<Vec<StockFactorSnapshot>> {
        let normalized = normalize_stock_code(code);
        Ok(self
            .data
            .factor_snapshots
            .get(code)
            .cloned()
            .or_else(|| self.data.factor_snapshots.get(&normalized).cloned())
            .unwrap_or_default())
    }

    fn factor_snapshot_codes(&self) -> CoreResult<Vec<String>> {
        let mut codes = self
            .data
            .factor_snapshots
            .keys()
            .map(|code| normalize_stock_code(code))
            .collect::<Vec<_>>();
        codes.sort();
        codes.dedup();
        Ok(codes)
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
    "balanced".to_string()
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
    "balanced_swing".to_string()
}

fn default_trend_signal_type() -> String {
    "trend_continuation".to_string()
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

fn clamp_series_limit(limit: usize) -> usize {
    limit.clamp(20, 10_000)
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

fn default_rebalance_frequency() -> String {
    "monthly".to_string()
}

fn default_transaction_cost_bps() -> f64 {
    10.0
}

fn default_backtest_benchmark() -> String {
    "candidate_equal_weight".to_string()
}

fn default_backtest_strategy_mode() -> String {
    "walk_forward".to_string()
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

pub fn screen_with_mock(criteria: &ScreenCriteria) -> ScreenResult {
    screen_with_source(&MockDataSource, criteria).expect("mock data source should be valid")
}

pub fn sector_screen_with_mock(request: &SectorScreenRequest) -> SectorScreenResult {
    let universe = mock_stocks();
    sector_screen_stocks(&universe, request)
}
pub fn screen_with_data(data: &CoreDataSet, criteria: &ScreenCriteria) -> CoreResult<ScreenResult> {
    let source = StaticDataSource::new(data);
    screen_with_source(&source, criteria)
}

pub fn screen_with_source(
    source: &impl MarketDataSource,
    criteria: &ScreenCriteria,
) -> CoreResult<ScreenResult> {
    let universe = source.stocks()?;
    Ok(screen_stocks(&universe, criteria))
}

pub fn sector_screen_with_data(
    data: &CoreDataSet,
    request: &SectorScreenRequest,
) -> CoreResult<SectorScreenResult> {
    let source = StaticDataSource::new(data);
    let universe = source.stocks()?;
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
        notes.push("基准为候选池买入持有等权曲线，用于衡量调仓与成本后的超额收益。".to_string());
    } else {
        notes.push("基准为候选池买入持有等权曲线，用于衡量调仓与成本后的超额收益。".to_string());
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
    let source = StaticDataSource::new(data);
    graph_screen_with_source(&source, request)
}

pub fn graph_screen_with_source(
    source: &impl MarketDataSource,
    request: &GraphScreenRequest,
) -> CoreResult<GraphScreenResult> {
    let universe = source.stocks()?;
    let relations = source.relations()?;
    Ok(graph_screen_stocks(&universe, &relations, request))
}

pub fn backtest_with_mock(request: &BacktestRequest) -> CoreResult<BacktestResult> {
    backtest_with_source(&MockDataSource, request)
}

pub fn backtest_with_data(
    data: &CoreDataSet,
    request: &BacktestRequest,
) -> CoreResult<BacktestResult> {
    let source = StaticDataSource::new(data);
    backtest_with_source(&source, request)
}

pub fn backtest_selected_symbols(universe: &[StockItem], request: &BacktestRequest) -> Vec<String> {
    selected_backtest_items_from_universe(universe, request)
        .0
        .into_iter()
        .map(|item| item.stock.code)
        .collect()
}

pub fn backtest_with_source(
    source: &impl MarketDataSource,
    request: &BacktestRequest,
) -> CoreResult<BacktestResult> {
    let mut universe = source.stocks()?.to_vec();
    let strategy_mode = normalize_backtest_strategy_mode(&request.strategy_mode);
    let source_mode = normalize_backtest_source(&request.source);
    if strategy_mode == "walk_forward" && source_mode != "watchlist" {
        let mut known_codes = universe
            .iter()
            .map(|stock| stock.code.to_uppercase())
            .collect::<HashSet<_>>();
        for code in source.factor_snapshot_codes()? {
            let normalized = normalize_stock_code(&code);
            if normalized.is_empty() || !known_codes.insert(normalized.to_uppercase()) {
                continue;
            }
            universe.push(StockItem {
                code: normalized,
                ..StockItem::default()
            });
        }
    }
    let (selected, selection_notes) =
        if strategy_mode == "walk_forward" && source_mode != "watchlist" {
            (Vec::new(), Vec::new())
        } else {
            selected_backtest_items(&universe, request)
        };
    let symbols: Vec<String> = if strategy_mode == "walk_forward" && source_mode != "watchlist" {
        universe.iter().map(|item| item.code.clone()).collect()
    } else {
        selected
            .iter()
            .map(|item| item.stock.code.clone())
            .collect()
    };

    let history_symbols = if strategy_mode == "walk_forward" {
        strict_walk_forward_history_symbols(
            source,
            &symbols,
            &request.start_date,
            &request.end_date,
        )?
    } else {
        symbols.clone()
    };
    let histories = load_backtest_histories(
        source,
        &history_symbols,
        &request.start_date,
        &request.end_date,
    )?;
    if strategy_mode == "walk_forward" {
        let available_codes = histories
            .iter()
            .map(|history| history.code.to_uppercase())
            .collect::<HashSet<_>>();
        let missing_history_codes = history_symbols
            .iter()
            .filter(|code| !available_codes.contains(&code.to_uppercase()))
            .take(8)
            .cloned()
            .collect::<Vec<_>>();
        if !missing_history_codes.is_empty() {
            return Err(CoreError::new(format!(
                "walk_forward 严格回测缺少当期上市候选的历史行情：{}",
                missing_history_codes.join("、")
            )));
        }
    }
    if histories.is_empty() {
        if !symbols.is_empty() {
            return Err(CoreError::new(format!(
                "回测区间缺少已选 {} 只股票的历史日线，无法计算净值曲线。请联网重试或先刷新行情缓存。",
                symbols.len()
            )));
        }
        let reported_symbols = if strategy_mode == "walk_forward" {
            Vec::new()
        } else {
            symbols
        };
        return Ok(BacktestResult {
            metrics: empty_backtest_metrics(reported_symbols.len(), &strategy_mode),
            equity_curve: Vec::new(),
            benchmark_curve: Vec::new(),
            symbols: reported_symbols,
            benchmark_symbols: Vec::new(),
            rebalance_dates: Vec::new(),
            walk_forward_folds: Vec::new(),
            volatility_snapshots: Vec::new(),
            volatility_message: Some("回测区间没有可用于波动率计算的标的日线。".to_string()),
            strategy_mode,
            notes: selection_notes,
        });
    }

    let initial_cash = sanitize_initial_cash(request.initial_cash);
    let cost_rate = sanitize_transaction_cost_rate(request.transaction_cost_bps);
    let rebalance_frequency = normalize_rebalance_frequency(&request.rebalance_frequency);
    let simulation = match strategy_mode.as_str() {
        "walk_forward" => simulate_walk_forward_portfolio(
            source,
            &universe,
            request,
            &histories,
            initial_cash,
            cost_rate,
            &rebalance_frequency,
        )?,
        _ => simulate_equal_weight_portfolio(
            &histories,
            initial_cash,
            cost_rate,
            &rebalance_frequency,
        ),
    };
    let benchmark_enabled = normalize_backtest_benchmark(&request.benchmark) != "none";
    let benchmark_curve = if benchmark_enabled {
        equal_weight_benchmark_curve(&histories, initial_cash)
    } else {
        Vec::new()
    };

    let (total_return, annualized_return, max_drawdown) =
        backtest_metrics(&simulation.equity_curve, initial_cash);
    let (benchmark_total_return, benchmark_annualized_return, benchmark_max_drawdown) =
        if benchmark_curve.is_empty() {
            (None, None, None)
        } else {
            let (total, annualized, drawdown) = backtest_metrics(&benchmark_curve, initial_cash);
            (Some(total), annualized, drawdown)
        };
    let excess_return =
        benchmark_total_return.map(|benchmark_return| total_return - benchmark_return);
    let available_symbols: Vec<String> = histories
        .iter()
        .map(|history| history.code.clone())
        .collect();
    let walk_forward_folds = if strategy_mode == "walk_forward" {
        evaluate_walk_forward_folds(&histories, &simulation.selections)
    } else {
        Vec::new()
    };
    let reported_symbols = if strategy_mode == "walk_forward" {
        selected_symbols_from_rebalances(&histories, &simulation.selections)
    } else {
        available_symbols.clone()
    };
    let volatility_symbols = if strategy_mode == "walk_forward" {
        selected_symbols_from_latest_rebalance(&histories, &simulation.selections)
    } else {
        reported_symbols.clone()
    };
    let evaluated_selection_count = walk_forward_folds
        .iter()
        .map(|fold| fold.evaluated_selection_count)
        .sum::<usize>();
    let selection_hit_count = walk_forward_folds
        .iter()
        .map(|fold| fold.hit_count)
        .sum::<usize>();
    let precision_at_n = (evaluated_selection_count > 0)
        .then_some(selection_hit_count as f64 / evaluated_selection_count as f64);
    let oos_fold_count = walk_forward_folds
        .iter()
        .filter(|fold| fold.precision_at_n.is_some())
        .count();

    let mut notes = selection_notes;
    let missing_history_count = symbols.len().saturating_sub(available_symbols.len());
    if missing_history_count > 0 {
        notes.push(format!(
            "{} 只标的缺少有效历史行情，已从回测组合中剔除。",
            missing_history_count
        ));
    }
    notes.push(format!(
        "回测采用{}等权组合，交易成本为{:.1}bps。",
        backtest_rebalance_note(&rebalance_frequency),
        request.transaction_cost_bps.clamp(0.0, 500.0)
    ));
    notes.push(backtest_strategy_mode_note(&strategy_mode).to_string());
    if strategy_mode == "walk_forward" {
        notes.push(format!(
            "滚动样本外评估共 {} 折，{} 个入选样本可计算相对候选池基准的命中率。",
            oos_fold_count, evaluated_selection_count
        ));
    }
    if benchmark_enabled {
        notes.push("基准为候选池买入持有等权曲线，用于衡量调仓与成本后的超额收益。".to_string());
    }
    let history_by_code = histories
        .iter()
        .map(|history| (history.code.to_uppercase(), history))
        .collect::<HashMap<_, _>>();
    let volatility_snapshots = volatility_symbols
        .iter()
        .filter_map(|symbol| {
            history_by_code
                .get(&symbol.to_uppercase())
                .and_then(|history| {
                    volatility::calculate_volatility_snapshot(symbol, &history.bars)
                })
        })
        .collect::<Vec<_>>();
    let volatility_message = volatility_snapshots.is_empty().then(|| {
        if strategy_mode == "walk_forward" && volatility_symbols.is_empty() {
            "Walk-forward 末次调仓没有符合条件的标的。".to_string()
        } else if volatility_symbols.is_empty() {
            "候选快照没有符合条件的标的。".to_string()
        } else {
            "区间末标的没有可解析的日线日期。".to_string()
        }
    });
    notes.push(
        "波动率快照按区间末可见日线计算：ATR14、布林20/2、唐奇安20、凯尔特纳EMA20+2×ATR10、Chaikin 10/10、RVI14；历史不足的指标保持缺失。"
            .to_string(),
    );

    Ok(BacktestResult {
        metrics: BacktestMetrics {
            total_return,
            annualized_return,
            max_drawdown,
            num_stocks: reported_symbols.len(),
            benchmark_total_return,
            benchmark_annualized_return,
            benchmark_max_drawdown,
            excess_return,
            total_transaction_cost: simulation.total_transaction_cost,
            total_turnover: simulation.total_turnover,
            rebalance_count: simulation.rebalance_dates.len(),
            oos_fold_count,
            evaluated_selection_count,
            selection_hit_count,
            precision_at_n,
            strategy_mode: strategy_mode.clone(),
        },
        equity_curve: simulation.equity_curve,
        benchmark_curve,
        symbols: reported_symbols,
        benchmark_symbols: if benchmark_enabled {
            available_symbols
        } else {
            Vec::new()
        },
        rebalance_dates: simulation
            .rebalance_dates
            .iter()
            .map(|date| date.format("%Y-%m-%d").to_string())
            .collect(),
        walk_forward_folds,
        volatility_snapshots,
        volatility_message,
        strategy_mode,
        notes,
    })
}
#[derive(Clone, Debug)]
struct BacktestHistory {
    code: String,
    prices: BTreeMap<NaiveDate, f64>,
    bars: Vec<HistoryBar>,
}

#[derive(Clone, Debug)]
struct PortfolioSimulation {
    equity_curve: Vec<EquityPoint>,
    rebalance_dates: Vec<NaiveDate>,
    total_transaction_cost: f64,
    total_turnover: f64,
    selections: Vec<RebalanceSelection>,
}

#[derive(Clone, Debug)]
struct ActiveSelection {
    selected_indices: Vec<usize>,
    eligible_indices: Vec<usize>,
}

#[derive(Clone, Debug)]
struct RebalanceSelection {
    date: NaiveDate,
    selected_indices: Vec<usize>,
    eligible_indices: Vec<usize>,
}

fn empty_backtest_metrics(num_stocks: usize, strategy_mode: &str) -> BacktestMetrics {
    BacktestMetrics {
        total_return: 0.0,
        annualized_return: None,
        max_drawdown: None,
        num_stocks,
        benchmark_total_return: None,
        benchmark_annualized_return: None,
        benchmark_max_drawdown: None,
        excess_return: None,
        total_transaction_cost: 0.0,
        total_turnover: 0.0,
        rebalance_count: 0,
        oos_fold_count: 0,
        evaluated_selection_count: 0,
        selection_hit_count: 0,
        precision_at_n: None,
        strategy_mode: strategy_mode.to_string(),
    }
}

fn strict_walk_forward_history_symbols(
    source: &impl MarketDataSource,
    symbols: &[String],
    start_date: &str,
    end_date: &str,
) -> CoreResult<Vec<String>> {
    let start = parse_date(start_date)?;
    let end = parse_date(end_date)?;
    let mut required = Vec::new();
    let mut missing_snapshots = Vec::new();

    for code in symbols {
        let timeline = load_factor_snapshot_timeline(source, code)?;
        if timeline.is_empty() {
            missing_snapshots.push(code.clone());
            continue;
        }
        let tradable_at_start = timeline
            .range(..=start)
            .next_back()
            .map(|(_, snapshot)| {
                snapshot.is_listed == Some(true) && snapshot.is_tradable == Some(true)
            })
            .unwrap_or(false);
        let becomes_tradable_in_range = timeline.range(start..=end).any(|(_, snapshot)| {
            snapshot.is_listed == Some(true) && snapshot.is_tradable == Some(true)
        });
        if tradable_at_start || becomes_tradable_in_range {
            required.push(code.clone());
        }
    }

    if !missing_snapshots.is_empty() {
        let preview = missing_snapshots
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join("、");
        return Err(CoreError::new(format!(
            "walk_forward 严格回测要求完整的历史因子快照 factor_snapshots；缺少 {} 个：{}",
            missing_snapshots.len(),
            preview
        )));
    }

    Ok(required)
}

fn load_backtest_histories(
    source: &impl MarketDataSource,
    symbols: &[String],
    start_date: &str,
    end_date: &str,
) -> CoreResult<Vec<BacktestHistory>> {
    let mut histories = Vec::new();
    for code in symbols {
        let bars = source.get_history(code, start_date, end_date)?;
        let prices: BTreeMap<NaiveDate, f64> = history_points_from_bars(&bars)?
            .into_iter()
            .filter(|point| point.close.is_finite() && point.close > 0.0)
            .map(|point| (point.date, point.close))
            .collect();
        if !prices.is_empty() {
            histories.push(BacktestHistory {
                code: code.clone(),
                prices,
                bars,
            });
        }
    }
    Ok(histories)
}

fn simulate_equal_weight_portfolio(
    histories: &[BacktestHistory],
    initial_cash: f64,
    transaction_cost_rate: f64,
    rebalance_frequency: &str,
) -> PortfolioSimulation {
    let dates = backtest_calendar(histories);
    let mut cash = initial_cash;
    let mut holdings = vec![0.0; histories.len()];
    let mut last_rebalance_date: Option<NaiveDate> = None;
    let mut equity_curve = Vec::with_capacity(dates.len());
    let mut rebalance_dates = Vec::new();
    let mut total_transaction_cost = 0.0;
    let mut total_turnover = 0.0;

    for date in dates {
        let current_prices: Vec<Option<f64>> = histories
            .iter()
            .map(|history| latest_price_on_or_before(history, date))
            .collect();
        let equity_before_rebalance = cash
            + holdings
                .iter()
                .zip(current_prices.iter())
                .map(|(shares, price)| shares * price.unwrap_or(0.0))
                .sum::<f64>();

        if should_rebalance(date, last_rebalance_date, rebalance_frequency)
            && equity_before_rebalance > 0.0
        {
            let active_indices: Vec<usize> = current_prices
                .iter()
                .enumerate()
                .filter_map(|(index, price)| price.filter(|value| *value > 0.0).map(|_| index))
                .collect();
            if !active_indices.is_empty() {
                let target_weight = 1.0 / active_indices.len() as f64;
                let mut turnover_value = 0.0;
                for index in 0..histories.len() {
                    let price = current_prices[index].unwrap_or(0.0);
                    let target_value = if price > 0.0 && active_indices.contains(&index) {
                        equity_before_rebalance * target_weight
                    } else {
                        0.0
                    };
                    let current_value = holdings[index] * price;
                    turnover_value += (target_value - current_value).abs();
                }

                let transaction_cost = turnover_value * transaction_cost_rate;
                let investable_equity = (equity_before_rebalance - transaction_cost).max(0.0);
                cash = investable_equity;
                holdings.fill(0.0);
                for index in active_indices {
                    if let Some(price) = current_prices[index].filter(|value| *value > 0.0) {
                        let target_value = investable_equity * target_weight;
                        holdings[index] = target_value / price;
                        cash -= target_value;
                    }
                }
                total_transaction_cost += transaction_cost;
                total_turnover += turnover_value / equity_before_rebalance;
                rebalance_dates.push(date);
                last_rebalance_date = Some(date);
            }
        }

        let equity = cash
            + holdings
                .iter()
                .zip(current_prices.iter())
                .map(|(shares, price)| shares * price.unwrap_or(0.0))
                .sum::<f64>();
        equity_curve.push(EquityPoint {
            date: date.format("%Y-%m-%d").to_string(),
            equity,
        });
    }

    PortfolioSimulation {
        equity_curve,
        rebalance_dates,
        total_transaction_cost,
        total_turnover,
        selections: Vec::new(),
    }
}

fn simulate_walk_forward_portfolio(
    source: &impl MarketDataSource,
    universe: &[StockItem],
    request: &BacktestRequest,
    histories: &[BacktestHistory],
    initial_cash: f64,
    transaction_cost_rate: f64,
    rebalance_frequency: &str,
) -> CoreResult<PortfolioSimulation> {
    let dates = backtest_calendar(histories);
    let mut snapshots_by_code = HashMap::new();
    let mut missing_snapshot_codes = Vec::new();
    for history in histories {
        let timeline = load_factor_snapshot_timeline(source, &history.code)?;
        if timeline.is_empty() {
            missing_snapshot_codes.push(history.code.clone());
        }
        snapshots_by_code.insert(history.code.clone(), timeline);
    }
    if !missing_snapshot_codes.is_empty() {
        let preview = missing_snapshot_codes
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join("、");
        return Err(CoreError::new(format!(
            "walk_forward 严格回测要求每个有历史行情的标的都有历史因子快照 factor_snapshots；缺少 {} 个：{}",
            missing_snapshot_codes.len(),
            preview
        )));
    }
    Ok(simulate_walk_forward_portfolio_with_snapshots(
        universe,
        request,
        histories,
        &snapshots_by_code,
        dates,
        initial_cash,
        transaction_cost_rate,
        rebalance_frequency,
    ))
}

fn simulate_walk_forward_portfolio_with_snapshots(
    universe: &[StockItem],
    request: &BacktestRequest,
    histories: &[BacktestHistory],
    snapshots_by_code: &HashMap<String, BTreeMap<NaiveDate, StockFactorSnapshot>>,
    dates: Vec<NaiveDate>,
    initial_cash: f64,
    transaction_cost_rate: f64,
    rebalance_frequency: &str,
) -> PortfolioSimulation {
    let mut cash = initial_cash;
    let mut holdings = vec![0.0; histories.len()];
    let mut last_rebalance_date: Option<NaiveDate> = None;
    let mut equity_curve = Vec::with_capacity(dates.len());
    let mut rebalance_dates = Vec::new();
    let mut total_transaction_cost = 0.0;
    let mut total_turnover = 0.0;
    let mut selections = Vec::new();
    let history_index: HashMap<String, usize> = histories
        .iter()
        .enumerate()
        .map(|(index, history)| (history.code.to_uppercase(), index))
        .collect();

    for date in dates {
        let valuation_prices: Vec<Option<f64>> = histories
            .iter()
            .map(|history| latest_price_on_or_before(history, date))
            .collect();
        let trade_prices: Vec<Option<f64>> = histories
            .iter()
            .map(|history| price_on_date(history, date))
            .collect();
        let equity_before_rebalance = cash
            + holdings
                .iter()
                .zip(valuation_prices.iter())
                .map(|(shares, price)| shares * price.unwrap_or(0.0))
                .sum::<f64>();

        if should_rebalance(date, last_rebalance_date, rebalance_frequency)
            && equity_before_rebalance > 0.0
        {
            let active = walk_forward_active_indices(
                universe,
                request,
                snapshots_by_code,
                &history_index,
                &trade_prices,
                date,
            );
            let target_weight = if active.selected_indices.is_empty() {
                0.0
            } else {
                1.0 / active.selected_indices.len() as f64
            };
            let active_set: HashSet<usize> = active.selected_indices.iter().copied().collect();
            let tradable_equity_before = cash
                + holdings
                    .iter()
                    .zip(trade_prices.iter())
                    .map(|(shares, price)| shares * price.unwrap_or(0.0))
                    .sum::<f64>();
            let mut turnover_value = 0.0;
            for index in 0..histories.len() {
                let price = trade_prices[index].unwrap_or(0.0);
                let target_value = if price > 0.0 && active_set.contains(&index) {
                    tradable_equity_before * target_weight
                } else {
                    0.0
                };
                if price > 0.0 {
                    let current_value = holdings[index] * price;
                    turnover_value += (target_value - current_value).abs();
                }
            }

            let transaction_cost = turnover_value * transaction_cost_rate;
            let investable_equity = (tradable_equity_before - transaction_cost).max(0.0);
            cash = investable_equity;
            for (index, price) in trade_prices.iter().enumerate() {
                if price.is_some() {
                    holdings[index] = 0.0;
                }
            }
            for index in active.selected_indices.iter().copied() {
                if let Some(price) = trade_prices[index].filter(|value| *value > 0.0) {
                    let target_value = investable_equity * target_weight;
                    holdings[index] = target_value / price;
                    cash -= target_value;
                }
            }
            total_transaction_cost += transaction_cost;
            total_turnover += turnover_value / equity_before_rebalance;
            rebalance_dates.push(date);
            selections.push(RebalanceSelection {
                date,
                selected_indices: active.selected_indices,
                eligible_indices: active.eligible_indices,
            });
            last_rebalance_date = Some(date);
        }

        let equity = cash
            + holdings
                .iter()
                .zip(valuation_prices.iter())
                .map(|(shares, price)| shares * price.unwrap_or(0.0))
                .sum::<f64>();
        equity_curve.push(EquityPoint {
            date: date.format("%Y-%m-%d").to_string(),
            equity,
        });
    }

    PortfolioSimulation {
        equity_curve,
        rebalance_dates,
        total_transaction_cost,
        total_turnover,
        selections,
    }
}

fn load_factor_snapshot_timeline(
    source: &impl MarketDataSource,
    code: &str,
) -> CoreResult<BTreeMap<NaiveDate, StockFactorSnapshot>> {
    let mut timeline = BTreeMap::new();
    for snapshot in source.get_factor_snapshots(code)? {
        let available_date = snapshot.available_date.as_deref().ok_or_else(|| {
            CoreError::new(format!(
                "历史因子快照缺少 available_date，无法判断数据何时可用：{code} {}",
                snapshot.date
            ))
        })?;
        let report_date = parse_date(&snapshot.date).map_err(|_| {
            CoreError::new(format!(
                "历史因子快照报告期 date 无效：{code} {}",
                snapshot.date
            ))
        })?;
        if snapshot.is_st.is_none()
            || snapshot.is_listed.is_none()
            || snapshot.is_tradable.is_none()
        {
            return Err(CoreError::new(format!(
                "历史因子快照缺少 is_st/is_listed/is_tradable 状态：{code} {available_date}"
            )));
        }
        let availability_date = parse_date(available_date).map_err(|_| {
            CoreError::new(format!(
                "历史因子快照 available_date 无效：{code} {available_date}"
            ))
        })?;
        if availability_date < report_date {
            return Err(CoreError::new(format!(
                "历史因子快照 available_date 早于报告期 date：{code} {} 早于 {}",
                available_date, snapshot.date
            )));
        }
        timeline.insert(availability_date, snapshot);
    }
    Ok(timeline)
}

fn walk_forward_active_indices(
    universe: &[StockItem],
    request: &BacktestRequest,
    snapshots_by_code: &HashMap<String, BTreeMap<NaiveDate, StockFactorSnapshot>>,
    history_index: &HashMap<String, usize>,
    current_prices: &[Option<f64>],
    date: NaiveDate,
) -> ActiveSelection {
    if normalize_backtest_source(&request.source) == "watchlist" {
        let eligible_indices = dedupe_stock_codes(&request.stock_codes)
            .into_iter()
            .filter_map(|code| {
                let index = history_index.get(&code.to_uppercase()).copied()?;
                let snapshot = latest_factor_snapshot_on_or_before(snapshots_by_code, &code, date)?;
                if snapshot.is_listed != Some(true)
                    || snapshot.is_tradable != Some(true)
                    || (!request.criteria.include_st && snapshot.is_st == Some(true))
                {
                    return None;
                }
                current_prices
                    .get(index)
                    .and_then(|price| *price)
                    .filter(|price| price.is_finite() && *price > 0.0)?;
                Some(index)
            })
            .collect::<Vec<_>>();
        let selected_indices = eligible_indices
            .iter()
            .take(request.top_n.clamp(1, 100))
            .copied()
            .collect();
        return ActiveSelection {
            eligible_indices,
            selected_indices,
        };
    }

    let visible_universe = point_in_time_universe(
        universe,
        snapshots_by_code,
        history_index,
        current_prices,
        date,
    );
    let selected = selected_backtest_items_from_universe(&visible_universe, request).0;
    let eligible_indices = visible_universe
        .iter()
        .filter_map(|stock| history_index.get(&stock.code.to_uppercase()).copied())
        .collect::<Vec<_>>();
    let mut indices = Vec::new();
    for item in selected.into_iter().take(request.top_n.clamp(1, 100)) {
        let code = item.stock.code.to_uppercase();
        if let Some(index) = history_index.get(&code).copied() {
            if current_prices
                .get(index)
                .and_then(|price| *price)
                .filter(|price| *price > 0.0)
                .is_some()
            {
                indices.push(index);
            }
        }
    }
    ActiveSelection {
        selected_indices: indices,
        eligible_indices,
    }
}

fn point_in_time_universe(
    universe: &[StockItem],
    snapshots_by_code: &HashMap<String, BTreeMap<NaiveDate, StockFactorSnapshot>>,
    history_index: &HashMap<String, usize>,
    current_prices: &[Option<f64>],
    date: NaiveDate,
) -> Vec<StockItem> {
    universe
        .iter()
        .filter_map(|stock| {
            let snapshot =
                latest_factor_snapshot_on_or_before(snapshots_by_code, &stock.code, date)?;
            if snapshot.is_listed != Some(true) || snapshot.is_tradable != Some(true) {
                return None;
            }
            snapshot.is_st?;
            let index = history_index.get(&stock.code.to_uppercase()).copied()?;
            let price = current_prices
                .get(index)
                .and_then(|price| *price)
                .filter(|price| price.is_finite() && *price > 0.0)?;
            Some(apply_factor_snapshot(stock, snapshot, price))
        })
        .collect()
}

fn latest_factor_snapshot_on_or_before<'a>(
    snapshots_by_code: &'a HashMap<String, BTreeMap<NaiveDate, StockFactorSnapshot>>,
    code: &str,
    date: NaiveDate,
) -> Option<&'a StockFactorSnapshot> {
    snapshots_by_code
        .get(code)
        .or_else(|| snapshots_by_code.get(&code.to_uppercase()))
        .and_then(|timeline| {
            timeline
                .range(..=date)
                .next_back()
                .map(|(_, snapshot)| snapshot)
        })
}

fn apply_factor_snapshot(
    stock: &StockItem,
    snapshot: &StockFactorSnapshot,
    historical_price: f64,
) -> StockItem {
    let finite = |value: Option<f64>| value.filter(|value| value.is_finite());
    StockItem {
        code: stock.code.clone(),
        name: snapshot.name.clone().unwrap_or_default(),
        industry: snapshot.industry.clone().unwrap_or_default(),
        is_st: snapshot.is_st.unwrap_or(true),
        price: historical_price,
        pe: finite(snapshot.pe),
        pb: finite(snapshot.pb),
        roe: finite(snapshot.roe),
        market_cap_billion: finite(snapshot.market_cap_billion),
        dividend_yield: finite(snapshot.dividend_yield),
        latest_eps: None,
        deducted_net_profit_billion: finite(snapshot.deducted_net_profit_billion),
        deducted_net_profit_margin: finite(snapshot.deducted_net_profit_margin),
        deducted_net_profit_growth_rate: finite(snapshot.deducted_net_profit_growth_rate),
        change_pct: finite(snapshot.change_pct),
        volume: finite(snapshot.volume),
        amount: finite(snapshot.amount),
        turnover_rate: finite(snapshot.turnover_rate),
        volume_ratio: finite(snapshot.volume_ratio),
        quote_time: snapshot.available_date.clone(),
    }
}

fn equal_weight_benchmark_curve(
    histories: &[BacktestHistory],
    initial_cash: f64,
) -> Vec<EquityPoint> {
    backtest_calendar(histories)
        .into_iter()
        .filter_map(|date| {
            let values: Vec<f64> = histories
                .iter()
                .filter_map(|history| {
                    let first = history.prices.values().next().copied()?;
                    let price = latest_price_on_or_before(history, date)?;
                    if first > 0.0 && price > 0.0 {
                        Some(price / first)
                    } else {
                        None
                    }
                })
                .collect();
            if values.is_empty() {
                None
            } else {
                Some(EquityPoint {
                    date: date.format("%Y-%m-%d").to_string(),
                    equity: initial_cash * values.iter().sum::<f64>() / values.len() as f64,
                })
            }
        })
        .collect()
}

fn backtest_calendar(histories: &[BacktestHistory]) -> Vec<NaiveDate> {
    let mut dates = BTreeMap::new();
    for history in histories {
        for date in history.prices.keys() {
            dates.insert(*date, ());
        }
    }
    dates.into_keys().collect()
}

fn latest_price_on_or_before(history: &BacktestHistory, date: NaiveDate) -> Option<f64> {
    history
        .prices
        .range(..=date)
        .next_back()
        .map(|(_, price)| *price)
}

fn price_on_date(history: &BacktestHistory, date: NaiveDate) -> Option<f64> {
    history.prices.get(&date).copied()
}

fn selected_symbols_from_latest_rebalance(
    histories: &[BacktestHistory],
    selections: &[RebalanceSelection],
) -> Vec<String> {
    selections
        .last()
        .map(|selection| {
            selection
                .selected_indices
                .iter()
                .filter_map(|index| histories.get(*index).map(|history| history.code.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn selected_symbols_from_rebalances(
    histories: &[BacktestHistory],
    selections: &[RebalanceSelection],
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut symbols = Vec::new();
    for selection in selections {
        for index in &selection.selected_indices {
            let Some(history) = histories.get(*index) else {
                continue;
            };
            if seen.insert(history.code.clone()) {
                symbols.push(history.code.clone());
            }
        }
    }
    symbols
}

fn evaluate_walk_forward_folds(
    histories: &[BacktestHistory],
    selections: &[RebalanceSelection],
) -> Vec<WalkForwardFold> {
    let final_date = backtest_calendar(histories).last().copied();
    selections
        .iter()
        .enumerate()
        .map(|(position, selection)| {
            let evaluation_end = selections
                .get(position + 1)
                .map(|next| next.date)
                .or(final_date)
                .filter(|end| *end > selection.date);
            let selected_symbols = selection
                .selected_indices
                .iter()
                .filter_map(|index| histories.get(*index).map(|history| history.code.clone()))
                .collect::<Vec<_>>();
            let selected_returns = evaluation_end
                .map(|end| {
                    selection
                        .selected_indices
                        .iter()
                        .map(|index| {
                            histories
                                .get(*index)
                                .and_then(|history| forward_return(history, selection.date, end))
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let benchmark_returns = evaluation_end
                .map(|end| {
                    selection
                        .eligible_indices
                        .iter()
                        .filter_map(|index| {
                            forward_return(histories.get(*index)?, selection.date, end)
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let benchmark_forward_return = mean(&benchmark_returns);
            let realized_selected_returns = selected_returns
                .iter()
                .filter_map(|value| *value)
                .collect::<Vec<_>>();
            let average_forward_return = mean(&realized_selected_returns);
            let (evaluated_selection_count, hit_count, precision_at_n) =
                if let Some(benchmark) = benchmark_forward_return {
                    let hit_count = selected_returns
                        .iter()
                        .filter_map(|value| *value)
                        .filter(|value| *value > benchmark)
                        .count();
                    let evaluated = selected_returns.len();
                    let precision = (evaluated > 0).then_some(hit_count as f64 / evaluated as f64);
                    (evaluated, hit_count, precision)
                } else {
                    (0, 0, None)
                };
            WalkForwardFold {
                selection_date: selection.date.format("%Y-%m-%d").to_string(),
                evaluation_end_date: evaluation_end.map(|date| date.format("%Y-%m-%d").to_string()),
                selected_symbols,
                eligible_symbol_count: selection.eligible_indices.len(),
                evaluated_selection_count,
                hit_count,
                precision_at_n,
                average_forward_return,
                benchmark_forward_return,
                average_excess_return: average_forward_return
                    .zip(benchmark_forward_return)
                    .map(|(selected, benchmark)| selected - benchmark),
            }
        })
        .collect()
}

fn forward_return(history: &BacktestHistory, start: NaiveDate, end: NaiveDate) -> Option<f64> {
    let start_price = price_on_date(history, start)?;
    let end_price = price_on_date(history, end)?;
    (start_price > 0.0 && end_price > 0.0).then_some(end_price / start_price - 1.0)
}

fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then_some(values.iter().sum::<f64>() / values.len() as f64)
}

fn should_rebalance(
    date: NaiveDate,
    last_rebalance_date: Option<NaiveDate>,
    frequency: &str,
) -> bool {
    let Some(previous) = last_rebalance_date else {
        return true;
    };
    match frequency {
        "none" => false,
        "quarterly" => date.year() != previous.year() || quarter(date) != quarter(previous),
        _ => date.year() != previous.year() || date.month() != previous.month(),
    }
}

fn quarter(date: NaiveDate) -> u32 {
    (date.month0() / 3) + 1
}

fn sanitize_initial_cash(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        default_initial_cash()
    }
}

fn sanitize_transaction_cost_rate(bps: f64) -> f64 {
    if bps.is_finite() {
        bps.clamp(0.0, 500.0) / 10_000.0
    } else {
        default_transaction_cost_bps() / 10_000.0
    }
}

fn normalize_rebalance_frequency(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" => "none".to_string(),
        "quarterly" => "quarterly".to_string(),
        _ => "monthly".to_string(),
    }
}

fn normalize_backtest_benchmark(value: &str) -> String {
    if value.trim().eq_ignore_ascii_case("none") {
        "none".to_string()
    } else {
        "candidate_equal_weight".to_string()
    }
}

fn normalize_backtest_strategy_mode(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "candidate_snapshot" | "snapshot" => "candidate_snapshot".to_string(),
        _ => "walk_forward".to_string(),
    }
}

fn backtest_strategy_mode_note(value: &str) -> &'static str {
    match value {
        "walk_forward" => "策略模式：walk_forward，仅使用 available_date 不晚于调仓日的因子快照和历史上市/ST/可交易状态；调仓只使用当日实际报价。",
        _ => "策略模式：candidate_snapshot，展示当前候选池的历史组合表现，不等同于严格可交易的逐日选股策略。",
    }
}

fn backtest_rebalance_note(value: &str) -> &'static str {
    match value {
        "none" => "买入持有",
        "quarterly" => "季度调仓",
        _ => "月度调仓",
    }
}
fn selected_backtest_items(
    universe: &[StockItem],
    request: &BacktestRequest,
) -> (Vec<ScreenedStock>, Vec<String>) {
    selected_backtest_items_from_universe(universe, request)
}

fn selected_backtest_items_from_universe(
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
    let snapshot_period = financial.and_then(|snapshot| snapshot.period.as_deref());

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
    let snapshot_roe = financial.and_then(|snapshot| snapshot.roe);
    if snapshot_roe.is_some() {
        push_indicator_with_meta(
            &mut items,
            "ROE",
            snapshot_roe,
            format_percentage_points,
            indicator_tone(snapshot_roe),
            Some("%"),
            Some("roe"),
            snapshot_period,
        );
    } else {
        push_indicator_with_meta(
            &mut items,
            "ROE",
            stock.roe,
            format_percent,
            indicator_tone(stock.roe),
            Some("%"),
            Some("roe"),
            None,
        );
    }
    push_indicator(
        &mut items,
        "\u{5e02}\u{503c}",
        stock.market_cap_billion,
        |value| format!("{}\u{4ebf}", format_number(value)),
        "neutral",
    );
    let snapshot_dividend_yield = financial.and_then(|snapshot| snapshot.dividend_yield);
    if snapshot_dividend_yield.is_some() {
        push_indicator_with_meta(
            &mut items,
            "\u{80a1}\u{606f}\u{7387}",
            snapshot_dividend_yield,
            format_percentage_points,
            "neutral",
            Some("%"),
            Some("dividend_yield"),
            financial.and_then(|snapshot| snapshot.dividend_period.as_deref()),
        );
    } else {
        push_indicator_with_meta(
            &mut items,
            "\u{80a1}\u{606f}\u{7387}",
            stock.dividend_yield,
            format_percent,
            "neutral",
            Some("%"),
            Some("dividend_yield"),
            None,
        );
    }
    push_indicator_with_meta(
        &mut items,
        "营业总收入",
        financial.and_then(|snapshot| snapshot.operating_revenue_billion),
        |value| format!("{}亿", format_number(value)),
        "neutral",
        Some("亿"),
        Some("operating_revenue"),
        snapshot_period,
    );
    push_indicator_with_meta(
        &mut items,
        "总营收同比",
        financial.and_then(|snapshot| snapshot.operating_revenue_yoy),
        format_percentage_points,
        indicator_tone(financial.and_then(|snapshot| snapshot.operating_revenue_yoy)),
        Some("%"),
        Some("operating_revenue_yoy"),
        snapshot_period,
    );
    push_indicator_with_meta(
        &mut items,
        "归母净利润",
        financial.and_then(|snapshot| snapshot.parent_net_profit_billion),
        |value| format!("{}亿", format_number(value)),
        indicator_tone(financial.and_then(|snapshot| snapshot.parent_net_profit_billion)),
        Some("亿"),
        Some("parent_net_profit"),
        snapshot_period,
    );
    push_indicator_with_meta(
        &mut items,
        "归母净利同比",
        financial.and_then(|snapshot| snapshot.parent_net_profit_yoy),
        format_percentage_points,
        indicator_tone(financial.and_then(|snapshot| snapshot.parent_net_profit_yoy)),
        Some("%"),
        Some("parent_net_profit_yoy"),
        snapshot_period,
    );
    for (label, metric_key, value, metric_period) in [
        (
            "毛利率",
            "gross_margin",
            financial.and_then(|snapshot| snapshot.gross_margin),
            snapshot_period,
        ),
        (
            "净利率",
            "net_margin",
            financial.and_then(|snapshot| snapshot.net_margin),
            snapshot_period,
        ),
        (
            "资产负债率",
            "asset_liability_ratio",
            financial.and_then(|snapshot| snapshot.asset_liability_ratio),
            snapshot_period,
        ),
        (
            "商誉净资产比",
            "goodwill_to_net_assets",
            financial.and_then(|snapshot| snapshot.goodwill_to_net_assets),
            financial.and_then(|snapshot| snapshot.goodwill_period.as_deref()),
        ),
        (
            "质押总股本比",
            "pledged_share_ratio",
            financial.and_then(|snapshot| snapshot.pledged_share_ratio),
            financial.and_then(|snapshot| snapshot.pledged_share_period.as_deref()),
        ),
        (
            "股利支付率(静)",
            "dividend_payout_ratio",
            financial.and_then(|snapshot| snapshot.dividend_payout_ratio),
            financial.and_then(|snapshot| snapshot.dividend_period.as_deref()),
        ),
    ] {
        push_indicator_with_meta(
            &mut items,
            label,
            value,
            format_percentage_points,
            "neutral",
            Some("%"),
            Some(metric_key),
            metric_period,
        );
    }
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
        format_percentage_points,
        "neutral",
    );
    push_indicator(
        &mut items,
        "\u{6263}\u{975e}\u{51c0}\u{5229}\u{6da6}\u{589e}\u{957f}\u{7387}",
        stock.deducted_net_profit_growth_rate,
        format_percentage_points,
        indicator_tone(stock.deducted_net_profit_growth_rate),
    );

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
            notes
                .push("基准为候选池买入持有等权曲线，用于衡量调仓与成本后的超额收益。".to_string());
        }
    } else {
        notes.push("基准为候选池买入持有等权曲线，用于衡量调仓与成本后的超额收益。".to_string());
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

fn format_percentage_points(value: f64) -> String {
    format!("{}%", format_number(value))
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
            "未调用模型，综合资金证据分由本地规则按资金流、机构席位、消息情绪和技术推断四类权重生成。",
        );
    }
    if !capital_bucket_has_evidence(&items, ["fund_flow", "", ""]) {
        push_capital_note(
            &mut notes,
            "资金流外部证据暂缺，本次按中性权重保留该项，不把缺失数据当作流入或流出结论。",
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
            "综合资金证据分 {}，已纳入{}；缺失分数的证据项按中性权重处理。",
            format_number(composite_score),
            scored_labels.join("、")
        );
    }
    if !evidence_labels.is_empty() {
        return format!(
            "综合资金证据分 {}，当前只有{}状态证据；缺失分数的证据项按中性权重处理。",
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
    metrics.insert("隐性资金代理分".to_string(), format_number(score));
    metrics.insert(
        "推断方向".to_string(),
        fund_flow_proxy_direction(score).to_string(),
    );
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

fn fund_flow_proxy_direction(score: f64) -> &'static str {
    if score >= 60.0 {
        "偏流入"
    } else if score <= 40.0 {
        "偏流出"
    } else {
        "中性"
    }
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
        title: "消息情绪暂无外部证据".to_string(),
        date: Some(as_of_trade_date.to_string()),
        metrics,
        sentiment: "uncertain".to_string(),
        weight: 0.15,
        confidence: "低".to_string(),
        url: None,
        score: None,
        note: Some("当前窗口没有可展示的新闻或社区情绪证据；该项仅作为证据缺口提示。".to_string()),
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
    let source = StaticDataSource::new(data);
    trend_screen_with_source(&source, request)
}

pub fn trend_screen_with_source(
    source: &impl MarketDataSource,
    request: &TrendScreenRequest,
) -> CoreResult<TrendScreenResult> {
    let universe = source.stocks()?;
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
        reasons.extend(signal.reasons.clone());
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

pub fn agent_skill_registry() -> Vec<AgentSkill> {
    vec![
        agent_skill(
            "stock_snapshot",
            "Stock Snapshot",
            "Local quote, observe and financial snapshot.",
            "desktop,android",
            "local_cache",
            "Return available observe data or explain missing local cache.",
        ),
        agent_skill(
            "stock_news",
            "Stock News",
            "News and RAG evidence for recent positive and negative signals.",
            "desktop,android",
            "rag_or_news_cache",
            "Return short-link news or imported mobile RAG package when full RAG is unavailable.",
        ),
        agent_skill(
            "stock_screen",
            "Stock Screen",
            "Rule based candidate screening over the local universe.",
            "desktop,android",
            "local_cache",
            "Ask for clearer criteria when the intent cannot be mapped.",
        ),
        agent_skill(
            "trend_analysis",
            "Trend Analysis",
            "Technical trend ranking and local OHLC indicators.",
            "desktop,android",
            "local_history",
            "Return skipped-count warnings when OHLC history is incomplete.",
        ),
        agent_skill(
            "sector_analysis",
            "Sector Analysis",
            "Sector and relation driven candidate discovery.",
            "desktop",
            "local_relations",
            "Fall back to ordinary screening when relation data is unavailable.",
        ),
        agent_skill(
            "watchlist_action",
            "Watchlist Action",
            "Local watchlist-only add, remove and review workflow.",
            "desktop,android",
            "local_state",
            "Never place trades; explain local-only scope.",
        ),
        agent_skill(
            "portfolio_simulation",
            "Portfolio Simulation",
            "Watchlist or candidate portfolio observation and backtest.",
            "desktop",
            "local_backtest",
            "Return research-only backtest or desktop-only downgrade notice.",
        ),
    ]
}

fn agent_skill(
    key: &str,
    title: &str,
    description: &str,
    platforms: &str,
    evidence_level: &str,
    fallback: &str,
) -> AgentSkill {
    AgentSkill {
        key: key.to_string(),
        title: title.to_string(),
        description: description.to_string(),
        input_schema: json!({ "query": "string", "symbols": ["string"], "mode": "quick|expert|research" }),
        output_schema: json!({ "sections": ["summary", "evidence", "risks"], "research_only": true }),
        platforms: platforms.split(',').map(|item| item.to_string()).collect(),
        evidence_level: evidence_level.to_string(),
        fallback: fallback.to_string(),
    }
}

fn agent_finalize_response(
    mut response: AgentResponse,
    message: &str,
    mode: Option<&str>,
) -> AgentResponse {
    let mode_text = mode.unwrap_or("quick").to_string();
    let action = response.action.clone();
    let symbols = extract_codes(message);
    response.intent = Some(AgentIntent {
        kind: agent_intent_kind(&action).to_string(),
        query: message.trim().to_string(),
        symbols,
        window: agent_window_hint(message),
        depth: agent_depth_for_mode(&mode_text).to_string(),
        mode: mode_text,
    });
    response.tool_calls = agent_tool_calls_for_response(&response);
    response.evidence_summary = agent_evidence_for_response(&response);
    response.answer_sections = agent_beginner_sections_for_response(&response);
    response.warnings = agent_warnings_for_response(&response);
    response.next_actions = agent_next_actions_for_response(&response);
    response
}

fn agent_intent_kind(action: &str) -> &'static str {
    match action {
        "trend_screen" => "trend_analysis",
        "graph_screen" => "sector_analysis",
        "backtest" => "portfolio_simulation",
        "screen" => "stock_screen",
        "observe_stock" => "stock_snapshot",
        "news_rag" => "stock_news",
        "watchlist_action" => "watchlist_action",
        _ => "clarify",
    }
}
fn agent_depth_for_mode(mode: &str) -> &'static str {
    match mode {
        "research" => "research",
        "expert" => "expert",
        _ => "quick",
    }
}

fn agent_window_hint(message: &str) -> Option<String> {
    let lower = message.to_lowercase();
    if lower.contains("recent") || message.contains("近期") || message.contains("最近") {
        Some("recent".to_string())
    } else if lower.contains("today") || message.contains("今天") || message.contains("今日") {
        Some("today".to_string())
    } else {
        None
    }
}
fn agent_tool_calls_for_response(response: &AgentResponse) -> Vec<AgentToolCall> {
    if response.action == "clarify" {
        return vec![AgentToolCall {
            id: "intent_parser".to_string(),
            tool: "agent_plan_request".to_string(),
            label: "Understand request".to_string(),
            status: "needs_input".to_string(),
            input: json!({ "action": response.action }),
            output_summary: Some("Need a clearer stock research task.".to_string()),
            warnings: Vec::new(),
        }];
    }
    let tool = agent_intent_kind(&response.action);
    vec![
        AgentToolCall {
            id: "intent_parser".to_string(),
            tool: "agent_plan_request".to_string(),
            label: "Understand request".to_string(),
            status: "ok".to_string(),
            input: json!({ "action": response.action }),
            output_summary: Some(format!("Routed to {tool}.")),
            warnings: Vec::new(),
        },
        AgentToolCall {
            id: format!("tool_{tool}"),
            tool: tool.to_string(),
            label: agent_tool_label(tool).to_string(),
            status: if response.data.is_some() {
                "ok"
            } else {
                "degraded"
            }
            .to_string(),
            input: json!({ "action": response.action }),
            output_summary: Some(agent_data_summary(response)),
            warnings: agent_warnings_for_response(response),
        },
        AgentToolCall {
            id: "synthesizer".to_string(),
            tool: "agent_synthesize_response".to_string(),
            label: "Structure answer".to_string(),
            status: "ok".to_string(),
            input: json!({ "research_only": true }),
            output_summary: Some("Prepared sections, evidence and risk boundary.".to_string()),
            warnings: Vec::new(),
        },
    ]
}

fn agent_tool_label(tool: &str) -> &'static str {
    match tool {
        "trend_analysis" => "运行趋势筛选",
        "sector_analysis" => "运行自定义板块筛选",
        "portfolio_simulation" => "运行本地组合观察",
        "stock_screen" => "运行本地选股",
        "stock_snapshot" => "生成个股速览",
        "stock_news" => "整理资讯证据",
        "watchlist_action" => "读取本地自选股",
        _ => "运行本地工具",
    }
}
fn agent_data_summary(response: &AgentResponse) -> String {
    let Some(data) = response.data.as_ref() else {
        return "未返回结构化数据。".to_string();
    };
    let returned = data
        .get("returned")
        .and_then(Value::as_u64)
        .or_else(|| {
            data.get("items")
                .and_then(Value::as_array)
                .map(|items| items.len() as u64)
        })
        .or_else(|| {
            data.get("equity_curve")
                .and_then(Value::as_array)
                .map(|items| items.len() as u64)
        });
    match returned {
        Some(count) => format!("返回 {count} 条本地记录。"),
        None => "已返回本地结构化数据。".to_string(),
    }
}
fn agent_evidence_for_response(response: &AgentResponse) -> Vec<AgentEvidenceItem> {
    let source = match response.action.as_str() {
        "trend_screen" => "本地 K 线与趋势指标",
        "graph_screen" => "本地自定义筛选兼容结果",
        "backtest" => "本地回测引擎",
        "screen" => "本地股票池缓存",
        "observe_stock" => "本地行情与观察数据",
        "news_rag" => "本地新闻/RAG 降级结果",
        "watchlist_action" => "本地自选股观察池",
        _ => "本地 Agent 路由",
    };
    vec![AgentEvidenceItem {
        title: agent_tool_label(agent_intent_kind(&response.action)).to_string(),
        source: source.to_string(),
        level: if response.data.is_some() {
            "primary"
        } else {
            "degraded"
        }
        .to_string(),
        summary: agent_data_summary(response),
    }]
}
fn agent_beginner_sections_for_response(response: &AgentResponse) -> Vec<AgentAnswerSection> {
    if response.action == "observe_stock" {
        if let Some(sections) = agent_beginner_stock_sections(response) {
            return sections;
        }
    }
    agent_sections_for_response(response)
}

fn agent_sections_for_response(response: &AgentResponse) -> Vec<AgentAnswerSection> {
    vec![
        AgentAnswerSection {
            title: "结论概览".to_string(),
            bullets: vec![
                response.reply.clone(),
                "仅供选股研究，不构成投资建议。".to_string(),
            ],
        },
        AgentAnswerSection {
            title: "证据摘要".to_string(),
            bullets: agent_evidence_for_response(response)
                .into_iter()
                .map(|item| item.summary)
                .collect(),
        },
        AgentAnswerSection {
            title: "风险边界".to_string(),
            bullets: vec!["不输出买入、卖出、目标价、仓位或收益承诺。".to_string()],
        },
    ]
}
fn agent_beginner_stock_sections(response: &AgentResponse) -> Option<Vec<AgentAnswerSection>> {
    let data = response.data.as_ref()?;
    let stock = data.get("stock")?;
    let name = json_string(stock, "name").unwrap_or_else(|| "这只股票".to_string());
    let code = json_string(stock, "code").unwrap_or_default();
    let price = json_f64(stock, "price").map(format_price_text);
    let change = json_f64(stock, "change_pct").map(format_signed_percent_text);
    let pe = json_f64(stock, "pe").map(|value| format_number(value));
    let pb = json_f64(stock, "pb").map(|value| format_number(value));
    let roe = json_f64(stock, "roe").map(format_percent);
    let market_cap =
        json_f64(stock, "market_cap_billion").map(|value| format!("{}亿", format_number(value)));

    let trend_signal = data.get("trend").and_then(|trend| trend.get("signal"));
    let status = trend_signal.and_then(|signal| json_string(signal, "status"));
    let signal_type = trend_signal.and_then(|signal| json_string(signal, "signal_type"));
    let swl = trend_signal.and_then(|signal| json_f64(signal, "swl"));
    let sws = trend_signal.and_then(|signal| json_f64(signal, "sws"));
    let support = trend_signal.and_then(|signal| json_f64(signal, "support"));
    let resistance = trend_signal.and_then(|signal| json_f64(signal, "resistance"));
    let breakout = trend_signal.and_then(|signal| json_f64(signal, "breakout"));
    let reversal = trend_signal.and_then(|signal| json_f64(signal, "reversal"));
    let k = trend_signal.and_then(|signal| json_f64(signal, "k"));
    let d = trend_signal.and_then(|signal| json_f64(signal, "d"));
    let j = trend_signal.and_then(|signal| json_f64(signal, "j"));
    let golden_cross = trend_signal
        .and_then(|signal| signal.get("golden_cross"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let dead_cross = trend_signal
        .and_then(|signal| signal.get("dead_cross"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let quant_score = trend_signal.and_then(|signal| json_i64(signal, "quant_score"));
    let quant_score_max = trend_signal
        .and_then(|signal| json_i64(signal, "quant_score_max"))
        .unwrap_or(90);
    let risk_flags = trend_signal
        .and_then(|signal| signal.get("risk_flags"))
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();

    let capital = data.get("capital_evidence");
    let capital_score = capital.and_then(|value| json_f64(value, "composite_score"));
    let capital_confidence = capital.and_then(|value| json_string(value, "confidence"));
    let institution_summary = agent_capital_item_summary(capital, "institution_seat");
    let fund_summary = agent_capital_item_summary(capital, "fund_flow");
    let technical_summary = agent_capital_item_summary(capital, "technical_inference");

    let mut conclusion = Vec::new();
    let quote = [
        price.map(|value| format!("最新价 {value}")),
        change.map(|value| format!("涨跌幅 {value}")),
        pe.map(|value| format!("市盈率 {value}")),
        pb.map(|value| format!("市净率 {value}")),
        roe.map(|value| format!("ROE {value}")),
        market_cap.map(|value| format!("总市值 {value}")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("，");
    if quote.is_empty() {
        conclusion.push(format!(
            "{}{}：已找到本地个股数据，但行情字段不完整。",
            name,
            code_suffix(&code)
        ));
    } else {
        conclusion.push(format!(
            "{}{}：{}。这些是先看公司价格、估值和盈利质量的基础信息。",
            name,
            code_suffix(&code),
            quote
        ));
    }
    conclusion.push(agent_beginner_trend_sentence(
        status.as_deref(),
        signal_type.as_deref(),
        swl,
        sws,
        quant_score,
        quant_score_max,
    ));
    conclusion.push("简单理解：现在更像是“可继续观察”的状态，不等于马上买入；新手应先看趋势有没有延续，再看跌破哪个位置说明判断失效。".to_string());

    let mut evidence = Vec::new();
    if let Some(score) = capital_score {
        evidence.push(format!("资金证据综合分 {}，置信度{}。分数越高，说明本地资金、机构、技术证据越一致；50 分附近代表偏中性，不能单独当买入理由。", format_number(score), capital_confidence.unwrap_or_else(|| "未知".to_string())));
    } else {
        evidence.push("资金证据暂不完整，所以不要只根据这一轮 Agent 结果判断强弱。".to_string());
    }
    if let Some(text) = institution_summary {
        evidence.push(format!("机构席位：{text}。新手可以把它理解为“大资金是否留下过明显痕迹”的线索，但它不是实时买卖信号。"));
    }
    if let Some(text) = fund_summary {
        evidence.push(format!(
            "量价资金：{text}。这是用本地成交量和价格估算的资金线索，可靠性低于真实资金流数据。"
        ));
    }
    if let Some(text) = technical_summary {
        evidence.push(format!(
            "技术承接：{text}。意思是价格趋势和指标是否互相支持。"
        ));
    }
    if evidence.len() == 1 {
        evidence.push(agent_data_summary(response));
    }

    let mut levels = Vec::new();
    if let (Some(support), Some(resistance)) = (support, resistance) {
        levels.push(format!("支撑位约 {}、压力位约 {}。支撑位可以理解为“跌到这里附近要小心是否撑不住”，压力位可以理解为“涨到这里附近可能遇到卖压”。", format_price_text(support), format_price_text(resistance)));
    }
    if let Some(breakout) = breakout {
        levels.push(format!(
            "突破观察位约 {}：如果放量站上，才说明上攻更有说服力。",
            format_price_text(breakout)
        ));
    }
    if let Some(reversal) = reversal {
        levels.push(format!(
            "风险观察位约 {}：如果跌破，说明原来的上升判断可能失效。",
            format_price_text(reversal)
        ));
    }
    if let (Some(k), Some(d), Some(j)) = (k, d, j) {
        let cross = if golden_cross {
            "当前出现 KDJ 金叉，短线动能转强的信号更明显。"
        } else if dead_cross {
            "当前出现 KDJ 死叉，短线动能转弱，需要更谨慎。"
        } else {
            "当前没有明确 KDJ 金叉或死叉，说明短线拐点信号还不够明确。"
        };
        levels.push(format!(
            "KDJ：K={}、D={}、J={}。{cross}",
            format_number(k),
            format_number(d),
            format_number(j)
        ));
    }
    if levels.is_empty() {
        levels.push("关键价位和指标暂不完整，建议先补足历史行情后再判断趋势。".to_string());
    }

    let mut risks = Vec::new();
    if risk_flags.is_empty() {
        risks.push("本地指标没有给出明确风险旗标，但这不代表没有风险；它只代表当前规则没有识别到特别异常。".to_string());
    } else {
        risks.push(format!(
            "本地风险旗标：{}。出现风险旗标时，新手应降低仓促决策的倾向。",
            risk_flags.join("、")
        ));
    }
    risks.push("不要把“趋势较好”理解成“必涨”。股价会受大盘、行业、公告、业绩和情绪影响，任何单一指标都可能失效。".to_string());
    risks.push("这不是投资建议，不输出买入、卖出、目标价、仓位或收益承诺。".to_string());

    Some(vec![
        AgentAnswerSection {
            title: "一句话结论".to_string(),
            bullets: conclusion,
        },
        AgentAnswerSection {
            title: "为什么这么判断".to_string(),
            bullets: evidence,
        },
        AgentAnswerSection {
            title: "新手要盯的价位".to_string(),
            bullets: levels,
        },
        AgentAnswerSection {
            title: "风险和下一步".to_string(),
            bullets: risks,
        },
    ])
}

fn agent_beginner_trend_sentence(
    status: Option<&str>,
    signal_type: Option<&str>,
    swl: Option<f64>,
    sws: Option<f64>,
    quant_score: Option<i64>,
    quant_score_max: i64,
) -> String {
    let status_text = match status.unwrap_or_default() {
        "uptrend" => "趋势偏上升",
        "downtrend" => "趋势偏走弱",
        "sideways" => "趋势偏横盘",
        _ => "趋势状态不明确",
    };
    let signal_text = match signal_type.unwrap_or_default() {
        "trend_continuation" => "更像是原有趋势还在延续",
        "breakout" => "更像是尝试突破",
        "reversal" => "更像是可能反转",
        "pullback" => "更像是上涨后的回落观察",
        _ => "暂时没有特别清晰的信号类型",
    };
    let line_text = match (swl, sws) {
        (Some(swl), Some(sws)) if swl > sws => format!(
            "SWL 高于 SWS（{} > {}），可以粗略理解为短中期趋势线偏强。",
            format_price_text(swl),
            format_price_text(sws)
        ),
        (Some(swl), Some(sws)) if swl < sws => format!(
            "SWL 低于 SWS（{} < {}），可以粗略理解为趋势线偏弱。",
            format_price_text(swl),
            format_price_text(sws)
        ),
        (Some(swl), Some(sws)) => format!(
            "SWL 和 SWS 接近（{} / {}），说明趋势强弱还不明显。",
            format_price_text(swl),
            format_price_text(sws)
        ),
        _ => "SWL/SWS 趋势线数据不完整。".to_string(),
    };
    let score_text = quant_score
        .map(|score| format!("本地量化分 {score}/{quant_score_max}。"))
        .unwrap_or_else(|| "本地量化分暂缺。".to_string());
    format!("趋势：{status_text}，{signal_text}。{line_text}{score_text}")
}

fn agent_capital_item_summary(capital: Option<&Value>, category: &str) -> Option<String> {
    let item = capital?
        .get("items")?
        .as_array()?
        .iter()
        .find(|item| json_string(item, "category").as_deref() == Some(category))?;
    if let Some(note) = json_string(item, "note").filter(|text| !text.trim().is_empty()) {
        return Some(note);
    }
    if let Some(score) = json_f64(item, "score") {
        return Some(format!(
            "得分 {}，方向 {}",
            format_number(score),
            json_string(item, "sentiment").unwrap_or_else(|| "未知".to_string())
        ));
    }
    json_string(item, "title")
}

fn json_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn json_f64(value: &Value, key: &str) -> Option<f64> {
    value
        .get(key)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

fn json_i64(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

fn code_suffix(code: &str) -> String {
    if code.is_empty() {
        String::new()
    } else {
        format!("（{code}）")
    }
}

fn format_price_text(value: f64) -> String {
    if value.abs() >= 100.0 {
        format!("{value:.2}")
    } else {
        format!("{value:.3}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

fn format_signed_percent_text(value: f64) -> String {
    let percent = as_percent(value).unwrap_or(value);
    format!("{percent:+.2}%")
}
fn agent_warnings_for_response(response: &AgentResponse) -> Vec<String> {
    let mut warnings = vec!["仅供选股研究，不构成投资建议。".to_string()];
    if response.action == "backtest" {
        warnings.push("组合观察只使用本地自选股/候选股数据，不是真实交易。".to_string());
    }
    if response.action == "news_rag" {
        warnings.push("当前新闻能力可能使用本地缓存或降级结果，需复核原始来源。".to_string());
    }
    if response.data.is_none() {
        warnings.push("本次请求没有可用的本地证据载荷。".to_string());
    }
    warnings
}
fn agent_next_actions_for_response(response: &AgentResponse) -> Vec<String> {
    match response.action.as_str() {
        "trend_screen" => vec![
            "查看候选股详情".to_string(),
            "运行本地回测".to_string(),
            "加入自选股观察".to_string(),
        ],
        "graph_screen" => vec![
            "调整自定义筛选条件".to_string(),
            "比较板块趋势强度".to_string(),
        ],
        "backtest" => vec!["查看回撤与换手".to_string(), "调整回测时间窗口".to_string()],
        "screen" => vec!["收紧筛选条件".to_string(), "继续做趋势分析".to_string()],
        "observe_stock" => vec!["查看趋势指标".to_string(), "补充资讯证据".to_string()],
        "news_rag" => vec!["核验原始来源".to_string(), "继续看财务和资金面".to_string()],
        "watchlist_action" => vec![
            "用自选股做组合观察".to_string(),
            "对自选股运行回测".to_string(),
        ],
        _ => vec!["补充股票代码、板块或筛选条件".to_string()],
    }
}
pub fn run_agent_with_mock(message: &str) -> CoreResult<AgentResponse> {
    run_agent_with_source(&MockDataSource, message, None)
}

pub fn run_agent_with_data(data: &CoreDataSet, message: &str) -> CoreResult<AgentResponse> {
    run_agent_with_data_and_context(data, message, &AgentContext::default())
}

pub fn run_agent_with_data_and_context(
    data: &CoreDataSet,
    message: &str,
    context: &AgentContext,
) -> CoreResult<AgentResponse> {
    let source = StaticDataSource::new(data);
    run_agent_with_source_and_context(&source, message, None, context)
}

pub fn run_agent_stream_with_data_events(
    data: &CoreDataSet,
    message: &str,
    run_id: Option<&str>,
    mode: Option<&str>,
) -> Vec<AgentStreamEvent> {
    run_agent_stream_with_data_events_with_context(
        data,
        message,
        run_id,
        mode,
        &AgentContext::default(),
    )
}

pub fn run_agent_stream_with_data_events_with_context(
    data: &CoreDataSet,
    message: &str,
    run_id: Option<&str>,
    mode: Option<&str>,
    context: &AgentContext,
) -> Vec<AgentStreamEvent> {
    let source = StaticDataSource::new(data);
    run_agent_stream_with_source_events_with_context(&source, message, run_id, mode, context)
}

pub fn run_agent_with_source(
    source: &impl MarketDataSource,
    message: &str,
    mode: Option<&str>,
) -> CoreResult<AgentResponse> {
    run_agent_with_source_and_context(source, message, mode, &AgentContext::default())
}

pub fn run_agent_with_source_and_context(
    source: &impl MarketDataSource,
    message: &str,
    mode: Option<&str>,
    context: &AgentContext,
) -> CoreResult<AgentResponse> {
    let lower = message.to_lowercase();
    let criteria = heuristic_criteria(message);
    let codes = extract_codes(message);
    let watchlist_codes = agent_watchlist_codes(context);
    let wants_watchlist = contains_any(&lower, &["自选", "自选股", "观察池", "watchlist"]);
    let wants_portfolio = contains_any(
        &lower,
        &[
            "组合",
            "模拟组合",
            "组合观察",
            "回测",
            "backtest",
            "收益曲线",
        ],
    );

    if wants_watchlist && !wants_portfolio {
        let data = agent_watchlist_data(context);
        return Ok(agent_finalize_response(
            AgentResponse {
                intent: None,
                tool_calls: Vec::new(),
                evidence_summary: Vec::new(),
                answer_sections: Vec::new(),
                warnings: Vec::new(),
                next_actions: Vec::new(),
                reply: mode_reply(mode, "已读取本地自选股观察池。"),
                action: "watchlist_action".to_string(),
                criteria: None,
                backtest: None,
                graph_screen: None,
                trend_screen: None,
                data: Some(data),
            },
            message,
            mode,
        ));
    }

    if wants_portfolio {
        let default_end_date = current_system_date_yyyymmdd();
        let use_watchlist = wants_watchlist || !watchlist_codes.is_empty();
        let backtest = BacktestRequest {
            criteria,
            source: if use_watchlist {
                "watchlist".to_string()
            } else {
                default_backtest_source()
            },
            strategy_mode: default_backtest_strategy_mode(),
            stock_codes: if use_watchlist {
                watchlist_codes
            } else {
                Vec::new()
            },
            start_date: extract_date(message, "20200101", true),
            end_date: extract_date(message, &default_end_date, false),
            top_n: default_top_n(),
            initial_cash: default_initial_cash(),
            rebalance_frequency: default_rebalance_frequency(),
            transaction_cost_bps: default_transaction_cost_bps(),
            benchmark: default_backtest_benchmark(),
        };
        let data = serde_json::to_value(backtest_with_source(source, &backtest)?)?;
        return Ok(agent_finalize_response(
            AgentResponse {
                intent: None,
                tool_calls: Vec::new(),
                evidence_summary: Vec::new(),
                answer_sections: Vec::new(),
                warnings: Vec::new(),
                next_actions: Vec::new(),
                reply: mode_reply(mode, "已基于本地数据完成组合观察/回测。"),
                action: "backtest".to_string(),
                criteria: None,
                backtest: Some(backtest),
                graph_screen: None,
                trend_screen: None,
                data: Some(data),
            },
            message,
            mode,
        ));
    }

    if contains_any(
        &lower,
        &[
            "利好", "利空", "新闻", "资讯", "消息", "公告", "舆情", "news", "rag",
        ],
    ) {
        let data = agent_news_data(source, message, &codes)?;
        return Ok(agent_finalize_response(
            AgentResponse {
                intent: None,
                tool_calls: Vec::new(),
                evidence_summary: Vec::new(),
                answer_sections: Vec::new(),
                warnings: Vec::new(),
                next_actions: Vec::new(),
                reply: mode_reply(mode, "已整理本地可用的资讯线索与风险边界。"),
                action: "news_rag".to_string(),
                criteria: None,
                backtest: None,
                graph_screen: None,
                trend_screen: None,
                data: Some(data),
            },
            message,
            mode,
        ));
    }

    if contains_any(
        &lower,
        &[
            "财务",
            "资金",
            "资金面",
            "个股",
            "速览",
            "分析",
            "看一下",
            "观察",
            "snapshot",
            "observe",
        ],
    ) && !codes.is_empty()
    {
        let code = codes.first().cloned().unwrap_or_default();
        let observe_request = StockObserveRequest {
            code,
            start_date: extract_date(message, &default_start_date(), true),
            end_date: extract_date(message, &default_end_date(), false),
            series_limit: default_series_limit(),
            include_order_book: false,
        };
        let data = serde_json::to_value(observe_with_source(source, &observe_request)?)?;
        return Ok(agent_finalize_response(
            AgentResponse {
                intent: None,
                tool_calls: Vec::new(),
                evidence_summary: Vec::new(),
                answer_sections: Vec::new(),
                warnings: Vec::new(),
                next_actions: Vec::new(),
                reply: mode_reply(mode, "已生成个股本地观察摘要。"),
                action: "observe_stock".to_string(),
                criteria: None,
                backtest: None,
                graph_screen: None,
                trend_screen: None,
                data: Some(data),
            },
            message,
            mode,
        ));
    }

    if contains_any(
        &lower,
        &[
            "趋势",
            "趋势股",
            "上升趋势",
            "技术",
            "量价",
            "短线",
            "主力",
            "支撑",
            "阻力",
            "swl",
            "sws",
            "trend",
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
        return Ok(agent_finalize_response(
            AgentResponse {
                intent: None,
                tool_calls: Vec::new(),
                evidence_summary: Vec::new(),
                answer_sections: Vec::new(),
                warnings: Vec::new(),
                next_actions: Vec::new(),
                reply: mode_reply(mode, "已按趋势指标完成候选股排序。"),
                action: "trend_screen".to_string(),
                criteria: None,
                backtest: None,
                graph_screen: None,
                trend_screen: Some(trend_request),
                data: Some(data),
            },
            message,
            mode,
        ));
    }

    if contains_any(
        &lower,
        &[
            "板块",
            "行业",
            "主题",
            "概念",
            "半导体",
            "新能源",
            "白酒",
            "银行",
            "sector",
        ],
    ) {
        let data = serde_json::to_value(screen_with_source(source, &criteria)?)?;
        return Ok(agent_finalize_response(
            AgentResponse {
                intent: None,
                tool_calls: Vec::new(),
                evidence_summary: Vec::new(),
                answer_sections: Vec::new(),
                warnings: Vec::new(),
                next_actions: Vec::new(),
                reply: mode_reply(mode, "已按板块/主题条件完成本地候选筛选。"),
                action: "screen".to_string(),
                criteria: Some(criteria),
                backtest: None,
                graph_screen: None,
                trend_screen: None,
                data: Some(data),
            },
            message,
            mode,
        ));
    }

    if contains_any(
        &lower,
        &["选股", "筛选", "挑股", "候选", "roe", "pe", "pb", "screen"],
    ) {
        let data = serde_json::to_value(screen_with_source(source, &criteria)?)?;
        return Ok(agent_finalize_response(
            AgentResponse {
                intent: None,
                tool_calls: Vec::new(),
                evidence_summary: Vec::new(),
                answer_sections: Vec::new(),
                warnings: Vec::new(),
                next_actions: Vec::new(),
                reply: mode_reply(mode, "已按描述筛选候选股票。"),
                action: "screen".to_string(),
                criteria: Some(criteria),
                backtest: None,
                graph_screen: None,
                trend_screen: None,
                data: Some(data),
            },
            message,
            mode,
        ));
    }

    Ok(agent_finalize_response(
        AgentResponse {
            intent: None,
            tool_calls: Vec::new(),
            evidence_summary: Vec::new(),
            answer_sections: Vec::new(),
            warnings: Vec::new(),
            next_actions: Vec::new(),
            reply: mode_reply(
                mode,
                "请说明要看个股、资讯、趋势筛选、自选股组合观察，还是普通选股。",
            ),
            action: "clarify".to_string(),
            criteria: None,
            backtest: None,
            graph_screen: None,
            trend_screen: None,
            data: None,
        },
        message,
        mode,
    ))
}
pub fn run_agent_stream_with_source_events(
    source: &impl MarketDataSource,
    message: &str,
    run_id: Option<&str>,
    mode: Option<&str>,
) -> Vec<AgentStreamEvent> {
    run_agent_stream_with_source_events_with_context(
        source,
        message,
        run_id,
        mode,
        &AgentContext::default(),
    )
}

pub fn run_agent_stream_with_source_events_with_context(
    source: &impl MarketDataSource,
    message: &str,
    run_id: Option<&str>,
    mode: Option<&str>,
    context: &AgentContext,
) -> Vec<AgentStreamEvent> {
    let run_id = run_id.unwrap_or("gp-agent-run").to_string();
    let mut events = vec![
        agent_status_event(&run_id, "understand", "理解任务", 8, None),
        agent_status_event(&run_id, "intent", "选择工具", 24, None),
        agent_status_event(&run_id, "execute", "执行本地工具", 64, None),
    ];

    match run_agent_with_source_and_context(source, message, mode, context) {
        Ok(response) => {
            let action = response.action.clone();
            for call in response.tool_calls.clone() {
                events.push(agent_payload_event(
                    &run_id,
                    "tool_start",
                    Some(action.as_str()),
                    json!({
                        "id": call.id,
                        "tool": call.tool,
                        "label": call.label,
                        "input": call.input,
                    }),
                ));
                events.push(agent_payload_event(
                    &run_id,
                    "tool_result",
                    Some(action.as_str()),
                    json!({
                        "id": call.id,
                        "tool": call.tool,
                        "status": call.status,
                        "output_summary": call.output_summary,
                        "warnings": call.warnings,
                    }),
                ));
            }
            if !response.evidence_summary.is_empty() {
                events.push(agent_payload_event(
                    &run_id,
                    "evidence",
                    Some(action.as_str()),
                    json!({ "items": response.evidence_summary.clone() }),
                ));
            }
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
            events.push(agent_payload_event(
                &run_id,
                "final",
                Some(action.as_str()),
                json!({
                    "intent": response.intent.clone(),
                    "warnings": response.warnings.clone(),
                    "next_actions": response.next_actions.clone(),
                }),
            ));
            events.push(AgentStreamEvent {
                run_id,
                event_type: "result".to_string(),
                stage: None,
                label: None,
                percent: None,
                action: Some(action),
                response: Some(response),
                payload: None,
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
                payload: None,
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
        payload: None,
        message: None,
    }
}

fn agent_payload_event(
    run_id: &str,
    event_type: &str,
    action: Option<&str>,
    payload: Value,
) -> AgentStreamEvent {
    AgentStreamEvent {
        run_id: run_id.to_string(),
        event_type: event_type.to_string(),
        stage: None,
        label: None,
        percent: None,
        action: action.map(ToOwned::to_owned),
        response: None,
        payload: Some(payload),
        message: None,
    }
}

pub fn run_mobile_stock_skill(request: &MobileStockSkillRequest) -> MobileStockSkillResult {
    let mut positive_factors = Vec::new();
    let mut negative_factors = Vec::new();
    let mut neutral_information = Vec::new();
    let mut unverified_leads = Vec::new();
    let mut notes = vec![
        "手机端股票分析 Skill 已按结构化信源生成结论。".to_string(),
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
fn agent_watchlist_codes(context: &AgentContext) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut codes = Vec::new();
    for item in &context.watchlist {
        let code = normalize_stock_code(&item.code);
        if !code.is_empty() && seen.insert(code.clone()) {
            codes.push(code);
        }
    }
    codes
}

fn agent_watchlist_data(context: &AgentContext) -> Value {
    let codes = agent_watchlist_codes(context);
    let items = context
        .watchlist
        .iter()
        .map(|item| {
            json!({
                "code": normalize_stock_code(&item.code),
                "name": item.name,
                "industry": item.industry,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "source": "local_watchlist",
        "total": items.len(),
        "returned": items.len(),
        "codes": codes,
        "items": items,
        "notes": ["只读取本地自选股观察池，不执行真实交易。"],
    })
}

fn agent_news_data(
    source: &impl MarketDataSource,
    message: &str,
    codes: &[String],
) -> CoreResult<Value> {
    let universe = source.stocks()?;
    let mut items = Vec::new();
    for code in codes.iter().take(5) {
        let stock = universe
            .iter()
            .find(|item| normalize_stock_code(&item.code) == normalize_stock_code(code));
        items.push(json!({
            "code": code,
            "name": stock.map(|item| item.name.clone()).unwrap_or_default(),
            "source": "local_cache",
            "level": "degraded",
            "summary": "当前核心层未接入外部新闻抓取；请结合桌面新闻 RAG 或导入的移动 RAG 包复核。",
        }));
    }
    if items.is_empty() {
        items.push(json!({
            "source": "local_cache",
            "level": "degraded",
            "summary": "未识别到股票代码，已保留为待验证资讯研究问题。",
        }));
    }
    Ok(json!({
        "query": message,
        "source": "local_agent_news_stub",
        "returned": items.len(),
        "items": items,
        "notes": ["社区或未验证来源只作为待验证线索；不输出买卖、目标价或收益承诺。"],
    }))
}
fn mode_reply(mode: Option<&str>, text: &str) -> String {
    match mode.unwrap_or("research") {
        "quick" => text.to_string(),
        "expert" => format!(
            "{text}\n\n风险提示：以上为本地策略结果，需结合基本面与市场环境复核。\n下一步：可对候选股做观察或回测验证。"
        ),
        _ => format!("{text} 仅供选股研究，不构成投资建议。"),
    }
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
            "改善", "突破", "获批", "投产", "景气", "涨价", "positive", "beat", "upgrade",
        ],
    );
    let negative = contains_any(
        &text,
        &[
            "下滑",
            "下降",
            "亏损",
            "预亏",
            "减持",
            "处罚",
            "调查",
            "诉讼",
            "仲裁",
            "违约",
            "终止",
            "取消",
            "风险",
            "计提",
            "减值",
            "停产",
            "限产",
            "退市",
            "negative",
            "miss",
            "downgrade",
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
    format!("{prefix}: {}", truncate_chars(&body, 120))
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

    apply_industry_relative_scores(&mut screened, &criteria.score_profile);
    sort_screened(&mut screened, criteria);

    let limit = criteria.limit.clamp(1, 200);
    let (items, promoted) = primary_screen_items(&screened, criteria, limit);
    let groups = screen_result_groups(&screened);
    let mut notes = notes;
    if promoted {
        notes.push("基准为候选池买入持有等权曲线，用于衡量调仓与成本后的超额收益。".to_string());
    }
    notes.push(format!(
        "普通筛选已拆成热门股和综合分大于 {} 的潜力股，每类最多 10 只。",
        POTENTIAL_SCORE_THRESHOLD
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
        vec![format!("扣非净利润规则已启用；当前股票池 {with_metrics}/{} 只股票带扣非财务字段，缺字段股票按不达标处理。", universe.len())]
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
    _provider_relations: &[StockRelation],
    request: &GraphScreenRequest,
) -> GraphScreenResult {
    let mut criteria = request.criteria.clone();
    criteria.limit = request.limit.clamp(1, 100);
    let screen_result = screen_stocks(universe, &criteria);
    let center_context = GraphCenterContext {
        mode: "custom_criteria".to_string(),
        label: "自定义筛选".to_string(),
        codes: Vec::new(),
    };

    let mut signals: Vec<GraphStockSignal> = screen_result
        .items
        .into_iter()
        .map(|screened| {
            let score = round6(screened.score);
            GraphStockSignal {
                stock: screened.stock,
                base_score: score,
                relation_score: 0.0,
                final_score: score,
                suggested_weight: 0.0,
                reasons: screened.reasons,
                related: Vec::new(),
                explanation: SelectionExplanation {
                    basis: vec![screened.score_explanation],
                    ..SelectionExplanation::default()
                },
            }
        })
        .collect();
    signals.sort_by(|left, right| right.final_score.total_cmp(&left.final_score));
    signals.truncate(request.limit.clamp(1, 100));
    assign_weights(&mut signals);

    let mut notes = vec![
        "自定义筛选使用页面筛选条件，不再依赖中心股票或关系传播。".to_string(),
        "旧关系图入口已降级为自定义筛选兼容结果，relation_count 固定为 0。".to_string(),
    ];
    notes.extend(screen_result.notes);

    GraphScreenResult {
        total: screen_result.total,
        returned: signals.len(),
        relation_count: 0,
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

fn finite_positive(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite() && *value > 0.0)
}

fn meets_min_percent(value: Option<f64>, minimum: f64, inclusive: bool) -> bool {
    let Some(value) = value.and_then(as_percent) else {
        return false;
    };
    let Some(minimum) = as_percent(minimum) else {
        return false;
    };
    if inclusive {
        value >= minimum
    } else {
        value > minimum
    }
}

fn matches_stock(stock: &StockItem, criteria: &ScreenCriteria) -> Option<Vec<String>> {
    let mut reasons = Vec::new();
    if !stock.price.is_finite() || stock.price <= 0.0 {
        return None;
    }
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
        if !meets_min_percent(stock.roe, min_roe, true) {
            return None;
        }
        reasons.push("roe_ok".to_string());
    }

    if let Some(max_pe) = criteria.max_pe {
        if !max_pe.is_finite()
            || max_pe <= 0.0
            || finite_positive(stock.pe)
                .map(|pe| pe > max_pe)
                .unwrap_or(true)
        {
            return None;
        }
        reasons.push("pe_ok".to_string());
    }

    if let Some(max_pb) = criteria.max_pb {
        if !max_pb.is_finite()
            || max_pb <= 0.0
            || finite_positive(stock.pb)
                .map(|pb| pb > max_pb)
                .unwrap_or(true)
        {
            return None;
        }
        reasons.push("pb_ok".to_string());
    }

    if let Some(min_market_cap) = criteria.min_market_cap_billion {
        if !min_market_cap.is_finite()
            || min_market_cap < 0.0
            || finite_positive(stock.market_cap_billion)
                .map(|market_cap| market_cap < min_market_cap)
                .unwrap_or(true)
        {
            return None;
        }
        reasons.push("mcap_ok".to_string());
    }

    if let Some(min_profit) = criteria.min_deducted_net_profit_billion {
        if !min_profit.is_finite()
            || stock
                .deducted_net_profit_billion
                .filter(|profit| profit.is_finite())
                .map(|profit| profit <= min_profit)
                .unwrap_or(true)
        {
            return None;
        }
        reasons.push("deducted_net_profit_ok".to_string());
    }

    if let Some(min_margin) = criteria.min_deducted_net_profit_margin {
        if !meets_min_percent(stock.deducted_net_profit_margin, min_margin, false) {
            return None;
        }
        reasons.push("deducted_net_profit_margin_ok".to_string());
    }

    if let Some(min_growth) = criteria.min_deducted_net_profit_growth_rate {
        if !meets_min_percent(stock.deducted_net_profit_growth_rate, min_growth, false) {
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
const CONCEPT_GROUP_RULES: [(&str, &[&str]); 20] = [
    (
        "半导体设计",
        &[
            "半导体设计",
            "芯片设计",
            "集成电路设计",
            "ic设计",
            "soc",
            "mcu",
            "模拟芯片",
            "功率芯片",
            "射频芯片",
            "传感器芯片",
            "eda",
        ],
    ),
    (
        "半导体设备",
        &[
            "半导体设备",
            "光刻机",
            "刻蚀",
            "薄膜沉积",
            "离子注入",
            "量测设备",
            "检测设备",
            "清洗设备",
            "涂胶显影",
            "测试设备",
            "探针台",
            "分选机",
        ],
    ),
    (
        "半导体材料",
        &[
            "半导体材料",
            "光刻胶",
            "电子特气",
            "湿电子化学品",
            "靶材",
            "抛光液",
            "抛光垫",
            "cmp",
            "硅片",
            "衬底",
            "碳化硅",
            "sic",
            "氮化镓",
            "gan",
        ],
    ),
    (
        "半导体晶圆",
        &[
            "半导体晶圆",
            "晶圆",
            "晶圆代工",
            "晶圆制造",
            "晶圆厂",
            "硅晶圆",
            "外延片",
            "外延硅片",
            "半导体衬底",
            "碳化硅衬底",
            "sic衬底",
            "抛光片",
            "8英寸",
            "12英寸",
        ],
    ),
    (
        "半导体封测",
        &[
            "封测",
            "封装测试",
            "半导体封装",
            "芯片封装",
            "先进封装",
            "测试服务",
            "晶圆测试",
            "chiplet",
            "2.5d",
            "3d封装",
            "cowos",
        ],
    ),
    (
        "存储芯片",
        &[
            "存储芯片",
            "存储器",
            "dram",
            "nand",
            "nor flash",
            "hbm",
            "固态硬盘",
            "ssd",
            "闪存",
            "内存",
        ],
    ),
    (
        "AI算力与芯片",
        &[
            "算力",
            "人工智能",
            "ai",
            "光模块",
            "cpo",
            "服务器",
            "液冷",
            "gpu",
            "数据中心",
            "云计算",
            "大模型",
            "aigc",
            "边缘计算",
            "pcb",
        ],
    ),
    (
        "新材料",
        &[
            "氟化工",
            "氟材料",
            "锂电材料",
            "电解液",
            "六氟磷酸锂",
            "新能材",
            "新材料",
            "固态电池",
            "磁材",
        ],
    ),
    (
        "新能源与储能",
        &[
            "新能源",
            "电池",
            "储能",
            "光伏",
            "电力",
            "能源",
            "风电",
            "充电桩",
        ],
    ),
    (
        "游戏传媒",
        &[
            "游戏",
            "网络游戏",
            "手游",
            "电竞",
            "云游戏",
            "互动娱乐",
            "传媒",
            "广告营销",
        ],
    ),
    (
        "机器人与高端制造",
        &[
            "机器人",
            "工业母机",
            "自动化",
            "高端制造",
            "智能制造",
            "机械设备",
        ],
    ),
    (
        "消费零售",
        &[
            "食品",
            "饮料",
            "白酒",
            "休闲食品",
            "一般零售",
            "商贸零售",
            "家电",
            "旅游",
            "酒店",
            "餐饮",
        ],
    ),
    (
        "医药医疗",
        &[
            "医药",
            "医疗",
            "生物制品",
            "创新药",
            "中药",
            "化学制药",
            "医疗器械",
            "cro",
        ],
    ),
    (
        "金融地产",
        &["银行", "证券", "保险", "房地产", "地产", "物业"],
    ),
    (
        "基建建筑",
        &[
            "建筑",
            "房屋建设",
            "工程建设",
            "基础建设",
            "水泥",
            "铁路",
            "公路",
            "装修装饰",
        ],
    ),
    (
        "周期资源",
        &[
            "煤炭", "钢铁", "普钢", "有色", "金属", "化工", "石油", "油气", "矿业",
        ],
    ),
    (
        "汽车产业链",
        &[
            "汽车",
            "整车",
            "零部件",
            "轮胎",
            "智能驾驶",
            "无人驾驶",
            "汽车服务",
        ],
    ),
    (
        "军工航天",
        &["军工", "航天", "航空", "卫星", "船舶", "无人机", "国防"],
    ),
    (
        "交运物流",
        &[
            "物流",
            "航运",
            "港口",
            "机场",
            "航空运输",
            "铁路运输",
            "快递",
        ],
    ),
    ("公用环保", &["环保", "水务", "燃气", "供热", "公用事业"]),
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
    let quality_profile = normalized_score_profile(score_profile) == "quality";
    let cold_penalty = cold && !quality_profile;
    let theme_score = theme.as_ref().map(|(_, _, score)| *score).unwrap_or(0.35);
    let fundamental = fundamental_score(stock);
    let valuation = valuation_score(stock);
    let size = size_score(stock);
    let risk = risk_score(stock, cold_penalty);
    let data_quality = data_quality_score(stock);
    let market_heat = market_heat_score(stock);
    let overheat_penalty = overheat_penalty_score(stock);

    let scoring_factors = BTreeMap::from([
        ("theme".to_string(), theme_score),
        ("fundamental".to_string(), fundamental),
        ("valuation".to_string(), valuation),
        ("size".to_string(), size),
        ("risk".to_string(), risk),
        ("data_quality".to_string(), data_quality),
        ("market_heat".to_string(), market_heat),
        ("overheat_penalty".to_string(), overheat_penalty),
    ]);

    let quality_score = profile_score(&scoring_factors, "quality");
    let trend_score = profile_score(&scoring_factors, "trend");
    let balanced_score = profile_score(&scoring_factors, "balanced");
    let selected_score = profile_score(&scoring_factors, score_profile);
    let score = (selected_score * SCREEN_SCORE_SCALE).clamp(0.0, SCREEN_SCORE_SCALE);

    let mut public_factors = BTreeMap::from([
        ("theme".to_string(), theme_score),
        ("fundamental".to_string(), fundamental),
        ("valuation".to_string(), valuation),
        ("size".to_string(), size),
        ("risk".to_string(), risk),
        ("data_quality".to_string(), data_quality),
        ("quality_score".to_string(), quality_score),
        ("trend_score".to_string(), trend_score),
        ("balanced_score".to_string(), balanced_score),
    ]);
    if uses_market_heat_score_profile(score_profile) {
        public_factors.insert("market_heat".to_string(), market_heat);
        public_factors.insert("overheat_penalty".to_string(), overheat_penalty);
    }

    let profile_theme = if quality_profile {
        None
    } else {
        theme.as_ref()
    };
    let mut all_reasons = reasons.to_vec();
    all_reasons.extend(factor_reasons(
        profile_theme,
        cold_penalty,
        &scoring_factors,
    ));
    let reason_tags = build_reason_tags(profile_theme, &scoring_factors);
    let risk_tags = build_risk_tags(stock, cold_penalty, &scoring_factors);
    let suitable_periods = suitable_periods_for_scores(quality_score, trend_score, risk);
    let score_breakdown = profile_score_breakdown(score_profile, &scoring_factors);
    let score_explanation = explain_score(stock, profile_theme, cold_penalty, &scoring_factors);
    let rounded_scores = public_factors
        .into_iter()
        .map(|(key, value)| (key, round6(value)))
        .collect();

    ScreenedStock {
        stock: stock.clone(),
        score: round6(score),
        reasons: all_reasons,
        quality_score: round6(quality_score * SCREEN_SCORE_SCALE),
        trend_score: round6(trend_score * SCREEN_SCORE_SCALE),
        risk_score: round6(risk * SCREEN_SCORE_SCALE),
        balanced_score: round6(balanced_score * SCREEN_SCORE_SCALE),
        factor_scores: rounded_scores,
        score_breakdown,
        score_explanation,
        reason_tags,
        risk_tags,
        suitable_periods,
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
    let Some(value) = value.filter(|value| value.is_finite()) else {
        return 0.52;
    };
    match value {
        value if value <= 0.0 => 0.25,
        value if value <= 15.0 => 1.0,
        value if value <= 30.0 => 0.72,
        value if value <= 60.0 => 0.45,
        _ => 0.28,
    }
}

fn pb_score(value: Option<f64>) -> f64 {
    let Some(value) = value.filter(|value| value.is_finite()) else {
        return 0.52;
    };
    match value {
        value if value <= 0.0 => 0.25,
        value if value <= 1.5 => 1.0,
        value if value <= 3.0 => 0.74,
        value if value <= 6.0 => 0.5,
        value if value <= 10.0 => 0.34,
        _ => 0.22,
    }
}

fn size_score(stock: &StockItem) -> f64 {
    match finite_positive(stock.market_cap_billion) {
        None => 0.55,
        Some(value) if value < 20.0 => 0.36,
        Some(value) if value < 100.0 => 0.68,
        Some(value) if value < 500.0 => 0.88,
        Some(value) if value < 2000.0 => 0.78,
        Some(_) => 0.62,
    }
}

fn data_quality_score(stock: &StockItem) -> f64 {
    let mut score: f64 = 0.0;
    if stock.roe.and_then(as_percent).is_some() {
        score += 0.30;
    }
    if finite_positive(stock.pe).is_some() {
        score += 0.20;
    }
    if finite_positive(stock.pb).is_some() {
        score += 0.15;
    }
    if finite_positive(stock.market_cap_billion).is_some() {
        score += 0.10;
    }
    if stock
        .deducted_net_profit_billion
        .filter(|value| value.is_finite())
        .is_some()
    {
        score += 0.10;
    }
    if stock
        .deducted_net_profit_growth_rate
        .and_then(as_percent)
        .is_some()
    {
        score += 0.10;
    }
    if stock
        .dividend_yield
        .and_then(as_percent)
        .filter(|value| *value >= 0.0)
        .is_some()
    {
        score += 0.05;
    }
    round6(score.clamp(0.0, 1.0))
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

fn normalized_score_profile(score_profile: &str) -> &'static str {
    match score_profile.trim().to_ascii_lowercase().as_str() {
        "rotation" => "rotation",
        "trend" | "trend_swing" => "trend",
        "quality" => "quality",
        _ => "balanced",
    }
}

fn uses_market_heat_score_profile(score_profile: &str) -> bool {
    matches!(
        normalized_score_profile(score_profile),
        "balanced" | "trend" | "rotation"
    )
}

fn profile_score(factor_scores: &BTreeMap<String, f64>, score_profile: &str) -> f64 {
    let theme = factor_scores.get("theme").copied().unwrap_or(0.35);
    let fundamental = factor_scores.get("fundamental").copied().unwrap_or(0.5);
    let valuation = factor_scores.get("valuation").copied().unwrap_or(0.5);
    let size = factor_scores.get("size").copied().unwrap_or(0.55);
    let risk = factor_scores.get("risk").copied().unwrap_or(1.0);
    let data_quality = factor_scores.get("data_quality").copied().unwrap_or(0.0);
    let market_heat = factor_scores.get("market_heat").copied().unwrap_or(0.5);
    let overheat_penalty = factor_scores
        .get("overheat_penalty")
        .copied()
        .unwrap_or(0.0);

    let raw = match normalized_score_profile(score_profile) {
        "rotation" => {
            theme * 0.18
                + fundamental * 0.18
                + valuation * 0.16
                + market_heat * 0.22
                + size * 0.08
                + risk * 0.18
                - overheat_penalty * 0.10
        }
        "trend" => {
            theme * 0.16
                + fundamental * 0.16
                + valuation * 0.12
                + market_heat * 0.30
                + size * 0.08
                + risk * 0.18
                - overheat_penalty * 0.16
        }
        "quality" => {
            fundamental * 0.40 + valuation * 0.28 + size * 0.10 + risk * 0.10 + data_quality * 0.12
        }
        _ => {
            theme * 0.16
                + fundamental * 0.26
                + valuation * 0.20
                + market_heat * 0.16
                + size * 0.08
                + risk * 0.22
                - overheat_penalty * 0.08
        }
    };
    raw.clamp(0.0, 1.0)
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

fn overheat_penalty_score(stock: &StockItem) -> f64 {
    let change_pct = stock.change_pct.and_then(as_percent).unwrap_or(0.0);
    let turnover = stock.turnover_rate.and_then(as_percent).unwrap_or(0.0);
    let volume_ratio = finite_positive(stock.volume_ratio).unwrap_or(0.0);
    let change_penalty = if change_pct > 7.0 {
        ((change_pct - 7.0) / 5.0).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let turnover_penalty = if turnover > 8.0 {
        ((turnover - 8.0) / 8.0).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let volume_penalty = if volume_ratio > 3.0 {
        ((volume_ratio - 3.0) / 2.0).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (change_penalty * 0.50 + turnover_penalty * 0.28 + volume_penalty * 0.22).clamp(0.0, 1.0)
}

fn profile_score_breakdown(
    score_profile: &str,
    factor_scores: &BTreeMap<String, f64>,
) -> Vec<ScoreContribution> {
    let weights: &[(&str, &str, f64)] = match normalized_score_profile(score_profile) {
        "quality" => &[
            ("fundamental", "质量盈利", 0.40),
            ("valuation", "估值安全", 0.28),
            ("size", "规模流动性", 0.10),
            ("risk", "基础风险", 0.10),
            ("data_quality", "数据完整度", 0.12),
        ],
        "trend" => &[
            ("theme", "主题匹配", 0.16),
            ("fundamental", "质量底座", 0.16),
            ("valuation", "估值约束", 0.12),
            ("market_heat", "趋势热度", 0.30),
            ("size", "规模流动性", 0.08),
            ("risk", "风险控制", 0.18),
            ("overheat_penalty", "过热惩罚", -0.16),
        ],
        "rotation" => &[
            ("theme", "主题匹配", 0.18),
            ("fundamental", "质量底座", 0.18),
            ("valuation", "估值约束", 0.16),
            ("market_heat", "轮动热度", 0.22),
            ("size", "规模流动性", 0.08),
            ("risk", "风险控制", 0.18),
            ("overheat_penalty", "过热惩罚", -0.10),
        ],
        _ => &[
            ("theme", "主题匹配", 0.16),
            ("fundamental", "质量盈利", 0.26),
            ("valuation", "估值安全", 0.20),
            ("market_heat", "趋势热度", 0.16),
            ("size", "规模流动性", 0.08),
            ("risk", "风险控制", 0.22),
            ("overheat_penalty", "过热惩罚", -0.08),
        ],
    };
    weights
        .iter()
        .map(|(key, label, weight)| {
            let value = factor_scores
                .get(*key)
                .copied()
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            let contribution = value * weight * SCREEN_SCORE_SCALE;
            let tone = if *weight < 0.0 {
                if value >= 0.5 {
                    "weak"
                } else {
                    "neutral"
                }
            } else if value >= 0.74 {
                "strong"
            } else if value >= 0.55 {
                "positive"
            } else if value <= 0.35 {
                "weak"
            } else {
                "neutral"
            };
            ScoreContribution {
                key: (*key).to_string(),
                label: (*label).to_string(),
                value: Some(round6(value * SCREEN_SCORE_SCALE)),
                contribution: Some(round6(contribution)),
                tone: tone.to_string(),
            }
        })
        .collect()
}

fn build_reason_tags(
    theme: Option<&(&'static str, &'static str, f64)>,
    factor_scores: &BTreeMap<String, f64>,
) -> Vec<String> {
    let mut tags = Vec::new();
    if let Some((_, label, _)) = theme {
        tags.push(format!("主题:{label}"));
    }
    if factor_scores.get("fundamental").copied().unwrap_or(0.0) >= 0.72 {
        tags.push("质量较强".to_string());
    }
    if factor_scores.get("valuation").copied().unwrap_or(0.0) >= 0.72 {
        tags.push("估值有安全边际".to_string());
    }
    if factor_scores.get("market_heat").copied().unwrap_or(0.0) >= 0.72 {
        tags.push("趋势热度较高".to_string());
    }
    if factor_scores.get("risk").copied().unwrap_or(0.0) >= 0.72 {
        tags.push("风险约束通过".to_string());
    }
    if factor_scores.get("data_quality").copied().unwrap_or(0.0) >= 0.85 {
        tags.push("核心数据完整".to_string());
    }
    tags
}

fn build_risk_tags(
    stock: &StockItem,
    cold: bool,
    factor_scores: &BTreeMap<String, f64>,
) -> Vec<String> {
    let mut tags = Vec::new();
    if stock.is_st {
        tags.push("ST 风险".to_string());
    }
    if cold {
        tags.push("低热度板块降权".to_string());
    }
    if factor_scores.get("valuation").copied().unwrap_or(0.0) <= 0.38 {
        tags.push("估值偏高".to_string());
    }
    if factor_scores.get("fundamental").copied().unwrap_or(0.0) <= 0.38 {
        tags.push("质量偏弱".to_string());
    }
    if factor_scores.get("data_quality").copied().unwrap_or(0.0) < 0.70 {
        tags.push("核心财务数据缺失较多".to_string());
    }
    if factor_scores
        .get("overheat_penalty")
        .copied()
        .unwrap_or(0.0)
        >= 0.35
    {
        tags.push("短线过热".to_string());
    }
    if tags.is_empty() {
        tags.push("未识别明确高风险".to_string());
    }
    tags
}

fn suitable_periods_for_scores(quality_score: f64, trend_score: f64, risk: f64) -> Vec<String> {
    let mut periods = Vec::new();
    if trend_score >= 0.62 && risk >= 0.5 {
        periods.push("5-10日观察".to_string());
    }
    if quality_score >= 0.58 && risk >= 0.5 {
        periods.push("20-60日观察".to_string());
    }
    if periods.is_empty() {
        periods.push("仅观察验证".to_string());
    }
    periods
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
    if factor_scores.get("data_quality").copied().unwrap_or(0.0) >= 0.85 {
        reasons.push("核心财务数据完整".to_string());
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
    parts.push(format!(
        "数据完整度{}",
        tier_word(factor_scores.get("data_quality").copied().unwrap_or(0.0))
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

fn is_cold_sector(industry: &str) -> bool {
    let normalized = industry.trim().to_lowercase();
    if normalized.is_empty() {
        return false;
    }
    COLD_SECTOR_KEYWORDS
        .iter()
        .any(|keyword| normalized.contains(keyword))
}

fn should_promote_hot_sectors(criteria: &ScreenCriteria) -> bool {
    criteria.industry.as_deref().unwrap_or("").trim().is_empty()
        && criteria.sort_by.trim().eq_ignore_ascii_case("score")
        && !criteria.sort_dir.trim().eq_ignore_ascii_case("asc")
        && normalized_score_profile(&criteria.score_profile) != "quality"
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

fn apply_industry_relative_scores(screened: &mut [ScreenedStock], score_profile: &str) {
    if screened.len() < 3 {
        return;
    }

    let quality_values = industry_percentiles(screened, "fundamental");
    let valuation_values = industry_percentiles(screened, "valuation");
    let profile = normalized_score_profile(score_profile);

    for item in screened.iter_mut() {
        let quality_pct = quality_values.get(&item.stock.code).copied().unwrap_or(0.5);
        let valuation_pct = valuation_values
            .get(&item.stock.code)
            .copied()
            .unwrap_or(0.5);
        let relative_score = ((quality_pct + valuation_pct) / 2.0).clamp(0.0, 1.0);
        let old_quality = item.quality_score;
        let old_balanced = item.balanced_score;
        item.quality_score = round6(
            (old_quality * 0.82 + relative_score * SCREEN_SCORE_SCALE * 0.18)
                .clamp(0.0, SCREEN_SCORE_SCALE),
        );
        item.balanced_score = round6(
            (old_balanced * 0.88 + relative_score * SCREEN_SCORE_SCALE * 0.12)
                .clamp(0.0, SCREEN_SCORE_SCALE),
        );
        item.factor_scores
            .insert("industry_quality_pct".to_string(), round6(quality_pct));
        item.factor_scores
            .insert("industry_valuation_pct".to_string(), round6(valuation_pct));
        item.factor_scores.insert(
            "industry_relative_score".to_string(),
            round6(relative_score),
        );
        item.reason_tags.push("行业内相对评分".to_string());

        item.score = match profile {
            "quality" => item.quality_score,
            "balanced" => item.balanced_score,
            _ => item.score,
        };
    }
}

fn industry_percentiles(screened: &[ScreenedStock], factor_key: &str) -> HashMap<String, f64> {
    let mut by_industry: HashMap<String, Vec<(String, f64)>> = HashMap::new();
    for item in screened {
        let industry = if item.stock.industry.trim().is_empty() {
            "__unknown__"
        } else {
            item.stock.industry.trim()
        };
        by_industry.entry(industry.to_string()).or_default().push((
            item.stock.code.clone(),
            item.factor_scores.get(factor_key).copied().unwrap_or(0.5),
        ));
    }

    let mut result = HashMap::new();
    for values in by_industry.values_mut() {
        values.sort_by(|left, right| {
            left.1
                .total_cmp(&right.1)
                .then_with(|| left.0.cmp(&right.0))
        });
        let count = values.len();
        let denom = count.saturating_sub(1).max(1) as f64;
        let reliability = (count.saturating_sub(1) as f64 / 7.0).clamp(0.0, 1.0);
        let mut start = 0;
        while start < count {
            let mut end = start + 1;
            while end < count && values[end].1.total_cmp(&values[start].1).is_eq() {
                end += 1;
            }
            let mid_rank = (start + end - 1) as f64 / 2.0;
            let raw_percentile = if count == 1 { 0.5 } else { mid_rank / denom };
            let percentile = 0.5 + (raw_percentile - 0.5) * reliability;
            for (code, _) in &values[start..end] {
                result.insert(code.clone(), percentile.clamp(0.0, 1.0));
            }
            start = end;
        }
    }
    result
}

fn diversify_by_industry(screened: &[ScreenedStock], limit: usize) -> Vec<ScreenedStock> {
    if limit == 0 || screened.len() <= 3 {
        return screened.iter().take(limit).cloned().collect();
    }
    let per_industry_limit = ((limit as f64) * 0.45).ceil().max(2.0) as usize;
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut selected = Vec::new();
    let mut deferred = Vec::new();

    for item in screened {
        let industry = if item.stock.industry.trim().is_empty() {
            "__unknown__".to_string()
        } else {
            item.stock.industry.trim().to_string()
        };
        let count = counts.get(&industry).copied().unwrap_or(0);
        if count < per_industry_limit {
            counts.insert(industry, count + 1);
            selected.push(item.clone());
        } else {
            deferred.push(item.clone());
        }
        if selected.len() >= limit {
            return selected;
        }
    }

    for item in deferred {
        selected.push(item);
        if selected.len() >= limit {
            break;
        }
    }
    selected
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

fn should_diversify_by_industry(criteria: &ScreenCriteria) -> bool {
    criteria.industry.as_deref().unwrap_or("").trim().is_empty()
        && criteria.sort_by.trim().eq_ignore_ascii_case("score")
        && !criteria.sort_dir.trim().eq_ignore_ascii_case("asc")
        && normalized_score_profile(&criteria.score_profile) != "quality"
}

fn primary_screen_items(
    screened: &[ScreenedStock],
    criteria: &ScreenCriteria,
    limit: usize,
) -> (Vec<ScreenedStock>, bool) {
    if !should_promote_hot_sectors(criteria) || limit == 0 {
        let items = if should_diversify_by_industry(criteria) {
            diversify_by_industry(screened, limit)
        } else {
            screened.iter().take(limit).cloned().collect()
        };
        let changed = items
            .iter()
            .map(|item| item.stock.code.as_str())
            .collect::<Vec<_>>()
            != screened
                .iter()
                .take(limit)
                .map(|item| item.stock.code.as_str())
                .collect::<Vec<_>>();
        return (items, changed);
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
    let value = match sort_by {
        "price" => Some(item.stock.price),
        "pe" => item.stock.pe,
        "pb" => item.stock.pb,
        "roe" => item.stock.roe.and_then(as_percent),
        "market_cap" | "market_cap_billion" => item.stock.market_cap_billion,
        "change_pct" | "pct" => item.stock.change_pct.and_then(as_percent),
        _ => Some(item.score),
    };
    value.filter(|value| value.is_finite())
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
            open: open[index],
            high: high[index],
            low: low[index],
            volume: volume[index],
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
        signal_type: default_trend_signal_type(),
        risk_flags: trend_risk_flags_from_bar(bar),
        technical_score: None,
        pattern_layer_score: None,
        quality_score: None,
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
        open: finite_round4(bar.open),
        high: finite_round4(bar.high),
        low: finite_round4(bar.low),
        volume: finite_round4(bar.volume),
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

#[derive(Clone, Debug)]
struct TrendLayerScores {
    technical_score: f64,
    pattern_score: f64,
    quality_score: f64,
    trend_score: f64,
    final_score: f64,
    signal_type: String,
    risk_flags: Vec<String>,
    reason_tags: Vec<String>,
}

fn layered_trend_score(stock: &StockItem, trend: &TrendIndicatorResult) -> TrendLayerScores {
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

fn trend_screen_candidate_allowed(layered: &TrendLayerScores) -> bool {
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
    if let Some(roe) = stock.roe.and_then(as_percent) {
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
    if let Some(growth) = stock.deducted_net_profit_growth_rate.and_then(as_percent) {
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
    if let Some(margin) = stock.deducted_net_profit_margin.and_then(as_percent) {
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
    let mut flags = Vec::new();
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

fn trend_risk_flags_from_bar(bar: &ComputedTrendBar) -> Vec<String> {
    let mut flags = Vec::new();
    if bar.white_exit {
        flags.push("white_exit".to_string());
    }
    if bar.kdj_overbought {
        flags.push("kdj_overbought".to_string());
    }
    if bar.kdj_dead_cross {
        flags.push("kdj_dead_cross".to_string());
    }
    if !bar.swl_above_sws {
        flags.push("swl_below_sws".to_string());
    }
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

fn pct_change(current: f64, previous: f64) -> Option<f64> {
    if current.is_finite() && previous.is_finite() && previous.abs() > f64::EPSILON {
        Some((current / previous - 1.0) * 100.0)
    } else {
        None
    }
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
fn trend_screen_explanation(
    candidate: &ScreenedStock,
    signal: &TrendIndicatorSignal,
    layered: &TrendLayerScores,
) -> SelectionExplanation {
    let mut basis = vec![
        format!(
            "Passed the current base screen with raw score {:.2}.",
            candidate.score
        ),
        format!(
            "{} signal with technical {:.2}, pattern {:.2}, quality {:.2}.",
            signal.signal_type,
            layered.technical_score,
            layered.pattern_score,
            layered.quality_score
        ),
    ];
    if let Some(pattern_text) = pattern_basis_text(signal) {
        basis.push(pattern_text);
    }

    let base_component = candidate.score.clamp(0.0, 20.0) / 20.0 * 10.0;
    SelectionExplanation {
        basis,
        score_breakdown: vec![
            score_contribution("technical_score", "Technical layer", layered.technical_score, layered.technical_score * 0.70, 75.0, 60.0),
            score_contribution("pattern_score", "Pattern layer", layered.pattern_score, layered.pattern_score * 0.20, 70.0, 55.0),
            score_contribution("quality_score", "Quality soft score", layered.quality_score, layered.quality_score * 0.10, 70.0, 50.0),
            score_contribution("base_score", "Base screen soft tie-breaker", candidate.score, base_component, 14.0, 9.0),
            score_contribution("final_score", "Final score", layered.final_score, layered.final_score, 80.0, 65.0),
        ],
        risk_checks: trend_risk_checks(signal),
        verification: vec![
            "Confirm the signal with next-day price-volume action before entry.".to_string(),
            "Use trend-screen labels to separate breakout, pullback, continuation, and risk candidates.".to_string(),
        ],
    }
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

fn trend_risk_checks(signal: &TrendIndicatorSignal) -> Vec<String> {
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
        risks.push("No major break or high-zone risk is active, but next-day price-volume confirmation is still required.".to_string());
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
        factor_snapshot_symbol_count: data.factor_snapshots.len(),
        factor_snapshot_count: data.factor_snapshots.values().map(Vec::len).sum(),
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

fn backtest_metrics(
    equity_curve: &[EquityPoint],
    initial_cash: f64,
) -> (f64, Option<f64>, Option<f64>) {
    let Some(first) = equity_curve.first() else {
        return (0.0, None, None);
    };
    let Some(last) = equity_curve.last() else {
        return (0.0, None, None);
    };

    let denominator = if initial_cash.is_finite() && initial_cash > 0.0 {
        initial_cash
    } else {
        first.equity
    };
    let total_return = last.equity / denominator - 1.0;
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
        ("食品饮料", "食品饮料"),
        ("电池", "动力电池"),
        ("动力电池", "动力电池"),
        ("新能源", "动力电池"),
        ("汽车", "汽车"),
        ("电子", "电子制造"),
        ("半导体", "电子制造"),
        ("芯片", "电子制造"),
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
    let Ok(regex) = Regex::new(r"\b(\d{6})(?:\.(SH|SZ|BJ))?\b") else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    regex
        .captures_iter(&message.to_uppercase())
        .filter_map(|captures| {
            let code = captures.get(1)?.as_str();
            let market = captures.get(2).map(|m| m.as_str()).unwrap_or_else(|| {
                if code.starts_with('6') {
                    "SH"
                } else if code.starts_with('8') || code.starts_with('4') {
                    "BJ"
                } else {
                    "SZ"
                }
            });
            let normalized = format!("{code}.{market}");
            if seen.insert(normalized.clone()) {
                Some(normalized)
            } else {
                None
            }
        })
        .collect()
}

fn extract_date(message: &str, default: &str, first: bool) -> String {
    let Ok(date_regex) = Regex::new(r"\b(20\d{2})(?:[-/.年])?(\d{1,2})(?:[-/.月])?(\d{1,2})日?\b")
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

fn score_contribution(
    key: &str,
    label: &str,
    value: f64,
    contribution: f64,
    strong_threshold: f64,
    positive_threshold: f64,
) -> ScoreContribution {
    let tone = if value >= strong_threshold {
        "strong"
    } else if value >= positive_threshold {
        "positive"
    } else if value <= 0.0 {
        "weak"
    } else {
        "neutral"
    };
    ScoreContribution {
        key: key.to_string(),
        label: label.to_string(),
        value: Some(round6(value)),
        contribution: Some(round6(contribution)),
        tone: tone.to_string(),
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

mod ffi;

#[cfg(test)]
mod tests;
