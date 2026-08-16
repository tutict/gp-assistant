use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering as AtomicOrdering},
        Once,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::Manager;

const AGENT_LEDGER_SCHEMA_VERSION: i64 = 2;
pub(crate) const MAX_AGENT_LEDGER_ID_BYTES: usize = 256;
const AGENT_CONVERSATION_DELETED_ERROR: &str = "agent conversation was deleted";

pub(crate) struct AgentRunStore {
    connection: Connection,
}

pub(crate) fn with_app_store<T, F>(app: &tauri::AppHandle, operation: F) -> Result<T, String>
where
    F: FnOnce(&AgentRunStore) -> Result<T, String>,
{
    let mut root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve agent ledger directory: {error}"))?;
    root.push("agent");
    root.push("agent-runs.sqlite");
    let store = AgentRunStore::open(root)?;
    operation(&store)
}

pub(crate) fn current_epoch_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

static RUN_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// `run_id` is the primary key, so a millisecond timestamp alone collides whenever two
/// runs start in the same millisecond. The counter makes it unique within the process
/// and the pid keeps two app instances sharing the ledger file apart.
pub(crate) fn next_run_id() -> String {
    format!(
        "gp-agent-run-{}-{}-{}",
        current_epoch_millis(),
        std::process::id(),
        RUN_ID_COUNTER.fetch_add(1, AtomicOrdering::Relaxed)
    )
}

