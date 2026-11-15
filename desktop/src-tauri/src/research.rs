use serde_json::Value;

mod app_store;
mod background;
mod commands;
mod schema;
mod sources;
mod storage;
mod time;
mod validation;
pub(crate) use app_store::market_from_payload;
pub(crate) use app_store::with_app_hk_model_evidence_send;
pub(crate) use app_store::with_app_store_for_market;
pub(crate) use app_store::with_app_store_snapshot_for_market;
#[allow(unused_imports)] // Stable root alias used by feature-gated consumers or tests.
pub(crate) use app_store::with_model_evidence_database_guard;
pub(crate) use background::schedule_research_embeddings;
#[allow(unused_imports)]
pub(crate) use background::schedule_research_embeddings_for_market;
pub(crate) use background::schedule_research_maintenance;
#[cfg(target_os = "windows")]
#[allow(unused_imports)] // Stable root alias used by feature-gated consumers or tests.
pub(crate) use background::ResearchEmbeddingJobGate;
pub(crate) use sources::ingest_news_cache;
pub(crate) use storage::packs::import_app_pack;
#[allow(unused_imports)] // Stable root alias used by feature-gated consumers or tests.
pub(crate) use storage::vectors::finalize_research_index_status;
#[allow(unused_imports)] // Stable root alias used by feature-gated consumers or tests.
pub(crate) use storage::ResearchStore;
pub(crate) use time::normalize_research_as_of;
pub(crate) use time::research_evidence_available_as_of;

#[path = "research_hk_cache.rs"]
mod hk_cache;

#[path = "research_recovery.rs"]
mod recovery;

#[cfg(test)]
#[path = "research_tests.rs"]
mod tests;

#[cfg(target_os = "windows")]
#[cfg(test)]
#[path = "research_embedding_market_gate_tests.rs"]
mod embedding_market_gate_tests;

#[cfg(test)]
#[path = "research_schema_reliability_tests.rs"]
mod schema_reliability_tests;

#[cfg(test)]
#[path = "research_local_schema_migration_tests.rs"]
mod local_schema_migration_tests;

#[tauri::command]
pub(crate) async fn api_research_overview(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    commands::api_research_overview(app, payload).await
}

#[tauri::command]
pub(crate) async fn api_research_messages(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    commands::api_research_messages(app, payload).await
}

#[tauri::command]
pub(crate) async fn api_research_mark_read(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    commands::api_research_mark_read(app, payload).await
}

#[tauri::command]
pub(crate) async fn api_research_query(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    commands::api_research_query(app, payload).await
}

#[tauri::command]
pub(crate) async fn api_research_refresh(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    commands::api_research_refresh(app, payload).await
}

#[tauri::command]
pub(crate) async fn api_research_threads(
    app: tauri::AppHandle,
    market: Option<String>,
) -> Result<Value, String> {
    commands::api_research_threads(app, market).await
}

#[tauri::command]
pub(crate) async fn api_research_thread_create(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    commands::api_research_thread_create(app, payload).await
}

#[tauri::command]
pub(crate) async fn api_research_thread_detail(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    commands::api_research_thread_detail(app, payload).await
}

#[tauri::command]
pub(crate) async fn api_research_thread_delete(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    commands::api_research_thread_delete(app, payload).await
}

#[tauri::command]
pub(crate) async fn api_research_index_status(
    app: tauri::AppHandle,
    market: Option<String>,
) -> Result<Value, String> {
    commands::api_research_index_status(app, market).await
}

#[tauri::command]
pub(crate) async fn api_research_rebuild_index(
    app: tauri::AppHandle,
    payload: Option<Value>,
) -> Result<Value, String> {
    commands::api_research_rebuild_index(app, payload).await
}

#[tauri::command]
pub(crate) async fn api_research_rebuild_embeddings(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    commands::api_research_rebuild_embeddings(app, payload).await
}

#[tauri::command]
pub(crate) async fn api_research_pack_export(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    commands::api_research_pack_export(app, payload).await
}

#[tauri::command]
pub(crate) async fn api_research_pack_import(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    commands::api_research_pack_import(app, payload).await
}

#[tauri::command]
pub(crate) async fn api_research_pack_rollback(
    app: tauri::AppHandle,
    payload: Option<Value>,
) -> Result<Value, String> {
    commands::api_research_pack_rollback(app, payload).await
}
