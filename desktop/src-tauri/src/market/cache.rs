#[cfg(test)]
mod external_market_cache_tests;
pub(crate) mod records;
pub(crate) mod snapshots;
pub(crate) mod versions;

use crate::market::cache::versions::external_market_data_file_versions;
use crate::market::store::files::sha256_file;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use stock_optimizer_core as gp_core;

pub(crate) static REFRESH_SEED_CACHE: OnceLock<Mutex<HashMap<PathBuf, Value>>> = OnceLock::new();

pub(crate) static REFRESH_FINANCIAL_SNAPSHOT_CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<Value>>>> =
    OnceLock::new();

pub(crate) static MOBILE_MARKET_DATA_CACHE: OnceLock<
    Mutex<HashMap<PathBuf, MobileMarketDataCacheEntry>>,
> = OnceLock::new();

pub(crate) static SCREEN_STOCK_OVERLAY_CACHE: OnceLock<
    Mutex<HashMap<PathBuf, ScreenStockOverlayCacheEntry>>,
> = OnceLock::new();

pub(crate) static MOBILE_MARKET_UPDATE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

static MOBILE_MARKET_READ_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone)]
pub(crate) struct MobileMarketDataCacheEntry {
    pub(crate) bytes: u64,
    pub(crate) modified_at_epoch_ms: Option<u128>,
    store_version: Option<ExternalMarketDataCacheVersion>,
    pub(crate) data: Arc<Value>,
    pub(crate) typed: Arc<gp_core::CoreDataSet>,
    pub(crate) summary: Value,
}

pub(crate) struct MarketDataSnapshot {
    pub(crate) data: Arc<Value>,
    pub(crate) typed: Arc<gp_core::CoreDataSet>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MarketDataFileVersion {
    bytes: u64,
    modified_at_epoch_ms: u128,
    modified_at_epoch_ns: u128,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ExternalMarketDataCacheVersion {
    database: MarketDataFileVersion,
    wal: Option<MarketDataFileVersion>,
    shm: Option<MarketDataFileVersion>,
    legacy: Option<MarketDataFileVersion>,
    legacy_checksum_sha256: Option<String>,
}

impl ExternalMarketDataCacheVersion {
    fn same_files(&self, other: &Self) -> bool {
        self.database == other.database
            && self.wal == other.wal
            && self.shm == other.shm
            && self.legacy == other.legacy
    }

    fn external_bytes(&self) -> u64 {
        self.database.bytes
            + self.wal.as_ref().map(|file| file.bytes).unwrap_or(0)
            + self.shm.as_ref().map(|file| file.bytes).unwrap_or(0)
    }

    fn legacy_modified_at_epoch_ms(&self) -> Option<u128> {
        self.legacy.as_ref().map(|file| file.modified_at_epoch_ms)
    }
}

#[derive(Clone)]
pub(crate) struct ScreenStockOverlayCacheEntry {
    pub(crate) data: Arc<gp_core::CoreDataSet>,
    pub(crate) financial_snapshot: Arc<Value>,
    pub(crate) stocks: Arc<Vec<gp_core::StockItem>>,
}

pub(crate) fn refresh_seed_cache() -> &'static Mutex<HashMap<PathBuf, Value>> {
    REFRESH_SEED_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn refresh_financial_snapshot_cache() -> &'static Mutex<HashMap<PathBuf, Arc<Value>>> {
    REFRESH_FINANCIAL_SNAPSHOT_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn mobile_market_data_cache(
) -> &'static Mutex<HashMap<PathBuf, MobileMarketDataCacheEntry>> {
    MOBILE_MARKET_DATA_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn screen_stock_overlay_cache(
) -> &'static Mutex<HashMap<PathBuf, ScreenStockOverlayCacheEntry>> {
    SCREEN_STOCK_OVERLAY_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn mobile_market_update_lock() -> &'static Mutex<()> {
    MOBILE_MARKET_UPDATE_LOCK.get_or_init(|| Mutex::new(()))
}

fn mobile_market_read_lock() -> &'static Mutex<()> {
    MOBILE_MARKET_READ_LOCK.get_or_init(|| Mutex::new(()))
}

pub(crate) fn cached_mobile_market_data_entry(
    path: &Path,
    bytes: u64,
    modified_at_epoch_ms: Option<u128>,
) -> Option<MobileMarketDataCacheEntry> {
    let slot = mobile_market_data_cache().lock().ok()?;
    let entry = slot.get(path)?;
    if entry.store_version.is_none()
        && entry.bytes == bytes
        && entry.modified_at_epoch_ms == modified_at_epoch_ms
    {
        Some(entry.clone())
    } else {
        None
    }
}

#[cfg(test)]
fn cached_external_market_data_entry(
    path: &Path,
    version: &ExternalMarketDataCacheVersion,
) -> Option<MobileMarketDataCacheEntry> {
    let slot = mobile_market_data_cache().lock().ok()?;
    let entry = slot.get(path)?;
    (entry.store_version.as_ref() == Some(version)).then(|| entry.clone())
}

fn cached_external_market_data_entry_for_files(
    cache_path: &Path,
    external_path: &Path,
    legacy_path: &Path,
) -> Option<(ExternalMarketDataCacheVersion, MobileMarketDataCacheEntry)> {
    let file_versions = external_market_data_file_versions(external_path, legacy_path)?;
    let entry = {
        let slot = mobile_market_data_cache().lock().ok()?;
        let entry = slot.get(cache_path)?;
        let cached_version = entry.store_version.as_ref()?;
        if !cached_version.same_files(&file_versions) {
            return None;
        }
        entry.clone()
    };

    let mut current_version = file_versions;
    if current_version.legacy.is_some() {
        current_version.legacy_checksum_sha256 = Some(sha256_file(legacy_path).ok()?);
        let after_hash = external_market_data_file_versions(external_path, legacy_path)?;
        if !current_version.same_files(&after_hash) {
            return None;
        }
    }
    let cached_version = entry.store_version.as_ref()?;
    (cached_version == &current_version).then_some((current_version, entry))
}

pub(crate) fn remember_mobile_market_data_cache(
    path: &Path,
    bytes: u64,
    modified_at_epoch_ms: Option<u128>,
    data: &Value,
    typed: Arc<gp_core::CoreDataSet>,
    summary: &Value,
) {
    if let Ok(mut slot) = mobile_market_data_cache().lock() {
        slot.insert(
            path.to_path_buf(),
            MobileMarketDataCacheEntry {
                bytes,
                modified_at_epoch_ms,
                store_version: None,
                data: Arc::new(data.clone()),
                typed,
                summary: summary.clone(),
            },
        );
    }
}

fn remember_external_market_data_cache(
    path: &Path,
    version: ExternalMarketDataCacheVersion,
    data: &Value,
    typed: Arc<gp_core::CoreDataSet>,
    summary: &Value,
) {
    if let Ok(mut slot) = mobile_market_data_cache().lock() {
        slot.insert(
            path.to_path_buf(),
            MobileMarketDataCacheEntry {
                bytes: version.external_bytes(),
                modified_at_epoch_ms: version.legacy_modified_at_epoch_ms(),
                store_version: Some(version),
                data: Arc::new(data.clone()),
                typed,
                summary: summary.clone(),
            },
        );
    }
}

pub(crate) fn forget_mobile_market_data_cache(path: &Path) {
    if let Ok(mut slot) = mobile_market_data_cache().lock() {
        slot.remove(path);
    }
}
