use super::store::RefreshRunState;
use super::{DataDomain, DataStore, Market, RefreshRequest, RefreshScope, StoredRecord};
use rusqlite::{Connection, TransactionBehavior};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct TempStore(PathBuf);

impl TempStore {
    fn new(label: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "gp-external-data-{label}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn db(&self) -> PathBuf {
        self.0.join("external-data.sqlite")
    }
}

impl Drop for TempStore {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn record(source_id: &str, record_key: &str, payload: Value) -> StoredRecord {
    StoredRecord::new(
        DataDomain::Quote,
        Market::HongKong,
        "00700.HK",
        record_key,
        source_id,
        "public",
        "2026-10-20T00:00:00Z",
        Some("2026-10-19T08:00:00Z"),
        None,
        "unverified",
        vec![],
        payload,
    )
}

#[test]
fn fresh_store_reopens_and_reads_its_records() {
    let fixture = TempStore::new("reopen");
    {
        let mut store = DataStore::open(fixture.db()).unwrap();
        store
            .upsert_batch(&[record("tencent", "quote", json!({"price": 417.8}))])
            .unwrap();
    }

    let store = DataStore::open(fixture.db()).unwrap();
    let records = store
        .read_records(DataDomain::Quote, Market::HongKong, "00700.HK")
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].payload["price"], 417.8);
    assert_eq!(records[0].metadata.source_id, "tencent");
}

#[test]
fn repeated_upsert_is_idempotent_and_preserves_source_candidates() {
    let fixture = TempStore::new("candidates");
    let mut store = DataStore::open(fixture.db()).unwrap();
    let candidate = record("tencent", "price", json!({"value": 417.8}));
    store.upsert_batch(&[candidate.clone()]).unwrap();
    store.upsert_batch(&[candidate]).unwrap();
    store
        .upsert_batch(&[record("eastmoney", "price", json!({"value": 418.0}))])
        .unwrap();

    let records = store
        .read_records(DataDomain::Quote, Market::HongKong, "00700.HK")
        .unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(
        records
            .iter()
            .filter(|item| item.metadata.source_id == "tencent")
            .count(),
        1
    );
    assert_eq!(
        records
            .iter()
            .filter(|item| item.metadata.source_id == "eastmoney")
            .count(),
        1
    );
}

#[test]
fn invalid_record_rejects_the_whole_batch_before_any_write() {
    let fixture = TempStore::new("atomic-validation");
    let mut store = DataStore::open(fixture.db()).unwrap();
    let valid = record("tencent", "quote", json!({"price": 417.8}));
    let mut invalid = record("tencent", "bad", json!({"price": 1.0}));
    invalid.entity_key.clear();

    assert!(store.upsert_batch(&[valid, invalid]).is_err());
    assert!(store
        .read_records(DataDomain::Quote, Market::HongKong, "00700.HK")
        .unwrap()
        .is_empty());
}

#[test]
fn database_failure_after_a_valid_row_rolls_back_the_entire_batch() {
    let fixture = TempStore::new("atomic-sql");
    let mut store = DataStore::open(fixture.db()).unwrap();
    let connection = Connection::open(fixture.db()).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_fixture_record BEFORE INSERT ON records
             WHEN NEW.record_key='bad'
             BEGIN SELECT RAISE(ABORT, 'fixture rejected row'); END;",
        )
        .unwrap();

    let valid = record("tencent", "quote", json!({"price": 417.8}));
    let bad = record("tencent", "bad", json!({"value": 1}));
    assert!(store.upsert_batch(&[valid, bad]).is_err());
    assert!(store
        .read_records(DataDomain::Quote, Market::HongKong, "00700.HK")
        .unwrap()
        .is_empty());
}

#[test]
fn future_schema_is_rejected_without_rewriting_the_database() {
    let fixture = TempStore::new("future-schema");
    let db = fixture.db();
    {
        let conn = Connection::open(&db).unwrap();
        conn.pragma_update(None, "application_id", 0x47504558_i64)
            .unwrap();
        conn.pragma_update(None, "user_version", 99_i64).unwrap();
        conn.execute_batch(
            "CREATE TABLE sentinel (value TEXT); INSERT INTO sentinel VALUES ('keep');",
        )
        .unwrap();
    }
    let before = fs::read(&db).unwrap();

    assert!(DataStore::open(&db).is_err());
    assert_eq!(fs::read(&db).unwrap(), before);
}

