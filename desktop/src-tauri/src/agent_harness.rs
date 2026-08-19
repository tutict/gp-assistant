use crate::{build_http_client_with_proxy, runtime};
use futures::StreamExt;
use percent_encoding::percent_decode_str;
use serde_json::{json, Map, Value};
use std::{collections::HashMap, net::IpAddr, time::Duration};
use stock_optimizer_core as gp_core;

const BASE_PROMPT: &str = include_str!("../../../app/prompts/stock_soul.md");
const HOT_MONEY_PROMPT: &str = include_str!("../../../app/prompts/hot_money_early_v1.md");
const VALUE_COMPOUNDER_PROMPT: &str = include_str!("../../../app/prompts/value_compounder_v1.md");
const PROMPT_VERSION: &str = "agent-harness-v2.0";
const AGENT_POLICY_VERSION: &str = "agent-policy-v1";
const RISK_NOTICE: &str = "仅供选股研究，不构成投资建议。";
const MAX_HARNESS_PAYLOAD_BYTES: usize = 512 * 1024;
const MAX_MESSAGE_CHARS: usize = 8_000;
const MAX_HISTORY_MESSAGES: usize = 12;
const MAX_HISTORY_CHARS: usize = 2_000;
const MAX_MODEL_REQUEST_BYTES: usize = 2 * 1024 * 1024;
const MAX_MODEL_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_MERGED_REPLY_CHARS: usize = 6_000;
const MIN_TOOL_REPLY_CHARS: usize = 1_000;
const MAX_EVIDENCE_CATALOG_ITEMS: usize = 16;
const MAX_SECRET_PERCENT_DECODE_DEPTH: usize = 8;
const MAX_SECRET_REDACTION_PASSES: usize = 32;
const MAX_REDACTION_SECRETS: usize = 128;
const MAX_REDACTION_SECRET_BYTES: usize = 16 * 1024;

struct PromptProfile {
    id: &'static str,
    instructions: &'static str,
}

#[derive(Clone, Debug)]
struct RedactionSecret {
    value: String,
    bounded: bool,
    ascii_case_insensitive: bool,
    fail_closed: bool,
}

#[derive(Clone, Debug)]
struct LlmConfig {
    api_key: String,
    base_url: String,
    model: String,
    api_format: String,
    endpoint_mode: String,
    custom_user_agent: Option<String>,
    temperature: f64,
    timeout_seconds: u64,
    json_mode: bool,
    organization: Option<String>,
    project: Option<String>,
}

#[derive(Debug)]
struct ModelCallFailure {
    outcome: &'static str,
    message: String,
}

impl ModelCallFailure {
    fn policy(message: impl Into<String>) -> Self {
        Self {
            outcome: "policy_rejected",
            message: message.into(),
        }
    }

    fn request(message: impl Into<String>) -> Self {
        Self {
            outcome: "request_failed",
            message: message.into(),
        }
    }

    fn classify(message: String) -> Self {
        let policy_rejected = message.starts_with("Agent LLM request exceeds")
            || message.starts_with("Agent LLM response exceeds")
            || message.starts_with("Agent LLM endpoint is invalid")
            || message.starts_with("parse Agent LLM envelope failed")
            || message.starts_with("Agent LLM response is missing generated text")
            || message.starts_with("parse Agent model JSON failed");
        Self {
            outcome: if policy_rejected {
                "policy_rejected"
            } else {
                "request_failed"
            },
            message,
        }
    }
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
        "policy_version": AGENT_POLICY_VERSION,
        "profile_id": profile.id,
        "limits": {
            "max_message_chars": MAX_MESSAGE_CHARS,
            "max_history_messages": MAX_HISTORY_MESSAGES,
            "max_history_chars": MAX_HISTORY_CHARS,
            "max_evidence_items": MAX_EVIDENCE_CATALOG_ITEMS,
        },
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
    let target = tool_response
        .as_object_mut()
        .ok_or_else(|| "agent tool response must be an object".to_string())?;
    let raw_reply = model_response
        .get("reply")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "agent model response is missing reply".to_string())?;
    let tool_reply = target
        .get("reply")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let model_prefix = "\n\n模型综合：";
    let model_reply_limit = if let Some(tool_reply) = tool_reply.as_ref() {
        MAX_MERGED_REPLY_CHARS
            .saturating_sub(model_prefix.chars().count())
            .saturating_sub(tool_reply.chars().count().min(MIN_TOOL_REPLY_CHARS))
    } else {
        MAX_MERGED_REPLY_CHARS
    };
    let reply = limit_chars(raw_reply, model_reply_limit);
    let sections = sanitized_sections(model_response.get("answer_sections"));
    validate_model_evidence(
        &json!({
            "reply": reply.clone(),
            "answer_sections": sections.clone().unwrap_or_default(),
        }),
        evidence_count,
    )?;
    let combined_reply = if let Some(tool_reply) = tool_reply {
        let tool_reply_limit = MAX_MERGED_REPLY_CHARS
            .saturating_sub(model_prefix.chars().count())
            .saturating_sub(reply.chars().count());
        format!(
            "{}{model_prefix}{reply}",
            limit_chars(&tool_reply, tool_reply_limit)
        )
    } else {
        reply
    };
    target.insert("reply".to_string(), Value::String(combined_reply));

    if let Some(sections) = sections {
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
            "policy_version": AGENT_POLICY_VERSION,
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

/// Counts encoded bytes without materializing the encoding. `validate_payload` runs on every
/// request, once in the Tauri command (to gate the ledger insert) and once here, so allocating a
/// 512 KiB `Vec` twice per request just to read its length is pure waste.
struct EncodedByteCounter(usize);

impl std::io::Write for EncodedByteCounter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 += buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(crate) fn validate_payload(payload: &Value) -> Result<(), String> {
    let mut payload_bytes = EncodedByteCounter(0);
    serde_json::to_writer(&mut payload_bytes, payload)
        .map_err(|error| format!("serialize Agent payload failed: {error}"))?;
    if payload_bytes.0 > MAX_HARNESS_PAYLOAD_BYTES {
        return Err("Agent payload exceeds 512 KiB".to_string());
    }
    let message = payload
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if message.is_empty() {
        return Err("Agent message is required".to_string());
    }
    if message.chars().count() > MAX_MESSAGE_CHARS {
        return Err(format!(
            "Agent message exceeds the {MAX_MESSAGE_CHARS} character limit"
        ));
    }
    Ok(())
}

pub(crate) async fn execute_with_event_sink<F>(
    payload: Value,
    data: Value,
    mut sink: F,
) -> Result<AgentHarnessOutcome, String>
where
    F: FnMut(Value) + Send,
{
    validate_payload(&payload)?;
    let message = payload
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
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
        call_model_with_config(config.as_ref(), &preview).await
    } else {
        Ok(None)
    };
    let (response, model_outcome) = match model_result {
        Ok(Some(model_response)) => match merge_model_response(
            tool_response.clone(),
            &model_response,
            &profile_id,
            model_name,
        ) {
            Ok(response) => (response, "model_success"),
            Err(error) => (
                fallback_response(
                    tool_response,
                    &profile_id,
                    model_name,
                    Some(format!("模型输出未通过安全校验，已回退本地结果：{error}")),
                ),
                "policy_rejected",
            ),
        },
        Ok(None) => (
            fallback_response(tool_response, &profile_id, None, None),
            if should_call_model {
                "not_configured"
            } else {
                "not_requested"
            },
        ),
        Err(error) => (
            fallback_response(
                tool_response,
                &profile_id,
                model_name,
                Some(model_failure_warning(&error.message, config.as_ref())),
            ),
            error.outcome,
        ),
    };
    let response = annotate_harness_diagnostics(
        redact_response_for_config(response, config.as_ref()),
        &profile_id,
        config.as_ref(),
        model_outcome,
    );
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
                "policy_version": AGENT_POLICY_VERSION,
                "profile_id": profile_id,
                "model_used": false,
                "model": model_name,
            }),
        );
    }
    tool_response
}

