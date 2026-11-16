use crate::research::storage::ResearchStore;
use crate::research::storage::INITIALIZED_DATABASES;
use rusqlite::Connection;
use std::fs;

#[test]
fn local_citation_extension_is_versioned_separately_from_portable_v2() {
    let root = std::env::temp_dir().join(crate::agent_ledger::next_run_id());
    fs::create_dir_all(&root).unwrap();
    let path = root.join("research.sqlite");
    drop(ResearchStore::open(&path).unwrap());
    let old = Connection::open(&path).unwrap();
    old.execute_batch("DROP TABLE research_unavailable_citations; PRAGMA user_version=2;")
        .unwrap();
    drop(old);
    INITIALIZED_DATABASES
        .get()
        .unwrap()
        .lock()
        .unwrap()
        .remove(&path);
    let store = ResearchStore::open(&path).unwrap();
    assert_eq!(
        store
            .connection()
            .unwrap()
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert!(fs::read_dir(&root).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains("before-v3")));
    drop(store);
    fs::remove_dir_all(root).unwrap();
}
