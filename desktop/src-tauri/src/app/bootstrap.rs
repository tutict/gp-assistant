use tauri::Manager;

use super::{command_registry, desktop_window, lifecycle, AppState};

pub(crate) fn run() {
    let builder = tauri::Builder::default();
    #[cfg(not(mobile))]
    let builder = {
        let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }));
        lifecycle::configure_close_events(builder)
    };
    let builder = builder.plugin(crate::credentials::init());
    let builder = command_registry::register(builder)
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let _ = app.manage(AppState::default());
            #[cfg(not(mobile))]
            desktop_window::setup(app)?;
            #[cfg(mobile)]
            {
                desktop_window::setup_reliability(app.handle());
                crate::research::schedule_research_maintenance(app.handle().clone());
            }
            Ok(())
        });
    builder
        .build(tauri::generate_context!())
        .expect("error while building 股选优")
        .run(lifecycle::on_run_event);
}
