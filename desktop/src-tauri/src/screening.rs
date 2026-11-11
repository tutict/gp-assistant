#[path = "screening_financial_overlay.rs"]
mod financial_overlay;
#[path = "cn_backtest_history.rs"]
mod cn_backtest_history;
#[path = "hk_research_history.rs"]
mod hk_history;
#[path = "local_data_policy.rs"]
pub(crate) mod local_data_policy;
use crate::screening::local_data_policy::{self as local_data, DataPolicy};
use futures::stream::{self, FuturesUnordered, StreamExt};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
use stock_optimizer_core as gp_core;
use tauri::{Emitter, Manager};

pub(crate) fn local_snapshot(
    app: &tauri::AppHandle,
    payload: &Value,
    operation: &str,
) -> Result<Arc<gp_core::CoreDataSet>, String> {
    DataPolicy::parse(payload)?;
    let data = crate::market::cached_market_data_snapshot(app).map_err(|error| {
        local_data::missing_error(
            operation,
            &[json!({"kind":"market_cache", "reason":error.chars().take(240).collect::<String>()})],
        )
    })?;
    if data.stocks.is_empty() {
        return Err(local_data::missing_error(
            operation,
            &[
                json!({"kind":"stock_universe", "reason":"No persisted market stocks. Refresh the market universe explicitly."}),
            ],
        ));
    }
    if !data
        .stocks
        .iter()
        .any(|stock| stock.price.is_finite() && stock.price > 0.0)
    {
        return Err(local_data::missing_error(
            operation,
            &[
                json!({"kind":"quotes", "reason":"No usable cached stock prices; refresh market quotes explicitly."}),
            ],
        ));
    }
    Ok(data)
}

pub(crate) fn local_history_coverage(
    data: &gp_core::CoreDataSet,
    overrides: &HashMap<String, Vec<gp_core::HistoryBar>>,
    codes: &[String],
    start: &str,
    end: &str,
    required: usize,
) -> Vec<Value> {
    codes
        .iter()
        .map(|code| {
            let normalized =
                crate::market::normalize_stock_code(code).unwrap_or_else(|| code.clone());
            let rows = overrides
                .get(code)
                .or_else(|| data.histories.get(code))
                .or_else(|| data.histories.get(&normalized));
            let mut coverage = local_data::history_coverage(
                rows.into_iter()
                    .flatten()
                    .filter(|bar| bar.close.is_finite() && bar.close > 0.0)
                    .map(|bar| bar.date.as_str()),
                start,
                end,
                required,
            );
            coverage["code"] = json!(code);
            coverage
        })
        .collect()
}

pub(crate) fn trend_screen_refresh_has_usable_history(coverage: &[Value]) -> bool {
    coverage
        .iter()
        .any(|item| item.get("sufficient").and_then(Value::as_bool) == Some(true))
}

pub(crate) fn local_trend_history_coverage(
    data: &gp_core::CoreDataSet,
    overrides: &HashMap<String, Vec<gp_core::HistoryBar>>,
    universe: &[gp_core::StockItem],
    codes: &[String],
    start: &str,
    end: &str,
    required: usize,
) -> Vec<Value> {
    let quotes = universe
        .iter()
        .map(|stock| (stock.code.as_str(), stock))
        .collect::<HashMap<_, _>>();
    let start = crate::market::compact_date_key(start).unwrap_or_default();
    let end = crate::market::compact_date_key(end).unwrap_or_default();
    let mut coverage = local_history_coverage(data, overrides, codes, &start, &end, required);
    for item in &mut coverage {
        let code = item["code"].as_str().unwrap_or_default();
        let quote_date = quotes
            .get(code)
            .and_then(|stock| stock.quote_time.as_deref())
            .and_then(crate::market::compact_date_key);
        // Historical windows remain readable; current windows must cover their quote date.
        let target = quote_date
            .as_ref()
            .filter(|date| **date >= start && **date <= end);
        let behind = target.is_some_and(|target| {
            item["as_of"]
                .as_str()
                .is_none_or(|latest| latest < target.as_str())
        });
        item["quote_as_of"] = json!(quote_date);
        item["history_behind_quote"] = json!(behind);
        if behind {
            item["sufficient"] = json!(false);
            item["reason"] = json!("日线日期落后于行情日期，需要补齐日线后重新计算趋势依据");
        }
    }
    coverage
}
pub(crate) fn adaptive_local_coverage(
    data: &gp_core::CoreDataSet,
    overrides: &HashMap<String, Vec<gp_core::HistoryBar>>,
    codes: &[String],
    as_of: Option<&str>,
) -> Vec<Value> {
    codes.iter().map(|code| {
        let normalized = crate::market::normalize_stock_code(code).unwrap_or_else(|| code.clone());
        let rows = overrides.get(code).or_else(|| data.histories.get(code)).or_else(|| data.histories.get(&normalized)).map(Vec::as_slice).unwrap_or(&[]);
        let window = adaptive_history_window(rows, as_of);
        let mut item = local_data::history_coverage(window.iter().filter(|bar| bar.close.is_finite() && bar.close > 0.0).map(|bar| bar.date.as_str()), "19900101", as_of.unwrap_or("20501231"), MIN_ADAPTIVE_SCREEN_HISTORY_BARS);
        item["code"] = json!(code);
        item["reason"] = json!("Requires 60 bars in the 120-bar PIT window, including the exact common as-of date.");
        item
    }).collect()
}

pub(crate) fn adaptive_screen_candidate_gaps(coverage: &[Value]) -> Result<Vec<Value>, String> {
    let benchmarks=crate::market::adaptive_benchmark_codes();
    let benchmark_coverage=benchmarks.iter().map(|code| {
        coverage.iter().find(|item|item.get("code").and_then(Value::as_str)==Some(*code)).cloned()
            .unwrap_or_else(||json!({"code":code,"kind":"history","sufficient":false,"available_bars":0,"required_bars":MIN_ADAPTIVE_SCREEN_HISTORY_BARS}))
    }).collect::<Vec<_>>();
    local_data::require_coverage("adaptive_screen", &benchmark_coverage)?;
    let is_current_short_history=|item:&Value| {
        let available=item.get("available_bars").and_then(Value::as_u64).unwrap_or(0);
        let required=item.get("required_bars").and_then(Value::as_u64).unwrap_or(MIN_ADAPTIVE_SCREEN_HISTORY_BARS as u64);
        available>0 && available<required
            && item.get("as_of").and_then(Value::as_str).zip(item.get("end_date").and_then(Value::as_str))
                .is_some_and(|(as_of,end)|as_of==end)
    };
    let blocking=coverage.iter().filter(|item|item.get("sufficient").and_then(Value::as_bool)!=Some(true)
        && !is_current_short_history(item)).cloned().collect::<Vec<_>>();
    local_data::require_coverage("adaptive_screen", &blocking)?;
    // The core omits candidates without a complete technical window and still
    // requires at least 60% of the initial pool. Keep those coverage checks there;
    // a newly listed stock must not make an otherwise usable market request fail.
    Ok(coverage.iter().filter(|item|is_current_short_history(item)
        && !item.get("code").and_then(Value::as_str).is_some_and(|code|benchmarks.contains(&code)))
        .cloned().collect())
}
pub(crate) fn local_metadata(
    data: &gp_core::CoreDataSet,
    policy: DataPolicy,
    coverage: &[Value],
) -> Value {
    let quote_as_of = data
        .stocks
        .iter()
        .filter_map(|stock| stock.quote_time.as_deref())
        .filter_map(crate::market::compact_date_key)
        .min();
    let mut metadata = local_data::metadata(policy, quote_as_of, coverage);
    metadata["stock_count"] = json!(data.stocks.len());
    metadata["factor_snapshot_symbol_count"] = json!(data.factor_snapshots.len());
    metadata
}

pub(crate) const ADAPTIVE_SCREEN_DB_FILE: &str = "adaptive-screen.sqlite";

pub(crate) const TREND_SCREEN_HISTORY_TIMEOUT_SECS: u64 = 45;

pub(crate) const TREND_SCREEN_HISTORY_FETCH_TIMEOUT_SECS: u64 = 30;

pub(crate) const TREND_SCREEN_HISTORY_CONCURRENCY: usize = 12;

pub(crate) const TREND_SCREEN_HISTORY_PREFETCH_LIMIT: usize = 200;

pub(crate) const MIN_TREND_SCREEN_HISTORY_BARS: usize = 45;

pub(crate) const ADAPTIVE_SCREEN_TOTAL_TIMEOUT_SECS: u64 = 120;

pub(crate) const ADAPTIVE_SCREEN_HISTORY_CONCURRENCY: usize = 24;

pub(crate) const ADAPTIVE_SCREEN_HISTORY_PREFETCH_TIMEOUT_SECS: u64 = 90;

pub(crate) const ADAPTIVE_SCREEN_HISTORY_PREFETCH_LIMIT: usize = 80;

pub(crate) const MIN_ADAPTIVE_SCREEN_HISTORY_BARS: usize = 60;

pub(crate) const ADAPTIVE_RELEASE_MIN_OOS_FOLDS: usize = 60;

pub(crate) const BACKTEST_HISTORY_TIMEOUT_SECS: u64 = 30;

pub(crate) const ADAPTIVE_RELEASE_BACKTEST_HISTORY_TIMEOUT_SECS: u64 = 180;

pub(crate) const BACKTEST_HISTORY_CONCURRENCY: usize = 6;

pub(crate) const MIN_BACKTEST_HISTORY_BARS: usize = 2;

pub(crate) struct PreparedTrendScreen {
    pub(crate) data: Arc<gp_core::CoreDataSet>,
    pub(crate) stock_override: Option<Arc<Vec<gp_core::StockItem>>>,
    pub(crate) history_override: HashMap<String, Vec<gp_core::HistoryBar>>,
    pub(crate) request: gp_core::TrendScreenRequest,
    pub(crate) notes: Vec<String>,
    pub(crate) metadata: Value,
}

pub(crate) struct PreparedAdaptiveScreen {
    pub(crate) data: Arc<gp_core::CoreDataSet>,
    pub(crate) stock_override: Option<Arc<Vec<gp_core::StockItem>>>,
    pub(crate) candidate_codes: Vec<String>,
    pub(crate) history_override: HashMap<String, Vec<gp_core::HistoryBar>>,
    pub(crate) request: gp_core::AdaptiveScreenRequest,
    pub(crate) recent_exposure: Vec<gp_core::AdaptiveRecentExposure>,
    pub(crate) notes: Vec<String>,
    pub(crate) cache_hit: bool,
    pub(crate) metadata: Value,
}

pub(crate) struct AdaptiveHistoryFetchOutcome {
    pub(crate) results: Vec<(String, Result<Vec<Value>, String>)>,
    pub(crate) timed_out: bool,
}

fn adaptive_perf_counts(
    data: &gp_core::CoreDataSet,
    stock_override: Option<&[gp_core::StockItem]>,
    history_override: Option<&HashMap<String, Vec<gp_core::HistoryBar>>>,
) -> crate::perf_diagnostics::DataCounts {
    crate::perf_diagnostics::DataCounts {
        stock_count: stock_override.map_or(data.stocks.len(), <[_]>::len),
        history_rows: data.histories.values().map(Vec::len).sum::<usize>()
            + history_override
                .into_iter()
                .flat_map(HashMap::values)
                .map(Vec::len)
                .sum::<usize>(),
        financial_records: data.financials.len(),
    }
}

pub(crate) struct PreparedBacktest {
    pub(crate) data: Arc<gp_core::CoreDataSet>,
    pub(crate) stock_override: Option<Arc<Vec<gp_core::StockItem>>>,
    pub(crate) history_override: HashMap<String, Vec<gp_core::HistoryBar>>,
    pub(crate) request: gp_core::BacktestRequest,
    pub(crate) notes: Vec<String>,
    pub(crate) metadata: Value,
}

#[tauri::command]
pub(crate) async fn api_screen(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    if is_hk_research_request(&payload) {
        return api_hk_screen(&app, payload).await;
    }
    if !legacy_screen_requested(&payload) {
        if let Some(result) =
            crate::stock_research_runtime::screen_if_selected(app.clone(), payload.clone()).await?
        {
            return Ok(result);
        }
    }
    if legacy_screen_requested(&payload) {
        return api_legacy_screen(app, payload).await;
    }
    let diagnostics_enabled = crate::perf_diagnostics::requested(&payload);
    let cache_strategy = if payload.get("data_policy").and_then(Value::as_str) == Some("refresh") {
        crate::perf_diagnostics::CacheStrategy::Refresh
    } else {
        crate::perf_diagnostics::CacheStrategy::CacheOnly
    };
    let started = Instant::now();
    let result = adaptive_screen_with_timeout(
        Duration::from_secs(ADAPTIVE_SCREEN_TOTAL_TIMEOUT_SECS),
        api_adaptive_screen(app, payload),
    )
    .await;
    if result
        .as_ref()
        .err()
        .is_some_and(|error| error.contains("端到端计算超过"))
    {
        crate::perf_diagnostics::record(
            diagnostics_enabled,
            crate::perf_diagnostics::PerfEvent::new(
                crate::perf_diagnostics::Flow::Screening,
                crate::perf_diagnostics::Stage::CommandTotal,
                started.elapsed(),
            )
            .with_cache_strategy(cache_strategy)
            .with_outcome(crate::perf_diagnostics::Outcome::Timeout),
        );
    }
    result
}

fn is_hk_research_request(payload: &Value) -> bool {
    payload.get("market").and_then(Value::as_str) == Some("hk")
}

async fn api_hk_screen(app: &tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let policy = DataPolicy::parse(&payload)?;
    let request = legacy_screen_criteria_from_payload(payload.clone())?;
    let (cache, data) = hk_projected_cache(app)?;
    if data.stocks.is_empty() {
        return Err(local_data::missing_error(
            "hk_screen",
            &[
                json!({"kind":"hk_quotes", "reason":"No HKD-listed equity quotes are cached yet. Refresh the HKEX directory to batch-load market quotes."}),
            ],
        ));
    }
    let provider_payload = &payload;
    let completed =
        hydrate_hk_screen_candidates(cache, data, HkCandidateScreenRequest::Screen(&request), Duration::from_secs(HK_SCREEN_FINANCIAL_PREPARE_TIMEOUT_SECS), |cache, codes, deadline| async move {
            hydrate_hk_screen_financials(app, provider_payload, &cache, &codes, deadline).await
        })
        .await?;
    let final_cache = completed.cache;
    let data = completed.data;
    let mut result = completed.result;
    let financial_hydration = completed.summary;
    let enriched_count =
        crate::hk_research::attach_cached_hk_financial_metrics(&mut result, &final_cache);
    append_hk_financial_hydration_note(&mut result, &financial_hydration);
    if let Some(object) = result.as_object_mut() {
        object.insert("market".to_owned(), json!("hk"));
        object.insert(
            "quality".to_owned(),
            json!({
                "mode":"cached_hk_screen",
                "exploratory":true,
                "limitations": crate::hk_research::hk_exploratory_limitations(),
                "quote_source_count": data.stocks.len(),
                "cache_only": true,
                "requested_data_policy": policy.as_str(),
                "financial_hydration": financial_hydration,
                "financial_enriched_count": enriched_count,
            }),
        );
    }
    local_data::attach_metadata(
        &mut result,
        hk_research_metadata(&final_cache, &data, policy.as_str()),
        &payload,
    );
    Ok(result)
}

struct HkScreenHydrationResult {
    cache: crate::hk_market::HkMarketCache,
    data: gp_core::CoreDataSet,
    result: Value,
    summary: Value,
}

const HK_SCREEN_FINANCIAL_PREPARE_TIMEOUT_SECS: u64 = 30;
const HK_SCREEN_FINANCIAL_CONCURRENCY: usize = 6;
const HK_SCREEN_FINANCIAL_DEFERRED: &str = "财务预载达到限时，点击刷新财务可重试。";

fn mark_hk_financial_preparation_errors(cache: &mut crate::hk_market::HkMarketCache, codes: &[String], reason: &str) {
    for code in codes {
        cache.financial_errors.entry(code.clone()).or_insert_with(|| reason.to_string());
    }
}

fn preserve_hk_prepared_financials(previous: &crate::hk_market::HkMarketCache, latest: &mut crate::hk_market::HkMarketCache) {
    let fetched_time = |snapshot: &crate::hk_market::HkFinancialSnapshot| {
        snapshot.candidate_fetched_at.parse::<u128>().ok()
            .or_else(|| crate::market::parse_cache_datetime_epoch_ms(&snapshot.candidate_fetched_at)).unwrap_or(0)
    };
    for (code, snapshot) in &previous.financials {
        if snapshot.code != *code { continue; }
        let previous_time = fetched_time(snapshot);
        let keep = latest.financials.get(code).is_none_or(|current| {
            previous_time > fetched_time(current)
        });
        if keep {
            latest.financials.insert(code.clone(), snapshot.clone());
            if let Some(error) = previous.financial_errors.get(code) { latest.financial_errors.insert(code.clone(), error.clone()); }
            else { latest.financial_errors.remove(code); }
        }
    }
    for (code, error) in &previous.financial_errors {
        let keep_error = latest.financials.get(code).is_none_or(|current| {
            previous.financials.get(code).is_some_and(|old| {
                fetched_time(current) <= fetched_time(old)
            })
        });
        if keep_error { latest.financial_errors.entry(code.clone()).or_insert_with(|| error.clone()); }
    }
}

#[derive(Clone, Copy)]
enum HkCandidateScreenRequest<'a> {
    Screen(&'a gp_core::ScreenCriteria),
    Grouped(&'a gp_core::SectorScreenRequest),
}

impl<'a> HkCandidateScreenRequest<'a> {
    fn criteria(self) -> &'a gp_core::ScreenCriteria {
        match self {
            Self::Screen(request) => request,
            Self::Grouped(request) => &request.criteria,
        }
    }

    fn calculate(self, data: &gp_core::CoreDataSet) -> Result<Value, String> {
        match self {
            Self::Screen(request) => serde_json::to_value(gp_core::screen_stocks(&data.stocks, request)),
            Self::Grouped(request) => serde_json::to_value(gp_core::sector_screen_stocks(&data.stocks, request)),
        }.map_err(|error| error.to_string())
    }

    fn limit(self) -> usize {
        match self {
            Self::Screen(request) => request.limit.clamp(1, 200),
            Self::Grouped(request) => request.criteria.limit.clamp(1, 200),
        }
    }
}

fn hk_unresolved_filter_candidates(data: &gp_core::CoreDataSet, criteria: &gp_core::ScreenCriteria) -> Vec<gp_core::StockItem> {
    data.stocks.iter().filter_map(|stock| {
        let mut relaxed = criteria.clone();
        let mut unresolved = false;
        if criteria.min_roe.is_some() && stock.roe.is_none() {
            relaxed.min_roe = None;
            unresolved = true;
        }
        if criteria.industry.as_deref().is_some_and(|industry| !industry.trim().is_empty()) && stock.industry.trim().is_empty() {
            relaxed.industry = None;
            unresolved = true;
        }
        if criteria.min_market_cap_billion.is_some() && stock.market_cap_billion.is_none() {
            relaxed.min_market_cap_billion = None;
            unresolved = true;
        }
        if criteria.min_deducted_net_profit_billion.is_some() && stock.deducted_net_profit_billion.is_none() {
            relaxed.min_deducted_net_profit_billion = None;
            unresolved = true;
        }
        if criteria.min_deducted_net_profit_margin.is_some() && stock.deducted_net_profit_margin.is_none() {
            relaxed.min_deducted_net_profit_margin = None;
            unresolved = true;
        }
        if criteria.min_deducted_net_profit_growth_rate.is_some() && stock.deducted_net_profit_growth_rate.is_none() {
            relaxed.min_deducted_net_profit_growth_rate = None;
            unresolved = true;
        }
        // Keep every known identity, quote and financial rejection. Unknown inputs only
        // make a candidate eligible for preparation, never for the final result.
        (unresolved && gp_core::screen_stocks(std::slice::from_ref(stock), &relaxed).total > 0).then(|| stock.clone())
    }).collect()
}

fn hk_filter_preparation_codes(
    data: &gp_core::CoreDataSet,
    request: HkCandidateScreenRequest<'_>,
    attempted: &HashSet<String>,
    cache: &crate::hk_market::HkMarketCache,
    now: u128,
) -> Vec<String> {
    let unresolved = hk_unresolved_filter_candidates(data, request.criteria()).into_iter()
        .filter(|stock| crate::hk_market::canonical_hk_code(&stock.code).is_some_and(|code| {
            !attempted.contains(&code)
                && hk_screen_candidate_refresh_needed(cache.financials.get(&code), now)
        }))
        .collect::<Vec<_>>();
    let mut ranking = request.criteria().clone();
    ranking.min_roe = None;
    ranking.industry = None;
    ranking.min_market_cap_billion = None;
    ranking.min_deducted_net_profit_billion = None;
    ranking.min_deducted_net_profit_margin = None;
    ranking.min_deducted_net_profit_growth_rate = None;
    ranking.limit = request.limit();
    gp_core::screen_stocks(&unresolved, &ranking).items.into_iter()
        .filter_map(|item| crate::hk_market::canonical_hk_code(&item.stock.code)).collect()
}

