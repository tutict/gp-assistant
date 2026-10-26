use super::{DataDomain, DataStore, Market, StoredRecord};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct LegacyImportReport {
    pub domain: String,
    pub market: String,
    pub source_path: String,
    pub checksum_sha256: String,
    pub parsed_count: usize,
    pub imported_count: usize,
    pub validation_state: String,
    pub promoted: bool,
    pub error: Option<String>,
}

/// Import one legacy JSON domain without modifying its source. `normalize` must pass each
/// item through that domain's existing typed parser before returning records.
pub(crate) fn import_legacy_json<F>(
    store: &mut DataStore,
    source_path: &Path,
    domain: DataDomain,
    market: Market,
    normalize: F,
) -> Result<LegacyImportReport, String>
where
    F: FnOnce(Value, &str, &str) -> Result<Vec<StoredRecord>, String>,
{
    let source_bytes = std::fs::read(source_path)
        .map_err(|error| format!("read legacy cache {}: {error}", source_path.display()))?;
    let checksum = sha256_hex(&source_bytes);
    let source_path_text = source_path.to_string_lossy().into_owned();
    if let Some(existing) =
        store.legacy_import_status(domain, market, &source_path_text, &checksum)?
    {
        if existing.promoted && existing.validation_state == "validated" {
            return Ok(existing.into());
        }
    }
    if let Some(promoted) =
        store.latest_promoted_legacy_import(domain, market, &source_path_text)?
    {
        // After parity promotion the legacy file is a read-only recovery copy. A later
        // external replacement must not silently resurrect data that the user cleared.
        return Ok(promoted.into());
    }

    let json: Value = match serde_json::from_slice(&source_bytes) {
        Ok(json) => json,
        Err(error) => {
            return record_failure(
                store,
                domain,
                market,
                &source_path_text,
                &checksum,
                0,
                &format!("legacy JSON is malformed: {error}"),
            )
        }
    };
    let records = match normalize(json, &source_path_text, &checksum) {
        Ok(records) => records,
        Err(error) => {
            return record_failure(
                store,
                domain,
                market,
                &source_path_text,
                &checksum,
                0,
                &format!("legacy normalization failed: {error}"),
            )
        }
    };
    if records
        .iter()
        .any(|record| record.domain != domain || record.market != market)
    {
        return record_failure(
            store,
            domain,
            market,
            &source_path_text,
            &checksum,
            records.len(),
            "normalized legacy record escaped its domain or market",
        );
    }

    let expected = canonical_record_set(&records)?;
    if expected.len() != records.len() {
        return record_failure(
            store,
            domain,
            market,
            &source_path_text,
            &checksum,
            records.len(),
            "legacy cache contains duplicate canonical records",
        );
    }
    let imported_count = match store.upsert_batch(&records) {
        Ok(count) => count,
        Err(error) => {
            return record_failure(
                store,
                domain,
                market,
                &source_path_text,
                &checksum,
                records.len(),
                &format!("legacy import write failed: {error}"),
            )
        }
    };
    let actual = canonical_record_set(&store.read_domain_records(domain, market)?)?;
    let expected_for_domain = expected
        .iter()
        .filter(|(key, _)| {
            key.starts_with(&format!(
                "{}\u{1f}{}\u{1f}",
                domain.as_str(),
                market.as_str()
            ))
        })
        .cloned()
        .collect::<Vec<_>>();
    let expected_for_domain = expected_for_domain
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>();
    let expected_keys = expected_for_domain
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let actual_for_domain = actual
        .into_iter()
        .filter(|(key, _)| expected_keys.contains(key))
        .collect::<Vec<_>>();
    let actual_for_domain = actual_for_domain
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>();
    if expected_for_domain != actual_for_domain {
        return record_failure(
            store,
            domain,
            market,
            &source_path_text,
            &checksum,
            records.len(),
            "legacy and external store projections differ",
        );
    }

    store.record_legacy_import(
        domain,
        market,
        &source_path_text,
        &checksum,
        records.len(),
        imported_count,
        "validated",
        true,
        None,
    )?;
    Ok(LegacyImportReport {
        domain: domain.as_str().to_string(),
        market: market.as_str().to_string(),
        source_path: source_path_text,
        checksum_sha256: checksum,
        parsed_count: records.len(),
        imported_count,
        validation_state: "validated".into(),
        promoted: true,
        error: None,
    })
}

