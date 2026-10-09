use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use tauri::Manager;

pub(crate) const WATCHLIST_DB_FILE: &str = "watchlist.sqlite";
const WATCHLIST_SCHEMA_VERSION: i64 = 2;

#[tauri::command]
pub(crate) async fn api_watchlist_snapshot(app: tauri::AppHandle) -> Result<Value, String> {
    crate::runtime::run_user_state_bound("api_watchlist_snapshot", move || {
        watchlist_snapshot(&mut open_watchlist_db(&app)?)
    })
    .await?
}

#[tauri::command]
pub(crate) async fn api_watchlist_mutate(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    crate::runtime::run_user_state_bound("api_watchlist_mutate", move || {
        mutate_watchlist(&mut open_watchlist_db(&app)?, &payload)
    })
    .await?
}

// Legacy commands retain their array responses, but all writes take the same
// IMMEDIATE transaction and increment the same revision as the delta protocol.
#[tauri::command]
pub(crate) async fn api_watchlist_list(app: tauri::AppHandle) -> Result<Value, String> {
    crate::runtime::run_user_state_bound("api_watchlist_list", move || watchlist_list_sync(&app))
        .await?
}

#[tauri::command]
pub(crate) async fn api_watchlist_replace(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    crate::runtime::run_user_state_bound("api_watchlist_replace", move || {
        let items = watchlist_items_from_payload(&payload)?;
        watchlist_replace_sync(&app, items)
    })
    .await?
}

#[tauri::command]
pub(crate) async fn api_watchlist_add(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    crate::runtime::run_user_state_bound("api_watchlist_add", move || {
        legacy_mutation(
            &mut open_watchlist_db(&app)?,
            WatchlistMutation::Delta {
                upserts: vec![payload],
                removes: vec![],
            },
        )
    })
    .await?
}

#[tauri::command]
pub(crate) async fn api_watchlist_remove(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    crate::runtime::run_user_state_bound("api_watchlist_remove", move || {
        let code = payload
            .get("code")
            .and_then(Value::as_str)
            .ok_or_else(|| "watchlist code is required".to_string())?;
        legacy_mutation(
            &mut open_watchlist_db(&app)?,
            WatchlistMutation::Delta {
                upserts: vec![],
                removes: vec![code.to_string()],
            },
        )
    })
    .await?
}

#[tauri::command]
pub(crate) async fn api_watchlist_clear(app: tauri::AppHandle) -> Result<Value, String> {
    crate::runtime::run_user_state_bound("api_watchlist_clear", move || {
        legacy_mutation(&mut open_watchlist_db(&app)?, WatchlistMutation::Clear)
    })
    .await?
}

