use crate::{agent_harness, research, rig_runtime, runtime, sentiment_agent, sentiment_data};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{Mutex, OnceLock},
};
use tauri::{AppHandle, Manager};

static RUNS: OnceLock<Mutex<HashMap<String, Value>>> = OnceLock::new();
fn runs() -> &'static Mutex<HashMap<String, Value>> {
    RUNS.get_or_init(|| Mutex::new(HashMap::new()))
}
fn now() -> i64 {
    crate::epoch_millis() as i64
}
fn id() -> String {
    crate::agent_ledger::next_run_id()
}
fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn field<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .ok_or_else(|| format!("缺少有效的 {key}"))
}
fn code(v: &Value) -> Result<String, String> {
    crate::normalize_stock_code(field(v, "stock_code")?).ok_or_else(|| "股票代码无效".into())
}

fn open(path: &Path) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let connection = Connection::open(path).map_err(|e| e.to_string())?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    connection.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE IF NOT EXISTS analyses(id TEXT PRIMARY KEY,stock_code TEXT NOT NULL,generation TEXT NOT NULL,created_at INTEGER NOT NULL,result_json TEXT NOT NULL); CREATE INDEX IF NOT EXISTS sentiment_stock_time ON analyses(stock_code,created_at DESC); CREATE TABLE IF NOT EXISTS followups(id TEXT PRIMARY KEY,analysis_id TEXT NOT NULL REFERENCES analyses(id) ON DELETE CASCADE,question TEXT NOT NULL,result_json TEXT NOT NULL,created_at INTEGER NOT NULL);").map_err(|e|e.to_string())?;
    Ok(connection)
}
fn connection(app: &AppHandle) -> Result<Connection, String> {
    open(
        &app.path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("research/sentiment.sqlite"),
    )
}
fn generation(app: &AppHandle) -> Result<String, String> {
    research::with_app_store(app, |store| store.sentiment_generation())
}

async fn freeze(app: AppHandle, payload: Value) -> Result<Value, String> {
    let stock = code(&payload)?;
    let window = payload
        .get("window_days")
        .and_then(Value::as_u64)
        .unwrap_or(30);
    if window != 30 {
        return Err("当前情绪分析窗口为30个自然日".into());
    }
    runtime::run_io_bound("sentiment_snapshot", move || {
        let cutoff = now();
        let data = crate::cached_market_data(&app)?;
        research::with_app_store(&app, |store| {
            sentiment_data::build_snapshot(
                &stock,
                30,
                cutoff,
                &store.sentiment_generation()?,
                store.sentiment_documents(&stock, cutoff)?,
                data,
                payload.get("industry").and_then(Value::as_str),
            )
        })
    })
    .await?
}

#[tauri::command]
pub(crate) async fn api_sentiment_snapshot(
    app: AppHandle,
    payload: Value,
) -> Result<Value, String> {
    freeze(app, payload).await
}

