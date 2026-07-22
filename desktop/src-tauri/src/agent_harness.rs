use crate::{build_http_client_with_proxy, runtime};
use futures::StreamExt;
use serde_json::{json, Map, Value};
use std::{env, time::Duration};
use stock_optimizer_core as gp_core;

const BASE_PROMPT: &str = include_str!("../../../app/prompts/stock_soul.md");
const HOT_MONEY_PROMPT: &str = include_str!("../../../app/prompts/hot_money_early_v1.md");
const VALUE_COMPOUNDER_PROMPT: &str = include_str!("../../../app/prompts/value_compounder_v1.md");
const PROMPT_VERSION: &str = "agent-harness-v2.0";
const RISK_NOTICE: &str = "仅供选股研究，不构成投资建议。";
const MAX_HARNESS_PAYLOAD_BYTES: usize = 512 * 1024;
const MAX_MESSAGE_CHARS: usize = 8_000;
const MAX_HISTORY_MESSAGES: usize = 12;
const MAX_HISTORY_CHARS: usize = 2_000;
const MAX_MODEL_REQUEST_BYTES: usize = 2 * 1024 * 1024;
const MAX_MODEL_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

struct PromptProfile {
    id: &'static str,
    instructions: &'static str,
}

#[derive(Clone, Debug)]
struct LlmConfig {
    api_key: String,
    base_url: String,
    model: String,
    temperature: f64,
    timeout_seconds: u64,
    json_mode: bool,
    organization: Option<String>,
    project: Option<String>,
}

pub(crate) struct AgentHarnessOutcome {
    #[cfg(test)]
    pub(crate) events: Vec<Value>,
    pub(crate) response: Value,
}

fn publish_event<F>(events: &mut Vec<Value>, sink: &mut F, event: Value)
where
    F: FnMut(Value),
{
    sink(event.clone());
    events.push(event);
}

pub(crate) fn prompt_preview(payload: &Value, tool_response: &Value) -> Value {
    let mode = payload
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("quick");
    let profile = profile_for_mode(mode);
    let compact_tool_result = compact_json(tool_response, 0);
    let tool_result_truncated = compact_tool_result != *tool_response;
    let evidence_catalog = build_evidence_catalog(tool_response);
    let system_prompt = format!(
        "{}\n\n# 当前模式方法框架\n{}\n\n# 输出契约\n只返回 JSON，字段必须为 reply、answer_sections（每项含 title 和 bullets）、warnings、next_actions。不得修改、覆盖或虚构工具事实。reply 和每条事实 bullet 必须邻近引用证据目录中的有效编号，例如 [E1]；不得生成未知编号。",
        BASE_PROMPT.trim(),
        profile.instructions.trim(),
    );
    json!({
        "prompt_version": PROMPT_VERSION,
        "profile_id": profile.id,
        "system_prompt": system_prompt,
        "user_payload": {
            "question": payload.get("message").and_then(Value::as_str).unwrap_or("").trim(),
            "history": bounded_history(payload.get("history")),
            "tool_result": compact_tool_result,
            "tool_result_truncated": tool_result_truncated,
            "evidence_catalog": evidence_catalog,
        }
    })
}

