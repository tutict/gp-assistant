mod agent_harness;
mod agent_ledger;
mod app;
mod cn_market;
mod core_api;
mod credentials;
mod diagnostics;
mod durability;
#[cfg(any(test, feature = "eval-replay"))]
pub mod eval_replay;
mod evolution;
mod external_data;
mod gepa_lab;
mod hk_agent_research;
mod hk_market;
mod hk_research;
mod jobs;
mod llm;
mod market;
mod market_capabilities;
mod market_domain;
mod market_events;
mod metric_units;
mod network;
mod news_rag;
mod observe;
mod perf_diagnostics;
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
mod stock_research_runtime;
#[cfg(test)]
mod tests;
mod user_backup;
mod watchlist;
mod wire_values;
mod workspace;

/// Starts the desktop or mobile Tauri application.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    app::run();
}
