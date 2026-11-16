use crate::market::providers::eastmoney::fetch_eastmoney_daily_history;
use crate::market::providers::tencent::fetch_tencent_daily_history;
use crate::network::http::build_tencent_http_client;
#[cfg(windows)]
use crate::network::http::decode_utf8_lossy;
#[cfg(windows)]
use crate::network::powershell::powershell_http_get_bytes;
use serde_json::Value;
use std::time::Duration;

pub(crate) const OBSERVE_FUNDAMENTAL_PREFERRED_TIMEOUT_SECS: u64 = 3;

pub(crate) const OBSERVE_HISTORY_TIMEOUT_SECS: u64 = 8;

pub(crate) async fn fetch_observe_daily_history(
    code: &str,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<Value>, String> {
    let timeout = Duration::from_secs(OBSERVE_HISTORY_TIMEOUT_SECS);
    let client = build_tencent_http_client("Mozilla/5.0 GuXuanYou/0.3 observe history", timeout)?;
    let mut errors = Vec::new();
    let tencent_rows = match fetch_tencent_daily_history(&client, code, start_date, end_date).await
    {
        Ok(rows) if !rows.is_empty() => Some(rows),
        Ok(_) => {
            errors.push("腾讯日线返回空数据".to_string());
            None
        }
        Err(error) => {
            errors.push(format!("腾讯日线：{error}"));
            None
        }
    };
    if tencent_rows.as_ref().is_some_and(|rows| rows.len() >= 60) {
        return Ok(tencent_rows.expect("checked above"));
    }
    let eastmoney_rows =
        match fetch_eastmoney_daily_history(&client, code, start_date, end_date).await {
            Ok(rows) if !rows.is_empty() => Some(rows),
            Ok(_) => {
                errors.push("东方财富日线返回空数据".to_string());
                None
            }
            Err(error) => {
                errors.push(format!("东方财富日线：{error}"));
                None
            }
        };
    choose_longer_history(tencent_rows, eastmoney_rows).ok_or_else(|| errors.join("\u{ff1b}"))
}

pub(crate) fn choose_longer_history(
    primary: Option<Vec<Value>>,
    secondary: Option<Vec<Value>>,
) -> Option<Vec<Value>> {
    match (primary, secondary) {
        (Some(primary), Some(secondary)) => Some(if secondary.len() > primary.len() {
            secondary
        } else {
            primary
        }),
        (Some(rows), None) | (None, Some(rows)) => Some(rows),
        (None, None) => None,
    }
}

pub(crate) async fn fetch_daily_history_text(
    client: &reqwest::Client,
    url: &str,
    label: &str,
) -> Result<String, String> {
    let primary_error = match client.get(url).send().await {
        Ok(response) if response.status().is_success() => match response.text().await {
            Ok(text) => return Ok(text),
            Err(error) => format!("{label} response read failed: {error}"),
        },
        Ok(response) => format!("{label} HTTP {}", response.status().as_u16()),
        Err(error) => format!("{label} request failed: {error}"),
    };

    #[cfg(windows)]
    {
        let fallback_url = url.to_string();
        let bytes = crate::runtime::run_transport_bound("market blocking transport", move || {
            powershell_http_get_bytes(&fallback_url, OBSERVE_HISTORY_TIMEOUT_SECS)
        })
        .await
        .map_err(|error| format!("{primary_error}; PowerShell fallback task failed: {error}"))?
        .map_err(|error| format!("{primary_error}; PowerShell fallback failed: {error}"))?;
        return Ok(decode_utf8_lossy(bytes));
    }

    #[cfg(not(windows))]
    Err(primary_error)
}