pub(crate) fn merge_model_response(
    mut tool_response: Value,
    model_response: &Value,
    profile_id: &str,
    model_name: Option<&str>,
) -> Result<Value, String> {
    if contains_forbidden_model_instruction(model_response) {
        return Err(
            "agent model response contains a prohibited trading or manipulation instruction"
                .to_string(),
        );
    }
    let evidence_count = build_evidence_catalog(&tool_response).len();
    validate_model_evidence(model_response, evidence_count)?;
    let target = tool_response
        .as_object_mut()
        .ok_or_else(|| "agent tool response must be an object".to_string())?;
    let reply = model_response
        .get("reply")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "agent model response is missing reply".to_string())?;
    let tool_reply = target
        .get("reply")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let combined_reply = tool_reply
        .map(|tool_reply| format!("{tool_reply}\n\n模型综合：{reply}"))
        .unwrap_or_else(|| reply.to_string());
    target.insert(
        "reply".to_string(),
        Value::String(limit_chars(&combined_reply, 6_000)),
    );

    if let Some(sections) = sanitized_sections(model_response.get("answer_sections")) {
        target.insert("model_answer_sections".to_string(), Value::Array(sections));
    }

    let mut actions = sanitized_strings(target.get("next_actions"), 12, 300).unwrap_or_default();
    for action in sanitized_strings(model_response.get("next_actions"), 6, 200).unwrap_or_default()
    {
        if !actions.contains(&action) {
            actions.push(action);
        }
    }
    if !actions.is_empty() {
        target.insert("next_actions".to_string(), Value::Array(actions));
    }

    let mut warnings = sanitized_strings(target.get("warnings"), 12, 500).unwrap_or_default();
    for warning in sanitized_strings(model_response.get("warnings"), 8, 300).unwrap_or_default() {
        if !warnings.contains(&warning) {
            warnings.push(warning);
        }
    }
    if !warnings
        .iter()
        .any(|item| item.as_str() == Some(RISK_NOTICE))
    {
        warnings.push(Value::String(RISK_NOTICE.to_string()));
    }
    target.insert("warnings".to_string(), Value::Array(warnings));
    target.insert(
        "harness".to_string(),
        json!({
            "prompt_version": PROMPT_VERSION,
            "profile_id": profile_id,
            "model_used": true,
            "model": model_name,
        }),
    );
    Ok(tool_response)
}

#[cfg(test)]
pub(crate) async fn execute(payload: Value, data: Value) -> Result<AgentHarnessOutcome, String> {
    execute_with_event_sink(payload, data, |_| {}).await
}

pub(crate) async fn execute_with_event_sink<F>(
    payload: Value,
    data: Value,
    mut sink: F,
) -> Result<AgentHarnessOutcome, String>
where
    F: FnMut(Value) + Send,
{
    let payload_bytes = serde_json::to_vec(&payload)
        .map_err(|error| format!("serialize Agent payload failed: {error}"))?
        .len();
    if payload_bytes > MAX_HARNESS_PAYLOAD_BYTES {
        return Err("Agent payload exceeds 512 KiB".to_string());
    }
    let message = payload
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if message.is_empty() {
        return Err("Agent message is required".to_string());
    }
    if message.chars().count() > MAX_MESSAGE_CHARS {
        return Err(format!(
            "Agent message exceeds the {MAX_MESSAGE_CHARS} character limit"
        ));
    }
    let run_id = payload
        .get("run_id")
        .and_then(Value::as_str)
        .filter(|item| !item.is_empty())
        .unwrap_or("gp-agent-run")
        .to_string();
    let mode = payload
        .get("mode")
        .and_then(Value::as_str)
        .filter(|item| !item.is_empty())
        .unwrap_or("quick")
        .to_string();
    let mut core_request = json!({
        "data": data,
        "message": message,
        "run_id": run_id,
        "mode": mode,
        "context": bounded_context(payload.get("context")),
    });
    for key in ["platform", "network"] {
        if let Some(value) = payload.get(key) {
            core_request[key] = value.clone();
        }
    }

    let mut events = Vec::new();
    publish_event(
        &mut events,
        &mut sink,
        status_event(&run_id, "tools", "执行本地工具", 12),
    );
    let core_events = runtime::run_cpu_bound("agent_harness_tools", move || {
        gp_core::agent_stream_with_data_events_value(core_request)
            .map_err(|error| error.to_string())
    })
    .await??;
    let mut tool_response = None;
    let mut core_error = None;
    for event in core_events {
        let value = serde_json::to_value(event).map_err(|error| error.to_string())?;
        match value.get("type").and_then(Value::as_str) {
            Some("result") => tool_response = value.get("response").cloned(),
            Some("error") => {
                core_error = value
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            }
            Some("final") => {}
            Some("status")
                if matches!(
                    value.get("stage").and_then(Value::as_str),
                    Some("format" | "complete")
                ) => {}
            _ => publish_event(&mut events, &mut sink, value),
        }
    }
    let tool_response = tool_response.ok_or_else(|| {
        core_error.unwrap_or_else(|| "Agent tools did not produce a result".to_string())
    })?;
    let preview = prompt_preview(&payload, &tool_response);
    let profile_id = preview
        .get("profile_id")
        .and_then(Value::as_str)
        .unwrap_or("deterministic_v1")
        .to_string();
    let should_call_model = profile_id != "deterministic_v1";
    let llm_value = should_call_model.then(|| payload.get("llm")).flatten();
    let config = should_call_model
        .then(|| resolve_llm_config(llm_value))
        .flatten();
    let model_name = config.as_ref().map(|item| item.model.as_str());

    publish_event(
        &mut events,
        &mut sink,
        status_event(
            &run_id,
            "model",
            if profile_id == "deterministic_v1" {
                "本地工具执行"
            } else if config.is_some() {
                "模型综合"
            } else {
                "本地降级整理"
            },
            86,
        ),
    );
    let model_result = if should_call_model {
        call_model(llm_value, &preview).await
    } else {
        Ok(None)
    };
    let response = match model_result {
        Ok(Some(model_response)) => match merge_model_response(
            tool_response.clone(),
            &model_response,
            &profile_id,
            model_name,
        ) {
            Ok(response) => response,
            Err(error) => fallback_response(
                tool_response,
                &profile_id,
                model_name,
                Some(format!("模型输出未通过安全校验，已回退本地结果：{error}")),
            ),
        },
        Ok(None) => fallback_response(tool_response, &profile_id, None, None),
        Err(error) => fallback_response(
            tool_response,
            &profile_id,
            model_name,
            Some(format!(
                "模型调用失败，已回退本地结果：{}",
                redact_model_error(&error, config.as_ref().map(|item| item.api_key.as_str()))
            )),
        ),
    };
    publish_event(
        &mut events,
        &mut sink,
        status_event(&run_id, "validate", "校验证据与风险边界", 94),
    );
    publish_event(
        &mut events,
        &mut sink,
        status_event(&run_id, "complete", "完成", 100),
    );
    publish_event(
        &mut events,
        &mut sink,
        json!({
            "run_id": run_id,
            "type": "final",
            "action": response.get("action"),
            "payload": response.get("harness"),
        }),
    );
    publish_event(
        &mut events,
        &mut sink,
        json!({
            "run_id": run_id,
            "type": "result",
            "action": response.get("action"),
            "response": response.clone(),
        }),
    );
    Ok(AgentHarnessOutcome {
        #[cfg(test)]
        events,
        response,
    })
}