async fn hydrate_hk_screen_candidates<F, Fut>(
    mut cache: crate::hk_market::HkMarketCache,
    mut data: gp_core::CoreDataSet,
    request: HkCandidateScreenRequest<'_>,
    budget: Duration,
    mut hydrate: F,
) -> Result<HkScreenHydrationResult, String>
where
    F: FnMut(crate::hk_market::HkMarketCache, Vec<String>, tokio::time::Instant) -> Fut,
    Fut: std::future::Future<Output = Result<(crate::hk_market::HkMarketCache, Value), String>>,
{
    let mut result = request.calculate(&data)?;
    let mut financial_hydration = serde_json::json!({
        "requested_count": 0,
        "attempted_count": 0,
        "updated_count": 0,
        "failed_count": 0,
        "deferred_count": 0,
        "cache_write_failed_count": 0,
        "errors": [],
    });
    let mut attempted_codes = HashSet::new();
    let deadline = tokio::time::Instant::now() + budget;
    // New candidates can enter after every reranking. Only visit new codes, and continue until
    // the visible result has been checked; the cached security universe bounds the work.
    loop {
        let now = crate::hk_market::current_epoch_millis()
            .parse::<u128>()
            .unwrap_or_default();
        let candidate_codes = hk_screen_candidate_codes(&result, request.limit())
            .into_iter()
            .chain(hk_filter_preparation_codes(&data, request, &attempted_codes, &cache, now))
            .filter(|code| {
                !attempted_codes.contains(code)
                    && hk_screen_candidate_refresh_needed(cache.financials.get(code), now)
            })
            .collect::<Vec<_>>();
        let mut seen = HashSet::new();
        let candidate_codes = candidate_codes.into_iter().filter(|code| seen.insert(code.clone())).collect::<Vec<_>>();
        if candidate_codes.is_empty() {
            break;
        }
        attempted_codes.extend(candidate_codes.iter().cloned());
        let requested_count = candidate_codes.len();
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            mark_hk_financial_preparation_errors(&mut cache, &candidate_codes, HK_SCREEN_FINANCIAL_DEFERRED);
            merge_hk_hydration_summaries(&mut financial_hydration, &json!({"requested_count":requested_count,"deferred_count":requested_count,"errors":[HK_SCREEN_FINANCIAL_DEFERRED]}));
            break;
        }
        match tokio::time::timeout_at(deadline, hydrate(cache.clone(), candidate_codes.clone(), deadline)).await {
            Ok(Ok((mut updated_cache, hydration))) => {
                let updated_count = hydration["updated_count"].as_u64().unwrap_or(0);
                // A later successful write may reread a disk snapshot which lacks
                // earlier source data retained after a failed write in this run.
                preserve_hk_prepared_financials(&cache, &mut updated_cache);
                if hydration["failed_count"].as_u64().unwrap_or(0) > 0 {
                    let missing = candidate_codes.iter().filter(|code| !updated_cache.financials.contains_key(*code)).cloned().collect::<Vec<_>>();
                    let reason = hydration["errors"].as_array().and_then(|errors| errors.first()).and_then(Value::as_str).unwrap_or("本轮财务数据未准备完成，请点击刷新财务重试。");
                    mark_hk_financial_preparation_errors(&mut updated_cache, &missing, reason);
                }
                merge_hk_hydration_summaries(&mut financial_hydration, &hydration);
                cache = updated_cache;
                if updated_count > 0 {
                    if let Ok(refreshed_data) = crate::hk_research::project_hk_market_data(&cache) {
                        if let Ok(refreshed_result) = request.calculate(&refreshed_data) {
                            data = refreshed_data;
                            result = refreshed_result;
                        }
                    }
                } else {
                    break;
                }
            }
            Ok(Err(error)) => {
                mark_hk_financial_preparation_errors(&mut cache, &candidate_codes, &error);
                merge_hk_hydration_summaries(
                    &mut financial_hydration,
                    &json!({
                        "requested_count":requested_count,
                        "attempted_count":0,
                        "updated_count":0,
                        "failed_count":requested_count,
                        "errors":[format!("港股财务缓存刷新失败：{error}")],
                    }),
                );
                break;
            }
            Err(_) => {
                mark_hk_financial_preparation_errors(&mut cache, &candidate_codes, HK_SCREEN_FINANCIAL_DEFERRED);
                merge_hk_hydration_summaries(&mut financial_hydration, &json!({"requested_count":requested_count,"deferred_count":requested_count,"errors":[HK_SCREEN_FINANCIAL_DEFERRED]}));
                break;
            }
        }
    }
    let unresolved_codes = hk_unresolved_filter_candidates(&data, request.criteria()).into_iter()
        .filter_map(|stock| crate::hk_market::canonical_hk_code(&stock.code)).collect::<Vec<_>>();
    financial_hydration["filter_coverage"] = json!({
        "complete": unresolved_codes.is_empty(),
        "unresolved_count": unresolved_codes.len(),
        "unresolved_codes": unresolved_codes,
        "errors": unresolved_codes.iter().flat_map(|code| {
            let mut errors = cache.financial_errors.get(code).map(|reason| json!({"code":code,"reason":reason})).into_iter().collect::<Vec<_>>();
            if let Some(snapshot) = cache.financials.get(code) {
                errors.extend(snapshot.field_errors.iter().map(|(field, reason)| json!({"code":code,"field":field,"reason":reason})));
            }
            errors
        }).collect::<Vec<_>>(),
    });
    Ok(HkScreenHydrationResult {
        cache,
        data,
        result,
        summary: financial_hydration,
    })
}

fn hk_screen_candidate_codes(result: &Value, limit: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    let groups = result
        .get("groups")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|group| group.get("items").and_then(Value::as_array))
        .collect::<Vec<_>>();
    let visible_groups_have_rows = groups.iter().any(|items| !items.is_empty());
    let visible_items = if visible_groups_have_rows {
        // ScreenPanel renders all returned group rows; per-sector requests may return
        // more than ten, so financial preparation must include every visible candidate.
        groups
            .into_iter()
            .flat_map(|items| items.iter())
            .collect::<Vec<_>>()
    } else {
        result
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(limit)
            .collect::<Vec<_>>()
    };
    visible_items
        .into_iter()
        .filter_map(|item| {
            item.get("stock")
                .and_then(|stock| stock.get("code"))
                .and_then(Value::as_str)
                .and_then(crate::hk_market::canonical_hk_code)
                .filter(|code| seen.insert(code.clone()))
        })
        .collect()
}

fn hk_financial_snapshot_missing_core_fields(
    snapshot: &crate::hk_market::HkFinancialSnapshot,
) -> bool {
    snapshot.eps.value.is_none()
        || snapshot.roe.value.is_none()
        || snapshot.revenue.value.is_none()
        || snapshot.net_profit.value.is_none()
}

fn hk_financial_refresh_needed(
    snapshot: Option<&crate::hk_market::HkFinancialSnapshot>,
    now: u128,
) -> bool {
    const FRESH_FOR_MS: u128 = 12 * 60 * 60 * 1000;
    let Some(snapshot) = snapshot else {
        return true;
    };
    let fetched = snapshot.candidate_fetched_at.parse::<u128>().ok();
    if fetched.is_some_and(|fetched| now.saturating_sub(fetched) < FRESH_FOR_MS) {
        if !hk_financial_snapshot_missing_core_fields(snapshot) {
            return false;
        }
        // Older cache records predate Sina supplementation metadata. Give those incomplete
        // records one refresh attempt so the supplemental provider can fill their missing fields.
        // Once Sina was queried (successfully, with a period mismatch, or with an error), keep
        // the TTL to avoid hammering providers for values they did not supply.
        let sina_was_attempted = snapshot.supplemental_source.is_some()
            || snapshot.supplemental_fetched_at.is_some()
            || snapshot.candidate_source.contains("Sina")
            || snapshot.field_errors.contains_key("sina_supplement");
        return !sina_was_attempted;
    }
    true
}

fn hk_screen_candidate_refresh_needed(
    snapshot: Option<&crate::hk_market::HkFinancialSnapshot>,
    now: u128,
) -> bool {
    hk_financial_refresh_needed(snapshot, now)
        || snapshot.is_some_and(|snapshot| {
            snapshot.company_profile.is_none()
                && !snapshot.field_errors.contains_key("company_profile")
        })
}

fn merge_hk_hydration_summaries(total: &mut Value, batch: &Value) {
    for key in [
        "requested_count",
        "attempted_count",
        "updated_count",
        "failed_count",
        "deferred_count",
        "cache_write_failed_count",
    ] {
        total[key] = json!(total[key].as_u64().unwrap_or(0) + batch[key].as_u64().unwrap_or(0));
    }
    if let (Some(total_errors), Some(batch_errors)) =
        (total["errors"].as_array_mut(), batch["errors"].as_array())
    {
        total_errors.extend(
            batch_errors
                .iter()
                .take(3usize.saturating_sub(total_errors.len()))
                .cloned(),
        );
    }
}

#[derive(Default, serde::Serialize)]
struct HkScreenFinancialHydration {
    requested_count: usize,
    attempted_count: usize,
    updated_count: usize,
    failed_count: usize,
    deferred_count: usize,
    cache_write_failed_count: usize,
    errors: Vec<String>,
}

type HkScreenFinancialOutcome = (String, Result<crate::hk_market::HkFinancialSnapshot, String>);

struct HkScreenFinancialFetch {
    outcomes: Vec<HkScreenFinancialOutcome>,
    deferred_codes: Vec<String>,
    attempted_count: usize,
}

async fn collect_hk_screen_financials<F, Fut>(codes: Vec<String>, deadline: tokio::time::Instant, mut fetch: F) -> HkScreenFinancialFetch
where F: FnMut(String) -> Fut, Fut: std::future::Future<Output = Result<crate::hk_market::HkFinancialSnapshot, String>> {
    let mut pending = codes.iter().cloned().collect::<HashSet<_>>();
    let mut attempted_count = 0;
    let mut fetching = stream::iter(codes.into_iter().map(|code| {
        attempted_count += 1;
        let future = fetch(code.clone());
        async move { (code, future.await) }
    })).buffer_unordered(HK_SCREEN_FINANCIAL_CONCURRENCY);
    let mut outcomes = Vec::new();
    while let Ok(Some(outcome)) = tokio::time::timeout_at(deadline, fetching.next()).await {
        pending.remove(&outcome.0);
        outcomes.push(outcome);
    }
    drop(fetching);
    let mut deferred_codes = pending.into_iter().collect::<Vec<_>>();
    deferred_codes.sort();
    HkScreenFinancialFetch { outcomes, deferred_codes, attempted_count }
}

fn apply_hk_screen_financial_outcomes(cache: &mut crate::hk_market::HkMarketCache, outcomes: &[HkScreenFinancialOutcome]) {
    for (code, financial) in outcomes {
        match financial {
            Ok(snapshot) => {
                let mut snapshot = snapshot.clone();
                if let Some(previous) = cache.financials.get(code) {
                    crate::hk_market::preserve_same_period_hk_financials(previous, &mut snapshot);
                }
                if let Some(quote) = cache.quotes.get(code) {
                    crate::hk_market::enrich_hk_financial_snapshot_from_quote(&mut snapshot, quote);
                }
                cache.financials.insert(code.clone(), snapshot);
                cache.financial_errors.remove(code);
            }
            Err(error) => { cache.financial_errors.insert(code.clone(), error.clone()); }
        }
    }
}

async fn hydrate_hk_screen_financials(
    app: &tauri::AppHandle,
    payload: &Value,
    cache: &crate::hk_market::HkMarketCache,
    candidate_codes: &[String],
    deadline: tokio::time::Instant,
) -> Result<(crate::hk_market::HkMarketCache, Value), String> {
    let path = crate::hk_market::hk_market_data_path(app)?;
    let now = crate::hk_market::current_epoch_millis()
        .parse::<u128>()
        .unwrap_or_default();
    let requested = candidate_codes
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    let codes = candidate_codes
        .iter()
        .filter(|code| hk_screen_candidate_refresh_needed(cache.financials.get(*code), now))
        .cloned()
        .collect::<Vec<_>>();
    let mut summary = HkScreenFinancialHydration {
        requested_count: requested.len(),
        ..Default::default()
    };
    if codes.is_empty() {
        return Ok((
            cache.clone(),
            serde_json::to_value(summary).unwrap_or(Value::Null),
        ));
    }
    let timeout = Duration::from_secs(8);
    let client = crate::market::build_http_client_with_proxy(
        "Mozilla/5.0 GuXuanYou HK screening financials",
        timeout,
        Some(payload),
    );
    let client = match client {
        Ok(client) => client,
        Err(error) => {
            summary.failed_count = codes.len();
            let reason = format!("无法创建财务数据客户端：{error}");
            summary.errors.push(reason.clone());
            let mut refreshed = cache.clone();
            mark_hk_financial_preparation_errors(&mut refreshed, &codes, &reason);
            return Ok((
                refreshed,
                serde_json::to_value(summary).unwrap_or(Value::Null),
            ));
        }
    };
    let fetched_at = crate::hk_market::current_epoch_millis();
    let fetched = collect_hk_screen_financials(codes, deadline, |code| {
        let client = client.clone();
        let fetched_at = fetched_at.clone();
        async move {
            crate::hk_market::fetch_hk_financial_snapshot(&client, &code, &fetched_at).await
        }
    }).await;
    summary.attempted_count = fetched.attempted_count;
    summary.deferred_count = fetched.deferred_codes.len();
    for (code, financial) in &fetched.outcomes {
        match financial {
            Ok(_) => summary.updated_count += 1,
            Err(error) => {
                summary.failed_count += 1;
                if summary.errors.len() < 3 { summary.errors.push(format!("{code}: {error}")); }
            }
        }
    }

    let mut refreshed = cache.clone();
    // Keep accepted source data in this response even when persistence fails.
    apply_hk_screen_financial_outcomes(&mut refreshed, &fetched.outcomes);
    if !fetched.outcomes.is_empty() {
        let result = crate::hk_market::update_hk_market_cache(&path, |latest| {
            apply_hk_screen_financial_outcomes(latest, &fetched.outcomes);
            Ok(())
        });
        match result {
            Ok((updated_cache, ())) => refreshed = updated_cache,
            Err(error) => {
                summary.cache_write_failed_count = fetched.outcomes.len();
                let reason = format!("财务缓存写入失败：{error}");
                let codes = fetched.outcomes.iter().map(|(code, _)| code.clone()).collect::<Vec<_>>();
                mark_hk_financial_preparation_errors(&mut refreshed, &codes, &reason);
                if summary.errors.len() < 3 { summary.errors.push(reason); }
            }
        }
    }
    mark_hk_financial_preparation_errors(&mut refreshed, &fetched.deferred_codes, HK_SCREEN_FINANCIAL_DEFERRED);
    Ok((
        refreshed,
        serde_json::to_value(summary).unwrap_or(Value::Null),
    ))
}

fn append_hk_financial_hydration_note(result: &mut Value, summary: &Value) {
    let updated = summary["updated_count"].as_u64().unwrap_or(0);
    let failed = summary["failed_count"].as_u64().unwrap_or(0);
    let attempted = summary["attempted_count"].as_u64().unwrap_or(0);
    let deferred = summary["deferred_count"].as_u64().unwrap_or(0);
    let cache_failed = summary["cache_write_failed_count"].as_u64().unwrap_or(0);
    let Some(object) = result.as_object_mut() else {
        return;
    };
    let notes = object.entry("notes").or_insert_with(|| json!([]));
    let Some(notes) = notes.as_array_mut() else {
        return;
    };
    if updated > 0 {
        notes.push(json!(format!(
            "本次港股选股已补充 {updated}/{attempted} 只候选的财务数据，并用更新后的缓存重新计分。"
        )));
    }
    if failed > 0 {
        let errors = summary["errors"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("；")
            })
            .unwrap_or_default();
        notes.push(json!(format!(
            "财务数据补充失败 {failed} 只，已有缓存继续使用。{}",
            if errors.is_empty() {
                String::new()
            } else {
                format!(" 示例：{errors}")
            }
        )));
    }
    if deferred > 0 { notes.push(json!(format!("财务预载暂未完成 {deferred} 只，已有数据可直接查看；需要时可点击刷新财务。"))); }
    if cache_failed > 0 { notes.push(json!(format!("{cache_failed} 只财务缓存保存失败，本页保留当前可读取的数据。"))); }
    let unresolved = summary["filter_coverage"]["unresolved_count"].as_u64().unwrap_or(0);
    if unresolved > 0 { notes.push(json!(format!("{unresolved} 只候选缺少筛选条件所需的财务或行业数据，匹配覆盖尚未完整；这些候选暂未计入匹配数，不能视为已确认不符合条件。"))); }
}

fn hk_projected_cache(
    app: &tauri::AppHandle,
) -> Result<(crate::hk_market::HkMarketCache, gp_core::CoreDataSet), String> {
    let path = crate::hk_market::hk_market_data_path(app)?;
    let cache = crate::hk_market::read_hk_market_cache(&path)?;
    let data = crate::hk_research::project_hk_market_data(&cache)?;
    Ok((cache, data))
}

fn hk_research_metadata(
    cache: &crate::hk_market::HkMarketCache,
    data: &gp_core::CoreDataSet,
    requested_data_policy: &str,
) -> Value {
    let quote_as_of = data
        .stocks
        .iter()
        .filter_map(|stock| stock.quote_time.as_deref())
        .filter_map(crate::market::compact_date_key)
        .max();
    let history_as_of = data
        .histories
        .values()
        .flat_map(|bars| bars.iter().map(|bar| bar.date.as_str()))
        .filter_map(crate::market::compact_date_key)
        .max();
    json!({
        "market":"hk",
        "source":"hk_market_cache",
        "cache_only":true,
        "data_policy":"cache_only",
        "requested_data_policy":requested_data_policy,
        "quote_as_of":quote_as_of,
        "history_as_of":history_as_of,
        "universe_count":data.stocks.len(),
        "history_symbol_count":data.histories.len(),
        "quote_provenance":crate::hk_research::quote_provenance(cache, data),
        "securities_updated_at":cache.securities_updated_at,
        "limitations":crate::hk_research::hk_exploratory_limitations(),
    })
}

pub(crate) fn legacy_screen_requested(payload: &Value) -> bool {
    payload
        .get("internal_algorithm")
        .and_then(Value::as_str)
        .is_some_and(|algorithm| algorithm.eq_ignore_ascii_case("legacy_balanced"))
}

pub(crate) async fn adaptive_screen_with_timeout<T>(
    duration: Duration,
    future: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    tokio::time::timeout(duration, future).await.map_err(|_| {
        format!(
            "智能选股端到端计算超过 {ADAPTIVE_SCREEN_TOTAL_TIMEOUT_SECS} 秒，请检查网络或刷新行情后重试"
        )
    })?
}

pub(crate) async fn api_adaptive_screen(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let transport = payload.clone();
    let diagnostics_enabled = crate::perf_diagnostics::requested(&transport);
    let cache_strategy = if transport.get("data_policy").and_then(Value::as_str) == Some("refresh")
    {
        crate::perf_diagnostics::CacheStrategy::Refresh
    } else {
        crate::perf_diagnostics::CacheStrategy::CacheOnly
    };
    let started_at = Instant::now();
    let prepared = match prepare_adaptive_screen(&app, payload).await {
        Ok(prepared) => prepared,
        Err(error) => {
            crate::perf_diagnostics::record(
                diagnostics_enabled,
                crate::perf_diagnostics::PerfEvent::new(
                    crate::perf_diagnostics::Flow::Screening,
                    crate::perf_diagnostics::Stage::CommandTotal,
                    started_at.elapsed(),
                )
                .with_cache_strategy(cache_strategy)
                .with_outcome(crate::perf_diagnostics::Outcome::Error),
            );
            return Err(error);
        }
    };
    let PreparedAdaptiveScreen {
        data,
        stock_override,
        candidate_codes,
        history_override,
        request,
        recent_exposure,
        notes,
        metadata,
        cache_hit,
    } = prepared;
    let counts = adaptive_perf_counts(
        data.as_ref(),
        stock_override.as_deref().map(Vec::as_slice),
        Some(&history_override),
    );
    let candidate_count = candidate_codes.len();
    emit_adaptive_screen_progress(
        &app,
        request.run_id.as_deref(),
        "regime",
        82,
        "判断市场状态",
    );
    let calculation_request = request.clone();
    let calculation_started = Instant::now();
    let calculation = crate::runtime::run_cpu_bound("api_screen", move || {
        let calculation_as_of = calculation_request.as_of_date.as_deref();
        let universe = stock_override
            .as_deref()
            .map(Vec::as_slice)
            .unwrap_or(data.stocks.as_slice());
        let mut histories = candidate_codes
            .iter()
            .map(String::as_str)
            .chain(crate::market::adaptive_benchmark_codes())
            .filter_map(|code| {
                data.histories.get(code).map(|rows| {
                    (
                        code.to_string(),
                        adaptive_history_window(rows, calculation_as_of),
                    )
                })
            })
            .collect::<HashMap<_, _>>();
        histories.extend(
            history_override
                .into_iter()
                .map(|(code, rows)| (code, adaptive_history_window(&rows, calculation_as_of))),
        );
        let benchmarks = crate::market::adaptive_benchmark_codes()
            .into_iter()
            .filter_map(|code| {
                histories
                    .get(code)
                    .cloned()
                    .map(|rows| (code.to_string(), rows))
            })
            .collect::<HashMap<_, _>>();
        let point_in_time_universe =
            adaptive_point_in_time_universe(universe, &histories, calculation_as_of);
        let result = gp_core::adaptive_screen_stocks(
            &point_in_time_universe,
            &histories,
            &benchmarks,
            &recent_exposure,
            &calculation_request,
        )
        .map_err(|error| error.to_string())?;
        serde_json::to_value(result).map_err(|error| error.to_string())
    })
    .await
    .and_then(|result| result);
    let calculation_elapsed = calculation_started.elapsed();
    let calculation_event = crate::perf_diagnostics::PerfEvent::new(
        crate::perf_diagnostics::Flow::Screening,
        crate::perf_diagnostics::Stage::RustCalculation,
        calculation_elapsed,
    )
    .with_cache_strategy(cache_strategy)
    .with_counts(
        counts.stock_count,
        counts.history_rows,
        counts.financial_records,
    )
    .with_candidate_count(candidate_count)
    .with_outcome(if calculation.is_ok() {
        crate::perf_diagnostics::Outcome::Ok
    } else {
        crate::perf_diagnostics::Outcome::Error
    });
    crate::perf_diagnostics::record(diagnostics_enabled, calculation_event);
    let mut result = calculation?;
    crate::market::append_result_notes(&mut result, notes);
    emit_adaptive_screen_progress(
        &app,
        request.run_id.as_deref(),
        "ranking",
        94,
        "生成主榜与探索榜",
    );
    let exposure_app = app.clone();
    let exposure_result = result.clone();
    let exposure_date = result
        .pointer("/market_regime/as_of_date")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            crate::market::local_yyyymmdd_from_epoch_ms(crate::market::epoch_millis())
                .unwrap_or_else(|| "19700101".to_string())
        });
    let exposure_write_date = exposure_date.clone();
    let persistence_started = Instant::now();
    let exposure_write =
        crate::runtime::run_io_bound("adaptive_screen_exposure_write", move || {
            adaptive_exposure_record_sync(&exposure_app, &exposure_result, &exposure_write_date)
        })
        .await?;
    let persistence_elapsed = persistence_started.elapsed();
    crate::perf_diagnostics::record(
        diagnostics_enabled,
        crate::perf_diagnostics::PerfEvent::new(
            crate::perf_diagnostics::Flow::Screening,
            crate::perf_diagnostics::Stage::ResultPersistence,
            persistence_elapsed,
        )
        .with_cache_strategy(cache_strategy)
        .with_counts(
            counts.stock_count,
            counts.history_rows,
            counts.financial_records,
        )
        .with_candidate_count(candidate_count)
        .with_outcome(if exposure_write.is_ok() {
            crate::perf_diagnostics::Outcome::Ok
        } else {
            crate::perf_diagnostics::Outcome::Error
        }),
    );
    if let Err(error) = exposure_write {
        crate::market::append_result_notes(
            &mut result,
            vec![format!("近期曝光记录写入失败：{error}")],
        );
    }
    emit_adaptive_screen_progress(&app, request.run_id.as_deref(), "complete", 100, "选股完成");
    let run_app = app.clone();
    let run_result = result.clone();
    let run_id = request.run_id.clone();
    let release_evidence_qualified = adaptive_release_screen_request_qualified(&request);
    let elapsed_millis = started_at.elapsed().as_millis().min(u64::MAX as u128) as u64;
    tauri::async_runtime::spawn(async move {
        let _ = crate::runtime::run_io_bound("adaptive_screen_run_record", move || {
            adaptive_release_run_record_sync(
                &run_app,
                run_id.as_deref(),
                &run_result,
                &exposure_date,
                elapsed_millis,
                cache_hit,
                release_evidence_qualified,
            )
        })
        .await;
    });
    local_data::attach_metadata(&mut result, metadata, &transport);
    let total_elapsed = started_at.elapsed();
    crate::perf_diagnostics::record(
        diagnostics_enabled,
        crate::perf_diagnostics::PerfEvent::new(
            crate::perf_diagnostics::Flow::Screening,
            crate::perf_diagnostics::Stage::CommandTotal,
            total_elapsed,
        )
        .with_cache_strategy(cache_strategy)
        .with_counts(
            counts.stock_count,
            counts.history_rows,
            counts.financial_records,
        )
        .with_candidate_count(candidate_count)
        .with_outcome(if cache_hit {
            crate::perf_diagnostics::Outcome::CacheHit
        } else {
            crate::perf_diagnostics::Outcome::CacheMiss
        }),
    );
    Ok(result)
}

