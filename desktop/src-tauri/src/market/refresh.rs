use crate::market::cache::records::write_mobile_market_data_record;
use crate::market::cache::snapshots::refresh_financial_snapshot_payload;
use crate::market::cache::snapshots::refresh_seed_payload;
use crate::market::providers::history::fetch_observe_daily_history;
use crate::market::providers::tencent::candidate_batch_window;
use crate::market::providers::tencent::fetch_tencent_quotes;
use crate::market::providers::tencent::parse_tencent_quotes;
use crate::market::providers::tencent::TencentQuotePayload;
use crate::market::providers::tencent::MAX_TENCENT_WEBVIEW_QUOTE_BYTES;
use crate::market::providers::tencent::TENCENT_BATCH_SIZE;
use crate::market::providers::tencent::TENCENT_BATCH_TIMEOUT_SECS;
use crate::market::providers::tencent::TENCENT_DEFAULT_MAX_CANDIDATES;
use crate::market::providers::tencent::TENCENT_DEFAULT_MAX_FAILED_BATCHES;
use crate::market::providers::tencent::TENCENT_FETCH_CONCURRENCY;
use crate::market::providers::tencent::TENCENT_REQUEST_TIMEOUT_SECS;
use crate::market_domain::codes::adaptive_benchmark_codes;
use crate::market_domain::codes::build_candidate_codes;
use crate::market_domain::codes::dedupe_stock_codes;
use crate::market_domain::codes::normalize_stock_code;
use crate::market_domain::datasets::append_all_preserved_seed_stocks;
use crate::market_domain::datasets::filter_seed_relations;
use crate::market_domain::datasets::seed_stock_maps;
use crate::market_domain::dates::epoch_millis;
use crate::market_domain::dates::market_refresh_history_start_date;
use crate::market_domain::financial::enriched_stock_maps;
use crate::market_domain::financial::filtered_financial_snapshot_map;
use crate::market_domain::history::history_cache_has_bars;
use crate::market_domain::history::MARKET_REFRESH_HISTORY_MIN_BARS;
use crate::market_events::emit_market_refresh_log;
use crate::network::http::build_http_client_with_proxy;
use base64::engine::general_purpose;
use base64::Engine;
use futures::stream;
use futures::StreamExt;
use serde_json::json;
use serde_json::Value;
use std::collections::HashSet;
use std::time::Duration;
use stock_optimizer_core as gp_core;

/// The market refresh owns the first bounded history warm-up used by the
/// local-only research actions. Keeping this small makes a full quote refresh
/// useful without turning it into an unbounded history crawl.
pub(crate) const MARKET_REFRESH_HISTORY_PREFETCH_LIMIT: usize = 80;

pub(crate) const MARKET_REFRESH_HISTORY_PREFETCH_CONCURRENCY: usize = 24;

pub(crate) async fn resume_explicit_cn_refreshes(app: tauri::AppHandle) -> Result<Value, String> {
    let store = crate::external_data::open_app_store(&app)?;
    let runs = store.foreground_recoveries()?;
    drop(store);
    let mut resumed = 0_usize;
    let mut failed = Vec::new();
    for run in runs {
        if run.request.market != crate::external_data::Market::China {
            continue;
        }
        if run.request.scope != crate::external_data::RefreshScope::MarketWide {
            failed.push(json!({
                "run_id": run.run_id,
                "error": "stored A-share recovery requires a market-wide refresh; retry from the user action"
            }));
            continue;
        }
        match resume_explicit_cn_run(&app, &run).await {
            Ok(()) => resumed += 1,
            Err(error) => failed.push(json!({ "run_id": run.run_id, "error": error })),
        }
    }
    Ok(json!({"resumed": resumed, "failed": failed}))
}

