mod credentials;
mod diagnostics;
mod durability;
mod jobs;
mod user_backup;
mod workspace;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

static CLOSE_HANDLER_READY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static CLOSE_SAVED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static STARTUP_WARNINGS: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());

#[cfg(not(mobile))]
use tauri::{webview::PageLoadEvent, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_shell::ShellExt;

mod agent_harness;
mod agent_ledger;
mod core_api;
mod evolution;
mod gepa_lab;
mod llm;
mod market;
mod news_rag;
mod observe;
mod prompt_upgrade;
mod rag_pack;
mod research;
#[cfg(target_os = "windows")]
mod research_embeddings;
mod research_import;
mod rig_runtime;
mod runtime;
mod screening;
mod sentiment;
mod sentiment_agent;
mod sentiment_data;
mod watchlist;
#[tauri::command]
fn api_app_close_handler_ready(ready: bool) {
    CLOSE_HANDLER_READY.store(ready, std::sync::atomic::Ordering::SeqCst);
}
#[tauri::command]
fn api_app_confirm_close(app: AppHandle, saved: bool) {
    CLOSE_SAVED.store(saved, std::sync::atomic::Ordering::SeqCst);
    CLOSE_HANDLER_READY.store(false, std::sync::atomic::Ordering::SeqCst);
    app.exit(0);
}
#[cfg(any(test, feature = "eval-replay"))]
pub mod eval_replay;

#[tauri::command]
fn api_health() -> Result<Value, String> {
    let warnings = STARTUP_WARNINGS
        .lock()
        .map_err(|_| "startup status unavailable")?
        .clone();
    Ok(
        json!({"status": if warnings.is_empty() { "ok" } else { "degraded" }, "runtime": "tauri", "warnings": warnings}),
    )
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
    let builder = tauri::Builder::default();
    #[cfg(not(mobile))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
    }));
    #[cfg(not(mobile))]
    let builder = builder.on_window_event(|window, event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            if window.label() == "main"
                && CLOSE_HANDLER_READY.load(std::sync::atomic::Ordering::SeqCst)
            {
                api.prevent_close();
                let _ = tauri::Emitter::emit(window.app_handle(), "client-close-requested", ());
            }
        }
    });
    let builder = builder
        .plugin(credentials::init())
        .invoke_handler(tauri::generate_handler![
            api_app_close_handler_ready,
            api_app_confirm_close,
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
            watchlist::api_watchlist_snapshot,
            watchlist::api_watchlist_mutate,
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
            gepa_lab::api_agent_gepa_status,
            gepa_lab::api_agent_gepa_start,
            gepa_lab::api_agent_gepa_cancel,
            gepa_lab::api_agent_gepa_report,
            gepa_lab::api_agent_gepa_apply,
            rig_runtime::api_agent_run_list,
            rig_runtime::api_agent_run_metrics,
            rig_runtime::api_agent_run_get,
            rig_runtime::api_agent_run_delete_conversation,
            jobs::api_job_run,
            jobs::api_job_status,
            jobs::api_job_cancel,
            workspace::api_workspace_load,
            workspace::api_workspace_commit,
            diagnostics::api_diagnostics_status,
            diagnostics::api_diagnostics_preview,
            diagnostics::api_diagnostics_set_gepa,
            user_backup::api_user_backup_export,
            user_backup::api_user_backup_preview,
            user_backup::api_user_backup_stage,
            user_backup::api_user_backup_restore_empty,
            user_backup::api_user_backup_merge_agent,
            credentials::api_credential_put,
            credentials::api_credential_status,
            credentials::api_credential_delete,
            evolution::api_evolution_settings,
            evolution::api_evolution_profile,
            evolution::api_evolution_profile_reset,
            evolution::api_evolution_review,
            evolution::api_evolution_review_enhance,
            evolution::api_evolution_confirm_rule,
            evolution::api_evolution_edit_rule,
            evolution::api_evolution_rule_status,
            evolution::api_evolution_delete_rule,
            evolution::api_evolution_suppress_kind,
            evolution::api_evolution_unsuppress_kind,
            evolution::api_sentiment_strategy_save,
            evolution::api_sentiment_strategies,
            evolution::api_sentiment_strategy_versions,
            evolution::api_sentiment_strategy_status,
            evolution::api_sentiment_strategy_rollback,
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
        setup_reliability(app.handle());
        research::schedule_research_maintenance(app.handle().clone());
        Ok(())
    });

    builder
        .build(tauri::generate_context!())
        .expect("error while building 股选优")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit)
                && CLOSE_SAVED.load(std::sync::atomic::Ordering::SeqCst)
                && !jobs::has_active_work(app)
            {
                let _ = diagnostics::mark_clean_shutdown();
            }
        });
}

#[cfg(not(mobile))]
fn setup_desktop(app: &mut tauri::App) -> tauri::Result<()> {
    setup_reliability(app.handle());
    WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("股选优")
        .inner_size(1280.0, 860.0)
        .min_inner_size(960.0, 680.0)
        .visible(false)
        .on_page_load(|window, payload| {
            if matches!(payload.event(), PageLoadEvent::Started) {
                CLOSE_HANDLER_READY.store(false, std::sync::atomic::Ordering::SeqCst);
            }
            if matches!(payload.event(), PageLoadEvent::Finished) {
                let _ = window.show();
                let _ = window.set_focus();
            }
        })
        .build()?;

    research::schedule_research_maintenance(app.handle().clone());
    gepa_lab::maybe_start_headless_from_env(app.handle().clone());

    Ok(())
}

fn setup_reliability(app: &tauri::AppHandle) {
    // A damaged optional store must not turn the entire local workbench into a blank window.
    let diagnostics_ok = app
        .path()
        .app_data_dir()
        .ok()
        .is_some_and(|root| diagnostics::init(&root).is_ok());
    if !diagnostics_ok {
        if let Ok(mut warnings) = STARTUP_WARNINGS.lock() {
            warnings.push("diagnostics_storage_unavailable");
        }
    }
    if jobs::init(app).is_err() {
        if let Ok(mut warnings) = STARTUP_WARNINGS.lock() {
            warnings.push("jobs_storage_unavailable");
        }
        let _ = diagnostics::record(diagnostics::OperationalEvent::StorageCheckFailed);
    }
}

#[cfg(test)]
mod tests;
