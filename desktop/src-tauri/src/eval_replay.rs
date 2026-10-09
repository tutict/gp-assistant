use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{env, fs, path::{Path, PathBuf}, time::{SystemTime, UNIX_EPOCH}};

use crate::{research::ResearchStore, rig_runtime};
use stock_optimizer_core as gp_core;

const FIXTURE_VERSION: &str = "research-retrieval-eval-v4";
const EMBEDDING_MODEL: &str = "retrieval-replay-v3";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReplayProfile {
    Deterministic,
    ModelBacked,
}

impl ReplayProfile {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "deterministic" | "deterministic_v1" => Ok(Self::Deterministic),
            "model-backed" | "model_backed" => Ok(Self::ModelBacked),
            other => Err(format!("unknown replay profile: {other}")),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Deterministic => "deterministic_v1",
            Self::ModelBacked => "model-backed_v1",
        }
    }
}

fn agent_model_config_from_env() -> Result<Value, String> {
    let required = |name: &str| {
        env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| format!("missing {name}; model-backed replay never falls back to judge configuration"))
    };
    let base_url = required("RAG_EVAL_AGENT_BASE_URL")?;
    let api_key = required("RAG_EVAL_AGENT_API_KEY")?;
    let model = required("RAG_EVAL_AGENT_MODEL")?;
    let prompt_version = required("RAG_EVAL_AGENT_PROMPT_VERSION")?;
    let lowercase_key = api_key.to_ascii_lowercase();
    if ["paste_", "paste-", "your_", "your-", "replace_", "replace-"]
        .iter()
        .any(|prefix| lowercase_key.starts_with(prefix))
    {
        return Err("RAG_EVAL_AGENT_API_KEY is still a placeholder".to_string());
    }
    let temperature = env::var("RAG_EVAL_AGENT_TEMPERATURE")
        .unwrap_or_else(|_| "0".to_string())
        .parse::<f64>()
        .map_err(|_| "RAG_EVAL_AGENT_TEMPERATURE must be numeric".to_string())?;
    if temperature != 0.0 {
        return Err("RAG_EVAL_AGENT_TEMPERATURE must be 0 for reproducible replay".to_string());
    }
    let api_format = env::var("RAG_EVAL_AGENT_API_FORMAT").unwrap_or_else(|_| "openai_chat".to_string());
    let public = json!({
        "base_url": base_url,
        "model": model,
        "api_format": api_format,
        "temperature": temperature,
        "prompt_version": prompt_version,
    });
    let fingerprint = Sha256::digest(serde_json::to_vec(&public).map_err(|error| error.to_string())?)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut config = public;
    config["api_key"] = Value::String(api_key);
    config["agent_model_fingerprint"] = Value::String(fingerprint);
    Ok(config)
}

fn vec_for(label: &str) -> Vec<f32> {
    let mut v = vec![0.0; 512];
    match label { "query" => v[0] = 1.0, "secondary" => { v[0] = 0.8; v[1] = 0.6; }, _ => v[1] = 1.0 }
    v
}

fn temp_db(case_id: &str) -> PathBuf {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    env::temp_dir().join(format!("gp-assistant-replay-{case_id}-{stamp}.sqlite"))
}

fn fixture_documents(case: &Value) -> Vec<Value> {
    let mut docs = case.get("documents").and_then(Value::as_array).cloned().unwrap_or_default();
    for doc in &mut docs {
        let repeat = doc.get("repeat_content").and_then(Value::as_u64).unwrap_or(1).min(100) as usize;
        if repeat > 1 {
            doc["content"] = Value::String(doc.get("content").and_then(Value::as_str).unwrap_or_default().repeat(repeat));
        }
    }
    if let Some(generator) = case.get("generated_documents") {
        let count = generator.get("count").and_then(Value::as_u64).unwrap_or(0).min(100) as usize;
        let prefix = generator.get("document_id_prefix").and_then(Value::as_str).unwrap_or("generated-");
        for index in 0..count {
            let mut doc = json!({
                "document_id": format!("{prefix}{index}"),
                "title": generator.get("title").cloned().unwrap_or_else(|| json!("Generated evidence")),
                "content": generator.get("content").cloned().unwrap_or_else(|| json!("generated evidence")),
                "source_tier": generator.get("source_tier").cloned().unwrap_or_else(|| json!("news")),
                "stock_codes": generator.get("stock_codes").cloned().unwrap_or_else(|| json!([])),
            });
            if let Some(published) = generator.get("published_at") { doc["published_at"] = published.clone(); }
            docs.push(doc);
        }
    }
    docs
}