#[derive(Clone, Debug)]
pub(crate) struct WatchlistRecord {
    pub(crate) code: String,
    pub(crate) name: Option<String>,
    pub(crate) industry: Option<String>,
    pub(crate) added_at: String,
    pub(crate) source: Option<String>,
    pub(crate) screen_criteria_summary: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum WatchlistMutation {
    Delta {
        upserts: Vec<Value>,
        removes: Vec<String>,
    },
    Clear,
    Migrate {
        items: Vec<Value>,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MutationRequest {
    operation_id: String,
    expected_revision: i64,
    mutation: WatchlistMutation,
}

pub(crate) fn watchlist_db_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("get app data dir failed: {error}"))?;
    root.push("watchlist");
    fs::create_dir_all(&root)
        .map_err(|error| format!("create watchlist dir failed: {}: {error}", root.display()))?;
    root.push(WATCHLIST_DB_FILE);
    Ok(root)
}

pub(crate) fn open_watchlist_db(app: &tauri::AppHandle) -> Result<Connection, String> {
    open_watchlist_db_path(&watchlist_db_path(app)?)
}

fn open_watchlist_db_path(path: &Path) -> Result<Connection, String> {
    // Connection::open silently accepts an existing zero-byte database. Check
    // existence and its header first; damage must not become a fresh install.
    let fresh_file = match fs::symlink_metadata(path) {
        Ok(_) => {
            crate::durability::validate_sqlite_header(path)?;
            false
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) => {
            return Err(format!(
                "inspect watchlist sqlite failed: {}: {error}",
                path.display()
            ))
        }
    };
    let mut conn = Connection::open(path)
        .map_err(|error| format!("open watchlist sqlite failed: {}: {error}", path.display()))?;
    crate::durability::backup_before_migration(
        &conn,
        path,
        schema_version(&conn)?,
        WATCHLIST_SCHEMA_VERSION,
    )?;
    initialize_watchlist_db(&mut conn, fresh_file)?;
    Ok(conn)
}

fn sql_error(error: rusqlite::Error) -> String {
    format!("watchlist sqlite: {error}")
}

fn schema_version(conn: &Connection) -> Result<i64, String> {
    let version = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(sql_error)?;
    if !(0..=WATCHLIST_SCHEMA_VERSION).contains(&version) {
        return Err(format!(
            "unsupported watchlist schema version {version}; maximum is {WATCHLIST_SCHEMA_VERSION}"
        ));
    }
    Ok(version)
}

fn initialize_watchlist_db(conn: &mut Connection, fresh_file: bool) -> Result<(), String> {
    // Reject a newer database before changing even its journal policy.
    schema_version(conn)?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(sql_error)?;
    conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL;")
        .map_err(sql_error)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let version = schema_version(&tx)?;
    if version == 0 {
        let existing: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='watchlist')",
            [], |r| r.get(0),
        ).map_err(sql_error)?;
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS watchlist (
                code TEXT PRIMARY KEY NOT NULL, name TEXT, industry TEXT, added_at TEXT NOT NULL,
                source TEXT, screen_criteria_summary TEXT, updated_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_watchlist_added_at ON watchlist(added_at DESC);
             CREATE TABLE watchlist_state (
                singleton INTEGER PRIMARY KEY CHECK(singleton=1),
                revision INTEGER NOT NULL CHECK(revision>=0),
                migration_complete INTEGER NOT NULL CHECK(migration_complete IN (0,1)),
                restore_pristine INTEGER NOT NULL DEFAULT 0 CHECK(restore_pristine IN (0,1))
             );
             CREATE TABLE watchlist_operations (
                operation_id TEXT PRIMARY KEY NOT NULL,
                payload TEXT NOT NULL,
                response TEXT NOT NULL
             );",
        )
        .map_err(sql_error)?;
        // Existing v0 databases, even empty ones, are authoritative. Empty is
        // never a heuristic for importing stale localStorage again.
        tx.execute(
            "INSERT INTO watchlist_state(singleton,revision,migration_complete,restore_pristine) VALUES (1,0,?1,?2)",
            params![existing || !fresh_file, fresh_file && !existing],
        )
        .map_err(sql_error)?;
    } else if version == 1 {
        // Empty/revision-zero v1 is not proof of a never-used database. Its
        // history is unknown, so it must not authorize native backup restore.
        tx.execute_batch("ALTER TABLE watchlist_state ADD COLUMN restore_pristine INTEGER NOT NULL DEFAULT 0 CHECK(restore_pristine IN (0,1));")
            .map_err(sql_error)?;
    }
    if version < WATCHLIST_SCHEMA_VERSION {
        tx.pragma_update(None, "user_version", WATCHLIST_SCHEMA_VERSION)
            .map_err(sql_error)?;
    }
    tx.commit().map_err(sql_error)
}

