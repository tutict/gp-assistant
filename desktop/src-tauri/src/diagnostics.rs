//! Local-only operational diagnostics: no free-form event data, log messages, or automatic export.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

const MAX_EVENTS: usize = 64;
const MAX_COUNTER: u32 = 1_000_000;
const MAX_STATE_BYTES: u64 = 4096;

/// Deliberately a closed enum, not a string or object with caller-provided metadata.
/// Never add questions, symbols, URLs, paths, credentials, IDs, timings or raw errors here.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OperationalEvent {
    Started,
    CleanShutdown,
    PreviousUncleanExit,
    StorageCheckFailed,
    StorageRecoveryCompleted,
    OfflineOperationCompleted,
    TaskCompleted,
    TaskFailed,
    TaskCancelled,
    BackgroundRequest,
    GepaSettingChanged,
    GepaBlocked,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PreviousExit {
    FirstRun,
    Clean,
    Unclean,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GepaPreference {
    pub enabled: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Lifecycle {
    schema_version: u8,
    clean_shutdown: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct FeatureStatus {
    pub gepa_requested_enabled: bool,
    pub gepa_effective_enabled: bool,
    pub gepa_compiled: bool,
    pub safe_start: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct DiagnosticPreview {
    pub schema_version: u8,
    pub previous_exit: PreviousExit,
    pub settings: FeatureStatus,
    pub events: Vec<OperationalEvent>,
    pub counters: BTreeMap<OperationalEvent, u32>,
    pub dropped_events: u32,
}

/// Counters and the ring are session-local; disk holds only preference + lifecycle marker.
/// Use the process-global facade in production to serialize updates. The parent enforces
/// a single process per app-data directory before init, and marks clean only after flushes.
struct Diagnostics {
    root: PathBuf,
    preference: GepaPreference,
    safe_start: bool,
    previous_exit: PreviousExit,
    events: VecDeque<OperationalEvent>,
    counters: BTreeMap<OperationalEvent, u32>,
    dropped_events: u32,
    closed: bool,
}

fn read_state<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("diagnostics_read_failed".into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_STATE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "diagnostics_read_failed")?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Err("diagnostics_state_invalid".into());
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| "diagnostics_state_invalid".into())
}

fn save_state(path: &Path, state: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec(state).map_err(|_| "diagnostics_encode_failed")?;
    // Reuse the parent's synced atomic replacement, but never expose its path-bearing errors.
    crate::durability::atomic_write(path, &bytes).map_err(|_| "diagnostics_write_failed".into())
}

impl Diagnostics {
    fn open(root: &Path, safe_start: bool) -> Result<Self, String> {
        let preference =
            read_state::<GepaPreference>(&root.join("features.json"))?.unwrap_or_default();
        let previous_exit = match read_state::<Lifecycle>(&root.join("lifecycle.json"))? {
            None => PreviousExit::FirstRun,
            Some(Lifecycle {
                schema_version: 1,
                clean_shutdown: true,
            }) => PreviousExit::Clean,
            Some(Lifecycle {
                schema_version: 1,
                clean_shutdown: false,
            }) => PreviousExit::Unclean,
            Some(_) => return Err("diagnostics_state_invalid".into()),
        };
        save_state(
            &root.join("lifecycle.json"),
            &Lifecycle {
                schema_version: 1,
                clean_shutdown: false,
            },
        )?;
        let mut store = Self {
            root: root.to_path_buf(),
            preference,
            safe_start,
            previous_exit,
            events: VecDeque::with_capacity(MAX_EVENTS),
            counters: BTreeMap::new(),
            dropped_events: 0,
            closed: false,
        };
        store.record(OperationalEvent::Started)?;
        if previous_exit == PreviousExit::Unclean {
            store.record(OperationalEvent::PreviousUncleanExit)?;
        }
        Ok(store)
    }
    fn record(&mut self, event: OperationalEvent) -> Result<(), String> {
        if self.closed {
            return Err("diagnostics_session_closed".into());
        }
        let count = self.counters.entry(event).or_default();
        *count = count.saturating_add(1).min(MAX_COUNTER);
        if self.events.len() == MAX_EVENTS {
            self.events.pop_front();
            self.dropped_events = self.dropped_events.saturating_add(1).min(MAX_COUNTER);
        }
        self.events.push_back(event);
        Ok(())
    }
    fn gepa_allowed(&self) -> bool {
        !self.closed && self.preference.enabled && !self.safe_start
    }
    fn feature_status(&self) -> FeatureStatus {
        let compiled = cfg!(all(
            feature = "gepa-lab",
            any(target_os = "windows", target_os = "android")
        ));
        FeatureStatus {
            gepa_requested_enabled: self.preference.enabled,
            gepa_effective_enabled: self.gepa_allowed() && compiled,
            gepa_compiled: compiled,
            safe_start: self.safe_start,
        }
    }
    fn preview(&self) -> DiagnosticPreview {
        DiagnosticPreview {
            schema_version: 1,
            previous_exit: self.previous_exit,
            settings: self.feature_status(),
            events: self.events.iter().copied().collect(),
            counters: self.counters.clone(),
            dropped_events: self.dropped_events,
        }
    }
    fn set_gepa(&mut self, preference: GepaPreference) -> Result<FeatureStatus, String> {
        if self.closed {
            return Err("diagnostics_session_closed".into());
        }
        save_state(&self.root.join("features.json"), &preference)?;
        self.preference = preference;
        self.record(OperationalEvent::GepaSettingChanged)?;
        Ok(self.feature_status())
    }
    fn mark_clean_shutdown(&mut self) -> Result<(), String> {
        if self.closed {
            return Ok(());
        }
        save_state(
            &self.root.join("lifecycle.json"),
            &Lifecycle {
                schema_version: 1,
                clean_shutdown: true,
            },
        )?;
        self.record(OperationalEvent::CleanShutdown)?;
        self.closed = true;
        Ok(())
    }
}

pub(crate) fn safe_start_requested<'a>(
    environment: Option<&str>,
    arguments: impl IntoIterator<Item = &'a str>,
) -> bool {
    matches!(environment, Some("1" | "true"))
        || arguments.into_iter().any(|arg| arg == "--safe-start")
}

static STORE: OnceLock<Mutex<Option<Diagnostics>>> = OnceLock::new();
fn with_store<T>(f: impl FnOnce(&mut Diagnostics) -> Result<T, String>) -> Result<T, String> {
    let mut guard = STORE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "diagnostics_unavailable")?;
    f(guard.as_mut().ok_or("diagnostics_not_initialized")?)
}