pub(crate) async fn api_legacy_screen(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let policy = DataPolicy::parse(&payload)?;
    let transport = payload.clone();
    let data = local_snapshot(&app, &payload, "api_legacy_screen")?;
    let metadata = local_metadata(data.as_ref(), policy, &[]);
    let stock_override = screen_stock_override(&app, &data, &payload)?;
    let criteria = legacy_screen_criteria_from_payload(payload)?;
    let mut result = crate::runtime::run_cpu_bound("api_legacy_screen", move || {
        let result = match stock_override.as_deref() {
            Some(stocks) => gp_core::screen_stocks(stocks, &criteria),
            None => gp_core::screen_with_data(data.as_ref(), &criteria)
                .map_err(|error| error.to_string())?,
        };
        serde_json::to_value(result).map_err(|error| error.to_string())
    })
    .await??;
    if let Some(object) = result.as_object_mut() {
        object.insert(
            "algorithm_version".to_string(),
            Value::String("legacy_balanced".to_string()),
        );
        object.insert(
            "rollout".to_string(),
            json!({
                "adaptive_available": true,
                "adaptive_default_enabled": true,
                "reason": "legacy_balanced was requested explicitly for compatibility"
            }),
        );
    }
    crate::market::append_result_notes(
        &mut result,
        vec![
            "本次选股按显式兼容请求使用 legacy_balanced；未指定算法时默认使用 adaptive_swing_v1。"
                .to_string(),
        ],
    );
    local_data::attach_metadata(&mut result, metadata, &transport);
    Ok(result)
}

pub(crate) fn legacy_screen_criteria_from_payload(
    mut payload: Value,
) -> Result<gp_core::ScreenCriteria, String> {
    let nested = payload.get("criteria").is_some();
    let legacy_limit = if nested {
        payload.get("primary_limit")
    } else {
        payload.get("limit")
    }
    .and_then(Value::as_u64)
    .and_then(|value| usize::try_from(value).ok())
    .unwrap_or(10)
    .clamp(1, 50);
    if let Some(criteria) = payload.get("criteria").cloned() {
        payload = criteria;
    }
    if let Some(object) = payload.as_object_mut() {
        object.remove("internal_algorithm");
        object.insert("limit".to_string(), json!(legacy_limit));
    }
    serde_json::from_value::<gp_core::ScreenCriteria>(local_data::core_payload(payload))
        .map_err(|error| format!("invalid legacy screen request: {error}"))
}

pub(crate) fn adaptive_screen_request_from_payload(
    payload: Value,
) -> Result<gp_core::AdaptiveScreenRequest, String> {
    let payload = local_data::core_payload(payload);
    if payload.get("criteria").is_some() {
        serde_json::from_value(payload)
            .map_err(|error| format!("invalid adaptive screen request: {error}"))
    } else {
        let criteria = serde_json::from_value::<gp_core::ScreenCriteria>(payload)
            .map_err(|error| format!("invalid legacy screen request: {error}"))?;
        Ok(gp_core::AdaptiveScreenRequest {
            criteria,
            ..gp_core::AdaptiveScreenRequest::default()
        })
    }
}

pub(crate) fn adaptive_required_history_codes(candidates: &[String]) -> Vec<String> {
    let mut required_codes = candidates.to_vec();
    required_codes.extend(
        crate::market::adaptive_benchmark_codes()
            .into_iter()
            .map(str::to_string),
    );
    crate::market::dedupe_stock_codes(&mut required_codes);
    required_codes
}

pub(crate) fn adaptive_missing_history_codes(
    data: &gp_core::CoreDataSet,
    required_codes: &[String],
    target_history_date: Option<&str>,
) -> Vec<String> {
    required_codes
        .iter()
        .filter(|code| {
            !adaptive_history_cache_is_usable(
                data,
                code,
                target_history_date,
                MIN_ADAPTIVE_SCREEN_HISTORY_BARS,
            )
        })
        .cloned()
        .collect()
}

pub(crate) fn emit_adaptive_screen_progress(
    app: &tauri::AppHandle,
    run_id: Option<&str>,
    stage: &str,
    percent: usize,
    message: &str,
) {
    let _ = app.emit(
        "adaptive-screen-progress",
        adaptive_screen_progress_payload(run_id, stage, percent, message),
    );
}

pub(crate) fn adaptive_screen_progress_payload(
    run_id: Option<&str>,
    stage: &str,
    percent: usize,
    message: &str,
) -> Value {
    json!({
        "run_id": run_id,
        "stage": stage,
        "percent": percent,
        "message": message,
    })
}

#[tauri::command]
pub(crate) async fn api_sector_screen(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    if is_hk_research_request(&payload) {
        let policy = DataPolicy::parse(&payload)?;
        let transport = payload.clone();
        let request = serde_json::from_value::<gp_core::SectorScreenRequest>(
            local_data::core_payload(payload),
        )
        .map_err(|error| format!("invalid HK sector screen request: {error}"))?;
        let (cache, data) = hk_projected_cache(&app)?;
        let provider_app = &app;
        let provider_payload = &transport;
        let completed = hydrate_hk_screen_candidates(cache, data, HkCandidateScreenRequest::Grouped(&request), Duration::from_secs(HK_SCREEN_FINANCIAL_PREPARE_TIMEOUT_SECS), |cache, codes, deadline| async move {
            hydrate_hk_screen_financials(provider_app, provider_payload, &cache, &codes, deadline).await
        }).await?;
        let mut result = completed.result;
        let enriched = crate::hk_research::attach_cached_hk_financial_metrics(&mut result, &completed.cache);
        append_hk_financial_hydration_note(&mut result, &completed.summary);
        result["market"] = json!("hk");
        let mut metadata = hk_research_metadata(&completed.cache, &completed.data, policy.as_str());
        metadata["financial_hydration"] = completed.summary;
        metadata["financial_enriched_count"] = json!(enriched);
        local_data::attach_metadata(
            &mut result,
            metadata,
            &transport,
        );
        return Ok(result);
    }
    let policy = DataPolicy::parse(&payload)?;
    let transport = payload.clone();
    let data = local_snapshot(&app, &payload, "api_sector_screen")?;
    let metadata = local_metadata(data.as_ref(), policy, &[]);
    let stock_override = screen_stock_override(&app, &data, &payload)?;
    let request =
        serde_json::from_value::<gp_core::SectorScreenRequest>(local_data::core_payload(payload))
            .map_err(|error| format!("invalid sector screen request: {error}"))?;
    let mut result = crate::runtime::run_cpu_bound("api_sector_screen", move || {
        let result = match stock_override.as_deref() {
            Some(stocks) => gp_core::sector_screen_stocks(stocks, &request),
            None => gp_core::sector_screen_with_data(data.as_ref(), &request)
                .map_err(|error| error.to_string())?,
        };
        serde_json::to_value(result).map_err(|error| error.to_string())
    })
    .await??;
    local_data::attach_metadata(&mut result, metadata, &transport);
    Ok(result)
}

#[tauri::command]
pub(crate) async fn api_custom_screen(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    run_graph_screen_command("api_custom_screen", app, payload).await
}

#[tauri::command]
pub(crate) async fn api_graph_screen(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    run_graph_screen_command("api_graph_screen", app, payload).await
}

#[tauri::command]
pub(crate) async fn api_trend_analyze(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    if is_hk_research_request(&payload) {
        let policy = DataPolicy::parse(&payload)?;
        let transport = payload.clone();
        let request = serde_json::from_value::<gp_core::TrendIndicatorRequest>(
            local_data::core_payload(payload),
        )
        .map_err(|error| format!("invalid HK trend request: {error}"))?;
        let (cache, data) = hk_projected_cache(&app)?;
        let calc_data = data.clone();
        let mut result = crate::runtime::run_cpu_bound("api_hk_trend_analyze", move || {
            let source = gp_core::StaticDataSource::new(&calc_data);
            let result =
                gp_core::trend_with_source(&source, &request).map_err(|error| error.to_string())?;
            serde_json::to_value(result).map_err(|error| error.to_string())
        })
        .await??;
        local_data::attach_metadata(
            &mut result,
            hk_research_metadata(&cache, &data, policy.as_str()),
            &transport,
        );
        return Ok(result);
    }
    let policy = DataPolicy::parse(&payload)?;
    let transport = payload.clone();
    let data = local_snapshot(&app, &payload, "api_trend_analyze")?;
    let stock_override = screen_stock_override(&app, &data, &payload)?;
    let request =
        serde_json::from_value::<gp_core::TrendIndicatorRequest>(local_data::core_payload(payload))
            .map_err(|error| format!("invalid trend request: {error}"))?;
    let coverage = local_history_coverage(
        data.as_ref(),
        &HashMap::new(),
        &[request.code.clone()],
        &request.start_date,
        &request.end_date,
        1,
    );
    local_data::require_coverage("trend", &coverage)?;
    let metadata = local_metadata(data.as_ref(), policy, &coverage);
    let mut result = crate::runtime::run_cpu_bound("api_trend_analyze", move || {
        let source = match stock_override.as_deref() {
            Some(stocks) => gp_core::StaticDataSource::with_stocks(data.as_ref(), stocks),
            None => gp_core::StaticDataSource::new(data.as_ref()),
        };
        let result =
            gp_core::trend_with_source(&source, &request).map_err(|error| error.to_string())?;
        serde_json::to_value(result).map_err(|error| error.to_string())
    })
    .await??;
    local_data::attach_metadata(&mut result, metadata, &transport);
    Ok(result)
}

#[tauri::command]
pub(crate) async fn api_trend_screen(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    api_trend_screen_inner(app, payload).await
}

pub(crate) async fn api_trend_screen_inner(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    if is_hk_research_request(&payload) {
        let policy = DataPolicy::parse(&payload)?;
        let transport = payload.clone();
        let request = serde_json::from_value::<gp_core::TrendScreenRequest>(
            local_data::core_payload(payload),
        )
        .map_err(|error| format!("invalid HK trend screen request: {error}"))?;
        let (cache, data) = hk_projected_cache(&app)?;
        let candidates = gp_core::trend_screen_candidate_codes(&data.stocks, &request);
        let prepared = hk_history::prepare(
            &app,
            &transport,
            cache,
            candidates,
            &request.start_date,
            &request.end_date,
            MIN_TREND_SCREEN_HISTORY_BARS,
        )
        .await?;
        hk_history::require_usable(&prepared, "hk_trend_screen", false)?;
        let hk_history::PreparedHistory {
            cache,
            data,
            summary,
            coverage,
            ..
        } = prepared;
        let calc_data = data.clone();
        let mut result = crate::runtime::run_cpu_bound("api_hk_trend_screen", move || {
            let result = gp_core::trend_screen_with_data(&calc_data, &request)
                .map_err(|error| error.to_string())?;
            serde_json::to_value(result).map_err(|error| error.to_string())
        })
        .await??;
        crate::hk_research::attach_cached_hk_financial_metrics(&mut result, &cache);
        append_hk_history_note(&mut result, &summary);
        local_data::attach_metadata(
            &mut result,
            hk_history_metadata(&cache, &data, &summary, &coverage, policy.as_str()),
            &transport,
        );
        return Ok(result);
    }
    let transport = payload.clone();
    let prepared = tokio::time::timeout(
        Duration::from_secs(TREND_SCREEN_HISTORY_TIMEOUT_SECS),
        prepare_trend_screen(&app, payload),
    )
    .await
    .map_err(|_| {
        format!("趋势日线准备超过 {TREND_SCREEN_HISTORY_TIMEOUT_SECS} 秒，请稍后重新运行筛选。")
    })??;
    let PreparedTrendScreen {
        data,
        stock_override,
        history_override,
        request,
        notes,
        metadata,
    } = prepared;
    let mut result = crate::runtime::run_cpu_bound("api_trend_screen", move || {
        let history_override = (!history_override.is_empty()).then_some(&history_override);
        let source = gp_core::StaticDataSource::with_overrides(
            data.as_ref(),
            stock_override.as_deref().map(Vec::as_slice),
            history_override,
        );
        let result = gp_core::trend_screen_with_source(&source, &request)
            .map_err(|error| error.to_string())?;
        serde_json::to_value(result).map_err(|error| error.to_string())
    })
    .await??;
    crate::market::append_result_notes(&mut result, notes);
    local_data::attach_metadata(&mut result, metadata, &transport);
    Ok(result)
}

pub(crate) fn backtest_history_timeout_secs(payload: &Value) -> u64 {
    if payload
        .get(stringify!(internal_release_validation))
        .and_then(Value::as_bool)
        == Some(true)
    {
        ADAPTIVE_RELEASE_BACKTEST_HISTORY_TIMEOUT_SECS
    } else {
        BACKTEST_HISTORY_TIMEOUT_SECS
    }
}

#[tauri::command]
pub(crate) async fn api_backtest(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    if is_hk_research_request(&payload) {
        return api_hk_backtest(&app, payload).await;
    }
    if let Some(result) =
        crate::stock_research_runtime::backtest_if_selected(app.clone(), payload.clone()).await?
    {
        return Ok(result);
    }
    let transport = payload.clone();
    let history_timeout_secs = backtest_history_timeout_secs(&payload);
    let prepared = tokio::time::timeout(
        Duration::from_secs(history_timeout_secs),
        prepare_backtest(&app, payload),
    )
    .await
    .map_err(|_| {
        format!(
            "backtest history prefetch exceeded {history_timeout_secs}s; retry after refreshing market data."
        )
    })??;
    let PreparedBacktest {
        data,
        stock_override,
        history_override,
        request,
        notes,
        metadata,
    } = prepared;
    let adaptive_backtest = request
        .strategy_mode
        .trim()
        .to_ascii_lowercase()
        .starts_with("adaptive_swing_v1");
    let operational_evidence = if adaptive_backtest {
        let evidence_app = app.clone();
        crate::runtime::run_io_bound("adaptive_release_operational_evidence", move || {
            adaptive_release_operational_evidence_sync(&evidence_app)
        })
        .await??
    } else {
        (None, None, None)
    };
    let calculation = crate::runtime::run_cpu_bound("api_backtest", move || {
        let history_override = (!history_override.is_empty()).then_some(&history_override);
        let source = gp_core::StaticDataSource::with_overrides(
            data.as_ref(),
            stock_override.as_deref().map(Vec::as_slice),
            history_override,
        );
        let result =
            gp_core::backtest_with_source(&source, &request).map_err(|error| error.to_string())?;
        let mut value = serde_json::to_value(&result).map_err(|error| error.to_string())?;
        if adaptive_backtest {
            let mut legacy_request = request.clone();
            legacy_request.strategy_mode = "walk_forward".to_string();
            legacy_request.criteria.score_profile = "balanced".to_string();
            let legacy = gp_core::backtest_with_source(&source, &legacy_request)
                .map_err(|error| format!("legacy balanced comparison failed: {error}"))?;
            let precision_pair = (request.top_n == 10)
                .then_some((legacy.metrics.precision_at_n, result.metrics.precision_at_n));
            let release_qualification =
                adaptive_release_backtest_qualification(&request, result.metrics.oos_fold_count);
            let gate_input = gp_core::AdaptiveReleaseGateInput {
                release_configuration_qualified: release_qualification
                    .get("qualified")
                    .and_then(Value::as_bool),
                legacy_annualized_return: legacy.metrics.annualized_return,
                adaptive_annualized_return: result.metrics.annualized_return,
                legacy_max_drawdown: legacy.metrics.max_drawdown,
                adaptive_max_drawdown: result.metrics.max_drawdown,
                legacy_precision_at_10: precision_pair.and_then(|pair| pair.0),
                adaptive_precision_at_10: precision_pair.and_then(|pair| pair.1),
                max_primary_industry_count: adaptive_backtest_max_industry_count(
                    &result,
                    data.as_ref(),
                ),
                average_adjacent_jaccard: adaptive_backtest_average_jaccard(&result),
                five_run_unique_coverage: operational_evidence.0,
                first_run_millis: operational_evidence.1,
                cached_run_millis: operational_evidence.2,
            };
            let gate = gp_core::evaluate_adaptive_release_gate(&gate_input);
            if let Some(object) = value.as_object_mut() {
                object.insert(
                    "legacy_balanced_backtest".to_string(),
                    serde_json::to_value(legacy).map_err(|error| error.to_string())?,
                );
                object.insert(
                    "adaptive_release_gate".to_string(),
                    serde_json::to_value(gate).map_err(|error| error.to_string())?,
                );
                object.insert(
                    "adaptive_release_gate_input".to_string(),
                    serde_json::to_value(gate_input).map_err(|error| error.to_string())?,
                );
                object.insert(
                    "adaptive_release_qualification".to_string(),
                    release_qualification,
                );
            }
        }
        Ok(value)
    })
    .await?;
    let mut result = calculation.map_err(|error| {
        if notes.is_empty() {
            error
        } else {
            format!("{error}；数据准备：{}", notes.join(" | "))
        }
    })?;
    if adaptive_backtest {
        let gate_input = serde_json::from_value::<gp_core::AdaptiveReleaseGateInput>(
            result
                .get("adaptive_release_gate_input")
                .cloned()
                .ok_or_else(|| "adaptive release gate input is missing".to_string())?,
        )
        .map_err(|error| format!("invalid adaptive release gate input: {error}"))?;
        let gate_report = serde_json::from_value::<gp_core::AdaptiveReleaseGateReport>(
            result
                .get("adaptive_release_gate")
                .cloned()
                .ok_or_else(|| "adaptive release gate report is missing".to_string())?,
        )
        .map_err(|error| format!("invalid adaptive release gate report: {error}"))?;
        let qualification = result
            .get("adaptive_release_qualification")
            .cloned()
            .ok_or_else(|| "adaptive release qualification is missing".to_string())?;
        let gate_app = app.clone();
        let gate_write = crate::runtime::run_io_bound("adaptive_release_gate_store", move || {
            adaptive_release_gate_store_sync(&gate_app, &gate_input, &gate_report, &qualification)
        })
        .await?;
        if let Err(error) = gate_write {
            crate::market::append_result_notes(
                &mut result,
                vec![format!("发布门槛报告写入失败：{error}")],
            );
        }
    }
    crate::market::append_result_notes(&mut result, notes);
    local_data::attach_metadata(&mut result, metadata, &transport);
    Ok(result)
}

async fn api_hk_backtest(app: &tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let policy = DataPolicy::parse(&payload)?;
    let path = crate::hk_market::hk_market_data_path(app)?;
    let cache = crate::hk_market::read_hk_market_cache(&path)?;
    let data = std::sync::Arc::new(crate::hk_research::project_hk_market_data(&cache)?);
    if data.stocks.is_empty() {
        return Err(local_data::missing_error(
            "hk_backtest",
            &[
                json!({"kind":"hk_quotes", "reason":"No HKD-listed equity quotes are cached yet. Refresh the HKEX directory to batch-load market quotes."}),
            ],
        ));
    }
    let request = hk_backtest_request(&payload, data.as_ref())?;
    let selected = gp_core::backtest_selected_symbols(&data.stocks, &request);
    if selected.is_empty() {
        return Err("当前港股条件没有选出可回测证券，请调整行业或选择港股自选股。".to_owned());
    }
    let prepared = hk_history::prepare(
        app,
        &payload,
        cache,
        selected,
        &request.start_date,
        &request.end_date,
        2,
    )
    .await?;
    hk_history::require_usable(&prepared, "hk_backtest", true)?;
    let hk_history::PreparedHistory {
        cache,
        data,
        summary,
        coverage,
        ..
    } = prepared;
    let calculation_data = data.clone();
    let mut result = crate::runtime::run_cpu_bound("hk_candidate_snapshot_backtest", move || {
        hk_candidate_backtest_value(&calculation_data, &request)
    })
    .await??;
    append_hk_history_note(&mut result, &summary);
    if let Some(object) = result.as_object_mut() {
        object.insert("market".into(), json!("hk"));
        object.insert(
            "backtest_quality".into(),
            json!({
                "label":"exploratory",
                "strategy_mode":"candidate_snapshot",
                "limitations":crate::hk_research::hk_exploratory_limitations(),
                "history_source":"Tencent Finance HK daily history",
                "network_history_fetch":summary["network_history_fetch"],
            }),
        );
    }
    local_data::attach_metadata(
        &mut result,
        {
            let mut metadata =
                hk_history_metadata(&cache, &data, &summary, &coverage, policy.as_str());
            metadata["strategy_mode"] = json!("candidate_snapshot");
            metadata["exploratory_limitations"] =
                json!(crate::hk_research::hk_exploratory_limitations());
            metadata
        },
        &payload,
    );
    Ok(result)
}

