use crate::research::hk_cache;
use crate::research::recovery;
use crate::research::sources::ingest_news_cache_file_for_market;
use crate::research::sources::migrate_legacy_sources;
use crate::research::storage::ResearchStore;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering as AtomicOrdering;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::RwLock;
use tauri::Manager;

pub(in crate::research) fn open_app_store(app: &tauri::AppHandle) -> Result<ResearchStore, String> {
    open_app_store_for_market(app, "cn")
}

pub(crate) fn market_from_payload(payload: &Value) -> Result<&'static str, String> {
    match payload
        .get("market")
        .and_then(Value::as_str)
        .unwrap_or("cn")
    {
        "cn" => Ok("cn"),
        "hk" => Ok("hk"),
        market => Err(format!("unsupported research market: {market}")),
    }
}

pub(in crate::research) fn research_database_path(
    research_dir: &Path,
    market: &str,
) -> Result<PathBuf, String> {
    match market {
        "cn" => Ok(research_dir.join("research.sqlite")),
        "hk" => Ok(research_dir.join("research-hk.sqlite")),
        other => Err(format!("unsupported research market: {other}")),
    }
}

fn open_app_store_for_market(
    app: &tauri::AppHandle,
    market: &str,
) -> Result<ResearchStore, String> {
    let migration_lock = LEGACY_MIGRATION_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = migration_lock
        .lock()
        .map_err(|_| "legacy research migration lock is poisoned".to_string())?;
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data directory: {error}"))?;
    let research_dir = app_data.join("research");
    fs::create_dir_all(&research_dir)
        .map_err(|error| format!("failed to create research directory: {error}"))?;
    let store = ResearchStore::open(research_database_path(&research_dir, market)?)?;
    if market == "cn" {
        migrate_legacy_sources(&store, &app_data)?;
    } else {
        // The local HK observation cache already contains currency-bearing reports.
        // Make them searchable alongside news without requiring a second online fetch.
        let cache = crate::hk_market::read_hk_market_cache(
            &crate::hk_market::hk_market_cache_path(&app_data),
        )?;
        store.ingest_documents(&hk_cache::documents(&cache, &[]))?;
        ingest_news_cache_file_for_market(
            &store,
            &app_data.join("news").join("news-cache-hk.json"),
            crate::external_data::Market::HongKong,
        )?;
    }
    Ok(store)
}

pub(crate) fn with_app_store<T>(
    app: &tauri::AppHandle,
    operation: impl FnOnce(&ResearchStore) -> Result<T, String>,
) -> Result<T, String> {
    with_recovered_app_database(app, || {
        let store = open_app_store(app)?;
        operation(&store)
    })
}

pub(crate) fn with_app_store_for_market<T>(
    app: &tauri::AppHandle,
    market: &str,
    operation: impl FnOnce(&ResearchStore) -> Result<T, String>,
) -> Result<T, String> {
    with_recovered_app_database(app, || {
        let store = open_app_store_for_market(app, market)?;
        operation(&store)
    })
}

pub(crate) fn with_app_store_snapshot<T>(
    app: &tauri::AppHandle,
    operation: impl FnOnce(&ResearchStore) -> Result<T, String>,
) -> Result<(u64, T), String> {
    with_recovered_app_database(app, || {
        let generation = APP_RESEARCH_DATABASE_GENERATION.load(AtomicOrdering::Acquire);
        let store = open_app_store(app)?;
        operation(&store).map(|result| (generation, result))
    })
}

pub(crate) fn with_app_store_snapshot_for_market<T>(
    app: &tauri::AppHandle,
    market: &str,
    operation: impl FnOnce(&ResearchStore) -> Result<T, String>,
) -> Result<(u64, T), String> {
    with_recovered_app_database(app, || {
        let generation = APP_RESEARCH_DATABASE_GENERATION.load(AtomicOrdering::Acquire);
        let store = open_app_store_for_market(app, market)?;
        operation(&store).map(|result| (generation, result))
    })
}

