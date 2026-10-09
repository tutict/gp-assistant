use rusqlite::{params, types::Type, Connection, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

use crate::agent_ledger;

const MAX_PROFILE_RULES: usize = 100;
const MAX_RULE_CHARS: usize = 800;
const MAX_REVIEW_CHARS: usize = 24_000;
const MAX_STRATEGIES: usize = 100;
const MAX_SUPPRESSED_KINDS: usize = 100;
const EVOLUTION_SCHEMA_VERSION: i64 = 2;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct EvolutionSettings {
    pub enabled: bool,
    pub coach_mode: bool,
    pub local_only: bool,
    pub review_trigger: String,
    pub retain_raw_conversation: bool,
    pub profile_version: i64,
}

impl Default for EvolutionSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            coach_mode: false,
            local_only: true,
            review_trigger: "explicit".into(),
            retain_raw_conversation: false,
            profile_version: 0,
        }
    }
}

fn evolution_path(app: &AppHandle) -> Result<PathBuf, String> {
    let mut root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve evolution directory: {error}"))?;
    root.push("evolution");
    root.push("self-evolution.sqlite");
    Ok(root)
}

pub(crate) struct EvolutionStore {
    connection: Connection,
}

fn schema_version(connection: &Connection) -> Result<i64, String> {
    connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|error| error.to_string())
}