fn hk_candidate_backtest_value(
    data: &gp_core::CoreDataSet,
    request: &gp_core::BacktestRequest,
) -> Result<Value, String> {
    let result = gp_core::backtest_with_data(data, request).map_err(|error| error.to_string())?;
    serde_json::to_value(result).map_err(|error| error.to_string())
}

fn append_hk_history_note(result: &mut Value, summary: &Value) {
    crate::market::append_result_notes(
        result,
        vec![format!(
            "港股日线：可用 {}/{} 只，本轮补齐 {} 只，失败 {} 只。",
            summary["available_count"],
            summary["candidate_count"],
            summary["updated_count"],
            summary["failed_count"]
        ), format!("日线按完整区间验证复权价格：前复权 {} 只、后复权 {} 只，单只证券不混用价格口径。", summary["qfq_count"], summary["hfq_count"])],
    );
    if let Some(errors) = summary["errors"].as_array() {
        crate::market::append_result_notes(
            result,
            errors
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        );
    }
    if let Some(count) = summary["suspended_count"].as_u64().filter(|count| *count > 0) {
        crate::market::append_result_notes(result, vec![format!("{count} 只停牌证券使用最后成交日行情。")]);
    }
}

fn hk_history_metadata(
    cache: &crate::hk_market::HkMarketCache,
    data: &gp_core::CoreDataSet,
    summary: &Value,
    coverage: &[Value],
    requested_policy: &str,
) -> Value {
    let mut metadata = hk_research_metadata(cache, data, requested_policy);
    let fetched = summary["network_history_fetch"].as_bool() == Some(true);
    metadata["data_policy"] = json!(if fetched { "refresh" } else { "cache_only" });
    metadata["cache_only"] = json!(!fetched);
    metadata["history_hydration"] = summary.clone();
    metadata["coverage"] = json!(coverage.iter().take(20).collect::<Vec<_>>());
    metadata["coverage_truncated"] = json!(coverage.len() > 20);
    metadata
}

fn hk_backtest_request(
    payload: &Value,
    data: &gp_core::CoreDataSet,
) -> Result<gp_core::BacktestRequest, String> {
    let mut request = serde_json::from_value::<gp_core::BacktestRequest>(
        crate::market::strip_core_side_payload_fields(payload.clone()),
    )
    .map_err(|error| format!("invalid HK backtest request: {error}"))?;
    request.strategy_mode = "candidate_snapshot".to_owned();
    if !matches!(
        request.benchmark.as_str(),
        "none" | "candidate_equal_weight"
    ) {
        request.benchmark = "none".to_owned();
    }
    request.criteria.include_st = true;
    request.criteria.market_scope = None;
    request.criteria.min_deducted_net_profit_billion = None;
    request.criteria.min_deducted_net_profit_margin = None;
    request.criteria.min_deducted_net_profit_growth_rate = None;
    for code in &mut request.stock_codes {
        let canonical = crate::hk_market::canonical_hk_code(code)
            .ok_or_else(|| "港股回测仅支持有效港股代码。".to_owned())?;
        *code = format!("{canonical}.HK");
    }
    let available = data
        .stocks
        .iter()
        .map(|stock| stock.code.as_str())
        .collect::<HashSet<_>>();
    if request
        .stock_codes
        .iter()
        .any(|code| !available.contains(code.as_str()))
    {
        return Err("HK backtest stock_codes must identify cached HKEX-listed equities".to_owned());
    }
    gp_core::validate_backtest_date_range(&request)
        .map_err(|error| format!("invalid HK backtest date range: {error}"))?;
    Ok(request)
}

pub(crate) fn adaptive_backtest_max_industry_count(
    result: &gp_core::BacktestResult,
    data: &gp_core::CoreDataSet,
) -> Option<usize> {
    let counts = result
        .walk_forward_folds
        .iter()
        .filter(|fold| !fold.selected_symbols.is_empty())
        .map(|fold| adaptive_backtest_fold_max_industry_count(fold, data))
        .collect::<Option<Vec<_>>>()?;
    counts.into_iter().max()
}

pub(crate) fn adaptive_backtest_fold_max_industry_count(
    fold: &gp_core::WalkForwardFold,
    data: &gp_core::CoreDataSet,
) -> Option<usize> {
    let as_of = crate::market::compact_date_key(
        fold.signal_date
            .as_deref()
            .unwrap_or(fold.selection_date.as_str()),
    )?;
    let mut counts = HashMap::<String, usize>::new();
    for code in &fold.selected_symbols {
        let normalized =
            crate::market::normalize_stock_code(code).unwrap_or_else(|| code.to_ascii_uppercase());
        let industry = data
            .factor_snapshots
            .get(code)
            .or_else(|| data.factor_snapshots.get(&normalized))
            .into_iter()
            .flatten()
            .filter_map(|snapshot| {
                let available = snapshot
                    .available_date
                    .as_deref()
                    .and_then(crate::market::compact_date_key)?;
                let industry = snapshot
                    .industry
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())?;
                (available <= as_of).then(|| (available, industry.to_string()))
            })
            .max_by(|left, right| left.0.cmp(&right.0))
            .map(|(_, industry)| industry)?;
        *counts.entry(industry).or_default() += 1;
    }
    counts.values().copied().max()
}

pub(crate) fn adaptive_backtest_average_jaccard(result: &gp_core::BacktestResult) -> Option<f64> {
    let values = result
        .walk_forward_folds
        .windows(2)
        .filter_map(|pair| {
            let left = pair[0]
                .selected_symbols
                .iter()
                .map(|code| code.to_ascii_uppercase())
                .collect::<HashSet<_>>();
            let right = pair[1]
                .selected_symbols
                .iter()
                .map(|code| code.to_ascii_uppercase())
                .collect::<HashSet<_>>();
            let union = left.union(&right).count();
            (union > 0).then_some(left.intersection(&right).count() as f64 / union as f64)
        })
        .collect::<Vec<_>>();
    (!values.is_empty()).then_some(values.iter().sum::<f64>() / values.len() as f64)
}

pub(crate) fn adaptive_release_implementation_fingerprint() -> &'static str {
    static FINGERPRINT: OnceLock<String> = OnceLock::new();
    FINGERPRINT
        .get_or_init(|| {
            gp_core::source_text_fingerprint(
                b"adaptive-release-contract-v4-lf\0",
                &[
                    (
                        "core_fingerprint",
                        gp_core::scoring_implementation_fingerprint().as_bytes(),
                    ),
                    (
                        "research_fingerprint",
                        gp_research::research_implementation_fingerprint().as_bytes(),
                    ),
                    (
                        "adaptive_screen.rs",
                        include_bytes!("../../../native/gp-core/src/adaptive_screen.rs").as_slice(),
                    ),
                    (
                        "core/lib.rs",
                        include_bytes!("../../../native/gp-core/src/lib.rs").as_slice(),
                    ),
                    ("lib.rs", include_bytes!("lib.rs").as_slice()),
                    ("screening.rs", include_bytes!("screening.rs").as_slice()),
                    (
                        "screening_financial_overlay.rs",
                        include_bytes!("screening_financial_overlay.rs").as_slice(),
                    ),
                    (
                        "metric_units.rs",
                        include_bytes!("metric_units.rs").as_slice(),
                    ),
                    (
                        "stock_research_runtime.rs",
                        include_bytes!("stock_research_runtime.rs").as_slice(),
                    ),
                    ("Cargo.toml", include_bytes!("../Cargo.toml").as_slice()),
                    ("Cargo.lock", include_bytes!("../Cargo.lock").as_slice()),
                ],
            )
        })
        .as_str()
}

pub(crate) fn adaptive_release_criteria_is_full_universe(
    criteria: &gp_core::ScreenCriteria,
) -> bool {
    criteria.min_roe.is_none()
        && criteria.max_pe.is_none()
        && criteria.max_pb.is_none()
        && criteria.min_market_cap_billion.is_none()
        && criteria.min_deducted_net_profit_billion.is_none()
        && criteria.min_deducted_net_profit_margin.is_none()
        && criteria.min_deducted_net_profit_growth_rate.is_none()
        && criteria
            .industry
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        && criteria
            .market_scope
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        && !criteria.include_st
}

pub(crate) fn adaptive_release_screen_request_qualified(
    request: &gp_core::AdaptiveScreenRequest,
) -> bool {
    request.mode.trim().eq_ignore_ascii_case("auto")
        && request.horizon.trim().eq_ignore_ascii_case("swing_10_30d")
        && request.primary_limit == 10
        && request.exploration_limit == 10
        && adaptive_release_criteria_is_full_universe(&request.criteria)
}

pub(crate) fn adaptive_release_requested_mode(strategy_mode: &str) -> String {
    let normalized = strategy_mode.trim().to_ascii_lowercase();
    normalized
        .split_once(':')
        .map(|(_, mode)| mode)
        .filter(|mode| matches!(*mode, "auto" | "range" | "trend" | "defensive"))
        .unwrap_or("auto")
        .to_string()
}

pub(crate) fn adaptive_release_backtest_qualification(
    request: &gp_core::BacktestRequest,
    oos_fold_count: usize,
) -> Value {
    let mode = adaptive_release_requested_mode(&request.strategy_mode);
    let full_universe = request.source.trim().eq_ignore_ascii_case("criteria")
        && request.stock_codes.is_empty()
        && adaptive_release_criteria_is_full_universe(&request.criteria);
    let qualified = mode == "auto"
        && full_universe
        && request.top_n == 10
        && request
            .rebalance_frequency
            .trim()
            .eq_ignore_ascii_case("monthly")
        && (request.transaction_cost_bps - 10.0).abs() <= f64::EPSILON
        && request
            .benchmark
            .trim()
            .eq_ignore_ascii_case("candidate_equal_weight")
        && oos_fold_count >= ADAPTIVE_RELEASE_MIN_OOS_FOLDS;
    json!({
        "implementation_fingerprint": adaptive_release_implementation_fingerprint(),
        "qualified": qualified,
        "mode": mode,
        "full_universe": full_universe,
        "source": request.source.as_str(),
        "top_n": request.top_n,
        "start_date": request.start_date.as_str(),
        "end_date": request.end_date.as_str(),
        "rebalance_frequency": request.rebalance_frequency.as_str(),
        "transaction_cost_bps": request.transaction_cost_bps,
        "benchmark": request.benchmark.as_str(),
        "oos_fold_count": oos_fold_count,
    })
}

#[tauri::command]
pub(crate) fn api_strategies() -> Result<Value, String> {
    Ok(json!({
        "strategies": [
            {
                "id": "quality_value",
                "name": "质量价值",
                "description": "低估值且净资产收益率为正。"
            },
            {
                "id": "defensive_dividend",
                "name": "防御分红",
                "description": "盈利稳定、估值适中、分红较好。"
            }
        ]
    }))
}

pub(crate) fn adaptive_release_validation_force_cold_start(payload: &Value) -> bool {
    payload
        .get(stringify!(internal_release_validation_cold_start))
        .and_then(Value::as_bool)
        == Some(true)
}

pub(crate) fn adaptive_release_force_cold_start_code(
    missing: &mut Vec<String>,
    required_codes: &[String],
) {
    if let Some(code) = required_codes.first() {
        if !missing.contains(code) {
            missing.push(code.clone());
        }
    }
}

pub(crate) fn adaptive_history_progress_percent(completed: usize, total: usize) -> usize {
    if total == 0 {
        return 72;
    }
    let completed = completed.min(total);
    24 + ((completed * 48 + total - 1) / total).min(48)
}

pub(crate) async fn collect_adaptive_history_results<F, Fut, P>(
    fetch_codes: Vec<String>,
    history_start_date: String,
    timeout: Duration,
    concurrency: usize,
    fetcher: F,
    mut on_progress: P,
) -> AdaptiveHistoryFetchOutcome
where
    F: Fn(&str, &str) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<Vec<Value>, String>> + Send + 'static,
    P: FnMut(usize, usize) + Send,
{
    let total = fetch_codes.len();
    if total == 0 {
        return AdaptiveHistoryFetchOutcome {
            results: Vec::new(),
            timed_out: false,
        };
    }

    let fetcher = Arc::new(fetcher);
    let make_fetch = {
        let fetcher = Arc::clone(&fetcher);
        let history_start_date = history_start_date.clone();
        move |code: String| {
            let fetcher = Arc::clone(&fetcher);
            let history_start_date = history_start_date.clone();
            async move {
                let result = fetcher(&code, &history_start_date).await;
                (code, result)
            }
        }
    };
    let mut codes = fetch_codes.into_iter();
    let mut pending = FuturesUnordered::new();
    let concurrency = concurrency.max(1);
    for _ in 0..concurrency {
        if let Some(code) = codes.next() {
            pending.push(make_fetch(code));
        }
    }

    let deadline = Instant::now() + timeout;
    let mut results = Vec::with_capacity(total);
    let mut completed = 0usize;
    let mut timed_out = false;
    while completed < total {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            timed_out = true;
            break;
        }
        match tokio::time::timeout(remaining, pending.next()).await {
            Ok(Some(result)) => {
                completed += 1;
                on_progress(completed, total);
                results.push(result);
                if let Some(code) = codes.next() {
                    pending.push(make_fetch(code));
                }
            }
            Ok(None) => break,
            Err(_) => {
                timed_out = true;
                break;
            }
        }
    }

    AdaptiveHistoryFetchOutcome { results, timed_out }
}

pub(crate) async fn prepare_adaptive_screen(
    app: &tauri::AppHandle,
    payload: Value,
) -> Result<PreparedAdaptiveScreen, String> {
    let diagnostics_enabled = crate::perf_diagnostics::requested(&payload);
    let cache_prepare_started = Instant::now();
    let force_cold_start = adaptive_release_validation_force_cold_start(&payload);
    let policy = DataPolicy::parse(&payload)?;
    let data = local_snapshot(app, &payload, "adaptive_screen")?;
    let stock_override = screen_stock_override(app, &data, &payload)?;
    let mut request = adaptive_screen_request_from_payload(payload)?;
    let cache_prepare_elapsed = cache_prepare_started.elapsed();
    let cache_counts = adaptive_perf_counts(
        data.as_ref(),
        stock_override.as_deref().map(Vec::as_slice),
        None,
    );
    crate::perf_diagnostics::record(
        diagnostics_enabled,
        crate::perf_diagnostics::PerfEvent::new(
            crate::perf_diagnostics::Flow::Screening,
            crate::perf_diagnostics::Stage::CacheSnapshotPrepare,
            cache_prepare_elapsed,
        )
        .with_cache_strategy(match policy {
            DataPolicy::CacheOnly => crate::perf_diagnostics::CacheStrategy::CacheOnly,
            DataPolicy::Refresh => crate::perf_diagnostics::CacheStrategy::Refresh,
        })
        .with_counts(
            cache_counts.stock_count,
            cache_counts.history_rows,
            cache_counts.financial_records,
        )
        .with_outcome(crate::perf_diagnostics::Outcome::Ok),
    );
    emit_adaptive_screen_progress(
        app,
        request.run_id.as_deref(),
        "candidate_scan",
        8,
        "初选候选池",
    );
    let candidate_data = Arc::clone(&data);
    let candidate_stocks = stock_override.clone();
    let criteria = request.criteria.clone();
    let candidate_started = Instant::now();
    let candidates = crate::runtime::run_cpu_bound("adaptive_screen_candidates", move || {
        let universe = candidate_stocks
            .as_deref()
            .map(Vec::as_slice)
            .unwrap_or(candidate_data.stocks.as_slice());
        gp_core::adaptive_candidate_codes(
            universe,
            &criteria,
            ADAPTIVE_SCREEN_HISTORY_PREFETCH_LIMIT,
        )
    })
    .await?;
    let candidate_elapsed = candidate_started.elapsed();
    let candidate_counts = adaptive_perf_counts(
        data.as_ref(),
        stock_override.as_deref().map(Vec::as_slice),
        None,
    );
    crate::perf_diagnostics::record(
        diagnostics_enabled,
        crate::perf_diagnostics::PerfEvent::new(
            crate::perf_diagnostics::Flow::Screening,
            crate::perf_diagnostics::Stage::CandidateGeneration,
            candidate_elapsed,
        )
        .with_cache_strategy(match policy {
            DataPolicy::CacheOnly => crate::perf_diagnostics::CacheStrategy::CacheOnly,
            DataPolicy::Refresh => crate::perf_diagnostics::CacheStrategy::Refresh,
        })
        .with_counts(
            candidate_counts.stock_count,
            candidate_counts.history_rows,
            candidate_counts.financial_records,
        )
        .with_candidate_count(candidates.len())
        .with_outcome(crate::perf_diagnostics::Outcome::Ok),
    );
    emit_adaptive_screen_progress(
        app,
        request.run_id.as_deref(),
        "history_fetch",
        24,
        "补齐120日行情",
    );

    let required_codes = adaptive_required_history_codes(&candidates);
    let target_history_date = request
        .as_of_date
        .as_deref()
        .and_then(crate::market::compact_date_key)
        .or_else(|| {
            let universe = stock_override
                .as_deref()
                .map(Vec::as_slice)
                .unwrap_or(data.stocks.as_slice());
            adaptive_quote_target_date(universe, &candidates)
        })
        .or_else(|| {
            crate::market::expected_market_quote_date_from_epoch_ms(crate::market::epoch_millis())
        });
    // Age alone is not a miss. Choose a common cached PIT date unless explicitly requested.
    if policy == DataPolicy::CacheOnly {
        let as_of = adaptive_screen_as_of_date(
            request.as_of_date.clone(),
            target_history_date.clone(),
            latest_adaptive_data_date(data.as_ref(), &HashMap::new(), &required_codes),
        );
        let coverage = adaptive_local_coverage(
            data.as_ref(),
            &HashMap::new(),
            &required_codes,
            as_of.as_deref(),
        );
        adaptive_screen_candidate_gaps(&coverage)?;
        request.as_of_date = as_of;
    }
    let mut missing = if policy == DataPolicy::CacheOnly {
        Vec::new()
    } else {
        adaptive_missing_history_codes(
            data.as_ref(),
            &required_codes,
            target_history_date.as_deref(),
        )
    };
    if force_cold_start {
        adaptive_release_force_cold_start_code(&mut missing, &required_codes);
    }
    let cache_hit = missing.is_empty();

    let history_supplement_started = Instant::now();
    let mut history_override = HashMap::new();
    let mut notes = Vec::new();
    let mut history_prefetch_timed_out = false;
    if missing.is_empty() {
        notes.push(format!(
            "自适应选股复用本地日线缓存：候选 {} 只、宽基指数 3 个。",
            candidates.len()
        ));
    } else {
        let fetch_codes = missing.clone();
        let history_start_date = adaptive_history_start_date();
        let progress_app = app.clone();
        let progress_run_id = request.run_id.clone();
        let fetch_outcome = crate::runtime::with_heavy_network_permit(
            "adaptive_screen_history_fetch",
            async move {
                let outcome = collect_adaptive_history_results(
                    fetch_codes,
                    history_start_date,
                    Duration::from_secs(ADAPTIVE_SCREEN_HISTORY_PREFETCH_TIMEOUT_SECS),
                    ADAPTIVE_SCREEN_HISTORY_CONCURRENCY,
                    |code, start_date| {
                        let code = code.to_string();
                        let start_date = start_date.to_string();
                        async move {
                            crate::market::fetch_observe_daily_history(
                                &code,
                                &start_date,
                                "20501231",
                            )
                            .await
                        }
                    },
                    move |completed, total| {
                        let message = format!("补齐120日行情 {completed}/{total}");
                        emit_adaptive_screen_progress(
                            &progress_app,
                            progress_run_id.as_deref(),
                            "history_fetch",
                            adaptive_history_progress_percent(completed, total),
                            &message,
                        );
                    },
                )
                .await;
                Ok(outcome)
            },
        )
        .await?;
        let AdaptiveHistoryFetchOutcome {
            results: fetches,
            timed_out,
        } = fetch_outcome;
        history_prefetch_timed_out = timed_out;
        if timed_out {
            notes.push(format!(
                "历史行情预取达到 {} 秒预算，使用已完成的行情继续计算。",
                ADAPTIVE_SCREEN_HISTORY_PREFETCH_TIMEOUT_SECS
            ));
        }
        let mut history_patch = serde_json::Map::new();
        let mut failed = 0usize;
        for (code, result) in fetches {
            match result {
                Ok(rows) if !rows.is_empty() => {
                    let rows = rows
                        .into_iter()
                        .rev()
                        .take(120)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect::<Vec<_>>();
                    let typed = serde_json::from_value::<Vec<gp_core::HistoryBar>>(Value::Array(
                        rows.clone(),
                    ))
                    .map_err(|error| {
                        format!("adaptive screen history parse failed for {code}: {error}")
                    })?;
                    history_patch.insert(code.clone(), Value::Array(rows));
                    history_override.insert(code, typed);
                }
                _ => failed += 1,
            }
        }
        if !history_patch.is_empty() {
            if let Err(error) = crate::market::persist_market_data_patch_updates(
                app.clone(),
                json!({ "histories": history_patch }),
            )
            .await
            {
                notes.push(format!("行情已获取但写入本地缓存失败：{error}"));
            }
        }
        notes.push(format!(
            "自适应选股日线预取：需要 {} 个标的，补取成功 {} 个，失败 {} 个。",
            required_codes.len(),
            history_override.len(),
            failed
        ));
    }
    let history_supplement_elapsed = history_supplement_started.elapsed();
    let history_counts = adaptive_perf_counts(
        data.as_ref(),
        stock_override.as_deref().map(Vec::as_slice),
        Some(&history_override),
    );
    crate::perf_diagnostics::record(
        diagnostics_enabled,
        crate::perf_diagnostics::PerfEvent::new(
            crate::perf_diagnostics::Flow::Screening,
            crate::perf_diagnostics::Stage::HistorySupplement,
            history_supplement_elapsed,
        )
        .with_cache_strategy(match policy {
            DataPolicy::CacheOnly => crate::perf_diagnostics::CacheStrategy::CacheOnly,
            DataPolicy::Refresh => crate::perf_diagnostics::CacheStrategy::Refresh,
        })
        .with_counts(
            history_counts.stock_count,
            history_counts.history_rows,
            history_counts.financial_records,
        )
        .with_candidate_count(candidates.len())
        .with_outcome(if cache_hit {
            crate::perf_diagnostics::Outcome::CacheHit
        } else if history_prefetch_timed_out || history_override.len() < missing.len() {
            crate::perf_diagnostics::Outcome::Partial
        } else {
            crate::perf_diagnostics::Outcome::Ok
        }),
    );
    if policy == DataPolicy::Refresh {
        request.as_of_date = adaptive_screen_as_of_date(
            request.as_of_date.clone(),
            target_history_date,
            latest_adaptive_data_date(data.as_ref(), &history_override, &required_codes),
        );
    }
    let coverage = adaptive_local_coverage(
        data.as_ref(),
        &history_override,
        &required_codes,
        request.as_of_date.as_deref(),
    );
    let excluded = adaptive_screen_candidate_gaps(&coverage)?;
    if !excluded.is_empty() {
        let details=excluded.iter().take(5).map(|item|format!("{}（日线 {}/{}）",
            item.get("code").and_then(Value::as_str).unwrap_or("未知证券"),
            item.get("available_bars").and_then(Value::as_u64).unwrap_or(0),
            item.get("required_bars").and_then(Value::as_u64).unwrap_or(MIN_ADAPTIVE_SCREEN_HISTORY_BARS as u64)))
            .collect::<Vec<_>>().join("、");
        notes.push(format!("本次未纳入 {} 只历史长度不足的候选：{details}。其余候选继续按原算法筛选。",excluded.len()));
    }
    let mut metadata = local_metadata(data.as_ref(), policy, &coverage);
    metadata["excluded_history"] = json!({"count":excluded.len(),"items":excluded});
    emit_adaptive_screen_progress(
        app,
        request.run_id.as_deref(),
        "history_fetch",
        72,
        if history_prefetch_timed_out {
            "历史行情预取已达时限，使用已完成数据继续"
        } else {
            "历史行情准备完成"
        },
    );

    let exposure_app = app.clone();
    let exposure_date = request.as_of_date.clone();
    let recent_exposure =
        crate::runtime::run_io_bound("adaptive_screen_exposure_read", move || {
            adaptive_exposure_recent_sync(&exposure_app, exposure_date.as_deref())
        })
        .await??;
    Ok(PreparedAdaptiveScreen {
        data,
        stock_override,
        candidate_codes: candidates,
        history_override,
        request,
        recent_exposure,
        notes,
        cache_hit,
        metadata,
    })
}

