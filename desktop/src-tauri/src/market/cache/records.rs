use crate::market::cache::cached_external_market_data_entry_for_files;
use crate::market::cache::cached_mobile_market_data_entry;
use crate::market::cache::forget_mobile_market_data_cache;
use crate::market::cache::mobile_market_data_cache;
use crate::market::cache::mobile_market_read_lock;
use crate::market::cache::mobile_market_update_lock;
use crate::market::cache::remember_external_market_data_cache;
use crate::market::cache::remember_mobile_market_data_cache;
use crate::market::cache::snapshots::remember_refresh_seed;
use crate::market::cache::versions::external_market_data_cache_version;
use crate::market::cache::versions::file_modified_millis;
use crate::market::cache::ExternalMarketDataCacheVersion;
use crate::market::cache::MarketDataSnapshot;
use crate::market::store::files::apply_persisted_market_data_patches;
use crate::market::store::files::clear_mobile_market_data_patches;
use crate::market::store::files::mobile_market_data_path;
use crate::market::store::files::persist_market_data_patch_sync;
use crate::market::store::files::read_json_file;
use crate::market::store::files::write_mobile_market_data_with_retry;
use crate::market::store::files::MOBILE_MARKET_DATA_FILE;
use crate::market::store::legacy::all_market_domains_promoted;
use crate::market::store::legacy::import_legacy_market_domains;
use crate::market::store::legacy::import_legacy_market_source;
use crate::market::store::projection::external_market_domains;
use crate::market::store::projection::project_market_store;
use crate::market::store::projection::project_market_store_with_legacy;
use crate::market::store::projection::store_market_dataset;
use crate::market_domain::datasets::apply_market_data_patch;
use crate::market_domain::datasets::market_quote_coverage;
use crate::market_domain::datasets::parse_mobile_market_data_snapshot;
use crate::market_domain::dates::cache_epoch_ms;
use crate::market_domain::dates::epoch_millis;
use crate::market_domain::dates::expected_market_quote_date_with_calendar;
use crate::market_domain::dates::local_yyyymmdd_from_epoch_ms;
use crate::market_domain::dates::market_quote_cache_stale_with_calendar;
use serde_json::json;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use stock_optimizer_core as gp_core;

pub(crate) fn cached_market_data(app: &tauri::AppHandle) -> Result<Value, String> {
    let cache = read_mobile_market_data(app)?;
    if !cache
        .get("exists")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err("股票池为空，请先联网更新股票池。".to_string());
    }
    cache
        .get("data")
        .cloned()
        .ok_or_else(|| "股票池缓存缺少 data 字段，请清理缓存后重新联网更新。".to_string())
}

