mod history;
mod metrics;
pub(crate) mod request;
mod selection;
mod service;
mod simulation;
mod types;

pub use request::BacktestRequest;
pub use selection::backtest_selected_symbols;
pub use service::{
    backtest_required_history_symbols, backtest_with_data, backtest_with_mock,
    backtest_with_source, validate_backtest_date_range,
};
pub use types::{BacktestMetrics, BacktestResult, EquityPoint, WalkForwardFold};

#[cfg(test)]
mod tests;