pub(crate) fn adaptive_history_start_date() -> String {
    const LOOKBACK_MILLIS: u128 = 220 * 24 * 60 * 60 * 1_000;
    crate::market::local_yyyymmdd_from_epoch_ms(
        crate::market::epoch_millis().saturating_sub(LOOKBACK_MILLIS),
    )
    .unwrap_or_else(|| "20200101".to_string())
}

pub(crate) fn adaptive_history_window(
    rows: &[gp_core::HistoryBar],
    as_of_date: Option<&str>,
) -> Vec<gp_core::HistoryBar> {
    let target = match as_of_date {
        Some(value) => match crate::market::compact_date_key(value) {
            Some(target) => Some(target),
            None => return Vec::new(),
        },
        None => None,
    };
    let window = rows
        .iter()
        .filter(|bar| {
            target.as_ref().is_none_or(|target| {
                crate::market::compact_date_key(&bar.date)
                    .is_some_and(|date| date.as_str() <= target.as_str())
            })
        })
        .rev()
        .take(120)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>();
    if target.is_some()
        && window
            .last()
            .and_then(|bar| crate::market::compact_date_key(&bar.date))
            != target
    {
        return Vec::new();
    }
    window
}

pub(crate) fn adaptive_point_in_time_universe(
    universe: &[gp_core::StockItem],
    histories: &HashMap<String, Vec<gp_core::HistoryBar>>,
    as_of_date: Option<&str>,
) -> Vec<gp_core::StockItem> {
    let target = as_of_date.and_then(crate::market::compact_date_key);
    universe
        .iter()
        .map(|stock| {
            let mut point_in_time = stock.clone();
            let quote_matches_target = target.as_ref().is_some_and(|target| {
                stock
                    .quote_time
                    .as_deref()
                    .and_then(crate::market::compact_date_key)
                    .is_some_and(|quote_date| quote_date == *target)
            });
            let bars = histories.get(&stock.code);
            if let Some(latest) = bars.and_then(|rows| rows.last()) {
                point_in_time.price = latest.close;
                point_in_time.volume = latest.volume;
                point_in_time.change_pct = bars.and_then(|rows| {
                    let previous = rows.get(rows.len().checked_sub(2)?)?.close;
                    (previous.is_finite() && previous > 0.0 && latest.close.is_finite())
                        .then_some(latest.close / previous - 1.0)
                });
            } else if target.is_some() && !quote_matches_target {
                point_in_time.price = 0.0;
                point_in_time.change_pct = None;
                point_in_time.volume = None;
            }
            if !quote_matches_target {
                // These quote fields cannot be reconstructed reliably from OHLCV. Keeping a
                // newer snapshot would leak future liquidity and overheat information.
                point_in_time.amount = None;
                point_in_time.turnover_rate = None;
                point_in_time.volume_ratio = None;
            }
            point_in_time.quote_time = target.clone();
            point_in_time
        })
        .collect()
}

pub(crate) fn latest_adaptive_data_date(
    data: &gp_core::CoreDataSet,
    history_override: &HashMap<String, Vec<gp_core::HistoryBar>>,
    required_codes: &[String],
) -> Option<String> {
    required_codes
        .iter()
        .filter_map(|code| {
            history_override
                .get(code)
                .or_else(|| data.histories.get(code))
                .and_then(|bars| {
                    bars.iter()
                        .filter_map(|bar| crate::market::compact_date_key(&bar.date))
                        .max()
                })
        })
        .min()
}

pub(crate) fn adaptive_screen_as_of_date(
    explicit: Option<String>,
    quote_target: Option<String>,
    history_latest: Option<String>,
) -> Option<String> {
    explicit.or(quote_target).or(history_latest)
}

pub(crate) fn adaptive_quote_target_date(
    universe: &[gp_core::StockItem],
    candidate_codes: &[String],
) -> Option<String> {
    let candidates = candidate_codes
        .iter()
        .map(|code| code.to_ascii_uppercase())
        .collect::<HashSet<_>>();
    universe
        .iter()
        .filter(|stock| candidates.contains(&stock.code.to_ascii_uppercase()))
        .filter_map(|stock| stock.quote_time.as_deref())
        .filter_map(crate::market::compact_date_key)
        .max()
}

pub(crate) fn adaptive_history_cache_is_usable(
    data: &gp_core::CoreDataSet,
    code: &str,
    target_date: Option<&str>,
    min_bars: usize,
) -> bool {
    if !crate::market::typed_history_cache_has_bars(data, code, "20200101", "20501231", min_bars) {
        return false;
    }
    let normalized = crate::market::normalize_stock_code(code).unwrap_or_else(|| code.to_string());
    let latest = data
        .histories
        .get(code)
        .or_else(|| data.histories.get(&normalized))
        .into_iter()
        .flatten()
        .filter_map(|bar| crate::market::compact_date_key(&bar.date))
        .max();
    match (latest, target_date) {
        (Some(latest), Some(target)) => latest.as_str() >= target,
        (Some(_), None) => true,
        _ => false,
    }
}

pub(crate) async fn prepare_trend_screen(
    app: &tauri::AppHandle,
    payload: Value,
) -> Result<PreparedTrendScreen, String> {
    let policy = DataPolicy::parse(&payload)?;
    let data = local_snapshot(app, &payload, "trend_screen")?;
    let stock_override = screen_stock_override(app, &data, &payload)?;
    let request =
        serde_json::from_value::<gp_core::TrendScreenRequest>(local_data::core_payload(payload))
            .map_err(|error| format!("invalid trend screen request: {error}"))?;
    let candidate_request = request.clone();
    let candidate_data = Arc::clone(&data);
    let candidate_stocks = stock_override.clone();
    let candidate_codes = crate::runtime::run_cpu_bound("api_trend_screen_candidates", move || {
        let universe = candidate_stocks
            .as_deref()
            .map(Vec::as_slice)
            .unwrap_or(candidate_data.stocks.as_slice());
        gp_core::trend_screen_candidate_codes(universe, &candidate_request)
    })
    .await?;
    let candidates = normalize_trend_prefetch_codes(candidate_codes.iter().map(String::as_str));
    let coverage = local_trend_history_coverage(
        data.as_ref(),
        &HashMap::new(),
        stock_override
            .as_deref()
            .map(Vec::as_slice)
            .unwrap_or(&data.stocks),
        &candidates,
        &request.start_date,
        &request.end_date,
        MIN_TREND_SCREEN_HISTORY_BARS,
    );
    let missing = coverage
        .iter()
        .filter(|item| item["sufficient"].as_bool() != Some(true))
        .filter_map(|item| item["code"].as_str().map(str::to_owned))
        .take(TREND_SCREEN_HISTORY_PREFETCH_LIMIT)
        .collect::<Vec<_>>();
    if policy == DataPolicy::CacheOnly {
        local_data::require_coverage("trend_screen", &coverage)?;
    }
    let metadata = local_metadata(data.as_ref(), policy, &coverage);
    let mut notes = Vec::new();
    if policy == DataPolicy::CacheOnly || missing.is_empty() {
        return Ok(PreparedTrendScreen {
            data,
            stock_override,
            metadata,
            history_override: HashMap::new(),
            request,
            notes,
        });
    }

    let start_date = request.start_date.clone();
    let end_date = request.end_date.clone();
    let fetch_missing = missing.clone();
    let fetch_outcome =
        crate::runtime::with_heavy_network_permit("api_trend_screen_history_fetch", async move {
            Ok(collect_adaptive_history_results(
                fetch_missing,
                start_date,
                Duration::from_secs(TREND_SCREEN_HISTORY_FETCH_TIMEOUT_SECS),
                TREND_SCREEN_HISTORY_CONCURRENCY,
                move |code, start_date| {
                    let code = code.to_string();
                    let start_date = start_date.to_string();
                    let end_date = end_date.clone();
                    async move {
                        crate::market::fetch_observe_daily_history(&code, &start_date, &end_date)
                            .await
                    }
                },
                |_, _| {},
            )
            .await)
        })
        .await?;
    let AdaptiveHistoryFetchOutcome {
        results: fetches,
        timed_out,
    } = fetch_outcome;
    let completed = fetches.len();

    let mut history_override = HashMap::new();
    let mut history_patch = serde_json::Map::new();
    let mut fetched = 0usize;
    let mut failed = missing.len().saturating_sub(completed);
    for (code, result) in fetches {
        match result {
            Ok(rows) if !rows.is_empty() => {
                let typed =
                    serde_json::from_value::<Vec<gp_core::HistoryBar>>(Value::Array(rows.clone()))
                        .map_err(|error| {
                            format!("trend history parse failed for {code}: {error}")
                        })?;
                history_patch.insert(code.clone(), Value::Array(rows));
                history_override.insert(code, typed);
                fetched += 1;
            }
            _ => failed += 1,
        }
    }
    if !history_patch.is_empty() {
        let patch = json!({ "histories": history_patch });
        if let Err(error) =
            crate::market::persist_market_data_patch_updates(app.clone(), patch).await
        {
            notes.push(format!(
                "Trend screen fetched OHLCV for {fetched} candidates, but cache patch write failed: {error}"
            ));
        }
    }
    let coverage = local_trend_history_coverage(
        data.as_ref(),
        &history_override,
        stock_override
            .as_deref()
            .map(Vec::as_slice)
            .unwrap_or(&data.stocks),
        &candidates,
        &request.start_date,
        &request.end_date,
        MIN_TREND_SCREEN_HISTORY_BARS,
    );
    if policy == DataPolicy::CacheOnly {
        local_data::require_coverage("trend_screen", &coverage)?;
    } else if !trend_screen_refresh_has_usable_history(&coverage) {
        local_data::require_coverage("trend_screen", &coverage)?;
    }
    let mut metadata = local_metadata(data.as_ref(), policy, &coverage);
    metadata["history_prefetch"] = json!({
        "requested": missing.len(), "completed": completed, "fetched": fetched, "failed": failed,
        "concurrency": TREND_SCREEN_HISTORY_CONCURRENCY, "timed_out": timed_out,
    });
    Ok(PreparedTrendScreen {
        data,
        stock_override,
        history_override,
        metadata,
        request,
        notes,
    })
}

pub(crate) async fn prepare_backtest(
    app: &tauri::AppHandle,
    payload: Value,
) -> Result<PreparedBacktest, String> {
    let policy = DataPolicy::parse(&payload)?;
    let data = local_snapshot(app, &payload, "backtest")?;
    let raw = crate::market::cached_market_data_views(app)?.data;
    // Backtests use the persisted market snapshot and their own history preparation path.
    // Screen-only financial snapshot overlays would make candidate selection depend on the
    // custom-screen transport instead of the backtest data set.
    let stock_override = None;
    let request =
        serde_json::from_value::<gp_core::BacktestRequest>(local_data::core_payload(payload))
            .map_err(|error| format!("invalid backtest request: {error}"))?;
    gp_core::validate_backtest_date_range(&request)
        .map_err(|error| format!("invalid backtest date range: {error}"))?;
    let adaptive_backtest = request
        .strategy_mode
        .trim()
        .to_ascii_lowercase()
        .starts_with("adaptive_swing_v1");

    if request
        .strategy_mode
        .trim()
        .eq_ignore_ascii_case("walk_forward")
    {
        // Keep the strict core's PIT/factor-snapshot validation before any history work.
        let requirement_data = Arc::clone(&data);
        let requirement_stocks = stock_override.clone();
        let requirement_request = request.clone();
        let codes =
            crate::runtime::run_cpu_bound("strict_backtest_local_requirements", move || {
                backtest_history_requirements(
                    requirement_data.as_ref(),
                    requirement_stocks.as_deref().map(Vec::as_slice),
                    &requirement_request,
                )
            })
            .await??;
        let coverage = cn_backtest_history::coverage(
            data.as_ref(),
            raw.as_ref(),
            &HashMap::new(),
            &serde_json::Map::new(),
            &codes,
            &request.start_date,
            &request.end_date,
            MIN_BACKTEST_HISTORY_BARS,
        );
        local_data::require_coverage("backtest", &coverage)?;
        let metadata = local_metadata(data.as_ref(), policy, &coverage);
        return Ok(PreparedBacktest {
            data,
            stock_override,
            metadata,
            history_override: HashMap::new(),
            request,
            notes: Vec::new(),
        });
    }

    let mut candidates = if adaptive_backtest {
        let candidate_data = Arc::clone(&data);
        let candidate_stocks = stock_override.clone();
        let candidate_request = request.clone();
        crate::runtime::run_cpu_bound("api_backtest_requirements", move || {
            backtest_history_requirements(
                candidate_data.as_ref(),
                candidate_stocks.as_deref().map(Vec::as_slice),
                &candidate_request,
            )
        })
        .await??
    } else {
        let candidate_data = Arc::clone(&data);
        let candidate_stocks = stock_override.clone();
        let candidate_request = request.clone();
        crate::runtime::run_cpu_bound("api_backtest_candidates", move || {
            let universe = candidate_stocks
                .as_deref()
                .map(Vec::as_slice)
                .unwrap_or(candidate_data.stocks.as_slice());
            backtest_history_prefetch_codes(universe, &candidate_request)
        })
        .await?
    };
    if adaptive_backtest {
        candidates.extend(
            crate::market::adaptive_benchmark_codes()
                .into_iter()
                .map(str::to_string),
        );
        crate::market::dedupe_stock_codes(&mut candidates);
    }
    let history_start_date = if adaptive_backtest {
        "19900101".to_string()
    } else {
        request.start_date.clone()
    };
    let minimum_bars = if adaptive_backtest {
        MIN_ADAPTIVE_SCREEN_HISTORY_BARS
    } else {
        MIN_BACKTEST_HISTORY_BARS
    };
    let coverage = cn_backtest_history::coverage(
        data.as_ref(),
        raw.as_ref(),
        &HashMap::new(),
        &serde_json::Map::new(),
        &candidates,
        &history_start_date,
        &request.end_date,
        minimum_bars,
    );
    let missing = coverage.iter().filter(|item| item["sufficient"] != true)
        .filter_map(|item| item["code"].as_str().map(str::to_owned)).collect::<Vec<_>>();
    if policy == DataPolicy::CacheOnly {
        local_data::require_coverage("backtest", &coverage)?;
    }
    let metadata = local_metadata(data.as_ref(), policy, &coverage);
    let mut notes = Vec::new();
    if policy == DataPolicy::CacheOnly || missing.is_empty() {
        notes.push(format!(
            "已复用 {} 只入选股票的本地日线缓存。",
            candidates.len()
        ));
        return Ok(PreparedBacktest {
            data,
            stock_override,
            metadata,
            history_override: HashMap::new(),
            request,
            notes,
        });
    }

    let start_date = history_start_date;
    let end_date = request.end_date.clone();
    let fetch_missing = missing.clone();
    let fetch_raw = Arc::clone(&raw);
    let known = cn_backtest_history::known_through(raw.as_ref(), crate::market::epoch_millis())?;
    let fetches =
        crate::runtime::with_heavy_network_permit("api_backtest_history_fetch", async move {
            let results = stream::iter(fetch_missing)
                .map(|code| {
                    let start_date = start_date.clone();
                    let end_date = end_date.clone();
                    let listed = cn_backtest_history::listing_date(fetch_raw.as_ref(), &code);
                    let known = known.clone();
                    async move {
                        let result = cn_backtest_history::fetch_range(
                            &code,
                            &start_date,
                            &end_date,
                            listed.as_deref(),
                            &known,
                        )
                        .await;
                        (code, result)
                    }
                })
                .buffer_unordered(BACKTEST_HISTORY_CONCURRENCY)
                .collect::<Vec<_>>()
                .await;
            Ok(results)
        })
        .await?;

    let mut history_override = HashMap::new();
    let mut history_patch = serde_json::Map::new();
    let mut range_proofs = serde_json::Map::new();
    let mut fetched = 0usize;
    let mut failed = 0usize;
    let mut failure_samples = Vec::new();
    for (code, result) in fetches {
        match result {
            Ok(history) if history.rows.len() >= minimum_bars => {
                history_patch.insert(code.clone(), serde_json::to_value(&history.rows).map_err(|error| error.to_string())?);
                range_proofs.insert(code.clone(), history.proof);
                history_override.insert(code, history.rows);
                fetched += 1;
            }
            Ok(history) => {
                failed += 1;
                if failure_samples.len() < 3 {
                    failure_samples.push(format!(
                        "{code}：所选区间返回 {} 根日线，至少需要 {minimum_bars} 根",
                        history.rows.len()
                    ));
                }
            }
            Err(error) => {
                failed += 1;
                if failure_samples.len() < 3 {
                    failure_samples.push(format!("{code}: {error}"));
                }
            }
        }
    }
    if fetched == 0 && !missing.is_empty() {
        return Err(format!(
            "无法获取回测所需的历史日线（{} 只）：{}",
            missing.len(),
            failure_samples.join("；")
        ));
    }
    if !history_patch.is_empty() {
        let patch = json!({ "histories": history_patch, "cn_reference": { "history_ranges": range_proofs } });
        if let Err(error) =
            crate::market::persist_market_data_patch_updates(app.clone(), patch).await
        {
            notes.push(format!(
                "Backtest fetched daily history for {fetched} stocks, but cache patch write failed: {error}"
            ));
        }
    }
    let fetched_bars = history_override.values().map(Vec::len).sum::<usize>();
    notes.push(format!(
        "回测数据准备：入选 {} 只，需联网补取 {} 只，成功 {fetched} 只（共 {fetched_bars} 根日线），失败 {failed} 只。",
        candidates.len(),
        missing.len()
    ));
    notes.push("回测日线已按所选区间验证，使用不复权价格；新股从上市日开始提供实际日线。".to_string());
    let coverage = cn_backtest_history::coverage(
        data.as_ref(),
        raw.as_ref(),
        &history_override,
        &range_proofs,
        &candidates,
        if adaptive_backtest {
            "19900101"
        } else {
            &request.start_date
        },
        &request.end_date,
        minimum_bars,
    );
    local_data::require_coverage("backtest", &coverage)?;
    let metadata = local_metadata(data.as_ref(), policy, &coverage);
    Ok(PreparedBacktest {
        data,
        stock_override,
        history_override,
        metadata,
        request,
        notes,
    })
}

pub(crate) fn backtest_history_prefetch_codes(
    universe: &[gp_core::StockItem],
    request: &gp_core::BacktestRequest,
) -> Vec<String> {
    gp_core::backtest_selected_symbols(universe, request)
}

pub(crate) fn backtest_history_requirements(
    data: &gp_core::CoreDataSet,
    stocks: Option<&[gp_core::StockItem]>,
    request: &gp_core::BacktestRequest,
) -> Result<Vec<String>, String> {
    let source = gp_core::StaticDataSource::with_overrides(data, stocks, None);
    gp_core::backtest_required_history_symbols(&source, request).map_err(|error| error.to_string())
}

pub(crate) fn backtest_history_rows_are_usable(rows: &[Value]) -> bool {
    let mut dates = HashSet::new();
    rows.iter()
        .filter_map(|row| {
            let close = crate::market::json_f64(row.get("close"))?;
            (close > 0.0)
                .then(|| row.get("date").and_then(Value::as_str))
                .flatten()
                .and_then(crate::market::compact_date_key)
        })
        .filter(|date| dates.insert(date.clone()))
        .take(MIN_BACKTEST_HISTORY_BARS)
        .count()
        >= MIN_BACKTEST_HISTORY_BARS
}

