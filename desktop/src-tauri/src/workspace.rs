//! L3 client workspace. Register api_workspace_load/api_workspace_commit in the parent lib.rs.
//! Only small non-secret UI state belongs here; ledgers/results/credentials have other owners.
use rusqlite::{params, Connection, OpenFlags, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{fs, path::Path, sync::Mutex};
use tauri::Manager;

const SCHEMA: i64 = 1;
const MAX_VALUE: usize = 256 * 1024;
const MAX_TOTAL: i64 = 2 * 1024 * 1024;
const MAX_KEYS: i64 = 128;
const MAX_REVISION: i64 = 9_007_199_254_740_991;
static WORKSPACE_OPERATION: Mutex<()> = Mutex::new(());

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    schema_version: i64,
    revision: i64,
    values: Map<String, Value>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceCommit {
    expected_revision: i64,
    changes: Map<String, Value>,
}
fn err(error: impl std::fmt::Display) -> String {
    format!("workspace storage: {error}")
}
fn bounded_string(value: &Value, max: usize) -> bool {
    value.as_str().is_some_and(|text| text.len() <= max)
}
fn only_fields(value: &Map<String, Value>, fields: &[&str]) -> bool {
    value.keys().all(|key| fields.contains(&key.as_str()))
}
fn value_limit(key: &str) -> usize {
    if key == "agent.conversations" {
        MAX_TOTAL as usize
    } else {
        MAX_VALUE
    }
}
fn valid_core_criteria(value: &Value) -> bool {
    value.as_object().is_some_and(|fields| {
        only_fields(
            fields,
            &[
                "min_roe",
                "max_pe",
                "max_pb",
                "min_market_cap_billion",
                "min_deducted_net_profit_billion",
                "min_deducted_net_profit_margin",
                "min_deducted_net_profit_growth_rate",
                "industry",
                "market_scope",
                "require_institution_buy_ratio_gt_sell_ratio",
                "include_st",
                "limit",
                "sort_by",
                "sort_dir",
                "score_profile",
            ],
        ) && fields
            .values()
            .all(|v| v.is_null() || v.is_boolean() || v.is_number() || bounded_string(v, 256))
    })
}
fn validate_value(key: &str, value: &Value) -> Result<String, String> {
    if key.len() > 280 || key.chars().any(char::is_control) {
        return Err(err("invalid key"));
    }
    let valid = match key {
        "app.view" => value
            .as_str()
            .is_some_and(|v| ["screen", "observe", "backtest", "news", "agent"].contains(&v)),
        "app.stock" | "news.stock" => bounded_string(value, 32),
        "app.backtest.source" => value == "criteria" || value == "watchlist",
        "news.thread" | "agent.active" => bounded_string(value, 256),
        "news.question" => bounded_string(value, 32000),
        "migration.localStorage.v1" => value == true,
        "backtest.start"
        | "backtest.end"
        | "backtest.rebalance"
        | "backtest.benchmark"
        | "backtest.strategyMode" => bounded_string(value, 64),
        "backtest.topN" | "backtest.costBps" => value.is_number(),
        "backtest.adaptiveSpec" => {
            value.is_null()
                || (value.as_object().is_some_and(|fields| {
                    only_fields(
                        fields,
                        &[
                            "criteria",
                            "mode",
                            "horizon",
                            "primary_limit",
                            "exploration_limit",
                            "run_id",
                        ],
                    )
                }) && valid_core_criteria(&value["criteria"])
                    && value["mode"].as_str().is_some_and(|mode| {
                        ["auto", "range", "trend", "defensive"].contains(&mode)
                    })
                    && value["horizon"] == "swing_10_30d"
                    && value["primary_limit"].is_number()
                    && value["exploration_limit"].is_number()
                    && bounded_string(&value["run_id"], 256))
        }
        "backtest.criteria" | "app.criteria" => value.as_object().is_some_and(|fields| {
            only_fields(
                fields,
                &[
                    "includeSt",
                    "requireInstitutionBuyRatio",
                    "minRoe",
                    "maxPe",
                    "maxPb",
                    "minMcap",
                    "industry",
                    "marketScope",
                    "resultLimit",
                    "sortBy",
                    "sortDir",
                    "scoreProfile",
                ],
            ) && fields.iter().all(|(key, value)| match key.as_str() {
                "includeSt" | "requireInstitutionBuyRatio" => value.is_boolean(),
                "resultLimit" => value.is_number(),
                _ => bounded_string(value, 256),
            })
        }),
        "agent.conversations" => value.as_array().is_some_and(|items| {
            items.len() <= 40
                && items.iter().all(|item| {
                    item.as_object().is_some_and(|fields| {
                        only_fields(
                            fields,
                            &["id", "title", "mode", "messages", "createdAt", "updatedAt"],
                        )
                    }) && bounded_string(&item["id"], 256)
                        && item["id"] != ""
                        && bounded_string(&item["title"], 180)
                        && item["mode"]
                            .as_str()
                            .is_some_and(|v| ["quick", "expert", "research"].contains(&v))
                        && item["createdAt"].is_number()
                        && item["updatedAt"].is_number()
                        && item["messages"].as_array().is_some_and(|messages| {
                            messages.len() <= 200
                                && messages.iter().all(|message| {
                                    message.as_object().is_some_and(|fields| {
                                        only_fields(
                                            fields,
                                            &["role", "content", "timestamp", "runId", "error"],
                                        )
                                    }) && (message["role"] == "user"
                                        || message["role"] == "assistant")
                                        && bounded_string(&message["content"], 128 * 1024)
                                        && message["timestamp"].is_number()
                                        && message
                                            .get("runId")
                                            .is_none_or(|id| bounded_string(id, 256))
                                        && message.get("error").is_none_or(Value::is_boolean)
                                })
                        })
                })
        }),
        _ if key.starts_with("agent.draft:") && key.len() > 12 => {
            value.is_null() || bounded_string(value, 32000)
        }
        _ => false,
    };
    if !valid {
        return Err(err("unknown key or invalid value; original retained"));
    }
    let json = serde_json::to_string(value).map_err(err)?;
    if json.len() > value_limit(key) {
        return Err(err(
            "JSON value exceeds storage limit; content was not truncated",
        ));
    }
    Ok(json)
}
fn open_workspace(root: &Path) -> Result<Connection, String> {
    let path = root.join("client-state.sqlite");
    let existing = path.exists();
    if existing {
        crate::durability::validate_sqlite_header(&path)?;
    } else {
        fs::create_dir_all(root).map_err(err)?;
    }
    let mut connection = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | if existing {
                OpenFlags::empty()
            } else {
                OpenFlags::SQLITE_OPEN_CREATE
            },
    )
    .map_err(err)?;
    // Fail closed BEFORE WAL setup or schema writes, especially after restoring a future backup.
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(err)?;
    if version != 0 && version != SCHEMA {
        return Err(err("unsupported schema version; original preserved"));
    }
    let tables: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get(0),
        )
        .map_err(err)?;
    if version == 0 && tables != 0 {
        return Err(err("unversioned nonempty database; original preserved"));
    }
    crate::durability::configure_user_connection(&connection)?;
    let journal: String = connection
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .map_err(err)?;
    let synchronous: i64 = connection
        .pragma_query_value(None, "synchronous", |row| row.get(0))
        .map_err(err)?;
    if journal.to_lowercase() != "wal" || synchronous != 2 {
        return Err(err("FULL/WAL unavailable"));
    }
    if version == 0 {
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(err)?;
        tx.execute_batch("CREATE TABLE workspace_meta (id INTEGER PRIMARY KEY CHECK(id=1), revision INTEGER NOT NULL CHECK(revision>=0));
            INSERT INTO workspace_meta VALUES(1,0);
            CREATE TABLE workspace_values (key TEXT PRIMARY KEY, json TEXT NOT NULL CHECK(json_valid(json)), revision INTEGER NOT NULL CHECK(revision>0));
            PRAGMA user_version=1;").map_err(err)?;
        tx.commit().map_err(err)?;
    }
    Ok(connection)
}
fn read_snapshot(connection: &mut Connection) -> Result<WorkspaceSnapshot, String> {
    let tx = connection.transaction().map_err(err)?;
    let revision: i64 = tx
        .query_row(
            "SELECT revision FROM workspace_meta WHERE id=1",
            [],
            |row| row.get(0),
        )
        .map_err(err)?;
    if !(0..=MAX_REVISION).contains(&revision) {
        return Err(err("invalid revision"));
    }
    let mut values = Map::new();
    let mut total = 0;
    {
        let mut statement = tx
            .prepare("SELECT key, json, revision FROM workspace_values LIMIT 129")
            .map_err(err)?;
        let mut rows = statement.query([]).map_err(err)?;
        while let Some(row) = rows.next().map_err(err)? {
            let key: String = row.get(0).map_err(err)?;
            let json: String = row.get(1).map_err(err)?;
            let row_revision: i64 = row.get(2).map_err(err)?;
            total += json.len() as i64 + key.len() as i64;
            if json.len() > value_limit(&key)
                || total > MAX_TOTAL
                || values.len() >= MAX_KEYS as usize
                || !(1..=revision).contains(&row_revision)
            {
                return Err(err("invalid workspace bounds/revision"));
            }
            let value: Value = serde_json::from_str(&json)
                .map_err(|_| err("invalid stored JSON; original preserved"))?;
            validate_value(&key, &value)?;
            values.insert(key, value);
        }
    }
    tx.commit().map_err(err)?;
    Ok(WorkspaceSnapshot {
        schema_version: SCHEMA,
        revision,
        values,
    })
}
fn commit_changes(connection: &mut Connection, payload: WorkspaceCommit) -> Result<i64, String> {
    if payload.changes.is_empty()
        || payload.changes.len() > MAX_KEYS as usize
        || !(0..MAX_REVISION).contains(&payload.expected_revision)
    {
        return Err(err("invalid transaction bounds"));
    }
    let changes = payload
        .changes
        .iter()
        .map(|(key, value)| Ok((key, value, validate_value(key, value)?)))
        .collect::<Result<Vec<_>, String>>()?;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(err)?;
    let revision: i64 = tx
        .query_row(
            "SELECT revision FROM workspace_meta WHERE id=1",
            [],
            |row| row.get(0),
        )
        .map_err(err)?;
    if revision != payload.expected_revision {
        return Err(err("revision conflict; reload and explicitly retry"));
    }
    let next = revision + 1;
    for (key, value, json) in changes {
        if value.is_null() {
            tx.execute("DELETE FROM workspace_values WHERE key=?1", [key])
                .map_err(err)?;
        } else {
            tx.execute("INSERT INTO workspace_values(key,json,revision) VALUES(?1,?2,?3) ON CONFLICT(key) DO UPDATE SET json=excluded.json,revision=excluded.revision", params![key,json,next]).map_err(err)?;
        }
    }
    let (count, total): (i64, i64) = tx.query_row("SELECT count(*), COALESCE(sum(length(CAST(key AS BLOB))+length(CAST(json AS BLOB))),0) FROM workspace_values", [], |row| Ok((row.get(0)?,row.get(1)?))).map_err(err)?;
    if count > MAX_KEYS || total > MAX_TOTAL {
        return Err(err("workspace quota exceeded; transaction rolled back"));
    }
    tx.execute("UPDATE workspace_meta SET revision=?1 WHERE id=1", [next])
        .map_err(err)?;
    tx.commit().map_err(err)?;
    Ok(next)
}
#[tauri::command]
pub async fn api_workspace_load(app: tauri::AppHandle) -> Result<WorkspaceSnapshot, String> {
    let root = app.path().app_data_dir().map_err(err)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = WORKSPACE_OPERATION
            .lock()
            .map_err(|_| err("workspace lock poisoned"))?;
        read_snapshot(&mut open_workspace(&root)?)
    })
    .await
    .map_err(err)?
}
#[tauri::command]
pub async fn api_workspace_commit(
    app: tauri::AppHandle,
    payload: WorkspaceCommit,
) -> Result<i64, String> {
    let root = app.path().app_data_dir().map_err(err)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = WORKSPACE_OPERATION
            .lock()
            .map_err(|_| err("workspace lock poisoned"))?;
        let mut connection = open_workspace(&root)?;
        read_snapshot(&mut connection)?; // Never overwrite malformed existing state.
        commit_changes(&mut connection, payload)
    })
    .await
    .map_err(err)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Scratch(std::path::PathBuf);
    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "gp-workspace-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn patch(revision: i64, values: Value) -> WorkspaceCommit {
        WorkspaceCommit {
            expected_revision: revision,
            changes: values.as_object().unwrap().clone(),
        }
    }
    #[test]
    fn workspace_roundtrip_uses_full_wal_and_revision_compare_and_swap() {
        let root = Scratch::new();
        let mut db = open_workspace(&root.0).unwrap();
        assert_eq!(
            db.pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))
                .unwrap(),
            "wal"
        );
        assert_eq!(
            db.pragma_query_value(None, "synchronous", |row| row.get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            commit_changes(
                &mut db,
                patch(0, json!({"news.question":"unsent","news.thread":"t1"}))
            )
            .unwrap(),
            1
        );
        assert!(commit_changes(&mut db, patch(0, json!({"news.question":"stale"}))).is_err());
        drop(db);
        let saved = read_snapshot(&mut open_workspace(&root.0).unwrap()).unwrap();
        assert_eq!(saved.revision, 1);
        assert_eq!(saved.values["news.question"], "unsent");
    }
    #[test]
    fn workspace_rejects_future_unknown_corrupt_and_oversized_without_replacement() {
        let root = Scratch::new();
        let path = root.0.join("client-state.sqlite");
        let db = Connection::open(&path).unwrap();
        db.pragma_update(None, "user_version", 99).unwrap();
        drop(db);
        let before = fs::read(&path).unwrap();
        assert!(open_workspace(&root.0).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        let corrupt = Scratch::new();
        fs::write(corrupt.0.join("client-state.sqlite"), b"not sqlite").unwrap();
        assert!(open_workspace(&corrupt.0).is_err());
        let clean = Scratch::new();
        let mut db = open_workspace(&clean.0).unwrap();
        for values in [
            json!({"llm.api_key":"secret"}),
            json!({"news.question":7}),
            json!({"news.question":"x".repeat(32001)}),
            json!({"app.criteria":{"api_key":"secret"}}),
        ] {
            assert!(commit_changes(&mut db, patch(0, values)).is_err());
            assert_eq!(read_snapshot(&mut db).unwrap().revision, 0);
        }
    }
    #[test]
    fn workspace_quota_failure_rolls_back_all_keys_and_revision() {
        let root = Scratch::new();
        let mut db = open_workspace(&root.0).unwrap();
        let values: Map<String, Value> = (0..70)
            .map(|n| (format!("agent.draft:{n:04}"), json!("x".repeat(32000))))
            .collect();
        assert!(commit_changes(
            &mut db,
            WorkspaceCommit {
                expected_revision: 0,
                changes: values
            }
        )
        .is_err());
        let saved = read_snapshot(&mut db).unwrap();
        assert_eq!(saved.revision, 0);
        assert!(saved.values.is_empty());
    }
    #[test]
    fn workspace_invalid_stored_json_is_not_reset() {
        let root = Scratch::new();
        let mut db = open_workspace(&root.0).unwrap();
        commit_changes(&mut db, patch(0, json!({"news.question":"safe"}))).unwrap();
        db.execute_batch(
            "PRAGMA ignore_check_constraints=ON; UPDATE workspace_values SET json='broken';",
        )
        .unwrap();
        assert!(read_snapshot(&mut db).is_err());
        assert_eq!(
            db.query_row("SELECT json FROM workspace_values", [], |row| row
                .get::<_, String>(0))
                .unwrap(),
            "broken"
        );
    }
    #[test]
    fn preserves_full_transcript_and_backtest_inputs() {
        let root = Scratch::new();
        let mut db = open_workspace(&root.0).unwrap();
        let messages:Vec<Value>=(0..25).map(|i|json!({"role":"assistant","content":"完整回答".repeat(600),"timestamp":i,"error":false})).collect();
        let conversations = json!([{"id":"history","title":"history","mode":"quick","messages":messages,"createdAt":1,"updatedAt":1}]);
        let spec = json!({"criteria":{"limit":10},"mode":"range","horizon":"swing_10_30d","primary_limit":10,"exploration_limit":10,"run_id":"saved"});
        commit_changes(&mut db,patch(0,json!({"agent.conversations":conversations,"backtest.start":"2021-02-03","backtest.costBps":25,"backtest.adaptiveSpec":spec}))).unwrap();
        let restored = read_snapshot(&mut db).unwrap();
        assert_eq!(restored.values["agent.conversations"], conversations);
        assert_eq!(restored.values["backtest.adaptiveSpec"], spec);
        assert!(commit_changes(&mut db,patch(1,json!({"backtest.adaptiveSpec":{"criteria":{"api_key":"secret"},"mode":"auto","horizon":"swing_10_30d","primary_limit":10,"exploration_limit":10,"run_id":"bad"}}))).is_err());
    }
}