fn fallback_response(
    mut tool_response: Value,
    profile_id: &str,
    model_name: Option<&str>,
    model_warning: Option<String>,
) -> Value {
    if let Some(target) = tool_response.as_object_mut() {
        let warnings = target
            .entry("warnings".to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        if !warnings.is_array() {
            *warnings = Value::Array(Vec::new());
        }
        let warnings = warnings.as_array_mut().expect("warnings was normalized");
        if !warnings
            .iter()
            .any(|item| item.as_str() == Some(RISK_NOTICE))
        {
            warnings.push(Value::String(RISK_NOTICE.to_string()));
        }
        if let Some(warning) = model_warning {
            warnings.push(Value::String(limit_chars(&warning, 500)));
        } else if profile_id != "deterministic_v1" {
            warnings.push(Value::String(
                "未配置可用模型，本次仅返回本地工具事实，未执行方法框架综合。".to_string(),
            ));
        }
        target.insert(
            "harness".to_string(),
            json!({
                "prompt_version": PROMPT_VERSION,
                "profile_id": profile_id,
                "model_used": false,
                "model": model_name,
            }),
        );
    }
    tool_response
}

fn status_event(run_id: &str, stage: &str, label: &str, percent: u8) -> Value {
    json!({
        "run_id": run_id,
        "type": "status",
        "stage": stage,
        "label": label,
        "percent": percent,
    })
}

fn redact_model_error(error: &str, api_key: Option<&str>) -> String {
    let mut result = error.to_string();
    if let Some(secret) = api_key.filter(|item| !item.is_empty()) {
        result = result.replace(secret, "***");
    }
    limit_chars(&result, 400)
}

pub(crate) async fn call_model(
    llm_value: Option<&Value>,
    preview: &Value,
) -> Result<Option<Value>, String> {
    let Some(config) = resolve_llm_config(llm_value) else {
        return Ok(None);
    };
    validate_llm_config(&config)?;
    let client = build_http_client_with_proxy(
        "Mozilla/5.0 GuXuanYou/0.4 agent-harness",
        Duration::from_secs(config.timeout_seconds),
        None,
    )
    .map_err(|error| format!("create Agent LLM client failed: {error}"))?;
    let system_prompt = preview
        .get("system_prompt")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let user_payload = json!({
        "prompt_version": preview.get("prompt_version"),
        "profile_id": preview.get("profile_id"),
        "input": preview.get("user_payload"),
    });
    let mut request = json!({
        "model": config.model,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_payload.to_string()}
        ],
        "temperature": config.temperature,
    });
    if config.json_mode {
        request["response_format"] = json!({"type": "json_object"});
    }

    match post_model_request(&client, &config, &request).await {
        Ok(value) => Ok(Some(value)),
        Err(first_error) if config.json_mode && should_retry_without_json_mode(&first_error) => {
            if let Some(object) = request.as_object_mut() {
                object.remove("response_format");
            }
            post_model_request(&client, &config, &request)
                .await
                .map(Some)
                .map_err(|second_error| {
                    format!(
                        "{first_error}; Agent model retry without JSON mode failed: {second_error}"
                    )
                })
        }
        Err(error) => Err(error),
    }
}

