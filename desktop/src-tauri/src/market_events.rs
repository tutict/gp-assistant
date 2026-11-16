use crate::market_domain::dates::epoch_millis;
use serde_json::json;
use serde_json::Value;
use tauri::Emitter;

pub(crate) fn emit_market_refresh_log(
    app: &tauri::AppHandle,
    stage: &str,
    tone: &str,
    payload: Value,
) {
    let _ = app.emit(
        "market-refresh-log",
        json!({
            "stage": stage,
            "tone": tone,
            "payload": payload,
            "timestamp_ms": epoch_millis(),
        }),
    );
}