async fn resume_explicit_cn_run(
    app: &tauri::AppHandle,
    run: &crate::external_data::RefreshRunStatus,
) -> Result<(), String> {
    use crate::external_data::{Market, RefreshRunState};
    let now = crate::agent_ledger::current_epoch_millis();
    let mut store = crate::external_data::open_app_store(app)?;
    store.start_refresh_run(&run.run_id, true, now)?;
    store.record_source_attempt(
        "mobile-market",
        run.request.domain,
        Market::China,
        &now.to_string(),
    )?;
    drop(store);
    let result = core_mobile_market_data_refresh_tencent(
        app.clone(),
        json!({
            "data_policy": "refresh",
            "domain": run.request.domain.as_str(),
            "request_id": run.request.request_id,
            "batch_start": 0,
            "batch_count": TENCENT_BATCH_SIZE,
            "max_candidates": TENCENT_DEFAULT_MAX_CANDIDATES,
            "validate_after_write": true,
        }),
    )
    .await;
    let finished_at = crate::agent_ledger::current_epoch_millis();
    let mut store = crate::external_data::open_app_store(app)?;
    match result {
        Ok(result) => {
            let partial = result
                .get("failed_batches")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                > 0
                || result
                    .get("history_failed")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    > 0
                || !result.get("done").and_then(Value::as_bool).unwrap_or(true)
                || result
                    .get("stopped_early")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
            store.finish_refresh_run(
                &run.run_id,
                if partial {
                    RefreshRunState::Retryable
                } else {
                    RefreshRunState::Completed
                },
                finished_at,
                partial.then_some("partial_recovery"),
                partial
                    .then_some("foreground recovery made partial progress; user retry is required"),
            )?;
            store.record_source_success(
                "mobile-market",
                run.request.domain,
                Market::China,
                &finished_at.to_string(),
                result.get("fetched").and_then(Value::as_u64).unwrap_or(0) as usize,
            )?;
            Ok(())
        }
        Err(error) => {
            let lower = error.to_ascii_lowercase();
            let category = if lower.contains("timeout") || lower.contains("timed out") {
                "timeout"
            } else if lower.contains("429") {
                "http_429"
            } else if lower.contains("http 5") {
                "http_5xx"
            } else if lower.contains("connection") {
                "connection"
            } else {
                "provider_error"
            };
            store.record_source_failure(
                "mobile-market",
                run.request.domain,
                Market::China,
                &finished_at.to_string(),
                category,
                &error,
            )?;
            let transient = matches!(category, "timeout" | "http_429" | "http_5xx" | "connection");
            store.finish_refresh_run(
                &run.run_id,
                if transient {
                    RefreshRunState::Retryable
                } else {
                    RefreshRunState::Failed
                },
                finished_at,
                Some(category),
                Some(&error),
            )?;
            Err(error)
        }
    }
}

pub(crate) async fn core_mobile_market_data_refresh_tencent_inner(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let mut seed = refresh_seed_payload(&app, &payload);
    let financial_snapshot = refresh_financial_snapshot_payload(&app, &payload);
    let scan_candidates = payload
        .get("scan_candidates")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let use_previous_close = payload
        .get("use_previous_close")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let max_candidates = payload
        .get("max_candidates")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or(TENCENT_DEFAULT_MAX_CANDIDATES);
    let max_failed_batches = payload
        .get("max_failed_batches")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or(TENCENT_DEFAULT_MAX_FAILED_BATCHES);
    let batch_start = payload
        .get("batch_start")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0);
    let batch_count = payload
        .get("batch_count")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0);
    let history_prefetch_limit = payload
        .get("prefetch_history_limit")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0)
        .min(MARKET_REFRESH_HISTORY_PREFETCH_LIMIT);
    emit_market_refresh_log(
        &app,
        "command_received",
        "info",
        json!({
            "seed_stock_count": seed.get("stocks").and_then(Value::as_array).map(|stocks| stocks.len()).unwrap_or(0),
            "scan_candidates": scan_candidates,
            "max_candidates": max_candidates,
            "batch_start": batch_start,
            "batch_count": batch_count,
            "prefetch_history_limit": history_prefetch_limit,
        }),
    );
    let reference_refresh = if batch_start == 0 && scan_candidates {
        let summary = crate::cn_market::refresh_reference_data(&app, &mut seed, &payload).await?;
        seed["cn_reference"]["last_refresh"] = summary.clone();
        if summary
            .get("financial_fetched")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            > 0
            || summary
                .get("directory_count")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                > 0
        {
            // Financial successes survive a later quote failure or cancellation.
            write_mobile_market_data_record(&app, seed.clone(), false)?;
        }
        summary
    } else {
        seed.pointer("/cn_reference/last_refresh")
            .cloned()
            .unwrap_or(Value::Null)
    };
    let refresh = refresh_tencent_market_data(
        &app,
        seed,
        scan_candidates,
        max_candidates,
        use_previous_close,
        max_failed_batches,
        batch_start,
        batch_count,
        financial_snapshot,
        history_prefetch_limit,
        Some(&payload),
    )
    .await?;
    emit_market_refresh_log(
        &app,
        "command_complete",
        "ok",
        json!({
            "fetched": refresh.fetched,
            "preserved": refresh.preserved,
            "failed_batches": refresh.failed_batches,
            "empty_batches": refresh.empty_batches,
            "next_batch_start": refresh.next_batch_start,
            "total_batches": refresh.total_batches,
            "done": refresh.done,
            "history_prefetched": refresh.history_prefetched,
            "history_failed": refresh.history_failed,
        }),
    );
    let cache = write_mobile_market_data_record(&app, refresh.dataset, false)?;
    let mut notes = vec![format!(
        "Tencent quote refresh finished: fetched {} of {} candidates, preserved {} local rows",
        refresh.fetched, refresh.requested, refresh.preserved
    )];
    match refresh.stop_reason.as_deref() {
        Some("failed_batches") => {
            notes.push(format!(
                "Tencent quote refresh stopped early after {} failed batches",
                refresh.failed_batches
            ));
        }
        _ => {}
    }
    if !refresh.error_samples.is_empty() {
        notes.push(format!(
            "Recent Tencent network errors: {}",
            refresh.error_samples.join(" | ")
        ));
    }
    if refresh.history_prefetched > 0 || refresh.history_failed > 0 {
        notes.push(format!(
            "Research history warm-up finished: {} symbols ready, {} failed",
            refresh.history_prefetched, refresh.history_failed
        ));
    }
    Ok(json!({
        "refreshed": true,
        "source": "tencent",
        "requested": refresh.requested,
        "fetched": refresh.fetched,
        "preserved": refresh.preserved,
        "failed_batches": refresh.failed_batches,
        "empty_batches": refresh.empty_batches,
        "error_samples": refresh.error_samples.clone(),
        "stopped_early": refresh.stopped_early,
        "stop_reason": refresh.stop_reason,
        "batch_start": refresh.batch_start,
        "batch_count": refresh.batch_count,
        "next_batch_start": refresh.next_batch_start,
        "total_batches": refresh.total_batches,
        "done": refresh.done,
        "processed_codes": refresh.processed_codes,
        "total_candidates": refresh.total_candidates,
        "history_prefetched": refresh.history_prefetched,
        "history_failed": refresh.history_failed,
        "reference_refresh": reference_refresh,
        "status": cache,
        "notes": notes
    }))
}