fn annotate_harness_diagnostics(
    mut response: Value,
    profile_id: &str,
    config: Option<&LlmConfig>,
    model_outcome: &str,
) -> Value {
    if let Some(harness) = response
        .as_object_mut()
        .and_then(|object| object.get_mut("harness"))
        .and_then(Value::as_object_mut)
    {
        harness.insert(
            "policy_version".to_string(),
            Value::String(AGENT_POLICY_VERSION.to_string()),
        );
        harness.insert(
            "profile_id".to_string(),
            Value::String(profile_id.to_string()),
        );
        harness.insert(
            "model_outcome".to_string(),
            Value::String(model_outcome.to_string()),
        );
        harness.insert(
            "api_format".to_string(),
            Value::String(
                config
                    .map(|item| item.api_format.clone())
                    .unwrap_or_else(|| "none".to_string()),
            ),
        );
    }
    response
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

fn model_failure_warning(error: &str, config: Option<&LlmConfig>) -> String {
    format!(
        "模型调用失败，已回退本地结果：{}",
        redact_model_error(error, config)
    )
}

fn redact_model_error(error: &str, config: Option<&LlmConfig>) -> String {
    limit_chars(
        &redact_url_literals(&redact_text(
            error,
            &config
                .map(model_config_sensitive_values)
                .unwrap_or_default(),
        )),
        400,
    )
}

fn model_config_sensitive_values(config: &LlmConfig) -> Vec<RedactionSecret> {
    let mut secrets = Vec::new();
    push_redaction_secret(&mut secrets, &config.api_key);
    if let Some(organization) = config
        .organization
        .as_ref()
        .filter(|value| !value.is_empty())
    {
        push_redaction_secret(&mut secrets, organization);
    }
    if let Some(project) = config.project.as_ref().filter(|value| !value.is_empty()) {
        push_redaction_secret(&mut secrets, project);
    }
    if let Some(custom_user_agent) = config
        .custom_user_agent
        .as_ref()
        .filter(|value| !value.is_empty())
    {
        push_redaction_secret(&mut secrets, custom_user_agent);
    }
    push_url_redaction_secret(&mut secrets, &config.base_url, false);
    if redaction_is_fail_closed(&secrets) {
        return secrets;
    }
    let Ok(url) = reqwest::Url::parse(&config.base_url) else {
        return secrets;
    };
    push_url_redaction_secret(&mut secrets, url.as_str(), false);
    if let Some(host) = url.host_str().filter(|value| !value.is_empty()) {
        if is_sensitive_url_host(host) {
            push_ascii_case_insensitive_redaction_secret(&mut secrets, host, false);
            let components = host.split('.').collect::<Vec<_>>();
            for component in components {
                if !is_generic_url_component(component) {
                    push_ascii_case_insensitive_redaction_secret(&mut secrets, component, true);
                }
            }
        }
    }
    if !url.path().is_empty() && url.path() != "/" {
        if let Some(segments) = url.path_segments() {
            let sensitive_segments = segments
                .filter(|segment| !is_generic_url_component(segment))
                .collect::<Vec<_>>();
            if !sensitive_segments.is_empty() {
                push_url_redaction_secret(&mut secrets, url.path(), false);
            }
            for segment in sensitive_segments {
                push_url_redaction_secret(&mut secrets, segment, true);
            }
        }
    }
    if !url.username().is_empty() {
        push_url_redaction_secret(&mut secrets, url.username(), true);
    }
    if let Some(password) = url.password().filter(|value| !value.is_empty()) {
        push_url_redaction_secret(&mut secrets, password, false);
    }
    if let Some(query) = url.query().filter(|value| !value.is_empty()) {
        push_url_redaction_secret(&mut secrets, query, false);
        for pair in query.split('&') {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            push_url_redaction_secret(&mut secrets, key, true);
            push_url_redaction_secret(&mut secrets, value, false);
            if redaction_is_fail_closed(&secrets) {
                break;
            }
        }
    }
    if !redaction_is_fail_closed(&secrets) {
        for (key, value) in url.query_pairs() {
            push_url_redaction_secret(&mut secrets, &key, true);
            push_url_redaction_secret(&mut secrets, &value, false);
            if redaction_is_fail_closed(&secrets) {
                break;
            }
        }
    }
    if let Some(fragment) = url.fragment().filter(|value| !value.is_empty()) {
        push_url_redaction_secret(&mut secrets, fragment, false);
    }
    let mut without_fragment = url;
    without_fragment.set_fragment(None);
    push_url_redaction_secret(&mut secrets, without_fragment.as_str(), false);
    secrets
}

fn is_generic_url_component(value: &str) -> bool {
    let mut decoded = value.to_string();
    for _ in 0..=MAX_SECRET_PERCENT_DECODE_DEPTH {
        if matches!(
            decoded.to_ascii_lowercase().as_str(),
            "" | "api"
                | "chat"
                | "completions"
                | "model"
                | "models"
                | "responses"
                | "v1"
                | "v2"
                | "v3"
                | "www"
        ) {
            return true;
        }
        let next = percent_decode_str(&decoded)
            .decode_utf8_lossy()
            .into_owned();
        if next == decoded {
            break;
        }
        decoded = next;
    }
    false
}

fn is_sensitive_url_host(host: &str) -> bool {
    is_loopback_or_private_network_host(Some(host))
        || !host.contains('.')
        || [".internal", ".lan", ".local"]
            .iter()
            .any(|suffix| host.to_ascii_lowercase().ends_with(suffix))
}

fn push_redaction_secret(secrets: &mut Vec<RedactionSecret>, value: &str) {
    push_redaction_secret_with_flags(secrets, value, false, false);
}

fn push_bounded_redaction_secret(secrets: &mut Vec<RedactionSecret>, value: &str) {
    push_redaction_secret_with_flags(secrets, value, true, false);
}

fn push_ascii_case_insensitive_redaction_secret(
    secrets: &mut Vec<RedactionSecret>,
    value: &str,
    bounded: bool,
) {
    push_redaction_secret_with_flags(secrets, value, bounded, true);
}

fn push_redaction_secret_with_flags(
    secrets: &mut Vec<RedactionSecret>,
    value: &str,
    bounded: bool,
    ascii_case_insensitive: bool,
) {
    if value.is_empty() || redaction_is_fail_closed(secrets) {
        return;
    }
    let total_bytes = secrets.iter().fold(0usize, |total, secret| {
        total.saturating_add(secret.value.len())
    });
    if secrets.len() >= MAX_REDACTION_SECRETS
        || total_bytes.saturating_add(value.len()) > MAX_REDACTION_SECRET_BYTES
    {
        mark_redaction_fail_closed(secrets);
        return;
    }
    secrets.push(RedactionSecret {
        value: value.to_string(),
        bounded,
        ascii_case_insensitive,
        fail_closed: false,
    });
}

fn redaction_is_fail_closed(secrets: &[RedactionSecret]) -> bool {
    secrets.iter().any(|secret| secret.fail_closed)
}

fn mark_redaction_fail_closed(secrets: &mut Vec<RedactionSecret>) {
    secrets.clear();
    secrets.push(RedactionSecret {
        value: String::new(),
        bounded: false,
        ascii_case_insensitive: false,
        fail_closed: true,
    });
}

fn push_url_redaction_secret(secrets: &mut Vec<RedactionSecret>, value: &str, bounded: bool) {
    if bounded {
        push_bounded_redaction_secret(secrets, value);
    } else {
        push_redaction_secret(secrets, value);
    }
    if redaction_is_fail_closed(secrets) {
        return;
    }
    let mut decoded = value.to_string();
    for _ in 0..MAX_SECRET_PERCENT_DECODE_DEPTH {
        let next = percent_decode_str(&decoded)
            .decode_utf8_lossy()
            .into_owned();
        if next == decoded {
            break;
        }
        if bounded {
            push_bounded_redaction_secret(secrets, &next);
        } else {
            push_redaction_secret(secrets, &next);
        }
        decoded = next;
    }
    let next = percent_decode_str(&decoded)
        .decode_utf8_lossy()
        .into_owned();
    if next != decoded {
        mark_redaction_fail_closed(secrets);
    }
}

fn redact_text(value: &str, secrets: &[RedactionSecret]) -> String {
    if redaction_is_fail_closed(secrets) {
        return "***".to_string();
    }
    let mut result = value.to_string();
    let mut unique_secrets = HashMap::<String, RedactionSecret>::with_capacity(secrets.len());
    for secret in secrets.iter().cloned() {
        unique_secrets
            .entry(secret.value.clone())
            .and_modify(|existing| {
                existing.bounded &= secret.bounded;
                existing.ascii_case_insensitive |= secret.ascii_case_insensitive;
            })
            .or_insert(secret);
    }
    let mut unique_secrets = unique_secrets.into_values().collect::<Vec<_>>();
    unique_secrets.sort_by_key(|secret| std::cmp::Reverse(secret.value.len()));
    for secret in unique_secrets {
        let bounded = secret.bounded || secret.value.chars().count() < 4;
        result = if secret.ascii_case_insensitive {
            redact_ascii_case_insensitive_secret(&result, &secret.value, bounded)
        } else if bounded {
            redact_bounded_secret(&result, &secret.value)
        } else {
            result.replace(&secret.value, "***")
        };
        result = redact_percent_encoded_secret(
            &result,
            &secret.value,
            bounded,
            secret.ascii_case_insensitive,
        );
    }
    result
}

fn redact_ascii_case_insensitive_secret(value: &str, secret: &str, bounded: bool) -> String {
    let folded_value = value.to_ascii_lowercase();
    let folded_secret = secret.to_ascii_lowercase();
    let spans = folded_value
        .match_indices(&folded_secret)
        .filter_map(|(start, _)| {
            let end = start + secret.len();
            if bounded && !has_ascii_token_boundaries(value.as_bytes(), start, end) {
                return None;
            }
            Some((start, end))
        })
        .collect::<Vec<_>>();
    replace_byte_spans(value, &spans)
}

fn redact_percent_encoded_secret(
    value: &str,
    secret: &str,
    bounded: bool,
    ascii_case_insensitive: bool,
) -> String {
    let mut result = value.to_string();
    for _ in 0..MAX_SECRET_REDACTION_PASSES {
        let scan = percent_encoded_secret_spans(
            &result,
            secret.as_bytes(),
            bounded,
            ascii_case_insensitive,
        );
        if scan.depth_exhausted {
            return "***".to_string();
        }
        if scan.spans.is_empty() {
            break;
        }
        result = replace_byte_spans(&result, &scan.spans);
    }
    let remaining =
        percent_encoded_secret_spans(&result, secret.as_bytes(), bounded, ascii_case_insensitive);
    if remaining.depth_exhausted || !remaining.spans.is_empty() {
        "***".to_string()
    } else {
        result
    }
}

struct PercentEncodedSecretScan {
    spans: Vec<(usize, usize)>,
    depth_exhausted: bool,
}

fn percent_encoded_secret_spans(
    value: &str,
    secret: &[u8],
    bounded: bool,
    ascii_case_insensitive: bool,
) -> PercentEncodedSecretScan {
    if secret.is_empty() {
        return PercentEncodedSecretScan {
            spans: Vec::new(),
            depth_exhausted: false,
        };
    }
    let mut decoded = value.as_bytes().to_vec();
    let mut origins = (0..decoded.len())
        .map(|index| (index, index + 1))
        .collect::<Vec<_>>();
    for _ in 0..MAX_SECRET_PERCENT_DECODE_DEPTH {
        let (next, next_origins, changed) = percent_decode_bytes_with_origins(&decoded, &origins);
        if !changed {
            return PercentEncodedSecretScan {
                spans: Vec::new(),
                depth_exhausted: false,
            };
        }
        decoded = next;
        origins = next_origins;
        let mut spans = Vec::new();
        let mut cursor = 0;
        while cursor + secret.len() <= decoded.len() {
            let candidate = &decoded[cursor..cursor + secret.len()];
            let matches = if ascii_case_insensitive {
                candidate.eq_ignore_ascii_case(secret)
            } else {
                candidate == secret
            };
            if matches
                && (!bounded || has_ascii_token_boundaries(&decoded, cursor, cursor + secret.len()))
            {
                spans.push((origins[cursor].0, origins[cursor + secret.len() - 1].1));
                cursor += secret.len();
            } else {
                cursor += 1;
            }
        }
        if !spans.is_empty() {
            spans.sort_unstable();
            spans.dedup();
            return PercentEncodedSecretScan {
                spans,
                depth_exhausted: false,
            };
        }
    }
    let (_, _, depth_exhausted) = percent_decode_bytes_with_origins(&decoded, &origins);
    PercentEncodedSecretScan {
        spans: Vec::new(),
        depth_exhausted,
    }
}

fn percent_decode_bytes_with_origins(
    value: &[u8],
    origins: &[(usize, usize)],
) -> (Vec<u8>, Vec<(usize, usize)>, bool) {
    let mut decoded = Vec::with_capacity(value.len());
    let mut decoded_origins = Vec::with_capacity(origins.len());
    let mut cursor = 0;
    let mut changed = false;
    while cursor < value.len() {
        let decoded_byte = if cursor + 2 < value.len() && value[cursor] == b'%' {
            match (hex_value(value[cursor + 1]), hex_value(value[cursor + 2])) {
                (Some(high), Some(low)) => Some((high << 4) | low),
                _ => None,
            }
        } else {
            None
        };
        if let Some(decoded_byte) = decoded_byte {
            decoded.push(decoded_byte);
            decoded_origins.push((origins[cursor].0, origins[cursor + 2].1));
            cursor += 3;
            changed = true;
        } else {
            decoded.push(value[cursor]);
            decoded_origins.push(origins[cursor]);
            cursor += 1;
        }
    }
    (decoded, decoded_origins, changed)
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn has_ascii_token_boundaries(value: &[u8], start: usize, end: usize) -> bool {
    let before_is_token = start > 0 && is_secret_token_byte(value[start - 1]);
    let after_is_token = end < value.len() && is_secret_token_byte(value[end]);
    !before_is_token && !after_is_token
}

fn is_secret_token_byte(value: u8) -> bool {
    value.is_ascii_alphanumeric() || matches!(value, b'_' | b'-')
}

fn replace_byte_spans(value: &str, spans: &[(usize, usize)]) -> String {
    if spans.is_empty() {
        return value.to_string();
    }
    let mut redacted = value.to_string();
    for &(start, end) in spans.iter().rev() {
        if redacted.is_char_boundary(start) && redacted.is_char_boundary(end) {
            redacted.replace_range(start..end, "***");
        }
    }
    redacted
}

fn redact_bounded_secret(value: &str, secret: &str) -> String {
    let mut redacted = String::with_capacity(value.len());
    let mut cursor = 0;
    let mut changed = false;
    for (start, _) in value.match_indices(secret) {
        if start < cursor {
            continue;
        }
        let end = start + secret.len();
        let before_is_token = value[..start]
            .chars()
            .next_back()
            .is_some_and(is_secret_token_char);
        let after_is_token = value[end..]
            .chars()
            .next()
            .is_some_and(is_secret_token_char);
        if before_is_token || after_is_token {
            continue;
        }
        redacted.push_str(&value[cursor..start]);
        redacted.push_str("***");
        cursor = end;
        changed = true;
    }
    if !changed {
        return value.to_string();
    }
    redacted.push_str(&value[cursor..]);
    redacted
}

fn is_secret_token_char(value: char) -> bool {
    value.is_ascii_alphanumeric() || matches!(value, '_' | '-')
}

fn redact_response_for_config(value: Value, config: Option<&LlmConfig>) -> Value {
    let secrets = config
        .map(model_config_sensitive_values)
        .unwrap_or_default();
    redact_json_strings_with_urls(value, &secrets)
}

pub(crate) fn redact_persisted_response(response: &Value, llm_value: Option<&Value>) -> Value {
    let config = resolve_llm_config(llm_value);
    redact_response_for_config(response.clone(), config.as_ref())
}

pub(crate) fn redact_persisted_events(events: &[Value], llm_value: Option<&Value>) -> Vec<Value> {
    let config = resolve_llm_config(llm_value);
    let secrets = config
        .as_ref()
        .map(model_config_sensitive_values)
        .unwrap_or_default();
    events
        .iter()
        .cloned()
        .map(|event| redact_json_strings_with_urls(event, &secrets))
        .collect()
}

pub(crate) fn redact_persisted_error(error: &str, llm_value: Option<&Value>) -> String {
    let config = resolve_llm_config(llm_value);
    redact_model_error(error, config.as_ref())
}

pub(crate) fn redact_persisted_question(question: &str, llm_value: Option<&Value>) -> String {
    let config = resolve_llm_config(llm_value);
    let secrets = config
        .as_ref()
        .map(model_config_sensitive_values)
        .unwrap_or_default();
    redact_url_literals(&redact_text(question, &secrets))
}

fn redact_json_strings_with_urls(value: Value, secrets: &[RedactionSecret]) -> Value {
    match value {
        Value::String(value) => Value::String(redact_url_literals(&redact_text(&value, secrets))),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| redact_json_strings_with_urls(item, secrets))
                .collect(),
        ),
        Value::Object(items) => Value::Object(
            items
                .into_iter()
                .map(|(key, item)| (key, redact_json_strings_with_urls(item, secrets)))
                .collect(),
        ),
        value => value,
    }
}

