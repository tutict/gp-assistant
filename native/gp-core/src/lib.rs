mod backtest;

mod capital_evidence;

mod data_source;

mod dates;

mod demo_data;

mod error;

mod market_data;

mod numeric;

pub use backtest::{
    backtest_required_history_symbols, backtest_selected_symbols, backtest_with_data,
    backtest_with_mock, backtest_with_source, validate_backtest_date_range, BacktestMetrics,
    BacktestRequest, BacktestResult, EquityPoint, WalkForwardFold,
};

mod release_gate;

pub use release_gate::{
    evaluate_adaptive_release_gate, AdaptiveReleaseGateCheck, AdaptiveReleaseGateInput,
    AdaptiveReleaseGateReport,
};

mod trend;

pub use trend::{
    trend_with_data, trend_with_mock, trend_with_source, TrendIndicatorPoint,
    TrendIndicatorRequest, TrendIndicatorResult, TrendIndicatorSignal,
};

mod trend_screen;

pub use trend_screen::{
    trend_screen_candidate_codes, trend_screen_with_data, trend_screen_with_mock,
    trend_screen_with_source, TrendScreenRequest, TrendScreenResult, TrendStockSignal,
};

mod observation;

pub use observation::{
    observe_with_data, observe_with_mock, observe_with_source, FinancialIndicatorItem,
    FinancialIndicators, StockObservation, StockObserveRequest,
};

mod graph_compat;

mod screening;

mod text;

pub use graph_compat::{
    graph_screen_stocks, graph_screen_with_data, graph_screen_with_mock, graph_screen_with_source,
    GraphCenterContext, GraphScreenRequest, GraphScreenResult, GraphStockSignal,
};

pub use screening::{
    screen_stocks, screen_with_data, screen_with_mock, screen_with_source, sector_screen_stocks,
    sector_screen_with_data, sector_screen_with_mock, ScoreContribution, ScreenCriteria,
    ScreenResult, ScreenResultGroup, ScreenedStock, SectorScreenGroup, SectorScreenRequest,
    SectorScreenResult, SelectionExplanation,
};

pub use capital_evidence::{CapitalEvidenceItem, CapitalEvidenceResult, CapitalEvidenceSection};

pub use data_source::{
    validate_data_set, CoreDataSet, DataSourceSummary, MarketDataSource, StaticDataSource,
};

pub use demo_data::{mock_relations, mock_stocks, MockDataSource};

pub use error::{CoreError, CoreResult};

pub use market_data::{
    FinancialCoverage, HistoryBar, QuarterlyEpsPoint, StockFactorSnapshot, StockFinancialSnapshot,
    StockItem, StockRelation,
};

mod adaptive_screen;

mod rates;

mod scoring_parameters;

mod source_fingerprint;

pub use scoring_parameters::{
    adaptive_score_factor_vector, fixed_scoring_parameters, scoring_implementation_fingerprint,
    validate_scoring_parameters, ModeParameters, ScoringParameters, METRIC_CONTRACT_VERSION,
    SCORING_REVISION,
};

pub use source_fingerprint::source_text_fingerprint;

mod industry_rank;

#[cfg(test)]
mod scoring_contract_tests;

pub use rates::{RateRatio, RateUnit};

mod volatility;

pub use adaptive_screen::{
    adaptive_candidate_codes, adaptive_scoring_features, adaptive_screen_stocks,
    adaptive_screen_stocks_with_parameters, AdaptiveFeatureRow, AdaptiveFeatureSet,
    AdaptiveMarketRegime, AdaptiveRecentExposure, AdaptiveScreenRequest, AdaptiveScreenResult,
    MIN_MARKET_BREADTH_COVERAGE,
};

pub use volatility::{
    AtrSnapshot, BollingerBandsSnapshot, ChaikinVolatilitySnapshot, DonchianChannelSnapshot,
    IndicatorUnavailable, KeltnerChannelSnapshot, RviSnapshot, VolatilitySnapshot,
};

mod ffi;

#[cfg(test)]
mod tests;

mod agent;

pub use agent::{
    agent_skill_registry, run_agent_stream_with_data_events,
    run_agent_stream_with_data_events_with_context, run_agent_stream_with_source_events,
    run_agent_stream_with_source_events_with_context, run_agent_with_data,
    run_agent_with_data_and_context, run_agent_with_mock, run_agent_with_source,
    run_agent_with_source_and_context, AgentAnswerSection, AgentContext, AgentEvidenceItem,
    AgentIntent, AgentRequest, AgentResponse, AgentSkill, AgentStreamEvent, AgentToolCall,
    AgentWatchlistItem,
};

mod mobile_skill;

pub use mobile_skill::{
    run_mobile_stock_skill, MobileStockSkillFinding, MobileStockSkillOverview,
    MobileStockSkillRequest, MobileStockSkillResult, MobileStockSourceItem,
};

mod json_api;

pub use json_api::{
    agent_stream_with_data_events_value, agent_value, agent_with_data_value, backtest_value,
    backtest_with_data_value, graph_screen_value, graph_screen_with_data_value,
    mobile_stock_skill_value, observe_value, observe_with_data_value, screen_value,
    screen_with_data_value, sector_screen_value, sector_screen_with_data_value, trend_screen_value,
    trend_screen_with_data_value, trend_value, trend_with_data_value, validate_data_source_value,
    AgentStreamWithDataRequest, AgentWithDataRequest, BacktestWithDataRequest,
    GraphScreenWithDataRequest, ObserveWithDataRequest, ScreenWithDataRequest,
    SectorScreenWithDataRequest, TrendScreenWithDataRequest, TrendWithDataRequest,
};

#[cfg(test)]
use std::collections::HashSet;

#[cfg(test)]
use std::collections::{BTreeMap, HashMap};

#[cfg(test)]
use chrono::Days;

#[cfg(test)]
use chrono::NaiveDate;

#[cfg(test)]
use serde_json::{json, Value};

#[cfg(test)]
use crate::backtest::request::{
    adaptive_backtest_requested_mode, normalize_backtest_strategy_mode,
};

#[cfg(test)]
use crate::backtest::request::{
    default_backtest_benchmark, default_backtest_source, default_rebalance_frequency,
    default_transaction_cost_bps,
};

#[cfg(test)]
use crate::dates::default_end_date;

#[cfg(test)]
use crate::observation::financial::format_percentage_points;

#[cfg(test)]
use crate::observation::financial::format_percent;

#[cfg(test)]
use crate::screening::classification::{concept_group_for_stock, theme_match_for_stock};

#[cfg(test)]
use crate::screening::criteria::matches_stock;

#[cfg(test)]
use crate::screening::scoring::score_stock;

#[cfg(test)]
use crate::screening::scoring::{
    data_quality_score, fundamental_score, pb_score, pe_score, profile_score, SCREEN_SCORE_SCALE,
};

#[cfg(test)]
use crate::screening::types::default_sector_group_by;

#[cfg(test)]
use industry_rank::industry_rank_values;

#[cfg(test)]
use rates::ratio_percentage_points;

#[cfg(test)]
use crate::agent::{format_signed_percent_text, heuristic_criteria};