pub(crate) fn core_mobile_market_data_ingest_tencent_quotes(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let seed = refresh_seed_payload(&app, &payload);
    let financial_snapshot = refresh_financial_snapshot_payload(&app, &payload);
    let scan_candidates = payload
        .get("scan_candidates")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let use_previous_close = payload
        .get("use_previous_close")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let max_candidates = payload
        .get("max_candidates")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or(TENCENT_DEFAULT_MAX_CANDIDATES);
    let batch_start = payload
        .get("batch_start")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0);
    let batch_count = payload
        .get("batch_count")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0);
    let quote_bytes = payload
        .get("quote_bytes_base64")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(|encoded| {
            if encoded.len() > MAX_TENCENT_WEBVIEW_QUOTE_BYTES * 2 {
                return Err("Tencent WebView quote payload is too large".to_string());
            }
            let bytes = general_purpose::STANDARD
                .decode(encoded)
                .map_err(|error| format!("decode Tencent WebView quote bytes failed: {error}"))?;
            if bytes.len() > MAX_TENCENT_WEBVIEW_QUOTE_BYTES {
                return Err("Tencent WebView quote payload is too large".to_string());
            }
            Ok(bytes)
        })
        .transpose()?;
    let quote_text = if let Some(bytes) = quote_bytes.as_ref() {
        let (text, _, _) = encoding_rs::GBK.decode(bytes);
        text.into_owned()
    } else {
        let text = payload
            .get("quote_text")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing Tencent quote text from WebView".to_string())?
            .to_string();
        if text.len() > MAX_TENCENT_WEBVIEW_QUOTE_BYTES {
            return Err("Tencent WebView quote payload is too large".to_string());
        }
        text
    };
    if quote_text.trim().is_empty() {
        return Err("Tencent quote text from WebView is empty".to_string());
    }
    let webview_status = payload
        .get("webview_status")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .unwrap_or(200);
    let webview_byte_len = payload
        .get("webview_byte_len")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .or_else(|| quote_bytes.as_ref().map(Vec::len))
        .unwrap_or_else(|| quote_text.len());
    let webview_elapsed_ms = payload
        .get("webview_elapsed_ms")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    emit_market_refresh_log(
        &app,
        "webview_ingest_received",
        "info",
        json!({
            "seed_stock_count": seed.get("stocks").and_then(Value::as_array).map(|stocks| stocks.len()).unwrap_or(0),
            "scan_candidates": scan_candidates,
            "max_candidates": max_candidates,
            "batch_start": batch_start,
            "batch_count": batch_count,
            "webview_status": webview_status,
            "webview_byte_len": webview_byte_len,
            "webview_elapsed_ms": webview_elapsed_ms,
        }),
    );

    let refresh = ingest_tencent_market_data(
        &app,
        seed,
        scan_candidates,
        max_candidates,
        use_previous_close,
        batch_start,
        batch_count,
        &quote_text,
        webview_status,
        webview_byte_len,
        webview_elapsed_ms,
        financial_snapshot,
    )?;
    emit_market_refresh_log(
        &app,
        "command_complete",
        "ok",
        json!({
            "fetched": refresh.fetched,
            "preserved": refresh.preserved,
            "failed_batches": refresh.failed_batches,
            "empty_batches": refresh.empty_batches,
            "next_batch_start": refresh.next_batch_start,
            "total_batches": refresh.total_batches,
            "done": refresh.done,
            "history_prefetched": refresh.history_prefetched,
            "history_failed": refresh.history_failed,
        }),
    );
    let cache = write_mobile_market_data_record(&app, refresh.dataset, false)?;
    Ok(json!({
        "refreshed": true,
        "source": "tencent-webview",
        "requested": refresh.requested,
        "fetched": refresh.fetched,
        "preserved": refresh.preserved,
        "failed_batches": refresh.failed_batches,
        "empty_batches": refresh.empty_batches,
        "error_samples": refresh.error_samples.clone(),
        "stopped_early": refresh.stopped_early,
        "stop_reason": refresh.stop_reason,
        "batch_start": refresh.batch_start,
        "batch_count": refresh.batch_count,
        "next_batch_start": refresh.next_batch_start,
        "total_batches": refresh.total_batches,
        "done": refresh.done,
        "processed_codes": refresh.processed_codes,
        "total_candidates": refresh.total_candidates,
        "history_prefetched": refresh.history_prefetched,
        "history_failed": refresh.history_failed,
        "status": cache,
        "notes": [format!(
            "Tencent WebView quote ingest finished: fetched {} of {} candidates, preserved {} local rows",
            refresh.fetched, refresh.requested, refresh.preserved
        )]
    }))
}

