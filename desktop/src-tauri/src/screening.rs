use futures::stream::{self, FuturesUnordered, StreamExt};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{PathBuf},
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
use stock_optimizer_core as gp_core;
use tauri::{Emitter, Manager};


pub(crate) const ADAPTIVE_SCREEN_DB_FILE: &str = "adaptive-screen.sqlite";

pub(crate) const TREND_SCREEN_HISTORY_TIMEOUT_SECS: u64 = 18;

pub(crate) const TREND_SCREEN_HISTORY_CONCURRENCY: usize = 6;

pub(crate) const TREND_SCREEN_HISTORY_PREFETCH_LIMIT: usize = 80;

pub(crate) const MIN_TREND_SCREEN_HISTORY_BARS: usize = 45;

pub(crate) const ADAPTIVE_SCREEN_TOTAL_TIMEOUT_SECS: u64 = 120;

pub(crate) const ADAPTIVE_SCREEN_HISTORY_CONCURRENCY: usize = 6;

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
}

pub(crate) struct AdaptiveHistoryFetchOutcome {
    pub(crate) results: Vec<(String, Result<Vec<Value>, String>)>,
    pub(crate) timed_out: bool,
}

pub(crate) struct PreparedBacktest {
    pub(crate) data: Arc<gp_core::CoreDataSet>,
    pub(crate) stock_override: Option<Arc<Vec<gp_core::StockItem>>>,
    pub(crate) history_override: HashMap<String, Vec<gp_core::HistoryBar>>,
    pub(crate) request: gp_core::BacktestRequest,
    pub(crate) notes: Vec<String>,
}

