use serde_json::Value;

pub(crate) mod cache;
pub(crate) mod commands;
pub(crate) mod providers;
pub(crate) mod refresh;
pub(crate) mod store;
pub(crate) use crate::market::cache::records::cached_market_data;
pub(crate) use crate::market::cache::records::cached_market_data_snapshot;
pub(crate) use crate::market::cache::records::cached_market_data_views;
pub(crate) use crate::market::cache::records::market_data_status;
pub(crate) use crate::market::cache::records::persist_market_data_patch_updates;
pub(crate) use crate::market::refresh::resume_explicit_cn_refreshes;
#[allow(unused_imports)]
pub(crate) use crate::market::store::projection::project_market_store;
#[allow(unused_imports)]
pub(crate) use crate::market::store::projection::store_market_dataset;

#[tauri::command]
pub(crate) async fn api_market_status(app: tauri::AppHandle) -> Result<Value, String> {
    commands::api_market_status(app).await
}

#[tauri::command]
pub(crate) async fn api_market_refresh(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    commands::api_market_refresh(app, payload).await
}

#[tauri::command]
pub(crate) fn api_market_ingest_tencent_quotes(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    commands::api_market_ingest_tencent_quotes(app, payload)
}

#[tauri::command]
pub(crate) fn api_market_clear_cache(app: tauri::AppHandle) -> Result<Value, String> {
    commands::api_market_clear_cache(app)
}

#[tauri::command]
pub(crate) fn api_stock_search(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    commands::api_stock_search(app, payload)
}

#[tauri::command]
pub(crate) fn api_data_sources(app: tauri::AppHandle) -> Result<Value, String> {
    commands::api_data_sources(app)
}

#[tauri::command]
pub(crate) fn api_stock_get(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    commands::api_stock_get(app, payload)
}

#[tauri::command]
pub(crate) fn api_minutes(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    commands::api_minutes(app, payload)
}

#[tauri::command]
pub(crate) fn api_order_book(_app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    commands::api_order_book(_app, payload)
}

#[tauri::command]
pub(crate) async fn core_validate_data_source(payload: Value) -> Result<Value, String> {
    commands::core_validate_data_source(payload).await
}

#[tauri::command]
pub(crate) fn core_mobile_market_data_read(app: tauri::AppHandle) -> Result<Value, String> {
    commands::core_mobile_market_data_read(app)
}

#[tauri::command]
pub(crate) fn core_mobile_market_data_write(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    commands::core_mobile_market_data_write(app, payload)
}

#[tauri::command]
pub(crate) fn core_mobile_market_data_clear(app: tauri::AppHandle) -> Result<Value, String> {
    commands::core_mobile_market_data_clear(app)
}

#[tauri::command]
pub(crate) async fn core_mobile_network_probe(payload: Option<Value>) -> Result<Value, String> {
    commands::core_mobile_network_probe(payload).await
}

#[tauri::command]
pub(crate) async fn core_mobile_market_data_refresh_tencent(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    commands::core_mobile_market_data_refresh_tencent(app, payload).await
}
