use futures::stream::StreamExt;
use serde_json::{json, Value};
use std::{
    collections::{HashSet},
    time::{Duration},
};

#[tauri::command]
pub(crate) async fn api_llm_models(payload: Value) -> Result<Value, String> {
    let base_url = payload
        .get("base_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "请先填写供应商接口地址。".to_string())?;
    let provider = payload
        .get("provider")
        .and_then(Value::as_str)
        .unwrap_or("openai-compatible")
        .trim()
        .to_ascii_lowercase();
    let api_format = llm_api_format(&payload);
    let api_key = payload
        .get("api_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let timeout_seconds = crate::market::payload_usize_field(&payload, "timeout_seconds", 60, 10, 120);
    let user_agent = payload
        .get("custom_user_agent")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("gp-assistant/0.4 llm-model-catalog");
    if user_agent.len() > 256 || user_agent.contains(['\r', '\n']) {
        return Err("自定义 User-Agent 格式不正确。".to_string());
    }
    let endpoint = llm_models_endpoint(base_url, api_format)?;
    let client = crate::market::build_http_client_with_proxy(
        user_agent,
        Duration::from_secs(timeout_seconds as u64),
        Some(&payload),
    )?;

    let mut request = client
        .get(endpoint.clone())
        .header("Accept", "application/json");
    if llm_models_uses_anthropic_auth(base_url, api_format) {
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
        .map_err(|error| format!("连接供应商失败：{error}"))?;
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|bytes| bytes > crate::market::LLM_MODEL_LIST_MAX_BYTES as u64)
    {
        return Err("供应商返回的模型列表过大，已停止读取。".to_string());
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("读取供应商模型列表失败：{error}"))?;
        if body.len().saturating_add(chunk.len()) > crate::market::LLM_MODEL_LIST_MAX_BYTES {
            return Err("供应商返回的模型列表过大，已停止读取。".to_string());
        }
        body.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        return Err(llm_models_http_error(status.as_u16(), &body, api_key));
    }

    let response_json: Value = serde_json::from_slice(&body)
        .map_err(|error| format!("供应商模型列表不是有效 JSON：{error}"))?;
    let models = parse_llm_model_options(&response_json);
    Ok(json!({
        "provider": provider,
        "endpoint": endpoint.to_string(),
        "count": models.len(),
        "models": models,
    }))
}

#[tauri::command]
pub(crate) async fn api_llm_test(payload: Value) -> Result<Value, String> {
    let base_url = payload
        .get("base_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "请先填写供应商接口地址。".to_string())?;
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "请先填写默认模型。".to_string())?;
    let api_key = payload
        .get("api_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let api_format = llm_api_format(&payload);
    let full_url = payload.get("endpoint_mode").and_then(Value::as_str) == Some("full_url");
    let endpoint = llm_inference_endpoint(base_url, api_format, full_url)?;
    let custom_user_agent = payload
        .get("custom_user_agent")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("gp-assistant/0.4 llm-connection-test");
    if custom_user_agent.len() > 256 || custom_user_agent.contains(['\r', '\n']) {
        return Err("自定义 User-Agent 格式不正确。".to_string());
    }
    let timeout_seconds = crate::market::payload_usize_field(&payload, "timeout_seconds", 30, 5, 120);
    let client = crate::market::build_http_client_with_proxy(
        custom_user_agent,
        Duration::from_secs(timeout_seconds as u64),
        Some(&payload),
    )?;
    let body = match api_format {
        "openai_responses" => json!({
            "model": model,
            "input": "Reply with OK.",
            "max_output_tokens": 16,
        }),
        "anthropic_messages" => json!({
            "model": model,
            "messages": [{"role": "user", "content": "Reply with OK."}],
            "max_tokens": 16,
        }),
        _ => json!({
            "model": model,
            "messages": [{"role": "user", "content": "Reply with OK."}],
            "max_tokens": 16,
        }),
    };
    let body =
        serde_json::to_vec(&body).map_err(|error| format!("序列化连接测试请求失败：{error}"))?;
    let mut request = client
        .post(endpoint.clone())
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .body(body);
    if api_format == "anthropic_messages" {
        request = request.header("anthropic-version", "2023-06-01");
        if !api_key.is_empty() {
            request = request.header("x-api-key", api_key);
        }
    } else if !api_key.is_empty() {
        request = request.bearer_auth(api_key);
    }

    let started_at = crate::market::epoch_millis();
    let response = request
        .send()
        .await
        .map_err(|error| format!("连接供应商失败：{error}"))?;
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|bytes| bytes > crate::market::LLM_MODEL_LIST_MAX_BYTES as u64)
    {
        return Err("供应商测试响应过大，已停止读取。".to_string());
    }
    let mut response_body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("读取供应商测试响应失败：{error}"))?;
        if response_body.len().saturating_add(chunk.len()) > crate::market::LLM_MODEL_LIST_MAX_BYTES {
            return Err("供应商测试响应过大，已停止读取。".to_string());
        }
        response_body.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        return Err(llm_connection_http_error(
            status.as_u16(),
            &response_body,
            api_key,
        ));
    }
    let response_json: Value = serde_json::from_slice(&response_body)
        .map_err(|error| format!("供应商测试响应不是有效 JSON：{error}"))?;
    if !llm_test_response_has_content(&response_json, api_format) {
        return Err("供应商已响应，但返回格式与所选协议不匹配。".to_string());
    }
    Ok(json!({
        "ok": true,
        "endpoint": endpoint.to_string(),
        "status": status.as_u16(),
        "elapsed_ms": crate::market::epoch_millis().saturating_sub(started_at),
        "api_format": api_format,
    }))
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
                    .any(|content| content.get("text").and_then(Value::as_str).is_some())
        }
        "anthropic_messages" => response
            .get("content")
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items
                    .iter()
                    .any(|item| item.get("text").and_then(Value::as_str).is_some())
            }),
        _ => response
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .is_some(),
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
                .map(|value| redact_llm_error_detail(&crate::market::truncate_for_note(value, 180), api_key))
        });
    detail
        .map(|message| format!("供应商返回 HTTP {status}：{message}"))
        .unwrap_or_else(|| format!("供应商返回 HTTP {status}，连接测试失败。"))
}

pub(crate) fn llm_models_endpoint(base_url: &str, api_format: &str) -> Result<reqwest::Url, String> {
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

pub(crate) fn is_official_deepseek_anthropic_base_url(url: &reqwest::Url, api_format: &str) -> bool {
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
                .map(|value| redact_llm_error_detail(&crate::market::truncate_for_note(value, 180), api_key))
        });
    detail
        .map(|message| format!("供应商返回 HTTP {status}：{message}"))
        .unwrap_or_else(|| format!("供应商返回 HTTP {status}，未能拉取模型列表。"))
}

pub(crate) fn redact_llm_error_detail(detail: &str, api_key: &str) -> String {
    let api_key = api_key.trim();
    if api_key.len() < 4 {
        return detail.to_string();
    }
    detail.replace(api_key, "[已隐藏密钥]")
}
