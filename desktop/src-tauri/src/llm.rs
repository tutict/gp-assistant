use crate::credentials;

use futures::stream::StreamExt;
use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::HashSet, time::Duration};
use zeroize::Zeroizing;

#[derive(Debug, Serialize)]
pub(crate) struct LlmDiagnosticError {
    pub(crate) code: &'static str,
    pub(crate) message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    endpoint: Option<String>,
    elapsed_ms: u128,
    api_format: &'static str,
}
impl std::fmt::Display for LlmDiagnosticError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}
impl std::error::Error for LlmDiagnosticError {}

struct LlmDiagnosticContext {
    started_at: u128,
    api_format: &'static str,
    endpoint: Option<reqwest::Url>,
    secrets: Vec<Zeroizing<String>>,
}
impl LlmDiagnosticContext {
    fn new(payload: &Value) -> Self {
        Self {
            started_at: crate::market::epoch_millis(),
            api_format: llm_api_format(payload),
            endpoint: None,
            secrets: payload
                .get("api_key")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|key| !key.is_empty())
                .map(|key| Zeroizing::new(key.to_owned()))
                .into_iter()
                .collect(),
        }
    }
    fn redact(&mut self, key: &str) {
        if !key.is_empty() && !self.secrets.iter().any(|secret| secret.as_str() == key) {
            self.secrets.push(Zeroizing::new(key.to_owned()));
        }
    }
    fn safe_endpoint(&self) -> Option<String> {
        self.endpoint.as_ref().map(|endpoint| {
            let mut safe = endpoint.clone();
            let _ = safe.set_username("");
            let _ = safe.set_password(None);
            safe.set_query(None);
            safe.set_fragment(None);
            let decoded_path = percent_encoding::percent_decode_str(safe.path())
                .decode_utf8_lossy()
                .into_owned();
            let mut path = decoded_path.clone();
            for secret in &self.secrets {
                path = path.replace(secret.as_str(), "[redacted]");
            }
            if path != decoded_path {
                safe.set_path(&path);
            }
            let mut text = safe.to_string();
            for secret in &self.secrets {
                text = text.replace(secret.as_str(), "[redacted]");
            }
            text
        })
    }
    fn error(&self, code: &'static str, message: impl Into<String>) -> LlmDiagnosticError {
        let endpoint = self.safe_endpoint();
        let mut message = message.into();
        for secret in &self.secrets {
            message = message.replace(secret.as_str(), "[redacted]");
        }
        LlmDiagnosticError {
            code,
            message,
            status: None,
            endpoint,
            elapsed_ms: crate::market::epoch_millis().saturating_sub(self.started_at),
            api_format: self.api_format,
        }
    }
    fn response_error(
        &self,
        status: u16,
        code: &'static str,
        message: impl Into<String>,
    ) -> LlmDiagnosticError {
        let mut error = self.error(code, message);
        error.status = Some(status);
        error
    }
    fn transport_error(&self, error: &reqwest::Error) -> LlmDiagnosticError {
        if error.is_timeout() {
            self.error("timeout", "请求超时，请检查网络、代理或调整超时时间。")
        } else {
            self.error(
                "network",
                "无法连接供应商，请检查接口地址、网络或代理设置。",
            )
        }
    }
}