fn snapshot_in_transaction(conn: &Connection) -> Result<Value, String> {
    let (revision, migration_complete): (i64, bool) = conn
        .query_row(
            "SELECT revision, migration_complete FROM watchlist_state WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(sql_error)?;
    Ok(
        json!({"revision": revision, "migrationComplete": migration_complete, "items": watchlist_rows(conn)?}),
    )
}

fn watchlist_snapshot(conn: &mut Connection) -> Result<Value, String> {
    let tx = conn.transaction().map_err(sql_error)?;
    let result = snapshot_in_transaction(&tx)?;
    tx.commit().map_err(sql_error)?;
    Ok(result)
}

fn upsert_records(conn: &Connection, items: Vec<WatchlistRecord>) -> Result<(), String> {
    let mut stmt = conn.prepare(
        "INSERT INTO watchlist (code,name,industry,added_at,source,screen_criteria_summary,updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(code) DO UPDATE SET name=excluded.name, industry=excluded.industry,
         source=excluded.source, screen_criteria_summary=excluded.screen_criteria_summary,
         updated_at=excluded.updated_at",
    ).map_err(sql_error)?;
    for item in normalize_watchlist_records(items) {
        stmt.execute(params![
            item.code,
            item.name,
            item.industry,
            item.added_at,
            item.source,
            item.screen_criteria_summary,
            crate::market::epoch_millis() as i64
        ])
        .map_err(sql_error)?;
    }
    Ok(())
}

fn apply_mutation(conn: &Connection, mutation: WatchlistMutation) -> Result<(), String> {
    let mut consumes_pristine = true;
    match mutation {
        WatchlistMutation::Delta { upserts, removes } => {
            let records = upserts
                .iter()
                .map(watchlist_item_from_value)
                .collect::<Result<Vec<_>, _>>()?;
            for code in removes {
                let code = crate::market::normalize_stock_code(&code)
                    .ok_or_else(|| "watchlist code is required".to_string())?;
                conn.execute("DELETE FROM watchlist WHERE code=?1", params![code])
                    .map_err(sql_error)?;
            }
            upsert_records(conn, records)?;
        }
        WatchlistMutation::Clear => {
            conn.execute("DELETE FROM watchlist", [])
                .map_err(sql_error)?;
        }
        WatchlistMutation::Migrate { items } => {
            consumes_pristine = false;
            let complete: bool = conn
                .query_row(
                    "SELECT migration_complete FROM watchlist_state WHERE singleton=1",
                    [],
                    |r| r.get(0),
                )
                .map_err(sql_error)?;
            if !complete {
                let records = items
                    .iter()
                    .map(watchlist_item_from_value)
                    .collect::<Result<Vec<_>, _>>()?;
                consumes_pristine = !records.is_empty();
                upsert_records(conn, records)?;
            }
        }
    }
    conn.execute(
        "UPDATE watchlist_state SET revision=revision+1, migration_complete=1,
         restore_pristine=CASE WHEN ?1 THEN 0 ELSE restore_pristine END WHERE singleton=1",
        params![consumes_pristine],
    )
    .map_err(sql_error)?;
    Ok(())
}

fn mutate_watchlist(conn: &mut Connection, payload: &Value) -> Result<Value, String> {
    let request: MutationRequest = serde_json::from_value(payload.clone())
        .map_err(|e| format!("invalid watchlist mutation: {e}"))?;
    if request.operation_id.trim().is_empty()
        || request.operation_id.len() > 200
        || request.expected_revision < 0
    {
        return Err(
            "watchlist operationId (1..200 bytes) and nonnegative expectedRevision are required"
                .to_string(),
        );
    }
    let encoded = serde_json::to_string(payload).map_err(|e| e.to_string())?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let duplicate: Option<(String, String)> = tx
        .query_row(
            "SELECT payload,response FROM watchlist_operations WHERE operation_id=?1",
            params![request.operation_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(sql_error)?;
    if let Some((original, response)) = duplicate {
        if original != encoded {
            return Err(json!({"code":"WATCHLIST_OPERATION_REUSED", "message":"operationId already committed with a different payload"}).to_string());
        }
        // Check duplicate BEFORE revision. A lost acknowledgement must be
        // retryable even after other writers have advanced the database.
        return serde_json::from_str(&response).map_err(|e| e.to_string());
    }
    let revision: i64 = tx
        .query_row(
            "SELECT revision FROM watchlist_state WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .map_err(sql_error)?;
    if revision != request.expected_revision {
        return Err(json!({"code":"WATCHLIST_CONFLICT", "revision":revision, "message":"watchlist changed; reload and explicitly retry the delta"}).to_string());
    }
    apply_mutation(&tx, request.mutation)?;
    let response = snapshot_in_transaction(&tx)?;
    tx.execute(
        "INSERT INTO watchlist_operations(operation_id,payload,response) VALUES (?1,?2,?3)",
        params![request.operation_id, encoded, response.to_string()],
    )
    .map_err(sql_error)?;
    tx.commit().map_err(sql_error)?;
    Ok(response)
}

fn legacy_mutation(conn: &mut Connection, mutation: WatchlistMutation) -> Result<Value, String> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    apply_mutation(&tx, mutation)?;
    let items = watchlist_rows(&tx)?;
    tx.commit().map_err(sql_error)?;
    Ok(items)
}

pub(crate) fn watchlist_list_sync(app: &tauri::AppHandle) -> Result<Value, String> {
    watchlist_rows(&open_watchlist_db(app)?)
}

pub(crate) fn watchlist_replace_sync(
    app: &tauri::AppHandle,
    items: Vec<WatchlistRecord>,
) -> Result<Value, String> {
    let mut conn = open_watchlist_db(app)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    apply_mutation(&tx, WatchlistMutation::Clear)?;
    upsert_records(&tx, items)?;
    let result = watchlist_rows(&tx)?;
    tx.commit().map_err(sql_error)?;
    Ok(result)
}
pub(crate) fn watchlist_rows(conn: &Connection) -> Result<Value, String> {
    let mut stmt = conn
        .prepare(
            "SELECT code, name, industry, added_at, source, screen_criteria_summary
             FROM watchlist
             ORDER BY added_at DESC, updated_at DESC, code ASC",
        )
        .map_err(|error| format!("prepare watchlist query failed: {error}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(json!({
                "code": row.get::<_, String>(0)?,
                "name": row.get::<_, Option<String>>(1)?,
                "industry": row.get::<_, Option<String>>(2)?,
                "added_at": row.get::<_, String>(3)?,
                "source": row.get::<_, Option<String>>(4)?,
                "screenCriteriaSummary": row.get::<_, Option<String>>(5)?,
            }))
        })
        .map_err(|error| format!("query watchlist failed: {error}"))?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row.map_err(|error| format!("read watchlist row failed: {error}"))?);
    }
    Ok(Value::Array(items))
}