fn query_vector(case: &Value) -> Option<Vec<f32>> { case.get("query_vector").and_then(Value::as_str).map(vec_for) }

fn seed_embeddings(store: &ResearchStore, docs: &[Value]) -> Result<(), String> {
    let work = store.pending_embedding_chunks(50_000)?;
    let items = work.into_iter().map(|item| {
        let label = docs.iter().find(|doc| doc.get("document_id").and_then(Value::as_str).is_some_and(|id| item.chunk_id.starts_with(&format!("{id}:")))).and_then(|doc| doc.get("embedding")).and_then(Value::as_str).unwrap_or("orthogonal");
        (item, vec_for(label))
    }).collect::<Vec<_>>();
    if !items.is_empty() { store.store_embeddings(&items, EMBEDDING_MODEL)?; }
    Ok(())
}

fn citations(response: &Value) -> (Vec<String>, Vec<String>) {
    let rows = response.get("citations").and_then(Value::as_array).cloned().unwrap_or_default();
    let contexts = rows.iter().filter_map(|row| row.get("excerpt").and_then(Value::as_str)).map(ToOwned::to_owned).collect();
    let ids = rows.iter().filter_map(|row| row.get("document_id").and_then(Value::as_str)).map(ToOwned::to_owned).collect();
    (contexts, ids)
}

fn replay_agent(
    case: &Value,
    response: &Value,
    profile: ReplayProfile,
    agent_config: Option<&Value>,
) -> Result<(Vec<Value>, Value, Vec<Value>), String> {
    let data = serde_json::to_value(gp_core::CoreDataSet::default()).map_err(|e| e.to_string())?;
    // Expected answers/traces never enter the runtime. Only input and actual evidence do.
    let mut payload = json!({
        "run_id": format!("replay-{}", case["id"].as_str().unwrap_or("case")),
        "mode": if profile == ReplayProfile::ModelBacked { "model" } else { "deterministic_v1" },
        "message": case["question"].as_str().unwrap_or(""),
        "context": {"stock_code": case["request"]["stock_code"]},
        "research_evidence": response,
    });
    if let Some(config) = agent_config {
        payload["llm"] = config.clone();
    }
    let mut events = Vec::new();
    let outcome = tauri::async_runtime::block_on(rig_runtime::execute_with_event_sink(
        payload, data, |event| events.push(event),
    ))?;
    let trace = events.iter().filter(|e| e["type"] == "tool_start").map(|event| {
        let id = event["payload"]["id"].as_str().unwrap_or("");
        let status = events.iter().find(|e| e["type"] == "tool_result" && e["payload"]["tool_call_id"] == id)
            .and_then(|e| e["payload"]["status"].as_str()).unwrap_or("missing_result");
        json!({"name": event["payload"]["tool"], "arguments": event["payload"]["input"], "status": status})
    }).collect();
    Ok((trace, outcome.response, events))
}