fn llm_diagnostic_timeout(payload: &Value) -> u64 {
    crate::market::payload_usize_field(payload, "timeout_seconds", 60, 10, 180) as u64
}
async fn read_llm_diagnostic_response(
    response: reqwest::Response,
    context: &LlmDiagnosticContext,
) -> Result<Value, LlmDiagnosticError> {
    let status = response.status();
    let status_number = status.as_u16();
    if !status.is_success() {
        let (code, message) = match status_number {
            400 | 405 | 415 | 422 => (
                "invalid_config",
                "供应商不接受当前请求，请检查接口地址、协议、模型与高级参数。",
            ),
            401 | 403 => ("auth", "供应商鉴权失败，请检查密钥及账号权限。"),
            404 => (
                "not_found",
                "接口地址或模型未找到，请检查地址模式、协议和模型 ID。",
            ),
            429 => ("rate_limited", "供应商限制了请求，请检查额度并稍后重试。"),
            408 | 504 => ("timeout", "上游处理请求超时，请稍后重试或检查服务状态。"),
            _ => ("upstream", "供应商返回异常状态，请稍后重试或检查服务状态。"),
        };
        // Arbitrary provider response bodies can echo secrets, including URL credentials.
        return Err(context.response_error(status_number, code, message));
    }
    if response
        .content_length()
        .is_some_and(|bytes| bytes > crate::market::LLM_MODEL_LIST_MAX_BYTES as u64)
    {
        return Err(context.response_error(
            status_number,
            "invalid_response",
            "供应商响应过大，已停止读取。",
        ));
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| {
            let mut diagnostic = context.transport_error(&error);
            diagnostic.status = Some(status_number);
            diagnostic
        })?;
        if body.len().saturating_add(chunk.len()) > crate::market::LLM_MODEL_LIST_MAX_BYTES {
            return Err(context.response_error(
                status_number,
                "invalid_response",
                "供应商响应过大，已停止读取。",
            ));
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| {
        context.response_error(
            status_number,
            "invalid_response",
            "供应商返回了无效 JSON，请检查地址及接口协议。",
        )
    })
}

