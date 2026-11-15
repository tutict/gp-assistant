use crate::error::{CoreError, CoreResult};
use crate::scoring_parameters::{
    fixed_scoring_parameters, validate_scoring_parameters, ScoringParameters,
};
use crate::screening::types::ScreenCriteria;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BacktestRequest {
    #[serde(default)]
    pub algorithm_version: Option<String>,
    #[serde(default)]
    pub parameter_set_id: Option<String>,
    #[serde(default)]
    pub scoring_parameters: Option<ScoringParameters>,
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

impl Default for BacktestRequest {
    fn default() -> Self {
        Self {
            criteria: ScreenCriteria::default(),
            source: default_backtest_source(),
            strategy_mode: default_backtest_strategy_mode(),
            algorithm_version: None,
            parameter_set_id: None,
            scoring_parameters: None,
            stock_codes: Vec::new(),
            start_date: String::new(),
            end_date: String::new(),
            top_n: default_top_n(),
            initial_cash: default_initial_cash(),
            rebalance_frequency: default_rebalance_frequency(),
            transaction_cost_bps: default_transaction_cost_bps(),
            benchmark: default_backtest_benchmark(),
        }
    }
}

pub(crate) fn default_top_n() -> usize {
    10
}

pub(crate) fn default_initial_cash() -> f64 {
    1_000_000.0
}

pub(crate) fn default_rebalance_frequency() -> String {
    "monthly".to_string()
}

pub(crate) fn default_transaction_cost_bps() -> f64 {
    10.0
}

pub(crate) fn default_backtest_benchmark() -> String {
    "candidate_equal_weight".to_string()
}

pub(crate) fn default_backtest_strategy_mode() -> String {
    "walk_forward".to_string()
}

pub(crate) fn default_backtest_source() -> String {
    "criteria".to_string()
}

pub(super) fn sanitize_initial_cash(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        default_initial_cash()
    }
}

pub(super) fn sanitize_transaction_cost_rate(bps: f64) -> f64 {
    if bps.is_finite() {
        bps.clamp(0.0, 500.0) / 10_000.0
    } else {
        default_transaction_cost_bps() / 10_000.0
    }
}

pub(super) fn normalize_rebalance_frequency(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" => "none".to_string(),
        "quarterly" => "quarterly".to_string(),
        _ => "monthly".to_string(),
    }
}

pub(super) fn normalize_backtest_benchmark(value: &str) -> String {
    if value.trim().eq_ignore_ascii_case("none") {
        "none".to_string()
    } else {
        "candidate_equal_weight".to_string()
    }
}

pub(crate) fn normalize_backtest_strategy_mode(value: &str) -> String {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.split(':').next() == Some("adaptive_swing_v2") {
        return "adaptive_swing_v2".to_string();
    }
    if normalized.split(':').next() == Some("adaptive_swing_v1") {
        return "adaptive_swing_v1".to_string();
    }
    match normalized.as_str() {
        "candidate_snapshot" | "snapshot" => "candidate_snapshot".to_string(),
        _ => "walk_forward".to_string(),
    }
}

pub(super) fn backtest_strategy_mode(request: &BacktestRequest) -> String {
    request
        .algorithm_version
        .clone()
        .unwrap_or_else(|| normalize_backtest_strategy_mode(&request.strategy_mode))
}

pub(super) fn is_adaptive_backtest(request: &BacktestRequest) -> bool {
    matches!(
        backtest_strategy_mode(request).as_str(),
        "adaptive_swing_v1" | "adaptive_swing_v2"
    )
}

pub(super) fn backtest_scoring_parameters(
    request: &BacktestRequest,
) -> CoreResult<Option<ScoringParameters>> {
    let mode = backtest_strategy_mode(request);
    let declared = normalize_backtest_strategy_mode(&request.strategy_mode);
    if request.algorithm_version.is_some()
        && !matches!(mode.as_str(), "adaptive_swing_v1" | "adaptive_swing_v2")
    {
        return Err(CoreError::new("unsupported backtest algorithm_version"));
    }
    if request.algorithm_version.is_some()
        && (declared == "candidate_snapshot"
            || (declared.starts_with("adaptive_swing_") && declared != mode))
    {
        return Err(CoreError::new(
            "algorithm_version conflicts with explicit strategy_mode",
        ));
    }
    if !is_adaptive_backtest(request) {
        if request.scoring_parameters.is_some() || request.parameter_set_id.is_some() {
            return Err(CoreError::new(
                "scoring parameters require an adaptive backtest algorithm",
            ));
        }
        return Ok(None);
    }
    let parameters = match &request.scoring_parameters {
        Some(parameters) => parameters.clone(),
        None if mode == "adaptive_swing_v1" => fixed_scoring_parameters(),
        None => {
            return Err(CoreError::new(
                "adaptive_swing_v2 requires scoring parameters",
            ))
        }
    };
    validate_scoring_parameters(&parameters)?;
    if parameters.algorithm_version != mode {
        return Err(CoreError::new(
            "scoring parameters algorithm_version does not match backtest",
        ));
    }
    if request
        .parameter_set_id
        .as_ref()
        .is_some_and(|id| *id != parameters.parameter_set_id)
    {
        return Err(CoreError::new("backtest parameter_set_id mismatch"));
    }
    Ok(Some(parameters))
}

pub(crate) fn adaptive_backtest_requested_mode(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .split_once(':')
        .map(|(_, mode)| mode)
        .filter(|mode| matches!(*mode, "auto" | "range" | "trend" | "defensive"))
        .unwrap_or("auto")
        .to_string()
}

pub(super) fn backtest_strategy_mode_note(value: &str) -> &'static str {
    match value {
        "adaptive_swing_v2" => "策略模式：adaptive_swing_v2；使用经校验的冻结参数及前一交易日可见数据生成信号，在下一交易日按实际报价调仓。",
        "adaptive_swing_v1" => "策略模式：adaptive_swing_v1；使用前一交易日收盘前可见的财务快照、OHLCV、三宽基指数和市场宽度生成信号，并在下一交易日按实际报价调仓。",
        "walk_forward" => "策略模式：walk_forward，仅使用成交日前一交易日已公开的因子快照和历史上市/ST/可交易状态；成交只使用执行日实际报价。",
        _ => "策略模式：candidate_snapshot，展示当前候选池的历史组合表现，不等同于严格可交易的逐日选股策略。",
    }
}

pub(super) fn backtest_rebalance_note(value: &str) -> &'static str {
    match value {
        "none" => "买入持有",
        "quarterly" => "季度调仓",
        _ => "月度调仓",
    }
}

pub(super) fn normalize_backtest_source(source: &str) -> String {
    if source.trim().eq_ignore_ascii_case("watchlist") {
        "watchlist".to_string()
    } else {
        "criteria".to_string()
    }
}
