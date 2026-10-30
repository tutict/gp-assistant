use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::fmt;

pub type ResearchResult<T> = Result<T, ResearchError>;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResearchError {
    pub code: String,
    pub message: String,
    pub details: Vec<String>,
}
impl ResearchError {
    pub(crate) fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: vec![],
        }
    }
}
impl fmt::Display for ResearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for ResearchError {}
impl From<rusqlite::Error> for ResearchError {
    fn from(e: rusqlite::Error) -> Self {
        Self::new("sqlite", e.to_string())
    }
}
impl From<std::io::Error> for ResearchError {
    fn from(e: std::io::Error) -> Self {
        Self::new("io", e.to_string())
    }
}
impl From<serde_json::Error> for ResearchError {
    fn from(e: serde_json::Error) -> Self {
        Self::new("json", e.to_string())
    }
}
impl From<csv::Error> for ResearchError {
    fn from(e: csv::Error) -> Self {
        Self::new("csv", e.to_string())
    }
}
impl From<gp_core::CoreError> for ResearchError {
    fn from(e: gp_core::CoreError) -> Self {
        Self::new("core", e.to_string())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DatasetRecord {
    pub dataset_id: String,
    pub name: String,
    pub data_fingerprint: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub real_data: bool,
    pub full_universe: bool,
    pub security_count: usize,
    pub bar_count: usize,
    pub source: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct EvaluationConfig {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub initial_cash: f64,
    pub transaction_cost_bps: f64,
    pub top_n: usize,
    pub mode: String,
    pub rebalance_frequency: String,
    pub strategy: String,
    pub criteria: gp_core::ScreenCriteria,
    pub signal_cutoff: Option<NaiveDate>,
}
impl Default for EvaluationConfig {
    fn default() -> Self {
        Self {
            start_date: NaiveDate::from_ymd_opt(2021, 10, 1).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2026, 9, 30).unwrap(),
            initial_cash: 1e6,
            transaction_cost_bps: 10.0,
            top_n: 10,
            mode: "auto".into(),
            rebalance_frequency: "monthly".into(),
            strategy: "adaptive".into(),
            criteria: gp_core::ScreenCriteria::default(),
            signal_cutoff: None,
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct QualityReport {
    pub passed: bool,
    pub real_data: bool,
    pub full_universe: bool,
    pub checked_sessions: usize,
    pub slow_factor_coverage: f64,
    pub issues: Vec<String>,
    pub missing_details: Vec<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PortfolioMetrics {
    pub total_return: f64,
    pub annualized_return: Option<f64>,
    pub max_drawdown: Option<f64>,
    pub annualized_turnover: f64,
    pub total_transaction_cost: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MonthlyFold {
    pub signal_date: NaiveDate,
    pub selection_date: NaiveDate,
    pub evaluation_end_date: NaiveDate,
    pub selected_symbols: Vec<String>,
    pub eligible_symbol_count: usize,
    pub net_return: f64,
    pub benchmark_return: f64,
    pub precision_at_n: Option<f64>,
    pub complete: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EquityPoint {
    pub date: NaiveDate,
    pub equity: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvaluationReport {
    pub run_id: String,
    pub dataset_id: String,
    pub data_fingerprint: String,
    pub implementation_fingerprint: String,
    pub research_fingerprint: String,
    pub parameter_fingerprint: String,
    pub parameters: gp_core::ScoringParameters,
    pub config: EvaluationConfig,
    pub data_quality: QualityReport,
    pub metrics: PortfolioMetrics,
    pub benchmark_metrics: PortfolioMetrics,
    pub monthly_folds: Vec<MonthlyFold>,
    pub equity_curve: Vec<EquityPoint>,
    pub benchmark_curve: Vec<EquityPoint>,
    pub notes: Vec<String>,
    pub trades: Vec<TradeRecord>,
    pub exposures: Vec<ExposureRecord>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: String,
    pub dataset_id: String,
    pub kind: String,
    pub status: String,
    pub parameter_set_id: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TradeRecord {
    pub date: NaiveDate,
    pub code: String,
    pub side: String,
    pub shares: f64,
    pub price: Option<f64>,
    pub notional: f64,
    pub cost: f64,
    pub executed: bool,
    pub reason: Option<String>,
    pub benchmark: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExposureRecord {
    pub date: NaiveDate,
    pub code: String,
    pub shares: f64,
    pub last_price: f64,
    pub locked: bool,
    pub benchmark: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceCoverage {
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub corporate_actions_complete: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BundleFile {
    pub role: String,
    pub path: String,
    pub sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BundleManifest {
    pub format: String,
    pub name: String,
    pub market: String,
    pub timezone: String,
    pub source: String,
    #[serde(default)]
    pub real_data: bool,
    pub full_universe: bool,
    pub coverage: SourceCoverage,
    pub rate_units: std::collections::BTreeMap<String, String>,
    pub money_unit: String,
    pub files: Vec<BundleFile>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Security {
    pub code: String,
    pub name: String,
    pub security_type: String,
    pub listed_date: NaiveDate,
    pub delisted_date: Option<NaiveDate>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DailyBar {
    pub code: String,
    pub date: NaiveDate,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: Option<f64>,
    pub amount: Option<f64>,
    pub total_shares: Option<f64>,
    pub float_shares: Option<f64>,
    pub turnover_rate: Option<f64>,
    pub volume_ratio: Option<f64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FinancialEvent {
    pub code: String,
    pub report_date: NaiveDate,
    pub available_at: String,
    pub revision: u32,
    pub source: String,
    pub eps_ttm: Option<f64>,
    pub bps: Option<f64>,
    pub roe: Option<f64>,
    pub deducted_net_profit_billion: Option<f64>,
    pub deducted_net_profit_growth_rate: Option<f64>,
    pub deducted_net_profit_margin: Option<f64>,
    pub dividend_yield: Option<f64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DailyStatus {
    pub code: String,
    pub date: NaiveDate,
    pub name: String,
    pub industry: String,
    pub is_listed: bool,
    pub is_st: bool,
    pub can_buy: bool,
    pub can_sell: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CorporateAction {
    pub code: String,
    pub event_id: String,
    pub kind: String,
    pub known_at: String,
    pub ex_date: NaiveDate,
    pub pay_date: NaiveDate,
    pub cash_per_share: Option<f64>,
    pub share_multiplier: Option<f64>,
}