#[tauri::command]
pub(crate) async fn api_llm_models(payload: Value) -> Result<Value, LlmDiagnosticError> {
    let mut context = LlmDiagnosticContext::new(&payload);
    let base_url = payload
        .get("base_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| context.error("invalid_config", "请先填写供应商接口地址。"))?;
    let provider = payload
        .get("provider")
        .and_then(Value::as_str)
        .unwrap_or("openai-compatible")
        .trim()
        .to_ascii_lowercase();
    let endpoint = llm_models_endpoint(base_url, context.api_format)
        .map_err(|message| context.error("invalid_config", message))?;
    context.endpoint = Some(endpoint.clone());
    let resolved_key = credentials::resolve_api_key(&payload)
        .map_err(|_| context.error("credential", "系统密钥不可用，请重新输入或重试。"))?;
    let api_key = resolved_key.as_str();
    context.redact(api_key);
    let user_agent = payload
        .get("custom_user_agent")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("gp-assistant/0.4 llm-model-catalog");
    if user_agent.len() > 256 || user_agent.contains(['\r', '\n']) {
        return Err(context.error("invalid_config", "自定义 User-Agent 格式不正确。"));
    }
    let client = crate::market::build_http_client_with_proxy(
        user_agent,
        Duration::from_secs(llm_diagnostic_timeout(&payload)),
        Some(&payload),
    )
    .map_err(|_| context.error("invalid_config", "网络或代理配置无效，请调整后重试。"))?;
    let mut request = client
        .get(endpoint.clone())
        .header("Accept", "application/json");
    if llm_models_uses_anthropic_auth(base_url, context.api_format) {
        request = request.header("anthropic-version", "2023-06-01");
        if !api_key.is_empty() {
            request = request.header("x-api-key", api_key);
        }
    } else if !api_key.is_empty() {
        request = request.bearer_auth(api_key);
    }
    let response = request
        .send()
        .await
        .map_err(|error| context.transport_error(&error))?;
    let status = response.status().as_u16();
    let response_json = read_llm_diagnostic_response(response, &context).await?;
    if !response_json.get("data").is_some_and(Value::is_array)
        && !response_json.get("models").is_some_and(Value::is_array)
        && !response_json
            .get("result")
            .and_then(|value| value.get("data"))
            .is_some_and(Value::is_array)
    {
        return Err(context.response_error(
            status,
            "invalid_response",
            "模型列表响应格式不正确，可继续手动填写模型 ID。",
        ));
    }
    let models = parse_llm_model_options(&response_json);
    Ok(
        json!({ "provider": provider, "endpoint": context.safe_endpoint(), "count": models.len(), "models": models }),
    )
}

#[tauri::command]
pub(crate) async fn api_llm_test(payload: Value) -> Result<Value, LlmDiagnosticError> {
    let mut context = LlmDiagnosticContext::new(&payload);
    let base_url = payload
        .get("base_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| context.error("invalid_config", "请先填写供应商接口地址。"))?;
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| context.error("invalid_config", "请先填写默认模型。"))?;
    let full_url = payload.get("endpoint_mode").and_then(Value::as_str) == Some("full_url");
    let endpoint = llm_inference_endpoint(base_url, context.api_format, full_url)
        .map_err(|message| context.error("invalid_config", message))?;
    context.endpoint = Some(endpoint.clone());
    let resolved_key = credentials::resolve_api_key(&payload)
        .map_err(|_| context.error("credential", "系统密钥不可用，请重新输入或重试。"))?;
    let api_key = resolved_key.as_str();
    context.redact(api_key);
    let user_agent = payload
        .get("custom_user_agent")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("gp-assistant/0.4 llm-connection-test");
    if user_agent.len() > 256 || user_agent.contains(['\r', '\n']) {
        return Err(context.error("invalid_config", "自定义 User-Agent 格式不正确。"));
    }
    let client = crate::market::build_http_client_with_proxy(
        user_agent,
        Duration::from_secs(llm_diagnostic_timeout(&payload)),
        Some(&payload),
    )
    .map_err(|_| context.error("invalid_config", "网络或代理配置无效，请调整后重试。"))?;
    let json_mode = payload
        .get("json_mode")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let prompt = if json_mode {
        "Return a JSON object with ok=true."
    } else {
        "Reply with OK."
    };
    let mut body = match context.api_format {
        "openai_responses" => json!({ "model": model, "input": prompt, "max_output_tokens": 16 }),
        "anthropic_messages" => {
            json!({ "model": model, "messages": [{"role": "user", "content": prompt}], "max_tokens": 16 })
        }
        _ => {
            json!({ "model": model, "messages": [{"role": "user", "content": prompt}], "max_tokens": 16 })
        }
    };
    if let Some(temperature) = payload.get("temperature") {
        let maximum = if context.api_format == "anthropic_messages" {
            1.0
        } else {
            2.0
        };
        let temperature = temperature
            .as_f64()
            .filter(|value| value.is_finite() && *value >= 0.0 && *value <= maximum)
            .ok_or_else(|| context.error("invalid_config", "温度参数超出所选协议允许的范围。"))?;
        body["temperature"] = json!(temperature);
    }
    if json_mode {
        match context.api_format {
            "openai_responses" => body["text"] = json!({ "format": { "type": "json_object" } }),
            "openai_chat" => body["response_format"] = json!({ "type": "json_object" }),
            _ => {}
        }
    }
    let mut request = client
        .post(endpoint.clone())
        .header("Accept", "application/json")
        .json(&body);
    if context.api_format == "anthropic_messages" {
        request = request.header("anthropic-version", "2023-06-01");
        if !api_key.is_empty() {
            request = request.header("x-api-key", api_key);
        }
    } else if !api_key.is_empty() {
        request = request.bearer_auth(api_key);
    }
    let response = request
        .send()
        .await
        .map_err(|error| context.transport_error(&error))?;
    let status = response.status().as_u16();
    let response_json = read_llm_diagnostic_response(response, &context).await?;
    if !llm_test_response_has_content(&response_json, context.api_format) {
        return Err(context.response_error(
            status,
            "invalid_response",
            "供应商已响应，但内容为空或与所选协议不匹配。",
        ));
    }
    Ok(
        json!({ "ok": true, "endpoint": context.safe_endpoint(), "status": status,
        "elapsed_ms": crate::market::epoch_millis().saturating_sub(context.started_at), "api_format": context.api_format }),
    )
}

pub(crate) fn llm_inference_endpoint(
    base_url: &str,
    api_format: &str,
    full_url: bool,
) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse(base_url.trim())
        .map_err(|_| "供应商接口地址格式不正确。".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("供应商接口地址仅支持有效的 http 或 https 地址。".to_string());
    }
    if url.scheme() == "http" && !llm_plain_http_host_allowed(url.host_str()) {
        return Err("公网模型接口必须使用 HTTPS；HTTP 仅允许本机或私有局域网地址。".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("供应商接口地址不能包含用户名或密码。".to_string());
    }
    if full_url {
        if api_format == "anthropic_messages" {
            if url.query().is_some() || url.fragment().is_some() {
                return Err("Anthropic 完整 URL 不能包含查询参数或片段。".to_string());
            }
            if !url.path().trim_end_matches('/').ends_with("/v1/messages") {
                return Err("Anthropic 完整 URL 必须以 /v1/messages 结尾。".to_string());
            }
        }
        return Ok(url);
    }
    let suffix = match api_format {
        "openai_responses" => "/responses",
        "anthropic_messages" => "/v1/messages",
        _ => "/chat/completions",
    };
    let mut path = url.path().trim_end_matches('/').to_string();
    for known_suffix in [
        "/chat/completions",
        "/responses",
        "/v1/messages",
        "/messages",
    ] {
        if let Some(base) = path.strip_suffix(known_suffix) {
            path = base.to_string();
            break;
        }
    }
    if api_format == "anthropic_messages" {
        if let Some(base) = path.strip_suffix("/v1") {
            path = base.to_string();
        }
    }
    if !path.ends_with(suffix) {
        path.push_str(suffix);
    }
    if !path.starts_with('/') {
        path.insert(0, '/');
    }
    url.set_path(&path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

pub(crate) fn llm_test_response_has_content(response: &Value, api_format: &str) -> bool {
    match api_format {
        "openai_responses" => {
            response
                .get("output_text")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
                || response
                    .get("output")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|item| item.get("content").and_then(Value::as_array))
                    .flatten()
                    .any(|content| {
                        content
                            .get("text")
                            .and_then(Value::as_str)
                            .is_some_and(|text| !text.trim().is_empty())
                    })
        }
        "anthropic_messages" => response
            .get("content")
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items.iter().any(|item| {
                    item.get("text")
                        .and_then(Value::as_str)
                        .is_some_and(|text| !text.trim().is_empty())
                })
            }),
        _ => response
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .is_some_and(|text| !text.trim().is_empty()),
    }
}

