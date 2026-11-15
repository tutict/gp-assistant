use serde_json::Value;

pub(crate) fn apply_proxy_url(
    builder: reqwest::ClientBuilder,
    proxy: Option<&str>,
) -> Result<reqwest::ClientBuilder, String> {
    let Some(proxy) = proxy else {
        return Ok(builder);
    };
    let reqwest_proxy = reqwest::Proxy::all(proxy)
        .map_err(|error| format!("invalid proxy URL {proxy}: {error}"))?;
    Ok(builder.proxy(reqwest_proxy))
}

pub(crate) fn proxy_from_payload(payload: Option<&Value>) -> Option<String> {
    let payload = payload?;
    let (mode, url) = if let Some(mode) = payload.get("network_proxy_mode").and_then(Value::as_str)
    {
        (
            mode,
            payload.get("network_proxy_url").and_then(Value::as_str),
        )
    } else {
        (
            payload
                .get("proxy_mode")
                .and_then(Value::as_str)
                .unwrap_or("none"),
            payload.get("proxy_url").and_then(Value::as_str),
        )
    };
    let mode = mode.trim().to_ascii_lowercase();
    match mode.as_str() {
        "manual" => url
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(normalize_manual_proxy_url),
        "system" => system_environment_proxy(),
        _ => None,
    }
}

fn system_environment_proxy() -> Option<String> {
    [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ]
    .iter()
    .find_map(|key| std::env::var(key).ok())
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty())
}

pub(in crate::network) fn validate_network_proxy_settings(
    payload: Option<&Value>,
) -> Result<(), String> {
    let payload = payload.ok_or_else(|| "港股网络代理设置缺失".to_string())?;
    let mode = payload
        .get("network_proxy_mode")
        .and_then(Value::as_str)
        .ok_or_else(|| "港股网络代理模式无效".to_string())?;
    match mode {
        "direct" | "system" => Ok(()),
        "manual" => {
            let raw_url = payload
                .get("network_proxy_url")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "请输入手动代理地址".to_string())?;
            let url = reqwest::Url::parse(raw_url)
                .map_err(|_| "代理地址格式无效，请使用不带账号密码的 HTTP(S) 地址".to_string())?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.path() != "/"
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err("代理地址仅支持不带账号密码的 HTTP(S) 主机和端口".to_string());
            }
            Ok(())
        }
        _ => Err("港股网络代理模式无效".to_string()),
    }
}

pub(crate) fn normalize_manual_proxy_url(value: &str) -> String {
    #[cfg(target_os = "android")]
    {
        if let Ok(mut url) = reqwest::Url::parse(value) {
            let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
            if host == "127.0.0.1" || host == "localhost" || host == "::1" {
                if url.set_host(Some("10.0.2.2")).is_ok() {
                    return url.to_string();
                }
            }
        }
    }
    value.to_string()
}

pub(crate) fn proxy_mode_from_payload(payload: Option<&Value>) -> String {
    payload
        .and_then(|value| {
            value
                .get("network_proxy_mode")
                .or_else(|| value.get("proxy_mode"))
        })
        .and_then(Value::as_str)
        .unwrap_or("none")
        .trim()
        .to_ascii_lowercase()
}