pub(crate) fn market_data_status(app: &tauri::AppHandle) -> Result<Value, String> {
    let _update_guard = mobile_market_update_lock()
        .lock()
        .map_err(|_| "mobile market update lock is poisoned".to_string())?;
    let cache = read_mobile_market_data_record(app, false)?;
    let exists = cache
        .get("exists")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !exists {
        return Ok(json!({
            "source": "tencent",
            "universe_count": 0,
            "cache_bytes": 0,
            "cache_limit_bytes": 0,
            "universe_updated_at": Value::Null,
            "quote_generated_at": Value::Null,
            "quote_trade_date": Value::Null,
            "current_trade_date": local_yyyymmdd_from_epoch_ms(epoch_millis()),
            "stale": true,
            "policy": { "mode": "empty" },
            "notes": ["mobile market cache is empty; refresh is required"]
        }));
    }
    let summary = cache.get("summary").cloned().unwrap_or_else(|| json!({}));
    let stock_count = summary
        .get("stock_count")
        .and_then(Value::as_u64)
        .or_else(|| cache.get("stock_count").and_then(Value::as_u64))
        .unwrap_or(0);
    let mut notes = vec![format!(
        "Tauri/Rust cached universe currently contains {stock_count} stocks."
    )];
    if let Some(data_notes) = cache.get("data_notes").and_then(Value::as_array) {
        notes.extend(
            data_notes
                .iter()
                .filter_map(Value::as_str)
                .take(2)
                .map(ToOwned::to_owned),
        );
    }
    if let Some(warnings) = summary.get("warnings").and_then(Value::as_array) {
        notes.extend(
            warnings
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned),
        );
    }
    let generated_at_epoch_ms = cache_epoch_ms(cache.get("generated_at_epoch_ms"))
        .or_else(|| cache_epoch_ms(cache.get("generated_at")));
    let updated_at_epoch_ms = cache_epoch_ms(cache.get("updated_at_epoch_ms"));
    let now_epoch_ms = epoch_millis();
    let calendar = cache.pointer("/cn_reference/calendar");
    let quote_date = generated_at_epoch_ms
        .and_then(|epoch| expected_market_quote_date_with_calendar(epoch, calendar));
    let current_date = expected_market_quote_date_with_calendar(now_epoch_ms, calendar);
    let quote_coverage_ratio = cache.get("quote_coverage_ratio").and_then(Value::as_f64);
    let quote_requested = cache
        .get("quote_requested")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let quote_observed = cache
        .get("quote_observed")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let stale = market_quote_cache_stale_with_calendar(
        generated_at_epoch_ms,
        now_epoch_ms,
        quote_coverage_ratio,
        calendar,
    );
    if stale {
        notes.push(format!(
            "cached Tencent quote snapshot is stale or incomplete: {quote_observed}/{quote_requested} same-day changes; refresh before rotation screening"
        ));
    }
    Ok(json!({
        "source": "tencent",
        "universe_count": stock_count,
        "cache_bytes": cache.get("bytes").and_then(Value::as_u64).unwrap_or(0),
        "cache_limit_bytes": 0,
        "universe_updated_at": updated_at_epoch_ms.map(|value| json!(value)).unwrap_or(Value::Null),
        "quote_generated_at": generated_at_epoch_ms.map(|value| json!(value)).unwrap_or(Value::Null),
        "quote_trade_date": quote_date,
        "quote_coverage_trade_date": cache.get("quote_coverage_trade_date").cloned().unwrap_or(Value::Null),
        "quote_requested": quote_requested,
        "quote_observed": quote_observed,
        "quote_coverage_ratio": quote_coverage_ratio.map(|value| json!(value)).unwrap_or(Value::Null),
        "current_trade_date": current_date,
        "stale": stale,
        "policy": { "mode": "tauri_native", "source": "cache" },
        "cn_reference": cache.get("cn_reference").cloned().unwrap_or(Value::Null),
        "financial_updated_at": cache.pointer("/cn_reference/financial/fetched_at").cloned().unwrap_or(Value::Null),
        "notes": notes
    }))
}

pub(crate) fn read_mobile_market_data(app: &tauri::AppHandle) -> Result<Value, String> {
    read_mobile_market_data_record(app, true)
}