pub(crate) fn llm_connection_http_error(status: u16, body: &[u8], api_key: &str) -> String {
    if matches!(status, 401 | 403) {
        return format!("供应商鉴权失败，请检查 API 密钥（HTTP {status}）。");
    }
    let detail = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|error| error.get("message").or(Some(error)))
                .and_then(Value::as_str)
                .or_else(|| value.get("message").and_then(Value::as_str))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| {
                    crate::market::truncate_for_note(&redact_llm_error_detail(value, api_key), 180)
                })
        });
    detail
        .map(|message| format!("供应商返回 HTTP {status}：{message}"))
        .unwrap_or_else(|| format!("供应商返回 HTTP {status}，连接测试失败。"))
}

pub(crate) fn llm_models_endpoint(
    base_url: &str,
    api_format: &str,
) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse(base_url.trim())
        .map_err(|_| "供应商接口地址格式不正确。".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("供应商接口地址仅支持 http 或 https。".to_string());
    }
    if url.host_str().is_none() {
        return Err("供应商接口地址缺少主机名。".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("供应商接口地址不能包含用户名或密码。".to_string());
    }
    if url.scheme() == "http" && !llm_plain_http_host_allowed(url.host_str()) {
        return Err("公网模型接口必须使用 HTTPS；HTTP 仅允许本机或私有局域网地址。".to_string());
    }

    if is_official_deepseek_anthropic_base_url(&url, api_format) {
        url.set_path("/models");
        url.set_query(None);
        url.set_fragment(None);
        return Ok(url);
    }

    let mut path = url.path().trim_end_matches('/').to_string();
    for suffix in [
        "/chat/completions",
        "/responses",
        "/v1/messages",
        "/messages",
        "/models",
    ] {
        if let Some(base) = path.strip_suffix(suffix) {
            path = base.to_string();
            break;
        }
    }
    if api_format == "anthropic_messages" {
        if let Some(base) = path.strip_suffix("/v1") {
            path = base.to_string();
        }
    }
    let suffix = if api_format == "anthropic_messages" {
        "/v1/models"
    } else {
        "/models"
    };
    if !path.ends_with(suffix) {
        path.push_str(suffix);
    }
    if path.is_empty() || !path.starts_with('/') {
        path.insert(0, '/');
    }
    url.set_path(&path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

pub(crate) fn llm_api_format(payload: &Value) -> &'static str {
    match payload.get("api_format").and_then(Value::as_str) {
        Some("openai_responses") => "openai_responses",
        Some("anthropic_messages") => "anthropic_messages",
        Some("openai_chat") => "openai_chat",
        None if payload
            .get("provider")
            .and_then(Value::as_str)
            .is_some_and(|provider| {
                provider.eq_ignore_ascii_case("anthropic-compatible")
                    || provider.eq_ignore_ascii_case("anthropic")
            }) =>
        {
            "anthropic_messages"
        }
        _ => "openai_chat",
    }
}

pub(crate) fn llm_models_uses_anthropic_auth(base_url: &str, api_format: &str) -> bool {
    api_format == "anthropic_messages"
        && !reqwest::Url::parse(base_url)
            .ok()
            .is_some_and(|url| is_official_deepseek_anthropic_base_url(&url, api_format))
}

pub(crate) fn is_official_deepseek_anthropic_base_url(
    url: &reqwest::Url,
    api_format: &str,
) -> bool {
    api_format == "anthropic_messages"
        && url
            .host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case("api.deepseek.com"))
        && url.path().trim_end_matches('/') == "/anthropic"
}