fn resolve_llm_config(value: Option<&Value>) -> Option<LlmConfig> {
    let explicitly_configured = value
        .and_then(Value::as_object)
        .is_some_and(|object| !object.is_empty());
    let api_key = if explicitly_configured {
        config_string(value, "api_key").unwrap_or_default()
    } else {
        env::var("OPENAI_API_KEY").ok().unwrap_or_default()
    };
    let base_url = if explicitly_configured {
        config_string(value, "base_url")?
    } else {
        env::var("OPENAI_BASE_URL")
            .ok()
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string())
    }
    .trim_end_matches('/')
    .to_string();
    let model = if explicitly_configured {
        config_string(value, "model")?
    } else {
        env::var("OPENAI_MODEL").ok().unwrap_or_default()
    };
    if model.is_empty() && api_key.is_empty() {
        return None;
    }
    Some(LlmConfig {
        api_key,
        base_url,
        model: if model.is_empty() {
            "gpt-4o-mini".to_string()
        } else {
            model
        },
        temperature: if explicitly_configured {
            value
                .and_then(|item| item.get("temperature"))
                .and_then(Value::as_f64)
        } else {
            env::var("OPENAI_TEMPERATURE")
                .ok()
                .and_then(|item| item.parse().ok())
        }
        .unwrap_or(0.2)
        .clamp(0.0, 2.0),
        timeout_seconds: if explicitly_configured {
            value
                .and_then(|item| item.get("timeout_seconds"))
                .and_then(Value::as_u64)
        } else {
            env::var("OPENAI_TIMEOUT_SECONDS")
                .ok()
                .and_then(|item| item.parse().ok())
        }
        .unwrap_or(45)
        .clamp(1, 180),
        json_mode: if explicitly_configured {
            value
                .and_then(|item| item.get("json_mode"))
                .and_then(Value::as_bool)
        } else {
            env::var("OPENAI_JSON_MODE")
                .ok()
                .and_then(|item| parse_bool(&item))
        }
        .unwrap_or(true),
        organization: if explicitly_configured {
            config_string(value, "organization")
        } else {
            env::var("OPENAI_ORG_ID").ok()
        },
        project: if explicitly_configured {
            config_string(value, "project")
        } else {
            env::var("OPENAI_PROJECT_ID").ok()
        },
    })
}

