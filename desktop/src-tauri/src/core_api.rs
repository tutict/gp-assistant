use serde_json::Value;
use stock_optimizer_core as gp_core;

async fn run_core<T, E, F>(label: &'static str, task: F) -> Result<T, String>
where
    E: ToString,
    F: FnOnce() -> Result<T, E> + Send + 'static,
    T: Send + 'static,
{
    crate::runtime::run_cpu_bound(label, move || task().map_err(|error| error.to_string())).await?
}

#[tauri::command]
pub(crate) async fn core_screen(payload: Value) -> Result<Value, String> {
    run_core("core_screen", move || gp_core::screen_value(payload)).await
}

#[tauri::command]
pub(crate) async fn core_screen_with_data(payload: Value) -> Result<Value, String> {
    run_core("core_screen_with_data", move || gp_core::screen_with_data_value(payload)).await
}

#[tauri::command]
pub(crate) async fn core_graph_screen(payload: Value) -> Result<Value, String> {
    run_core("core_graph_screen", move || gp_core::graph_screen_value(payload)).await
}

#[tauri::command]
pub(crate) async fn core_graph_screen_with_data(payload: Value) -> Result<Value, String> {
    run_core("core_graph_screen_with_data", move || gp_core::graph_screen_with_data_value(payload)).await
}

#[tauri::command]
pub(crate) async fn core_backtest(payload: Value) -> Result<Value, String> {
    run_core("core_backtest", move || gp_core::backtest_value(payload)).await
}

#[tauri::command]
pub(crate) async fn core_backtest_with_data(payload: Value) -> Result<Value, String> {
    run_core("core_backtest_with_data", move || gp_core::backtest_with_data_value(payload)).await
}

#[tauri::command]
pub(crate) async fn core_trend(payload: Value) -> Result<Value, String> {
    run_core("core_trend", move || gp_core::trend_value(payload)).await
}

#[tauri::command]
pub(crate) async fn core_trend_with_data(payload: Value) -> Result<Value, String> {
    run_core("core_trend_with_data", move || gp_core::trend_with_data_value(payload)).await
}

#[tauri::command]
pub(crate) async fn core_trend_screen(payload: Value) -> Result<Value, String> {
    run_core("core_trend_screen", move || gp_core::trend_screen_value(payload)).await
}

#[tauri::command]
pub(crate) async fn core_trend_screen_with_data(payload: Value) -> Result<Value, String> {
    run_core("core_trend_screen_with_data", move || gp_core::trend_screen_with_data_value(payload)).await
}

#[tauri::command]
pub(crate) async fn core_agent(payload: Value) -> Result<Value, String> {
    run_core("core_agent", move || gp_core::agent_value(payload)).await
}

#[tauri::command]
pub(crate) async fn core_agent_with_data(payload: Value) -> Result<Value, String> {
    run_core("core_agent_with_data", move || gp_core::agent_with_data_value(payload)).await
}

#[tauri::command]
pub(crate) async fn core_mobile_stock_skill(payload: Value) -> Result<Value, String> {
    run_core("core_mobile_stock_skill", move || gp_core::mobile_stock_skill_value(payload)).await
}