pub(crate) fn llm_plain_http_host_allowed(host: Option<&str>) -> bool {
    let Some(host) = host else {
        return false;
    };
    host.eq_ignore_ascii_case("localhost")
        || host.ends_with(".localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| match address {
                std::net::IpAddr::V4(address) => {
                    address.is_loopback() || address.is_private() || address.is_link_local()
                }
                std::net::IpAddr::V6(address) => {
                    address.is_loopback()
                        || address.is_unique_local()
                        || address.is_unicast_link_local()
                }
            })
}

pub(crate) fn parse_llm_model_options(response: &Value) -> Vec<Value> {
    let rows = response
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| response.get("models").and_then(Value::as_array))
        .or_else(|| {
            response
                .get("result")
                .and_then(|value| value.get("data"))
                .and_then(Value::as_array)
        });
    let Some(rows) = rows else {
        return Vec::new();
    };

    let mut seen = HashSet::new();
    let mut models = Vec::new();
    for row in rows {
        let id = row
            .as_str()
            .or_else(|| row.get("id").and_then(Value::as_str))
            .or_else(|| row.get("model").and_then(Value::as_str))
            .or_else(|| row.get("name").and_then(Value::as_str))
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.len() <= 256);
        let Some(id) = id else {
            continue;
        };
        if !seen.insert(id.to_string()) {
            continue;
        }
        let display_name = row
            .get("display_name")
            .and_then(Value::as_str)
            .or_else(|| row.get("name").and_then(Value::as_str))
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != id);
        let owned_by = row
            .get("owned_by")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        models.push(json!({
            "id": id,
            "name": display_name,
            "owned_by": owned_by,
        }));
    }
    models
}

pub(crate) fn llm_models_http_error(status: u16, body: &[u8], api_key: &str) -> String {
    match status {
        401 | 403 => return format!("供应商鉴权失败，请检查 API 密钥（HTTP {status}）。"),
        404 => {
            return "未找到模型列表接口（HTTP 404），请确认接口地址包含正确的版本路径，例如 /v1。"
                .to_string()
        }
        _ => {}
    }
    let detail = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|error| error.get("message").or(Some(error)))
                .and_then(Value::as_str)
                .or_else(|| value.get("message").and_then(Value::as_str))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| {
                    crate::market::truncate_for_note(&redact_llm_error_detail(value, api_key), 180)
                })
        });
    detail
        .map(|message| format!("供应商返回 HTTP {status}：{message}"))
        .unwrap_or_else(|| format!("供应商返回 HTTP {status}，未能拉取模型列表。"))
}

