use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
#[cfg(all(
    feature = "gepa-lab",
    target_os = "windows",
    not(mobile),
    debug_assertions
))]
use std::env;
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};
use tauri::{AppHandle, Emitter, Manager};
#[cfg(all(
    feature = "gepa-lab",
    target_os = "windows",
    not(mobile),
    debug_assertions
))]
use tokio::time::{sleep, Duration};

use crate::{agent_harness, prompt_upgrade, rig_runtime};

const EVAL_SUITE: &str = include_str!("../../../app/prompts/agent_gepa_eval_cases.json");
const ENGINE_VERSION: &str = "dsrust-gepa-0.1.0-alpha.3";
const SEED: u64 = 42;
const EVENT_NAME: &str = "agent-gepa-event";

#[derive(Debug, Deserialize, Clone)]
struct EvalSuiteFile {
    version: String,
    profiles: BTreeMap<String, EvalProfile>,
}
#[derive(Debug, Deserialize, Clone)]
struct EvalProfile {
    #[serde(skip)]
    profile_id: String,
    mode: String,
    tool_result: Value,
    train: Vec<EvalCase>,
    validation: Vec<EvalCase>,
    holdout: Vec<EvalCase>,
}
#[derive(Debug, Deserialize, Clone)]
struct EvalCase {
    id: String,
    question: String,
    rubric: Vec<RubricCriterion>,
}
#[derive(Debug, Deserialize, Clone)]
struct RubricCriterion {
    id: String,
    any_of: Vec<String>,
}
#[derive(Debug, Serialize, Deserialize, Clone)]
struct CaseSummary {
    id: String,
    score: f64,
    hard_failure: Option<String>,
    feedback: Vec<String>,
    response: Value,
    #[serde(default)]
    trajectory: Vec<Value>,
}
#[derive(Debug)]
struct CaseEvaluation {
    score: f64,
    feedback: Vec<String>,
    hard_failure: Option<String>,
    merged_response: Value,
}
#[derive(Debug, Serialize, Deserialize, Clone)]
struct GepaRunReport {
    run_id: String,
    status: String,
    profile_id: String,
    base_prompt_version: String,
    candidate_prompt_version: Option<String>,
    eval_suite_version: String,
    #[serde(default = "default_replay_profile")]
    replay_profile: String,
    dataset_sha256: String,
    engine_version: String,
    seed: u64,
    max_metric_calls: usize,
    baseline_validation_score: Option<f64>,
    baseline_holdout_score: Option<f64>,
    candidate_validation_score: Option<f64>,
    candidate_holdout_score: Option<f64>,
    baseline_validation: Vec<CaseSummary>,
    baseline_holdout: Vec<CaseSummary>,
    candidate_validation: Vec<CaseSummary>,
    candidate_holdout: Vec<CaseSummary>,
    candidate_body: Option<String>,
    #[serde(default)]
    apply_gate: Value,
    error: Option<String>,
}

fn default_replay_profile() -> String {
    "legacy_gepa_runtime".to_string()
}