fn record_failure(
    store: &DataStore,
    domain: DataDomain,
    market: Market,
    source_path: &str,
    checksum: &str,
    parsed_count: usize,
    error: &str,
) -> Result<LegacyImportReport, String> {
    let error = truncate_utf8(error, 2 * 1024);
    store.record_legacy_import(
        domain,
        market,
        source_path,
        checksum,
        parsed_count,
        0,
        "migration_degraded",
        false,
        Some(error),
    )?;
    Ok(LegacyImportReport {
        domain: domain.as_str().to_string(),
        market: market.as_str().to_string(),
        source_path: source_path.to_string(),
        checksum_sha256: checksum.to_string(),
        parsed_count,
        imported_count: 0,
        validation_state: "migration_degraded".into(),
        promoted: false,
        error: Some(error.to_string()),
    })
}

fn canonical_record_set(records: &[StoredRecord]) -> Result<Vec<(String, String)>, String> {
    let mut canonical = records
        .iter()
        .map(|record| {
            let key = format!(
                "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
                record.domain.as_str(),
                record.market.as_str(),
                record.entity_key,
                record.record_key,
                record.metadata.source_id,
            );
            let payload = serde_json::to_vec(&record.payload)
                .map_err(|error| format!("encode legacy record for parity check: {error}"))?;
            Ok((key, sha256_hex(&payload)))
        })
        .collect::<Result<Vec<_>, String>>()?;
    canonical.sort();
    Ok(canonical)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn truncate_utf8(value: &str, max_bytes: usize) -> &str {
    let mut end = value.len().min(max_bytes);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

impl From<super::store::LegacyImportStatus> for LegacyImportReport {
    fn from(status: super::store::LegacyImportStatus) -> Self {
        Self {
            domain: status.domain,
            market: status.market,
            source_path: status.source_path,
            checksum_sha256: status.checksum_sha256,
            parsed_count: status.parsed_count,
            imported_count: status.imported_count,
            validation_state: status.validation_state,
            promoted: status.promoted,
            error: status.last_error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("gp-legacy-import-{}-{stamp}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn db(&self) -> PathBuf {
            self.0.join("external-data.sqlite")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn fixture_record(checksum: &str, payload: Value) -> StoredRecord {
        StoredRecord::new(
            DataDomain::Quote,
            Market::HongKong,
            "00700.HK",
            "quote",
            "legacy-test",
            "legacy",
            "2026-10-20T00:00:00Z",
            None,
            None,
            "unverified",
            vec![format!("legacy_checksum:{checksum}")],
            payload,
        )
    }

    #[test]
    fn valid_fixture_imports_once_and_keeps_legacy_file_byte_identical() {
        let fixture = Fixture::new();
        let source = fixture.0.join("hk-market-data.json");
        let bytes = br#"{"quotes":[{"code":"00700.HK","price":417.8}]}"#;
        fs::write(&source, bytes).unwrap();
        let mut store = DataStore::open(fixture.db()).unwrap();

        let normalize = |value: Value, _path: &str, checksum: &str| {
            let quote = value["quotes"]
                .as_array()
                .and_then(|items| items.first())
                .ok_or_else(|| "fixture quote missing".to_string())?;
            Ok(vec![fixture_record(
                checksum,
                json!({"code": quote["code"], "price": quote["price"]}),
            )])
        };
        let report = import_legacy_json(
            &mut store,
            &source,
            DataDomain::Quote,
            Market::HongKong,
            normalize,
        )
        .unwrap();
        assert!(report.promoted);
        assert_eq!(report.parsed_count, 1);
        assert_eq!(report.imported_count, 1);
        assert_eq!(fs::read(&source).unwrap(), bytes);

        let checksum = report.checksum_sha256.clone();
        let normalize_again = move |value: Value, _path: &str, actual_checksum: &str| {
            if checksum != actual_checksum {
                return Err("checksum unexpectedly changed".into());
            }
            let quote = value["quotes"]
                .as_array()
                .and_then(|items| items.first())
                .ok_or_else(|| "fixture quote missing".to_string())?;
            Ok(vec![fixture_record(
                actual_checksum,
                json!({"code": quote["code"], "price": quote["price"]}),
            )])
        };
        let repeated = import_legacy_json(
            &mut store,
            &source,
            DataDomain::Quote,
            Market::HongKong,
            normalize_again,
        )
        .unwrap();
        assert_eq!(repeated.imported_count, 1);
        assert_eq!(repeated.validation_state, "validated");
    }

    #[test]
    fn malformed_fixture_is_reported_without_changing_source_or_promoting() {
        let fixture = Fixture::new();
        let source = fixture.0.join("news-cache.json");
        let bytes = b"{corrupt legacy cache";
        fs::write(&source, bytes).unwrap();
        let mut store = DataStore::open(fixture.db()).unwrap();
        let report = import_legacy_json(
            &mut store,
            &source,
            DataDomain::News,
            Market::Global,
            |_, _, _| Ok(Vec::new()),
        )
        .unwrap();
        assert!(!report.promoted);
        assert_eq!(report.validation_state, "migration_degraded");
        assert_eq!(fs::read(&source).unwrap(), bytes);
        let recorded = store
            .legacy_import_status(
                DataDomain::News,
                Market::Global,
                &source.to_string_lossy(),
                &report.checksum_sha256,
            )
            .unwrap()
            .unwrap();
        assert!(!recorded.promoted);
    }

    #[test]
    fn changed_legacy_copy_after_promotion_cannot_resurrect_cleared_data() {
        let fixture = Fixture::new();
        let source = fixture.0.join("hk-market-data.json");
        fs::write(&source, br#"{"price":100}"#).unwrap();
        let mut store = DataStore::open(fixture.db()).unwrap();
        let importer =
            |value: Value, _: &str, checksum: &str| Ok(vec![fixture_record(checksum, value)]);
        let first = import_legacy_json(
            &mut store,
            &source,
            DataDomain::Quote,
            Market::HongKong,
            importer,
        )
        .unwrap();
        assert!(first.promoted);

        store
            .replace_domain_batch(DataDomain::Quote, Market::HongKong, &[])
            .unwrap();
        fs::write(&source, br#"{"price":99}"#).unwrap();
        let changed = import_legacy_json(
            &mut store,
            &source,
            DataDomain::Quote,
            Market::HongKong,
            |value, _, checksum| Ok(vec![fixture_record(checksum, value)]),
        )
        .unwrap();
        assert_eq!(changed.checksum_sha256, first.checksum_sha256);
        assert!(store
            .read_domain_records(DataDomain::Quote, Market::HongKong)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn normalized_records_must_stay_inside_requested_market_domain() {
        let fixture = Fixture::new();
        let source = fixture.0.join("hk.json");
        fs::write(&source, b"{}").unwrap();
        let mut store = DataStore::open(fixture.db()).unwrap();
        let report = import_legacy_json(
            &mut store,
            &source,
            DataDomain::Quote,
            Market::HongKong,
            |_, _, checksum| {
                Ok(vec![StoredRecord::new(
                    DataDomain::Quote,
                    Market::China,
                    "600000.SH",
                    "quote",
                    "legacy-test",
                    "legacy",
                    "now",
                    None,
                    None,
                    "unverified",
                    vec![checksum.to_string()],
                    json!({"price": 1.0}),
                )])
            },
        )
        .unwrap();
        assert!(!report.promoted);
        assert_eq!(report.validation_state, "migration_degraded");
    }
}