pub(crate) fn redact_llm_error_detail(detail: &str, api_key: &str) -> String {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return detail.to_string();
    }
    detail.replace(api_key, "[已隐藏密钥]")
}

#[cfg(test)]
mod credential_redaction_tests {
    use super::*;
    #[test]
    fn llm_test_rejects_empty_protocol_content() {
        for (format, response) in [
            (
                "openai_chat",
                json!({"choices":[{"message":{"content":" "}}]}),
            ),
            (
                "openai_responses",
                json!({"output":[{"content":[{"type":"output_text","text":""}]}]}),
            ),
            (
                "anthropic_messages",
                json!({"content":[{"type":"text","text":""}]}),
            ),
        ] {
            assert!(
                !llm_test_response_has_content(&response, format),
                "{format}"
            );
        }
    }
    #[test]
    fn redacts_short_and_long_resolved_keys_before_truncating_errors() {
        assert!(!redact_llm_error_detail("upstream echoed ab", "ab").contains("ab"));
        let key = "synthetic-key".repeat(30);
        let body =
            serde_json::to_vec(&json!({"error": {"message": format!("echo {key}")}})).unwrap();
        assert!(!llm_models_http_error(500, &body, &key).contains("synthetic-key"));
        assert!(!llm_connection_http_error(500, &body, &key).contains("synthetic-key"));
    }
}