#[tauri::command]
pub(crate) async fn api_screen(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    if legacy_screen_requested(&payload) {
        return api_legacy_screen(app, payload).await;
    }
    adaptive_screen_with_timeout(
        Duration::from_secs(ADAPTIVE_SCREEN_TOTAL_TIMEOUT_SECS),
        api_adaptive_screen(app, payload),
    )
    .await
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

pub(crate) async fn api_adaptive_screen(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let started_at = Instant::now();
    let prepared = prepare_adaptive_screen(&app, payload).await?;
    let PreparedAdaptiveScreen {
        data,
        stock_override,
        candidate_codes,
        history_override,
        request,
        recent_exposure,
        notes,
        cache_hit,
    } = prepared;
    emit_adaptive_screen_progress(
        &app,
        request.run_id.as_deref(),
        "regime",
        82,
        "判断市场状态",
    );
    let calculation_request = request.clone();
    let mut result = crate::runtime::run_cpu_bound("api_screen", move || {
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
    .await??;
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
            crate::market::local_yyyymmdd_from_epoch_ms(crate::market::epoch_millis()).unwrap_or_else(|| "19700101".to_string())
        });
    let exposure_write_date = exposure_date.clone();
    let exposure_write = crate::runtime::run_io_bound("adaptive_screen_exposure_write", move || {
        adaptive_exposure_record_sync(&exposure_app, &exposure_result, &exposure_write_date)
    })
    .await?;
    if let Err(error) = exposure_write {
        crate::market::append_result_notes(&mut result, vec![format!("近期曝光记录写入失败：{error}")]);
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
    Ok(result)
}

pub(crate) async fn api_legacy_screen(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let data = crate::market::cached_market_data_snapshot(&app)?;
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
    serde_json::from_value::<gp_core::ScreenCriteria>(crate::market::strip_core_side_payload_fields(payload))
        .map_err(|error| format!("invalid legacy screen request: {error}"))
}

pub(crate) fn adaptive_screen_request_from_payload(
    payload: Value,
) -> Result<gp_core::AdaptiveScreenRequest, String> {
    let payload = crate::market::strip_core_side_payload_fields(payload);
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
    required_codes.extend(crate::market::adaptive_benchmark_codes().into_iter().map(str::to_string));
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
pub(crate) async fn api_sector_screen(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let data = crate::market::cached_market_data_snapshot(&app)?;
    let stock_override = screen_stock_override(&app, &data, &payload)?;
    let request = serde_json::from_value::<gp_core::SectorScreenRequest>(
        crate::market::strip_core_side_payload_fields(payload),
    )
    .map_err(|error| format!("invalid sector screen request: {error}"))?;
    crate::runtime::run_cpu_bound("api_sector_screen", move || {
        let result = match stock_override.as_deref() {
            Some(stocks) => gp_core::sector_screen_stocks(stocks, &request),
            None => gp_core::sector_screen_with_data(data.as_ref(), &request)
                .map_err(|error| error.to_string())?,
        };
        serde_json::to_value(result).map_err(|error| error.to_string())
    })
    .await?
}

#[tauri::command]
pub(crate) async fn api_custom_screen(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    run_graph_screen_command("api_custom_screen", app, payload).await
}

#[tauri::command]
pub(crate) async fn api_graph_screen(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    run_graph_screen_command("api_graph_screen", app, payload).await
}

#[tauri::command]
pub(crate) async fn api_trend_analyze(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let data = crate::market::cached_market_data_snapshot(&app)?;
    let stock_override = screen_stock_override(&app, &data, &payload)?;
    let request = serde_json::from_value::<gp_core::TrendIndicatorRequest>(
        crate::market::strip_core_side_payload_fields(payload),
    )
    .map_err(|error| format!("invalid trend request: {error}"))?;
    crate::runtime::run_cpu_bound("api_trend_analyze", move || {
        let source = match stock_override.as_deref() {
            Some(stocks) => gp_core::StaticDataSource::with_stocks(data.as_ref(), stocks),
            None => gp_core::StaticDataSource::new(data.as_ref()),
        };
        let result =
            gp_core::trend_with_source(&source, &request).map_err(|error| error.to_string())?;
        serde_json::to_value(result).map_err(|error| error.to_string())
    })
    .await?
}

#[tauri::command]
pub(crate) async fn api_trend_screen(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    api_trend_screen_inner(app, payload).await
}

pub(crate) async fn api_trend_screen_inner(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let prepared = tokio::time::timeout(
        Duration::from_secs(TREND_SCREEN_HISTORY_TIMEOUT_SECS),
        prepare_trend_screen(&app, payload),
    )
    .await
    .map_err(|_| format!("trend screen history prefetch exceeded {TREND_SCREEN_HISTORY_TIMEOUT_SECS}s; retry after refreshing market data."))??;
    let PreparedTrendScreen {
        data,
        stock_override,
        history_override,
        request,
        notes,
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
            crate::market::append_result_notes(&mut result, vec![format!("发布门槛报告写入失败：{error}")]);
        }
    }
    crate::market::append_result_notes(&mut result, notes);
    Ok(result)
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
        let normalized = crate::market::normalize_stock_code(code).unwrap_or_else(|| code.to_ascii_uppercase());
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
            let mut hasher = Sha256::new();
            hasher.update(b"adaptive_swing_v1-release-contract-v2");
            hasher.update(include_bytes!(
                "../../../native/gp-core/src/adaptive_screen.rs"
            ));
            hasher.update(include_bytes!("../../../native/gp-core/src/lib.rs"));
            hasher.update(include_bytes!("lib.rs"));
            let digest = hasher.finalize();
            digest.iter().map(|byte| format!("{byte:02x}")).collect()
        })
        .as_str()
}

