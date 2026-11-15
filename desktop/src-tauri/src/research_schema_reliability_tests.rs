use crate::research::storage::ResearchStore;
use crate::research::storage::INITIALIZED_DATABASES;
use rusqlite::Connection;
use std::fs;

#[test]
fn future_schema_is_rejected_without_changing_database_bytes() {
    let path = std::env::temp_dir().join(format!(
        "{}-future.sqlite",
        crate::agent_ledger::next_run_id()
    ));
    drop(ResearchStore::open(&path).unwrap());
    let c = Connection::open(&path).unwrap();
    c.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA user_version=99; CREATE TABLE future(value TEXT); INSERT INTO future VALUES('preserve');").unwrap();
    drop(c);
    let before = fs::read(&path).unwrap();
    INITIALIZED_DATABASES
        .get()
        .unwrap()
        .lock()
        .unwrap()
        .remove(&path); // simulate a new process
    assert!(ResearchStore::open(&path).is_err());
    assert!(
        fs::read(&path).unwrap() == before,
        "reject newer schema before changing journal mode"
    );
    fs::remove_file(path).unwrap();
}