#[tauri::command]
pub(crate) async fn api_sentiment_start(app: AppHandle, payload: Value) -> Result<Value, String> {
    let stock = code(&payload)?;
    let llm = payload.get("llm").ok_or("请先配置 API 模型，再分析情绪")?;
    let config = rig_runtime::normalize_provider_config(llm)?;
    rig_runtime::validate_provider_config(&config)?;
    let request_key = sha256_hex(&serde_json::to_vec(&json!({"stock":stock,"window":payload.get("window_days"),"industry":payload.get("industry"),"llm":llm})).map_err(|e|e.to_string())?);
    let run_id = id();
    {
        let mut state = runs().lock().map_err(|_| "任务锁不可用")?;
        if let Some((key, _)) = state
            .iter()
            .find(|(_, v)| v["request_key"] == request_key && v["status"] == "running")
        {
            return Ok(json!({"run_id":key}));
        }
        if state.values().filter(|v| v["status"] == "running").count() >= 4 {
            return Err("同时分析任务已达上限，请等待当前任务完成".into());
        }
        if state.len() > 100 {
            state.retain(|_, v| v["status"] == "running");
        }
        state.insert(run_id.clone(),json!({"run_id":run_id,"stock_code":stock,"status":"running","stage":"冻结证据与行情快照","progress":5,"request_key":request_key}));
    }
    let cancellation = rig_runtime::register_run(&run_id);
    let task_id = run_id.clone();
    tauri::async_runtime::spawn(async move {
        let outcome=async {
            let snapshot=freeze(app.clone(),payload.clone()).await?;
            let mut analysis=sentiment_agent::analyze(payload.clone(),snapshot.clone(),cancellation.clone(),|event| {
                if let Ok(mut state)=runs().lock() {if let Some(run)=state.get_mut(&task_id) {if run["status"]=="running" {run["stage"]=event.get("stage").cloned().unwrap_or(json!("分析中"));run["progress"]=event.get("progress").cloned().unwrap_or(json!(50));}}}
            }).await?;
            if cancellation.is_cancelled() {return Err("已取消分析".to_string());}
            analysis["analysis_id"]=json!(task_id); analysis["run_id"]=json!(task_id);
            analysis["snapshot_id"]=snapshot["snapshot_id"].clone(); analysis["stock_code"]=json!(stock);
            analysis["created_at"]=json!(now()); analysis["model"]=json!(config.model);
            analysis["rule_version"]=snapshot["rule_version"].clone();analysis["snapshot"]=snapshot;
            analysis=agent_harness::redact_persisted_response(&analysis,payload.get("llm"));
            let save_app=app.clone(); let save_id=task_id.clone();let save_cancel=cancellation.clone();
            runtime::run_io_bound("sentiment_save",move || {
                let mut state=runs().lock().map_err(|_|"任务锁不可用")?;
                if save_cancel.is_cancelled() || state.get(&save_id).is_some_and(|v|v["status"]=="cancelled") {return Err("已取消分析".into());}
                analysis["stale"]=json!(generation(&save_app)?!=analysis["snapshot"]["generation"].as_str().unwrap_or(""));
                connection(&save_app)?.execute("INSERT INTO analyses(id,stock_code,generation,created_at,result_json) VALUES(?1,?2,?3,?4,?5)",params![save_id,stock,analysis["snapshot"]["generation"].as_str().unwrap_or(""),now(),analysis.to_string()]).map_err(|e|e.to_string())?;
                if let Some(run)=state.get_mut(&save_id) {run["status"]=json!("completed");run["stage"]=json!("分析完成");run["progress"]=json!(100);run["result"]=analysis;}
                Ok::<(),String>(())
            }).await??;
            Ok::<(),String>(())
        }.await;
        if let Err(error) = outcome {
            if let Ok(mut state) = runs().lock() {
                if let Some(run) = state.get_mut(&task_id) {
                    run["status"] = json!(if cancellation.is_cancelled() {
                        "cancelled"
                    } else {
                        "failed"
                    });
                    run["error"] = json!(agent_harness::redact_persisted_error(
                        &error,
                        payload.get("llm")
                    ));
                }
            }
        }
        rig_runtime::unregister_run(&task_id);
    });
    Ok(json!({"run_id":run_id}))
}