fn redact_url_literals(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    while cursor < value.len() {
        let remainder = &value[cursor..];
        let normalized_remainder = remainder.to_ascii_lowercase();
        let next = [
            normalized_remainder.find("https://"),
            normalized_remainder.find("http://"),
        ]
        .into_iter()
        .flatten()
        .min();
        let Some(offset) = next else {
            output.push_str(remainder);
            break;
        };
        let start = cursor + offset;
        output.push_str(&value[cursor..start]);
        let tail = &value[start..];
        let end = tail
            .find(char::is_whitespace)
            .map(|length| start + length)
            .unwrap_or(value.len());
        let token = &value[start..end];
        let trailing_len = token
            .trim_end_matches(|character| matches!(character, '.' | ',' | ';' | ')' | ']' | '}'))
            .len();
        let url_token = &token[..trailing_len];
        if url_token
            .parse::<reqwest::Url>()
            .ok()
            .is_some_and(|url| persisted_url_is_sensitive(&url))
        {
            output.push_str("***");
        } else {
            output.push_str(url_token);
        }
        output.push_str(&token[trailing_len..]);
        cursor = start + token.len();
    }
    output
}

fn persisted_url_is_sensitive(url: &reqwest::Url) -> bool {
    let host = url.host_str().unwrap_or_default();
    !url.username().is_empty()
        || url.password().is_some()
        || is_sensitive_url_host(host)
        || host.ends_with(".internal")
        || host.ends_with(".local")
        || host.ends_with(".lan")
        || url.query_pairs().any(|(key, _)| {
            let key = key.to_ascii_lowercase().replace('-', "_").replace('.', "_");
            [
                "api_key",
                "apikey",
                "auth",
                "bearer",
                "cookie",
                "credential",
                "key",
                "password",
                "secret",
                "session",
                "sig",
                "signature",
                "token",
            ]
            .iter()
            .any(|marker| key == *marker || key.ends_with(&format!("_{marker}")))
        })
}

