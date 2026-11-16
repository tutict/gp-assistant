use tauri::{AppHandle, Manager, RunEvent};

use super::AppState;

#[cfg(not(mobile))]
pub(crate) fn configure_close_events<R: tauri::Runtime>(
    builder: tauri::Builder<R>,
) -> tauri::Builder<R> {
    builder.on_window_event(|window, event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            if window.label() == "main" && window.state::<AppState>().should_prevent_close() {
                api.prevent_close();
                let _ = tauri::Emitter::emit(window.app_handle(), "client-close-requested", ());
            }
        }
    })
}

pub(crate) fn on_run_event(app: &AppHandle, event: RunEvent) {
    if matches!(event, RunEvent::Exit)
        && app.state::<AppState>().should_mark_clean_shutdown()
        && !crate::jobs::has_active_work(app)
    {
        let _ = crate::diagnostics::mark_clean_shutdown();
    }
}