fn config_string(value: Option<&Value>, key: &str) -> Option<String> {
    value
        .and_then(|item| item.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn validate_llm_config(config: &LlmConfig) -> Result<(), String> {
    if config.base_url.len() > 2_048 {
        return Err("Agent LLM base URL exceeds 2048 bytes".to_string());
    }
    if config.model.chars().count() > 200 {
        return Err("Agent LLM model name exceeds 200 characters".to_string());
    }
    if config.api_key.len() > 8_192 {
        return Err("Agent LLM API key exceeds 8192 bytes".to_string());
    }
    let url = reqwest::Url::parse(&config.base_url)
        .map_err(|error| format!("Agent LLM base URL is invalid: {error}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Agent LLM base URL must use http or https".to_string());
    }
    if url.scheme() == "http" && !is_loopback_host(url.host_str()) {
        return Err(
            "Agent LLM remote endpoints must use https; http is limited to loopback".to_string(),
        );
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Agent LLM base URL must not contain credentials".to_string());
    }
    Ok(())
}

fn is_loopback_host(host: Option<&str>) -> bool {
    let Some(host) = host else {
        return false;
    };
    host.eq_ignore_ascii_case("localhost")
        || host.ends_with(".localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

pub(crate) fn should_retry_without_json_mode(error: &str) -> bool {
    let normalized = error.to_ascii_lowercase();
    let rejects_request_shape = normalized.contains("http 400") || normalized.contains("http 422");
    let identifies_json_mode = ["response_format", "json mode", "json_object", "json schema"]
        .iter()
        .any(|term| normalized.contains(term));
    rejects_request_shape && identifies_json_mode
}

async fn post_model_request(
    client: &reqwest::Client,
    config: &LlmConfig,
    request: &Value,
) -> Result<Value, String> {
    let endpoint = chat_completions_endpoint(&config.base_url);
    let body = serde_json::to_vec(request)
        .map_err(|error| format!("serialize Agent LLM request failed: {error}"))?;
    if body.len() > MAX_MODEL_REQUEST_BYTES {
        return Err("Agent LLM request exceeds 2 MiB".to_string());
    }
    let mut builder = client
        .post(endpoint)
        .header("Content-Type", "application/json")
        .body(body);
    if !config.api_key.is_empty() {
        builder = builder.header("Authorization", format!("Bearer {}", config.api_key));
    }
    if let Some(organization) = &config.organization {
        builder = builder.header("OpenAI-Organization", organization);
    }
    if let Some(project) = &config.project {
        builder = builder.header("OpenAI-Project", project);
    }
    let response = builder
        .send()
        .await
        .map_err(|error| format!("Agent LLM request failed: {error}"))?;
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|bytes| bytes > MAX_MODEL_RESPONSE_BYTES as u64)
    {
        return Err("Agent LLM response exceeds 2 MiB".to_string());
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("read Agent LLM response failed: {error}"))?;
        if body.len().saturating_add(chunk.len()) > MAX_MODEL_RESPONSE_BYTES {
            return Err("Agent LLM response exceeds 2 MiB".to_string());
        }
        body.extend_from_slice(&chunk);
    }
    let text = String::from_utf8_lossy(&body).into_owned();
    if !status.is_success() {
        return Err(format!(
            "Agent LLM HTTP {status}: {}",
            limit_chars(&text, 300)
        ));
    }
    let envelope: Value = serde_json::from_str(&text)
        .map_err(|error| format!("parse Agent LLM envelope failed: {error}"))?;
    let content = envelope
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .ok_or_else(|| "Agent LLM response is missing choices[0].message.content".to_string())?;
    parse_model_json(content)
}

fn chat_completions_endpoint(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/chat/completions") {
        base.to_string()
    } else {
        format!("{base}/chat/completions")
    }
}

fn parse_model_json(content: &str) -> Result<Value, String> {
    let trimmed = content.trim();
    let unwrapped = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|value| value.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed);
    serde_json::from_str(unwrapped).map_err(|error| {
        format!(
            "parse Agent model JSON failed: {error}: {}",
            limit_chars(unwrapped, 200)
        )
    })
}

