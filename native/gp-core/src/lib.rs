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
    pub industry: Option<String>,
    #[serde(default)]
    pub include_st: bool,
    #[serde(default = "default_screen_limit")]
    pub limit: usize,
    #[serde(default = "default_sort_by")]
    pub sort_by: String,
    #[serde(default = "default_sort_dir")]
    pub sort_dir: String,
}

impl Default for ScreenCriteria {
    fn default() -> Self {
        Self {
            min_roe: None,
            max_pe: None,
            max_pb: None,
            min_market_cap_billion: None,
            industry: None,
            include_st: false,
            limit: default_screen_limit(),
            sort_by: default_sort_by(),
            sort_dir: default_sort_dir(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScreenedStock {
    pub stock: StockItem,
    pub score: f64,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScreenResult {
    pub total: usize,
    pub returned: usize,
    pub items: Vec<ScreenedStock>,
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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphScreenResult {
    pub total: usize,
    pub returned: usize,
    pub relation_count: usize,
    pub items: Vec<GraphStockSignal>,
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
    #[serde(default)]
    pub swl: Option<f64>,
    #[serde(default)]
    pub sws: Option<f64>,
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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrendScreenResult {
    pub total: usize,
    pub returned: usize,
    pub items: Vec<TrendStockSignal>,
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
pub struct CoreDataSet {
    #[serde(default)]
    pub stocks: Vec<StockItem>,
    #[serde(default)]
    pub relations: Vec<StockRelation>,
    #[serde(default)]
    pub histories: HashMap<String, Vec<HistoryBar>>,
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
    swl: f64,
    sws: f64,
    star_line: f64,
    bull_line: f64,
    wait_line: f64,
    support: f64,
    resistance: f64,
    breakout: f64,
    reversal: f64,
    swl_above_sws: bool,
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

fn default_relation_weight() -> f64 {
    1.0
}

fn default_relation_depth() -> usize {
    1
}

fn default_relation_weight_param() -> f64 {
    0.35
}

fn default_graph_limit() -> usize {
    10
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

pub fn screen_with_data_value(payload: Value) -> CoreResult<Value> {
    let request: ScreenWithDataRequest = serde_json::from_value(payload)?;
    serde_json::to_value(screen_with_data(&request.data, &request.criteria)?).map_err(Into::into)
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

pub fn trend_with_data_value(payload: Value) -> CoreResult<Value> {
    let request: TrendWithDataRequest = serde_json::from_value(payload)?;
    serde_json::to_value(trend_with_data(&request.data, &request.request)?).map_err(Into::into)
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
            selected.push(ScreenedStock {
                stock: stock.clone(),
                score: 0.0,
                reasons: vec!["watchlist".to_string()],
            });
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
        if digits.chars().all(|ch| ch.is_ascii_digit()) && matches!(suffix, "SH" | "SZ") {
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
            signal: analysis.signal,
            reasons,
        });
    }

    items.sort_by(|left, right| right.final_score.total_cmp(&left.final_score));
    items.truncate(request.limit.clamp(1, 100));
    let mut notes = trend_notes();
    if skipped > 0 {
        notes.push(format!(
            "Skipped {skipped} stocks without usable OHLC history."
        ));
    }
    Ok(TrendScreenResult {
        total: candidate_pool.len(),
        returned: items.len(),
        items,
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

    for stock in universe {
        let Some(reasons) = matches_stock(stock, criteria) else {
            continue;
        };
        let score = score_stock(stock, &reasons);
        screened.push(ScreenedStock {
            stock: stock.clone(),
            score,
            reasons,
        });
    }

    let reverse = criteria.sort_dir.to_lowercase() != "asc";
    screened.sort_by(|left, right| {
        let left_value = sort_value(left, &criteria.sort_by);
        let right_value = sort_value(right, &criteria.sort_by);
        match (left_value, right_value) {
            (Some(left_value), Some(right_value)) => {
                let ordering = left_value.total_cmp(&right_value);
                if reverse {
                    ordering.reverse()
                } else {
                    ordering
                }
            }
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });

    let limit = criteria.limit.clamp(1, 200);
    let items = promote_hot_sector_items(&screened, criteria, limit);
    ScreenResult {
        total: screened.len(),
        returned: items.len(),
        items,
    }
}

pub fn graph_screen_stocks(
    universe: &[StockItem],
    provider_relations: &[StockRelation],
    request: &GraphScreenRequest,
) -> GraphScreenResult {
    let candidate_pool = screen_candidate_pool(universe, &request.criteria, request.limit);
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
    let seed_codes: HashSet<String> = request.seed_codes.iter().cloned().collect();
    let max_depth = request.relation_depth.clamp(1, 3);
    let relation_weight = request.relation_weight.clamp(0.0, 1.0);

    let mut signals = Vec::new();
    for (code, screened) in candidate_by_code {
        let relation_score =
            relation_score(&code, &base_scores, &adjacency, &seed_codes, max_depth);
        let base_score = *base_scores.get(&code).unwrap_or(&0.0);
        let final_score = (1.0 - relation_weight) * base_score + relation_weight * relation_score;
        signals.push(GraphStockSignal {
            stock: screened.stock.clone(),
            base_score: round6(base_score),
            relation_score: round6(relation_score),
            final_score: round6(final_score),
            suggested_weight: 0.0,
            reasons: graph_reasons(&screened, relation_score),
            related: top_related(&code, &relations, &all_stocks, 5),
        });
    }

    signals.sort_by(|left, right| right.final_score.total_cmp(&left.final_score));
    signals.truncate(request.limit.clamp(1, 100));
    assign_weights(&mut signals);

    let mut notes = vec![
        "关系图是轻量传播评分层，不是 LangGraph 工作流编排。".to_string(),
        "LangGraph 用于智能体状态流；股票关系由图学习或知识图谱数据建模。".to_string(),
    ];
    if relations.is_empty() {
        notes.push("当前没有可用股票关系，结果已回退为基础选股分。".to_string());
    }

    GraphScreenResult {
        total: candidate_pool.len(),
        returned: signals.len(),
        relation_count: relations.len(),
        items: signals,
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

    Some(reasons)
}

fn score_stock(stock: &StockItem, reasons: &[String]) -> f64 {
    let mut score = reasons.len() as f64;
    if let Some(pe) = stock.pe.filter(|value| *value != 0.0) {
        score += 0.0_f64.max(10.0 / pe);
    }
    if let Some(pb) = stock.pb.filter(|value| *value != 0.0) {
        score += 0.0_f64.max(2.0 / pb);
    }
    if let Some(roe) = stock.roe.filter(|value| *value != 0.0) {
        score += roe * 2.0;
    }
    if let Some(dividend_yield) = stock.dividend_yield.filter(|value| *value != 0.0) {
        score += (dividend_yield * 10.0).min(1.0);
    }
    score += hot_sector_bonus(&stock.industry);
    score
}

fn hot_sector_bonus(industry: &str) -> f64 {
    let normalized = industry.trim().to_lowercase();
    if normalized.is_empty() {
        return 0.0;
    }
    let keywords = [
        ("半导体", 0.55),
        ("芯片", 0.55),
        ("算力", 0.5),
        ("人工智能", 0.5),
        ("ai", 0.5),
        ("机器人", 0.46),
        ("软件", 0.44),
        ("通信", 0.42),
        ("科技", 0.42),
        ("电子", 0.38),
        ("新能源", 0.46),
        ("电池", 0.44),
        ("储能", 0.42),
        ("光伏", 0.4),
        ("电力", 0.34),
        ("能源", 0.34),
        ("油气", 0.28),
        ("煤炭", 0.24),
    ];
    keywords
        .iter()
        .filter_map(|(keyword, weight)| normalized.contains(keyword).then_some(*weight))
        .fold(0.0, f64::max)
}

fn hot_sector_category(industry: &str) -> Option<&'static str> {
    let normalized = industry.trim().to_lowercase();
    if normalized.is_empty() {
        return None;
    }
    let categories: [(&str, &[&str]); 2] = [
        (
            "tech",
            &[
                "半导体",
                "芯片",
                "算力",
                "人工智能",
                "ai",
                "机器人",
                "软件",
                "通信",
                "科技",
                "电子",
            ],
        ),
        (
            "energy",
            &[
                "新能源",
                "电池",
                "储能",
                "光伏",
                "电力",
                "能源",
                "油气",
                "煤炭",
            ],
        ),
    ];
    categories.iter().find_map(|(category, keywords)| {
        keywords
            .iter()
            .any(|keyword| normalized.contains(keyword))
            .then_some(*category)
    })
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
    for category in ["tech", "energy"] {
        if let Some(candidate) = screened.iter().find(|item| {
            !used_codes.contains(&item.stock.code)
                && hot_sector_category(&item.stock.industry) == Some(category)
        }) {
            promoted.push(candidate.clone());
            used_codes.insert(candidate.stock.code.clone());
            if promoted.len() >= limit {
                return promoted;
            }
        }
    }

    if promoted.is_empty() {
        return screened.iter().take(limit).cloned().collect();
    }

    for item in screened {
        if used_codes.contains(&item.stock.code) {
            continue;
        }
        promoted.push(item.clone());
        if promoted.len() >= limit {
            break;
        }
    }
    promoted
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
    let quant_score = quant_scores(&close, &high, &low, &volume);

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
        computed.push(ComputedTrendBar {
            date: bars[index].date,
            close: close[index],
            swl: swl[index],
            sws: sws[index],
            star_line: star_line[index],
            bull_line: bull_line[index],
            wait_line,
            support: 2.0 * e_value - high[index],
            resistance: 2.0 * e_value - low[index],
            breakout: e_value + (high[index] - low[index]),
            reversal: e_value - (high[index] - low[index]),
            swl_above_sws: swl[index] > sws[index],
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
        swl: finite_round4(bar.swl),
        sws: finite_round4(bar.sws),
        star_line: finite_round4(bar.star_line),
        bull_line: finite_round4(bar.bull_line),
        wait_line: finite_round4(bar.wait_line),
        support: finite_round4(bar.support),
        resistance: finite_round4(bar.resistance),
        breakout: finite_round4(bar.breakout),
        reversal: finite_round4(bar.reversal),
        swl_above_sws: bar.swl_above_sws,
        red_hold: bar.red_hold,
        cyan_watch: bar.cyan_watch,
        short_buy: bar.short_buy,
        white_exit: bar.white_exit,
        oversold: bar.oversold,
        quant_score: bar.quant_score,
        quant_score_max: default_quant_score_max(),
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
    reasons
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
    let mut score = (signal.quant_score as f64 / signal.quant_score_max.max(1) as f64) * 58.0;
    if signal.swl_above_sws {
        score += 10.0;
    }
    if signal.red_hold {
        score += 12.0;
    }
    if signal.short_buy {
        score += 16.0;
    }
    if signal
        .star_line
        .map(|star_line| signal.close > star_line)
        .unwrap_or(false)
    {
        score += 4.0;
    }
    if signal
        .bull_line
        .map(|bull_line| signal.close > bull_line)
        .unwrap_or(false)
    {
        score += 4.0;
    }
    if signal.white_exit {
        score -= 26.0;
    }
    if signal.cyan_watch {
        score -= 10.0;
    }
    if signal.oversold {
        score -= 8.0;
    }
    score.clamp(0.0, 100.0)
}

fn combined_trend_score(base_score: f64, trend_score: f64) -> f64 {
    (base_score.clamp(0.0, 10.0) * 4.0 + trend_score * 0.75).min(100.0)
}

fn trend_notes() -> Vec<String> {
    vec![
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
            },
        ];
        let relations = vec![StockRelation {
            source_code: "111111.SZ".to_string(),
            target_code: "222222.SZ".to_string(),
            relation_type: "custom_peer".to_string(),
            weight: 0.5,
            description: Some("native supplied relation".to_string()),
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
        assert_eq!(result.items[0].stock.code, "600000.SH");
        assert!(result.items[0].reasons.contains(&"pe_ok".to_string()));
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