pub(crate) fn watchlist_items_from_payload(
    payload: &Value,
) -> Result<Vec<WatchlistRecord>, String> {
    let items = payload
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| "watchlist items array is required".to_string())?;
    items.iter().map(watchlist_item_from_value).collect()
}

pub(crate) fn watchlist_item_from_value(value: &Value) -> Result<WatchlistRecord, String> {
    let code = value
        .get("code")
        .and_then(Value::as_str)
        .and_then(crate::market::normalize_stock_code)
        .ok_or_else(|| "watchlist code is required".to_string())?;
    Ok(WatchlistRecord {
        code,
        name: crate::market::optional_trimmed_string(value.get("name")),
        industry: crate::market::optional_trimmed_string(value.get("industry")),
        added_at: crate::market::optional_trimmed_string(value.get("added_at"))
            .unwrap_or_else(|| crate::market::epoch_millis().to_string()),
        source: crate::market::optional_trimmed_string(value.get("source")),
        screen_criteria_summary: crate::market::optional_trimmed_string(
            value.get("screenCriteriaSummary"),
        )
        .or_else(|| crate::market::optional_trimmed_string(value.get("screen_criteria_summary"))),
    })
}

pub(crate) fn normalize_watchlist_records(items: Vec<WatchlistRecord>) -> Vec<WatchlistRecord> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for item in items {
        if item.code.is_empty() || !seen.insert(item.code.clone()) {
            continue;
        }
        normalized.push(item);
    }
    normalized
}

#[cfg(test)]
mod reliability_tests {
    use super::*;