impl AgentRunStore {
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create agent ledger directory: {error}"))?;
        }
        let mut connection = Connection::open(path)
            .map_err(|error| format!("failed to open agent ledger: {error}"))?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|error| format!("failed to configure agent ledger: {error}"))?;
        ensure_supported_agent_ledger_schema(agent_ledger_schema_version(&connection)?)?;
        initialize_schema(&connection)?;
        migrate_agent_ledger(&mut connection)?;
        reconcile_interrupted_runs_once(&connection);
        Ok(Self { connection })
    }

    pub(crate) fn start_run(
        &self,
        payload: &Value,
        started_at_epoch_ms: i64,
    ) -> Result<(), String> {
        let run_id = required_string(payload, "run_id")?;
        let question = required_string(payload, "message")?;
        let conversation_id = required_string(payload, "conversation_id")?;
        if run_id.len() > MAX_AGENT_LEDGER_ID_BYTES {
            return Err(format!("run_id exceeds {MAX_AGENT_LEDGER_ID_BYTES} bytes"));
        }
        if conversation_id.len() > MAX_AGENT_LEDGER_ID_BYTES {
            return Err(format!(
                "conversation_id exceeds {MAX_AGENT_LEDGER_ID_BYTES} bytes"
            ));
        }
        let mode = optional_string(payload, "mode").unwrap_or_else(|| "quick".to_string());
        let request_json = serde_json::to_string(&sanitized_request(payload))
            .map_err(|error| format!("failed to encode agent request: {error}"))?;
        let changed = self
            .connection
            .execute(
                "INSERT INTO agent_runs (
                    run_id, conversation_id, question, mode, status, started_at_epoch_ms,
                    request_json, events_json
                 )
                 SELECT ?1, ?2, ?3, ?4, 'running', ?5, ?6, '[]'
                 WHERE NOT EXISTS (
                     SELECT 1 FROM agent_deleted_conversations WHERE conversation_id = ?2
                 )",
                params![
                    run_id,
                    conversation_id,
                    question,
                    mode,
                    started_at_epoch_ms,
                    request_json
                ],
            )
            .map_err(|error| format!("failed to start agent run: {error}"))?;
        if changed == 0 {
            return Err(format!(
                "{AGENT_CONVERSATION_DELETED_ERROR}: {conversation_id}"
            ));
        }
        Ok(())
    }

    pub(crate) fn complete_run(
        &self,
        run_id: &str,
        events: &[Value],
        result: &Value,
        completed_at_epoch_ms: i64,
    ) -> Result<(), String> {
        let events_json = serde_json::to_string(events)
            .map_err(|error| format!("failed to encode agent events: {error}"))?;
        let result_json = serde_json::to_string(result)
            .map_err(|error| format!("failed to encode agent result: {error}"))?;
        let changed = self
            .connection
            .execute(
                "UPDATE agent_runs
                 SET status = 'completed', completed_at_epoch_ms = ?2,
                     duration_ms = MAX(0, ?2 - started_at_epoch_ms),
                     events_json = ?3, result_json = ?4, error = NULL
                 WHERE run_id = ?1",
                params![run_id, completed_at_epoch_ms, events_json, result_json],
            )
            .map_err(|error| format!("failed to complete agent run: {error}"))?;
        ensure_run_updated(changed, run_id)
    }

    pub(crate) fn fail_run(
        &self,
        run_id: &str,
        events: &[Value],
        error: &str,
        completed_at_epoch_ms: i64,
    ) -> Result<(), String> {
        let events_json = serde_json::to_string(events)
            .map_err(|error| format!("failed to encode agent events: {error}"))?;
        let changed = self
            .connection
            .execute(
                "UPDATE agent_runs
                 SET status = 'failed', completed_at_epoch_ms = ?2,
                     duration_ms = MAX(0, ?2 - started_at_epoch_ms),
                     events_json = ?3, result_json = NULL, error = ?4
                 WHERE run_id = ?1",
                params![run_id, completed_at_epoch_ms, events_json, error],
            )
            .map_err(|error| format!("failed to record agent failure: {error}"))?;
        ensure_run_updated(changed, run_id)
    }

    pub(crate) fn list_runs(
        &self,
        limit: usize,
        conversation_id: Option<&str>,
    ) -> Result<Value, String> {
        let limit = limit.clamp(1, 200) as i64;
        let mut statement = self
            .connection
            .prepare(
                "SELECT run_id, conversation_id, question, mode, status,
                        started_at_epoch_ms, completed_at_epoch_ms, duration_ms, error
                 FROM agent_runs
                 WHERE (?1 IS NULL OR conversation_id = ?1)
                 ORDER BY started_at_epoch_ms DESC, run_id DESC
                 LIMIT ?2",
            )
            .map_err(|error| format!("failed to prepare agent run list: {error}"))?;
        let rows = statement
            .query_map(params![conversation_id, limit], |row| {
                Ok(json!({
                    "run_id": row.get::<_, String>(0)?,
                    "conversation_id": row.get::<_, Option<String>>(1)?,
                    "question": row.get::<_, String>(2)?,
                    "mode": row.get::<_, String>(3)?,
                    "status": row.get::<_, String>(4)?,
                    "started_at_epoch_ms": row.get::<_, i64>(5)?,
                    "completed_at_epoch_ms": row.get::<_, Option<i64>>(6)?,
                    "duration_ms": row.get::<_, Option<i64>>(7)?,
                    "error": row.get::<_, Option<String>>(8)?,
                }))
            })
            .map_err(|error| format!("failed to query agent run list: {error}"))?;
        rows.map(|row| row.map_err(|error| format!("failed to decode agent run summary: {error}")))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array)
    }

    pub(crate) fn get_run(&self, run_id: &str) -> Result<Option<Value>, String> {
        // `request_json` is deliberately not selected: the replay detail contract has no request
        // field, and schema v2 persists only a schema marker there.
        let stored = self
            .connection
            .query_row(
                "SELECT run_id, conversation_id, question, mode, status,
                        started_at_epoch_ms, completed_at_epoch_ms, duration_ms,
                        events_json, result_json, error
                 FROM agent_runs WHERE run_id = ?1",
                params![run_id],
                |row| {
                    Ok(StoredAgentRun {
                        run_id: row.get(0)?,
                        conversation_id: row.get(1)?,
                        question: row.get(2)?,
                        mode: row.get(3)?,
                        status: row.get(4)?,
                        started_at_epoch_ms: row.get(5)?,
                        completed_at_epoch_ms: row.get(6)?,
                        duration_ms: row.get(7)?,
                        events_json: row.get(8)?,
                        result_json: row.get(9)?,
                        error: row.get(10)?,
                    })
                },
            )
            .optional()
            .map_err(|error| format!("failed to read agent run: {error}"))?;
        stored.map(decode_stored_run).transpose()
    }

    pub(crate) fn delete_conversation_runs(&self, conversation_id: &str) -> Result<usize, String> {
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(|error| format!("failed to start agent conversation deletion: {error}"))?;
        transaction
            .execute(
                "INSERT INTO agent_deleted_conversations (conversation_id, deleted_at_epoch_ms)
                 VALUES (?1, ?2)
                 ON CONFLICT(conversation_id) DO UPDATE
                 SET deleted_at_epoch_ms = excluded.deleted_at_epoch_ms",
                params![conversation_id, current_epoch_millis()],
            )
            .map_err(|error| format!("failed to tombstone agent conversation: {error}"))?;
        let deleted = transaction
            .execute(
                "DELETE FROM agent_runs WHERE conversation_id = ?1",
                params![conversation_id],
            )
            .map_err(|error| format!("failed to delete agent conversation runs: {error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit agent conversation deletion: {error}"))?;
        Ok(deleted)
    }
}