pub(crate) fn read_mobile_market_data_record(
    app: &tauri::AppHandle,
    include_data: bool,
) -> Result<Value, String> {
    let _read_guard = mobile_market_read_lock()
        .lock()
        .map_err(|_| "mobile market read lock is poisoned".to_string())?;
    let path = mobile_market_data_path(app)?;
    let external_path = crate::external_data::store_path(
        path.parent()
            .ok_or_else(|| "mobile market cache path has no parent".to_string())?
            .parent()
            .ok_or_else(|| "mobile market cache path has no app data ancestor".to_string())?,
    );
    if external_path.exists() || path.exists() {
        if external_path.exists() {
            if let Some((version, entry)) =
                cached_external_market_data_entry_for_files(&path, &external_path, &path)
            {
                return Ok(market_cache_record(
                    &path,
                    version.external_bytes(),
                    version.legacy_modified_at_epoch_ms(),
                    entry.summary,
                    entry.data.as_ref(),
                    include_data,
                    "external market store loaded from memory",
                ));
            }
        }
        let mut store = crate::external_data::DataStore::open(&external_path)?;
        if path.exists() {
            let migration = import_legacy_market_source(&mut store, &path)?;
            let (promoted, source_checksum_sha256, source_json, normalized) =
                migration.into_parts();
            let mut active_domains = promoted;
            for domain in external_market_domains() {
                if store.domain_has_successful_source(domain.0, domain.1)? {
                    active_domains.insert(domain);
                }
            }
            let legacy_wrapper = source_json.transpose()?;
            let legacy = legacy_wrapper.map(|legacy_wrapper| {
                legacy_wrapper
                    .get("data")
                    .filter(|value| value.is_object())
                    .cloned()
                    .unwrap_or(legacy_wrapper)
            });
            if !normalized {
                if let Some(legacy) = legacy.as_ref() {
                    let _ = parse_mobile_market_data_snapshot(
                        legacy,
                        "legacy mobile market data projection",
                    )?;
                }
            }
            if let Some(data) =
                project_market_store_with_legacy(&store, legacy.as_ref(), &active_domains)?
            {
                let (typed, summary) = parse_mobile_market_data_snapshot(
                    &data,
                    "mixed external and legacy market projection",
                )?;
                let store_bytes = store.store_bytes()?;
                drop(store);
                let version = external_market_data_cache_version(
                    &external_path,
                    &path,
                    Some(&source_checksum_sha256),
                );
                let external_bytes = version
                    .as_ref()
                    .map(ExternalMarketDataCacheVersion::external_bytes)
                    .unwrap_or(store_bytes);
                if let Some(version) = version {
                    remember_external_market_data_cache(&path, version, &data, typed, &summary);
                } else {
                    forget_mobile_market_data_cache(&path);
                }
                let notes = if all_market_domains_promoted(&active_domains) {
                    "external market store loaded"
                } else {
                    "validated external domains loaded; unvalidated domains remain on legacy cache"
                };
                return Ok(market_cache_record(
                    &path,
                    external_bytes,
                    file_modified_millis(&path),
                    summary,
                    &data,
                    include_data,
                    notes,
                ));
            }
            forget_mobile_market_data_cache(&path);
            return Ok(json!({
                "exists": false,
                "bytes": 0,
                "path": path.display().to_string(),
                "notes": ["external market cache is empty; preserved legacy files are recovery copies only"]
            }));
        } else if let Some(data) = project_market_store(&store)? {
            let (typed, summary) =
                parse_mobile_market_data_snapshot(&data, "external market store")?;
            let store_bytes = store.store_bytes()?;
            drop(store);
            let version = external_market_data_cache_version(&external_path, &path, None);
            let external_bytes = version
                .as_ref()
                .map(ExternalMarketDataCacheVersion::external_bytes)
                .unwrap_or(store_bytes);
            if let Some(version) = version {
                remember_external_market_data_cache(&path, version, &data, typed, &summary);
            } else {
                forget_mobile_market_data_cache(&path);
            }
            return Ok(market_cache_record(
                &path,
                external_bytes,
                None,
                summary,
                &data,
                include_data,
                "external market store loaded",
            ));
        } else {
            forget_mobile_market_data_cache(&path);
            return Ok(json!({
                "exists": false,
                "bytes": 0,
                "path": path.display().to_string(),
                "notes": ["external market cache is empty"]
            }));
        }
    }
    if !path.exists() {
        forget_mobile_market_data_cache(&path);
        return Ok(json!({
            "exists": false,
            "bytes": 0,
            "path": path.display().to_string(),
            "notes": ["mobile market cache is empty"]
        }));
    }
    let bytes = fs::metadata(&path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let modified_at_epoch_ms = file_modified_millis(&path);
    if let Some(entry) = cached_mobile_market_data_entry(&path, bytes, modified_at_epoch_ms) {
        return Ok(market_cache_record(
            &path,
            entry.bytes,
            entry.modified_at_epoch_ms,
            entry.summary,
            entry.data.as_ref(),
            include_data,
            "mobile market cache loaded from memory",
        ));
    }

    let _update_guard = mobile_market_update_lock()
        .lock()
        .map_err(|_| "mobile market update lock is poisoned".to_string())?;
    let bytes = fs::metadata(&path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let modified_at_epoch_ms = file_modified_millis(&path);
    if let Some(entry) = cached_mobile_market_data_entry(&path, bytes, modified_at_epoch_ms) {
        return Ok(market_cache_record(
            &path,
            entry.bytes,
            entry.modified_at_epoch_ms,
            entry.summary,
            entry.data.as_ref(),
            include_data,
            "mobile market cache loaded from memory",
        ));
    }

    let mut data = read_json_file(&path)?;
    apply_persisted_market_data_patches(app, &mut data)?;
    let (typed, summary) = parse_mobile_market_data_snapshot(&data, "cached mobile market data")?;
    remember_mobile_market_data_cache(&path, bytes, modified_at_epoch_ms, &data, typed, &summary);
    Ok(market_cache_record(
        &path,
        bytes,
        modified_at_epoch_ms,
        summary,
        &data,
        include_data,
        "mobile market cache loaded",
    ))
}

pub(crate) fn write_mobile_market_data(
    app: &tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    write_mobile_market_data_record(app, payload, true)
}

pub(crate) fn write_mobile_market_data_record(
    app: &tauri::AppHandle,
    payload: Value,
    include_data: bool,
) -> Result<Value, String> {
    let _update_guard = mobile_market_update_lock()
        .lock()
        .map_err(|_| "mobile market update lock is poisoned".to_string())?;
    let (typed, summary) = parse_mobile_market_data_snapshot(&payload, "mobile market data")?;
    let path = mobile_market_data_path(app)?;
    let app_data = path
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "mobile market cache path has no app data ancestor".to_string())?;
    let external_path = crate::external_data::store_path(app_data);
    let mut store = crate::external_data::DataStore::open(&external_path)?;
    if path.exists()
        && !all_market_domains_promoted(&import_legacy_market_domains(&mut store, &path)?)
    {
        return write_mobile_market_data_legacy(app, path, payload, include_data, typed, summary);
    }
    if let Err(error) = store_market_dataset(&mut store, &payload, "tencent") {
        forget_mobile_market_data_cache(&path);
        return Err(error);
    }
    clear_mobile_market_data_patches(app)?;
    let bytes = serde_json::to_vec(&payload)
        .map_err(|error| format!("serialize mobile market data failed: {error}"))?;
    remember_mobile_market_data_cache(
        &path,
        bytes.len() as u64,
        file_modified_millis(&path),
        &payload,
        typed,
        &summary,
    );
    remember_refresh_seed(app, &payload);
    return Ok(market_cache_record(
        &path,
        bytes.len() as u64,
        file_modified_millis(&path),
        summary,
        &payload,
        include_data,
        "external market store written",
    ));
}

fn write_mobile_market_data_legacy(
    app: &tauri::AppHandle,
    path: PathBuf,
    payload: Value,
    include_data: bool,
    typed: Arc<gp_core::CoreDataSet>,
    summary: Value,
) -> Result<Value, String> {
    let root = path
        .parent()
        .ok_or_else(|| "mobile market cache path has no parent".to_string())?;
    fs::create_dir_all(root)
        .map_err(|error| format!("create mobile market cache dir failed: {error}"))?;
    let tmp_path = root.join(format!(
        "{}.tmp-{}",
        MOBILE_MARKET_DATA_FILE,
        epoch_millis()
    ));
    let bytes = serde_json::to_vec(&payload)
        .map_err(|error| format!("serialize mobile market data failed: {error}"))?;
    write_mobile_market_data_with_retry(&tmp_path, &path, &bytes)?;
    clear_mobile_market_data_patches(app)?;
    remember_mobile_market_data_cache(
        &path,
        bytes.len() as u64,
        file_modified_millis(&path),
        &payload,
        typed,
        &summary,
    );
    remember_refresh_seed(app, &payload);
    Ok(market_cache_record(
        &path,
        bytes.len() as u64,
        file_modified_millis(&path),
        summary,
        &payload,
        include_data,
        "mobile market cache written",
    ))
}

pub(crate) fn market_cache_record(
    path: &Path,
    bytes: u64,
    updated_at_epoch_ms: Option<u128>,
    summary: Value,
    data: &Value,
    include_data: bool,
    note: &str,
) -> Value {
    let stock_count = summary
        .get("stock_count")
        .and_then(Value::as_u64)
        .or_else(|| {
            data.get("stocks")
                .and_then(Value::as_array)
                .map(|stocks| stocks.len() as u64)
        })
        .unwrap_or(0);
    let generated_at = data
        .get("generated_at_epoch_ms")
        .cloned()
        .or_else(|| data.get("generated_at").cloned())
        .unwrap_or(Value::Null);
    let data_notes = data.get("notes").cloned().unwrap_or_else(|| json!([]));
    let generated_at_epoch_ms = cache_epoch_ms(Some(&generated_at));
    let quote_coverage_trade_date = generated_at_epoch_ms.and_then(|epoch| {
        expected_market_quote_date_with_calendar(epoch, data.pointer("/cn_reference/calendar"))
    });
    let (quote_requested, quote_observed, quote_coverage_ratio) =
        market_quote_coverage(data, quote_coverage_trade_date.as_deref());
    let mut record = serde_json::Map::new();
    record.insert("exists".to_string(), json!(true));
    record.insert("bytes".to_string(), json!(bytes));
    record.insert("path".to_string(), json!(path.display().to_string()));
    record.insert(
        "updated_at_epoch_ms".to_string(),
        updated_at_epoch_ms
            .map(|value| json!(value))
            .unwrap_or(Value::Null),
    );
    record.insert("summary".to_string(), summary);
    record.insert("stock_count".to_string(), json!(stock_count));
    record.insert("generated_at".to_string(), generated_at);
    record.insert(
        "generated_at_epoch_ms".to_string(),
        generated_at_epoch_ms
            .map(|value| json!(value))
            .unwrap_or(Value::Null),
    );
    record.insert(
        "quote_coverage_trade_date".to_string(),
        quote_coverage_trade_date
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    record.insert("quote_requested".to_string(), json!(quote_requested));
    record.insert("quote_observed".to_string(), json!(quote_observed));
    record.insert(
        "quote_coverage_ratio".to_string(),
        json!(quote_coverage_ratio),
    );
    record.insert("data_notes".to_string(), data_notes);
    if let Some(reference) = data.get("cn_reference") {
        let mut reference = reference.clone();
        if let Some(directory) = reference
            .get_mut("directory")
            .and_then(Value::as_object_mut)
        {
            directory.remove("codes");
        }
        record.insert("cn_reference".to_string(), reference);
    }
    if include_data {
        record.insert("data".to_string(), data.clone());
    }
    record.insert("notes".to_string(), json!([note]));
    Value::Object(record)
}

pub(crate) async fn persist_market_data_patch_updates(
    app: tauri::AppHandle,
    patch: Value,
) -> Result<usize, String> {
    crate::runtime::run_io_bound("persist market data patches", move || {
        let _update_guard = mobile_market_update_lock()
            .lock()
            .map_err(|_| "mobile market update lock is poisoned".to_string())?;
        let written = persist_market_data_patch_sync(&app, &patch)?;
        let path = mobile_market_data_path(&app)?;
        let mut data = if let Ok(slot) = mobile_market_data_cache().lock() {
            slot.get(&path)
                .map(|entry| entry.data.as_ref().clone())
                .unwrap_or(Value::Null)
        } else {
            Value::Null
        };
        if data.is_null() {
            let cached = read_mobile_market_data_record(&app, true)?;
            data = cached
                .get("data")
                .cloned()
                .ok_or_else(|| "external market dataset is empty".to_string())?;
            apply_persisted_market_data_patches(&app, &mut data)?;
        } else {
            apply_market_data_patch(&mut data, &patch);
        }
        let (typed, summary) = parse_mobile_market_data_snapshot(&data, "updated market data")?;
        let external_path = crate::external_data::store_path(
            path.parent()
                .and_then(Path::parent)
                .ok_or_else(|| "mobile market path has no app data ancestor".to_string())?,
        );
        if external_path.exists() {
            let mut store = crate::external_data::DataStore::open(&external_path)?;
            if let Err(error) = store_market_dataset(&mut store, &data, "tencent") {
                forget_mobile_market_data_cache(&path);
                return Err(error);
            }
        } else {
            let bytes = fs::metadata(&path)
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            remember_mobile_market_data_cache(
                &path,
                bytes,
                file_modified_millis(&path),
                &data,
                typed,
                &summary,
            );
        }
        // A subsequent quote refresh must start from the newly prepared history,
        // rather than resurrect an earlier rolling window from its separate seed.
        remember_refresh_seed(&app, &data);
        Ok(written)
    })
    .await?
}

pub(crate) fn cached_market_data_snapshot(
    app: &tauri::AppHandle,
) -> Result<Arc<gp_core::CoreDataSet>, String> {
    Ok(cached_market_data_views(app)?.typed)
}

pub(crate) fn cached_market_data_views(
    app: &tauri::AppHandle,
) -> Result<MarketDataSnapshot, String> {
    let path = mobile_market_data_path(app)?;
    let cache = read_mobile_market_data_record(app, false)?;
    if !cache
        .get("exists")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err("股票池为空，请先联网更新股票池。".to_string());
    }
    let slot = mobile_market_data_cache()
        .lock()
        .map_err(|_| "mobile market cache lock is poisoned".to_string())?;
    slot.get(&path)
        .map(|entry| MarketDataSnapshot {
            data: Arc::clone(&entry.data),
            typed: Arc::clone(&entry.typed),
        })
        .ok_or_else(|| "mobile market typed snapshot is unavailable".to_string())
}
