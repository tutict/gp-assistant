use crate::market::store::files::MOBILE_MARKET_DATA_FILE;
use crate::market::store::projection::external_market_domains;
use crate::market::store::projection::external_market_records_from_typed;
use crate::market_domain::datasets::parse_mobile_market_data_snapshot;
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;
use std::sync::OnceLock;

static LEGACY_MARKET_MIGRATION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn legacy_market_migration_lock() -> &'static Mutex<()> {
    LEGACY_MARKET_MIGRATION_LOCK.get_or_init(|| Mutex::new(()))
}

pub(in crate::market) struct LegacyMarketMigration {
    promoted: HashSet<(
        crate::external_data::DataDomain,
        crate::external_data::Market,
    )>,
    source_json: Option<Result<Value, String>>,
    source_checksum_sha256: String,
    normalized: bool,
}

pub(in crate::market) fn import_legacy_market_source(
    store: &mut crate::external_data::DataStore,
    path: &Path,
) -> Result<LegacyMarketMigration, String> {
    let _migration_guard = legacy_market_migration_lock()
        .lock()
        .map_err(|_| "legacy market migration lock is poisoned".to_string())?;
    let domains = external_market_domains();
    let batch: crate::external_data::LegacyJsonImportBatch =
        crate::external_data::import_legacy_json_domains(store, path, &domains, |value, _, _| {
            let data = value
                .get("data")
                .filter(|value| value.is_object())
                .cloned()
                .unwrap_or_else(|| value.clone());
            let (typed, _) = parse_mobile_market_data_snapshot(&data, "legacy mobile market data")?;
            external_market_records_from_typed(&data, &typed, MOBILE_MARKET_DATA_FILE)
        })?;
    let promoted = batch
        .reports
        .iter()
        .filter(|report| report.promoted)
        .filter_map(|report| {
            domains
                .iter()
                .find(|(domain, market)| {
                    domain.as_str() == report.domain && market.as_str() == report.market
                })
                .copied()
        })
        .collect();
    Ok(LegacyMarketMigration {
        promoted,
        source_json: batch.source_json,
        source_checksum_sha256: batch.source_checksum_sha256,
        normalized: batch.normalized,
    })
}

pub(crate) fn import_legacy_market_domains(
    store: &mut crate::external_data::DataStore,
    path: &Path,
) -> Result<
    HashSet<(
        crate::external_data::DataDomain,
        crate::external_data::Market,
    )>,
    String,
> {
    Ok(import_legacy_market_source(store, path)?.promoted)
}

pub(in crate::market) fn all_market_domains_promoted(
    promoted: &HashSet<(
        crate::external_data::DataDomain,
        crate::external_data::Market,
    )>,
) -> bool {
    external_market_domains()
        .iter()
        .all(|domain| promoted.contains(domain))
}

type LegacyMarketMigrationParts = (
    HashSet<(
        crate::external_data::DataDomain,
        crate::external_data::Market,
    )>,
    String,
    Option<Result<Value, String>>,
    bool,
);

impl LegacyMarketMigration {
    pub(in crate::market) fn into_parts(self) -> LegacyMarketMigrationParts {
        (
            self.promoted,
            self.source_checksum_sha256,
            self.source_json,
            self.normalized,
        )
    }
}
