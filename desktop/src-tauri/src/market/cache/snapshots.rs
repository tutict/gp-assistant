use crate::market::cache::records::read_mobile_market_data_record;
use crate::market::cache::refresh_financial_snapshot_cache;
use crate::market::cache::refresh_seed_cache;
use crate::market::cache::screen_stock_overlay_cache;
use crate::market::store::files::mobile_market_data_path;
use crate::market_domain::datasets::market_data_payload_present;
use crate::market_domain::financial::financial_snapshot_payload_present;
use serde_json::json;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

pub(crate) fn cache_context_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    mobile_market_data_path(app).ok()
}

pub(crate) fn remember_refresh_seed(app: &tauri::AppHandle, seed: &Value) {
    if !market_data_payload_present(seed) {
        return;
    }
    let Some(path) = cache_context_path(app) else {
        return;
    };
    if let Ok(mut slot) = refresh_seed_cache().lock() {
        slot.insert(path, seed.clone());
    }
}

pub(crate) fn clear_refresh_seed(app: &tauri::AppHandle) {
    if let Some(path) = cache_context_path(app) {
        if let Ok(mut slot) = refresh_seed_cache().lock() {
            slot.remove(&path);
        }
        if let Ok(mut slot) = refresh_financial_snapshot_cache().lock() {
            slot.remove(&path);
        }
        if let Ok(mut slot) = screen_stock_overlay_cache().lock() {
            slot.remove(&path);
        }
    } else {
        if let Ok(mut slot) = refresh_seed_cache().lock() {
            slot.clear();
        }
        if let Ok(mut slot) = refresh_financial_snapshot_cache().lock() {
            slot.clear();
        }
        if let Ok(mut slot) = screen_stock_overlay_cache().lock() {
            slot.clear();
        }
    }
}

pub(crate) fn remember_refresh_financial_snapshot(app: &tauri::AppHandle, snapshot: &Value) {
    if !financial_snapshot_payload_present(snapshot) {
        return;
    }
    let Some(path) = cache_context_path(app) else {
        return;
    };
    if let Ok(mut slot) = refresh_financial_snapshot_cache().lock() {
        slot.insert(path, Arc::new(snapshot.clone()));
    }
}

pub(crate) fn refresh_financial_snapshot_payload(app: &tauri::AppHandle, payload: &Value) -> Value {
    let snapshot = payload
        .get("financial_snapshot")
        .cloned()
        .unwrap_or(Value::Null);
    if financial_snapshot_payload_present(&snapshot) {
        remember_refresh_financial_snapshot(app, &snapshot);
        return snapshot;
    }
    let Some(path) = cache_context_path(app) else {
        return Value::Null;
    };
    if let Ok(slot) = refresh_financial_snapshot_cache().lock() {
        if let Some(cached) = slot.get(&path) {
            if financial_snapshot_payload_present(cached) {
                return cached.as_ref().clone();
            }
        }
    }
    Value::Null
}

pub(crate) fn refresh_seed_payload(app: &tauri::AppHandle, payload: &Value) -> Value {
    let seed = payload.get("seed").cloned().unwrap_or(Value::Null);
    if market_data_payload_present(&seed) {
        remember_refresh_seed(app, &seed);
        return seed;
    }
    if let Some(path) = cache_context_path(app) {
        if let Ok(slot) = refresh_seed_cache().lock() {
            if let Some(cached) = slot.get(&path) {
                if market_data_payload_present(cached) {
                    return cached.clone();
                }
            }
        }
    }
    let cached = read_mobile_market_data_record(app, true)
        .ok()
        .and_then(|record| record.get("data").cloned())
        .unwrap_or_else(|| json!({}));
    remember_refresh_seed(app, &cached);
    cached
}