fn replay_case(
    case: &Value,
    profile: ReplayProfile,
    agent_config: Option<&Value>,
) -> Result<Value, String> {
    let id = case.get("id").and_then(Value::as_str).ok_or_else(|| "case id missing".to_string())?;
    let db = temp_db(id);
    let store = ResearchStore::open(&db)?;
    let docs = fixture_documents(case);
    store.ingest_documents(&docs)?;
    let private = case["category"] == "private_evidence"
        || docs.iter().any(|doc| doc.get("user_imported").and_then(Value::as_bool).unwrap_or(false));
    let qv = query_vector(case);
    if qv.is_some() { seed_embeddings(&store, &docs)?; }
    let request = case.get("request").cloned().unwrap_or_else(|| json!({}));
    let mut retrieval_request = request.clone();
    if profile == ReplayProfile::ModelBacked {
        retrieval_request["remote_safe_only"] = Value::Bool(true);
    }
    let response = store.query_with_vector(&retrieval_request, qv.as_deref())?;
    let (remote_contexts, remote_ids) = citations(&response);

    let local_request = {
        let mut value = request.clone();
        value["remote_safe_only"] = Value::Bool(false);
        value
    };
    let local_response = store.query_with_vector(&local_request, qv.as_deref())?;
    let (local_contexts, local_ids) = citations(&local_response);
    let should_call_model = profile == ReplayProfile::ModelBacked && !private;
    let (trace, agent_response, agent_events) = replay_agent(
        case,
        &response,
        if should_call_model { ReplayProfile::ModelBacked } else { ReplayProfile::Deterministic },
        if should_call_model { agent_config } else { None },
    )?;
    let expect = case.get("expect").ok_or_else(|| format!("{id}: expect missing"))?;
    let contract = expect.get("answer_contract").cloned().unwrap_or_else(|| json!({}));
    let tools = expect
        .get("agent_contract")
        .and_then(|a| a.get("expected_trace").or_else(|| a.get("required_tools")))
        .cloned()
        .unwrap_or_else(|| json!([]));
    let actual_tool_trace = trace
        .iter()
        .map(|item| json!({"name": item["name"], "arguments": item["arguments"]}))
        .collect::<Vec<_>>();
    let failed_tool_statuses = trace
        .iter()
        .filter(|item| !matches!(item["status"].as_str(), Some("ok" | "degraded")))
        .cloned()
        .collect::<Vec<_>>();
    let expected_tool_trace = tools.as_array().cloned().unwrap_or_default();
    let forbidden_tools = expect
        .get("agent_contract")
        .and_then(|a| a.get("forbidden_tools"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let forbidden_tool_hits = actual_tool_trace
        .iter()
        .filter_map(|item| item.get("name").and_then(Value::as_str))
        .filter(|name| forbidden_tools.iter().any(|item| item.as_str() == Some(*name)))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let required_document_ids = expect
        .get("required_document_ids")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let forbidden_document_ids = expect
        .get("forbidden_document_ids")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let missing_required_document_ids = required_document_ids
        .iter()
        .filter_map(Value::as_str)
        .filter(|document_id| !local_ids.iter().any(|actual| actual == document_id))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let forbidden_document_ids_retrieved = forbidden_document_ids
        .iter()
        .filter_map(Value::as_str)
        .filter(|document_id| local_ids.iter().any(|actual| actual == document_id))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let expected_evidence = expect
        .get("required_evidence_sentences")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let missing_required_evidence = expected_evidence
        .iter()
        .filter_map(|item| item.get("sentence").and_then(Value::as_str))
        .filter(|sentence| !local_contexts.iter().any(|context| context.contains(sentence)))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let remote_forbidden_document_ids = docs
        .iter()
        .filter(|doc| {
            doc.get("user_imported").and_then(Value::as_bool).unwrap_or(false)
                && !doc.get("remote_export_allowed").and_then(Value::as_bool).unwrap_or(false)
        })
        .filter_map(|doc| doc.get("document_id").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let remote_forbidden_hits = remote_forbidden_document_ids
        .iter()
        .filter(|document_id| remote_ids.iter().any(|actual| actual == *document_id))
        .cloned()
        .collect::<Vec<_>>();
    let privacy_ok = remote_forbidden_hits.is_empty();
    let retrieval_passed = missing_required_document_ids.is_empty()
        && forbidden_document_ids_retrieved.is_empty()
        && missing_required_evidence.is_empty()
        && privacy_ok;
    let trace_passed = actual_tool_trace == expected_tool_trace
        && forbidden_tool_hits.is_empty()
        && failed_tool_statuses.is_empty();
    let generative_model_used = agent_response
        .get("harness")
        .and_then(|harness| harness.get("model_used"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let generation_eval_eligible = should_call_model && generative_model_used;
    let agent_model_fingerprint = agent_config
        .and_then(|config| config.get("agent_model_fingerprint"))
        .cloned()
        .unwrap_or(Value::Null);
    let actual_output = agent_response.get("reply").and_then(Value::as_str).unwrap_or_default();
    let expected_retrieval_context = expected_evidence
        .iter()
        .filter_map(|x| x.get("sentence").and_then(Value::as_str).map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    let retrieval_gate = json!({
        "passed": retrieval_passed,
        "missing_required_document_ids": missing_required_document_ids,
        "forbidden_document_ids_retrieved": forbidden_document_ids_retrieved,
        "missing_required_evidence": missing_required_evidence,
        "privacy_ok": privacy_ok,
    });
    let trace_gate = json!({
        "passed": trace_passed,
        "tool_sequence_match": actual_tool_trace == expected_tool_trace,
        "forbidden_tools_called": forbidden_tool_hits,
        "failed_tool_statuses": failed_tool_statuses,
    });
    let constructs = json!({
        "retrieval": {"passed": retrieval_passed},
        "trajectory": {"passed": trace_passed},
        "generation": {
            "status": if generation_eval_eligible { "eligible" } else { "not_applicable" },
            "reason": if generation_eval_eligible { Value::Null } else { Value::String("deterministic replay has no product-model answer".to_string()) },
        },
    });
    let replay = json!({
        "profile": profile.label(),
        "generative_model_used": generative_model_used,
        "agent_model_fingerprint": agent_model_fingerprint,
        "trace_origin": "runtime-events-not-oracle",
        "retrieval_query_mode": "fixture-token",
        "embedding_backend": if qv.is_some() { "synthetic-test-vector" } else { "bm25-only" },
        "retrieval_mode": response.get("retrieval_mode"),
        "response": response,
        "agent_response": agent_response,
        "agent_events": agent_events,
    });
    let output = json!({
        "id": id, "dataset_version": FIXTURE_VERSION, "category": case.get("category"),
        "privacy_reviewed": true, "private_evidence_present": private, "remote_export_allowed": !private, "cloud_eval_allowed": !private,
        "question": case["question"], "request": request,
        "actual_output": actual_output,
        "agent_model_output": if generation_eval_eligible { Value::String(actual_output.to_string()) } else { Value::Null },
        "generation_eval_eligible": generation_eval_eligible,
        "answer_source": if generation_eval_eligible { "model" } else { "deterministic_fallback" },
        "retrieval_answer": response.get("answer").and_then(Value::as_str).unwrap_or_default(),
        "expected_output": case["reference_answer"],
        "agent_actual_output": actual_output,
        "answer_contract": contract,
        "answer_facts": expect.get("answer_facts").cloned().unwrap_or_else(|| json!([])),
        "required_citations": expect.get("required_citations").cloned().unwrap_or_else(|| json!([])),
        "forbidden_claims": expect.get("answer_contract").and_then(|value| value.get("forbidden_claims")).cloned().unwrap_or_else(|| json!([])),
        "context": remote_contexts, "retrieval_context": remote_contexts,
        "deterministic_retrieval_context": local_contexts,
        "expected_retrieval_evidence": expected_evidence,
        "expected_retrieval_context": expected_retrieval_context,
        "retrieved_document_ids": local_ids,
        "remote_retrieved_document_ids": remote_ids,
        "remote_forbidden_document_ids": remote_forbidden_document_ids,
        "required_document_ids": required_document_ids, "forbidden_document_ids": forbidden_document_ids,
        "agent_trace": {"tools": trace}, "expected_tools": tools, "forbidden_tools": forbidden_tools,
        "retrieval_gate": retrieval_gate,
        "trace_gate": trace_gate,
        "constructs": constructs,
        "passed": retrieval_passed && trace_passed,
        "replay": replay,
        "provenance": case.get("provenance"),
    });
    drop(store);
    for suffix in ["", "-wal", "-shm"] {
        let _ = fs::remove_file(PathBuf::from(format!("{}{suffix}", db.display())));
    }
    Ok(output)
}

pub(crate) fn run(
    input: &Path,
    output: &Path,
    profile: ReplayProfile,
    agent_config: Option<&Value>,
) -> Result<Value, String> {
    let allowed = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../app/prompts/research_retrieval_eval_cases.json");
    if input.canonicalize().map_err(|e| e.to_string())? != allowed.canonicalize().map_err(|e| e.to_string())? {
        return Err("Only the checked-in frozen fixture is allowed".to_string());
    }
    let fixture: Value = serde_json::from_str(&fs::read_to_string(input).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    if fixture.get("version").and_then(Value::as_str) != Some(FIXTURE_VERSION) { return Err(format!("fixture must be {FIXTURE_VERSION}")); }
    let cases = fixture.get("cases").and_then(Value::as_array).ok_or_else(|| "cases missing".to_string())?;
    if profile == ReplayProfile::ModelBacked && agent_config.is_none() {
        return Err("model-backed replay requires explicit RAG_EVAL_AGENT_* configuration".to_string());
    }
    let rows = cases
        .iter()
        .map(|case| replay_case(case, profile, agent_config))
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(parent) = output.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
    let text = rows.iter().map(Value::to_string).collect::<Vec<_>>().join("\n") + "\n";
    fs::write(output, &text).map_err(|e| e.to_string())?;
    Ok(json!({"dataset_version": FIXTURE_VERSION, "case_count": rows.len(), "cloud_eval_case_count": rows.iter().filter(|row| row.get("cloud_eval_allowed") == Some(&Value::Bool(true))).count(), "output_path": output, "bytes": text.len()}))
}

pub fn cli(args: impl Iterator<Item = String>) -> Result<(), String> {
    let args = args.skip(1).collect::<Vec<_>>();
    if args.len() < 2 {
        return Err("usage: rag-replay <fixture> <output> [--profile deterministic|model-backed]".to_string());
    }
    let input = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    let mut profile = ReplayProfile::Deterministic;
    let mut index = 2;
    while index < args.len() {
        match args[index].as_str() {
            "--profile" => {
                index += 1;
                let value = args.get(index).ok_or_else(|| "--profile requires a value".to_string())?;
                profile = ReplayProfile::parse(value)?;
            }
            other => return Err(format!("unknown argument: {other}")),
        }
        index += 1;
    }
    let agent_config = if profile == ReplayProfile::ModelBacked {
        Some(agent_model_config_from_env()?)
    } else {
        None
    };
    println!("{}", run(&input, &output, profile, agent_config.as_ref())?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_agent_routing_does_not_use_expected_tools_as_an_execution_plan() {
        let case = json!({
            "id": "oracle-independence",
            "question": "Use news to verify the disclosed equipment purchase.",
            "documents": [{"document_id":"notice","title":"Equipment filing","content":"equipmentprobe: Purchased equipment for RMB 12 million.","source_tier":"filing"}],
            "request": {"query":"equipmentprobe"},
            "expect": {
                "required_document_ids":["notice"],
                "required_evidence_sentences":[{"document_id":"notice","sentence":"Purchased equipment for RMB 12 million."}],
                "answer_contract":{"acceptable_answers":["Purchase confirmed."]},
                "agent_contract":{"required_tools":[{"name":"portfolio_backtest","arguments":{"request":{}}}]}
            }
        });
        let result = replay_case(&case, ReplayProfile::Deterministic, None).expect("offline replay should execute");
        assert_eq!(result["agent_trace"]["tools"][0]["name"], "news_evidence");
        assert_eq!(result["agent_trace"]["tools"][0]["arguments"]["query"], case["question"]);
    }
}