#[cfg(test)]
mod hk_market_route_tests {
    use super::{
        hk_backtest_request, hk_financial_refresh_needed, hk_research_metadata,
        hk_screen_candidate_codes, is_hk_research_request,
    };
    use serde_json::json;
    use stock_optimizer_core as gp_core;

    #[test]
    fn only_explicit_hk_market_requests_use_hk_research_route() {
        assert!(is_hk_research_request(&json!({"market":"hk"})));
        assert!(!is_hk_research_request(&json!({})));
        assert!(!is_hk_research_request(&json!({"market":"cn"})));
        assert!(!is_hk_research_request(&json!({"market":"HK"})));
    }

    #[test]
    fn hk_screen_financial_hydration_uses_displayed_candidates_in_rank_order_and_deduplicates() {
        let result = json!({"items":[
            {"stock":{"code":"00700.HK"},"score":20},
            {"stock":{"code":"00005.HK"},"score":19},
            {"stock":{"code":"700.HK"},"score":18}
        ]});

        assert_eq!(hk_screen_candidate_codes(&result, 2), ["00700", "00005"]);
    }

    #[test]
    fn hk_screen_financial_hydration_includes_every_visible_sector_row() {
        let rows = (1..=12).map(|index| json!({"stock":{"code":format!("{index:05}.HK")}})).collect::<Vec<_>>();
        let result = json!({"groups":[{"sector":"新能源与储能","items":rows},{"sector":"公用环保","items":[{"stock":{"code":"00001.HK"}}]}]});
        let codes = hk_screen_candidate_codes(&result, 1);
        assert_eq!(codes.len(), 12);
        assert_eq!(codes.last().map(String::as_str), Some("00012"));
    }

    #[test]
    fn hk_screen_financial_hydration_includes_visible_hot_and_potential_group_candidates() {
        let result = json!({
            "items":[{"stock":{"code":"09999.HK"},"score":30}],
            "groups":[
                {"key":"hot","items":[
                    {"stock":{"code":"00700.HK"},"score":20},
                    {"stock":{"code":"00311.HK"},"score":19}
                ]},
                {"key":"potential","items":[
                    {"stock":{"code":"01003.HK"},"score":18},
                    {"stock":{"code":"06826.HK"},"score":17},
                    {"stock":{"code":"00311.HK"},"score":16}
                ]}
            ]
        });

        assert_eq!(
            hk_screen_candidate_codes(&result, 10),
            ["00700", "00311", "01003", "06826"]
        );
    }

    #[test]
    fn hk_screen_financial_hydration_respects_a_cache_ttl() {
        let mut snapshot = crate::hk_market::parse_hk_financial_snapshot(
            include_str!("../testdata/eastmoney-hk-financial.json"),
            "00700.HK",
            "1000000",
        )
        .unwrap();
        snapshot.eps.value = None;
        snapshot.supplemental_source = Some("Sina Finance HK statements".to_owned());
        snapshot.supplemental_fetched_at = Some("1000000".to_owned());

        assert!(!hk_financial_refresh_needed(
            Some(&snapshot),
            1000000 + 60_000
        ));
        assert!(hk_financial_refresh_needed(
            Some(&snapshot),
            1000000 + 13 * 60 * 60 * 1000
        ));
        assert!(hk_financial_refresh_needed(None, 1000000));
    }

    #[test]
    fn hk_screen_retries_recent_incomplete_legacy_financial_cache_without_sina_attempt_metadata() {
        let mut snapshot = crate::hk_market::parse_hk_financial_snapshot(
            include_str!("../testdata/eastmoney-hk-financial.json"),
            "00700.HK",
            "1000000",
        )
        .unwrap();
        snapshot.eps.value = None;

        assert!(hk_financial_refresh_needed(
            Some(&snapshot),
            1000000 + 60_000
        ));

        snapshot.supplemental_source = Some("Sina Finance HK statements".to_owned());
        snapshot.supplemental_fetched_at = Some("1000000".to_owned());
        assert!(!hk_financial_refresh_needed(
            Some(&snapshot),
            1000000 + 60_000
        ));
    }

    #[test]
    fn hk_screen_checks_for_new_reports_after_the_complete_financial_cache_expires() {
        let snapshot = crate::hk_market::parse_hk_financial_snapshot(
            include_str!("../testdata/eastmoney-hk-financial.json"),
            "00700.HK",
            "1000000",
        )
        .unwrap();
        assert!(!hk_financial_refresh_needed(
            Some(&snapshot),
            1000000 + 60_000
        ));
        assert!(hk_financial_refresh_needed(
            Some(&snapshot),
            1000000 + 13 * 60 * 60 * 1000
        ));
    }

    #[test]
    fn hk_screen_checks_legacy_company_metadata_once_without_repeating_a_failed_profile_lookup() {
        let mut snapshot = crate::hk_market::parse_hk_financial_snapshot(
            include_str!("../testdata/eastmoney-hk-financial.json"),
            "00700.HK",
            "1000000",
        )
        .unwrap();
        assert!(super::hk_screen_candidate_refresh_needed(
            Some(&snapshot),
            1000000 + 60_000
        ));
        snapshot.field_errors.insert(
            "company_profile".to_owned(),
            "profile unavailable".to_owned(),
        );
        assert!(!super::hk_screen_candidate_refresh_needed(
            Some(&snapshot),
            1000000 + 60_000
        ));
        assert!(super::hk_screen_candidate_refresh_needed(
            Some(&snapshot),
            1000000 + 13 * 60 * 60 * 1000
        ));
    }

    #[tokio::test]
    async fn hk_cold_filter_prepares_roe_and_industry_before_final_matching() {
        for criteria in [
            gp_core::ScreenCriteria { min_roe: Some(0.15), max_pe: Some(20.0), ..Default::default() },
            gp_core::ScreenCriteria { industry: Some("互联网".to_owned()), max_pe: Some(20.0), ..Default::default() },
        ] {
            let mut cache = cold_filter_cache();
            cache.quotes.get_mut("00002").unwrap().pe = Some(100.0);
            let data = crate::hk_research::project_hk_market_data(&cache).unwrap();
            let completed = super::hydrate_hk_screen_candidates(cache, data, super::HkCandidateScreenRequest::Screen(&criteria), std::time::Duration::from_secs(30), hydrate_cold_filter_fixture).await.unwrap();
            assert_eq!(completed.result["total"], 1);
            assert_eq!(completed.result["items"][0]["stock"]["code"], "00001.HK");
            assert_eq!(completed.summary["attempted_count"], 1, "known quote rejection must stay excluded");
            assert_eq!(completed.summary["filter_coverage"]["unresolved_count"], 0);
        }
    }

    fn cold_filter_cache() -> crate::hk_market::HkMarketCache {
        let mut cache = crate::hk_market::HkMarketCache::default();
        for code in ["00001", "00002"] {
            cache.securities.push(serde_json::from_value(json!({"code":code,"name":"互联网样本","currency":"HKD","security_type":"Equity","classification_status":"exchange_listed_equity"})).unwrap());
            cache.quotes.insert(code.to_owned(), serde_json::from_value(json!({"code":code,"name":"互联网样本","currency":"HKD","price":10.0,"price_semantics_version":1,"pe":10.0,"pb":1.0,"source":"fixture"})).unwrap());
        }
        cache
    }

    #[tokio::test]
    async fn hk_cold_filter_uses_preparation_slots_after_skipping_fresh_missing_metrics() {
        let mut cache = cold_filter_cache();
        let now = crate::hk_market::current_epoch_millis();
        let mut fresh = crate::hk_market::parse_hk_financial_snapshot(
            include_str!("../testdata/eastmoney-hk-financial.json"), "00001", &now,
        ).unwrap();
        fresh.roe.value = None;
        fresh.supplemental_source = Some("Sina HK financial".to_owned());
        fresh.supplemental_fetched_at = Some(now.clone());
        fresh.field_errors.insert("company_profile".to_owned(), "already attempted".to_owned());
        assert!(!super::hk_screen_candidate_refresh_needed(Some(&fresh), now.parse().unwrap()));
        cache.financials.insert("00001".to_owned(), fresh);
        let data = crate::hk_research::project_hk_market_data(&cache).unwrap();
        let criteria = gp_core::ScreenCriteria { min_roe: Some(0.15), limit: 1, ..Default::default() };
        let mut relaxed = criteria.clone();
        relaxed.min_roe = None;
        assert_eq!(gp_core::screen_stocks(&data.stocks, &relaxed).items[0].stock.code, "00001.HK");
        let mut requested = Vec::new();
        let completed = super::hydrate_hk_screen_candidates(
            cache, data, super::HkCandidateScreenRequest::Screen(&criteria),
            std::time::Duration::from_secs(30), |cache, codes, deadline| {
                requested.extend(codes.iter().cloned());
                hydrate_cold_filter_fixture(cache, codes, deadline)
            },
        ).await.unwrap();
        assert_eq!(requested, ["00002"], "a recent incomplete stock cannot consume the cold candidate's preparation slot");
        assert_eq!(completed.result["total"], 1);
        assert_eq!(completed.result["items"][0]["stock"]["code"], "00002.HK");
        assert_eq!(completed.summary["filter_coverage"]["unresolved_count"], 1);
    }

    async fn hydrate_cold_filter_fixture(mut cache: crate::hk_market::HkMarketCache, codes: Vec<String>, _: tokio::time::Instant) -> Result<(crate::hk_market::HkMarketCache, serde_json::Value), String> {
        for code in &codes {
            let mut snapshot = crate::hk_market::parse_hk_financial_snapshot(include_str!("../testdata/eastmoney-hk-financial.json"), code, &crate::hk_market::current_epoch_millis()).unwrap();
            snapshot.roe.value = Some(20.0);
            snapshot.company_profile = Some(serde_json::from_value(json!({"code":code,"name":"互联网样本","name_zh":null,"industry":"互联网","business_summary":"fixture","source":"fixture","fetched_at":crate::hk_market::current_epoch_millis()})).unwrap());
            cache.financials.insert(code.clone(), snapshot);
        }
        Ok((cache, json!({"requested_count":codes.len(),"attempted_count":codes.len(),"updated_count":codes.len(),"errors":[]})))
    }

    #[tokio::test]
    async fn hk_cold_filter_prepares_grouped_conditions_before_group_size_checks() {
        let cache = cold_filter_cache();
        let data = crate::hk_research::project_hk_market_data(&cache).unwrap();
        let request = gp_core::SectorScreenRequest { criteria: gp_core::ScreenCriteria { min_roe:Some(0.15), industry:Some("互联网".to_owned()), ..Default::default() }, group_by:"board".to_owned(), min_sector_candidates:2, ..Default::default() };
        let completed = super::hydrate_hk_screen_candidates(cache, data, super::HkCandidateScreenRequest::Grouped(&request), std::time::Duration::from_secs(30), hydrate_cold_filter_fixture).await.unwrap();
        assert_eq!(completed.result["returned"], 2);
        assert_eq!(completed.summary["updated_count"], 2);
        assert_eq!(completed.summary["filter_coverage"]["complete"], true);
    }

    #[tokio::test]
    async fn hk_cold_filter_reports_failed_and_deferred_nonmatches_as_unresolved() {
        for deferred in [false, true] {
            let cache = cold_filter_cache();
            let data = crate::hk_research::project_hk_market_data(&cache).unwrap();
            let criteria = gp_core::ScreenCriteria { min_roe:Some(0.15), industry:Some("互联网".to_owned()), ..Default::default() };
            let budget = if deferred { std::time::Duration::ZERO } else { std::time::Duration::from_secs(30) };
            let completed = super::hydrate_hk_screen_candidates(cache, data, super::HkCandidateScreenRequest::Screen(&criteria), budget, |_, _, _| async { Err("fixture financial source unavailable".to_owned()) }).await.unwrap();
            assert_eq!(completed.result["total"], 0);
            assert_eq!(completed.summary["filter_coverage"]["complete"], false);
            assert_eq!(completed.summary["filter_coverage"]["unresolved_count"], 2);
            assert_eq!(completed.summary[if deferred { "deferred_count" } else { "failed_count" }], 2);
            assert!(completed.cache.financial_errors.contains_key("00001"));
            let mut result = completed.result;
            super::append_hk_financial_hydration_note(&mut result, &completed.summary);
            assert!(result["notes"].as_array().unwrap().iter().any(|note| note.as_str().unwrap_or_default().contains("筛选条件")));
        }
    }

    #[tokio::test]
    async fn hk_cold_filter_applies_original_criteria_after_preparation_and_keeps_unknown_fields() {
        let cache = cold_filter_cache();
        let data = crate::hk_research::project_hk_market_data(&cache).unwrap();
        let criteria = gp_core::ScreenCriteria { min_roe:Some(0.15), industry:Some("互联网".to_owned()), limit:1, ..Default::default() };
        let completed = super::hydrate_hk_screen_candidates(cache, data, super::HkCandidateScreenRequest::Screen(&criteria), std::time::Duration::from_secs(30), |cache, codes, deadline| async move {
            let (mut cache, summary) = hydrate_cold_filter_fixture(cache, codes, deadline).await?;
            if let Some(snapshot) = cache.financials.get_mut("00001") { snapshot.roe.value = Some(5.0); }
            if let Some(snapshot) = cache.financials.get_mut("00002") {
                snapshot.roe.value = None;
                snapshot.field_errors.insert("roe".to_owned(), "fixture issuer does not disclose ROE".to_owned());
                snapshot.supplemental_source = Some("fixture attempted".to_owned());
                snapshot.supplemental_fetched_at = Some(crate::hk_market::current_epoch_millis());
            }
            Ok((cache, summary))
        }).await.unwrap();
        assert_eq!(completed.result["total"], 0, "prepared low ROE and unresolved ROE must not pass full criteria");
        assert_eq!(completed.summary["updated_count"], 2);
        assert_eq!(completed.summary["filter_coverage"]["unresolved_count"], 1);
        assert_eq!(completed.summary["filter_coverage"]["unresolved_codes"], json!(["00002"]));
        assert!(completed.summary["filter_coverage"]["errors"].as_array().unwrap().iter().any(|error| error["code"] == "00002" && error["field"] == "roe" && error["reason"] == "fixture issuer does not disclose ROE"));
    }

    #[tokio::test]
    async fn hk_screen_financial_hydration_checks_final_candidates_after_more_than_three_rerankings(
    ) {
        let mut cache = crate::hk_market::HkMarketCache::default();
        for index in 0..100 {
            let code = format!("{:05}", 10000 + index);
            cache.securities.push(serde_json::from_value(json!({"code":code,"name":format!("芯片样本{index}"),"currency":"HKD","board_lot":100,"security_type":"Equity","classification_status":"exchange_listed_equity"})).unwrap());
            cache.quotes.insert(code.clone(), serde_json::from_value(json!({"code":code,"name":format!("芯片样本{index}"),"price":10.0,"price_semantics_version":1,"pe":10.0,"pb":1.0,"dividend_yield_percent":0.0,"market_cap_hkd":10000000000.0,"currency":"HKD","source":"fixture","change_pct":0.0})).unwrap());
        }
        let data = crate::hk_research::project_hk_market_data(&cache).unwrap();
        let completed = super::hydrate_hk_screen_candidates(cache, data, super::HkCandidateScreenRequest::Screen(&gp_core::ScreenCriteria::default()), std::time::Duration::from_secs(30), |mut cache, codes, _| async move {
            let count = codes.len();
            for code in codes {
                assert!(!cache.financials.contains_key(&code), "a code must not be retried in the same screen run");
                let mut snapshot = crate::hk_market::parse_hk_financial_snapshot(include_str!("../testdata/eastmoney-hk-financial.json"), &code, &crate::hk_market::current_epoch_millis()).unwrap();
                snapshot.roe.value = Some(-100.0);
                snapshot.eps.value = Some(-1.0);
                snapshot.net_profit.value = Some(-10000000.0);
                snapshot.field_errors.insert("company_profile".to_owned(), "profile not supplied by fixture".to_owned());
                cache.financials.insert(code, snapshot);
            }
            Ok((cache, json!({"requested_count":count,"attempted_count":count,"updated_count":count,"failed_count":0,"errors":[]})))
        }).await.unwrap();
        assert_eq!(completed.cache.financials.len(), 100);
        for code in hk_screen_candidate_codes(&completed.result, 10) {
            assert!(
                completed.cache.financials.contains_key(&code),
                "final candidate {code} was never hydrated"
            );
        }
        assert_eq!(completed.summary["updated_count"], 100);
    }

    #[tokio::test]
    async fn hk_grouped_screen_prepares_the_final_concept_and_board_rows_and_reuses_financials() {
        for group_by in ["concept", "board"] {
            let mut cache = crate::hk_market::HkMarketCache::default();
            for index in 1..=4 {
                let code = format!("{index:05}");
                cache.securities.push(serde_json::from_value(json!({"code":code,"name":format!("新能源样本{index}"),"currency":"HKD","board_lot":100,"security_type":"Equity","classification_status":"exchange_listed_equity"})).unwrap());
                cache.quotes.insert(code.clone(), serde_json::from_value(json!({"code":code,"name":format!("新能源样本{index}"),"price":10.0,"price_semantics_version":1,"pe":10.0,"pb":1.0,"market_cap_hkd":10000000000.0,"currency":"HKD","source":"fixture","change_pct":0.0})).unwrap());
            }
            let request = gp_core::SectorScreenRequest { max_sectors: 5, per_sector_limit: 3, min_sector_candidates: 1, group_by: group_by.to_string(), ..Default::default() };
            let data = crate::hk_research::project_hk_market_data(&cache).unwrap();
            let completed = super::hydrate_hk_screen_candidates(cache, data, super::HkCandidateScreenRequest::Grouped(&request), std::time::Duration::from_secs(30), |mut cache, codes, _| async move {
                let count = codes.len();
                for code in codes {
                    let mut snapshot = crate::hk_market::parse_hk_financial_snapshot(include_str!("../testdata/eastmoney-hk-financial.json"), &code, &crate::hk_market::current_epoch_millis()).unwrap();
                    snapshot.field_errors.insert("company_profile".to_string(), "fixture has no company profile".to_string());
                    cache.financials.insert(code, snapshot);
                }
                Ok((cache, json!({"requested_count":count,"attempted_count":count,"updated_count":count,"failed_count":0,"errors":[]})))
            }).await.unwrap();
            let mut result = completed.result;
            crate::hk_research::attach_cached_hk_financial_metrics(&mut result, &completed.cache);
            assert_eq!(result["returned"], 3);
            for row in result["groups"].as_array().unwrap().iter().flat_map(|group| group["items"].as_array().unwrap()) {
                assert_eq!(row["stock"]["hk_financial_snapshot"]["eps"]["value"], 12.639);
                assert!(row["stock"]["hk_financial_snapshot"]["revenue"]["value"].as_f64().is_some());
            }
            let repeated = super::hydrate_hk_screen_candidates(completed.cache, completed.data, super::HkCandidateScreenRequest::Grouped(&request), std::time::Duration::from_secs(30), |_, _, _| async { panic!("prepared grouped rows must reuse the cached financials") }).await.unwrap();
            assert_eq!(repeated.summary["attempted_count"], 0);
        }
    }

    #[tokio::test]
    async fn hk_grouped_screen_surfaces_client_and_cache_write_failures_on_unprepared_rows() {
        for error in ["无法创建财务数据客户端", "财务缓存写入失败"] {
            let mut cache = crate::hk_market::HkMarketCache::default();
            cache.securities.push(serde_json::from_value(json!({"code":"01798","name":"大唐新能源","currency":"HKD","board_lot":100,"security_type":"Equity","classification_status":"exchange_listed_equity"})).unwrap());
            cache.quotes.insert("01798".to_string(), serde_json::from_value(json!({"code":"01798","name":"大唐新能源","price":1.0,"price_semantics_version":1,"currency":"HKD","pe":4.0,"pb":0.3,"market_cap_hkd":10000000000.0,"source":"fixture"})).unwrap());
            let data = crate::hk_research::project_hk_market_data(&cache).unwrap();
            let request = gp_core::SectorScreenRequest { min_sector_candidates: 1, ..Default::default() };
            let completed = super::hydrate_hk_screen_candidates(cache, data, super::HkCandidateScreenRequest::Grouped(&request), std::time::Duration::from_secs(30), |cache, codes, _| async move {
                Ok((cache, json!({"requested_count":codes.len(),"attempted_count":0,"updated_count":0,"failed_count":codes.len(),"errors":[error]})))
            }).await.unwrap();
            let mut result = completed.result;
            crate::hk_research::attach_cached_hk_financial_metrics(&mut result, &completed.cache);
            assert_eq!(result["groups"][0]["items"][0]["stock"]["hk_financial_error"], error);
            assert!(result["groups"][0]["items"][0]["stock"].get("hk_financial_snapshot").is_none());
        }
    }