fn format_reqwest_error(context: &str, error: reqwest::Error) -> String {
    format!("{context}: {}", error.without_url())
}

#[cfg(test)]
pub(crate) async fn call_model(
    llm_value: Option<&Value>,
    preview: &Value,
) -> Result<Option<Value>, String> {
    let config = resolve_llm_config(llm_value);
    call_model_with_config(config.as_ref(), preview)
        .await
        .map_err(|error| error.message)
}

async fn call_model_with_config(
    config: Option<&LlmConfig>,
    preview: &Value,
) -> Result<Option<Value>, ModelCallFailure> {
    let Some(config) = config else {
        return Ok(None);
    };
    validate_llm_config(&config).map_err(ModelCallFailure::policy)?;
    let client = build_http_client_with_proxy(
        config.custom_user_agent.as_deref().unwrap_or(concat!(
            "Mozilla/5.0 GuXuanYou/",
            env!("CARGO_PKG_VERSION"),
            " agent-harness"
        )),
        Duration::from_secs(config.timeout_seconds),
        None,
    )
    .map_err(|error| {
        ModelCallFailure::request(format!("create Agent LLM client failed: {error}"))
    })?;
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

    match post_model_request(&client, &config, &request)
        .await
        .map_err(ModelCallFailure::classify)
    {
        Ok(value) => Ok(Some(value)),
        Err(first_error)
            if config.json_mode && should_retry_without_json_mode(&first_error.message) =>
        {
            if let Some(object) = request.as_object_mut() {
                object.remove("response_format");
            }
            post_model_request(&client, &config, &request)
                .await
                .map(Some)
                .map_err(|second_error| {
                    let second_error = ModelCallFailure::classify(second_error);
                    ModelCallFailure {
                        outcome: second_error.outcome,
                        message: format!(
                            "{}; Agent model retry without JSON mode failed: {}",
                            first_error.message, second_error.message
                        ),
                    }
                })
        }
        Err(error) => Err(error),
    }
}

fn resolve_llm_config(value: Option<&Value>) -> Option<LlmConfig> {
    let value = value
        .and_then(Value::as_object)
        .filter(|object| !object.is_empty())?;
    let api_key = config_string(value, "api_key").unwrap_or_default();
    let configured_base_url = config_string(value, "base_url");
    if api_key.is_empty() && configured_base_url.is_none() {
        return None;
    }
    let base_url = configured_base_url
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string())
        .trim_end_matches('/')
        .to_string();
    let model = config_string(value, "model")?;
    let api_format = normalize_api_format(config_string(value, "api_format").as_deref());
    let endpoint_mode = if config_string(value, "endpoint_mode").as_deref() == Some("full_url") {
        "full_url".to_string()
    } else {
        "base_url".to_string()
    };
    Some(LlmConfig {
        api_key,
        base_url,
        model,
        api_format,
        endpoint_mode,
        custom_user_agent: config_string(value, "custom_user_agent"),
        temperature: value
            .get("temperature")
            .and_then(Value::as_f64)
            .unwrap_or(0.2)
            .clamp(0.0, 2.0),
        timeout_seconds: value
            .get("timeout_seconds")
            .and_then(Value::as_u64)
            .unwrap_or(45)
            .clamp(1, 180),
        json_mode: value
            .get("json_mode")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        organization: config_string(value, "organization"),
        project: config_string(value, "project"),
    })
}

fn config_string(value: &Map<String, Value>, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
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
    if config
        .custom_user_agent
        .as_ref()
        .is_some_and(|value| value.len() > 256 || value.contains(['\r', '\n']))
    {
        return Err("Agent LLM custom User-Agent is invalid".to_string());
    }
    let url = reqwest::Url::parse(&config.base_url)
        .map_err(|error| format!("Agent LLM base URL is invalid: {error}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Agent LLM base URL must use http or https".to_string());
    }
    if url.scheme() == "http" && !is_loopback_or_private_network_host(url.host_str()) {
        return Err(
            "Agent LLM remote endpoints must use https; http is limited to loopback or private LAN IP addresses".to_string(),
        );
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Agent LLM base URL must not contain credentials".to_string());
    }
    Ok(())
}

fn is_loopback_or_private_network_host(host: Option<&str>) -> bool {
    let Some(host) = host else {
        return false;
    };
    host.eq_ignore_ascii_case("localhost")
        || host.ends_with(".localhost")
        || host.parse::<IpAddr>().is_ok_and(|address| match address {
            IpAddr::V4(address) => {
                address.is_loopback() || address.is_private() || address.is_link_local()
            }
            IpAddr::V6(address) => {
                address.is_loopback()
                    || address.is_unique_local()
                    || address.is_unicast_link_local()
            }
        })
}
pub(crate) fn should_retry_without_json_mode(error: &str) -> bool {
    let normalized = error.to_ascii_lowercase();
    let rejects_request_shape = normalized.contains("http 400") || normalized.contains("http 422");
    let identifies_json_mode = [
        "response_format",
        "text.format",
        "json mode",
        "json_object",
        "json schema",
    ]
    .iter()
    .any(|term| normalized.contains(term));
    rejects_request_shape && identifies_json_mode
}