static REPORT_ROOT: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
fn reports_root(app: &AppHandle) -> Result<PathBuf, String> {
    let mut root = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve GEPA report directory: {e}"))?;
    root.push("agent");
    root.push("gepa-runs");
    fs::create_dir_all(&root)
        .map_err(|e| format!("failed to create GEPA report directory: {e}"))?;
    if let Ok(mut cached) = REPORT_ROOT.get_or_init(|| Mutex::new(None)).lock() {
        *cached = Some(root.clone());
    }
    Ok(root)
}
fn report_path(app: &AppHandle, run_id: &str) -> Result<PathBuf, String> {
    let mut p = reports_root(app)?;
    validate_run_id(run_id)?;
    p.push(format!("{run_id}.json"));
    Ok(p)
}
fn save_report(app: &AppHandle, report: &GepaRunReport) -> Result<(), String> {
    let path = report_path(app, &report.run_id)?;
    let text = serde_json::to_string_pretty(report)
        .map_err(|e| format!("encode GEPA report failed: {e}"))?;
    crate::durability::atomic_write_json(&path, text.as_bytes())
}
fn load_report(app: &AppHandle, run_id: &str) -> Result<GepaRunReport, String> {
    let text = fs::read_to_string(report_path(app, run_id)?)
        .map_err(|e| format!("GEPA report not found: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("GEPA report is invalid: {e}"))
}
fn validate_run_id(run_id: &str) -> Result<(), String> {
    if run_id.is_empty()
        || run_id.len() > 128
        || !run_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err("invalid GEPA run_id".to_string());
    }
    Ok(())
}

fn emit(app: &AppHandle, run_id: &str, event_type: &str, payload: Value) {
    let mut event = json!({"run_id": run_id, "type": event_type});
    if let Some(object) = event.as_object_mut() {
        if let Some(extra) = payload.as_object() {
            object.extend(extra.clone());
        }
    }
    let _ = app.emit(EVENT_NAME, event);
}
fn parse_suite() -> Result<EvalSuiteFile, String> {
    let mut suite: EvalSuiteFile = serde_json::from_str(EVAL_SUITE)
        .map_err(|e| format!("GEPA evaluation fixture is invalid JSON: {e}"))?;
    if suite.version != "agent-gepa-eval-v1" {
        return Err(format!(
            "unsupported GEPA evaluation fixture version: {}",
            suite.version
        ));
    }
    for (profile_id, mode) in [
        ("hot_money_early_v1", "expert"),
        ("value_compounder_v1", "research"),
    ] {
        let profile = suite
            .profiles
            .get_mut(profile_id)
            .ok_or_else(|| format!("missing GEPA profile {profile_id}"))?;
        if profile.mode != mode
            || profile.train.len() != 8
            || profile.validation.len() != 4
            || profile.holdout.len() != 4
        {
            return Err(format!(
                "GEPA profile {profile_id} does not satisfy the fixed fixture contract"
            ));
        }
        profile.profile_id = profile_id.to_string();
    }
    Ok(suite)
}
fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn dataset_sha256() -> String {
    let mut h = Sha256::new();
    h.update(EVAL_SUITE.as_bytes());
    hex_digest(h.finalize().as_slice())
}

fn build_gepa_replay_payload(
    profile: &EvalProfile,
    case: &EvalCase,
    method_card: &str,
    llm: &Value,
    run_id: &str,
) -> Value {
    let citations = profile.tool_result["evidence_summary"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(index, item)| {
            let citation_id = format!("E{}", index + 1);
            json!({
                "citation_id": citation_id,
                "document_id": format!("{}-synthetic-{}", profile.profile_id, index + 1),
                "chunk_id": format!("{}-synthetic-{}:0", profile.profile_id, index + 1),
                "title": item.get("title"),
                "excerpt": item.get("summary").and_then(Value::as_str).unwrap_or_default(),
                "source_tier": "research_report",
                "source_name": item.get("source").and_then(Value::as_str).unwrap_or("GEPA synthetic fixture"),
                "published_at": profile.tool_result["data"]["as_of"],
                "remote_export_allowed": true,
            })
        })
        .collect::<Vec<_>>();
    let answer = citations
        .iter()
        .map(|citation| {
            format!(
                "[{}] {}",
                citation["citation_id"].as_str().unwrap_or("E"),
                citation["excerpt"].as_str().unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    json!({
        "run_id": run_id,
        "mode": profile.mode,
        "message": case.question,
        "context": {},
        "research_evidence": {
            "mode": "evidence_only",
            "query": case.question,
            "answer": answer,
            "citations": citations,
            "community_only": false,
            "fact_supported": true,
            "remote_safe_only": true,
        },
        "method_card": method_card,
        "llm": llm,
    })
}

fn replay_tool_trajectory(events: &[Value]) -> Vec<Value> {
    events
        .iter()
        .filter(|event| event["type"] == "tool_start")
        .enumerate()
        .map(|(index, event)| {
            let payload = &event["payload"];
            let call_id = payload["id"].as_str().unwrap_or_default();
            let tool = payload["tool"].as_str().unwrap_or("unknown");
            let arguments = payload.get("input").cloned().unwrap_or(Value::Null);
            let arguments_bytes = serde_json::to_vec(&arguments).unwrap_or_default();
            let arguments_hash = hex_digest(Sha256::digest(arguments_bytes).as_slice());
            let result = events.iter().find(|candidate| {
                candidate["type"] == "tool_result"
                    && candidate["payload"]["tool_call_id"].as_str() == Some(call_id)
            });
            let evidence_document_ids = result
                .and_then(|event| event["payload"]["output"]["citations"].as_array())
                .into_iter()
                .flatten()
                .filter_map(|citation| citation["document_id"].as_str().map(ToOwned::to_owned))
                .collect::<Vec<_>>();
            json!({
                "tool_call_id": call_id,
                "tool_name": tool,
                "normalized_arguments": arguments,
                "arguments_hash": arguments_hash,
                "sequence_index": index + 1,
                "status": result.and_then(|event| event["payload"]["status"].as_str()).unwrap_or("missing_result"),
                "side_effect_class": if matches!(tool, "stock_screen" | "stock_observe" | "trend_screen" | "portfolio_backtest" | "news_evidence" | "watchlist_review") { "read_only" } else { "unknown" },
                "evidence_document_ids": evidence_document_ids,
            })
        })
        .collect()
}

fn validate_gepa_trajectory(trajectory: &[Value]) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    for step in trajectory {
        let tool = step["tool_name"].as_str().unwrap_or("unknown");
        if step["side_effect_class"] != "read_only" {
            return Err(format!("GEPA trajectory contains non-read-only tool {tool}"));
        }
        if !matches!(step["status"].as_str(), Some("ok" | "degraded")) {
            return Err(format!("GEPA trajectory tool {tool} did not complete successfully"));
        }
        let key = format!("{tool}:{}", step["arguments_hash"].as_str().unwrap_or_default());
        if !seen.insert(key) {
            return Err(format!("GEPA trajectory repeats the same read-only tool call: {tool}"));
        }
    }
    Ok(())
}

fn response_text(model_response: &Value) -> String {
    ["reply", "answer_sections"]
        .iter()
        .filter_map(|key| model_response.get(*key))
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

fn numeric_fact_tokens(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0usize;
    while index < chars.len() {
        if !chars[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let start = index;
        while index < chars.len() && (chars[index].is_ascii_digit() || chars[index] == '.') {
            index += 1;
        }
        if index < chars.len() && chars[index] == '%' {
            index += 1;
        } else if index < chars.len() && chars[index] == '万' {
            index += 1;
        }
        let token: String = chars[start..index].iter().collect();
        let citation = start >= 2 && chars[start - 2] == '[' && chars[start - 1] == 'E';
        if !citation
            && (token.contains('%')
                || token.contains('万')
                || token.contains('.')
                || token.len() >= 4)
        {
            tokens.push(token);
        }
    }
    tokens
}

fn numeric_token_variants(token: &str) -> Vec<String> {
    let numeric = token.trim_end_matches(['%', '万']);
    let Ok(value) = numeric.parse::<f64>() else {
        return vec![token.to_string()];
    };
    let mut variants = vec![token.to_string(), numeric.to_string()];
    if token.ends_with('%') {
        variants.push(
            format!("{:.6}", value / 100.0)
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string(),
        );
    }
    if token.ends_with('万') {
        variants.push(format!("{:.0}", value * 10_000.0));
    }
    variants
}

fn validate_frozen_fact_literals(
    profile: &EvalProfile,
    model_response: &Value,
) -> Result<(), String> {
    let corpus = serde_json::to_string(&profile.tool_result).unwrap_or_default();
    for token in numeric_fact_tokens(&response_text(model_response)) {
        if !numeric_token_variants(&token)
            .iter()
            .any(|variant| corpus.contains(variant))
        {
            return Err(format!(
                "model output contains unsupported frozen fact literal {token}"
            ));
        }
    }
    Ok(())
}

fn score_case(profile: &EvalProfile, case: &EvalCase, model_response: &Value) -> CaseEvaluation {
    if let Err(error) = validate_frozen_fact_literals(profile, model_response) {
        return CaseEvaluation {
            score: 0.0,
            feedback: vec![format!("hard gate failed: {error}")],
            hard_failure: Some(error),
            merged_response: Value::Null,
        };
    }
    let merged_response = match agent_harness::merge_model_response(
        profile.tool_result.clone(),
        model_response,
        &profile.profile_id,
        Some("gepa-eval"),
    ) {
        Ok(response) => response,
        Err(error) => {
            let safe = agent_harness::redact_persisted_error(&error, None);
            return CaseEvaluation {
                score: 0.0,
                feedback: vec![format!("hard gate failed: {safe}")],
                hard_failure: Some(safe),
                merged_response: Value::Null,
            };
        }
    };
    let output_text = response_text(model_response);
    let mut hits = 0usize;
    let mut feedback = Vec::new();
    for criterion in &case.rubric {
        if criterion
            .any_of
            .iter()
            .any(|term| output_text.contains(term))
        {
            hits += 1;
        } else {
            feedback.push(format!(
                "missing rubric criterion {}: {}",
                criterion.id,
                criterion.any_of.join(" / ")
            ));
        }
    }
    CaseEvaluation {
        score: hits as f64 / case.rubric.len() as f64,
        feedback,
        hard_failure: None,
        merged_response,
    }
}
fn average(scores: &[CaseSummary]) -> f64 {
    if scores.is_empty() {
        0.0
    } else {
        scores.iter().map(|item| item.score).sum::<f64>() / scores.len() as f64
    }
}

fn compute_apply_gate(
    baseline_validation: &[CaseSummary],
    baseline_holdout: &[CaseSummary],
    candidate_validation: &[CaseSummary],
    candidate_holdout: &[CaseSummary],
) -> Value {
    let validation_non_regression = average(candidate_validation) >= average(baseline_validation);
    let holdout_non_regression = average(candidate_holdout) >= average(baseline_holdout);
    let hard_failures = candidate_validation
        .iter()
        .chain(candidate_holdout.iter())
        .filter(|case| case.hard_failure.is_some())
        .count();
    json!({
        "passed": validation_non_regression && holdout_non_regression && hard_failures == 0,
        "validation_non_regression": validation_non_regression,
        "holdout_non_regression": holdout_non_regression,
        "hard_failures": hard_failures,
    })
}

#[cfg(all(feature = "gepa-lab", any(target_os = "windows", target_os = "android")))]
mod engine {
    use super::*;
    use gepa::progress::{Event, Progress};
    use gepa::{
        extract_new_instruction, render_prompt, Candidate, EvalBatch, GepaAdapter, GepaEngine,
        Reflective,
    };
    use rig_agent::{completion::Prompt, AgentBuilder};
    use stock_optimizer_core::CoreDataSet;
    use std::{future::Future, sync::Arc, time::Duration};
    #[derive(Clone)]
    struct Trace {
        question: String,
        response: Value,
        trajectory: Vec<Value>,
        feedback: Vec<String>,
        hard_failure: Option<String>,
        score: f64,
    }
    struct AppProgress {
        app: AppHandle,
        run_id: String,
    }
    impl Progress for AppProgress {
        fn report(&self, event: Event<'_>) {
            emit(
                &self.app,
                &self.run_id,
                "progress",
                json!({"message": event.message(), "iteration": event.iteration()}),
            );
        }
    }
    struct Adapter {
        app: AppHandle,
        run_id: String,
        llm: Value,
        profile: EvalProfile,
        cancellation: Arc<rig_runtime::RunCancellation>,
    }
    impl Adapter {
        async fn evaluate_cases(
            &self,
            cases: &[EvalCase],
            candidate: &Candidate,
            capture: bool,
        ) -> EvalBatch<Trace> {
            let body = candidate.get("method_card").cloned().unwrap_or_default();
            let mut scores = Vec::new();
            let mut traces = Vec::new();
            for case in cases {
                let replay_run_id = format!("{}-{}", self.run_id, case.id);
                let payload = build_gepa_replay_payload(
                    &self.profile,
                    case,
                    &body,
                    &self.llm,
                    &replay_run_id,
                );
                let data = match serde_json::to_value(CoreDataSet::default()) {
                    Ok(value) => value,
                    Err(error) => {
                        let message = format!("GEPA replay dataset serialization failed: {error}");
                        scores.push(0.0);
                        if capture {
                            traces.push(Trace {
                                question: case.question.clone(),
                                response: Value::Null,
                                trajectory: Vec::new(),
                                feedback: vec![message.clone()],
                                hard_failure: Some(message),
                                score: 0.0,
                            });
                        }
                        continue;
                    }
                };
                let mut events = Vec::new();
                let replay = tokio::select! {
                    _ = self.cancellation.cancelled() => {
                        rig_runtime::request_cancel(&replay_run_id);
                        Err("GEPA run cancelled".to_string())
                    }
                    result = rig_runtime::execute_with_event_sink(payload, data, |event| events.push(event)) => {
                        result.map(|outcome| outcome.response)
                    }
                };
                let trajectory = replay_tool_trajectory(&events);
                let (response, error) = match replay {
                    Ok(response) if response["harness"]["model_used"] == true => {
                        match validate_gepa_trajectory(&trajectory) {
                            Ok(()) => (response, None),
                            Err(error) => (response, Some(error)),
                        }
                    }
                    Ok(response) => {
                        let message = response["model_warning"]
                            .as_str()
                            .unwrap_or("shared Agent runtime did not produce a model-backed answer")
                            .to_string();
                        (response, Some(message))
                    }
                    Err(error) => (Value::Null, Some(error)),
                };
                let evaluation = error.map_or_else(
                    || score_case(&self.profile, case, &response),
                    |error| CaseEvaluation {
                        score: 0.0,
                        feedback: vec![format!("shared replay hard gate failed: {error}")],
                        hard_failure: Some(error),
                        merged_response: response.clone(),
                    },
                );
                scores.push(evaluation.score);
                if capture {
                    traces.push(Trace {
                        question: case.question.clone(),
                        response: evaluation.merged_response,
                        trajectory,
                        feedback: evaluation.feedback,
                        hard_failure: evaluation.hard_failure,
                        score: evaluation.score,
                    });
                }
            }
            EvalBatch {
                scores,
                captured_traces: capture,
                outputs: if capture { Some(traces) } else { None },
            }
        }
    }
    impl GepaAdapter for Adapter {
        type Output = Trace;
        fn evaluate_minibatch(
            &mut self,
            ids: &[usize],
            candidate: &Candidate,
            capture: bool,
        ) -> impl Future<Output = EvalBatch<Self::Output>> + Send {
            let cases = ids
                .iter()
                .filter_map(|id| self.profile.train.get(*id))
                .cloned()
                .collect::<Vec<_>>();
            async move { self.evaluate_cases(&cases, candidate, capture).await }
        }
        fn evaluate_valset(
            &mut self,
            candidate: &Candidate,
        ) -> impl Future<Output = EvalBatch<Self::Output>> + Send {
            let cases = self.profile.validation.clone();
            async move { self.evaluate_cases(&cases, candidate, false).await }
        }
        fn evaluate_valset_ids(
            &mut self,
            ids: &[usize],
            candidate: &Candidate,
        ) -> impl Future<Output = EvalBatch<Self::Output>> + Send {
            let cases = ids
                .iter()
                .filter_map(|id| self.profile.validation.get(*id))
                .cloned()
                .collect::<Vec<_>>();
            async move { self.evaluate_cases(&cases, candidate, false).await }
        }
        fn propose_new_texts(
            &mut self,
            candidate: &Candidate,
            _components: &[String],
            captured: &EvalBatch<Self::Output>,
        ) -> impl Future<Output = Result<Candidate, String>> + Send {
            let current = candidate.get("method_card").cloned().unwrap_or_default();
            let traces = captured.outputs.clone().unwrap_or_default();
            let dataset = traces
                .into_iter()
                .map(|trace| {
                    vec![
                        (
                            "Inputs".to_string(),
                            Reflective::Map(vec![(
                                "question".to_string(),
                                Reflective::Text(trace.question),
                            )]),
                        ),
                        (
                            "Generated Outputs".to_string(),
                            Reflective::Text(trace.response.to_string()),
                        ),
                        (
                            "Feedback".to_string(),
                            Reflective::Text(if trace.feedback.is_empty() {
                                format!("rubric score: {}", trace.score)
                            } else {
                                trace.feedback.join("; ")
                            }),
                        ),
                    ]
                })
                .collect::<Vec<_>>();
            let prompt = render_prompt(&current, &dataset, None);
            let llm = self.llm.clone();
            let cancellation = self.cancellation.clone();
            let app = self.app.clone();
            let run_id = self.run_id.clone();
            async move {
                if cancellation.is_cancelled() {
                    return Err("GEPA run cancelled".to_string());
                }
                let config = rig_runtime::normalize_provider_config(&llm)?;
                let model = rig_runtime::build_model_with_payload(&config, &llm).map_err(|e| {
                    agent_harness::redact_persisted_error(&e.to_string(), Some(&llm))
                })?;
                let agent = AgentBuilder::from_model_handle(model).name("gepa-reflection").preamble("你是 GEPA 提示词反思器。只返回新的研究方法卡，必须保留安全边界、证据优先和研究模式语义；把新方法卡放在 ``` 区块中，不输出解释。").default_max_turns(1).max_tokens(2_500).temperature(0.2).build();
                emit(
                    &app,
                    &run_id,
                    "reflection",
                    json!({"message":"正在根据固定评测反馈反思方法卡"}),
                );
                let raw = tokio::select! {
                    _ = cancellation.cancelled() => return Err("GEPA run cancelled".to_string()),
                    result = tokio::time::timeout(
                        Duration::from_secs(config.timeout_seconds.min(120).max(1)),
                        agent.prompt(prompt),
                    ) => result
                        .map_err(|_| "GEPA reflection timed out".to_string())?
                        .map_err(|e| agent_harness::redact_persisted_error(&e.to_string(), Some(&llm)))?,
                };
                let body = agent_harness::redact_persisted_question(
                    &extract_new_instruction(&raw),
                    Some(&llm),
                );
                if body.trim().is_empty() {
                    return Err("GEPA reflection returned an empty method card".to_string());
                }
                let mut next = candidate.clone();
                next.insert("method_card", body);
                Ok(next)
            }
        }
    }
    pub(super) async fn run(
        app: AppHandle,
        run_id: String,
        profile: EvalProfile,
        llm: Value,
        max_metric_calls: usize,
        base_version: String,
        cancellation: Arc<rig_runtime::RunCancellation>,
    ) -> GepaRunReport {
        emit(
            &app,
            &run_id,
            "status",
            json!({"stage":"preflight","percent":5,"message":"准备 GEPA 固定评测"}),
        );
        let card = prompt_upgrade::resolve_method_card(&profile.profile_id, Some(&app))
            .map(|v| v.body)
            .unwrap_or_default();
        let seed = Candidate::from([("method_card".to_string(), card)]);
        let baseline_body = seed.get("method_card").cloned().unwrap_or_default();
        let baseline = score_cases_direct(
            &app,
            &run_id,
            &profile,
            &llm,
            &baseline_body,
            &cancellation,
            &profile.validation,
        )
        .await;
        let baseline_holdout = score_cases_direct(
            &app,
            &run_id,
            &profile,
            &llm,
            &baseline_body,
            &cancellation,
            &profile.holdout,
        )
        .await;
        if cancellation.is_cancelled() {
            return cancelled_report(&run_id, &profile, &base_version, max_metric_calls);
        }
        emit(
            &app,
            &run_id,
            "status",
            json!({"stage":"search","percent":20,"message":"GEPA 正在搜索候选方法卡"}),
        );
        let engine = GepaEngine {
            adapter: Adapter {
                app: app.clone(),
                run_id: run_id.clone(),
                llm: llm.clone(),
                profile: profile.clone(),
                cancellation: cancellation.clone(),
            },
            trainset_size: profile.train.len(),
            valset_size: profile.validation.len(),
            minibatch_size: profile.train.len().min(4),
            max_metric_calls,
            perfect_score: 1.0,
            skip_perfect_score: true,
            seed: SEED,
            use_merge: false,
            max_merge_invocations: 0,
            candidate_selection_strategy: gepa::CandidateSelection::Pareto,
            component_selector: gepa::ComponentSelection::All,
            track_best_outputs: false,
            progress: std::sync::Arc::new(AppProgress {
                app: app.clone(),
                run_id: run_id.clone(),
            }),
        };
        let outcome = engine.optimize(seed).await;
        if cancellation.is_cancelled() {
            return cancelled_report(&run_id, &profile, &base_version, max_metric_calls);
        }
        let candidate_body = outcome.best.get("method_card").cloned().unwrap_or_default();
        let candidate_validation = score_cases_direct(
            &app,
            &run_id,
            &profile,
            &llm,
            &candidate_body,
            &cancellation,
            &profile.validation,
        )
        .await;
        let candidate_holdout = score_cases_direct(
            &app,
            &run_id,
            &profile,
            &llm,
            &candidate_body,
            &cancellation,
            &profile.holdout,
        )
        .await;
        if cancellation.is_cancelled() {
            return cancelled_report(&run_id, &profile, &base_version, max_metric_calls);
        }
        let candidate_body = agent_harness::redact_persisted_question(&candidate_body, Some(&llm));
        let apply_gate = compute_apply_gate(
            &baseline,
            &baseline_holdout,
            &candidate_validation,
            &candidate_holdout,
        );
        let report = GepaRunReport {
            run_id: run_id.clone(),
            status: "completed".to_string(),
            profile_id: profile.profile_id.clone(),
            base_prompt_version: base_version,
            candidate_prompt_version: Some(format!("gepa-candidate-{run_id}")),
            eval_suite_version: "agent-gepa-eval-v1".to_string(),
            replay_profile: "model-backed_v1_shared_runtime".to_string(),
            dataset_sha256: dataset_sha256(),
            engine_version: ENGINE_VERSION.to_string(),
            seed: SEED,
            max_metric_calls,
            baseline_validation_score: Some(average(&baseline)),
            baseline_holdout_score: Some(average(&baseline_holdout)),
            candidate_validation_score: Some(average(&candidate_validation)),
            candidate_holdout_score: Some(average(&candidate_holdout)),
            baseline_validation: baseline,
            baseline_holdout,
            candidate_validation,
            candidate_holdout,
            candidate_body: Some(candidate_body),
            apply_gate,
            error: None,
        };
        report
    }
    async fn score_cases_direct(
        app: &AppHandle,
        run_id: &str,
        profile: &EvalProfile,
        llm: &Value,
        body: &str,
        cancellation: &Arc<rig_runtime::RunCancellation>,
        cases: &[EvalCase],
    ) -> Vec<CaseSummary> {
        let adapter = Adapter {
            app: app.clone(),
            run_id: run_id.to_string(),
            llm: llm.clone(),
            profile: profile.clone(),
            cancellation: cancellation.clone(),
        };
        let candidate = Candidate::from([("method_card".to_string(), body.to_string())]);
        let batch = adapter.evaluate_cases(cases, &candidate, true).await;
        batch
            .outputs
            .unwrap_or_default()
            .into_iter()
            .zip(cases.iter())
            .map(|(trace, case)| CaseSummary {
                id: case.id.clone(),
                score: trace.score,
                hard_failure: trace
                    .hard_failure
                    .map(|item| agent_harness::redact_persisted_error(&item, Some(llm))),
                feedback: trace
                    .feedback
                    .into_iter()
                    .map(|item| agent_harness::redact_persisted_question(&item, Some(llm)))
                    .collect(),
                response: agent_harness::redact_persisted_response(&trace.response, Some(llm)),
                trajectory: agent_harness::redact_persisted_response(
                    &Value::Array(trace.trajectory),
                    Some(llm),
                )
                .as_array()
                .cloned()
                .unwrap_or_default(),
            })
            .collect()
    }
}

fn cancelled_report(
    run_id: &str,
    profile: &EvalProfile,
    base_version: &str,
    max_metric_calls: usize,
) -> GepaRunReport {
    GepaRunReport {
        run_id: run_id.to_string(),
        status: "cancelled".to_string(),
        profile_id: profile.profile_id.clone(),
        base_prompt_version: base_version.to_string(),
        candidate_prompt_version: None,
        eval_suite_version: "agent-gepa-eval-v1".to_string(),
            replay_profile: "model-backed_v1_shared_runtime".to_string(),
        dataset_sha256: dataset_sha256(),
        engine_version: ENGINE_VERSION.to_string(),
        seed: SEED,
        max_metric_calls,
        baseline_validation_score: None,
        baseline_holdout_score: None,
        candidate_validation_score: None,
        candidate_holdout_score: None,
        baseline_validation: Vec::new(),
        baseline_holdout: Vec::new(),
        candidate_validation: Vec::new(),
        candidate_holdout: Vec::new(),
        candidate_body: None,
        apply_gate: json!({"passed": false, "reason": "run cancelled"}),
        error: Some("GEPA run cancelled".to_string()),
    }
}

/// Debug-only bridge used by `tmp/run-gepa-codex.mjs`. It is intentionally gated out of
/// production and mobile builds; the script supplies an OpenAI-compatible local proxy.
pub(crate) fn maybe_start_headless_from_env(app: AppHandle) {
    #[cfg(all(
        feature = "gepa-lab",
        target_os = "windows",
        not(mobile),
        debug_assertions
    ))]
    {
        let Some(config_path) = env::var_os("GP_GEPA_HEADLESS_CONFIG") else {
            return;
        };
        let output_path = env::var_os("GP_GEPA_HEADLESS_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("tmp/gepa-codex-report.json"));
        tauri::async_runtime::spawn(async move {
            let result = async {
                let text = fs::read_to_string(&config_path)
                    .map_err(|error| format!("read GEPA headless config failed: {error}"))?;
                let payload: Value = serde_json::from_str(&text)
                    .map_err(|error| format!("parse GEPA headless config failed: {error}"))?;
                let started = api_agent_gepa_start(app.clone(), payload)?;
                let run_id = started
                    .get("run_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "GEPA headless start did not return run_id".to_string())?
                    .to_string();
                loop {
                    sleep(Duration::from_secs(1)).await;
                    if let Ok(report) = load_report(&app, &run_id) {
                        if report.status != "running" {
                            let output = json!({"started": started, "report": report});
                            write_headless_output(&output_path, &output)?;
                            return Ok::<(), String>(());
                        }
                    }
                }
            }
            .await;
            if let Err(error) = result {
                let _ = write_headless_output(&output_path, &json!({"error": error}));
                std::process::exit(1);
            }
            std::process::exit(0);
        });
    }
    #[cfg(not(all(
        feature = "gepa-lab",
        target_os = "windows",
        not(mobile),
        debug_assertions
    )))]
    let _ = app;
}

