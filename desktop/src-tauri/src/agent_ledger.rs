use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Map, Value};
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
        let connection = Connection::open(path)
            .map_err(|error| format!("failed to open agent ledger: {error}"))?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|error| format!("failed to configure agent ledger: {error}"))?;
        initialize_schema(&connection)?;
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
        let conversation_id = optional_string(payload, "conversation_id");
        let mode = optional_string(payload, "mode").unwrap_or_else(|| "quick".to_string());
        let request_json = serde_json::to_string(&sanitized_request(payload))
            .map_err(|error| format!("failed to encode agent request: {error}"))?;
        self.connection
            .execute(
                "INSERT INTO agent_runs (
                    run_id, conversation_id, question, mode, status, started_at_epoch_ms,
                    request_json, events_json
                 ) VALUES (?1, ?2, ?3, ?4, 'running', ?5, ?6, '[]')",
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
        // `request_json` is deliberately not selected: the replay drawer's AgentRunDetail has no
        // `request` field, and the stored request still holds the chat history, the watchlist
        // context, and the LLM endpoint — none of which belongs in the webview.
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

fn sanitized_request(payload: &Value) -> Value {
    let mut request = Map::new();
    for key in [
        "run_id",
        "conversation_id",
        "message",
        "mode",
        "context",
        "platform",
        "history",
    ] {
        if let Some(value) = payload.get(key) {
            request.insert(key.to_string(), value.clone());
        }
    }
    if let Some(llm) = payload.get("llm").and_then(Value::as_object) {
        let mut safe_llm = Map::new();
        for key in [
            "provider",
            "base_url",
            "model",
            "api_format",
            "endpoint_mode",
            "custom_user_agent",
            "temperature",
            "timeout_seconds",
            "json_mode",
        ] {
            if let Some(value) = llm.get(key) {
                safe_llm.insert(key.to_string(), value.clone());
            }
        }
        request.insert("llm".to_string(), Value::Object(safe_llm));
    }
    if let Some(network) = payload.get("network").and_then(Value::as_object) {
        let mut safe_network = Map::new();
        for key in ["proxy_mode", "android_short_sources"] {
            if let Some(value) = network.get(key) {
                safe_network.insert(key.to_string(), value.clone());
            }
        }
        request.insert("network".to_string(), Value::Object(safe_network));
    }
    Value::Object(request)
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
    use super::{next_run_id, reconcile_interrupted_runs, AgentRunStore};
    use serde_json::{json, Value};
    use std::{
        collections::HashSet,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temporary_database_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be valid")
            .as_nanos();
        std::env::temp_dir().join(format!("gp-assistant-agent-ledger-{name}-{unique}.sqlite"))
    }

    /// `get_run` no longer returns the request, so secret-redaction assertions read the column.
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
                "base_url": "https://llm.example.test/v1",
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

        // Secrets must be absent from the row itself, not merely from the detail response.
        let stored_request = stored_request_json(&store, "run-completed");
        assert_eq!(stored_request["llm"]["model"], "research-model");
        assert_eq!(stored_request["llm"]["api_key"], Value::Null);
        assert_eq!(stored_request["llm"]["organization"], Value::Null);
        assert_eq!(stored_request["llm"]["project"], Value::Null);
        let stored_request = stored_request.to_string();
        assert!(!stored_request.contains("must-never-be-persisted"));
        assert!(!stored_request.contains("sensitive-organization"));
        assert!(!stored_request.contains("sensitive-project"));
        assert!(!stored_request.contains("user:password@proxy.example.test"));

        drop(store);
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
    fn reconciles_interrupted_runs_to_unknown_without_touching_terminal_rows() {
        let path = temporary_database_path("reconcile");
        let store = AgentRunStore::open(&path).expect("agent run store should open");
        for (run_id, started_at) in [("run-stranded", 6_000), ("run-settled", 6_100)] {
            store
                .start_run(
                    &json!({
                        "run_id": run_id,
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
    fn generated_run_ids_stay_unique_within_one_millisecond() {
        let ids: HashSet<String> = (0..512).map(|_| next_run_id()).collect();
        assert_eq!(ids.len(), 512);
    }
}