    #[tokio::test]
    async fn hk_grouped_screen_retains_a_failed_write_when_the_next_batch_writes_successfully() {
        let mut cache = crate::hk_market::HkMarketCache::default();
        for code in ["00001", "00002"] {
            cache.securities.push(serde_json::from_value(json!({"code":code,"name":"新能源样本","currency":"HKD","board_lot":100,"security_type":"Equity","classification_status":"exchange_listed_equity"})).unwrap());
            cache.quotes.insert(code.to_string(), serde_json::from_value(json!({"code":code,"name":"新能源样本","price":10.0,"price_semantics_version":1,"pe":10.0,"pb":1.0,"market_cap_hkd":10000000000.0,"currency":"HKD","source":"fixture"})).unwrap());
        }
        let durable = cache.clone();
        let data = crate::hk_research::project_hk_market_data(&cache).unwrap();
        let request = gp_core::SectorScreenRequest { group_by:"board".to_string(), per_sector_limit:1, min_sector_candidates:1, ..Default::default() };
        let mut batch = 0;
        let completed = super::hydrate_hk_screen_candidates(cache, data, super::HkCandidateScreenRequest::Grouped(&request), std::time::Duration::from_secs(30), |cache, codes, _| {
            batch += 1;
            let write_failed = batch == 1;
            let mut returned = if write_failed { cache } else { durable.clone() };
            async move {
                for code in &codes {
                    let mut snapshot = crate::hk_market::parse_hk_financial_snapshot(include_str!("../testdata/eastmoney-hk-financial.json"), code, &crate::hk_market::current_epoch_millis()).unwrap();
                    snapshot.eps.value = Some(-1.0);
                    snapshot.roe.value = Some(-100.0);
                    snapshot.net_profit.value = Some(-10000000.0);
                    returned.financials.insert(code.clone(), snapshot);
                    if write_failed { returned.financial_errors.insert(code.clone(), "财务缓存写入失败".to_string()); }
                }
                Ok((returned,json!({"requested_count":codes.len(),"attempted_count":codes.len(),"updated_count":codes.len(),"cache_write_failed_count":if write_failed {codes.len()}else{0},"errors":[]})))
            }
        }).await.unwrap();
        assert_eq!(batch, 2);
        assert_eq!(completed.cache.financials.get("00001").and_then(|snapshot| snapshot.eps.value), Some(-1.0));
        assert_eq!(completed.cache.financials.len(), 2);
        for code in hk_screen_candidate_codes(&completed.result, 1) { assert!(completed.cache.financials.contains_key(&code)); }
    }

    #[test]
    fn hk_prepared_financials_preserve_errors_for_the_same_snapshot_until_newer_data_arrives() {
        let mut previous = crate::hk_market::HkMarketCache::default();
        let snapshot = crate::hk_market::parse_hk_financial_snapshot(include_str!("../testdata/eastmoney-hk-financial.json"), "00700", "1794116685279").unwrap();
        previous.financials.insert("00700".to_string(), snapshot);
        let mut durable = previous.clone();
        previous.financial_errors.insert("00700".to_string(), "本轮财务源请求及缓存写入失败".to_string());
        super::preserve_hk_prepared_financials(&previous, &mut durable);
        assert_eq!(durable.financial_errors.get("00700").map(String::as_str), Some("本轮财务源请求及缓存写入失败"));
        durable.financial_errors.clear();
        durable.financials.get_mut("00700").unwrap().candidate_fetched_at = "1794116685280".to_string();
        super::preserve_hk_prepared_financials(&previous, &mut durable);
        assert!(!durable.financial_errors.contains_key("00700"));
    }

    #[tokio::test]
    async fn hk_financial_preload_keeps_completed_data_when_a_source_stalls() {
        let fetched = super::collect_hk_screen_financials(vec!["00700".to_string(), "01798".to_string()], tokio::time::Instant::now() + std::time::Duration::from_millis(20), |code| async move {
            if code == "01798" { std::future::pending::<()>().await; }
            crate::hk_market::parse_hk_financial_snapshot(include_str!("../testdata/eastmoney-hk-financial.json"), &code, &crate::hk_market::current_epoch_millis())
        }).await;
        assert_eq!(fetched.outcomes.len(), 1);
        assert_eq!(fetched.outcomes[0].0, "00700");
        assert_eq!(fetched.outcomes[0].1.as_ref().unwrap().eps.value, Some(12.639));
        assert_eq!(fetched.deferred_codes, vec!["01798".to_string()]);
        assert_eq!(fetched.attempted_count, 2);
    }

    #[tokio::test]
    async fn hk_financial_preload_uses_one_deadline_across_preparation_and_returns_partial_data() {
        let mut cache = crate::hk_market::HkMarketCache::default();
        for code in ["00700", "01798"] {
            cache.securities.push(serde_json::from_value(json!({"code":code,"name":"新能源样本","currency":"HKD","board_lot":100,"security_type":"Equity","classification_status":"exchange_listed_equity"})).unwrap());
            cache.quotes.insert(code.to_string(), serde_json::from_value(json!({"code":code,"name":"新能源样本","price":10.0,"price_semantics_version":1,"pe":10.0,"pb":1.0,"market_cap_hkd":10000000000.0,"currency":"HKD","source":"fixture"})).unwrap());
        }
        let data = crate::hk_research::project_hk_market_data(&cache).unwrap();
        let request = gp_core::SectorScreenRequest { min_sector_candidates:1, per_sector_limit:5, ..Default::default() };
        let completed = super::hydrate_hk_screen_candidates(cache, data, super::HkCandidateScreenRequest::Grouped(&request), std::time::Duration::from_millis(30), |mut cache, codes, deadline| async move {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            let requested = codes.len();
            let fetched = super::collect_hk_screen_financials(codes, deadline, |code| async move {
                if code == "01798" { std::future::pending::<()>().await; }
                crate::hk_market::parse_hk_financial_snapshot(include_str!("../testdata/eastmoney-hk-financial.json"), &code, &crate::hk_market::current_epoch_millis())
            }).await;
            super::apply_hk_screen_financial_outcomes(&mut cache, &fetched.outcomes);
            super::mark_hk_financial_preparation_errors(&mut cache, &fetched.deferred_codes, super::HK_SCREEN_FINANCIAL_DEFERRED);
            Ok((cache,json!({"requested_count":requested,"attempted_count":fetched.attempted_count,"updated_count":fetched.outcomes.len(),"deferred_count":fetched.deferred_codes.len(),"errors":[]})))
        }).await.unwrap();
        assert_eq!(completed.summary["updated_count"], 1);
        assert_eq!(completed.summary["deferred_count"], 1);
        assert_eq!(completed.cache.financials["00700"].eps.value, Some(12.639));
        assert!(!completed.cache.financials.contains_key("01798"));
        assert!(completed.cache.financial_errors.contains_key("01798"));
    }

    #[tokio::test]
    async fn hk_financial_preload_bounds_cold_requests_and_marks_unfinished_rows_for_manual_retry() {
        let codes = (1..=120).map(|index| format!("{index:05}")).collect();
        let fetched = super::collect_hk_screen_financials(codes, tokio::time::Instant::now() + std::time::Duration::from_millis(20), |_| async {
            std::future::pending::<Result<crate::hk_market::HkFinancialSnapshot, String>>().await
        }).await;
        assert!(fetched.outcomes.is_empty());
        assert_eq!(fetched.attempted_count, 6);
        assert_eq!(fetched.deferred_codes.len(), 120);
        let mut cache = crate::hk_market::HkMarketCache::default();
        super::mark_hk_financial_preparation_errors(&mut cache, &fetched.deferred_codes, super::HK_SCREEN_FINANCIAL_DEFERRED);
        let mut result = json!({"groups":[{"sector":"新能源与储能","items":[{"stock":{"code":"00001.HK"}}]}]});
        crate::hk_research::attach_cached_hk_financial_metrics(&mut result, &cache);
        assert_eq!(result["groups"][0]["items"][0]["stock"]["hk_financial_error"], "财务预载达到限时，点击刷新财务可重试。");
        assert!(result["groups"][0]["items"][0]["stock"].get("hk_financial_snapshot").is_none());
    }

    #[test]
    fn hk_screening_metadata_identifies_cache_only_market_and_quote_sources() {
        let cache = crate::hk_market::HkMarketCache::default();
        let data = gp_core::CoreDataSet::default();
        let metadata = hk_research_metadata(&cache, &data, "refresh");

        assert_eq!(metadata["market"], "hk");
        assert_eq!(metadata["source"], "hk_market_cache");
        assert_eq!(metadata["cache_only"], true);
        assert_eq!(metadata["requested_data_policy"], "refresh");
        assert!(metadata["limitations"].as_array().unwrap().len() >= 3);
    }

    #[test]
    fn hk_screening_metadata_reports_the_actual_quote_and_history_dates() {
        let mut cache = crate::hk_market::HkMarketCache::default();
        cache.securities.push(crate::hk_market::HkSecurity {
            code: "00700".to_owned(),
            name: "Tencent".to_owned(),
            currency: "HKD".to_owned(),
            board_lot: Some(100),
            security_type: "Equity / Equity Securities (Main Board)".to_owned(),
            classification_status: crate::hk_market::HkClassificationStatus::ExchangeListedEquity,
        });
        cache.quotes.insert(
            "00700".to_owned(),
            crate::hk_market::HkQuote {
                code: "00700".to_owned(),
                name: "Tencent".to_owned(),
                name_zh: None,
                price: 400.0,
                price_semantics_version: 1,
                previous_close: None,
                open: None,
                high: None,
                low: None,
                change: None,
                change_pct: Some(0.01),
                volume_shares: Some(100),
                turnover_hkd: Some(40_000.0),
                pe: None,
                pb: None,
                dividend_yield_percent: None,
                market_cap_hkd: None,
                total_shares: None,
                circulating_shares: None,
                currency: "HKD".to_owned(),
                quote_time: Some("2026/11/05 16:08:22".to_owned()),
                source: "Tencent Finance".to_owned(),
                fetched_at: Some("2026-11-05T08:08:23Z".to_owned()),
                last_error: None,
            },
        );
        cache.histories.insert(
            "00700".to_owned(),
            crate::hk_market::HkHistoryRecord {
                request_range: None,
                request_ranges: Vec::new(),
                price_adjustment: Default::default(),
                bar_fetched_at: Default::default(),
                revision_reason: None,
                suspended_as_of: None,
                bars: vec![crate::hk_market::HkDailyBar {
                    date: "20261104".to_owned(),
                    open: 399.0,
                    high: 401.0,
                    low: 398.0,
                    close: 400.0,
                    volume_shares: Some(100),
                }],
                source: "Tencent Finance".to_owned(),
                fetched_at: "2026-11-05T08:08:24Z".to_owned(),
                last_error: None,
            },
        );
        let data = crate::hk_research::project_hk_market_data(&cache).unwrap();

        let metadata = hk_research_metadata(&cache, &data, "cache_only");

        assert_eq!(metadata["data_policy"], "cache_only");
        assert_eq!(metadata["quote_as_of"], "20261105");
        assert_eq!(metadata["history_as_of"], "20261104");
    }