fn write_headless_output(path: &PathBuf, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create GEPA headless output directory failed: {error}"))?;
    }
    let text = serde_json::to_string_pretty(value)
        .map_err(|error| format!("encode GEPA headless output failed: {error}"))?;
    fs::write(path, text).map_err(|error| format!("write GEPA headless output failed: {error}"))
}
#[tauri::command]
pub(crate) fn api_agent_gepa_status() -> Result<Value, String> {
    let compiled = cfg!(all(
        feature = "gepa-lab",
        any(target_os = "windows", target_os = "android")
    ));
    let enabled = compiled && crate::diagnostics::gepa_allowed();
    Ok(
        json!({"enabled": enabled, "compiled": compiled, "engine_version": if enabled { ENGINE_VERSION } else { "disabled" }}),
    )
}
#[tauri::command]
pub(crate) fn api_agent_gepa_start(app: AppHandle, payload: Value) -> Result<Value, String> {
    if !crate::diagnostics::gepa_allowed() {
        let _ = crate::diagnostics::record(crate::diagnostics::OperationalEvent::GepaBlocked);
        return Err("GEPA 已关闭或处于安全启动模式；请在本地设置中检查开关".into());
    }
    #[cfg(not(all(
        feature = "gepa-lab",
        any(target_os = "windows", target_os = "android")
    )))]
    {
        let _ = (app, payload);
        return Err("GEPA 仅在启用功能的 Windows 或 Android 构建中可用".to_string());
    }
    #[cfg(all(
        feature = "gepa-lab",
        any(target_os = "windows", target_os = "android")
    ))]
    {
        let profile_id = payload
            .get("profile_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let calls = payload
            .get("max_metric_calls")
            .and_then(Value::as_u64)
            .unwrap_or(60) as usize;
        if !matches!(calls, 30 | 60 | 120) {
            return Err("max_metric_calls must be 30, 60, or 120".to_string());
        }
        let mut suite = parse_suite()?;
        let profile = suite
            .profiles
            .remove(profile_id)
            .ok_or_else(|| "未知 GEPA profile".to_string())?;
        let llm = payload
            .get("llm")
            .cloned()
            .ok_or_else(|| "GEPA 需要当前 Agent 模型配置".to_string())?;
        rig_runtime::normalize_provider_config(&llm)?;
        let card = prompt_upgrade::resolve_method_card(profile_id, Some(&app))?;
        let run_id = crate::agent_ledger::next_run_id();
        let cancellation = rig_runtime::register_run(&run_id);
        let task_app = app.clone();
        let task_id = run_id.clone();
        let task_profile = profile.clone();
        let task_llm = llm.clone();
        let base_version = card.version.clone();
        let task_cancel = cancellation.clone();
        tauri::async_runtime::spawn(async move {
            let report = engine::run(
                task_app.clone(),
                task_id.clone(),
                task_profile,
                task_llm.clone(),
                calls,
                base_version,
                task_cancel,
            )
            .await;
            match save_report(&task_app, &report) {
                Ok(()) => emit(&task_app, &task_id, "complete", json!({"report": report})),
                Err(error) => emit(
                    &task_app,
                    &task_id,
                    "failed",
                    json!({"message": agent_harness::redact_persisted_error(&error, Some(&task_llm))}),
                ),
            }
            rig_runtime::unregister_run(&task_id);
        });
        Ok(
            json!({"run_id": run_id, "status":"running", "profile_id": profile_id, "base_prompt_version": card.version, "dataset_sha256": dataset_sha256(), "max_metric_calls": calls}),
        )
    }
}
#[tauri::command]
pub(crate) fn api_agent_gepa_cancel(payload: Value) -> Result<Value, String> {
    let id = payload
        .get("run_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    validate_run_id(id)?;
    Ok(json!({"run_id":id,"cancelled":rig_runtime::request_cancel(id)}))
}
#[tauri::command]
pub(crate) fn api_agent_gepa_report(app: AppHandle, payload: Value) -> Result<Value, String> {
    let id = payload
        .get("run_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    validate_run_id(id)?;
    serde_json::to_value(load_report(&app, id)?).map_err(|e| e.to_string())
}
#[tauri::command]
pub(crate) fn api_agent_gepa_apply(app: AppHandle, payload: Value) -> Result<Value, String> {
    if !cfg!(all(
        feature = "gepa-lab",
        any(target_os = "windows", target_os = "android")
    )) || !crate::diagnostics::gepa_allowed()
    {
        let _ = crate::diagnostics::record(crate::diagnostics::OperationalEvent::GepaBlocked);
        return Err("GEPA 已关闭或处于安全启动模式；不能应用实验候选".into());
    }
    let id = payload
        .get("run_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let expected = payload
        .get("expected_base_prompt_version")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    validate_run_id(id)?;
    if expected.is_empty() {
        return Err("expected_base_prompt_version is required".to_string());
    }
    let report = load_report(&app, id)?;
    if report.status != "completed" {
        return Err("only a completed GEPA run can be applied".to_string());
    }
    if report.apply_gate["passed"] != true {
        return Err("GEPA candidate failed validation/holdout apply gate".to_string());
    }
    let body = report
        .candidate_body
        .as_deref()
        .ok_or_else(|| "GEPA report has no candidate".to_string())?;
    let version = prompt_upgrade::activate_gepa_with_app(&app, &report.profile_id, expected, body)?;
    Ok(json!({"run_id":id,"profile_id":report.profile_id,"prompt_version":version,"applied":true}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_suite_has_two_profiles_and_frozen_splits() {
        let suite = parse_suite().expect("GEPA suite");
        assert_eq!(suite.profiles.len(), 2);
        for profile in suite.profiles.values() {
            assert_eq!(profile.train.len(), 8);
            assert_eq!(profile.validation.len(), 4);
            assert_eq!(profile.holdout.len(), 4);
        }
    }

    #[test]
    fn gepa_replay_payload_uses_shared_runtime_and_keeps_rubric_out_of_agent_input() {
        let suite = parse_suite().expect("GEPA suite");
        let profile = &suite.profiles["value_compounder_v1"];
        let case = &profile.train[0];
        let payload = build_gepa_replay_payload(
            profile,
            case,
            "method-card candidate",
            &json!({"model": "test-model", "api_key": "test-secret"}),
            "gepa-test-run",
        );

        assert_eq!(payload["mode"], profile.mode);
        assert_eq!(payload["message"], case.question);
        assert_eq!(payload["method_card"], "method-card candidate");
        assert_eq!(payload["research_evidence"]["citations"].as_array().unwrap().len(), 2);
        assert_eq!(payload["research_evidence"]["citations"][0]["remote_export_allowed"], true);
        assert!(payload.get("rubric").is_none());
        assert!(!payload.to_string().contains(&case.rubric[0].id));
    }

    #[test]
    fn trajectory_gate_rejects_repeated_or_mutating_calls() {
        let repeated = vec![
            json!({"tool_name":"news_evidence","arguments_hash":"same","status":"ok","side_effect_class":"read_only"}),
            json!({"tool_name":"news_evidence","arguments_hash":"same","status":"ok","side_effect_class":"read_only"}),
        ];
        assert!(validate_gepa_trajectory(&repeated).is_err());
        let mutating = vec![
            json!({"tool_name":"unknown","arguments_hash":"x","status":"ok","side_effect_class":"unknown"}),
        ];
        assert!(validate_gepa_trajectory(&mutating).is_err());
    }

    #[test]
    fn apply_gate_rejects_holdout_regression_and_hard_failures() {
        let baseline = CaseSummary {
            id: "baseline".to_string(),
            score: 1.0,
            hard_failure: None,
            feedback: vec![],
            response: Value::Null,
            trajectory: vec![],
        };
        let candidate = CaseSummary {
            id: "candidate".to_string(),
            score: 0.5,
            hard_failure: Some("trajectory failed".to_string()),
            feedback: vec![],
            response: Value::Null,
            trajectory: vec![],
        };
        let gate = compute_apply_gate(&[baseline.clone()], &[baseline], &[candidate.clone()], &[candidate]);
        assert_eq!(gate["passed"], false);
        assert_eq!(gate["hard_failures"], 2);
    }

    #[test]
    fn score_case_returns_rubric_coverage_and_preserves_frozen_tool_facts() {
        let suite = parse_suite().expect("valid fixture");
        let profile = &suite.profiles["value_compounder_v1"];
        let case = &profile.train[0];
        let evaluation = score_case(
            profile,
            case,
            &json!({
                "reply": "重点核验经营现金流和所有者收益，不能只看收入增速。[E1]",
                "answer_sections": [{"title":"盈利质量", "bullets":["对照经营现金流与资本开支。[E1]"]}]
            }),
        );
        assert!((evaluation.score - (2.0 / 3.0)).abs() < f64::EPSILON);
        assert!(evaluation.hard_failure.is_none());
        assert_eq!(
            evaluation.merged_response["action"],
            profile.tool_result["action"]
        );
        assert_eq!(
            evaluation.merged_response["data"],
            profile.tool_result["data"]
        );
        assert!(evaluation
            .feedback
            .iter()
            .any(|feedback| feedback.contains("management")));
    }

    #[test]
    fn unsafe_or_uncited_output_is_a_hard_zero() {
        let suite = parse_suite().expect("valid fixture");
        let profile = &suite.profiles["value_compounder_v1"];
        let case = &profile.train[0];
        for response in [
            json!({"reply":"建议买入并重仓持有。[E1]"}),
            json!({"reply":"经营现金流和所有者收益需要核验。"}),
            json!({"reply":"收入增长 99%。[E1]"}),
        ] {
            let evaluation = score_case(profile, case, &response);
            assert_eq!(evaluation.score, 0.0);
            assert!(evaluation.hard_failure.is_some());
        }
    }

    #[test]
    fn rubric_keywords_in_warnings_do_not_count_as_answer_coverage() {
        let suite = parse_suite().expect("valid fixture");
        let profile = &suite.profiles["value_compounder_v1"];
        let case = &profile.train[0];
        let evaluation = score_case(
            profile,
            case,
            &json!({
                "reply": "只说明需要继续核验。[E1]",
                "warnings": ["所有者收益、资本强度、管理质量"],
            }),
        );
        assert_eq!(evaluation.score, 0.0);
    }

    #[test]
    fn run_ids_are_path_safe() {
        assert!(validate_run_id("gp-agent-run-1_2").is_ok());
        assert!(validate_run_id("..\\escape").is_err());
        assert!(validate_run_id("run/id").is_err());
    }

    #[test]
    fn dataset_hash_is_stable_and_does_not_include_credentials() {
        let hash = dataset_sha256();
        assert_eq!(hash.len(), 64);
        assert!(!hash.contains("api_key"));
    }
}
