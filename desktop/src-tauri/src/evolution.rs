use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

use crate::agent_ledger;

const MAX_PROFILE_RULES: usize = 100;
const MAX_RULE_CHARS: usize = 800;
const MAX_REVIEW_CHARS: usize = 24_000;
const MAX_STRATEGIES: usize = 100;

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
    fn default() -> Self { Self { enabled: false, coach_mode: false, local_only: true, review_trigger: "explicit".into(), retain_raw_conversation: false, profile_version: 0 } }
}
fn evolution_path(app: &AppHandle) -> Result<PathBuf, String> {
    let mut root = app.path().app_data_dir().map_err(|e| format!("failed to resolve evolution directory: {e}"))?;
    root.push("evolution"); root.push("self-evolution.sqlite"); Ok(root)
}
pub(crate) struct EvolutionStore { connection: Connection }
impl EvolutionStore {
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref(); if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
        let connection = Connection::open(path).map_err(|e| e.to_string())?;
        connection.busy_timeout(std::time::Duration::from_secs(5)).map_err(|e| e.to_string())?;
        connection.execute_batch("PRAGMA foreign_keys=ON;
CREATE TABLE IF NOT EXISTS evolution_settings(id INTEGER PRIMARY KEY CHECK(id=1), enabled INTEGER NOT NULL DEFAULT 0, coach_mode INTEGER NOT NULL DEFAULT 0, local_only INTEGER NOT NULL DEFAULT 1, review_trigger TEXT NOT NULL DEFAULT 'explicit', retain_raw_conversation INTEGER NOT NULL DEFAULT 0, profile_version INTEGER NOT NULL DEFAULT 0);
INSERT OR IGNORE INTO evolution_settings(id) VALUES(1);
CREATE TABLE IF NOT EXISTS evolution_reviews(review_id TEXT PRIMARY KEY, run_id TEXT NOT NULL, conversation_id TEXT, status TEXT NOT NULL, payload_json TEXT NOT NULL, created_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS evolution_rules(rule_id TEXT PRIMARY KEY, kind TEXT NOT NULL, statement TEXT NOT NULL, source_review_id TEXT NOT NULL, evidence_refs_json TEXT NOT NULL, status TEXT NOT NULL, version INTEGER NOT NULL, confirmed_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS sentiment_strategies(strategy_id TEXT PRIMARY KEY, name TEXT NOT NULL, stage TEXT NOT NULL, market_regime TEXT, payload_json TEXT NOT NULL, status TEXT NOT NULL, version INTEGER NOT NULL, created_at INTEGER NOT NULL, source_analysis_id TEXT);
PRAGMA user_version=1;").map_err(|e| e.to_string())?;
        Ok(Self { connection })
    }
    fn settings(&self) -> Result<EvolutionSettings, String> {
        self.connection.query_row("SELECT enabled,coach_mode,local_only,review_trigger,retain_raw_conversation,profile_version FROM evolution_settings WHERE id=1", [], |r| Ok(EvolutionSettings { enabled: r.get::<_,i64>(0)? != 0, coach_mode: r.get::<_,i64>(1)? != 0, local_only: r.get::<_,i64>(2)? != 0, review_trigger: r.get(3)?, retain_raw_conversation: r.get::<_,i64>(4)? != 0, profile_version: r.get(5)? })).map_err(|e| e.to_string())
    }
    fn update_settings(&self, value: &Value) -> Result<EvolutionSettings, String> {
        let current = self.settings()?; let enabled = value.get("enabled").and_then(Value::as_bool).unwrap_or(current.enabled); let coach_mode = value.get("coach_mode").and_then(Value::as_bool).unwrap_or(current.coach_mode);
        self.connection.execute("UPDATE evolution_settings SET enabled=?1,coach_mode=?2,local_only=1,review_trigger='explicit',retain_raw_conversation=0 WHERE id=1", params![enabled as i64, coach_mode as i64]).map_err(|e| e.to_string())?; self.settings()
    }
    fn rules(&self) -> Result<Vec<Value>, String> {
        let mut stmt = self.connection.prepare("SELECT rule_id,kind,statement,source_review_id,evidence_refs_json,status,version,confirmed_at FROM evolution_rules WHERE status='active' ORDER BY confirmed_at DESC LIMIT ?1").map_err(|e| e.to_string())?;
        let rows = stmt.query_map([MAX_PROFILE_RULES as i64], |r| { let refs: String = r.get(4)?; Ok(json!({"rule_id":r.get::<_,String>(0)?,"kind":r.get::<_,String>(1)?,"statement":r.get::<_,String>(2)?,"source_review_id":r.get::<_,String>(3)?,"evidence_refs":serde_json::from_str::<Value>(&refs).unwrap_or_else(|_|json!([])),"status":r.get::<_,String>(5)?,"version":r.get::<_,i64>(6)?,"confirmed_at":r.get::<_,i64>(7)?})) }).map_err(|e| e.to_string())?;
        rows.map(|r| r.map_err(|e| e.to_string())).collect()
    }
    fn profile(&self) -> Result<Value, String> { let settings = self.settings()?; Ok(json!({"settings":settings,"profile_version":settings.profile_version,"rules":self.rules()?})) }
    fn reset_profile(&self) -> Result<Value, String> { self.connection.execute("UPDATE evolution_rules SET status='deleted' WHERE status='active'", []).map_err(|e| e.to_string())?; self.connection.execute("UPDATE evolution_settings SET profile_version=profile_version+1 WHERE id=1", []).map_err(|e| e.to_string())?; self.profile() }
    fn confirm_rule(&self, payload: &Value) -> Result<Value, String> {
        let statement = payload.get("statement").and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty() && v.chars().count() <= MAX_RULE_CHARS).ok_or("rule statement is empty or too long")?;
        let review_id = payload.get("source_review_id").and_then(Value::as_str).filter(|v| !v.trim().is_empty()).ok_or("source_review_id is required")?;
        let refs = payload.get("evidence_refs").cloned().unwrap_or_else(||json!([])); if !refs.is_array() || refs.as_array().map_or(true, Vec::is_empty) { return Err("confirmed evolution rules require evidence_refs".into()); }
        let rule_id = payload.get("rule_id").and_then(Value::as_str).filter(|v| !v.trim().is_empty()).unwrap_or("rule"); let kind = payload.get("kind").and_then(Value::as_str).unwrap_or("research_process");
        let version: i64 = self.connection.query_row("SELECT profile_version+1 FROM evolution_settings WHERE id=1", [], |r| r.get(0)).map_err(|e| e.to_string())?;
        self.connection.execute("INSERT OR REPLACE INTO evolution_rules(rule_id,kind,statement,source_review_id,evidence_refs_json,status,version,confirmed_at) VALUES(?1,?2,?3,?4,?5,'active',?6,?7)", params![rule_id,kind,statement,review_id,refs.to_string(),version,agent_ledger::current_epoch_millis()]).map_err(|e| e.to_string())?;
        self.connection.execute("UPDATE evolution_settings SET profile_version=?1 WHERE id=1", [version]).map_err(|e| e.to_string())?; self.profile()
    }
    fn delete_rule(&self, id: &str) -> Result<Value, String> { self.connection.execute("UPDATE evolution_rules SET status='deleted' WHERE rule_id=?1", [id]).map_err(|e| e.to_string())?; self.profile() }
    fn save_review(&self, id: &str, payload: &Value) -> Result<(), String> { let text = serde_json::to_string(payload).map_err(|e| e.to_string())?; if text.len() > MAX_REVIEW_CHARS * 4 { return Err("review payload is too large".into()); } self.connection.execute("INSERT OR REPLACE INTO evolution_reviews(review_id,run_id,conversation_id,status,payload_json,created_at) VALUES(?1,?2,?3,'draft',?4,?5)", params![id,payload.get("run_id").and_then(Value::as_str).unwrap_or(""),payload.get("conversation_id").and_then(Value::as_str),text,agent_ledger::current_epoch_millis()]).map_err(|e| e.to_string())?; Ok(()) }
    fn save_strategy(&self, payload: &Value) -> Result<Value, String> {
        let name = payload.get("name").and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()).ok_or("strategy name is required")?;
        let stage = payload.get("stage").and_then(Value::as_str).unwrap_or("证据不足");
        if stage == "证据不足" { return Err("insufficient-evidence strategies cannot be saved".into()); }
        let evidence_ids = payload.get("evidence_ids").and_then(Value::as_array).cloned().unwrap_or_default();
        if evidence_ids.is_empty() { return Err("strategies require evidence_ids".into()); }
        let backtest = payload.get("backtest").and_then(Value::as_object).ok_or("strategies require a backtest summary")?;
        if backtest.get("passed").and_then(Value::as_bool) != Some(true) { return Err("strategy backtest has not passed".into()); }
        let allowed = ["min_roe", "max_pe", "max_pb", "min_market_cap_billion", "min_deducted_net_profit_billion", "min_deducted_net_profit_margin", "min_deducted_net_profit_growth_rate", "score_profile", "limit", "primary_limit", "exploration_limit"];
        for change in payload.get("changes").and_then(Value::as_array).cloned().unwrap_or_default() {
            let field = change.get("field").and_then(Value::as_str).unwrap_or("");
            if !allowed.contains(&field) { return Err(format!("strategy field is not allowed: {field}")); }
            let ratio = change.get("relative_change").and_then(Value::as_f64).unwrap_or(f64::NAN);
            if !ratio.is_finite() || ratio.abs() > 0.2 { return Err("strategy parameter change exceeds 20% bound".into()); }
        }
        let id = payload.get("strategy_id").and_then(Value::as_str).filter(|v| !v.is_empty()).unwrap_or("strategy");
        let version: i64 = self.connection.query_row("SELECT COALESCE(MAX(version),0)+1 FROM sentiment_strategies WHERE strategy_id=?1", [id], |r| r.get(0)).map_err(|e| e.to_string())?;
        self.connection.execute("INSERT INTO sentiment_strategies(strategy_id,name,stage,market_regime,payload_json,status,version,created_at,source_analysis_id) VALUES(?1,?2,?3,?4,?5,'active',?6,?7,?8)", params![id,name,stage,payload.get("market_regime").and_then(Value::as_str),payload.to_string(),version,agent_ledger::current_epoch_millis(),payload.get("source_analysis_id").and_then(Value::as_str)]).map_err(|e| e.to_string())?;
        Ok(json!({"strategy_id":id,"version":version,"status":"active"}))
    }
    fn strategies(&self) -> Result<Value, String> { let mut stmt = self.connection.prepare("SELECT strategy_id,name,stage,market_regime,payload_json,status,version,created_at,source_analysis_id FROM sentiment_strategies ORDER BY created_at DESC LIMIT ?1").map_err(|e| e.to_string())?; let rows = stmt.query_map([MAX_STRATEGIES as i64], |r| { let payload: Value = serde_json::from_str::<Value>(&r.get::<_,String>(4)?).unwrap_or_else(|_|json!({})); Ok(json!({"strategy_id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"stage":r.get::<_,String>(2)?,"market_regime":r.get::<_,Option<String>>(3)?,"payload":payload,"status":r.get::<_,String>(5)?,"version":r.get::<_,i64>(6)?,"created_at":r.get::<_,i64>(7)?,"source_analysis_id":r.get::<_,Option<String>>(8)?})) }).map_err(|e| e.to_string())?; let items = rows.map(|r| r.map_err(|e| e.to_string())).collect::<Result<Vec<_>,_>>()?; Ok(json!({"items":items})) }
}
fn with_store<T,F>(app:&AppHandle, operation:F)->Result<T,String> where F:FnOnce(&EvolutionStore)->Result<T,String> { let store=EvolutionStore::open(evolution_path(app)?)?; operation(&store) }
fn deterministic_review(payload:&Value)->Value { let question=payload.get("question").and_then(Value::as_str).unwrap_or("").trim(); let answer=payload.get("answer").and_then(Value::as_str).unwrap_or("").trim(); let evidence=payload.get("evidence_ids").and_then(Value::as_array).cloned().unwrap_or_default(); let tools=payload.get("tool_calls").and_then(Value::as_array).cloned().unwrap_or_default(); let mut blind=Vec::new(); if evidence.is_empty(){blind.push(json!({"kind":"evidence_coverage","statement":"本次研究没有可确认的证据引用，不能把结论升级为长期规则。","evidence_refs":[]}));} if !answer.is_empty()&&!answer.contains("失效")&&!answer.contains("不确定"){blind.push(json!({"kind":"invalidation","statement":"本次回答没有明确写出失效条件或不确定性。","evidence_refs":evidence}));} json!({"review_id":agent_ledger::next_run_id(),"run_id":payload.get("run_id").cloned().unwrap_or_else(||json!("")),"conversation_id":payload.get("conversation_id").cloned().unwrap_or_else(||json!(null)),"status":"draft","review_mode":"deterministic","research_goal":question,"evidence_and_process":{"evidence_ids":evidence,"tool_calls":tools},"conclusion_quality":{"answer":answer,"has_answer":!answer.is_empty()},"blind_spot_candidates":blind,"rule_candidates":[],"model_feedback":null}) }

#[tauri::command]
pub(crate) fn api_evolution_settings(app:AppHandle,payload:Option<Value>)->Result<Value,String>{with_store(&app,|store|Ok(json!(match payload.as_ref().filter(|v|v.as_object().is_some_and(|o|!o.is_empty())){Some(v)=>store.update_settings(v)?,None=>store.settings()?})))}
#[tauri::command]
pub(crate) fn api_evolution_profile(app:AppHandle)->Result<Value,String>{with_store(&app,EvolutionStore::profile)}
#[tauri::command]
pub(crate) fn api_evolution_profile_reset(app:AppHandle)->Result<Value,String>{with_store(&app,EvolutionStore::reset_profile)}
#[tauri::command]
pub(crate) fn api_evolution_review(app:AppHandle,payload:Value)->Result<Value,String>{let review=deterministic_review(&payload); let id=review.get("review_id").and_then(Value::as_str).unwrap_or("review").to_string(); with_store(&app,|store|{store.save_review(&id,&review)?;Ok(review)})}
#[tauri::command]
pub(crate) fn api_evolution_confirm_rule(app:AppHandle,payload:Value)->Result<Value,String>{with_store(&app,|store|store.confirm_rule(&payload))}
#[tauri::command]
pub(crate) fn api_evolution_delete_rule(app:AppHandle,payload:Value)->Result<Value,String>{let id=payload.get("rule_id").and_then(Value::as_str).ok_or("rule_id is required")?;with_store(&app,|store|store.delete_rule(id))}
#[tauri::command]
pub(crate) fn api_sentiment_strategy_save(app:AppHandle,payload:Value)->Result<Value,String>{with_store(&app,|store|store.save_strategy(&payload))}
#[tauri::command]
pub(crate) fn api_sentiment_strategies(app:AppHandle)->Result<Value,String>{with_store(&app,EvolutionStore::strategies)}

#[cfg(test)]
mod tests {
 use super::*; use std::time::{SystemTime,UNIX_EPOCH};
 fn path(name:&str)->PathBuf{let stamp=SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();std::env::temp_dir().join(format!("evolution-{name}-{stamp}.sqlite"))}
 #[test] fn defaults_are_disabled_and_local_only(){let p=path("settings");let s=EvolutionStore::open(&p).unwrap();let x=s.settings().unwrap();assert!(!x.enabled);assert!(x.local_only);assert_eq!(x.review_trigger,"explicit");let _=std::fs::remove_file(p);}
 #[test] fn confirmed_rule_requires_evidence_and_increments_profile_version(){let p=path("rules");let s=EvolutionStore::open(&p).unwrap();assert!(s.confirm_rule(&json!({"statement":"always cite validity window","source_review_id":"r1"})).is_err());let profile=s.confirm_rule(&json!({"rule_id":"rule-1","statement":"always cite validity window","source_review_id":"r1","evidence_refs":["doc-1"]})).unwrap();assert_eq!(profile["profile_version"],1);assert_eq!(profile["rules"].as_array().unwrap().len(),1);let _=std::fs::remove_file(p);}
 #[test] fn strategy_save_requires_evidence_backtest_and_bounded_fields(){let p=path("strategy");let s=EvolutionStore::open(&p).unwrap();let base=json!({"strategy_id":"s1","name":"降温策略","stage":"降温","evidence_ids":["E1"],"backtest":{"passed":true},"changes":[{"field":"max_pe","relative_change":-0.2}]});assert!(s.save_strategy(&base).is_ok());let mut bad=base.clone();bad["changes"][0]["field"]=json!("market_scope");assert!(s.save_strategy(&bad).is_err());let _=std::fs::remove_file(p);}
 #[test] fn insufficient_evidence_review_is_explicit(){let review=deterministic_review(&json!({"run_id":"run-1","question":"q","answer":"a","evidence_ids":[],"tool_calls":[]}));assert_eq!(review["review_mode"],"deterministic");assert!(!review["blind_spot_candidates"].as_array().unwrap().is_empty());}
}