pub(crate) fn is_conversation_deleted_error(error: &str) -> bool {
    error.starts_with(AGENT_CONVERSATION_DELETED_ERROR)
}

struct StoredAgentRun {
    run_id: String,
    conversation_id: Option<String>,
    question: String,
    mode: String,
    status: String,
    started_at_epoch_ms: i64,
    completed_at_epoch_ms: Option<i64>,
    duration_ms: Option<i64>,
    events_json: String,
    result_json: Option<String>,
    error: Option<String>,
}

fn initialize_schema(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS agent_runs (
                 run_id TEXT PRIMARY KEY,
                 conversation_id TEXT,
                 question TEXT NOT NULL,
                 mode TEXT NOT NULL,
                 status TEXT NOT NULL,
                 started_at_epoch_ms INTEGER NOT NULL,
                 completed_at_epoch_ms INTEGER,
                 duration_ms INTEGER,
                 request_json TEXT NOT NULL,
                 events_json TEXT NOT NULL DEFAULT '[]',
                 result_json TEXT,
                 error TEXT
             );
             CREATE TABLE IF NOT EXISTS agent_deleted_conversations (
                 conversation_id TEXT PRIMARY KEY,
                 deleted_at_epoch_ms INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_agent_runs_started
               ON agent_runs(started_at_epoch_ms DESC, run_id DESC);
             CREATE INDEX IF NOT EXISTS idx_agent_runs_conversation
               ON agent_runs(conversation_id, started_at_epoch_ms DESC);",
        )
        .map_err(|error| format!("failed to initialize agent ledger: {error}"))
}

static RECONCILE_ONCE: Once = Once::new();

/// A run killed with the app (force quit, crash, webview death) never reaches
/// `complete_run`/`fail_run`, so its row stays at `running` forever and the replay drawer
/// keeps waiting on it. Settle those leftovers to the design's `unknown` status.
///
/// Only the first call in the process does the work: `open()` runs per command, and a
/// later sweep would clobber the run that is in flight right now. The first `open()`
/// necessarily precedes any `start_run` this process performs, so live runs are safe.
fn reconcile_interrupted_runs_once(connection: &Connection) {
    RECONCILE_ONCE.call_once(|| {
        if let Err(error) = reconcile_interrupted_runs(connection) {
            eprintln!("agent run ledger reconciliation failed: {error}");
        }
    });
}

fn reconcile_interrupted_runs(connection: &Connection) -> Result<usize, String> {
    connection
        .execute(
            "UPDATE agent_runs SET status = 'unknown' WHERE status = 'running'",
            [],
        )
        .map_err(|error| format!("failed to reconcile interrupted agent runs: {error}"))
}

