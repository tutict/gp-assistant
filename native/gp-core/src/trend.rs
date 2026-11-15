pub(crate) mod indicators;
pub(crate) mod service;
pub(crate) mod types;

pub use service::{trend_with_data, trend_with_mock, trend_with_source};
pub use types::{
    TrendIndicatorPoint, TrendIndicatorRequest, TrendIndicatorResult, TrendIndicatorSignal,
};