async fn post_model_request(
    client: &reqwest::Client,
    config: &LlmConfig,
    request: &Value,
) -> Result<Value, String> {
    let endpoint = crate::llm_inference_endpoint(
        &config.base_url,
        &config.api_format,
        config.endpoint_mode == "full_url",
    )
    .map_err(|error| format!("Agent LLM endpoint is invalid: {error}"))?;
    let outbound_request = adapt_llm_request(request, &config.api_format);
    let body = serde_json::to_vec(&outbound_request)
        .map_err(|error| format!("serialize Agent LLM request failed: {error}"))?;
    if body.len() > MAX_MODEL_REQUEST_BYTES {
        return Err("Agent LLM request exceeds 2 MiB".to_string());
    }
    let mut builder = client
        .post(endpoint)
        .header("Content-Type", "application/json")
        .body(body);
    if config.api_format == "anthropic_messages" {
        builder = builder.header("anthropic-version", "2023-06-01");
        if !config.api_key.is_empty() {
            builder = builder.header("x-api-key", &config.api_key);
        }
    } else if !config.api_key.is_empty() {
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
        .map_err(|error| format_reqwest_error("Agent LLM request failed", error))?;
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
        let chunk =
            chunk.map_err(|error| format_reqwest_error("read Agent LLM response failed", error))?;
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
    let content = llm_response_content(&envelope, &config.api_format)
        .ok_or_else(|| "Agent LLM response is missing generated text".to_string())?;
    parse_model_json(content)
}

fn normalize_api_format(value: Option<&str>) -> String {
    match value {
        Some("openai_responses") => "openai_responses",
        Some("anthropic_messages") => "anthropic_messages",
        _ => "openai_chat",
    }
    .to_string()
}

fn adapt_llm_request(request: &Value, api_format: &str) -> Value {
    if api_format == "openai_chat" {
        return request.clone();
    }
    if api_format == "openai_responses" {
        let mut adapted = json!({
            "model": request.get("model"),
            "input": request.get("messages").cloned().unwrap_or_else(|| json!([])),
            "temperature": request.get("temperature"),
        });
        if let Some(response_format) = request.get("response_format") {
            adapted["text"] = json!({"format": response_format});
        }
        return adapted;
    }

    let messages = request
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let system = messages
        .iter()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("system"))
        .filter_map(|message| message.get("content").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n\n");
    let messages = messages
        .into_iter()
        .filter(|message| message.get("role").and_then(Value::as_str) != Some("system"))
        .collect::<Vec<_>>();
    json!({
        "model": request.get("model"),
        "system": system,
        "messages": messages,
        "temperature": request.get("temperature"),
        "max_tokens": 4096,
    })
}

fn llm_response_content<'a>(envelope: &'a Value, api_format: &str) -> Option<&'a str> {
    match api_format {
        "openai_responses" => envelope
            .get("output_text")
            .and_then(Value::as_str)
            .or_else(|| {
                envelope
                    .get("output")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|item| item.get("content").and_then(Value::as_array))
                    .flatten()
                    .find_map(|content| content.get("text").and_then(Value::as_str))
            }),
        "anthropic_messages" => {
            envelope
                .get("content")
                .and_then(Value::as_array)
                .and_then(|items| {
                    items
                        .iter()
                        .find_map(|item| item.get("text").and_then(Value::as_str))
                })
        }
        _ => envelope
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str),
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
                .take(MAX_EVIDENCE_CATALOG_ITEMS)
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
    if reply.trim().is_empty() {
        return Err("agent model synthesis is missing a reply".to_string());
    }
    if !validate_evidence_refs(reply, evidence_count)? {
        return Err("agent model reply is missing an evidence citation".to_string());
    }
    if let Some(sections) = model_response
        .get("answer_sections")
        .and_then(Value::as_array)
    {
        for section in sections {
            if let Some(bullets) = section.get("bullets").and_then(Value::as_array) {
                for bullet in bullets.iter().filter_map(Value::as_str) {
                    if !validate_evidence_refs(bullet, evidence_count)? {
                        return Err("agent model factual bullet is missing an evidence citation"
                            .to_string());
                    }
                }
            }
        }
    }
    Ok(())
}

fn validate_evidence_refs(text: &str, evidence_count: usize) -> Result<bool, String> {
    let mut found = false;
    let mut remaining = text;
    while let Some(index) = remaining.find("[E") {
        let suffix = &remaining[index + 2..];
        let digit_count = suffix
            .bytes()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if digit_count == 0 || !suffix[digit_count..].starts_with(']') {
            return Err("agent model evidence citation is invalid".to_string());
        }
        found = true;
        let id = suffix[..digit_count]
            .parse::<usize>()
            .map_err(|_| "agent model evidence citation is invalid".to_string())?;
        if id == 0 || id > evidence_count {
            return Err(format!("agent model referenced unknown evidence [E{id}]"));
        }
        remaining = &suffix[digit_count + 1..];
    }
    Ok(found)
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
    if fixed_prohibitions.iter().any(|phrase| {
        text.split([
            '\u{3002}', '\u{ff01}', '!', '\u{ff1f}', '?', '\u{ff1b}', ';', '，', ',', '\n',
        ])
        .map(str::trim)
        .any(|sentence| sentence.contains(phrase) && !is_negated_prohibition(sentence, phrase))
    }) {
        return true;
    }
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
    .any(|phrase| {
        text.split([
            '\u{3002}', '\u{ff01}', '!', '\u{ff1f}', '?', '\u{ff1b}', ';', '，', ',', '\n',
        ])
        .map(str::trim)
        .any(|sentence| {
            sentence.to_ascii_lowercase().contains(phrase)
                && !is_negated_prohibition(sentence, phrase)
        })
    }) {
        return true;
    }
    text.split(['。', '！', '!', '？', '?', '；', ';', '\n'])
        .map(str::trim)
        .filter(|sentence| !sentence.is_empty())
        .any(is_direct_trading_sentence)
}

fn is_negated_prohibition(sentence: &str, phrase: &str) -> bool {
    let prefix = sentence
        .find(phrase)
        .map(|index| &sentence[..index])
        .unwrap_or(sentence);
    has_compliance_negation(prefix)
}

fn has_compliance_negation(text: &str) -> bool {
    [
        "不构成",
        "不提供",
        "不输出",
        "不作",
        "不得",
        "禁止",
        "严禁",
        "避免",
        "不要",
        "勿",
        "不建议",
    ]
    .iter()
    .any(|cue| text.contains(cue))
        || ["do not", "don't", "not a", "no ", "without"]
            .iter()
            .any(|cue| text.to_ascii_lowercase().contains(cue))
}

fn is_factual_trading_observation(sentence: &str) -> bool {
    ["主力资金", "北向资金", "机构资金", "成交数据", "资金流"]
        .iter()
        .any(|subject| sentence.contains(subject))
        && [
            "净买入",
            "净卖出",
            "买入金额",
            "卖出金额",
            "买入占比",
            "卖出占比",
        ]
        .iter()
        .any(|metric| sentence.contains(metric))
}
fn is_direct_trading_sentence(sentence: &str) -> bool {
    sentence
        .split(['，', ','])
        .map(str::trim)
        .filter(|clause| !clause.is_empty())
        .any(is_direct_trading_clause)
}