    fn db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        initialize_watchlist_db(&mut conn, true).unwrap();
        conn
    }
    fn item(code: &str) -> Value {
        json!({"code": code, "added_at": "2026-01-01"})
    }
    fn request(id: &str, revision: i64, mutation: Value) -> Value {
        json!({"operationId": id, "expectedRevision": revision, "mutation": mutation})
    }
    fn add(id: &str, revision: i64, code: &str) -> Value {
        request(
            id,
            revision,
            json!({"kind":"delta", "upserts":[item(code)], "removes":[]}),
        )
    }

    #[test]
    fn ordered_deltas_conflict_and_duplicate_safe_retry() {
        let mut conn = db();
        let first = add("one", 0, "000001.SZ");
        let first_result = mutate_watchlist(&mut conn, &first).unwrap();
        assert_eq!(first_result["revision"], 1);
        let second = add("two", 1, "000002.SZ");
        let second_result = mutate_watchlist(&mut conn, &second).unwrap();
        assert_eq!(second_result["items"].as_array().unwrap().len(), 2);
        assert_eq!(mutate_watchlist(&mut conn, &first).unwrap(), first_result);
        assert_eq!(watchlist_snapshot(&mut conn).unwrap()["revision"], 2);
        assert!(mutate_watchlist(&mut conn, &add("three", 0, "000003.SZ"))
            .unwrap_err()
            .contains("WATCHLIST_CONFLICT"));
        assert!(mutate_watchlist(&mut conn, &add("one", 0, "000003.SZ"))
            .unwrap_err()
            .contains("WATCHLIST_OPERATION_REUSED"));
        assert_eq!(watchlist_rows(&conn).unwrap().as_array().unwrap().len(), 2);
    }

    #[test]
    fn migration_runs_once_and_never_resurrects_cleared_database() {
        let mut conn = db();
        assert_eq!(
            watchlist_snapshot(&mut conn).unwrap()["migrationComplete"],
            false
        );
        let migrate = request(
            "import",
            0,
            json!({"kind":"migrate", "items":[item("000001.SZ")]}),
        );
        mutate_watchlist(&mut conn, &migrate).unwrap();
        mutate_watchlist(&mut conn, &request("clear", 1, json!({"kind":"clear"}))).unwrap();
        let result = mutate_watchlist(
            &mut conn,
            &request("import-again", 2, migrate["mutation"].clone()),
        )
        .unwrap();
        assert_eq!(result["items"], json!([]));
        assert_eq!(result["migrationComplete"], true);
    }

    #[test]
    fn even_empty_first_migration_is_final() {
        let mut conn = db();
        mutate_watchlist(
            &mut conn,
            &request("empty", 0, json!({"kind":"migrate", "items":[]})),
        )
        .unwrap();
        let result = mutate_watchlist(
            &mut conn,
            &request(
                "late",
                1,
                json!({"kind":"migrate", "items":[item("000001.SZ")]}),
            ),
        )
        .unwrap();
        assert_eq!(result["items"], json!([]));
    }

    #[test]
    fn v0_empty_and_populated_databases_are_already_authoritative() {
        for populated in [false, true] {
            let mut conn = Connection::open_in_memory().unwrap();
            conn.execute_batch("CREATE TABLE watchlist (code TEXT PRIMARY KEY NOT NULL, name TEXT, industry TEXT, added_at TEXT NOT NULL, source TEXT, screen_criteria_summary TEXT, updated_at INTEGER NOT NULL);").unwrap();
            if populated {
                conn.execute("INSERT INTO watchlist(code, added_at, updated_at) VALUES ('000001.SZ', 'old', 1)", []).unwrap();
            }
            initialize_watchlist_db(&mut conn, false).unwrap();
            let result = mutate_watchlist(
                &mut conn,
                &request(
                    "migration",
                    0,
                    json!({"kind":"migrate", "items":[item("000002.SZ")]}),
                ),
            )
            .unwrap();
            assert_eq!(
                result["items"].as_array().unwrap().len(),
                usize::from(populated)
            );
            if populated {
                assert_eq!(result["items"][0]["code"], "000001.SZ");
            }
            assert_eq!(
                conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                2
            );
        }
    }

    #[test]
    fn future_schema_is_rejected_without_rewriting_it() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA user_version = 99;").unwrap();
        assert!(initialize_watchlist_db(&mut conn, false)
            .unwrap_err()
            .contains("unsupported watchlist schema"));
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            99
        );
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='watchlist'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }

    #[test]
    fn invalid_delta_rolls_back_everything_including_revision_and_operation_id() {
        let mut conn = db();
        let bad = request(
            "bad",
            0,
            json!({"kind":"delta", "upserts":[item("000001.SZ"), item("")], "removes":[]}),
        );
        assert!(mutate_watchlist(&mut conn, &bad).is_err());
        let state = watchlist_snapshot(&mut conn).unwrap();
        assert_eq!(state["revision"], 0);
        assert_eq!(state["items"], json!([]));
        mutate_watchlist(&mut conn, &add("bad", 0, "000001.SZ")).unwrap();
    }

    #[test]
    fn legacy_writes_share_revision_and_disable_migration() {
        let mut conn = db();
        legacy_mutation(&mut conn, WatchlistMutation::Clear).unwrap();
        let state = watchlist_snapshot(&mut conn).unwrap();
        assert_eq!(state["revision"], 1);
        assert_eq!(state["migrationComplete"], true);
        assert!(mutate_watchlist(&mut conn, &add("stale", 0, "000001.SZ"))
            .unwrap_err()
            .contains("WATCHLIST_CONFLICT"));
    }

    struct TestDatabase(PathBuf);
    impl TestDatabase {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let suffix = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Self(std::env::temp_dir().join(format!(
                "watchlist-reliability-{}-{}-{suffix}.sqlite",
                std::process::id(),
                crate::market::epoch_millis()
            )))
        }
        fn open(&self) -> Connection {
            open_watchlist_db_path(&self.0).unwrap()
        }
    }
    impl Drop for TestDatabase {
        fn drop(&mut self) {
            for suffix in ["", "-wal", "-shm"] {
                let _ = fs::remove_file(format!("{}{suffix}", self.0.display()));
            }
        }
    }

    #[test]
    fn disk_reopen_preserves_ledger_marker_and_full_durability_policy() {
        let file = TestDatabase::new();
        let request = add("persistent-id", 0, "000001.SZ");
        let result = {
            let mut conn = file.open();
            assert_eq!(
                conn.query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0))
                    .unwrap(),
                "wal"
            );
            assert_eq!(
                conn.query_row("PRAGMA synchronous", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                2
            );
            assert_eq!(
                conn.query_row("PRAGMA busy_timeout", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                5000
            );
            mutate_watchlist(&mut conn, &request).unwrap()
        };
        let mut conn = file.open();
        assert_eq!(mutate_watchlist(&mut conn, &request).unwrap(), result);
        assert_eq!(
            watchlist_snapshot(&mut conn).unwrap()["migrationComplete"],
            true
        );
        assert_eq!(watchlist_snapshot(&mut conn).unwrap()["revision"], 1);
    }

    #[test]
    fn concurrent_writers_cannot_both_commit_the_same_revision() {
        let file = TestDatabase::new();
        drop(file.open());
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let threads: Vec<_> = ["000001.SZ", "000002.SZ"]
            .into_iter()
            .map(|code| {
                let barrier = barrier.clone();
                let path = file.0.clone();
                std::thread::spawn(move || {
                    let mut conn = Connection::open(path).unwrap();
                    initialize_watchlist_db(&mut conn, false).unwrap();
                    barrier.wait();
                    mutate_watchlist(&mut conn, &add(code, 0, code))
                })
            })
            .collect();
        let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert!(results
            .iter()
            .find_map(|r| r.as_ref().err())
            .unwrap()
            .contains("WATCHLIST_CONFLICT"));
        let state = watchlist_snapshot(&mut file.open()).unwrap();
        assert_eq!(state["revision"], 1);
        assert_eq!(state["items"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn partially_executed_delta_is_rolled_back_and_retry_id_remains_available() {
        let mut conn = db();
        mutate_watchlist(&mut conn, &add("seed", 0, "000001.SZ")).unwrap();
        let invalid = request(
            "remove",
            1,
            json!({"kind":"delta", "upserts":[], "removes":["000001.SZ", ""]}),
        );
        assert!(mutate_watchlist(&mut conn, &invalid).is_err());
        assert_eq!(
            watchlist_snapshot(&mut conn).unwrap()["items"][0]["code"],
            "000001.SZ"
        );
        let result = mutate_watchlist(
            &mut conn,
            &request(
                "remove",
                1,
                json!({"kind":"delta", "upserts":[], "removes":["000001.SZ"]}),
            ),
        )
        .unwrap();
        assert_eq!(result["items"], json!([]));
        assert_eq!(result["revision"], 2);
    }

    fn restore_pristine(conn: &Connection) -> bool {
        conn.query_row(
            "SELECT restore_pristine FROM watchlist_state WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn r2_existing_empty_or_invalid_header_is_not_recreated_or_migrated() {
        for contents in [
            b"".as_slice(),
            b"not a SQLite database at all".as_slice(),
            b"SQLite".as_slice(),
        ] {
            let file = TestDatabase::new();
            fs::write(&file.0, contents).unwrap();
            let result = open_watchlist_db_path(&file.0);
            assert!(
                result.is_err(),
                "existing damaged file must not be initialized"
            );
            assert_eq!(fs::read(&file.0).unwrap(), contents);
            assert!(!PathBuf::from(format!("{}-wal", file.0.display())).exists());
        }
    }

    #[test]
    fn fresh_database_is_pristine_even_after_empty_once_only_migration() {
        let file = TestDatabase::new();
        let mut conn = open_watchlist_db_path(&file.0).unwrap();
        assert!(restore_pristine(&conn));
        mutate_watchlist(
            &mut conn,
            &request("empty", 0, json!({"kind":"migrate","items":[]})),
        )
        .unwrap();
        assert!(restore_pristine(&conn));
        assert_eq!(
            watchlist_snapshot(&mut conn).unwrap()["migrationComplete"],
            true
        );
        drop(conn);
        assert!(restore_pristine(&open_watchlist_db_path(&file.0).unwrap()));
    }

    #[test]
    fn v2_pristine_is_consumed_by_delta_clear_legacy_or_nonempty_import() {
        for mutation in [
            json!({"kind":"delta","upserts":[],"removes":[]}),
            json!({"kind":"clear"}),
            json!({"kind":"migrate","items":[item("000001.SZ")]}),
        ] {
            let mut conn = db();
            assert!(restore_pristine(&conn));
            mutate_watchlist(&mut conn, &request("write", 0, mutation)).unwrap();
            assert!(!restore_pristine(&conn));
            mutate_watchlist(&mut conn, &request("clear", 1, json!({"kind":"clear"}))).unwrap();
            mutate_watchlist(
                &mut conn,
                &request("empty-import", 2, json!({"kind":"migrate","items":[]})),
            )
            .unwrap();
            assert!(!restore_pristine(&conn));
        }
        let mut conn = db();
        legacy_mutation(
            &mut conn,
            WatchlistMutation::Delta {
                upserts: vec![item("000001.SZ")],
                removes: vec![],
            },
        )
        .unwrap();
        assert!(!restore_pristine(&conn));
        let mut conn = db();
        legacy_mutation(&mut conn, WatchlistMutation::Clear).unwrap();
        assert!(!restore_pristine(&conn));
    }

    #[test]
    fn pristine_change_is_rolled_back_with_failed_delta() {
        let mut conn = db();
        assert!(mutate_watchlist(
            &mut conn,
            &request(
                "bad",
                0,
                json!({"kind":"delta","upserts":[item("")],"removes":[]})
            )
        )
        .is_err());
        assert!(restore_pristine(&conn));
        assert_eq!(watchlist_snapshot(&mut conn).unwrap()["revision"], 0);
    }

    #[test]
    fn existing_empty_v0_file_is_never_restore_pristine() {
        let file = TestDatabase::new();
        let conn = Connection::open(&file.0).unwrap();
        conn.execute_batch("CREATE TABLE watchlist(code TEXT PRIMARY KEY NOT NULL,name TEXT,industry TEXT,added_at TEXT NOT NULL,source TEXT,screen_criteria_summary TEXT,updated_at INTEGER NOT NULL);").unwrap();
        drop(conn);
        let mut conn = open_watchlist_db_path(&file.0).unwrap();
        assert!(!restore_pristine(&conn));
        assert_eq!(
            watchlist_snapshot(&mut conn).unwrap()["migrationComplete"],
            true
        );
        assert_eq!(schema_version(&conn).unwrap(), 2);
    }

    #[test]
    fn v1_to_v2_is_conservative_and_keeps_revision_items_and_receipts() {
        for populated in [false, true] {
            let mut conn = Connection::open_in_memory().unwrap();
            conn.execute_batch("CREATE TABLE watchlist(code TEXT PRIMARY KEY NOT NULL,name TEXT,industry TEXT,added_at TEXT NOT NULL,source TEXT,screen_criteria_summary TEXT,updated_at INTEGER NOT NULL);
                CREATE TABLE watchlist_state(singleton INTEGER PRIMARY KEY CHECK(singleton=1),revision INTEGER NOT NULL CHECK(revision>=0),migration_complete INTEGER NOT NULL CHECK(migration_complete IN (0,1)));
                CREATE TABLE watchlist_operations(operation_id TEXT PRIMARY KEY NOT NULL,payload TEXT NOT NULL,response TEXT NOT NULL);
                INSERT INTO watchlist_state VALUES(1,7,0);
                INSERT INTO watchlist_operations VALUES('kept','{}','{}'); PRAGMA user_version=1;").unwrap();
            if populated {
                conn.execute(
                    "INSERT INTO watchlist(code,added_at,updated_at) VALUES('000001.SZ','old',1)",
                    [],
                )
                .unwrap();
            }
            initialize_watchlist_db(&mut conn, false).unwrap();
            assert!(!restore_pristine(&conn));
            assert_eq!(schema_version(&conn).unwrap(), 2);
            assert_eq!(watchlist_snapshot(&mut conn).unwrap()["revision"], 7);
            assert_eq!(
                watchlist_snapshot(&mut conn).unwrap()["migrationComplete"],
                false
            );
            assert_eq!(
                watchlist_rows(&conn).unwrap().as_array().unwrap().len(),
                usize::from(populated)
            );
            assert_eq!(
                conn.query_row(
                    "SELECT payload FROM watchlist_operations WHERE operation_id='kept'",
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
                "{}"
            );
            initialize_watchlist_db(&mut conn, false).unwrap();
            assert!(!restore_pristine(&conn));
        }
    }

    #[test]
    fn v1_file_migration_preserves_the_parent_pre_migration_backup() {
        let file = TestDatabase::new();
        let mut conn = file.open();
        mutate_watchlist(&mut conn, &add("saved", 0, "000001.SZ")).unwrap();
        conn.execute_batch(
            "ALTER TABLE watchlist_state DROP COLUMN restore_pristine; PRAGMA user_version=1;",
        )
        .unwrap();
        drop(conn);
        let conn = open_watchlist_db_path(&file.0).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), 2);
        assert!(!restore_pristine(&conn));
        let prefix = format!(
            "{}.before-v2-",
            file.0.file_stem().unwrap().to_str().unwrap()
        );
        let backups: Vec<_> = fs::read_dir(file.0.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(&prefix)
            })
            .collect();
        assert_eq!(backups.len(), 1);
        let backup =
            Connection::open_with_flags(&backups[0], rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        assert_eq!(schema_version(&backup).unwrap(), 1);
        assert_eq!(watchlist_rows(&backup).unwrap()[0]["code"], "000001.SZ");
        assert_eq!(backup.query_row("SELECT count(*) FROM pragma_table_info('watchlist_state') WHERE name='restore_pristine'", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(
            backup
                .query_row(
                    "SELECT count(*) FROM watchlist_operations WHERE operation_id='saved'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        drop(backup);
        fs::remove_file(&backups[0]).unwrap();
    }
}
