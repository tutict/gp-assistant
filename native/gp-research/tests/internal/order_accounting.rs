use super::Portfolio;
use crate::{DatasetReader, ResearchStore};
use chrono::NaiveDate;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

fn reader() -> DatasetReader {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let app = std::env::temp_dir().join(format!(
        "gp-order-test-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let store = ResearchStore::open(&app).unwrap();
    let record = store
        .import_bundle(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tiny"))
        .unwrap();
    store.dataset(&record.dataset_id).unwrap()
}

#[test]
fn partial_buy_records_executed_and_unfunded_share_intent() {
    let dataset = reader();
    let mut portfolio = Portfolio::new(5.0);
    let mut trades = Vec::new();
    portfolio
        .execute(
            &dataset,
            NaiveDate::from_ymd_opt(2026, 1, 26).unwrap(),
            "000001.SZ",
            "buy",
            1.0,
            0.0,
            false,
            &mut trades,
        )
        .unwrap();
    assert_eq!(trades.len(), 2);
    assert!(trades[0].executed);
    assert_eq!(trades[0].shares, 0.5);
    assert_eq!(trades[0].notional, 5.0);
    assert!(!trades[1].executed);
    assert_eq!(trades[1].shares, 0.5);
    assert_eq!(
        trades[1].reason.as_deref(),
        Some("insufficient spendable cash")
    );
    assert_eq!(portfolio.cash, 0.0);
}

#[test]
fn unfunded_buy_is_an_execution_failure_with_exact_price_evidence() {
    let dataset = reader();
    let mut portfolio = Portfolio::new(0.0);
    let mut trades = Vec::new();
    portfolio
        .execute(
            &dataset,
            NaiveDate::from_ymd_opt(2026, 1, 26).unwrap(),
            "000001.SZ",
            "buy",
            1.0,
            0.0,
            false,
            &mut trades,
        )
        .unwrap();
    assert_eq!(trades.len(), 1);
    assert!(!trades[0].executed);
    assert_eq!(trades[0].shares, 1.0);
    assert_eq!(trades[0].price, Some(10.0));
    assert_eq!(
        trades[0].reason.as_deref(),
        Some("insufficient spendable cash")
    );
    assert!(portfolio.holdings.is_empty());
}
