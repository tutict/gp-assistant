use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};

/// Process-wide application state managed by Tauri.
///
/// The state stays private to the desktop shell. Frontend command arguments and results do not
/// expose these implementation details.
#[derive(Default)]
pub(crate) struct AppState {
    close_handler_ready: AtomicBool,
    close_saved: AtomicBool,
    startup_warnings: Mutex<Vec<&'static str>>,
}

impl AppState {
    pub(crate) fn set_close_handler_ready(&self, ready: bool) {
        self.close_handler_ready.store(ready, Ordering::SeqCst);
    }

    pub(crate) fn confirm_close(&self, saved: bool) {
        self.close_saved.store(saved, Ordering::SeqCst);
        self.close_handler_ready.store(false, Ordering::SeqCst);
    }

    pub(crate) fn should_prevent_close(&self) -> bool {
        self.close_handler_ready.load(Ordering::SeqCst)
    }

    pub(crate) fn should_mark_clean_shutdown(&self) -> bool {
        self.close_saved.load(Ordering::SeqCst)
    }

    pub(crate) fn add_startup_warning(&self, warning: &'static str) {
        if let Ok(mut warnings) = self.startup_warnings.lock() {
            warnings.push(warning);
        }
    }

    pub(crate) fn startup_warnings(&self) -> Result<Vec<&'static str>, &'static str> {
        self.startup_warnings
            .lock()
            .map(|warnings| warnings.clone())
            .map_err(|_| "startup status unavailable")
    }
}
