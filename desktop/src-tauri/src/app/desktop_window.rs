use tauri::Manager;
#[cfg(not(mobile))]
use tauri::{webview::PageLoadEvent, WebviewUrl, WebviewWindowBuilder};

use super::AppState;

#[cfg(not(mobile))]
pub(crate) fn setup(app: &mut tauri::App) -> tauri::Result<()> {
    setup_reliability(app.handle());
    WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("股选优")
        .inner_size(1280.0, 860.0)
        .min_inner_size(960.0, 680.0)
        .zoom_hotkeys_enabled(cfg!(target_os = "windows"))
        .visible(false)
        .on_page_load(|window, payload| {
            if matches!(payload.event(), PageLoadEvent::Started) {
                window.state::<AppState>().set_close_handler_ready(false);
            }
            if matches!(payload.event(), PageLoadEvent::Finished) {
                let _ = window.show();
                let _ = window.set_focus();
            }
        })
        .build()?;

    crate::research::schedule_research_maintenance(app.handle().clone());
    crate::gepa_lab::maybe_start_headless_from_env(app.handle().clone());

    Ok(())
}

pub(crate) fn setup_reliability(app: &tauri::AppHandle) {
    // A damaged optional store must not turn the entire local workbench into a blank window.
    let diagnostics_ok = app
        .path()
        .app_data_dir()
        .ok()
        .is_some_and(|root| crate::diagnostics::init(&root).is_ok());
    if !diagnostics_ok {
        app.state::<AppState>()
            .add_startup_warning("diagnostics_storage_unavailable");
    }
    if crate::jobs::init(app).is_err() {
        app.state::<AppState>()
            .add_startup_warning("jobs_storage_unavailable");
        let _ =
            crate::diagnostics::record(crate::diagnostics::OperationalEvent::StorageCheckFailed);
    }
}