fn build_evidence_catalog(tool_response: &Value) -> Vec<Value> {
    tool_response
        .get("evidence_summary")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .take(16)
                .enumerate()
                .map(|(index, item)| {
                    json!({
                        "id": format!("E{}", index + 1),
                        "title": limit_chars(item.get("title").and_then(Value::as_str).unwrap_or("本地证据"), 120),
                        "source": limit_chars(item.get("source").and_then(Value::as_str).unwrap_or("本地工具"), 160),
                        "level": limit_chars(item.get("level").and_then(Value::as_str).unwrap_or("evidence"), 40),
                        "summary": limit_chars(item.get("summary").and_then(Value::as_str).unwrap_or(""), 600),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn validate_model_evidence(model_response: &Value, evidence_count: usize) -> Result<(), String> {
    if evidence_count == 0 {
        return Err("agent model synthesis requires at least one local evidence item".to_string());
    }
    let reply = model_response
        .get("reply")
        .and_then(Value::as_str)
        .unwrap_or_default();
    validate_evidence_refs(reply, evidence_count)?;
    if let Some(sections) = model_response
        .get("answer_sections")
        .and_then(Value::as_array)
    {
        for section in sections {
            if let Some(bullets) = section.get("bullets").and_then(Value::as_array) {
                for bullet in bullets.iter().filter_map(Value::as_str) {
                    validate_evidence_refs(bullet, evidence_count)?;
                }
            }
        }
    }
    Ok(())
}

fn validate_evidence_refs(text: &str, evidence_count: usize) -> Result<(), String> {
    let mut found = false;
    for suffix in text.split("[E").skip(1) {
        let digits = suffix
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>();
        if digits.is_empty() || !suffix[digits.len()..].starts_with(']') {
            continue;
        }
        found = true;
        let id = digits
            .parse::<usize>()
            .map_err(|_| "agent model evidence citation is invalid".to_string())?;
        if id == 0 || id > evidence_count {
            return Err(format!("agent model referenced unknown evidence [E{id}]"));
        }
    }
    if !found {
        return Err("agent model statement is missing an adjacent evidence citation".to_string());
    }
    Ok(())
}

fn contains_forbidden_model_instruction(model_response: &Value) -> bool {
    let mut text = model_response
        .get("reply")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    for key in ["answer_sections", "warnings", "next_actions"] {
        if let Some(value) = model_response.get(key) {
            text.push_str(&value.to_string());
        }
    }
    let fixed_prohibitions = [
        "立即买入",
        "现在买入",
        "马上买入",
        "可以买入",
        "应当买入",
        "建议买入",
        "立即卖出",
        "现在卖出",
        "建议卖出",
        "必须卖出",
        "满仓",
        "清仓",
        "梭哈",
        "目标价",
        "稳赚",
        "稳赚不赔",
        "必涨",
        "保本",
        "无风险",
        "确定上涨",
        "收益承诺",
        "竞价介入",
        "止盈",
        "止损",
        "虚假申报",
        "拉抬股价",
        "对倒",
        "自买自卖",
        "账户协同",
        "利用未公开信息",
    ];
    if fixed_prohibitions
        .iter()
        .any(|phrase| text.contains(phrase))
    {
        return true;
    }
    let lowercase = text.to_ascii_lowercase();
    if [
        "buy now",
        "sell now",
        "recommend buying",
        "recommend selling",
        "should buy",
        "should sell",
        "buy this stock",
        "sell this stock",
        "go long",
        "go short",
        "guaranteed return",
        "price target",
    ]
    .iter()
    .any(|phrase| lowercase.contains(phrase))
    {
        return true;
    }
    text.split(['。', '！', '!', '？', '?', '；', ';', '\n'])
        .map(str::trim)
        .filter(|sentence| !sentence.is_empty())
        .any(is_direct_trading_sentence)
}

fn is_direct_trading_sentence(sentence: &str) -> bool {
    let action_terms = [
        "买入", "买进", "建仓", "加仓", "介入", "卖出", "卖掉", "减仓", "清仓", "退出", "持有",
    ];
    let directive_cues = [
        "建议", "应该", "应当", "可以", "适合", "推荐", "立即", "立刻", "现在", "马上", "务必",
        "必须", "直接", "明天", "今天", "今日", "开盘", "尾盘",
    ];
    let has_action = action_terms.iter().any(|term| sentence.contains(term));
    if has_action && directive_cues.iter().any(|cue| sentence.contains(cue)) {
        return true;
    }
    let has_position_number =
        (sentence.contains("仓位") || sentence.contains("持仓") || sentence.contains("资金参与"))
            && "0123456789一二三四五六七八九十半成%％"
                .chars()
                .any(|character| sentence.contains(character));
    if has_position_number
        || [
            "仓位建议",
            "重仓",
            "轻仓",
            "半仓",
            "上涨空间",
            "翻倍空间",
            "收益空间",
        ]
        .iter()
        .any(|term| sentence.contains(term))
    {
        return true;
    }
    for action in action_terms {
        if let Some(remainder) = sentence.strip_prefix(action) {
            let remainder = remainder.trim_start();
            let factual_suffix = [
                "金额", "数据", "行为", "记录", "席位", "比例", "占比", "资金", "统计", "信号",
                "净额", "期", "机制",
            ]
            .iter()
            .any(|suffix| remainder.starts_with(suffix));
            if !remainder.is_empty() && !factual_suffix {
                return true;
            }
        }
    }
    [
        "买入这",
        "买入该",
        "买进这",
        "买进该",
        "卖出这",
        "卖出该",
        "卖掉这",
        "卖掉该",
        "加仓这",
        "加仓该",
        "减仓这",
        "减仓该",
        "清仓",
        "重仓持有",
        "成仓",
    ]
    .iter()
    .any(|prefix| sentence.starts_with(prefix))
        || (has_action && sentence.ends_with('吧'))
}

fn sanitized_sections(value: Option<&Value>) -> Option<Vec<Value>> {
    let sections = value?.as_array()?;
    let result = sections
        .iter()
        .take(10)
        .filter_map(|section| {
            let title = section.get("title").and_then(Value::as_str)?.trim();
            if title.is_empty() {
                return None;
            }
            let bullets = sanitized_strings(section.get("bullets"), 8, 600).unwrap_or_default();
            if bullets.is_empty() {
                return None;
            }
            Some(json!({
                "title": limit_chars(title, 80),
                "bullets": bullets,
                "provenance": "model_inference",
                "evidence_basis": "使用 bullet 中邻近标注的本地证据编号",
            }))
        })
        .collect::<Vec<_>>();
    (!result.is_empty()).then_some(result)
}

fn sanitized_strings(value: Option<&Value>, limit: usize, chars: usize) -> Option<Vec<Value>> {
    let result = value?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .take(limit)
        .map(|item| Value::String(limit_chars(item, chars)))
        .collect::<Vec<_>>();
    (!result.is_empty()).then_some(result)
}

fn profile_for_mode(mode: &str) -> PromptProfile {
    match mode {
        "expert" => PromptProfile {
            id: "hot_money_early_v1",
            instructions: HOT_MONEY_PROMPT,
        },
        "research" => PromptProfile {
            id: "value_compounder_v1",
            instructions: VALUE_COMPOUNDER_PROMPT,
        },
        _ => PromptProfile {
            id: "deterministic_v1",
            instructions: "快速模式只整理工具事实，保持简洁，不引入人物方法。",
        },
    }
}

fn bounded_history(value: Option<&Value>) -> Vec<Value> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .rev()
                .filter_map(|item| {
                    let role = item.get("role").and_then(Value::as_str)?;
                    if !matches!(role, "user" | "assistant") {
                        return None;
                    }
                    let content = item.get("content").and_then(Value::as_str)?.trim();
                    if content.is_empty() {
                        return None;
                    }
                    Some(json!({
                        "role": role,
                        "content": limit_chars(content, MAX_HISTORY_CHARS),
                    }))
                })
                .take(MAX_HISTORY_MESSAGES)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect()
        })
        .unwrap_or_default()
}

fn bounded_context(value: Option<&Value>) -> Value {
    let watchlist = value
        .and_then(|item| item.get("watchlist"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .take(50)
                .filter_map(|item| {
                    let code = item.get("code").and_then(Value::as_str)?.trim();
                    if code.is_empty() {
                        return None;
                    }
                    Some(json!({
                        "code": limit_chars(code, 32),
                        "name": limit_chars(item.get("name").and_then(Value::as_str).unwrap_or("").trim(), 100),
                        "industry": item.get("industry").and_then(Value::as_str).map(|text| limit_chars(text.trim(), 100)),
                    }))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json!({"watchlist": watchlist})
}

fn compact_json(value: &Value, depth: usize) -> Value {
    if depth >= 6 {
        return Value::String("[内容已截断]".to_string());
    }
    match value {
        Value::String(text) => Value::String(limit_chars(text, 800)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .take(16)
                .map(|item| compact_json(item, depth + 1))
                .collect(),
        ),
        Value::Object(object) => {
            let mut compact = Map::new();
            for (key, item) in object.iter().take(48) {
                compact.insert(key.clone(), compact_json(item, depth + 1));
            }
            Value::Object(compact)
        }
        _ => value.clone(),
    }
}

fn limit_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}
