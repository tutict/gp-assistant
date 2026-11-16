use serde_json::{json, Value};
use tauri::{AppHandle, State};
use tauri_plugin_shell::ShellExt;

use super::AppState;

#[tauri::command]
pub(crate) fn api_app_close_handler_ready(ready: bool, state: State<'_, AppState>) {
    state.set_close_handler_ready(ready);
}

#[tauri::command]
pub(crate) fn api_app_confirm_close(app: AppHandle, saved: bool, state: State<'_, AppState>) {
    state.confirm_close(saved);
    app.exit(0);
}

#[tauri::command]
pub(crate) fn api_health(state: State<'_, AppState>) -> Result<Value, String> {
    let warnings = state.startup_warnings().map_err(str::to_string)?;
    Ok(json!({
        "status": if warnings.is_empty() { "ok" } else { "degraded" },
        "runtime": "tauri",
        "warnings": warnings
    }))
}

#[tauri::command]
#[allow(deprecated)]
pub(crate) fn open_external_url(app: AppHandle, url: String) -> Result<(), String> {
    let trimmed = url.trim();
    let parsed = reqwest::Url::parse(trimmed).map_err(|_| "来源链接格式不正确".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("只允许打开 http/https 来源链接".to_string());
    }
    app.shell()
        .open(parsed.as_str().to_string(), None)
        .map_err(|error| error.to_string())
}