    #[test]
    fn hk_backtest_drops_cn_only_filters_but_keeps_supported_hk_benchmarks() {
        let data = gp_core::CoreDataSet {
            stocks: vec![gp_core::StockItem {
                code: "00700.HK".to_owned(),
                price: 400.0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let request = hk_backtest_request(&json!({
            "criteria":{"market_scope":"创业板","require_institution_buy_ratio_gt_sell_ratio":true,
                "min_deducted_net_profit_billion":0,"min_deducted_net_profit_growth_rate":10},
            "benchmark":"candidate_equal_weight","start_date":"20200101","end_date":"20261107"
        }), &data).unwrap();
        assert_eq!(
            gp_core::backtest_selected_symbols(&data.stocks, &request),
            ["00700.HK"]
        );
        assert_eq!(request.benchmark, "candidate_equal_weight");
        assert!(request.criteria.market_scope.is_none());
        assert!(request
            .criteria
            .min_deducted_net_profit_growth_rate
            .is_none());
    }

    #[test]
    fn hk_backtest_returns_a_plain_result_and_propagates_missing_history() {
        let mut data = gp_core::CoreDataSet {
            stocks: vec![gp_core::StockItem {
                code: "00700.HK".to_owned(),
                price: 150.0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let request = hk_backtest_request(&json!({"source":"watchlist","stock_codes":["00700"],"start_date":"20200101","end_date":"20261107","transaction_cost_bps":0,"benchmark":"none"}),&data).unwrap();
        assert!(super::hk_candidate_backtest_value(&data, &request)
            .unwrap_err()
            .contains("历史日线"));
        data.histories.insert(
            "00700.HK".to_owned(),
            vec![
                gp_core::HistoryBar {
                    date: "20200102".to_owned(),
                    close: 100.0,
                    open: None,
                    high: None,
                    low: None,
                    volume: None,
                    capital: None,
                },
                gp_core::HistoryBar {
                    date: "20261106".to_owned(),
                    close: 150.0,
                    open: None,
                    high: None,
                    low: None,
                    volume: None,
                    capital: None,
                },
            ],
        );
        let result = super::hk_candidate_backtest_value(&data, &request).unwrap();
        assert_eq!(result["metrics"]["num_stocks"], 1);
        assert_eq!(result["metrics"]["total_return"], 0.5);
        assert!(result["equity_curve"].as_array().unwrap().len() > 1);
        assert!(result.get("Ok").is_none());
        assert!(result.get("Err").is_none());
    }

    #[test]
    fn hk_backtest_is_forced_to_cached_candidate_snapshot_without_cn_benchmark() {
        let data = gp_core::CoreDataSet {
            stocks: vec![gp_core::StockItem {
                code: "00700.HK".to_owned(),
                price: 400.0,
                ..gp_core::StockItem::default()
            }],
            ..gp_core::CoreDataSet::default()
        };
        let request = hk_backtest_request(
            &json!({
                "strategy_mode":"adaptive_swing_v1",
                "benchmark":"000300.SH",
                "stock_codes":["00700"],
                "start_date":"20261102",
                "end_date":"20261103"
            }),
            &data,
        )
        .unwrap();

        assert_eq!(request.strategy_mode, "candidate_snapshot");
        assert_eq!(request.benchmark, "none");
        assert_eq!(request.stock_codes, vec!["00700.HK"]);
        assert!(hk_backtest_request(
            &json!({
                "stock_codes":["600000"],
                "start_date":"20261102",
                "end_date":"20261103"
            }),
            &data,
        )
        .is_err());
    }
}

pub(crate) fn normalize_trend_prefetch_codes<'a>(
    codes: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for code in codes {
        if let Some(code) = crate::market::normalize_stock_code(code) {
            if seen.insert(code.clone()) {
                normalized.push(code);
            }
        }
    }
    normalized
}

pub(crate) fn screen_financial_snapshot(
    app: &tauri::AppHandle,
    payload: &Value,
) -> Result<Option<Arc<Value>>, String> {
    if let Some(snapshot) = payload
        .get("financial_snapshot")
        .filter(|snapshot| crate::market::financial_snapshot_payload_present(snapshot))
    {
        let snapshot = Arc::new(crate::metric_units::normalize_financial_snapshot(snapshot)?);
        if let Some(path) = crate::market::cache_context_path(app) {
            if let Ok(mut slot) = crate::market::refresh_financial_snapshot_cache().lock() {
                slot.insert(path, Arc::clone(&snapshot));
            }
        }
        return Ok(Some(snapshot));
    }
    let Some(path) = crate::market::cache_context_path(app) else {
        return Ok(None);
    };
    Ok(crate::market::refresh_financial_snapshot_cache()
        .lock()
        .map_err(|_| "财务缓存暂不可读".to_string())?
        .get(&path)
        .cloned())
}

pub(crate) fn screen_stock_override(
    app: &tauri::AppHandle,
    data: &Arc<gp_core::CoreDataSet>,
    payload: &Value,
) -> Result<Option<Arc<Vec<gp_core::StockItem>>>, String> {
    let Some(financial_snapshot) = screen_financial_snapshot(app, payload)? else {
        return Ok(None);
    };
    let path = crate::market::mobile_market_data_path(app)?;
    if let Ok(slot) = crate::market::screen_stock_overlay_cache().lock() {
        if let Some(entry) = slot.get(&path) {
            if Arc::ptr_eq(&entry.data, data)
                && (Arc::ptr_eq(&entry.financial_snapshot, &financial_snapshot)
                    || entry.financial_snapshot.as_ref() == financial_snapshot.as_ref())
            {
                return Ok(Some(Arc::clone(&entry.stocks)));
            }
        }
    }

    let stocks = Arc::new(financial_overlay::merge(
        data.as_ref(),
        financial_snapshot.as_ref(),
    ));
    if let Ok(mut slot) = crate::market::screen_stock_overlay_cache().lock() {
        slot.insert(
            path,
            crate::market::ScreenStockOverlayCacheEntry {
                data: Arc::clone(data),
                financial_snapshot,
                stocks: Arc::clone(&stocks),
            },
        );
    }
    Ok(Some(stocks))
}

pub(crate) async fn run_graph_screen_command(
    label: &'static str,
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    if is_hk_research_request(&payload) {
        let policy = DataPolicy::parse(&payload)?;
        let transport = payload.clone();
        let request = serde_json::from_value::<gp_core::GraphScreenRequest>(
            local_data::core_payload(payload),
        )
        .map_err(|error| format!("invalid HK graph screen request: {error}"))?;
        let (cache, data) = hk_projected_cache(&app)?;
        let calc_data = data.clone();
        let mut result = crate::runtime::run_cpu_bound(label, move || {
            let result = gp_core::graph_screen_with_data(&calc_data, &request)
                .map_err(|error| error.to_string())?;
            serde_json::to_value(result).map_err(|error| error.to_string())
        })
        .await??;
        crate::hk_research::attach_cached_hk_financial_metrics(&mut result, &cache);
        local_data::attach_metadata(
            &mut result,
            hk_research_metadata(&cache, &data, policy.as_str()),
            &transport,
        );
        return Ok(result);
    }
    let policy = DataPolicy::parse(&payload)?;
    let transport = payload.clone();
    let data = local_snapshot(&app, &payload, "run_graph_screen_command")?;
    let metadata = local_metadata(data.as_ref(), policy, &[]);
    let stock_override = screen_stock_override(&app, &data, &payload)?;
    let request =
        serde_json::from_value::<gp_core::GraphScreenRequest>(local_data::core_payload(payload))
            .map_err(|error| format!("invalid graph screen request: {error}"))?;
    let mut result = crate::runtime::run_cpu_bound(label, move || {
        let source = match stock_override.as_deref() {
            Some(stocks) => gp_core::StaticDataSource::with_stocks(data.as_ref(), stocks),
            None => gp_core::StaticDataSource::new(data.as_ref()),
        };
        let result = gp_core::graph_screen_with_source(&source, &request)
            .map_err(|error| error.to_string())?;
        serde_json::to_value(result).map_err(|error| error.to_string())
    })
    .await??;
    local_data::attach_metadata(&mut result, metadata, &transport);
    Ok(result)
}

#[cfg(test)]
pub(crate) fn merge_screen_financial_snapshot_into_data(
    data: &mut Value,
    financial_snapshot: &Value,
) {
    if !crate::market::financial_snapshot_payload_present(financial_snapshot) {
        return;
    }
    let (seed_stocks, seed_codes) = crate::market::seed_stock_maps(data);
    if seed_codes.is_empty() {
        return;
    }
    let enriched_stocks = crate::market::enriched_stock_maps(&seed_stocks, financial_snapshot);
    let mut stocks = Vec::with_capacity(seed_codes.len());
    let mut seen = HashSet::new();
    crate::market::append_preserved_seed_stocks(
        &seed_codes,
        &enriched_stocks,
        &mut stocks,
        &mut seen,
    );
    if let Some(object) = data.as_object_mut() {
        object.insert("stocks".to_string(), Value::Array(stocks));
    }
}

pub(crate) fn adaptive_screen_db_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("get app data dir failed: {error}"))?;
    root.push("screening");
    fs::create_dir_all(&root)
        .map_err(|error| format!("create screening dir failed: {}: {error}", root.display()))?;
    root.push(ADAPTIVE_SCREEN_DB_FILE);
    Ok(root)
}

pub(crate) fn initialize_adaptive_exposure_db(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         CREATE TABLE IF NOT EXISTS adaptive_screen_exposure (
             code TEXT NOT NULL,
             trade_date TEXT NOT NULL,
             bucket TEXT NOT NULL,
             mode TEXT NOT NULL,
             algorithm_version TEXT NOT NULL,
             selected_at INTEGER NOT NULL,
             PRIMARY KEY (code, trade_date, bucket)
         );
         CREATE INDEX IF NOT EXISTS idx_adaptive_exposure_date
           ON adaptive_screen_exposure(trade_date DESC, selected_at DESC);
         CREATE TABLE IF NOT EXISTS adaptive_screen_runs (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             run_id TEXT NOT NULL UNIQUE,
             trade_date TEXT NOT NULL,
             selected_codes_json TEXT NOT NULL,
             elapsed_millis INTEGER NOT NULL,
             cache_hit INTEGER NOT NULL,
             algorithm_version TEXT NOT NULL,
             recorded_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_adaptive_runs_recent
           ON adaptive_screen_runs(id DESC, recorded_at DESC);
         CREATE TABLE IF NOT EXISTS adaptive_screen_runs_v2 (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             run_id TEXT NOT NULL,
             implementation_fingerprint TEXT NOT NULL,
             trade_date TEXT NOT NULL,
             selected_codes_json TEXT NOT NULL,
             elapsed_millis INTEGER NOT NULL,
             cache_hit INTEGER NOT NULL,
             algorithm_version TEXT NOT NULL,
             recorded_at INTEGER NOT NULL,
             UNIQUE (run_id, implementation_fingerprint)
         );
         CREATE INDEX IF NOT EXISTS idx_adaptive_runs_v2_recent
           ON adaptive_screen_runs_v2(implementation_fingerprint, recorded_at ASC, id ASC);
         CREATE TABLE IF NOT EXISTS adaptive_screen_runs_v3 (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             run_id TEXT NOT NULL,
             implementation_fingerprint TEXT NOT NULL,
             release_evidence_qualified INTEGER NOT NULL,
             trade_date TEXT NOT NULL,
             selected_codes_json TEXT NOT NULL,
             elapsed_millis INTEGER NOT NULL,
             cache_hit INTEGER NOT NULL,
             algorithm_version TEXT NOT NULL,
             recorded_at INTEGER NOT NULL,
             UNIQUE (run_id, implementation_fingerprint)
         );
         CREATE INDEX IF NOT EXISTS idx_adaptive_runs_v3_recent
           ON adaptive_screen_runs_v3(implementation_fingerprint, release_evidence_qualified, recorded_at ASC, id ASC);
         CREATE TABLE IF NOT EXISTS adaptive_release_gate_reports (
             algorithm_version TEXT PRIMARY KEY,
             input_json TEXT NOT NULL,
             report_json TEXT NOT NULL,
             passed INTEGER NOT NULL,
             updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS adaptive_release_gate_reports_v2 (
             algorithm_version TEXT NOT NULL,
             implementation_fingerprint TEXT NOT NULL,
             qualification_json TEXT NOT NULL,
             input_json TEXT NOT NULL,
             report_json TEXT NOT NULL,
             passed INTEGER NOT NULL,
             updated_at INTEGER NOT NULL,
             PRIMARY KEY (algorithm_version, implementation_fingerprint)
         );",
    )
    .map_err(|error| format!("initialize adaptive screen sqlite failed: {error}"))
}

pub(crate) fn open_adaptive_screen_db(app: &tauri::AppHandle) -> Result<Connection, String> {
    let path = adaptive_screen_db_path(app)?;
    let conn = Connection::open(&path).map_err(|error| {
        format!(
            "open adaptive screen sqlite failed: {}: {error}",
            path.display()
        )
    })?;
    initialize_adaptive_exposure_db(&conn)?;
    Ok(conn)
}

pub(crate) fn adaptive_selected_codes(result: &Value) -> Vec<String> {
    let mut codes = result
        .get("groups")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|group| {
            group
                .get("items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|item| item.pointer("/stock/code").and_then(Value::as_str))
                .map(|code| code.to_ascii_uppercase())
        })
        .collect::<Vec<_>>();
    codes.sort();
    codes.dedup();
    codes
}

pub(crate) fn adaptive_release_run_record_rows(
    conn: &Connection,
    run_id: Option<&str>,
    result: &Value,
    trade_date: &str,
    elapsed_millis: u64,
    cache_hit: bool,
    release_evidence_qualified: bool,
) -> Result<(), String> {
    initialize_adaptive_exposure_db(conn)?;
    let trade_date = crate::market::compact_date_key(trade_date)
        .ok_or_else(|| "adaptive screen run trade date is invalid".to_string())?;
    let recorded_at = crate::market::epoch_millis().min(i64::MAX as u128) as i64;
    let keep_after = recorded_at.saturating_sub(30 * 24 * 60 * 60 * 1_000);
    let run_id = run_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("compat-{trade_date}-{recorded_at}"));
    let selected_codes_json = serde_json::to_string(&adaptive_selected_codes(result))
        .map_err(|error| format!("serialize adaptive run symbols failed: {error}"))?;
    let algorithm_version = result
        .get("algorithm_version")
        .and_then(Value::as_str)
        .unwrap_or("adaptive_swing_v1");
    let implementation_fingerprint = adaptive_release_implementation_fingerprint();
    conn.execute(
        "DELETE FROM adaptive_screen_runs_v3 WHERE recorded_at < ?1",
        params![keep_after],
    )
    .map_err(|error| format!("prune adaptive run evidence failed: {error}"))?;
    conn.execute(
        "INSERT INTO adaptive_screen_runs_v3
           (run_id, implementation_fingerprint, release_evidence_qualified, trade_date, selected_codes_json, elapsed_millis, cache_hit, algorithm_version, recorded_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(run_id, implementation_fingerprint) DO UPDATE SET
           release_evidence_qualified = excluded.release_evidence_qualified,
           trade_date = excluded.trade_date,
           selected_codes_json = excluded.selected_codes_json,
           elapsed_millis = excluded.elapsed_millis,
           cache_hit = excluded.cache_hit,
           algorithm_version = excluded.algorithm_version,
           recorded_at = excluded.recorded_at",
        params![
            run_id,
            implementation_fingerprint,
            i64::from(release_evidence_qualified),
            trade_date,
            selected_codes_json,
            elapsed_millis.min(i64::MAX as u64) as i64,
            i64::from(cache_hit),
            algorithm_version,
            recorded_at
        ],
    )
    .map_err(|error| format!("record adaptive run evidence failed: {error}"))?;
    adaptive_release_gate_recompute_operational_rows(conn)?;
    Ok(())
}

pub(crate) fn adaptive_release_run_record_sync(
    app: &tauri::AppHandle,
    run_id: Option<&str>,
    result: &Value,
    trade_date: &str,
    elapsed_millis: u64,
    cache_hit: bool,
    release_evidence_qualified: bool,
) -> Result<(), String> {
    adaptive_release_run_record_rows(
        &open_adaptive_screen_db(app)?,
        run_id,
        result,
        trade_date,
        elapsed_millis,
        cache_hit,
        release_evidence_qualified,
    )
}

pub(crate) fn adaptive_release_operational_evidence_rows(
    conn: &Connection,
) -> Result<(Option<usize>, Option<u64>, Option<u64>), String> {
    initialize_adaptive_exposure_db(conn)?;
    let keep_after = (crate::market::epoch_millis().min(i64::MAX as u128) as i64)
        .saturating_sub(30 * 24 * 60 * 60 * 1_000);
    let recent_json = {
        let mut statement = conn
            .prepare(
                "SELECT selected_codes_json
                 FROM adaptive_screen_runs_v3
                 WHERE algorithm_version = 'adaptive_swing_v1'
                   AND implementation_fingerprint = ?1
                   AND release_evidence_qualified = 1
                   AND recorded_at >= ?2
                 ORDER BY recorded_at ASC, id ASC",
            )
            .map_err(|error| format!("prepare adaptive run coverage query failed: {error}"))?;
        let rows = statement
            .query_map(
                params![adaptive_release_implementation_fingerprint(), keep_after],
                |row| row.get::<_, String>(0),
            )
            .map_err(|error| format!("query adaptive run coverage failed: {error}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("read adaptive run coverage failed: {error}"))?
    };
    let parsed_runs = recent_json
        .into_iter()
        .map(|encoded| {
            serde_json::from_str::<Vec<String>>(&encoded)
                .map(|codes| {
                    codes
                        .into_iter()
                        .map(|code| code.to_ascii_uppercase())
                        .collect::<HashSet<_>>()
                })
                .map_err(|error| format!("parse adaptive run symbols failed: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let five_run_unique_coverage = if parsed_runs.len() >= 5 {
        parsed_runs
            .windows(5)
            .map(|window| {
                window
                    .iter()
                    .flat_map(|codes| codes.iter().cloned())
                    .collect::<HashSet<_>>()
                    .len()
            })
            .min()
    } else {
        None
    };
    let max_elapsed = |cache_hit: bool| -> Result<Option<u64>, String> {
        let value = conn
            .query_row(
                "SELECT MAX(elapsed_millis)
                 FROM adaptive_screen_runs_v3
                 WHERE algorithm_version = 'adaptive_swing_v1'
                   AND implementation_fingerprint = ?1
                   AND release_evidence_qualified = 1
                   AND cache_hit = ?2
                   AND recorded_at >= ?3",
                params![
                    adaptive_release_implementation_fingerprint(),
                    i64::from(cache_hit),
                    keep_after
                ],
                |row| row.get::<_, Option<i64>>(0),
            )
            .map_err(|error| format!("query adaptive run latency failed: {error}"))?;
        Ok(value.and_then(|value| u64::try_from(value).ok()))
    };
    Ok((
        five_run_unique_coverage,
        max_elapsed(false)?,
        max_elapsed(true)?,
    ))
}

pub(crate) fn adaptive_release_operational_evidence_sync(
    app: &tauri::AppHandle,
) -> Result<(Option<usize>, Option<u64>, Option<u64>), String> {
    adaptive_release_operational_evidence_rows(&open_adaptive_screen_db(app)?)
}

pub(crate) fn adaptive_release_gate_store_rows(
    conn: &Connection,
    input: &gp_core::AdaptiveReleaseGateInput,
    report: &gp_core::AdaptiveReleaseGateReport,
    qualification: &Value,
) -> Result<(), String> {
    initialize_adaptive_exposure_db(conn)?;
    let input_json = serde_json::to_string(input)
        .map_err(|error| format!("serialize adaptive release input failed: {error}"))?;
    let report_json = serde_json::to_string(report)
        .map_err(|error| format!("serialize adaptive release report failed: {error}"))?;
    let qualification_json = serde_json::to_string(qualification)
        .map_err(|error| format!("serialize adaptive release qualification failed: {error}"))?;
    conn.execute(
        "INSERT INTO adaptive_release_gate_reports_v2
           (algorithm_version, implementation_fingerprint, qualification_json, input_json, report_json, passed, updated_at)
         VALUES ('adaptive_swing_v1', ?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(algorithm_version, implementation_fingerprint) DO UPDATE SET
           qualification_json = excluded.qualification_json,
           input_json = excluded.input_json,
           report_json = excluded.report_json,
           passed = excluded.passed,
           updated_at = excluded.updated_at",
        params![
            adaptive_release_implementation_fingerprint(),
            qualification_json,
            input_json,
            report_json,
            i64::from(report.passed),
            crate::market::epoch_millis().min(i64::MAX as u128) as i64
        ],
    )
    .map_err(|error| format!("store adaptive release gate failed: {error}"))?;
    Ok(())
}

pub(crate) fn adaptive_release_gate_context_rows(
    conn: &Connection,
) -> Result<Option<(gp_core::AdaptiveReleaseGateInput, Value)>, String> {
    let encoded = conn
        .query_row(
            "SELECT input_json, qualification_json
             FROM adaptive_release_gate_reports_v2
             WHERE algorithm_version = 'adaptive_swing_v1'
               AND implementation_fingerprint = ?1",
            params![adaptive_release_implementation_fingerprint()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|error| format!("load adaptive release gate context failed: {error}"))?;
    encoded
        .map(|(input_json, qualification_json)| {
            let input = serde_json::from_str::<gp_core::AdaptiveReleaseGateInput>(&input_json)
                .map_err(|error| format!("parse adaptive release input failed: {error}"))?;
            let qualification = serde_json::from_str::<Value>(&qualification_json)
                .map_err(|error| format!("parse adaptive release qualification failed: {error}"))?;
            Ok((input, qualification))
        })
        .transpose()
}

pub(crate) fn adaptive_release_gate_recompute_operational_rows(
    conn: &Connection,
) -> Result<(), String> {
    let Some((mut input, qualification)) = adaptive_release_gate_context_rows(conn)? else {
        return Ok(());
    };
    let evidence = adaptive_release_operational_evidence_rows(conn)?;
    input.five_run_unique_coverage = evidence.0;
    input.first_run_millis = evidence.1;
    input.cached_run_millis = evidence.2;
    let report = gp_core::evaluate_adaptive_release_gate(&input);
    adaptive_release_gate_store_rows(conn, &input, &report, &qualification)
}

#[cfg(test)]
pub(crate) fn adaptive_release_gate_load_rows(
    conn: &Connection,
) -> Result<Option<gp_core::AdaptiveReleaseGateReport>, String> {
    initialize_adaptive_exposure_db(conn)?;
    let encoded = conn
        .query_row(
            "SELECT input_json, report_json, qualification_json, passed
             FROM adaptive_release_gate_reports_v2
             WHERE algorithm_version = 'adaptive_swing_v1'
               AND implementation_fingerprint = ?1",
            params![adaptive_release_implementation_fingerprint()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("load adaptive release gate failed: {error}"))?;
    encoded
        .map(
            |(input_json, report_json, qualification_json, stored_passed)| {
                let input = serde_json::from_str::<gp_core::AdaptiveReleaseGateInput>(&input_json)
                    .map_err(|error| format!("parse adaptive release input failed: {error}"))?;
                let qualification =
                    serde_json::from_str::<Value>(&qualification_json).map_err(|error| {
                        format!("parse adaptive release qualification failed: {error}")
                    })?;
                let mut report =
                    serde_json::from_str::<gp_core::AdaptiveReleaseGateReport>(&report_json)
                        .map_err(|error| format!("parse adaptive release gate failed: {error}"))?;
                report.passed = report.passed
                    && stored_passed == 1
                    && input.release_configuration_qualified == Some(true)
                    && qualification.get("qualified").and_then(Value::as_bool) == Some(true)
                    && qualification
                        .get("implementation_fingerprint")
                        .and_then(Value::as_str)
                        == Some(adaptive_release_implementation_fingerprint());
                Ok(report)
            },
        )
        .transpose()
}

pub(crate) fn adaptive_release_gate_store_sync(
    app: &tauri::AppHandle,
    input: &gp_core::AdaptiveReleaseGateInput,
    report: &gp_core::AdaptiveReleaseGateReport,
    qualification: &Value,
) -> Result<(), String> {
    adaptive_release_gate_store_rows(&open_adaptive_screen_db(app)?, input, report, qualification)
}

#[cfg(test)]
pub(crate) fn adaptive_release_gate_refresh_and_load_rows(
    conn: &Connection,
) -> Result<Option<gp_core::AdaptiveReleaseGateReport>, String> {
    adaptive_release_gate_recompute_operational_rows(conn)?;
    adaptive_release_gate_load_rows(conn)
}

pub(crate) fn adaptive_exposure_recent_rows(
    conn: &Connection,
    current_trade_date: Option<&str>,
) -> Result<Vec<gp_core::AdaptiveRecentExposure>, String> {
    let cutoff_date = current_trade_date
        .and_then(crate::market::compact_date_key)
        .unwrap_or_else(|| "99999999".to_string());
    let mut statement = conn
        .prepare(
            "SELECT code, trade_date, bucket
             FROM adaptive_screen_exposure
             WHERE trade_date < ?1
               AND
             trade_date IN (
                  SELECT DISTINCT trade_date
                  FROM adaptive_screen_exposure
                  WHERE trade_date < ?1
                  ORDER BY trade_date DESC
                  LIMIT 5
              )
             ORDER BY trade_date DESC, code ASC, bucket ASC",
        )
        .map_err(|error| format!("prepare adaptive exposure query failed: {error}"))?;
    let rows = statement
        .query_map(params![cutoff_date], |row| {
            Ok(gp_core::AdaptiveRecentExposure {
                code: row.get(0)?,
                trade_date: row.get(1)?,
                bucket: row.get(2)?,
            })
        })
        .map_err(|error| format!("query adaptive exposure failed: {error}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("read adaptive exposure failed: {error}"))
}

pub(crate) fn adaptive_exposure_recent_sync(
    app: &tauri::AppHandle,
    current_trade_date: Option<&str>,
) -> Result<Vec<gp_core::AdaptiveRecentExposure>, String> {
    adaptive_exposure_recent_rows(&open_adaptive_screen_db(app)?, current_trade_date)
}

pub(crate) fn adaptive_exposure_record_sync(
    app: &tauri::AppHandle,
    result: &Value,
    trade_date: &str,
) -> Result<(), String> {
    let mut conn = open_adaptive_screen_db(app)?;
    adaptive_exposure_record_rows(&mut conn, result, trade_date)
}

pub(crate) fn adaptive_exposure_record_rows(
    conn: &mut Connection,
    result: &Value,
    trade_date: &str,
) -> Result<(), String> {
    initialize_adaptive_exposure_db(conn)?;
    let trade_date = crate::market::compact_date_key(trade_date)
        .ok_or_else(|| "adaptive screen trade date is invalid".to_string())?;
    let selected_at = crate::market::epoch_millis() as i64;
    let keep_after = selected_at.saturating_sub(30 * 24 * 60 * 60 * 1_000);
    let algorithm_version = result
        .get("algorithm_version")
        .and_then(Value::as_str)
        .unwrap_or("adaptive_swing_v1");
    let mode = result
        .pointer("/market_regime/effective")
        .and_then(Value::as_str)
        .unwrap_or("auto");
    let selected = result
        .get("groups")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|group| {
            let bucket = group
                .get("key")
                .and_then(Value::as_str)
                .unwrap_or("primary")
                .to_string();
            group
                .get("items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(move |item| {
                    item.pointer("/stock/code")
                        .and_then(Value::as_str)
                        .map(|code| (code.to_ascii_uppercase(), bucket.clone()))
                })
        })
        .collect::<Vec<_>>();
    let transaction = conn
        .transaction()
        .map_err(|error| format!("begin adaptive exposure transaction failed: {error}"))?;
    transaction
        .execute(
            "DELETE FROM adaptive_screen_exposure WHERE selected_at < ?1",
            params![keep_after],
        )
        .map_err(|error| format!("prune adaptive exposure failed: {error}"))?;
    {
        let mut statement = transaction
            .prepare(
                "INSERT INTO adaptive_screen_exposure
                   (code, trade_date, bucket, mode, algorithm_version, selected_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(code, trade_date, bucket) DO UPDATE SET
                   mode = excluded.mode,
                   algorithm_version = excluded.algorithm_version,
                   selected_at = excluded.selected_at",
            )
            .map_err(|error| format!("prepare adaptive exposure insert failed: {error}"))?;
        for (code, bucket) in selected {
            transaction
                .execute(
                    "DELETE FROM adaptive_screen_exposure WHERE code = ?1 AND trade_date = ?2",
                    params![code, trade_date],
                )
                .map_err(|error| format!("deduplicate adaptive exposure failed: {error}"))?;
            statement
                .execute(params![
                    code,
                    trade_date,
                    bucket,
                    mode,
                    algorithm_version,
                    selected_at
                ])
                .map_err(|error| format!("insert adaptive exposure failed: {error}"))?;
        }
    }
    transaction
        .commit()
        .map_err(|error| format!("commit adaptive exposure failed: {error}"))
}

#[cfg(test)]
mod local_first_tests {
    use super::*;
    #[test]
    fn trend_parallel_fetch_keeps_completed_stocks_when_one_source_stalls() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let fetch_active = Arc::clone(&active);
        let fetch_peak = Arc::clone(&peak);
        let mut codes = vec!["stalled".to_string()];
        codes.extend((0..14).map(|index| format!("ready-{index}")));
        let outcome = tauri::async_runtime::block_on(collect_adaptive_history_results(
            codes,
            "20260601".into(),
            Duration::from_millis(500),
            TREND_SCREEN_HISTORY_CONCURRENCY,
            move |code, _| {
                let code = code.to_string();
                let active = Arc::clone(&fetch_active);
                let peak = Arc::clone(&fetch_peak);
                async move {
                    let running = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(running, Ordering::SeqCst);
                    if code == "stalled" {
                        return std::future::pending().await;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                    Ok(vec![json!({"date":"2026-10-30", "close":10.0})])
                }
            },
            |_, _| {},
        ));
        assert!(outcome.timed_out);
        assert_eq!(outcome.results.len(), 14);
        assert!(outcome
            .results
            .iter()
            .all(|(code, result)| code != "stalled" && result.is_ok()));
        assert!(peak.load(Ordering::SeqCst) > 1);
        assert!(peak.load(Ordering::SeqCst) <= TREND_SCREEN_HISTORY_CONCURRENCY);
    }

    #[test]
    fn trend_coverage_refreshes_old_bars_even_when_the_count_is_sufficient() {
        let rows = (0..60)
            .map(|index| {
                let date = chrono::NaiveDate::from_ymd_opt(2026, 7, 26)
                    .unwrap()
                    .checked_add_days(chrono::Days::new(index))
                    .unwrap();
                serde_json::from_value::<gp_core::HistoryBar>(json!({
                    "date":date.format("%Y-%m-%d").to_string(), "close":10.0,
                    "open":10.0,"high":11.0,"low":9.0,"volume":100.0
                }))
                .unwrap()
            })
            .collect::<Vec<_>>();
        let code = "688279.SH".to_string();
        let stock = gp_core::StockItem {
            code: code.clone(),
            price: 173.73,
            quote_time: Some("20261030161451".into()),
            ..gp_core::StockItem::default()
        };
        let data = gp_core::CoreDataSet {
            stocks: vec![stock],
            histories: [(code.clone(), rows.clone())].into_iter().collect(),
            ..gp_core::CoreDataSet::default()
        };
        let codes = vec![code.clone()];
        let old = local_trend_history_coverage(
            &data,
            &HashMap::new(),
            &data.stocks,
            &codes,
            "20260601",
            "20261101",
            45,
        );
        assert_eq!(old[0]["available_bars"], 60);
        assert_eq!(old[0]["sufficient"], false);
        assert_eq!(old[0]["quote_as_of"], "20261030");
        assert_eq!(old[0]["as_of"], "20260923");
        assert_eq!(old[0]["history_behind_quote"], true);
        assert!(local_data::require_coverage("trend_screen", &old).is_err());

        let mut fresh_rows = rows;
        fresh_rows
            .push(serde_json::from_value(json!({"date":"2026-10-30", "close":173.73})).unwrap());
        let overrides = [(code, fresh_rows)].into_iter().collect();
        let fresh = local_trend_history_coverage(
            &data,
            &overrides,
            &data.stocks,
            &codes,
            "20260601",
            "20261101",
            45,
        );
        assert_eq!(fresh[0]["sufficient"], true);
        assert_eq!(fresh[0]["history_behind_quote"], false);

        let historical = local_trend_history_coverage(
            &data,
            &HashMap::new(),
            &data.stocks,
            &codes,
            "20260601",
            "20260923",
            45,
        );
        assert_eq!(historical[0]["sufficient"], true);
    }

    #[test]
    fn cached_adaptive_coverage_keeps_old_data_and_exact_pit_boundary() {
        let mut data = gp_core::CoreDataSet::default();
        let rows = (1..=3).flat_map(|month| (1..=28).map(move |day| format!("2020{month:02}{day:02}"))).take(60).map(|date| {
            serde_json::from_value::<gp_core::HistoryBar>(json!({"date":date,"open":10.0,"high":11.0,"low":9.0,"close":10.0,"volume":100.0})).unwrap()
        }).collect::<Vec<_>>();
        data.histories.insert("000001.SZ".into(), rows);
        let codes = vec!["000001.SZ".into()];
        let old = adaptive_local_coverage(&data, &HashMap::new(), &codes, Some("20200304"));
        assert!(local_data::require_coverage("adaptive_screen", &old).is_ok());
        let gap = adaptive_local_coverage(&data, &HashMap::new(), &codes, Some("20200305"));
        assert!(local_data::require_coverage("adaptive_screen", &gap).is_err());
        let future = adaptive_local_coverage(&data, &HashMap::new(), &codes, Some("20200228"));
        assert!(local_data::require_coverage("adaptive_screen", &future).is_err());
    }
    #[test]
    fn missing_required_history_is_detailed_and_never_succeeds() {
        let coverage = local_history_coverage(
            &gp_core::CoreDataSet::default(),
            &HashMap::new(),
            &["000001.SZ".into()],
            "20200101",
            "20201231",
            45,
        );
        let error: Value = serde_json::from_str(
            &local_data::require_coverage("trend_screen", &coverage).unwrap_err(),
        )
        .unwrap();
        assert_eq!(error["missing"][0]["code"], "000001.SZ");
        assert_eq!(error["missing"][0]["available_bars"], 0);
        assert_eq!(error["missing"][0]["required_bars"], 45);
    }
}
