use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{
    collections::{HashSet},
    fs,
    path::{PathBuf},
};
use tauri::{Manager};


pub(crate) const WATCHLIST_DB_FILE: &str = "watchlist.sqlite";

#[tauri::command]
pub(crate) async fn api_watchlist_list(app: tauri::AppHandle) -> Result<Value, String> {
    crate::runtime::run_io_bound("api_watchlist_list", move || watchlist_list_sync(&app)).await?
}

#[tauri::command]
pub(crate) async fn api_watchlist_replace(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    crate::runtime::run_io_bound("api_watchlist_replace", move || {
        let items = watchlist_items_from_payload(&payload)?;
        watchlist_replace_sync(&app, items)
    })
    .await?
}

#[tauri::command]
pub(crate) async fn api_watchlist_add(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    crate::runtime::run_io_bound("api_watchlist_add", move || {
        let item = watchlist_item_from_value(&payload)?;
        watchlist_upsert_sync(&app, item)?;
        watchlist_list_sync(&app)
    })
    .await?
}

#[tauri::command]
pub(crate) async fn api_watchlist_remove(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    crate::runtime::run_io_bound("api_watchlist_remove", move || {
        let code = payload
            .get("code")
            .and_then(Value::as_str)
            .and_then(crate::market::normalize_stock_code)
            .ok_or_else(|| "watchlist code is required".to_string())?;
        let conn = open_watchlist_db(&app)?;
        conn.execute("DELETE FROM watchlist WHERE code = ?1", params![code])
            .map_err(|error| format!("delete watchlist item failed: {error}"))?;
        watchlist_rows(&conn)
    })
    .await?
}

#[tauri::command]
pub(crate) async fn api_watchlist_clear(app: tauri::AppHandle) -> Result<Value, String> {
    crate::runtime::run_io_bound("api_watchlist_clear", move || {
        let conn = open_watchlist_db(&app)?;
        conn.execute("DELETE FROM watchlist", [])
            .map_err(|error| format!("clear watchlist failed: {error}"))?;
        Ok(json!([]))
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
    let path = watchlist_db_path(app)?;
    let conn = Connection::open(&path)
        .map_err(|error| format!("open watchlist sqlite failed: {}: {error}", path.display()))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         CREATE TABLE IF NOT EXISTS watchlist (
             code TEXT PRIMARY KEY NOT NULL,
             name TEXT,
             industry TEXT,
             added_at TEXT NOT NULL,
             source TEXT,
             screen_criteria_summary TEXT,
             updated_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_watchlist_added_at ON watchlist(added_at DESC);",
    )
    .map_err(|error| format!("initialize watchlist sqlite failed: {error}"))?;
    Ok(conn)
}

pub(crate) fn watchlist_list_sync(app: &tauri::AppHandle) -> Result<Value, String> {
    let conn = open_watchlist_db(app)?;
    watchlist_rows(&conn)
}

pub(crate) fn watchlist_replace_sync(
    app: &tauri::AppHandle,
    items: Vec<WatchlistRecord>,
) -> Result<Value, String> {
    let mut conn = open_watchlist_db(app)?;
    let tx = conn
        .transaction()
        .map_err(|error| format!("begin watchlist transaction failed: {error}"))?;
    tx.execute("DELETE FROM watchlist", [])
        .map_err(|error| format!("clear watchlist before replace failed: {error}"))?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO watchlist (code, name, industry, added_at, source, screen_criteria_summary, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .map_err(|error| format!("prepare watchlist replace failed: {error}"))?;
        for item in normalize_watchlist_records(items) {
            stmt.execute(params![
                item.code,
                item.name,
                item.industry,
                item.added_at,
                item.source,
                item.screen_criteria_summary,
                crate::market::epoch_millis() as i64,
            ])
            .map_err(|error| format!("insert watchlist item failed: {error}"))?;
        }
    }
    tx.commit()
        .map_err(|error| format!("commit watchlist replace failed: {error}"))?;
    watchlist_rows(&conn)
}

pub(crate) fn watchlist_upsert_sync(app: &tauri::AppHandle, item: WatchlistRecord) -> Result<(), String> {
    let conn = open_watchlist_db(app)?;
    let item = normalize_watchlist_records(vec![item])
        .into_iter()
        .next()
        .ok_or_else(|| "watchlist code is required".to_string())?;
    conn.execute(
        "INSERT INTO watchlist (code, name, industry, added_at, source, screen_criteria_summary, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(code) DO UPDATE SET
           name = excluded.name,
           industry = excluded.industry,
           source = excluded.source,
           screen_criteria_summary = excluded.screen_criteria_summary,
           updated_at = excluded.updated_at",
        params![
            item.code,
            item.name,
            item.industry,
            item.added_at,
            item.source,
            item.screen_criteria_summary,
            crate::market::epoch_millis() as i64,
        ],
    )
    .map_err(|error| format!("upsert watchlist item failed: {error}"))?;
    Ok(())
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

pub(crate) fn watchlist_items_from_payload(payload: &Value) -> Result<Vec<WatchlistRecord>, String> {
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
        screen_criteria_summary: crate::market::optional_trimmed_string(value.get("screenCriteriaSummary"))
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