pub(crate) fn adaptive_release_criteria_is_full_universe(criteria: &gp_core::ScreenCriteria) -> bool {
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

pub(crate) fn adaptive_release_screen_request_qualified(request: &gp_core::AdaptiveScreenRequest) -> bool {
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

pub(crate) fn adaptive_release_force_cold_start_code(missing: &mut Vec<String>, required_codes: &[String]) {
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
    let force_cold_start = adaptive_release_validation_force_cold_start(&payload);
    let data = crate::market::cached_market_data_snapshot(app)?;
    let stock_override = screen_stock_override(app, &data, &payload)?;
    let mut request = adaptive_screen_request_from_payload(payload)?;
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
        .or_else(|| crate::market::expected_market_quote_date_from_epoch_ms(crate::market::epoch_millis()));
    let mut missing = adaptive_missing_history_codes(
        data.as_ref(),
        &required_codes,
        target_history_date.as_deref(),
    );
    if force_cold_start {
        adaptive_release_force_cold_start_code(&mut missing, &required_codes);
    }
    let cache_hit = missing.is_empty();

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
        let fetch_outcome =
            crate::runtime::with_heavy_network_permit("adaptive_screen_history_fetch", async move {
                let outcome =
                    collect_adaptive_history_results(
                        fetch_codes,
                        history_start_date,
                        Duration::from_secs(ADAPTIVE_SCREEN_HISTORY_PREFETCH_TIMEOUT_SECS),
                        ADAPTIVE_SCREEN_HISTORY_CONCURRENCY,
                        |code, start_date| {
                            let code = code.to_string();
                            let start_date = start_date.to_string();
                            async move {
                                crate::market::fetch_observe_daily_history(&code, &start_date, "20501231").await
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
            })
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
    request.as_of_date =
        latest_adaptive_data_date(data.as_ref(), &history_override, &required_codes)
            .or(target_history_date);
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
    let recent_exposure = crate::runtime::run_io_bound("adaptive_screen_exposure_read", move || {
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
    })
}

pub(crate) fn adaptive_history_start_date() -> String {
    const LOOKBACK_MILLIS: u128 = 220 * 24 * 60 * 60 * 1_000;
    crate::market::local_yyyymmdd_from_epoch_ms(crate::market::epoch_millis().saturating_sub(LOOKBACK_MILLIS))
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
                crate::market::compact_date_key(&bar.date).is_some_and(|date| date.as_str() <= target.as_str())
            })
        })
        .rev()
        .take(120)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>();
    if target.is_some() && window.last().and_then(|bar| crate::market::compact_date_key(&bar.date)) != target {
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
    let data = crate::market::cached_market_data_snapshot(app)?;
    let stock_override = screen_stock_override(app, &data, &payload)?;
    let request = serde_json::from_value::<gp_core::TrendScreenRequest>(
        crate::market::strip_core_side_payload_fields(payload),
    )
    .map_err(|error| format!("invalid trend screen request: {error}"))?;
    let criteria = request.criteria.clone();
    let candidate_data = Arc::clone(&data);
    let candidate_stocks = stock_override.clone();
    let candidate_result = crate::runtime::run_cpu_bound("api_trend_screen_candidates", move || {
        let universe = candidate_stocks
            .as_deref()
            .map(Vec::as_slice)
            .unwrap_or(candidate_data.stocks.as_slice());
        gp_core::screen_stocks(universe, &criteria)
    })
    .await?;
    let candidates = trend_history_prefetch_codes_from_result(&candidate_result, request.limit);
    let missing = candidates
        .iter()
        .filter(|code| {
            !crate::market::typed_history_cache_has_bars(
                data.as_ref(),
                code,
                &request.start_date,
                &request.end_date,
                MIN_TREND_SCREEN_HISTORY_BARS,
            )
        })
        .take(TREND_SCREEN_HISTORY_PREFETCH_LIMIT)
        .cloned()
        .collect::<Vec<_>>();
    let mut notes = Vec::new();
    if missing.is_empty() {
        notes.push(format!(
            "Trend screen reused cached OHLCV history for {} candidates.",
            candidates.len()
        ));
        return Ok(PreparedTrendScreen {
            data,
            stock_override,
            history_override: HashMap::new(),
            request,
            notes,
        });
    }

    let start_date = request.start_date.clone();
    let end_date = request.end_date.clone();
    let fetch_missing = missing.clone();
    let fetches =
        crate::runtime::with_heavy_network_permit("api_trend_screen_history_fetch", async move {
            let results = stream::iter(fetch_missing)
                .map(|code| {
                    let start_date = start_date.clone();
                    let end_date = end_date.clone();
                    async move {
                        let result =
                            crate::market::fetch_observe_daily_history(&code, &start_date, &end_date).await;
                        (code, result)
                    }
                })
                .buffer_unordered(TREND_SCREEN_HISTORY_CONCURRENCY)
                .collect::<Vec<_>>()
                .await;
            Ok(results)
        })
        .await?;

    let mut history_override = HashMap::new();
    let mut history_patch = serde_json::Map::new();
    let mut fetched = 0usize;
    let mut failed = 0usize;
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
        if let Err(error) = crate::market::persist_market_data_patch_updates(app.clone(), patch).await {
            notes.push(format!(
                "Trend screen fetched OHLCV for {fetched} candidates, but cache patch write failed: {error}"
            ));
        }
    }
    notes.push(format!(
        "Trend screen OHLCV prefetch: candidates {}, missing {}, fetched {fetched}, failed {failed}. Algorithm uses MA/KDJ/MACD/SWL/SWS, quant score, volume-price heat, and quality overlays.",
        candidates.len(),
        missing.len()
    ));
    Ok(PreparedTrendScreen {
        data,
        stock_override,
        history_override,
        request,
        notes,
    })
}

pub(crate) async fn prepare_backtest(
    app: &tauri::AppHandle,
    payload: Value,
) -> Result<PreparedBacktest, String> {
    let data = crate::market::cached_market_data_snapshot(app)?;
    let stock_override = screen_stock_override(app, &data, &payload)?;
    let request =
        serde_json::from_value::<gp_core::BacktestRequest>(crate::market::strip_core_side_payload_fields(payload))
            .map_err(|error| format!("invalid backtest request: {error}"))?;
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
        return Ok(PreparedBacktest {
            data,
            stock_override,
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
        candidates.extend(crate::market::adaptive_benchmark_codes().into_iter().map(str::to_string));
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
    let missing = candidates
        .iter()
        .filter(|code| {
            !crate::market::typed_history_cache_has_bars(
                data.as_ref(),
                code,
                &history_start_date,
                &request.end_date,
                minimum_bars,
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut notes = Vec::new();
    if missing.is_empty() {
        notes.push(format!(
            "已复用 {} 只入选股票的本地日线缓存。",
            candidates.len()
        ));
        return Ok(PreparedBacktest {
            data,
            stock_override,
            history_override: HashMap::new(),
            request,
            notes,
        });
    }

    let start_date = history_start_date;
    let end_date = request.end_date.clone();
    let fetch_missing = missing.clone();
    let fetches = crate::runtime::with_heavy_network_permit("api_backtest_history_fetch", async move {
        let results = stream::iter(fetch_missing)
            .map(|code| {
                let start_date = start_date.clone();
                let end_date = end_date.clone();
                async move {
                    let result = crate::market::fetch_observe_daily_history(&code, &start_date, &end_date).await;
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
    let mut fetched = 0usize;
    let mut failed = 0usize;
    let mut failure_samples = Vec::new();
    for (code, result) in fetches {
        match result {
            Ok(rows) if backtest_history_rows_are_usable(&rows) => {
                let typed =
                    serde_json::from_value::<Vec<gp_core::HistoryBar>>(Value::Array(rows.clone()))
                        .map_err(|error| {
                            format!("backtest history parse failed for {code}: {error}")
                        })?;
                history_patch.insert(code.clone(), Value::Array(rows));
                history_override.insert(code, typed);
                fetched += 1;
            }
            Ok(rows) => {
                failed += 1;
                if failure_samples.len() < 3 {
                    failure_samples.push(format!(
                        "{code}: daily-history sources returned {} rows; at least {MIN_BACKTEST_HISTORY_BARS} are required",
                        rows.len()
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
        let patch = json!({ "histories": history_patch });
        if let Err(error) = crate::market::persist_market_data_patch_updates(app.clone(), patch).await {
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
    Ok(PreparedBacktest {
        data,
        stock_override,
        history_override,
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

pub(crate) fn trend_history_prefetch_codes_from_result(
    candidate_result: &gp_core::ScreenResult,
    limit: usize,
) -> Vec<String> {
    let pool_size = TREND_SCREEN_HISTORY_PREFETCH_LIMIT
        .max(limit.saturating_mul(5))
        .max(50)
        .min(200);
    normalize_trend_prefetch_codes(
        candidate_result
            .items
            .iter()
            .take(pool_size)
            .map(|item| item.stock.code.as_str()),
    )
}

pub(crate) fn normalize_trend_prefetch_codes<'a>(codes: impl IntoIterator<Item = &'a str>) -> Vec<String> {
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

#[cfg(test)]
pub(crate) fn trend_history_prefetch_codes(candidate_result: &Value, limit: usize) -> Vec<String> {
    let pool_size = TREND_SCREEN_HISTORY_PREFETCH_LIMIT
        .max(limit.saturating_mul(5))
        .max(50)
        .min(200);
    normalize_trend_prefetch_codes(
        candidate_result
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(pool_size)
            .filter_map(|item| {
                item.get("stock")
                    .and_then(|stock| stock.get("code"))
                    .and_then(Value::as_str)
            }),
    )
}

pub(crate) fn screen_financial_snapshot(app: &tauri::AppHandle, payload: &Value) -> Option<Arc<Value>> {
    if let Some(snapshot) = payload
        .get("financial_snapshot")
        .filter(|snapshot| crate::market::financial_snapshot_payload_present(snapshot))
    {
        let snapshot = Arc::new(snapshot.clone());
        if let Some(path) = crate::market::cache_context_path(app) {
            if let Ok(mut slot) = crate::market::refresh_financial_snapshot_cache().lock() {
                slot.insert(path, Arc::clone(&snapshot));
            }
        }
        return Some(snapshot);
    }
    let path = crate::market::cache_context_path(app)?;
    crate::market::refresh_financial_snapshot_cache()
        .lock()
        .ok()?
        .get(&path)
        .cloned()
}

pub(crate) fn screen_stock_override(
    app: &tauri::AppHandle,
    data: &Arc<gp_core::CoreDataSet>,
    payload: &Value,
) -> Result<Option<Arc<Vec<gp_core::StockItem>>>, String> {
    let Some(financial_snapshot) = screen_financial_snapshot(app, payload) else {
        return Ok(None);
    };
    let path = crate::market::mobile_market_data_path(app)?;
    if let Ok(slot) = crate::market::screen_stock_overlay_cache().lock() {
        if let Some(entry) = slot.get(&path) {
            if Arc::ptr_eq(&entry.data, data)
                && entry.financial_snapshot.as_ref() == financial_snapshot.as_ref()
            {
                return Ok(Some(Arc::clone(&entry.stocks)));
            }
        }
    }

    let mut stock_data = json!({ "stocks": &data.stocks });
    merge_screen_financial_snapshot_into_data(&mut stock_data, financial_snapshot.as_ref());
    let stocks = stock_data
        .as_object_mut()
        .and_then(|object| object.remove("stocks"))
        .unwrap_or_else(|| json!([]));
    let stocks = Arc::new(
        serde_json::from_value(stocks)
            .map_err(|error| format!("screen stock overlay parse failed: {error}"))?,
    );
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
    let data = crate::market::cached_market_data_snapshot(&app)?;
    let stock_override = screen_stock_override(&app, &data, &payload)?;
    let request = serde_json::from_value::<gp_core::GraphScreenRequest>(
        crate::market::strip_core_side_payload_fields(payload),
    )
    .map_err(|error| format!("invalid graph screen request: {error}"))?;
    crate::runtime::run_cpu_bound(label, move || {
        let source = match stock_override.as_deref() {
            Some(stocks) => gp_core::StaticDataSource::with_stocks(data.as_ref(), stocks),
            None => gp_core::StaticDataSource::new(data.as_ref()),
        };
        let result = gp_core::graph_screen_with_source(&source, &request)
            .map_err(|error| error.to_string())?;
        serde_json::to_value(result).map_err(|error| error.to_string())
    })
    .await?
}

pub(crate) fn merge_screen_financial_snapshot_into_data(data: &mut Value, financial_snapshot: &Value) {
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
    crate::market::append_preserved_seed_stocks(&seed_codes, &enriched_stocks, &mut stocks, &mut seen);
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
    let keep_after =
        (crate::market::epoch_millis().min(i64::MAX as u128) as i64).saturating_sub(30 * 24 * 60 * 60 * 1_000);
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

pub(crate) fn adaptive_release_gate_recompute_operational_rows(conn: &Connection) -> Result<(), String> {
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
