use crate::market::cache::ExternalMarketDataCacheVersion;
use crate::market::cache::MarketDataFileVersion;
use crate::market::store::files::sha256_file;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

pub(crate) fn file_modified_millis(path: &Path) -> Option<u128> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis())
}

fn market_data_file_version(path: &Path) -> Result<Option<MarketDataFileVersion>, std::io::Error> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let modified = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    Ok(Some(MarketDataFileVersion {
        bytes: metadata.len(),
        modified_at_epoch_ms: modified.as_millis(),
        modified_at_epoch_ns: modified.as_nanos(),
    }))
}

pub(in crate::market::cache) fn external_market_data_cache_version(
    external_path: &Path,
    legacy_path: &Path,
    known_legacy_checksum_sha256: Option<&str>,
) -> Option<ExternalMarketDataCacheVersion> {
    let mut version = external_market_data_file_versions(external_path, legacy_path)?;
    if version.legacy.is_some() {
        version.legacy_checksum_sha256 = Some(match known_legacy_checksum_sha256 {
            Some(checksum) => checksum.to_owned(),
            None => sha256_file(legacy_path).ok()?,
        });
    }
    Some(version)
}

pub(in crate::market::cache) fn external_market_data_file_versions(
    external_path: &Path,
    legacy_path: &Path,
) -> Option<ExternalMarketDataCacheVersion> {
    let database = market_data_file_version(external_path).ok()??;
    let wal_path = PathBuf::from(format!("{}-wal", external_path.to_string_lossy()));
    let shm_path = PathBuf::from(format!("{}-shm", external_path.to_string_lossy()));
    let wal = market_data_file_version(&wal_path).ok()?;
    let shm = market_data_file_version(&shm_path).ok()?;
    let legacy = market_data_file_version(legacy_path).ok()?;
    Some(ExternalMarketDataCacheVersion {
        database,
        wal,
        shm,
        legacy,
        legacy_checksum_sha256: None,
    })
}
