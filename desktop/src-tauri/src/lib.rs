use serde_json::{json, Value};
use tauri::AppHandle;

#[cfg(not(mobile))]
use tauri::{webview::PageLoadEvent, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_shell::ShellExt;

mod agent_harness;
mod agent_ledger;
mod prompt_upgrade;
mod news_rag;
mod rag_pack;
mod research;
#[cfg(target_os = "windows")]
mod research_embeddings;
mod research_import;
mod rig_runtime;
mod runtime;
mod sentiment;
mod sentiment_agent;
mod sentiment_data;
mod market;
mod observe;
mod screening;
mod llm;
mod watchlist;
mod core_api;

#[tauri::command]
fn api_health() -> Result<Value, String> {
    Ok(json!({"status": "ok", "runtime": "tauri"}))
}

#[tauri::command]
#[allow(deprecated)]
fn open_external_url(app: AppHandle, url: String) -> Result<(), String> {
    let trimmed = url.trim();
    let parsed = reqwest::Url::parse(trimmed).map_err(|_| "来源链接格式不正确".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("只允许打开 http/https 来源链接".to_string());
    }
    app.shell()
        .open(parsed.as_str().to_string(), None)
        .map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            core_api::core_screen,
            core_api::core_screen_with_data,
            core_api::core_graph_screen,
            core_api::core_graph_screen_with_data,
            core_api::core_backtest,
            core_api::core_backtest_with_data,
            core_api::core_trend,
            core_api::core_trend_with_data,
            core_api::core_trend_screen,
            core_api::core_trend_screen_with_data,
            core_api::core_agent,
            core_api::core_agent_with_data,
            core_api::core_mobile_stock_skill,
            api_health,
            screening::api_strategies,
            market::api_market_status,
            market::api_data_sources,
            market::api_market_refresh,
            market::api_market_ingest_tencent_quotes,
            market::api_market_clear_cache,
            screening::api_screen,
            screening::api_sector_screen,
            screening::api_custom_screen,
            screening::api_graph_screen,
            screening::api_trend_analyze,
            screening::api_trend_screen,
            observe::api_observe,
            screening::api_backtest,
            market::api_stock_search,
            market::api_stock_get,
            market::api_minutes,
            market::api_order_book,
            watchlist::api_watchlist_list,
            watchlist::api_watchlist_replace,
            watchlist::api_watchlist_add,
            watchlist::api_watchlist_remove,
            watchlist::api_watchlist_clear,
            news_rag::api_news_rag,
            research::api_research_overview,
            research::api_research_messages,
            research::api_research_mark_read,
            research::api_research_query,
            sentiment::api_sentiment_snapshot,
            sentiment::api_sentiment_start,
            sentiment::api_sentiment_status,
            sentiment::api_sentiment_cancel,
            sentiment::api_sentiment_latest,
            sentiment::api_sentiment_history,
            sentiment::api_sentiment_followup,
            research::api_research_refresh,
            research::api_research_threads,
            research::api_research_thread_create,
            research::api_research_thread_detail,
            research::api_research_thread_delete,
            research::api_research_index_status,
            research::api_research_rebuild_index,
            research::api_research_rebuild_embeddings,
            research_import::api_research_import_url,
            research_import::api_research_import_pdf,
            research::api_research_pack_export,
            research::api_research_pack_import,
            research::api_research_pack_rollback,
            rag_pack::api_rag_pack_status,
            rag_pack::api_rag_pack_build,
            rag_pack::api_rag_pack_build_from_news_cache,
            rag_pack::api_rag_pack_query,
            rag_pack::api_upstream_rag_status,
            rag_pack::api_upstream_rag_build,
            rag_pack::api_upstream_rag_transfer_start,
            rig_runtime::api_agent_stream,
            rig_runtime::api_agent_cancel,
            rig_runtime::api_agent_prompt_overlays,
            rig_runtime::api_agent_prompt_overlay_revert,
            rig_runtime::api_agent_run_list,
            rig_runtime::api_agent_run_metrics,
            rig_runtime::api_agent_run_get,
            rig_runtime::api_agent_run_delete_conversation,
            llm::api_llm_models,
            llm::api_llm_test,
            market::core_validate_data_source,
            market::core_mobile_market_data_read,
            market::core_mobile_market_data_write,
            market::core_mobile_market_data_clear,
            market::core_mobile_network_probe,
            market::core_mobile_market_data_refresh_tencent,
            rag_pack::core_upstream_rag_import,
            rag_pack::core_upstream_rag_list,
            rag_pack::core_upstream_rag_detail,
            rag_pack::core_upstream_rag_rollback,
            open_external_url
        ])
        .plugin(tauri_plugin_shell::init());

    #[cfg(not(mobile))]
    let builder = builder.setup(|app| setup_desktop(app).map_err(Into::into));

    #[cfg(mobile)]
    let builder = builder.setup(|app| {
        research::schedule_research_maintenance(app.handle().clone());
        Ok(())
    });

    builder
        .run(tauri::generate_context!())
        .expect("error while running 股选优");
}

#[cfg(not(mobile))]
fn setup_desktop(app: &mut tauri::App) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("股选优")
        .inner_size(1280.0, 860.0)
        .min_inner_size(960.0, 680.0)
        .visible(false)
        .on_page_load(|window, payload| {
            if matches!(payload.event(), PageLoadEvent::Finished) {
                let _ = window.show();
                let _ = window.set_focus();
            }
        })
        .build()?;

    research::schedule_research_maintenance(app.handle().clone());

    Ok(())
}

#[cfg(test)]
mod tests;