/// Call once after the single-instance check, before optional jobs. Pass the app-data root.
/// Failure leaves GEPA disabled; propagate a generic UI warning, not raw diagnostic payloads.
pub(crate) fn init(app_data: &Path) -> Result<(), String> {
    let mut guard = STORE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "diagnostics_unavailable")?;
    if guard.is_some() {
        return Ok(());
    }
    let env = std::env::var("GP_ASSISTANT_SAFE_START").ok();
    let args: Vec<_> = std::env::args_os().collect();
    let safe = safe_start_requested(env.as_deref(), args.iter().filter_map(|arg| arg.to_str()));
    *guard = Some(Diagnostics::open(&app_data.join("diagnostics"), safe)?);
    Ok(())
}

/// Rust-only event entrypoint. Do NOT register as IPC or accept a caller-provided payload.
pub(crate) fn record(event: OperationalEvent) -> Result<(), String> {
    with_store(|store| store.record(event))
}
pub(crate) fn mark_clean_shutdown() -> Result<(), String> {
    with_store(Diagnostics::mark_clean_shutdown)
}
pub(crate) fn gepa_allowed() -> bool {
    with_store(|store| Ok(store.gepa_allowed())).unwrap_or(false)
}

// Parent registers these local commands (no native export-to-path/upload API).
#[tauri::command]
pub(crate) fn api_diagnostics_status() -> Result<FeatureStatus, String> {
    with_store(|store| Ok(store.feature_status()))
}
#[tauri::command]
pub(crate) fn api_diagnostics_preview() -> Result<DiagnosticPreview, String> {
    with_store(|store| Ok(store.preview()))
}
#[tauri::command]
pub(crate) fn api_diagnostics_set_gepa(payload: GepaPreference) -> Result<FeatureStatus, String> {
    with_store(|store| store.set_gepa(payload))
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Temp(std::path::PathBuf);
    impl Temp {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "gp-diagnostics-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn rejects_arbitrary_event_strings_and_payload_fields() {
        for text in [
            r#""https://secret.example/600000?key=x""#,
            r#"{"kind":"started","question":"secret"}"#,
            r#""verified_crash""#,
        ] {
            assert!(serde_json::from_str::<OperationalEvent>(text).is_err());
        }
        assert!(
            serde_json::from_str::<GepaPreference>(r#"{"enabled":true,"key":"secret"}"#).is_err()
        );
        assert!(serde_json::from_str::<GepaPreference>(r#"{"enabled":"true"}"#).is_err());
    }
    #[test]
    fn privacy_snapshot_has_only_fixed_fields_enums_and_bounded_numbers() {
        let root = Temp::new();
        let mut store = Diagnostics::open(&root.0, false).unwrap();
        store.record(OperationalEvent::StorageCheckFailed).unwrap();
        let value = serde_json::to_value(store.preview()).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["previous_exit"], "first_run");
        assert_eq!(
            value["events"],
            serde_json::json!(["started", "storage_check_failed"])
        );
        assert_eq!(value["counters"]["started"], 1);
        assert_eq!(value["counters"]["storage_check_failed"], 1);
        let object = value.as_object().unwrap();
        assert_eq!(object.len(), 6);
        assert!(object.keys().all(|key| [
            "schema_version",
            "previous_exit",
            "settings",
            "events",
            "counters",
            "dropped_events"
        ]
        .contains(&key.as_str())));
        assert!(!value
            .to_string()
            .contains(&root.0.to_string_lossy().to_string()));
    }
    #[test]
    fn event_ring_and_counters_are_bounded_even_after_quota() {
        let root = Temp::new();
        let mut store = Diagnostics::open(&root.0, false).unwrap();
        for _ in 0..1_000_020 {
            store.record(OperationalEvent::BackgroundRequest).unwrap();
        }
        let preview = store.preview();
        assert_eq!(preview.events.len(), 64);
        assert_eq!(
            preview.counters[&OperationalEvent::BackgroundRequest],
            1_000_000
        );
        assert_eq!(preview.dropped_events, 999_957);
        assert!(serde_json::to_vec(&preview).unwrap().len() < 8192);
    }
    #[test]
    fn persisted_running_marker_means_unclean_not_verified_crash() {
        let root = Temp::new();
        {
            let store = Diagnostics::open(&root.0, false).unwrap();
            assert_eq!(store.preview().previous_exit, PreviousExit::FirstRun);
        }
        let mut next = Diagnostics::open(&root.0, false).unwrap();
        assert_eq!(next.preview().previous_exit, PreviousExit::Unclean);
        assert_eq!(
            next.preview().counters[&OperationalEvent::PreviousUncleanExit],
            1
        );
        assert!(!serde_json::to_string(&next.preview())
            .unwrap()
            .contains("crash"));
        next.mark_clean_shutdown().unwrap();
        next.mark_clean_shutdown().unwrap();
        assert!(next.record(OperationalEvent::BackgroundRequest).is_err());
        let final_run = Diagnostics::open(&root.0, false).unwrap();
        assert_eq!(final_run.preview().previous_exit, PreviousExit::Clean);
    }
    #[test]
    fn flag_defaults_false_persists_and_safe_start_only_overrides() {
        let root = Temp::new();
        let mut store = Diagnostics::open(&root.0, false).unwrap();
        assert!(!store.gepa_allowed());
        store.set_gepa(GepaPreference { enabled: true }).unwrap();
        assert!(store.gepa_allowed());
        drop(store);
        let mut safe = Diagnostics::open(&root.0, true).unwrap();
        assert!(safe.preview().settings.gepa_requested_enabled);
        assert!(!safe.gepa_allowed());
        assert!(safe.preview().settings.safe_start);
        safe.mark_clean_shutdown().unwrap();
        drop(safe);
        let mut normal = Diagnostics::open(&root.0, false).unwrap();
        assert!(normal.gepa_allowed());
        normal.set_gepa(GepaPreference { enabled: false }).unwrap();
        drop(normal);
        assert!(!Diagnostics::open(&root.0, false).unwrap().gepa_allowed());
    }
    #[test]
    fn malformed_or_oversized_preferences_fail_closed_and_preserve_bytes() {
        for bytes in [
            br#"{"enabled":true,"url":"private"}"#.to_vec(),
            vec![b' '; 5000],
        ] {
            let root = Temp::new();
            std::fs::write(root.0.join("features.json"), &bytes).unwrap();
            assert!(Diagnostics::open(&root.0, false).is_err());
            assert_eq!(std::fs::read(root.0.join("features.json")).unwrap(), bytes);
        }
    }
    #[test]
    fn corrupt_marker_is_not_silently_called_clean_or_crashed() {
        let root = Temp::new();
        std::fs::write(root.0.join("lifecycle.json"), b"invalid").unwrap();
        let error = Diagnostics::open(&root.0, true).err().unwrap();
        assert_eq!(error, "diagnostics_state_invalid");
        assert_eq!(
            std::fs::read(root.0.join("lifecycle.json")).unwrap(),
            b"invalid"
        );
    }
    #[test]
    fn failed_flag_save_does_not_change_effective_preference() {
        let root = Temp::new();
        let mut store = Diagnostics::open(&root.0, false).unwrap();
        std::fs::create_dir(root.0.join("features.json")).unwrap();
        assert_eq!(
            store
                .set_gepa(GepaPreference { enabled: true })
                .unwrap_err(),
            "diagnostics_write_failed"
        );
        assert!(!store.gepa_allowed());
    }
    #[test]
    fn safe_start_opt_in_is_explicit() {
        assert!(safe_start_requested(Some("1"), ["app"]));
        assert!(safe_start_requested(Some("true"), ["app"]));
        assert!(safe_start_requested(None, ["app", "--safe-start"]));
        assert!(!safe_start_requested(
            Some("0"),
            ["app", "--safe-start=false"]
        ));
        assert!(!safe_start_requested(None, ["app"]));
    }
}