#[tauri::command]
pub(crate) fn api_sentiment_status(payload: Value) -> Result<Value, String> {
    let state = runs().lock().map_err(|_| "任务锁不可用")?;
    let mut value = state
        .get(field(&payload, "run_id")?)
        .cloned()
        .ok_or("分析任务不存在或应用已重启，请查看分析历史")?;
    value.as_object_mut().map(|v| v.remove("request_key"));
    Ok(value)
}
#[tauri::command]
pub(crate) fn api_sentiment_cancel(payload: Value) -> Result<Value, String> {
    let key = field(&payload, "run_id")?;
    let mut state = runs().lock().map_err(|_| "任务锁不可用")?;
    let cancelled = if let Some(run) = state.get_mut(key) {
        if run["status"] == "running" {
            run["status"] = json!("cancelled");
            rig_runtime::request_cancel(key);
            true
        } else {
            false
        }
    } else {
        false
    };
    Ok(json!({"cancelled":cancelled}))
}
fn mark_stale(current: &str, value: &mut Value) {
    let stored = value
        .get("snapshot")
        .and_then(|snapshot| snapshot.get("generation"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "stale".to_string(),
            json!(stored.as_deref() != Some(current)),
        );
    }
}
fn load_history(app: &AppHandle, stock: Option<&str>) -> Result<Vec<Value>, String> {
    let current = generation(app)?;
    let db = connection(app)?;
    let mut query = db
        .prepare("SELECT result_json FROM analyses WHERE (?1 IS NULL OR stock_code=?1) ORDER BY created_at DESC LIMIT 30")
        .map_err(|e| e.to_string())?;
    let rows = query
        .query_map([stock], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    rows.map(|row| {
        let mut value: Value =
            serde_json::from_str(&row.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        mark_stale(&current, &mut value);
        Ok(value)
    })
    .collect()
}
fn code_list(payload: &Value) -> Result<Vec<String>, String> {
    let list = payload
        .get("stock_codes")
        .and_then(Value::as_array)
        .ok_or("缺少股票代码")?;
    if list.len() > 200 {
        return Err("一次最多查询 200 只股票的阶段".into());
    }
    let mut codes = Vec::new();
    for item in list {
        let Some(text) = item.as_str() else { continue };
        let Some(code) = crate::normalize_stock_code(text) else {
            continue;
        };
        if !codes.contains(&code) {
            codes.push(code);
        }
    }
    Ok(codes)
}
fn load_latest_for_codes(app: &AppHandle, codes: &[String]) -> Result<Vec<Value>, String> {
    if codes.is_empty() {
        return Ok(Vec::new());
    }
    let current = generation(app)?;
    let db = connection(app)?;
    let placeholders = vec!["?"; codes.len()].join(",");
    let sql = format!(
        "SELECT result_json FROM (SELECT result_json, ROW_NUMBER() OVER (PARTITION BY stock_code ORDER BY created_at DESC) AS rn FROM analyses WHERE stock_code IN ({placeholders})) WHERE rn = 1"
    );
    let mut query = db.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = query
        .query_map(params_from_iter(codes.iter()), |row| {
            row.get::<_, String>(0)
        })
        .map_err(|e| e.to_string())?;
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for row in rows {
        let mut value: Value =
            serde_json::from_str(&row.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let code = value
            .get("stock_code")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if code.is_empty() || !seen.insert(code) {
            continue;
        }
        mark_stale(&current, &mut value);
        items.push(value);
    }
    Ok(items)
}
#[tauri::command]
pub(crate) async fn api_sentiment_latest(app: AppHandle, payload: Value) -> Result<Value, String> {
    let stock = code(&payload)?;
    runtime::run_io_bound("sentiment_latest", move || {
        Ok(json!({"analysis":load_history(&app,Some(&stock))?.into_iter().next()}))
    })
    .await?
}
#[tauri::command]
pub(crate) async fn api_sentiment_history(app: AppHandle, payload: Value) -> Result<Value, String> {
    if payload.get("stock_codes").is_some() {
        let codes = code_list(&payload)?;
        return runtime::run_io_bound("sentiment_history_stages", move || {
            Ok(json!({"items": load_latest_for_codes(&app, &codes)?}))
        })
        .await?;
    }
    let stock = if payload.get("stock_code").is_some() {
        Some(code(&payload)?)
    } else {
        None
    };
    runtime::run_io_bound("sentiment_history", move || {
        Ok(json!({"items":load_history(&app,stock.as_deref())?}))
    })
    .await?
}
#[tauri::command]
pub(crate) async fn api_sentiment_followup(
    app: AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let analysis_id = field(&payload, "analysis_id")?.to_owned();
    let question = payload
        .get("question")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.chars().count() <= 8000)
        .ok_or("问题不能为空或超过8000字")?
        .to_owned();
    let load_app = app.clone();
    let load_id = analysis_id.clone();
    let snapshot = runtime::run_io_bound("sentiment_followup_load", move || {
        let text: Option<String> = connection(&load_app)?
            .query_row(
                "SELECT result_json FROM analyses WHERE id=?1",
                [load_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let value: Value =
            serde_json::from_str(&text.ok_or("分析记录不存在")?).map_err(|e| e.to_string())?;
        Ok::<Value, String>(value["snapshot"].clone())
    })
    .await??;
    let run_id = id();
    let cancel = rig_runtime::register_run(&run_id);
    let output = sentiment_agent::followup(payload.clone(), snapshot, cancel).await;
    rig_runtime::unregister_run(&run_id);
    let mut result = agent_harness::redact_persisted_response(&output?, payload.get("llm"));
    result["analysis_id"] = json!(analysis_id);
    result["created_at"] = json!(now());
    let safe_question = agent_harness::redact_persisted_question(&question, payload.get("llm"));
    let saved = result.clone();
    runtime::run_io_bound("sentiment_followup_save",move || {
        connection(&app)?.execute("INSERT INTO followups(id,analysis_id,question,result_json,created_at) VALUES(?1,?2,?3,?4,?5)",params![run_id,analysis_id,safe_question,saved.to_string(),now()]).map_err(|e|e.to_string())?;Ok::<(),String>(())
    }).await??;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn storage_is_separate_and_roundtrips_immutable_snapshot() {
        let path = std::env::temp_dir().join(format!("sentiment-test-{}.sqlite", id()));
        {
            let db = open(&path).unwrap();
            let result = json!({"snapshot":{"generation":"old","evidence":[{"id":"E1","excerpt":"original"}]}});
            db.execute(
                "INSERT INTO analyses VALUES('a','000001.SZ','old',1,?1)",
                [result.to_string()],
            )
            .unwrap();
            let stored: String = db
                .query_row("SELECT result_json FROM analyses WHERE id='a'", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(serde_json::from_str::<Value>(&stored).unwrap(), result);
        }
        std::fs::remove_file(path).unwrap();
    }
}