#[test]
fn checkpoint_keeps_committed_records_readable() {
    let fixture = TempStore::new("checkpoint");
    let mut store = DataStore::open(fixture.db()).unwrap();
    store
        .upsert_batch(&[record("tencent", "quote", json!({"price": 417.8}))])
        .unwrap();
    store.checkpoint().unwrap();

    assert_eq!(
        store
            .read_records(DataDomain::Quote, Market::HongKong, "00700.HK")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn write_lock_failure_is_bounded() {
    let fixture = TempStore::new("busy");
    let mut store = DataStore::open(fixture.db()).unwrap();
    let mut blocker = Connection::open(fixture.db()).unwrap();
    blocker.busy_timeout(Duration::from_secs(2)).unwrap();
    let transaction = blocker
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();

    let started = std::time::Instant::now();
    let result = store.upsert_batch(&[record("tencent", "quote", json!({"price": 417.8}))]);
    assert!(result.is_err());
    assert!(started.elapsed() < Duration::from_secs(2));
    drop(transaction);
}

#[test]
fn source_state_keeps_last_success_when_a_later_attempt_fails() {
    let fixture = TempStore::new("source-state");
    let store = DataStore::open(fixture.db()).unwrap();
    store
        .record_source_attempt("tencent", DataDomain::Quote, Market::HongKong, "attempt-1")
        .unwrap();
    store
        .record_source_success(
            "tencent",
            DataDomain::Quote,
            Market::HongKong,
            "success-1",
            4,
        )
        .unwrap();
    store
        .record_source_attempt("tencent", DataDomain::Quote, Market::HongKong, "attempt-2")
        .unwrap();
    let long_error = "港".repeat(1_000);
    store
        .record_source_failure(
            "tencent",
            DataDomain::Quote,
            Market::HongKong,
            "attempt-2",
            "timeout",
            &long_error,
        )
        .unwrap();

    let status = store
        .refresh_status(DataDomain::Quote, Market::HongKong)
        .unwrap();
    assert_eq!(status.len(), 1);
    assert_eq!(status[0].last_success_at.as_deref(), Some("success-1"));
    assert_eq!(status[0].last_attempt_at.as_deref(), Some("attempt-2"));
    assert_eq!(status[0].last_error_category.as_deref(), Some("timeout"));
    assert_eq!(status[0].record_count, 4);
    assert!(status[0].last_error.as_ref().unwrap().len() <= 2 * 1024);
    assert!(!status[0].last_error.as_ref().unwrap().contains('�'));
}

#[test]
fn oversized_record_rejects_the_whole_batch() {
    let fixture = TempStore::new("oversized");
    let mut store = DataStore::open(fixture.db()).unwrap();
    let valid = record("tencent", "quote", json!({"price": 417.8}));
    let oversized = record(
        "tencent",
        "too-large",
        json!({"payload": "x".repeat(2 * 1024 * 1024)}),
    );

    assert!(store.upsert_batch(&[valid, oversized]).is_err());
    assert!(store
        .read_records(DataDomain::Quote, Market::HongKong, "00700.HK")
        .unwrap()
        .is_empty());
}

fn refresh_request(code: &str) -> RefreshRequest {
    RefreshRequest {
        domain: DataDomain::Quote,
        market: Market::HongKong,
        scope: RefreshScope::Entities {
            codes: vec![code.to_string()],
        },
        request_id: Some("request-1".into()),
    }
}

#[test]
fn refresh_run_creation_is_idempotent_and_rejects_key_scope_conflicts() {
    let fixture = TempStore::new("refresh-dedupe");
    let mut store = DataStore::open(fixture.db()).unwrap();
    let request = refresh_request("00700.HK");
    let (created, is_new) = store.create_refresh_run(&request, "request-1", 10).unwrap();
    assert!(is_new);
    let (existing, is_new) = store.create_refresh_run(&request, "request-1", 11).unwrap();
    assert!(!is_new);
    assert_eq!(existing.run_id, created.run_id);
    assert_eq!(existing.state, RefreshRunState::Queued);

    assert!(store
        .create_refresh_run(&refresh_request("00005.HK"), "request-1", 12)
        .is_err());
}

#[test]
fn explicit_refresh_gets_one_foreground_recovery_then_needs_user_retry() {
    let fixture = TempStore::new("refresh-recovery");
    let mut store = DataStore::open(fixture.db()).unwrap();
    let request = refresh_request("00700.HK");
    let (run, _) = store.create_refresh_run(&request, "request-1", 10).unwrap();
    store.start_refresh_run(&run.run_id, false, 11).unwrap();
    store
        .finish_refresh_run(
            &run.run_id,
            RefreshRunState::Retryable,
            12,
            Some("timeout"),
            Some("source unavailable"),
        )
        .unwrap();

    let eligible = store.foreground_recoveries().unwrap();
    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].run_id, run.run_id);
    store.start_refresh_run(&run.run_id, true, 13).unwrap();
    store
        .finish_refresh_run(
            &run.run_id,
            RefreshRunState::Retryable,
            14,
            Some("http_5xx"),
            Some("provider still unavailable"),
        )
        .unwrap();

    let status = store.refresh_run_status(&run.run_id).unwrap().unwrap();
    assert_eq!(status.state, RefreshRunState::NeedsUserRetry);
    assert_eq!(status.recovery_count, 1);
    assert!(store.foreground_recoveries().unwrap().is_empty());
}