fn is_direct_trading_clause(sentence: &str) -> bool {
    let action_terms = [
        "买入", "买进", "建仓", "加仓", "介入", "卖出", "卖掉", "减仓", "清仓", "退出", "持有",
    ];
    let directive_cues = [
        "建议", "应该", "应当", "可以", "适合", "推荐", "立即", "立刻", "现在", "马上", "务必",
        "必须", "直接", "明天", "今天", "今日", "开盘", "尾盘",
    ];
    let has_action = action_terms.iter().any(|term| sentence.contains(term));
    if has_action
        && directive_cues.iter().any(|cue| sentence.contains(cue))
        && !has_compliance_negation(sentence)
        && !is_factual_trading_observation(sentence)
    {
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

#[cfg(test)]
mod harness_validation_tests {
    use super::*;

    fn llm_config(base_url: &str) -> LlmConfig {
        LlmConfig {
            api_key: String::new(),
            base_url: base_url.to_string(),
            model: "test-model".to_string(),
            api_format: "openai_chat".to_string(),
            endpoint_mode: "base_url".to_string(),
            custom_user_agent: None,
            temperature: 0.2,
            timeout_seconds: 30,
            json_mode: true,
            organization: None,
            project: None,
        }
    }

    async fn unresponsive_loopback_full_url() -> (tokio::net::TcpListener, String) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback port should be available");
        let address = listener
            .local_addr()
            .expect("the loopback listener should have an address");
        let url =
            format!("http://{address}/v1/chat?replay_token_key=replay-secret#fragment-secret");
        (listener, url)
    }

    #[test]
    fn validates_agent_payload_before_persistence() {
        let oversized = json!({
            "message": "screen the watchlist",
            "padding": "x".repeat(MAX_HARNESS_PAYLOAD_BYTES)
        });
        let error = validate_payload(&oversized)
            .expect_err("oversized Agent payloads must be rejected before persistence");
        assert!(error.contains("512 KiB"));
    }

    #[test]
    fn publishes_stable_governance_metadata_for_prompt_and_result() {
        let payload = json!({
            "mode": "expert",
            "message": "检查当前证据和风险"
        });
        let tool_response = json!({
            "reply": "本地工具已返回证据。",
            "evidence_summary": [{"title": "公告", "excerpt": "公告证据"}],
            "warnings": []
        });
        let preview = prompt_preview(&payload, &tool_response);
        assert_eq!(preview["prompt_version"], PROMPT_VERSION);
        assert_eq!(preview["policy_version"], AGENT_POLICY_VERSION);
        assert_eq!(preview["profile_id"], "hot_money_early_v1");
        assert_eq!(
            preview["limits"]["max_evidence_items"],
            MAX_EVIDENCE_CATALOG_ITEMS
        );

        let merged = merge_model_response(
            tool_response,
            &json!({"reply": "结论仍需核验。[E1]"}),
            "hot_money_early_v1",
            Some("test-model"),
        )
        .expect("model response should pass governance checks");
        assert_eq!(merged["harness"]["prompt_version"], PROMPT_VERSION);
        assert_eq!(merged["harness"]["policy_version"], AGENT_POLICY_VERSION);
        assert_eq!(merged["harness"]["profile_id"], "hot_money_early_v1");
    }

    #[test]
    fn classifies_local_and_model_execution_outcomes_for_governance() {
        let quick = tauri::async_runtime::block_on(execute(
            json!({
                "message": "查看自选股",
                "run_id": "outcome-quick",
                "mode": "quick",
                "llm": {"base_url": "http://127.0.0.1:9/v1", "model": "unused"},
                "context": {"watchlist": [{"code": "000001.SZ"}]}
            }),
            json!({}),
        ))
        .expect("quick execution should succeed");
        assert_eq!(quick.response["harness"]["model_outcome"], "not_requested");
        assert_eq!(quick.response["harness"]["api_format"], "none");

        let unconfigured = tauri::async_runtime::block_on(execute(
            json!({
                "message": "查看自选股",
                "run_id": "outcome-unconfigured",
                "mode": "expert",
                "context": {"watchlist": [{"code": "000001.SZ"}]}
            }),
            json!({}),
        ))
        .expect("unconfigured execution should fall back locally");
        assert_eq!(
            unconfigured.response["harness"]["model_outcome"],
            "not_configured"
        );
        assert_eq!(unconfigured.response["harness"]["api_format"], "none");

        for message in [
            "Agent LLM request exceeds 2 MiB",
            "Agent LLM response exceeds 2 MiB",
            "Agent LLM endpoint is invalid: unsupported endpoint",
            "parse Agent LLM envelope failed: invalid JSON",
            "Agent LLM response is missing generated text",
            "parse Agent model JSON failed: invalid JSON",
        ] {
            assert_eq!(
                ModelCallFailure::classify(message.to_string()).outcome,
                "policy_rejected",
                "{message}"
            );
        }
        assert_eq!(
            ModelCallFailure::classify("Agent LLM request failed: timeout".to_string()).outcome,
            "request_failed"
        );
    }

    #[test]
    fn classifies_local_model_configuration_rejections_as_policy_failures() {
        let mut invalid_user_agent = llm_config("https://models.example.test/v1");
        invalid_user_agent.custom_user_agent = Some("invalid\r\nuser-agent".to_string());
        let invalid_configs = [
            llm_config("http://models.example.test/v1"),
            llm_config("https://user:pass@models.example.test/v1"),
            invalid_user_agent,
        ];

        for config in invalid_configs {
            let failure =
                tauri::async_runtime::block_on(call_model_with_config(Some(&config), &json!({})))
                    .expect_err("local configuration rejection must not issue a request");
            assert_eq!(failure.outcome, "policy_rejected", "{}", failure.message);
        }
    }

    #[test]
    fn only_uses_explicit_agent_llm_configuration() {
        assert!(resolve_llm_config(None).is_none());
        assert!(resolve_llm_config(Some(&json!({}))).is_none());
        assert!(resolve_llm_config(Some(&json!({"model": "gpt-4o-mini"}))).is_none());
        let config = resolve_llm_config(Some(&json!({
            "api_key": "test-key",
            "model": "gpt-4o-mini"
        })))
        .expect("an explicit model config should resolve");
        assert_eq!(config.base_url, "https://api.openai.com/v1");
        assert_eq!(config.model, "gpt-4o-mini");
    }

    #[test]
    fn strips_full_url_from_transport_errors_at_request_and_call_boundaries() {
        tauri::async_runtime::block_on(async {
            let (_listener, full_url) = unresponsive_loopback_full_url().await;
            let mut config = llm_config(&full_url);
            config.endpoint_mode = "full_url".to_string();
            config.json_mode = false;
            config.timeout_seconds = 1;
            let client = build_http_client_with_proxy(
                "GuXuanYou agent harness test",
                Duration::from_secs(1),
                None,
            )
            .expect("the test HTTP client should build");

            let request_error =
                post_model_request(&client, &config, &json!({"model": "test-model"}))
                    .await
                    .expect_err("the released loopback port should reject the request");
            let call_error = call_model(
                Some(&json!({
                    "api_key": "api-key-secret",
                    "base_url": full_url,
                    "model": "test-model",
                    "api_format": "openai_chat",
                    "endpoint_mode": "full_url",
                    "json_mode": false,
                    "timeout_seconds": 1
                })),
                &json!({}),
            )
            .await
            .expect_err("the model call should preserve the transport failure");

            for error in [&request_error, &call_error] {
                assert!(error.contains("Agent LLM request failed"), "{error}");
                for secret in [
                    "replay-secret",
                    "replay_token_key",
                    "fragment-secret",
                    full_url.as_str(),
                ] {
                    assert!(!error.contains(secret), "leaked {secret:?} in {error:?}");
                }
            }
        });
    }

    #[test]
    fn redacts_api_key_and_structured_full_url_secrets_defensively() {
        let mut config = llm_config(
            "https://url-user:url-pass@example.test/v1/%74enant-secret/chat?replay_token_key=query-secret#fragment-secret",
        );
        config.api_key = "api-key-secret".to_string();
        config.endpoint_mode = "full_url".to_string();
        config.organization = Some("organization-secret".to_string());
        config.project = Some("project-secret".to_string());
        let error = format!(
            "request failed for {}; key api-key-secret; organization organization-secret; project project-secret; components url-user url-pass tenant-secret query-secret fragment-secret; 路径tenant-secret无效",
            config.base_url
        );

        let redacted = redact_model_error(&error, Some(&config));

        assert!(redacted.contains("request failed"));
        for secret in [
            "api-key-secret",
            config.base_url.as_str(),
            "url-user",
            "url-pass",
            "tenant-secret",
            "replay_token_key",
            "query-secret",
            "fragment-secret",
            "organization-secret",
            "project-secret",
        ] {
            assert!(
                !redacted.contains(secret),
                "leaked {secret:?} in {redacted:?}"
            );
        }
        assert!(redacted.contains("路径***无效"));
    }

    #[test]
    fn fallback_warning_does_not_expose_model_configuration_secrets() {
        let mut config = llm_config(
            "https://example.test/v1/path-secret?replay_token_key=query-secret#fragment-secret",
        );
        config.api_key = "api-key-secret".to_string();
        config.endpoint_mode = "full_url".to_string();
        config.organization = Some("organization-secret".to_string());
        config.project = Some("project-secret".to_string());
        let raw_error = format!(
            "Agent LLM request failed for {} with api-key-secret, organization organization-secret, and project project-secret",
            config.base_url
        );
        let response = fallback_response(
            json!({"warnings": []}),
            "value_compounder_v1",
            Some("test-model"),
            Some(model_failure_warning(&raw_error, Some(&config))),
        );
        let warnings = response["warnings"].to_string();

        assert!(warnings.contains("模型调用失败"));
        for secret in [
            "api-key-secret",
            config.base_url.as_str(),
            "replay_token_key",
            "query-secret",
            "fragment-secret",
            "organization-secret",
            "project-secret",
        ] {
            assert!(
                !warnings.contains(secret),
                "leaked {secret:?} in {warnings:?}"
            );
        }
    }

    #[test]
    fn redacts_configuration_values_from_live_model_output() {
        let mut config = llm_config(
            "https://example.test/v1/path-secret/chat?replay_token_key=query-secret#fragment-secret",
        );
        config.api_key = "api-key-secret".to_string();
        config.organization = Some("organization-secret".to_string());
        config.project = Some("project-secret".to_string());
        config.custom_user_agent = Some("user-agent-secret".to_string());
        let result = redact_response_for_config(
            json!({
                "reply": "api-key-secret query-secret replay_token_key path-secret user-agent-secret",
                "answer_sections": [{"bullets": ["organization-secret project-secret"]}],
                "warnings": ["https://example.test/v1/path-secret/chat?replay_token_key=query-secret#fragment-secret"]
            }),
            Some(&config),
        );
        let serialized = result.to_string();

        for secret in [
            "api-key-secret",
            "replay_token_key",
            "query-secret",
            "fragment-secret",
            "path-secret",
            "organization-secret",
            "project-secret",
            "user-agent-secret",
        ] {
            assert!(
                !serialized.contains(secret),
                "leaked {secret:?} in {serialized:?}"
            );
        }
    }

    #[test]
    fn redacts_unknown_sensitive_urls_from_live_model_output_case_insensitively() {
        let result = redact_response_for_config(
            json!({
                "reply": "upstream HTTPS://user:pass@PRIVATE.MODEL.TEST/v1/chat?X-API-KEY=query-secret failed",
                "warnings": ["public https://evidence.example/docs?page=2"]
            }),
            None,
        );
        let serialized = result.to_string();

        for secret in ["user:pass", "PRIVATE.MODEL.TEST", "query-secret"] {
            assert!(
                !serialized.contains(secret),
                "leaked {secret:?} in {serialized:?}"
            );
        }
        assert!(serialized.contains("https://evidence.example/docs?page=2"));
    }

    #[test]
    fn redacts_unknown_urls_from_persisted_errors_and_events() {
        let error =
            "request failed for HTTPS://user:pass@private.model.test/v1/chat?api_key=query-secret";
        let redacted_error = redact_persisted_error(error, None);
        assert!(!redacted_error.contains("user:pass"));
        assert!(!redacted_error.contains("private.model.test"));
        assert!(!redacted_error.contains("query-secret"));

        let events = redact_persisted_events(
            &[json!({
                "type": "error",
                "message": "upstream HTTP://user:pass@private.model.test/v1/chat?token=query-secret failed"
            })],
            None,
        );
        let serialized = serde_json::to_string(&events).expect("events should serialize");
        assert!(!serialized.contains("user:pass"));
        assert!(!serialized.contains("private.model.test"));
        assert!(!serialized.contains("query-secret"));

        let response = redact_persisted_response(
            &json!({
                "reply": "provider https://user:pass@private.model.test/v1/chat?token=query-secret"
            }),
            None,
        );
        let serialized = response.to_string();
        assert!(!serialized.contains("user:pass"));
        assert!(!serialized.contains("private.model.test"));
        assert!(!serialized.contains("query-secret"));
    }

    #[test]
    fn redacts_percent_encoded_secret_variants_from_model_output() {
        let config = llm_config("https://example.test/v1?token=abc-def");
        let result = redact_response_for_config(
            json!({
                "reply": "abc%2Ddef abc%2ddef abc%252Ddef abc%25252Ddef abc%2525252Ddef",
                "warnings": ["prefix-abc%2Ddef-suffix"]
            }),
            Some(&config),
        );
        let serialized = result.to_string();

        for encoded_secret in [
            "abc%2Ddef",
            "abc%2ddef",
            "abc%252Ddef",
            "abc%25252Ddef",
            "abc%2525252Ddef",
        ] {
            assert!(
                !serialized.contains(encoded_secret),
                "leaked {encoded_secret:?} in {serialized:?}"
            );
        }
        assert!(serialized.contains("prefix-***-suffix"));
    }

    #[test]
    fn redacts_raw_and_percent_encoded_plus_in_query_secrets() {
        let config = llm_config("https://example.test/v1?token=abc+def");
        let result = redact_response_for_config(
            json!({"reply": "abc+def abc%2Bdef abc%2bdef abc def"}),
            Some(&config),
        );
        let serialized = result.to_string();

        for secret in ["abc+def", "abc%2Bdef", "abc%2bdef", "abc def"] {
            assert!(
                !serialized.contains(secret),
                "leaked {secret:?} in {serialized:?}"
            );
        }
    }

    #[test]
    fn redacts_plaintext_from_deeply_encoded_configuration_secrets() {
        let config = llm_config("https://example.test/v1?token=abc%2525252Ddef");
        let result =
            redact_response_for_config(json!({"reply": "provider echoed abc-def"}), Some(&config));

        assert_eq!(result["reply"], "provider echoed ***");
    }

    #[test]
    fn fails_closed_when_configuration_encoding_exceeds_the_decode_budget() {
        let mut encoded_secret = "abc%2Ddef".to_string();
        for _ in 0..MAX_SECRET_PERCENT_DECODE_DEPTH {
            encoded_secret = encoded_secret.replace('%', "%25");
        }
        let config = llm_config(&format!("https://example.test/v1?token={encoded_secret}"));
        let result = redact_response_for_config(
            json!({"reply": "otherwise harmless model output"}),
            Some(&config),
        );

        assert_eq!(result["reply"], "***");
    }

    #[test]
    fn fails_closed_when_query_secrets_exceed_the_collection_budget() {
        let query = (0..MAX_REDACTION_SECRETS)
            .map(|index| format!("key-{index}=value-{index}"))
            .collect::<Vec<_>>()
            .join("&");
        let config = llm_config(&format!("https://example.test/v1?{query}"));
        let secrets = model_config_sensitive_values(&config);
        let result = redact_response_for_config(
            json!({"reply": "otherwise harmless model output"}),
            Some(&config),
        );

        assert_eq!(secrets.len(), 1);
        assert!(secrets[0].fail_closed);
        assert_eq!(result["reply"], "***");
    }

    #[test]
    fn fails_closed_when_percent_encoding_exceeds_the_redaction_budget() {
        let config = llm_config("https://example.test/v1?token=abc-def");
        let mut encoded_secret = "abc%2Ddef".to_string();
        for _ in 0..MAX_SECRET_PERCENT_DECODE_DEPTH {
            encoded_secret = encoded_secret.replace('%', "%25");
        }
        let result = redact_response_for_config(
            json!({"reply": format!("prefix {encoded_secret} suffix")}),
            Some(&config),
        );

        assert_eq!(result["reply"], "***");
    }

    #[test]
    fn redacts_private_hosts_case_insensitively() {
        let config = llm_config("https://model.internal/v1");
        let result = redact_response_for_config(
            json!({"reply": "MODEL.INTERNAL and MoDeL.InTeRnAl are private"}),
            Some(&config),
        );
        let serialized = result.to_string().to_ascii_lowercase();

        assert!(!serialized.contains("model.internal"), "{serialized}");
        assert!(
            serialized.contains("*** and *** are private"),
            "{serialized}"
        );
    }

    #[test]
    fn redacts_low_entropy_values_only_at_token_boundaries() {
        let mut config = llm_config("https://example.test/v1");
        config.api_key = "x".to_string();
        config.organization = Some("a".to_string());
        config.project = Some("b".to_string());
        config.custom_user_agent = Some("ua".to_string());
        let result = redact_response_for_config(
            json!({"reply": "analysis x a b ua xylophone remains readable; 前x后"}),
            Some(&config),
        );

        assert_eq!(
            result["reply"],
            "analysis *** *** *** *** xylophone remains readable; 前***后"
        );
    }

    #[test]
    fn keeps_public_url_components_that_are_not_configuration_secrets() {
        for base_url in [
            "https://api.openai.com/v1/chat/completions",
            "https://api.openai.com/%76%31/chat/completions",
        ] {
            let config = llm_config(base_url);
            let result = redact_response_for_config(
                json!({
                    "reply": "Latest comparison cites https://api.openai.com/docs and https://evidence.com/v1/chat/completions."
                }),
                Some(&config),
            );

            assert_eq!(
                result["reply"],
                "Latest comparison cites https://api.openai.com/docs and https://evidence.com/v1/chat/completions."
            );
        }
    }

    #[test]
    fn allows_plaintext_loopback_and_private_lan_endpoints_only() {
        for base_url in ["http://127.0.0.1:11434/v1", "http://192.168.1.20:11434/v1"] {
            assert!(
                validate_llm_config(&llm_config(base_url)).is_ok(),
                "{base_url}"
            );
        }
        let error = validate_llm_config(&llm_config("http://models.example.test/v1"))
            .expect_err("public plaintext endpoints must remain blocked");
        assert!(error.contains("must use https"));
    }

    #[test]
    fn adapts_endpoints_requests_and_responses_for_supported_protocols() {
        assert_eq!(
            crate::llm_inference_endpoint(
                "https://api.example/v1/chat/completions",
                "openai_responses",
                false
            )
            .unwrap()
            .as_str(),
            "https://api.example/v1/responses"
        );
        assert_eq!(
            crate::llm_inference_endpoint(
                "https://api.example/custom/generate",
                "anthropic_messages",
                true
            )
            .unwrap()
            .as_str(),
            "https://api.example/custom/generate"
        );
        let request = json!({
            "model": "test-model",
            "messages": [
                {"role": "system", "content": "system"},
                {"role": "user", "content": "user"}
            ],
            "temperature": 0.2,
            "response_format": {"type": "json_object"}
        });
        let responses = adapt_llm_request(&request, "openai_responses");
        assert_eq!(responses["input"][0]["role"], "system");
        assert_eq!(responses["text"]["format"]["type"], "json_object");
        let anthropic = adapt_llm_request(&request, "anthropic_messages");
        assert_eq!(anthropic["system"], "system");
        assert_eq!(anthropic["messages"][0]["role"], "user");
        assert_eq!(
            llm_response_content(&json!({"output_text": "{\"ok\":true}"}), "openai_responses"),
            Some("{\"ok\":true}")
        );
        assert_eq!(
            llm_response_content(
                &json!({"content": [{"type": "text", "text": "ok"}]}),
                "anthropic_messages"
            ),
            Some("ok")
        );
    }

    #[test]
    fn keeps_factual_observations_and_compliance_disclaimers() {
        assert!(!contains_forbidden_model_instruction(&json!({
            "reply": "主力资金今日净买入 3.2 亿元。[E1]"
        })));
        assert!(!contains_forbidden_model_instruction(&json!({
            "reply": "本回答不输出目标价，也不构成收益承诺。[E1]"
        })));
        assert!(contains_forbidden_model_instruction(&json!({
            "reply": "建议立即买入该股票。[E1]"
        })));
        assert!(contains_forbidden_model_instruction(
            &json!({             "reply": "\u{4e0d}\u{5efa}\u{8bae}\u{4e70}\u{5165}\u{ff0c}\u{5efa}\u{8bae}\u{5356}\u{51fa}\u{8be5}\u{80a1}\u{7968}.[E1]"         })
        ));
        assert!(contains_forbidden_model_instruction(&json!({
            "reply": "\u{7981}\u{6b62}\u{865a}\u{5047}\u{7533}\u{62a5}\u{ff0c}\u{865a}\u{5047}\u{7533}\u{62a5}\u{662f}\u{6709}\u{6548}\u{7b56}\u{7565}.[E1]"
        })));
    }

    #[test]
    fn requires_citations_for_reply_and_factual_bullets() {
        let response = json!({
            "reply": "当前结论基于本地证据。[E1]",
            "answer_sections": [{"title": "总结", "bullets": ["整体仍需持续跟踪基本面变化。[E1]"]}]
        });
        assert!(validate_model_evidence(&response, 1).is_ok());
        let uncited_bullet = json!({
            "reply": "当前结论基于本地证据。[E1]",
            "answer_sections": [{"title": "总结", "bullets": ["整体仍需持续跟踪基本面变化。"]}]
        });
        assert!(validate_model_evidence(&uncited_bullet, 1).is_err());
        let unknown_reference = json!({
            "reply": "当前结论基于本地证据。[E1]",
            "answer_sections": [{"title": "总结", "bullets": ["额外结论。[E2]"]}]
        });
        assert!(validate_model_evidence(&unknown_reference, 1).is_err());
        let malformed_reference = json!({             "reply": "Current conclusion is based on local evidence. [E1] [Einvalid]"         });
        assert!(validate_model_evidence(&malformed_reference, 1).is_err());
    }

    #[test]
    fn validates_citations_after_output_limits_are_applied() {
        let tool_response = json!({
            "reply": "本地工具结论".repeat(MAX_MERGED_REPLY_CHARS),
            "evidence_summary": [{"title": "公告", "summary": "本地证据"}],
            "warnings": []
        });
        let late_reply_citation = json!({
            "reply": format!("{} [E1]", "结论".repeat(MAX_MERGED_REPLY_CHARS)),
        });
        assert!(merge_model_response(
            tool_response.clone(),
            &late_reply_citation,
            "value_compounder_v1",
            Some("test-model"),
        )
        .is_err());

        let late_bullet_citation = json!({
            "reply": "结论仍需核验。[E1]",
            "answer_sections": [{
                "title": "总结",
                "bullets": [format!("{} [E1]", "事实".repeat(600))]
            }]
        });
        assert!(merge_model_response(
            tool_response.clone(),
            &late_bullet_citation,
            "value_compounder_v1",
            Some("test-model"),
        )
        .is_err());

        let bounded = merge_model_response(
            tool_response,
            &json!({
                "reply": format!("[E1] {}", "结论".repeat(MAX_MERGED_REPLY_CHARS)),
                "answer_sections": [{
                    "title": "总结",
                    "bullets": [format!("[E1] {}", "事实".repeat(600))]
                }]
            }),
            "value_compounder_v1",
            Some("test-model"),
        )
        .expect("citations inside the published bounds should remain valid");
        let reply = bounded["reply"].as_str().expect("reply should be text");
        assert!(reply.starts_with("本地工具结论"));
        assert!(reply.contains("[E1]"));
        assert!(reply.chars().count() <= MAX_MERGED_REPLY_CHARS);
        assert!(bounded["model_answer_sections"][0]["bullets"][0]
            .as_str()
            .is_some_and(|bullet| bullet.contains("[E1]")));
    }

    #[test]
    fn bounds_the_evidence_catalog_to_the_addressable_range() {
        let tool_response = json!({
            "evidence_summary": (0..20)
                .map(|index| json!({"title": format!("evidence-{index}")}))
                .collect::<Vec<_>>()
        });
        assert_eq!(
            build_evidence_catalog(&tool_response).len(),
            MAX_EVIDENCE_CATALOG_ITEMS
        );
    }
}