fn migrate_agent_ledger(connection: &mut Connection) -> Result<usize, String> {
    let version = agent_ledger_schema_version(connection)?;
    ensure_supported_agent_ledger_schema(version)?;
    if version == AGENT_LEDGER_SCHEMA_VERSION {
        return Ok(0);
    }

    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| format!("failed to start agent ledger migration: {error}"))?;
    let version = agent_ledger_schema_version(&transaction)?;
    ensure_supported_agent_ledger_schema(version)?;
    if version == AGENT_LEDGER_SCHEMA_VERSION {
        transaction
            .commit()
            .map_err(|error| format!("failed to finish agent ledger migration check: {error}"))?;
        return Ok(0);
    }

    let request_snapshot = sanitized_request(&Value::Null).to_string();
    let changed = transaction
        .execute(
            "UPDATE agent_runs
             SET status = 'unknown', request_json = ?1, events_json = '[]',
                 result_json = NULL, error = NULL",
            params![request_snapshot],
        )
        .map_err(|error| format!("failed to scrub legacy agent replay payloads: {error}"))?;
    let removed = transaction
        .execute(
            "DELETE FROM agent_runs
             WHERE trim(run_id) = '' OR trim(run_id) IN ('.', '..')
                OR length(CAST(run_id AS BLOB)) > ?1
                OR conversation_id IS NULL OR trim(conversation_id) = ''
                OR length(CAST(conversation_id AS BLOB)) > ?1",
            params![MAX_AGENT_LEDGER_ID_BYTES as i64],
        )
        .map_err(|error| format!("failed to remove orphaned legacy agent runs: {error}"))?;
    transaction
        .pragma_update(None, "user_version", AGENT_LEDGER_SCHEMA_VERSION)
        .map_err(|error| format!("failed to update agent ledger schema version: {error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("failed to commit agent ledger migration: {error}"))?;
    Ok(changed + removed)
}

fn agent_ledger_schema_version(connection: &Connection) -> Result<i64, String> {
    connection
        .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
        .map_err(|error| format!("failed to read agent ledger schema version: {error}"))
}

fn ensure_supported_agent_ledger_schema(version: i64) -> Result<(), String> {
    if version > AGENT_LEDGER_SCHEMA_VERSION {
        return Err(format!(
            "agent ledger schema version {version} is newer than supported version {AGENT_LEDGER_SCHEMA_VERSION}"
        ));
    }
    Ok(())
}

fn sanitized_request(_: &Value) -> Value {
    json!({"schema_version": AGENT_LEDGER_SCHEMA_VERSION})
}

fn required_string(value: &Value, key: &str) -> Result<String, String> {
    optional_string(value, key).ok_or_else(|| format!("{key} is required"))
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
}

fn ensure_run_updated(changed: usize, run_id: &str) -> Result<(), String> {
    if changed == 0 {
        Err(format!("agent run not found: {run_id}"))
    } else {
        Ok(())
    }
}

fn decode_json(text: &str, field: &str) -> Result<Value, String> {
    serde_json::from_str(text).map_err(|error| format!("failed to decode agent {field}: {error}"))
}

fn decode_stored_run(stored: StoredAgentRun) -> Result<Value, String> {
    let events = decode_json(&stored.events_json, "events")?;
    let result = stored
        .result_json
        .as_deref()
        .map(|value| decode_json(value, "result"))
        .transpose()?
        .unwrap_or(Value::Null);
    Ok(json!({
        "run_id": stored.run_id,
        "conversation_id": stored.conversation_id,
        "question": stored.question,
        "mode": stored.mode,
        "status": stored.status,
        "started_at_epoch_ms": stored.started_at_epoch_ms,
        "completed_at_epoch_ms": stored.completed_at_epoch_ms,
        "duration_ms": stored.duration_ms,
        "events": events,
        "result": result,
        "error": stored.error,
    }))
}

#[cfg(test)]
mod tests {
    use super::{
        is_conversation_deleted_error, next_run_id, reconcile_interrupted_runs, sanitized_request,
        AgentRunStore, AGENT_LEDGER_SCHEMA_VERSION,
    };
    use serde_json::{json, Value};
    use std::{
        collections::HashSet,
        path::PathBuf,
        sync::{Arc, Barrier},
        thread,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temporary_database_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be valid")
            .as_nanos();
        std::env::temp_dir().join(format!("gp-assistant-agent-ledger-{name}-{unique}.sqlite"))
    }

    /// `get_run` omits request snapshots, so storage assertions read the column directly.
    fn stored_request_json(store: &AgentRunStore, run_id: &str) -> Value {
        let raw: String = store
            .connection
            .query_row(
                "SELECT request_json FROM agent_runs WHERE run_id = ?1",
                [run_id],
                |row| row.get(0),
            )
            .expect("stored request should be readable");
        serde_json::from_str(&raw).expect("stored request should be valid JSON")
    }

    #[test]
    fn records_completed_run_without_persisting_llm_secrets() {
        let path = temporary_database_path("completed");
        let store = AgentRunStore::open(&path).expect("agent run store should open");
        let payload = json!({
            "run_id": "run-completed",
            "conversation_id": "conversation-1",
            "message": "Compare the candidates",
            "mode": "research",
            "context": {"watchlist": [{"code": "000001.SZ"}]},
            "history": [{"role": "user", "content": "Start with fundamentals"}],
            "network": {
                "proxy_mode": "custom",
                "proxy_url": "http://user:password@proxy.example.test:8080"
            },
            "llm": {
                "base_url": "https://ledger-user:ledger-pass@llm.example.test:8443/v1/chat?api_key=query-secret#fragment-secret",
                "model": "research-model",
                "api_key": "must-never-be-persisted",
                "organization": "sensitive-organization",
                "project": "sensitive-project"
            }
        });

        store
            .start_run(&payload, 1_000)
            .expect("run should be started");
        store
            .complete_run(
                "run-completed",
                &[json!({"type": "tool_result", "payload": {"tool": "screen"}})],
                &json!({"reply": "Candidate A has stronger evidence"}),
                1_275,
            )
            .expect("run should be completed");

        let record = store
            .get_run("run-completed")
            .expect("run lookup should succeed")
            .expect("run should exist");
        assert_eq!(record["run_id"], "run-completed");
        assert_eq!(record["conversation_id"], "conversation-1");
        assert_eq!(record["question"], "Compare the candidates");
        assert_eq!(record["mode"], "research");
        assert_eq!(record["status"], "completed");
        assert_eq!(record["started_at_epoch_ms"], 1_000);
        assert_eq!(record["completed_at_epoch_ms"], 1_275);
        assert_eq!(record["duration_ms"], 275);
        assert_eq!(record["events"][0]["type"], "tool_result");
        assert_eq!(
            record["result"]["reply"],
            "Candidate A has stronger evidence"
        );
        // The detail contract has no `request` field, so the stored request must not ride along.
        assert!(record.get("request").is_none());

        // The row itself retains only the non-sensitive schema marker.
        let stored_request = stored_request_json(&store, "run-completed");
        assert_eq!(stored_request, json!({"schema_version": 2}));

        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn stores_only_the_request_schema_marker() {
        let request = sanitized_request(&json!({
            "message": "Inspect the request",
            "history": [{"role": "user", "content": "private history"}],
            "context": {"watchlist": [{"code": "000001.SZ"}]},
            "llm": {
                "base_url": "https://user:password@example.test/v1/private-token?api_key=query-secret#fragment-secret",
                "model": "research-model"
            }
        }));

        assert_eq!(request, json!({"schema_version": 2}));
    }

    #[test]
    fn migrates_once_and_invalidates_legacy_replay_payloads() {
        let path = temporary_database_path("request-migration");
        let store = AgentRunStore::open(&path).expect("agent run store should open");
        store
            .start_run(
                &json!({
                    "run_id": "run-legacy-request",
                    "conversation_id": "conversation-legacy-request",
                    "message": "Question",
                    "mode": "quick"
                }),
                1_000,
            )
            .expect("run should be started");
        store
            .connection
            .execute(
                "UPDATE agent_runs
                 SET request_json = ?1, events_json = ?2, result_json = ?3,
                     error = ?4, status = 'completed'
                 WHERE run_id = ?5",
                rusqlite::params![
                    r#"{"history":["legacy-secret"],"llm":{"api_key":"legacy-key"}}"#,
                    r#"[{"type":"status","label":"legacy-event-secret"}]"#,
                    r#"{"reply":"legacy-result-secret"}"#,
                    "legacy-error-secret",
                    "run-legacy-request"
                ],
            )
            .expect("legacy snapshot should be seeded");
        store
            .connection
            .pragma_update(None, "user_version", 0)
            .expect("legacy schema version should be seeded");
        let overlong_legacy_run_id = "r".repeat(257);
        for (run_id, conversation_id) in [
            ("run-legacy-orphan".to_string(), None),
            (".".to_string(), Some("conversation-dot-run".to_string())),
            (
                "..".to_string(),
                Some("conversation-dot-dot-run".to_string()),
            ),
            (
                "run-legacy-overlong-conversation".to_string(),
                Some("中".repeat(100)),
            ),
            (
                overlong_legacy_run_id.clone(),
                Some("conversation-legacy-overlong-run".to_string()),
            ),
        ] {
            store
                .connection
                .execute(
                    "INSERT INTO agent_runs (
                        run_id, conversation_id, question, mode, status,
                        started_at_epoch_ms, request_json, events_json
                     ) VALUES (?1, ?2, 'Legacy', 'quick', 'completed', 1, '{}', '[]')",
                    rusqlite::params![run_id, conversation_id],
                )
                .expect("orphaned legacy row should be seeded");
        }
        drop(store);

        let migrated = AgentRunStore::open(&path).expect("ledger should reopen");
        assert_eq!(
            stored_request_json(&migrated, "run-legacy-request"),
            json!({"schema_version": 2})
        );
        let record = migrated
            .get_run("run-legacy-request")
            .expect("legacy run should be readable")
            .expect("legacy run should remain listed");
        assert_eq!(record["events"], json!([]));
        assert_eq!(record["result"], Value::Null);
        assert_eq!(record["error"], Value::Null);
        assert_eq!(record["status"], "unknown");
        assert!(migrated
            .get_run("run-legacy-orphan")
            .expect("legacy orphan lookup should succeed")
            .is_none());
        assert!(migrated
            .get_run(".")
            .expect("legacy dot run lookup should succeed")
            .is_none());
        assert!(migrated
            .get_run("..")
            .expect("legacy dot-dot run lookup should succeed")
            .is_none());
        assert!(migrated
            .get_run("run-legacy-overlong-conversation")
            .expect("legacy overlong lookup should succeed")
            .is_none());
        assert!(migrated
            .get_run(&overlong_legacy_run_id)
            .expect("legacy overlong run id lookup should succeed")
            .is_none());
        let version: i64 = migrated
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("migrated schema version should be readable");
        assert_eq!(version, AGENT_LEDGER_SCHEMA_VERSION);

        drop(migrated);
        let reopened = AgentRunStore::open(&path).expect("migrated ledger should reopen");
        assert_eq!(reopened.connection.total_changes(), 0);
        drop(reopened);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_newer_agent_ledger_schemas() {
        let path = temporary_database_path("newer-schema");
        let store = AgentRunStore::open(&path).expect("agent run store should open");
        store
            .connection
            .pragma_update(None, "user_version", AGENT_LEDGER_SCHEMA_VERSION + 1)
            .expect("future schema version should be seeded");
        drop(store);

        let error = AgentRunStore::open(&path)
            .err()
            .expect("newer schema should be rejected");
        assert!(error.contains("newer than supported"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn records_failed_run_with_partial_events() {
        let path = temporary_database_path("failed");
        let store = AgentRunStore::open(&path).expect("agent run store should open");
        store
            .start_run(
                &json!({
                    "run_id": "run-failed",
                    "conversation_id": "conversation-2",
                    "message": "Run the screen",
                    "mode": "expert"
                }),
                2_000,
            )
            .expect("run should be started");

        store
            .fail_run(
                "run-failed",
                &[json!({"type": "status", "stage": "tools"})],
                "market data unavailable",
                2_080,
            )
            .expect("run failure should be recorded");

        let record = store
            .get_run("run-failed")
            .expect("run lookup should succeed")
            .expect("run should exist");
        assert_eq!(record["status"], "failed");
        assert_eq!(record["duration_ms"], 80);
        assert_eq!(record["events"][0]["stage"], "tools");
        assert_eq!(record["result"], Value::Null);
        assert_eq!(record["error"], "market data unavailable");

        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn lists_newest_runs_as_lightweight_summaries() {
        let path = temporary_database_path("list");
        let store = AgentRunStore::open(&path).expect("agent run store should open");
        for (run_id, started_at) in [("run-old", 3_000), ("run-new", 4_000)] {
            store
                .start_run(
                    &json!({
                        "run_id": run_id,
                        "conversation_id": "conversation-list",
                        "message": format!("Question for {run_id}"),
                        "mode": "quick"
                    }),
                    started_at,
                )
                .expect("run should be started");
        }

        let runs = store
            .list_runs(1, Some("conversation-list"))
            .expect("run list should succeed");
        assert_eq!(runs.as_array().map(Vec::len), Some(1));
        assert_eq!(runs[0]["run_id"], "run-new");
        assert_eq!(runs[0]["status"], "running");
        assert!(runs[0].get("request").is_none());
        assert!(runs[0].get("events").is_none());
        assert!(runs[0].get("result").is_none());

        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn start_run_trims_run_id_so_terminal_updates_match_the_stored_row() {
        let path = temporary_database_path("trim");
        let store = AgentRunStore::open(&path).expect("agent run store should open");
        store
            .start_run(
                &json!({
                    "run_id": "  run-padded  ",
                    "conversation_id": "conversation-padded",
                    "message": "Question with a padded id",
                    "mode": "quick"
                }),
                5_000,
            )
            .expect("run should be started");

        // The row is keyed by the trimmed id, so callers must complete with the trimmed value.
        store
            .complete_run("run-padded", &[], &json!({"reply": "done"}), 5_100)
            .expect("trimmed run id should match the stored row");
        assert!(store
            .complete_run("  run-padded  ", &[], &json!({"reply": "done"}), 5_100)
            .is_err());

        let record = store
            .get_run("run-padded")
            .expect("run lookup should succeed")
            .expect("run should exist");
        assert_eq!(record["status"], "completed");

        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_run_and_conversation_ids_over_the_storage_boundary() {
        let path = temporary_database_path("bounded-identities");
        let store = AgentRunStore::open(&path).expect("agent run store should open");

        let conversation_error = store
            .start_run(
                &json!({
                    "run_id": "run-bounded-conversation",
                    "conversation_id": "中".repeat(100),
                    "message": "Question",
                    "mode": "quick"
                }),
                5_000,
            )
            .expect_err("multibyte conversation id should be bounded by UTF-8 bytes");
        assert!(conversation_error.contains("conversation_id exceeds 256 bytes"));

        let run_error = store
            .start_run(
                &json!({
                    "run_id": "r".repeat(257),
                    "conversation_id": "conversation-bounded-run",
                    "message": "Question",
                    "mode": "quick"
                }),
                5_000,
            )
            .expect_err("run id should share the storage boundary");
        assert!(run_error.contains("run_id exceeds 256 bytes"));

        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn reconciles_interrupted_runs_without_touching_terminal_rows() {
        let path = temporary_database_path("reconcile");
        let store = AgentRunStore::open(&path).expect("agent run store should open");
        for (run_id, started_at) in [("run-stranded", 6_000), ("run-settled", 6_100)] {
            store
                .start_run(
                    &json!({
                        "run_id": run_id,
                        "conversation_id": "conversation-reconcile",
                        "message": format!("Question for {run_id}"),
                        "mode": "quick"
                    }),
                    started_at,
                )
                .expect("run should be started");
        }
        store
            .complete_run("run-settled", &[], &json!({"reply": "done"}), 6_200)
            .expect("run should be completed");

        // `run-stranded` stands in for a run whose process died before its terminal update.
        let reconciled =
            reconcile_interrupted_runs(&store.connection).expect("reconciliation should succeed");
        assert_eq!(reconciled, 1);

        let stranded = store
            .get_run("run-stranded")
            .expect("run lookup should succeed")
            .expect("run should exist");
        assert_eq!(stranded["status"], "unknown");
        let settled = store
            .get_run("run-settled")
            .expect("run lookup should succeed")
            .expect("run should exist");
        assert_eq!(settled["status"], "completed");

        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn generated_run_ids_are_unique() {
        let ids: HashSet<String> = (0..512).map(|_| next_run_id()).collect();
        assert_eq!(ids.len(), 512);
    }

    #[test]
    fn deletes_only_the_selected_conversation_runs() {
        let path = temporary_database_path("delete-conversation");
        let store = AgentRunStore::open(&path).expect("agent run store should open");
        for (run_id, conversation_id) in [
            ("run-delete-a", "conversation-delete"),
            ("run-delete-b", "conversation-delete"),
            ("run-keep", "conversation-keep"),
        ] {
            store
                .start_run(
                    &json!({
                        "run_id": run_id,
                        "conversation_id": conversation_id,
                        "message": "Question",
                        "mode": "quick"
                    }),
                    7_000,
                )
                .expect("run should be started");
        }

        assert_eq!(
            store
                .delete_conversation_runs("conversation-delete")
                .expect("delete should succeed"),
            2
        );
        assert!(store
            .get_run("run-delete-a")
            .expect("lookup should succeed")
            .is_none());
        assert!(store
            .get_run("run-delete-b")
            .expect("lookup should succeed")
            .is_none());
        assert!(store
            .get_run("run-keep")
            .expect("lookup should succeed")
            .is_some());
        let deleted_error = store
            .start_run(
                &json!({
                    "run_id": "run-after-delete",
                    "conversation_id": "conversation-delete",
                    "message": "Must not be persisted",
                    "mode": "quick"
                }),
                7_100,
            )
            .expect_err("a deleted conversation must reject future runs");
        assert!(is_conversation_deleted_error(&deleted_error));
        store
            .start_run(
                &json!({
                    "run_id": "run-keep-later",
                    "conversation_id": "conversation-keep",
                    "message": "Still active",
                    "mode": "quick"
                }),
                7_100,
            )
            .expect("other conversations should remain writable");

        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn deletion_racing_start_cannot_leave_an_orphan_run() {
        let path = temporary_database_path("delete-start-race");
        drop(AgentRunStore::open(&path).expect("agent run store should open"));
        let barrier = Arc::new(Barrier::new(2));
        let delete_path = path.clone();
        let delete_barrier = Arc::clone(&barrier);
        let delete_thread = thread::spawn(move || {
            let store = AgentRunStore::open(&delete_path).expect("delete store should open");
            delete_barrier.wait();
            store
                .delete_conversation_runs("conversation-race")
                .expect("delete should succeed");
        });
        let start_path = path.clone();
        let start_barrier = Arc::clone(&barrier);
        let start_thread = thread::spawn(move || {
            let store = AgentRunStore::open(&start_path).expect("start store should open");
            start_barrier.wait();
            store.start_run(
                &json!({
                    "run_id": "run-race",
                    "conversation_id": "conversation-race",
                    "message": "Racing question",
                    "mode": "quick"
                }),
                8_000,
            )
        });

        delete_thread.join().expect("delete thread should finish");
        let _ = start_thread.join().expect("start thread should finish");
        let store = AgentRunStore::open(&path).expect("ledger should reopen");
        assert!(store
            .get_run("run-race")
            .expect("lookup should succeed")
            .is_none());

        drop(store);
        let _ = std::fs::remove_file(path);
    }
}