#[test]
fn explicit_refresh_can_manually_retry_a_retryable_run_without_consuming_recovery() {
    let fixture = TempStore::new("refresh-manual-retry");
    let mut store = DataStore::open(fixture.db()).unwrap();
    let request = refresh_request("00700.HK");
    let (run, _) = store.create_refresh_run(&request, "request-1", 10).unwrap();
    store.start_refresh_run(&run.run_id, false, 11).unwrap();
    store
        .finish_refresh_run(
            &run.run_id,
            RefreshRunState::Retryable,
            12,
            Some("timeout"),
            Some("source unavailable"),
        )
        .unwrap();

    store.start_refresh_run(&run.run_id, false, 13).unwrap();
    let status = store.refresh_run_status(&run.run_id).unwrap().unwrap();
    assert_eq!(status.state, RefreshRunState::Running);
    assert_eq!(status.attempt_count, 2);
    assert_eq!(status.recovery_count, 0);

    store
        .finish_refresh_run(&run.run_id, RefreshRunState::Completed, 14, None, None)
        .unwrap();
    assert_eq!(
        store.refresh_run_status(&run.run_id).unwrap().unwrap().state,
        RefreshRunState::Completed
    );
}

#[test]
fn cleanup_requires_a_matching_preview_and_only_removes_selected_external_domains() {
    let fixture = TempStore::new("cleanup-confirmation");
    let mut store = DataStore::open(fixture.db()).unwrap();
    store
        .upsert_batch(&[
            record("tencent", "quote", json!({"price": 417.8})),
            StoredRecord::new(
                DataDomain::DailyHistory,
                Market::HongKong,
                "00700.HK",
                "history",
                "tencent",
                "public",
                "2026-10-20T00:00:00Z",
                Some("2026-10-19"),
                None,
                "unverified",
                vec![],
                json!({"close": 417.8}),
            ),
        ])
        .unwrap();
    let domains = [DataDomain::Quote];
    let preview = store.cleanup_preview(&domains, None).unwrap();
    assert_eq!(preview["record_count"], 1);
    assert_eq!(preview["requires_confirmation"], true);
    let token = preview["preview_token"].as_str().unwrap().to_string();
    assert_eq!(
        store
            .cleanup_confirmed(&domains, None, "stale-token")
            .unwrap_err(),
        "cleanup scope changed; generate a new preview before confirming"
    );
    assert_eq!(store.cleanup_confirmed(&domains, None, &token).unwrap(), 1);
    assert!(store
        .read_records(DataDomain::Quote, Market::HongKong, "00700.HK")
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .read_records(DataDomain::DailyHistory, Market::HongKong, "00700.HK")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn changed_records_invalidate_the_cleanup_preview_token() {
    let fixture = TempStore::new("cleanup-preview-stale");
    let mut store = DataStore::open(fixture.db()).unwrap();
    store
        .upsert_batch(&[record("tencent", "quote", json!({"price": 417.8}))])
        .unwrap();
    let domains = [DataDomain::Quote];
    let preview = store.cleanup_preview(&domains, None).unwrap();
    let token = preview["preview_token"].as_str().unwrap().to_string();
    store
        .upsert_batch(&[record("tencent", "quote", json!({"price": 420.0}))])
        .unwrap();

    assert!(store.cleanup_confirmed(&domains, None, &token).is_err());
    assert_eq!(
        store
            .read_records(DataDomain::Quote, Market::HongKong, "00700.HK")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn cleanup_cutoff_normalizes_provider_dates_and_epoch_timestamps() {
    let fixture = TempStore::new("cleanup-date-normalization");
    let mut store = DataStore::open(fixture.db()).unwrap();
    store
        .upsert_batch(&[
            record("tencent", "slash-date", json!({"price": 10})),
            StoredRecord::new(
                DataDomain::Quote,
                Market::HongKong,
                "00005.HK",
                "epoch-time",
                "tencent",
                "public",
                "1735689600000",
                None,
                None,
                "unverified",
                vec![],
                json!({"price": 70}),
            ),
        ])
        .unwrap();
    store
        .upsert_batch(&[StoredRecord::new(
            DataDomain::Quote,
            Market::HongKong,
            "00005.HK",
            "slash-time",
            "tencent",
            "public",
            "2024/12/31 16:00:00",
            None,
            None,
            "unverified",
            vec![],
            json!({"price": 71}),
        )])
        .unwrap();

    let preview = store
        .cleanup_preview(&[DataDomain::Quote], Some("2025-01-02"))
        .unwrap();
    assert_eq!(preview["record_count"], 2);
    let token = preview["preview_token"].as_str().unwrap();
    assert_eq!(
        store
            .cleanup_confirmed(&[DataDomain::Quote], Some("2025-01-02"), token)
            .unwrap(),
        2
    );
}

#[test]
fn store_path_is_separate_from_user_research_database() {
    let app_data = Path::new("C:/test/app-data");
    assert_eq!(
        super::store_path(app_data),
        app_data.join("external-data").join("external-data.sqlite")
    );
}
