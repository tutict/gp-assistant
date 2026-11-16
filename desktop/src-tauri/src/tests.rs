use super::*;
use super::{hk_market::*, llm::*, observe::*, research::*, screening::*};
use crate::market::cache::cached_mobile_market_data_entry;
use crate::market::cache::forget_mobile_market_data_cache;
use crate::market::cache::records::market_cache_record;
use crate::market::cache::remember_mobile_market_data_cache;
use crate::market::providers::eastmoney::eastmoney_metric_period;
use crate::market::providers::eastmoney::eastmoney_result_rows;
use crate::market::providers::eastmoney::fetch_eastmoney_public_json_with_direct_retry;
use crate::market::providers::eastmoney::normalize_eastmoney_public_json;
use crate::market::providers::eps::parse_sina_quarterly_eps_html;
use crate::market::providers::eps::parse_ths_quarterly_eps_json;
use crate::market::providers::history::choose_longer_history;
use crate::market::providers::history::fetch_observe_daily_history;
use crate::market::providers::tencent::candidate_batch_window;
use crate::market::providers::tencent::parse_tencent_quotes;
use crate::market::providers::tencent::TENCENT_BATCH_SIZE;
use crate::market::refresh::market_refresh_history_codes;
use crate::market::store::files::market_data_patch_codes;
use crate::market::store::files::retry_with_attempts;
use crate::market::store::files::MOBILE_MARKET_DATA_FILE;
use crate::market::store::projection::external_market_domains;
use crate::market::store::projection::project_market_store;
use crate::market::store::projection::project_market_store_with_legacy;
use crate::market::store::projection::store_market_dataset;
use crate::market_domain::codes::adaptive_benchmark_codes;
use crate::market_domain::codes::append_tencent_candidate_codes;
use crate::market_domain::codes::build_candidate_codes;
use crate::market_domain::codes::normalize_stock_code;
use crate::market_domain::codes::tencent_symbol;
use crate::market_domain::datasets::append_all_preserved_seed_stocks;
use crate::market_domain::datasets::append_preserved_seed_stocks;
use crate::market_domain::datasets::apply_market_data_patch;
use crate::market_domain::datasets::filter_seed_histories;
use crate::market_domain::datasets::market_data_patch_for_codes;
use crate::market_domain::datasets::market_data_payload_present;
use crate::market_domain::datasets::parse_mobile_market_data_snapshot;
use crate::market_domain::datasets::seed_stock_maps;
use crate::market_domain::datasets::strip_core_side_payload_fields;
use crate::market_domain::dates::cache_epoch_ms;
use crate::market_domain::dates::epoch_millis;
use crate::market_domain::dates::expected_market_quote_date_from_epoch_ms;
use crate::market_domain::dates::expected_market_quote_date_with_calendar;
use crate::market_domain::dates::local_yyyymmdd_from_epoch_ms;
use crate::market_domain::dates::market_quote_cache_stale;
use crate::market_domain::dates::parse_cache_datetime_epoch_ms;
use crate::market_domain::financial::enriched_stock_maps;
use crate::market_domain::financial::filtered_financial_snapshot_map;
use crate::market_domain::financial::financial_quarterly_eps_count;
use crate::market_domain::financial::finite_object_number;
use crate::market_domain::financial::main_fund_involvement;
use crate::market_domain::financial::merge_basic_financial_from_stock;
use crate::market_domain::history::filter_daily_history_rows_by_date;
use crate::market_domain::history::history_cache_has_bars;
use crate::market_domain::history::tencent_daily_history_param;
use crate::market_domain::history::OBSERVE_DAILY_HISTORY_LIMIT;
use crate::wire_values::institution_buy_sell_ratio;
use crate::wire_values::object_f64;
use crate::wire_values::payload_usize_field;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use stock_optimizer_core as gp_core;

fn hk_test_cache() -> HkMarketCache {
    HkMarketCache {
        schema_version: 1,
        securities: vec![],
        securities_source: "HKEX".to_string(),
        securities_updated_at: None,
        securities_last_attempt_at: None,
        securities_last_error: None,
        quotes: HashMap::new(),
        histories: HashMap::new(),
        quote_errors: HashMap::new(),
        history_errors: HashMap::new(),
        financials: HashMap::new(),
        fx_rate: None,
        fx_error: None,
        disclosures: HashMap::new(),
        quarterly_updates: HashMap::new(),
        financial_errors: HashMap::new(),
        disclosure_errors: HashMap::new(),
    }
}

// Include each group in this module to retain every frozen tests:: identity.
include!("tests/adaptive.rs");
include!("tests/agent.rs");
include!("tests/hk_market.rs");
include!("tests/llm.rs");
include!("tests/market.rs");
include!("tests/misc.rs");
