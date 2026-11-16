use crate::backtest::request::default_backtest_strategy_mode;
use crate::volatility::VolatilitySnapshot;
use serde::{Deserialize, Serialize};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal_date: Option<String>,
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