pub(crate) fn with_app_store_at_generation<T>(
    app: &tauri::AppHandle,
    expected_generation: u64,
    operation: impl FnOnce(&ResearchStore) -> Result<T, String>,
) -> Result<T, String> {
    with_recovered_app_database(app, || {
        ensure_app_database_generation(expected_generation)?;
        let store = open_app_store(app)?;
        operation(&store)
    })
}

pub(crate) fn with_app_store_at_generation_for_market<T>(
    app: &tauri::AppHandle,
    expected_generation: u64,
    market: &str,
    operation: impl FnOnce(&ResearchStore) -> Result<T, String>,
) -> Result<T, String> {
    with_recovered_app_database(app, || {
        ensure_app_database_generation(expected_generation)?;
        let store = open_app_store_for_market(app, market)?;
        operation(&store)
    })
}

pub(crate) fn with_app_hk_model_evidence_send<T>(
    app: &tauri::AppHandle,
    expected_generation: u64,
    snapshots: &[Value],
    send: impl FnOnce() -> T,
) -> Result<T, String> {
    with_model_evidence_database_guard(|| {
        ensure_app_database_generation(expected_generation)?;
        let store = open_app_store_for_market(app, "hk")?;
        if !store.validate_hk_model_evidence(snapshots)? {
            return Err("研究资料、来源许可或证据库已经变化，请重新取证。".to_string());
        }
        Ok(send())
    })
}

pub(in crate::research) fn ensure_app_database_generation(
    expected_generation: u64,
) -> Result<(), String> {
    if APP_RESEARCH_DATABASE_GENERATION.load(AtomicOrdering::Acquire) != expected_generation {
        return Err(
            "research database changed while the answer was generated; retry the query".to_string(),
        );
    }
    Ok(())
}

fn with_recovered_app_database<T>(
    app: &tauri::AppHandle,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data directory: {error}"))?
        .join("research");
    let mut operation = Some(operation);
    loop {
        // Check AND execute under one shared guard: a failed delivery cannot slip between them.
        let result = with_shared_app_database(|| {
            if recovery::needs_recovery(&directory)? {
                return Ok(None);
            }
            operation
                .take()
                .expect("research operation runs exactly once")()
            .map(Some)
        })?;
        if let Some(value) = result {
            return Ok(value);
        }
        with_exclusive_app_database(|| {
            recover_research_files(&directory)?;
            open_app_store(app).map(|_| ())
        })?;
    }
}

pub(in crate::research) fn recover_research_files(research_dir: &Path) -> Result<(), String> {
    recovery::recover(research_dir)
}

static LEGACY_MIGRATION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub(in crate::research) static APP_RESEARCH_DATABASE_LOCK: OnceLock<RwLock<()>> = OnceLock::new();

pub(in crate::research) static APP_RESEARCH_DATABASE_GENERATION: AtomicU64 = AtomicU64::new(0);

pub(in crate::research) fn with_shared_app_database<T>(
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let lock = APP_RESEARCH_DATABASE_LOCK.get_or_init(|| RwLock::new(()));
    let _guard = lock
        .read()
        .map_err(|_| "research database coordination lock is poisoned".to_string())?;
    operation()
}

pub(crate) fn with_model_evidence_database_guard<T>(
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let lock = APP_RESEARCH_DATABASE_LOCK.get_or_init(|| RwLock::new(()));
    let _guard = lock
        .write()
        .map_err(|_| "research database coordination lock is poisoned".to_string())?;
    operation()
}

pub(in crate::research) fn with_exclusive_app_database<T>(
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let lock = APP_RESEARCH_DATABASE_LOCK.get_or_init(|| RwLock::new(()));
    let _guard = lock
        .write()
        .map_err(|_| "research database coordination lock is poisoned".to_string())?;
    APP_RESEARCH_DATABASE_GENERATION.fetch_add(1, AtomicOrdering::AcqRel);
    operation()
}