#[cfg(test)]
mod diagnostic_contract_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread::{self, JoinHandle};

    fn fixture(status: u16, body: &str, delay: Duration) -> (String, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let body = body.to_owned();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            loop {
                let mut buffer = [0_u8; 2048];
                let count = stream.read(&mut buffer).unwrap();
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..count]);
                assert!(request.len() < 65536, "fixture request too large");
                if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                    let length = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            if !delay.is_zero() {
                thread::sleep(delay);
            }
            let _ = write!(stream, "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            String::from_utf8(request).unwrap()
        });
        (format!("http://{address}"), server)
    }

    #[test]
    fn llm_native_diagnostic_timeout_matches_editor_limits() {
        assert_eq!(llm_diagnostic_timeout(&json!({})), 60);
        assert_eq!(llm_diagnostic_timeout(&json!({"timeout_seconds": 1})), 10);
        assert_eq!(
            llm_diagnostic_timeout(&json!({"timeout_seconds": 300})),
            180
        );
    }

    #[test]
    fn llm_native_diagnostic_preserves_encoded_paths_without_secrets() {
        let mut context = LlmDiagnosticContext::new(&json!({}));
        let endpoint = "https://api.example/custom%2Fchat%2Fmodel";
        context.endpoint = Some(reqwest::Url::parse(endpoint).unwrap());
        assert_eq!(context.safe_endpoint().as_deref(), Some(endpoint));
    }

    #[test]
    fn llm_native_diagnostic_tests_all_protocols_and_unauthenticated_local_services() {
        for (format, response) in [
            (
                "openai_chat",
                r#"{"choices":[{"message":{"content":"OK"}}]}"#,
            ),
            (
                "openai_responses",
                r#"{"output":[{"content":[{"text":"OK"}]}]}"#,
            ),
            (
                "anthropic_messages",
                r#"{"content":[{"type":"text","text":"OK"}]}"#,
            ),
        ] {
            let (base, server) = fixture(200, response, Duration::ZERO);
            let result = tauri::async_runtime::block_on(api_llm_test(json!({
                "base_url": format!("{base}/v1"), "model": "local-fixture",
                "api_format": format, "temperature": 0.4, "json_mode": true,
            })))
            .unwrap();
            let request = server.join().unwrap();
            let headers = request
                .split("\r\n\r\n")
                .next()
                .unwrap()
                .to_ascii_lowercase();
            assert!(!headers.contains("authorization:"));
            assert!(!headers.contains("x-api-key:"));
            let body: Value =
                serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(body["temperature"], 0.4);
            match format {
                "openai_chat" => assert_eq!(body["response_format"]["type"], "json_object"),
                "openai_responses" => assert_eq!(body["text"]["format"]["type"], "json_object"),
                _ => assert!(body.get("response_format").is_none() && body.get("text").is_none()),
            }
            assert_eq!(result["ok"], true);
            assert_eq!(result["api_format"], format);
            assert_eq!(result["status"], 200);
        }
    }

    #[test]
    fn llm_native_diagnostic_http_errors_include_safe_context() {
        for (status, code) in [
            (400, "invalid_config"),
            (401, "auth"),
            (403, "auth"),
            (404, "not_found"),
            (405, "invalid_config"),
            (415, "invalid_config"),
            (422, "invalid_config"),
            (429, "rate_limited"),
            (500, "upstream"),
            (504, "timeout"),
        ] {
            let secret = "synthetic-key-in-path";
            let (base, server) = fixture(
                status,
                r#"{"error":"echo synthetic-key-in-path fixture-query-secret"}"#,
                Duration::ZERO,
            );
            let error = tauri::async_runtime::block_on(api_llm_test(json!({
                "base_url": format!("{base}/custom/{secret}?api_key=fixture-query-secret"),
                "endpoint_mode": "full_url", "model": "fixture", "api_key": secret,
            })))
            .unwrap_err();
            server.join().unwrap();
            assert_eq!(error.code, code);
            assert_eq!(error.status, Some(status));
            let serialized = serde_json::to_string(&error).unwrap();
            assert!(!serialized.contains(secret));
            assert!(!serialized.contains("fixture-query-secret"));
            assert!(error.endpoint.is_some());
        }
    }

    #[test]
    fn llm_native_diagnostic_success_hides_url_secrets_without_changing_the_request() {
        let secret = "synthetic-path-key";
        let (base, server) = fixture(
            200,
            r#"{"choices":[{"message":{"content":"OK"}}]}"#,
            Duration::ZERO,
        );
        let result = tauri::async_runtime::block_on(api_llm_test(json!({
            "base_url": format!("{base}/custom/{secret}?api_key=query-secret"),
            "endpoint_mode": "full_url", "api_key": secret, "model": "fixture",
        })))
        .unwrap();
        let request = server.join().unwrap();
        assert!(request.starts_with(&format!("POST /custom/{secret}?api_key=query-secret ")));
        let endpoint = result["endpoint"].as_str().unwrap();
        assert!(!endpoint.contains(secret));
        assert!(!endpoint.contains("query-secret"));
    }

    #[test]
    fn llm_native_diagnostic_reports_malformed_or_mismatched_responses() {
        for response in [
            "not-json",
            r#"{"content":[{"text":"OK"}]}"#,
            r#"{"choices":[{"message":{"content":" "}}]}"#,
        ] {
            let (base, server) = fixture(200, response, Duration::ZERO);
            let error = tauri::async_runtime::block_on(api_llm_test(
                json!({"base_url": format!("{base}/v1"), "model": "fixture"}),
            ))
            .unwrap_err();
            server.join().unwrap();
            assert_eq!(error.code, "invalid_response");
            assert_eq!(error.status, Some(200));
        }
    }

    #[test]
    fn llm_native_diagnostic_catalog_preserves_nested_result_data() {
        let (base, server) = fixture(
            200,
            r#"{"result":{"data":[{"id":"nested-model"}]}}"#,
            Duration::ZERO,
        );
        let result = tauri::async_runtime::block_on(api_llm_models(
            json!({"base_url": format!("{base}/v1")}),
        ))
        .unwrap();
        server.join().unwrap();
        assert_eq!(result["models"][0]["id"], "nested-model");
    }

    #[test]
    fn llm_native_diagnostic_categorizes_a_real_request_timeout() {
        let (base, server) = fixture(
            200,
            r#"{"choices":[{"message":{"content":"OK"}}]}"#,
            Duration::from_secs(11),
        );
        let error = tauri::async_runtime::block_on(api_llm_test(json!({
            "base_url": format!("{base}/v1"), "model": "fixture", "timeout_seconds": 10,
        })))
        .unwrap_err();
        server.join().unwrap();
        assert_eq!(error.code, "timeout");
        assert!(error.elapsed_ms >= 9000);
    }
}