fn initialize_schema(connection: &mut Connection) -> Result<(), String> {
    let version = schema_version(connection)?;
    if version > EVOLUTION_SCHEMA_VERSION {
        return Err(format!(
            "evolution schema version {version} is newer than supported version {EVOLUTION_SCHEMA_VERSION}"
        ));
    }
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    transaction
        .execute_batch(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS evolution_settings(
               id INTEGER PRIMARY KEY CHECK(id=1), enabled INTEGER NOT NULL DEFAULT 0,
               coach_mode INTEGER NOT NULL DEFAULT 0, local_only INTEGER NOT NULL DEFAULT 1,
               review_trigger TEXT NOT NULL DEFAULT 'explicit', retain_raw_conversation INTEGER NOT NULL DEFAULT 0,
               profile_version INTEGER NOT NULL DEFAULT 0
             );
             INSERT OR IGNORE INTO evolution_settings(id) VALUES(1);
             CREATE TABLE IF NOT EXISTS evolution_reviews(
               review_id TEXT PRIMARY KEY, run_id TEXT NOT NULL, conversation_id TEXT,
               status TEXT NOT NULL, payload_json TEXT NOT NULL, created_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS evolution_rules(
               rule_id TEXT PRIMARY KEY, kind TEXT NOT NULL, statement TEXT NOT NULL,
               source_review_id TEXT NOT NULL, evidence_refs_json TEXT NOT NULL,
               status TEXT NOT NULL, version INTEGER NOT NULL, confirmed_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS evolution_suppressions(
               kind TEXT PRIMARY KEY, statement TEXT NOT NULL, source_review_id TEXT NOT NULL,
               created_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS sentiment_strategies(
               strategy_id TEXT PRIMARY KEY, name TEXT NOT NULL, stage TEXT NOT NULL,
               market_regime TEXT, payload_json TEXT NOT NULL, status TEXT NOT NULL,
               version INTEGER NOT NULL, created_at INTEGER NOT NULL, source_analysis_id TEXT
             );
             CREATE TABLE IF NOT EXISTS sentiment_strategy_versions(
               strategy_id TEXT NOT NULL, version INTEGER NOT NULL, payload_json TEXT NOT NULL,
               status TEXT NOT NULL, created_at INTEGER NOT NULL,
               PRIMARY KEY(strategy_id, version)
             );
             INSERT OR IGNORE INTO sentiment_strategy_versions(strategy_id,version,payload_json,status,created_at)
               SELECT strategy_id,version,payload_json,status,created_at FROM sentiment_strategies;
            ",
        )
        .map_err(|error| error.to_string())?;
    transaction
        .pragma_update(None, "user_version", EVOLUTION_SCHEMA_VERSION)
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())
}

impl EvolutionStore {
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut connection = Connection::open(path).map_err(|error| error.to_string())?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|error| error.to_string())?;
        initialize_schema(&mut connection)?;
        Ok(Self { connection })
    }

    fn settings(&self) -> Result<EvolutionSettings, String> {
        self.connection
            .query_row(
                "SELECT enabled,coach_mode,local_only,review_trigger,
                        retain_raw_conversation,profile_version
                 FROM evolution_settings WHERE id=1",
                [],
                |row| {
                    Ok(EvolutionSettings {
                        enabled: row.get::<_, i64>(0)? != 0,
                        coach_mode: row.get::<_, i64>(1)? != 0,
                        local_only: row.get::<_, i64>(2)? != 0,
                        review_trigger: row.get(3)?,
                        retain_raw_conversation: row.get::<_, i64>(4)? != 0,
                        profile_version: row.get(5)?,
                    })
                },
            )
            .map_err(|error| error.to_string())
    }

    fn update_settings(&self, value: &Value) -> Result<EvolutionSettings, String> {
        let current = self.settings()?;
        let enabled = value
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(current.enabled);
        let coach_mode = value
            .get("coach_mode")
            .and_then(Value::as_bool)
            .unwrap_or(current.coach_mode);
        self.connection
            .execute(
                "UPDATE evolution_settings SET enabled=?1,coach_mode=?2,
                 local_only=1,review_trigger='explicit',retain_raw_conversation=0 WHERE id=1",
                params![enabled as i64, coach_mode as i64],
            )
            .map_err(|error| error.to_string())?;
        self.settings()
    }

    fn rules(&self) -> Result<Vec<Value>, String> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT rule_id,kind,statement,source_review_id,evidence_refs_json,
                    status,version,confirmed_at
             FROM evolution_rules
             WHERE status != 'deleted'
             ORDER BY confirmed_at DESC LIMIT ?1",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement.query_map([MAX_PROFILE_RULES as i64], |row| {
            let evidence_refs: String = row.get(4)?;
            Ok(json!({
                "rule_id": row.get::<_, String>(0)?,
                "kind": row.get::<_, String>(1)?,
                "statement": row.get::<_, String>(2)?,
                "source_review_id": row.get::<_, String>(3)?,
                "evidence_refs": serde_json::from_str::<Value>(&evidence_refs).map_err(|error| rusqlite::Error::FromSqlConversionFailure(4, Type::Text, Box::new(error)))?,
                "status": row.get::<_, String>(5)?,
                "version": row.get::<_, i64>(6)?,
                "confirmed_at": row.get::<_, i64>(7)?,
            }))
        }).map_err(|error| error.to_string())?;
        rows.map(|row| row.map_err(|error| error.to_string()))
            .collect()
    }

    fn suppressed_kinds(&self) -> Result<Vec<String>, String> {
        let mut statement = self
            .connection
            .prepare("SELECT kind FROM evolution_suppressions ORDER BY created_at DESC LIMIT ?1")
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([MAX_SUPPRESSED_KINDS as i64], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        rows.map(|row| row.map_err(|error| error.to_string()))
            .collect()
    }

    fn profile(&self) -> Result<Value, String> {
        let settings = self.settings()?;
        Ok(json!({
            "settings": settings,
            "profile_version": settings.profile_version,
            "rules": self.rules()?,
            "suppressed_kinds": self.suppressed_kinds()?,
        }))
    }

    fn reset_profile(&self) -> Result<Value, String> {
        self.connection
            .execute(
                "UPDATE evolution_rules SET status='deleted' WHERE status != 'deleted'",
                [],
            )
            .map_err(|error| error.to_string())?;
        self.connection
            .execute("DELETE FROM evolution_suppressions", [])
            .map_err(|error| error.to_string())?;
        self.connection
            .execute(
                "UPDATE evolution_settings SET profile_version=profile_version+1 WHERE id=1",
                [],
            )
            .map_err(|error| error.to_string())?;
        self.profile()
    }

    fn confirm_rule(&self, payload: &Value) -> Result<Value, String> {
        let statement = payload
            .get("statement")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.chars().count() <= MAX_RULE_CHARS)
            .ok_or("rule statement is empty or too long")?;
        let review_id = payload
            .get("source_review_id")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or("source_review_id is required")?;
        let kind = payload
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("research_process");
        if self.suppressed_kinds()?.iter().any(|item| item == kind) {
            return Err("this research rule kind is suppressed; resume suggestions first".into());
        }
        let evidence_refs = payload
            .get("evidence_refs")
            .cloned()
            .unwrap_or_else(|| json!([]));
        if !evidence_refs.is_array() || evidence_refs.as_array().map_or(true, Vec::is_empty) {
            return Err("confirmed evolution rules require evidence_refs".into());
        }
        let rule_id = payload
            .get("rule_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.len() <= 128)
            .ok_or("rule_id is required")?;
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        let version: i64 = transaction
            .query_row(
                "SELECT profile_version+1 FROM evolution_settings WHERE id=1",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        transaction.execute(
            "INSERT OR REPLACE INTO evolution_rules
             (rule_id,kind,statement,source_review_id,evidence_refs_json,status,version,confirmed_at)
             VALUES(?1,?2,?3,?4,?5,'active',?6,?7)",
            params![rule_id,kind,statement,review_id,evidence_refs.to_string(),version,agent_ledger::current_epoch_millis()],
        ).map_err(|error| error.to_string())?;
        transaction
            .execute(
                "UPDATE evolution_settings SET profile_version=?1 WHERE id=1",
                [version],
            )
            .map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        self.profile()
    }

    fn edit_rule(&self, payload: &Value) -> Result<Value, String> {
        let rule_id = payload
            .get("rule_id")
            .and_then(Value::as_str)
            .ok_or("rule_id is required")?;
        let statement = payload
            .get("statement")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.chars().count() <= MAX_RULE_CHARS)
            .ok_or("rule statement is empty or too long")?;
        let evidence_refs = payload
            .get("evidence_refs")
            .cloned()
            .unwrap_or_else(|| json!([]));
        if !evidence_refs.is_array() || evidence_refs.as_array().map_or(true, Vec::is_empty) {
            return Err("edited rules require evidence_refs".into());
        }
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        let version: i64 = transaction
            .query_row(
                "SELECT profile_version+1 FROM evolution_settings WHERE id=1",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let changed = transaction.execute(
            "UPDATE evolution_rules SET statement=?1,evidence_refs_json=?2,version=?3,confirmed_at=?4 WHERE rule_id=?5",
            params![statement,evidence_refs.to_string(),version,agent_ledger::current_epoch_millis(),rule_id],
        ).map_err(|error| error.to_string())?;
        if changed == 0 {
            return Err("rule not found".into());
        }
        transaction
            .execute(
                "UPDATE evolution_settings SET profile_version=?1 WHERE id=1",
                [version],
            )
            .map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        self.profile()
    }

    fn set_rule_status(&self, rule_id: &str, status: &str) -> Result<Value, String> {
        if !matches!(status, "active" | "paused" | "never_suggested" | "deleted") {
            return Err("invalid rule status".into());
        }
        self.connection
            .execute(
                "UPDATE evolution_rules SET status=?1 WHERE rule_id=?2",
                params![status, rule_id],
            )
            .map_err(|error| error.to_string())?;
        self.connection
            .execute(
                "UPDATE evolution_settings SET profile_version=profile_version+1 WHERE id=1",
                [],
            )
            .map_err(|error| error.to_string())?;
        self.profile()
    }

    fn suppress_kind(&self, payload: &Value) -> Result<Value, String> {
        let kind = payload
            .get("kind")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or("kind is required")?;
        let statement = payload
            .get("statement")
            .and_then(Value::as_str)
            .unwrap_or("");
        let review_id = payload
            .get("source_review_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        self.connection.execute("INSERT OR REPLACE INTO evolution_suppressions(kind,statement,source_review_id,created_at) VALUES(?1,?2,?3,?4)", params![kind,statement,review_id,agent_ledger::current_epoch_millis()]).map_err(|error| error.to_string())?;
        self.connection
            .execute(
                "UPDATE evolution_settings SET profile_version=profile_version+1 WHERE id=1",
                [],
            )
            .map_err(|error| error.to_string())?;
        self.profile()
    }

    fn unsuppress_kind(&self, kind: &str) -> Result<Value, String> {
        self.connection
            .execute("DELETE FROM evolution_suppressions WHERE kind=?1", [kind])
            .map_err(|error| error.to_string())?;
        self.connection.execute("UPDATE evolution_rules SET status='active' WHERE kind=?1 AND status='never_suggested'", [kind]).map_err(|error| error.to_string())?;
        self.connection
            .execute(
                "UPDATE evolution_settings SET profile_version=profile_version+1 WHERE id=1",
                [],
            )
            .map_err(|error| error.to_string())?;
        self.profile()
    }

    fn save_review(&self, id: &str, payload: &Value) -> Result<(), String> {
        let text = serde_json::to_string(payload).map_err(|error| error.to_string())?;
        if text.len() > MAX_REVIEW_CHARS * 4 {
            return Err("review payload is too large".into());
        }
        self.connection.execute("INSERT OR REPLACE INTO evolution_reviews(review_id,run_id,conversation_id,status,payload_json,created_at) VALUES(?1,?2,?3,'draft',?4,?5)", params![id,payload.get("run_id").and_then(Value::as_str).unwrap_or(""),payload.get("conversation_id").and_then(Value::as_str),text,agent_ledger::current_epoch_millis()]).map_err(|error| error.to_string())?;
        Ok(())
    }

    fn save_strategy(&self, payload: &Value) -> Result<Value, String> {
        let name = payload
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or("strategy name is required")?;
        let stage = payload
            .get("stage")
            .and_then(Value::as_str)
            .unwrap_or("证据不足");
        if stage == "证据不足" {
            return Err("insufficient-evidence strategies cannot be saved".into());
        }
        let evidence_ids = payload
            .get("evidence_ids")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if evidence_ids.is_empty() {
            return Err("strategies require evidence_ids".into());
        }
        let backtest = payload
            .get("backtest")
            .and_then(Value::as_object)
            .ok_or("strategies require a backtest summary")?;
        if backtest.get("passed").and_then(Value::as_bool) != Some(true) {
            return Err("strategy backtest has not passed".into());
        }
        let allowed = [
            "min_roe",
            "max_pe",
            "max_pb",
            "min_market_cap_billion",
            "min_deducted_net_profit_billion",
            "min_deducted_net_profit_margin",
            "min_deducted_net_profit_growth_rate",
            "score_profile",
            "limit",
            "primary_limit",
            "exploration_limit",
        ];
        for change in payload
            .get("changes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
        {
            let field = change.get("field").and_then(Value::as_str).unwrap_or("");
            if !allowed.contains(&field) {
                return Err(format!("strategy field is not allowed: {field}"));
            }
            let ratio = change
                .get("relative_change")
                .and_then(Value::as_f64)
                .unwrap_or(f64::NAN);
            if !ratio.is_finite() || ratio.abs() > 0.2 {
                return Err("strategy parameter change exceeds 20% bound".into());
            }
        }
        let id = payload
            .get("strategy_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.len() <= 128)
            .ok_or("strategy_id is required")?;
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        let version: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(version),0)+1 FROM sentiment_strategy_versions WHERE strategy_id=?1",
            [id], |row| row.get(0),
        ).map_err(|error| error.to_string())?;
        let now = agent_ledger::current_epoch_millis();
        transaction.execute("UPDATE sentiment_strategy_versions SET status='superseded' WHERE strategy_id=?1", [id]).map_err(|error| error.to_string())?;
        transaction.execute(
            "INSERT INTO sentiment_strategy_versions(strategy_id,version,payload_json,status,created_at) VALUES(?1,?2,?3,'active',?4)",
            params![id,version,payload.to_string(),now],
        ).map_err(|error| error.to_string())?;
        transaction.execute(
            "INSERT OR REPLACE INTO sentiment_strategies(strategy_id,name,stage,market_regime,payload_json,status,version,created_at,source_analysis_id) VALUES(?1,?2,?3,?4,?5,'active',?6,?7,?8)",
            params![id,name,stage,payload.get("market_regime").and_then(Value::as_str),payload.to_string(),version,now,payload.get("source_analysis_id").and_then(Value::as_str)],
        ).map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(json!({"strategy_id":id,"version":version,"status":"active"}))
    }

    fn strategies(&self) -> Result<Value, String> {
        let mut statement = self.connection.prepare("SELECT strategy_id,name,stage,market_regime,payload_json,status,version,created_at,source_analysis_id FROM sentiment_strategies ORDER BY created_at DESC LIMIT ?1").map_err(|error| error.to_string())?;
        let rows = statement.query_map([MAX_STRATEGIES as i64], |row| { let payload: Value = serde_json::from_str::<Value>(&row.get::<_,String>(4)?).map_err(|error| rusqlite::Error::FromSqlConversionFailure(4, Type::Text, Box::new(error)))?; Ok(json!({"strategy_id":row.get::<_,String>(0)?,"name":row.get::<_,String>(1)?,"stage":row.get::<_,String>(2)?,"market_regime":row.get::<_,Option<String>>(3)?,"payload":payload,"status":row.get::<_,String>(5)?,"version":row.get::<_,i64>(6)?,"created_at":row.get::<_,i64>(7)?,"source_analysis_id":row.get::<_,Option<String>>(8)?})) }).map_err(|error| error.to_string())?;
        let items = rows
            .map(|row| row.map_err(|error| error.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(json!({"items":items}))
    }

    fn strategy_versions(&self, id: &str) -> Result<Value, String> {
        let mut statement = self.connection.prepare("SELECT version,payload_json,status,created_at FROM sentiment_strategy_versions WHERE strategy_id=?1 ORDER BY version DESC").map_err(|error| error.to_string())?;
        let rows = statement.query_map([id], |row| { let payload: Value = serde_json::from_str::<Value>(&row.get::<_,String>(1)?).map_err(|error| rusqlite::Error::FromSqlConversionFailure(1, Type::Text, Box::new(error)))?; Ok(json!({"version":row.get::<_,i64>(0)?,"payload":payload,"status":row.get::<_,String>(2)?,"created_at":row.get::<_,i64>(3)?})) }).map_err(|error| error.to_string())?;
        let items = rows
            .map(|row| row.map_err(|error| error.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(json!({"strategy_id":id,"items":items}))
    }

    fn set_strategy_status(&self, id: &str, status: &str) -> Result<Value, String> {
        if !matches!(status, "active" | "disabled") {
            return Err("invalid strategy status".into());
        }
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        let current_version: i64 = transaction
            .query_row(
                "SELECT version FROM sentiment_strategies WHERE strategy_id=?1",
                [id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let changed = transaction
            .execute(
                "UPDATE sentiment_strategies SET status=?1 WHERE strategy_id=?2",
                params![status, id],
            )
            .map_err(|error| error.to_string())?;
        if changed == 0 {
            return Err("strategy not found".into());
        }
        let version_status = if status == "disabled" {
            "disabled"
        } else {
            "superseded"
        };
        transaction
            .execute(
                "UPDATE sentiment_strategy_versions SET status=?1 WHERE strategy_id=?2",
                params![version_status, id],
            )
            .map_err(|error| error.to_string())?;
        if status == "active" {
            transaction.execute("UPDATE sentiment_strategy_versions SET status='active' WHERE strategy_id=?1 AND version=?2", params![id,current_version]).map_err(|error| error.to_string())?;
        }
        transaction.commit().map_err(|error| error.to_string())?;
        self.strategies()
    }

    fn rollback_strategy(&self, id: &str, version: i64) -> Result<Value, String> {
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        let payload_text: String = transaction.query_row(
            "SELECT payload_json FROM sentiment_strategy_versions WHERE strategy_id=?1 AND version=?2",
            params![id,version], |row| row.get(0),
        ).map_err(|error| error.to_string())?;
        let payload: Value =
            serde_json::from_str(&payload_text).map_err(|error| error.to_string())?;
        let next: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(version),0)+1 FROM sentiment_strategy_versions WHERE strategy_id=?1",
            [id], |row| row.get(0),
        ).map_err(|error| error.to_string())?;
        let mut rolled = payload.clone();
        rolled["rolled_back_from"] = json!(version);
        let now = agent_ledger::current_epoch_millis();
        transaction
            .execute(
                "UPDATE sentiment_strategy_versions SET status='superseded' WHERE strategy_id=?1",
                [id],
            )
            .map_err(|error| error.to_string())?;
        transaction.execute(
            "INSERT INTO sentiment_strategy_versions(strategy_id,version,payload_json,status,created_at) VALUES(?1,?2,?3,'active',?4)",
            params![id,next,rolled.to_string(),now],
        ).map_err(|error| error.to_string())?;
        let changed = transaction.execute(
            "UPDATE sentiment_strategies SET payload_json=?1,version=?2,status='active',created_at=?3 WHERE strategy_id=?4",
            params![rolled.to_string(),next,now,id],
        ).map_err(|error| error.to_string())?;
        if changed == 0 {
            return Err("strategy not found".into());
        }
        transaction.commit().map_err(|error| error.to_string())?;
        self.strategies()
    }
}

fn with_store<T, F>(app: &AppHandle, operation: F) -> Result<T, String>
where
    F: FnOnce(&EvolutionStore) -> Result<T, String>,
{
    let store = EvolutionStore::open(evolution_path(app)?)?;
    operation(&store)
}

fn deterministic_review(payload: &Value, suppressed: &[String]) -> Value {
    let question = payload
        .get("question")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let answer = payload
        .get("answer")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let evidence = payload
        .get("evidence_ids")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let tools = payload
        .get("tool_calls")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut blind = Vec::new();
    if evidence.is_empty() && !suppressed.iter().any(|kind| kind == "evidence_coverage") {
        blind.push(json!({"kind":"evidence_coverage","statement":"本次研究没有可确认的证据引用，不能把结论升级为长期规则。","evidence_refs":[]}));
    }
    if !answer.is_empty()
        && !answer.contains("失效")
        && !answer.contains("不确定")
        && !suppressed.iter().any(|kind| kind == "invalidation")
    {
        blind.push(json!({"kind":"invalidation","statement":"本次回答没有明确写出失效条件或不确定性。","evidence_refs":evidence}));
    }
    json!({"review_id":agent_ledger::next_run_id(),"run_id":payload.get("run_id").cloned().unwrap_or_else(||json!("")),"conversation_id":payload.get("conversation_id").cloned().unwrap_or_else(||json!(null)),"status":"draft","review_mode":"deterministic","research_goal":question,"evidence_and_process":{"evidence_ids":evidence,"tool_calls":tools},"conclusion_quality":{"answer":answer,"has_answer":!answer.is_empty()},"blind_spot_candidates":blind,"rule_candidates":[],"model_feedback":null})
}

fn is_loopback_host(host: Option<&str>) -> bool {
    let Some(host) = host else {
        return false;
    };
    let host = host.trim().trim_matches('[').trim_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host.ends_with(".localhost")
        || host
            .parse::<std::net::IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
}

#[tauri::command]
pub(crate) async fn api_evolution_review_enhance(
    app: AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let llm = payload
        .get("llm")
        .cloned()
        .ok_or("local model configuration is required")?;
    let base_url = llm.get("base_url").and_then(Value::as_str).unwrap_or("");
    let url =
        reqwest::Url::parse(base_url).map_err(|_| "invalid local model endpoint".to_string())?;
    if !is_loopback_host(url.host_str()) {
        return Err(
            "model enhancement is local-only; configure a localhost model endpoint".to_string(),
        );
    }
    if llm
        .get("proxy_mode")
        .and_then(Value::as_str)
        .unwrap_or("none")
        != "none"
    {
        return Err(
            "model enhancement refuses proxy-routed endpoints; use a direct localhost model"
                .to_string(),
        );
    }
    let review = payload.get("review").cloned().ok_or("review is required")?;
    let review_text = serde_json::to_string(&review).map_err(|error| error.to_string())?;
    if review_text.len() > MAX_REVIEW_CHARS {
        return Err("review payload is too large".into());
    }
    let prompt = format!("请对下面这次股票研究复盘做研究过程层面的增强。只能评价问题定义、证据覆盖、反证、失效条件和下一步验证；不要做人格诊断、心理诊断、交易建议或收益承诺。不要改写证据事实。输出简洁中文反馈。\\n复盘 JSON：{review_text}");
    let run_id = format!("evolution-review-{}", agent_ledger::current_epoch_millis());
    let outcome = crate::rig_runtime::execute_with_app_and_event_sink(
        app.clone(),
        json!({"run_id": run_id, "mode": "research", "message": prompt, "llm": llm}),
        json!({}),
        |_| {},
    )
    .await?;
    if outcome
        .response
        .get("harness")
        .and_then(|value| value.get("model_used"))
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Err("local model did not produce an enhanced review".into());
    }
    let feedback = outcome
        .response
        .get("reply")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if feedback.is_empty() {
        return Err("local model returned an empty review".into());
    }
    let mut result = review;
    if let Some(object) = result.as_object_mut() {
        object.insert("review_mode".into(), json!("model"));
        object.insert("model_feedback".into(), json!(feedback));
        object.insert("model_used".into(), json!(true));
    }
    let review_id = result
        .get("review_id")
        .and_then(Value::as_str)
        .ok_or("review_id is required")?
        .to_string();
    with_store(&app, |store| {
        store.save_review(&review_id, &result)?;
        Ok(())
    })?;
    Ok(result)
}

#[tauri::command]
pub(crate) fn api_evolution_settings(
    app: AppHandle,
    payload: Option<Value>,
) -> Result<Value, String> {
    with_store(&app, |store| {
        Ok(json!(match payload
            .as_ref()
            .filter(|value| value.as_object().is_some_and(|object| !object.is_empty()))
        {
            Some(value) => store.update_settings(value)?,
            None => store.settings()?,
        }))
    })
}
#[tauri::command]
pub(crate) fn api_evolution_profile(app: AppHandle) -> Result<Value, String> {
    with_store(&app, EvolutionStore::profile)
}
#[tauri::command]
pub(crate) fn api_evolution_profile_reset(app: AppHandle) -> Result<Value, String> {
    with_store(&app, EvolutionStore::reset_profile)
}
#[tauri::command]
pub(crate) fn api_evolution_review(app: AppHandle, payload: Value) -> Result<Value, String> {
    with_store(&app, |store| {
        let review = deterministic_review(&payload, &store.suppressed_kinds()?);
        let id = review
            .get("review_id")
            .and_then(Value::as_str)
            .unwrap_or("review")
            .to_string();
        store.save_review(&id, &review)?;
        Ok(review)
    })
}
#[tauri::command]
pub(crate) fn api_evolution_confirm_rule(app: AppHandle, payload: Value) -> Result<Value, String> {
    with_store(&app, |store| store.confirm_rule(&payload))
}
#[tauri::command]
pub(crate) fn api_evolution_edit_rule(app: AppHandle, payload: Value) -> Result<Value, String> {
    with_store(&app, |store| store.edit_rule(&payload))
}
#[tauri::command]
pub(crate) fn api_evolution_rule_status(app: AppHandle, payload: Value) -> Result<Value, String> {
    let id = payload
        .get("rule_id")
        .and_then(Value::as_str)
        .ok_or("rule_id is required")?;
    let status = payload
        .get("status")
        .and_then(Value::as_str)
        .ok_or("status is required")?;
    with_store(&app, |store| store.set_rule_status(id, status))
}
#[tauri::command]
pub(crate) fn api_evolution_delete_rule(app: AppHandle, payload: Value) -> Result<Value, String> {
    let id = payload
        .get("rule_id")
        .and_then(Value::as_str)
        .ok_or("rule_id is required")?;
    with_store(&app, |store| store.set_rule_status(id, "deleted"))
}
#[tauri::command]
pub(crate) fn api_evolution_suppress_kind(app: AppHandle, payload: Value) -> Result<Value, String> {
    with_store(&app, |store| store.suppress_kind(&payload))
}
#[tauri::command]
pub(crate) fn api_evolution_unsuppress_kind(
    app: AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let kind = payload
        .get("kind")
        .and_then(Value::as_str)
        .ok_or("kind is required")?;
    with_store(&app, |store| store.unsuppress_kind(kind))
}
#[tauri::command]
pub(crate) fn api_sentiment_strategy_save(app: AppHandle, payload: Value) -> Result<Value, String> {
    with_store(&app, |store| store.save_strategy(&payload))
}
#[tauri::command]
pub(crate) fn api_sentiment_strategies(app: AppHandle) -> Result<Value, String> {
    with_store(&app, EvolutionStore::strategies)
}
#[tauri::command]
pub(crate) fn api_sentiment_strategy_versions(
    app: AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let id = payload
        .get("strategy_id")
        .and_then(Value::as_str)
        .ok_or("strategy_id is required")?;
    with_store(&app, |store| store.strategy_versions(id))
}
#[tauri::command]
pub(crate) fn api_sentiment_strategy_status(
    app: AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let id = payload
        .get("strategy_id")
        .and_then(Value::as_str)
        .ok_or("strategy_id is required")?;
    let status = payload
        .get("status")
        .and_then(Value::as_str)
        .ok_or("status is required")?;
    with_store(&app, |store| store.set_strategy_status(id, status))
}
#[tauri::command]
pub(crate) fn api_sentiment_strategy_rollback(
    app: AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let id = payload
        .get("strategy_id")
        .and_then(Value::as_str)
        .ok_or("strategy_id is required")?;
    let version = payload
        .get("version")
        .and_then(Value::as_i64)
        .ok_or("version is required")?;
    with_store(&app, |store| store.rollback_strategy(id, version))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    fn path(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("evolution-{name}-{stamp}.sqlite"))
    }
    #[test]
    fn rejects_newer_schema_versions() {
        let p = path("future-schema");
        let connection = Connection::open(&p).unwrap();
        connection.pragma_update(None, "user_version", 99).unwrap();
        assert!(EvolutionStore::open(&p).is_err());
        let _ = std::fs::remove_file(p);
    }
    #[test]
    fn defaults_are_disabled_and_local_only() {
        let p = path("settings");
        let s = EvolutionStore::open(&p).unwrap();
        let x = s.settings().unwrap();
        assert!(!x.enabled);
        assert!(x.local_only);
        assert_eq!(x.review_trigger, "explicit");
        let _ = std::fs::remove_file(p);
    }
    #[test]
    fn confirmed_rule_requires_evidence_and_increments_profile_version() {
        let p = path("rules");
        let s = EvolutionStore::open(&p).unwrap();
        assert!(s
            .confirm_rule(
                &json!({"statement":"always cite validity window","source_review_id":"r1"})
            )
            .is_err());
        let profile=s.confirm_rule(&json!({"rule_id":"rule-1","statement":"always cite validity window","source_review_id":"r1","evidence_refs":["doc-1"]})).unwrap();
        assert_eq!(profile["profile_version"], 1);
        assert_eq!(profile["rules"].as_array().unwrap().len(), 1);
        let _ = std::fs::remove_file(p);
    }
    #[test]
    fn local_enhancement_endpoint_requires_direct_loopback() {
        assert!(is_loopback_host(Some("127.0.0.1")));
        assert!(is_loopback_host(Some("localhost")));
        assert!(!is_loopback_host(Some("api.example.com")));
    }
    #[test]
    fn strategy_save_requires_evidence_backtest_and_bounded_fields() {
        let p = path("strategy");
        let s = EvolutionStore::open(&p).unwrap();
        let base = json!({"strategy_id":"s1","name":"降温策略","stage":"降温","evidence_ids":["E1"],"backtest":{"passed":true},"changes":[{"field":"max_pe","relative_change":-0.2}]});
        assert!(s.save_strategy(&base).is_ok());
        let mut bad = base.clone();
        bad["changes"][0]["field"] = json!("market_scope");
        assert!(s.save_strategy(&bad).is_err());
        let _ = std::fs::remove_file(p);
    }
    #[test]
    fn strategy_versions_and_rollback_create_a_new_version() {
        let p = path("versions");
        let s = EvolutionStore::open(&p).unwrap();
        let base = json!({"strategy_id":"s1","name":"v1","stage":"降温","evidence_ids":["E1"],"backtest":{"passed":true},"changes":[]});
        assert!(s.save_strategy(&base).is_ok());
        let mut next = base.clone();
        next["name"] = json!("v2");
        assert!(s.save_strategy(&next).is_ok());
        let versions = s.strategy_versions("s1").unwrap()["items"].as_array().unwrap().clone();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0]["status"], "active");
        assert_eq!(versions[1]["status"], "superseded");
        assert!(s.rollback_strategy("s1", 1).is_ok());
        assert_eq!(
            s.strategy_versions("s1").unwrap()["items"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        let _ = std::fs::remove_file(p);
    }
    #[test]
    fn explicit_kind_suppression_is_persisted() {
        let p = path("rule-suppression");
        let s = EvolutionStore::open(&p).unwrap();
        s.suppress_kind(&json!({"kind":"invalidation","statement":"stop suggesting","source_review_id":"review"})).unwrap();
        assert!(s
            .suppressed_kinds()
            .unwrap()
            .contains(&"invalidation".to_string()));
        let _ = std::fs::remove_file(p);
    }
    #[test]
    fn suppressed_kind_is_not_suggested() {
        let p = path("suppression");
        let s = EvolutionStore::open(&p).unwrap();
        s.suppress_kind(
            &json!({"kind":"invalidation","statement":"stop suggesting","source_review_id":"r1"}),
        )
        .unwrap();
        let review = deterministic_review(
            &json!({"run_id":"run-1","question":"q","answer":"a","evidence_ids":["E1"],"tool_calls":[]}),
            &s.suppressed_kinds().unwrap(),
        );
        assert!(review["blind_spot_candidates"]
            .as_array()
            .unwrap()
            .is_empty());
        let _ = std::fs::remove_file(p);
    }
    #[test]
    fn insufficient_evidence_review_is_explicit() {
        let review = deterministic_review(
            &json!({"run_id":"run-1","question":"q","answer":"a","evidence_ids":[],"tool_calls":[]}),
            &[],
        );
        assert_eq!(review["review_mode"], "deterministic");
        assert!(!review["blind_spot_candidates"]
            .as_array()
            .unwrap()
            .is_empty());
    }
}