pub(crate) struct TencentRefreshResult {
    pub(crate) dataset: Value,
    pub(crate) requested: usize,
    pub(crate) fetched: usize,
    pub(crate) preserved: usize,
    pub(crate) failed_batches: usize,
    pub(crate) empty_batches: usize,
    pub(crate) error_samples: Vec<String>,
    pub(crate) stopped_early: bool,
    pub(crate) stop_reason: Option<String>,
    pub(crate) batch_start: usize,
    pub(crate) batch_count: usize,
    pub(crate) next_batch_start: usize,
    pub(crate) total_batches: usize,
    pub(crate) done: bool,
    pub(crate) processed_codes: usize,
    pub(crate) total_candidates: usize,
    pub(crate) history_prefetched: usize,
    pub(crate) history_failed: usize,
}

pub(crate) async fn refresh_tencent_market_data(
    app: &tauri::AppHandle,
    seed: Value,
    scan_candidates: bool,
    max_candidates: usize,
    use_previous_close: bool,
    max_failed_batches: usize,
    batch_start: usize,
    batch_count: Option<usize>,
    financial_snapshot: Value,
    history_prefetch_limit: usize,
    network_payload: Option<&Value>,
) -> Result<TencentRefreshResult, String> {
    let (seed_stocks, seed_codes) = seed_stock_maps(&seed);
    let enriched_stocks = enriched_stock_maps(&seed_stocks, &financial_snapshot);
    let candidate_codes = crate::cn_market::directory_candidate_codes(&seed)
        .map(|codes| codes.into_iter().take(max_candidates).collect())
        .unwrap_or_else(|| build_candidate_codes(&seed_codes, scan_candidates, max_candidates));
    if candidate_codes.is_empty() {
        return Err("mobile market refresh has no candidate stock codes".to_string());
    }

    let client = build_http_client_with_proxy(
        "Mozilla/5.0 GuXuanYou/0.3 mobile",
        Duration::from_secs(TENCENT_REQUEST_TIMEOUT_SECS),
        network_payload,
    )?;

    emit_market_refresh_log(
        app,
        "candidate_ready",
        "info",
        json!({
            "candidate_count": candidate_codes.len(),
            "seed_candidate_count": seed_stocks.len(),
            "scan_candidates": scan_candidates,
        }),
    );

    let (normalized_batch_start, batch_end, total_batches) =
        candidate_batch_window(candidate_codes.len(), batch_start, batch_count);
    let effective_batch_count = batch_end.saturating_sub(normalized_batch_start);
    let requested_codes: Vec<String> = candidate_codes
        .chunks(TENCENT_BATCH_SIZE)
        .skip(normalized_batch_start)
        .take(effective_batch_count)
        .flat_map(|chunk| chunk.iter().cloned())
        .collect();

    let mut stocks = Vec::new();
    let mut seen = HashSet::new();
    let mut failed_batches = 0usize;
    let mut empty_batches = 0usize;
    let mut error_samples = Vec::new();
    let mut stopped_early = false;
    let mut stop_reason = None;
    let request_timeout = Duration::from_secs(TENCENT_REQUEST_TIMEOUT_SECS);
    let batch_timeout = Duration::from_secs(TENCENT_BATCH_TIMEOUT_SECS);
    emit_market_refresh_log(
        app,
        "batch_window",
        "info",
        json!({
            "batch_start": normalized_batch_start,
            "batch_end": batch_end,
            "batch_count": effective_batch_count,
            "total_batches": total_batches,
            "request_timeout_seconds": request_timeout.as_secs(),
            "batch_timeout_seconds": batch_timeout.as_secs(),
        }),
    );
    // Fetch every batch in the window concurrently (bounded by TENCENT_FETCH_CONCURRENCY)
    // instead of awaiting them one at a time. qt.gtimg.cn tolerates parallel requests, so
    // wall-clock collapses from sum-of-batches to roughly slowest-batch * ceil(n / concurrency).
    // Codes are cloned into owned Vecs so the per-batch future captures no borrowed slice
    // (a borrowed `&[String]` trips higher-ranked lifetime inference inside the stream).
    let window: Vec<(usize, Vec<String>)> = candidate_codes
        .chunks(TENCENT_BATCH_SIZE)
        .enumerate()
        .skip(normalized_batch_start)
        .take(effective_batch_count)
        .map(|(offset, batch)| (offset, batch.to_vec()))
        .collect();
    let mut fetch_results: Vec<(usize, Result<(TencentQuotePayload, u128), (String, u128)>)> =
        stream::iter(window)
            .map(|(offset, batch)| {
                let client = &client;
                async move {
                    let batch_started_at = epoch_millis();
                    emit_market_refresh_log(
                        app,
                        "batch_request",
                        "info",
                        json!({
                            "batch_index": offset + 1,
                            "total_batches": total_batches,
                            "code_count": batch.len(),
                            "first_code": batch.first(),
                            "last_code": batch.last(),
                        }),
                    );
                    let fetch_result = match tokio::time::timeout(
                        batch_timeout,
                        fetch_tencent_quotes(client, &batch, request_timeout),
                    )
                    .await
                    {
                        Ok(result) => result,
                        Err(_) => Err(format!(
                            "Tencent quote batch timed out after {} seconds",
                            batch_timeout.as_secs()
                        )),
                    };
                    let elapsed_ms = epoch_millis().saturating_sub(batch_started_at);
                    let timed_result = fetch_result
                        .map(|payload| (payload, elapsed_ms))
                        .map_err(|error| (error, elapsed_ms));
                    (offset, timed_result)
                }
            })
            .buffer_unordered(TENCENT_FETCH_CONCURRENCY)
            .collect()
            .await;

    // The whole window is attempted, so advance the cursor to its end up front.
    let next_batch_start = batch_end;
    // Process responses in deterministic batch order so dedup and logs stay stable.
    fetch_results.sort_by_key(|(offset, _)| *offset);
    for (offset, fetch_result) in fetch_results {
        match fetch_result {
            Ok((payload, elapsed_ms)) => {
                let parsed_stocks =
                    parse_tencent_quotes(&payload.text, &enriched_stocks, use_previous_close);
                if parsed_stocks.is_empty() {
                    empty_batches += 1;
                }
                emit_market_refresh_log(
                    app,
                    "batch_response",
                    if parsed_stocks.is_empty() {
                        "warn"
                    } else {
                        "ok"
                    },
                    json!({
                        "batch_index": offset + 1,
                        "status": payload.status,
                        "transport": payload.transport,
                        "byte_len": payload.byte_len,
                        "parsed_count": parsed_stocks.len(),
                        "elapsed_ms": elapsed_ms,
                        "sample": payload.text.chars().take(120).collect::<String>(),
                    }),
                );
                for mut stock in parsed_stocks {
                    let Some(code) = stock
                        .get("code")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                    else {
                        continue;
                    };
                    if seen.insert(code) {
                        stocks.push(Value::Object(std::mem::take(&mut stock)));
                    }
                }
            }
            Err((error, elapsed_ms)) => {
                emit_market_refresh_log(
                    app,
                    "batch_error",
                    "error",
                    json!({
                        "batch_index": offset + 1,
                        "elapsed_ms": elapsed_ms,
                        "error": error,
                    }),
                );
                if error_samples.len() < 3 {
                    error_samples.push(error);
                }
                failed_batches += 1;
            }
        }
    }

    // With concurrent fetching the entire window is attempted before we decide anything, so
    // "early stop" becomes a post-hoc check: only bail when the window saw enough failures and
    // produced no fresh quotes at all (a network-down signal). The front-end then stops paging.
    if failed_batches >= max_failed_batches && seen.is_empty() {
        stopped_early = true;
        stop_reason = Some("failed_batches".to_string());
    }
    let fetched = seen.len();
    let preserved =
        append_all_preserved_seed_stocks(&seed_stocks, &enriched_stocks, &mut stocks, &mut seen);
    if stocks.is_empty() {
        let suffix = if error_samples.is_empty() {
            String::new()
        } else {
            format!("; recent errors: {}", error_samples.join(" | "))
        };
        return Err(format!(
            "Tencent quote refresh returned no valid stocks{suffix}"
        ));
    }
    stocks.sort_by(|left, right| {
        left.get("code")
            .and_then(Value::as_str)
            .unwrap_or("")
            .cmp(right.get("code").and_then(Value::as_str).unwrap_or(""))
    });
    let valid_codes: HashSet<String> = stocks
        .iter()
        .filter_map(|stock| {
            stock
                .get("code")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect();

    let (history_patch, history_prefetched, history_failed) =
        if history_prefetch_limit > 0 && (batch_end >= total_batches || stopped_early) {
            prefetch_market_refresh_histories(&seed, &stocks, history_prefetch_limit).await
        } else {
            (serde_json::Map::new(), 0, 0)
        };
    let mut histories = seed.get("histories").cloned().unwrap_or_else(|| json!({}));
    if let Some(items) = histories.as_object_mut() {
        items.extend(history_patch);
    }

    let mut dataset = json!({
        "source": "tencent",
        "generated_at": epoch_millis().to_string(),
        "generated_at_epoch_ms": epoch_millis(),
        "notes": [
            "mobile online refresh via Tencent quote",
            "industry and slow-changing metrics are merged from the previous local dataset",
            if use_previous_close {
                "price policy: previous close before market close"
            } else {
                "price policy: latest Tencent quote"
            }
        ],
        "stocks": stocks,
        "relations": filter_seed_relations(&seed, &valid_codes),
        "histories": histories,
        "financials": filtered_financial_snapshot_map(&seed, &financial_snapshot, &valid_codes)
    });
    for field in [
        "cn_reference",
        "archived_stocks",
        "factor_snapshots",
        "capital_evidence",
    ] {
        if let Some(value) = seed.get(field) {
            dataset[field] = value.clone();
        }
    }
    if let Some(entries) = seed.get("financials").and_then(Value::as_object) {
        for (code, entry) in entries {
            if !valid_codes.contains(code) {
                dataset["financials"][code] = entry.clone();
            }
        }
    }

    let done = stopped_early || next_batch_start >= total_batches;
    Ok(TencentRefreshResult {
        requested: candidate_codes.len(),
        fetched,
        preserved,
        failed_batches,
        empty_batches,
        error_samples,
        stopped_early,
        stop_reason,
        batch_start: normalized_batch_start,
        batch_count: effective_batch_count,
        next_batch_start,
        total_batches,
        done,
        processed_codes: requested_codes.len(),
        total_candidates: candidate_codes.len(),
        history_prefetched,
        history_failed,
        dataset,
    })
}

pub(crate) fn ingest_tencent_market_data(
    app: &tauri::AppHandle,
    seed: Value,
    scan_candidates: bool,
    max_candidates: usize,
    use_previous_close: bool,
    batch_start: usize,
    batch_count: Option<usize>,
    quote_text: &str,
    webview_status: u16,
    webview_byte_len: usize,
    webview_elapsed_ms: u64,
    financial_snapshot: Value,
) -> Result<TencentRefreshResult, String> {
    let (seed_stocks, seed_codes) = seed_stock_maps(&seed);
    let enriched_stocks = enriched_stock_maps(&seed_stocks, &financial_snapshot);
    let candidate_codes = crate::cn_market::directory_candidate_codes(&seed)
        .map(|codes| codes.into_iter().take(max_candidates).collect())
        .unwrap_or_else(|| build_candidate_codes(&seed_codes, scan_candidates, max_candidates));
    if candidate_codes.is_empty() {
        return Err("mobile market refresh has no candidate stock codes".to_string());
    }

    emit_market_refresh_log(
        app,
        "candidate_ready",
        "info",
        json!({
            "candidate_count": candidate_codes.len(),
            "seed_candidate_count": seed_stocks.len(),
            "scan_candidates": scan_candidates,
        }),
    );

    let (normalized_batch_start, batch_end, total_batches) =
        candidate_batch_window(candidate_codes.len(), batch_start, batch_count);
    let effective_batch_count = batch_end.saturating_sub(normalized_batch_start);
    let requested_codes: Vec<String> = candidate_codes
        .chunks(TENCENT_BATCH_SIZE)
        .skip(normalized_batch_start)
        .take(effective_batch_count)
        .flat_map(|chunk| chunk.iter().cloned())
        .collect();

    emit_market_refresh_log(
        app,
        "batch_window",
        "info",
        json!({
            "batch_start": normalized_batch_start,
            "batch_end": batch_end,
            "batch_count": effective_batch_count,
            "total_batches": total_batches,
            "request_timeout_seconds": 0,
            "batch_timeout_seconds": 0,
            "transport": "webview",
        }),
    );

    let parsed_stocks = parse_tencent_quotes(quote_text, &enriched_stocks, use_previous_close);
    let empty_batches = if parsed_stocks.is_empty() && effective_batch_count > 0 {
        effective_batch_count
    } else {
        0
    };
    emit_market_refresh_log(
        app,
        "batch_response",
        if parsed_stocks.is_empty() {
            "warn"
        } else {
            "ok"
        },
        json!({
            "batch_index": normalized_batch_start + 1,
            "status": webview_status,
            "byte_len": webview_byte_len,
            "parsed_count": parsed_stocks.len(),
            "elapsed_ms": webview_elapsed_ms,
            "transport": "webview",
            "sample": quote_text.chars().take(120).collect::<String>(),
        }),
    );

    let requested_set: HashSet<String> = requested_codes.iter().cloned().collect();
    let mut stocks = Vec::new();
    let mut seen = HashSet::new();
    let mut ignored_quote_codes = Vec::new();
    for mut stock in parsed_stocks {
        let Some(code) = stock
            .get("code")
            .and_then(Value::as_str)
            .and_then(normalize_stock_code)
        else {
            continue;
        };
        if !requested_set.contains(&code) {
            if ignored_quote_codes.len() < 8 {
                ignored_quote_codes.push(code);
            }
            continue;
        }
        stock.insert("code".to_string(), json!(code));
        let code = stock
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if seen.insert(code) {
            stocks.push(Value::Object(std::mem::take(&mut stock)));
        }
    }
    let fetched = seen.len();
    let missing_requested_count = requested_set.len().saturating_sub(fetched);
    if !ignored_quote_codes.is_empty() || missing_requested_count > 0 {
        emit_market_refresh_log(
            app,
            "webview_batch_mismatch",
            "warn",
            json!({
                "batch_index": normalized_batch_start + 1,
                "requested_count": requested_set.len(),
                "fetched_count": fetched,
                "missing_requested_count": missing_requested_count,
                "ignored_quote_codes": ignored_quote_codes,
                "transport": "webview",
            }),
        );
    }
    let preserved =
        append_all_preserved_seed_stocks(&seed_stocks, &enriched_stocks, &mut stocks, &mut seen);
    if stocks.is_empty() {
        return Err(format!(
            "Tencent WebView quote returned no valid stocks; sample: {}",
            quote_text.chars().take(160).collect::<String>()
        ));
    }
    stocks.sort_by(|left, right| {
        left.get("code")
            .and_then(Value::as_str)
            .unwrap_or("")
            .cmp(right.get("code").and_then(Value::as_str).unwrap_or(""))
    });
    let valid_codes: HashSet<String> = stocks
        .iter()
        .filter_map(|stock| {
            stock
                .get("code")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect();

    let mut dataset = json!({
        "source": "tencent",
        "generated_at": epoch_millis().to_string(),
        "generated_at_epoch_ms": epoch_millis(),
        "notes": [
            "mobile online refresh via WebView Tencent quote",
            "industry and slow-changing metrics are merged from the previous local dataset",
            if use_previous_close {
                "price policy: previous close before market close"
            } else {
                "price policy: latest Tencent quote"
            }
        ],
        "stocks": stocks,
        "relations": filter_seed_relations(&seed, &valid_codes),
        "histories": seed.get("histories").cloned().unwrap_or_else(||json!({})),
        "financials": filtered_financial_snapshot_map(&seed, &financial_snapshot, &valid_codes)
    });

    for field in [
        "cn_reference",
        "archived_stocks",
        "factor_snapshots",
        "capital_evidence",
    ] {
        if let Some(value) = seed.get(field) {
            dataset[field] = value.clone();
        }
    }
    if let Some(entries) = seed.get("financials").and_then(Value::as_object) {
        for (code, entry) in entries {
            if !valid_codes.contains(code) {
                dataset["financials"][code] = entry.clone();
            }
        }
    }

    Ok(TencentRefreshResult {
        requested: candidate_codes.len(),
        fetched,
        preserved,
        failed_batches: 0,
        empty_batches,
        error_samples: Vec::new(),
        stopped_early: false,
        stop_reason: None,
        batch_start: normalized_batch_start,
        batch_count: effective_batch_count,
        next_batch_start: batch_end,
        total_batches,
        done: batch_end >= total_batches,
        processed_codes: requested_codes.len(),
        total_candidates: candidate_codes.len(),
        history_prefetched: 0,
        history_failed: 0,
        dataset,
    })
}

pub(crate) fn market_refresh_history_codes(stocks: &[Value], limit: usize) -> Vec<String> {
    if limit == 0 {
        return adaptive_benchmark_codes()
            .into_iter()
            .map(str::to_string)
            .collect();
    }
    let typed = serde_json::from_value::<Vec<gp_core::StockItem>>(Value::Array(stocks.to_vec()))
        .unwrap_or_default();
    let mut codes =
        gp_core::adaptive_candidate_codes(&typed, &gp_core::ScreenCriteria::default(), limit);
    let mut trend_criteria = gp_core::ScreenCriteria::default();
    trend_criteria.limit = limit.clamp(1, 100);
    codes.extend(
        gp_core::screen_stocks(&typed, &trend_criteria)
            .items
            .into_iter()
            .map(|item| item.stock.code),
    );
    if codes.len() < limit {
        let mut fallback = typed
            .into_iter()
            .filter(|stock| stock.price.is_finite() && stock.price > 0.0)
            .map(|stock| stock.code)
            .collect::<Vec<_>>();
        fallback.sort();
        codes.extend(fallback);
    }
    dedupe_stock_codes(&mut codes);
    codes.truncate(limit.saturating_mul(2).max(limit));
    codes.extend(adaptive_benchmark_codes().into_iter().map(str::to_string));
    dedupe_stock_codes(&mut codes);
    codes
}

pub(crate) async fn prefetch_market_refresh_histories(
    seed: &Value,
    stocks: &[Value],
    limit: usize,
) -> (serde_json::Map<String, Value>, usize, usize) {
    let start_date = market_refresh_history_start_date();
    let end_date = "20501231";
    let fetch_codes = market_refresh_history_codes(stocks, limit)
        .into_iter()
        .filter(|code| {
            !history_cache_has_bars(
                seed,
                code,
                &start_date,
                end_date,
                MARKET_REFRESH_HISTORY_MIN_BARS,
            )
        })
        .collect::<Vec<_>>();
    if fetch_codes.is_empty() {
        return (serde_json::Map::new(), 0, 0);
    }

    let results = stream::iter(fetch_codes)
        .map(|code| {
            let start_date = start_date.clone();
            async move {
                let result = fetch_observe_daily_history(&code, &start_date, end_date).await;
                (code, result)
            }
        })
        .buffer_unordered(MARKET_REFRESH_HISTORY_PREFETCH_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
    let mut patch = serde_json::Map::new();
    let mut failed = 0usize;
    for (code, result) in results {
        match result {
            Ok(rows) if rows.len() >= MARKET_REFRESH_HISTORY_MIN_BARS => {
                patch.insert(code, Value::Array(rows));
            }
            _ => failed += 1,
        }
    }
    let fetched = patch.len();
    (patch, fetched, failed)
}

pub(crate) async fn core_mobile_market_data_refresh_tencent(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    crate::runtime::with_market_refresh_permit(
        "core_mobile_market_data_refresh_tencent",
        core_mobile_market_data_refresh_tencent_inner(app, payload),
    )
    .await
}
