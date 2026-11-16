use crate::market_domain::codes::normalize_stock_code;
use crate::market_domain::codes::ths_market_code;
use crate::market_domain::dates::current_calendar_year_utc;
use crate::market_domain::financial::eps_metric_value;
use crate::market_domain::financial::financial_metric_is_eps;
use crate::market_domain::financial::financial_period_from_text;
use crate::market_domain::financial::object_string_any;
use crate::market_domain::financial::quarterly_eps_value;
use crate::market_domain::financial::sort_dedup_quarterly_eps;
use crate::market_domain::financial::COMPLETE_QUARTERLY_EPS_POINTS;
use crate::network::http::build_tencent_http_client;
use crate::network::http::decode_utf8_lossy;
use crate::network::powershell::powershell_http_get_bytes;
use crate::wire_values::parse_f64_str;
use serde_json::Value;
use std::time::Duration;

pub(crate) const FINANCIAL_REQUEST_TIMEOUT_SECS: u64 = 6;

pub(crate) const THS_FINANCIAL_ENDPOINT: &str =
    "https://basic.10jqka.com.cn/basicapi/finance/index/v1/app_data/";

pub(crate) const SINA_FINANCIAL_GUIDELINE_ENDPOINT: &str =
    "https://money.finance.sina.com.cn/corp/go.php/vFD_FinancialGuideLine";

pub(crate) struct QuarterlyEpsFetchResult {
    pub(crate) points: Vec<Value>,
    pub(crate) sources: Vec<String>,
    pub(crate) errors: Vec<String>,
}

pub(crate) async fn fetch_quarterly_eps_chain(code: &str) -> QuarterlyEpsFetchResult {
    let timeout = Duration::from_secs(FINANCIAL_REQUEST_TIMEOUT_SECS);
    let client = match build_tencent_http_client("Mozilla/5.0 GuXuanYou/0.3 financial", timeout) {
        Ok(client) => client,
        Err(error) => {
            return QuarterlyEpsFetchResult {
                points: Vec::new(),
                sources: Vec::new(),
                errors: vec![format!("财报 HTTP 客户端创建失败：{error}")],
            };
        }
    };
    let mut points = Vec::new();
    let mut sources = Vec::new();
    let mut errors = Vec::new();

    match fetch_ths_quarterly_eps(&client, code).await {
        Ok(rows) if !rows.is_empty() => {
            points.extend(rows);
            sources.push("同花顺财务摘要".to_string());
        }
        Ok(_) => errors.push("同花顺：返回空 EPS 明细".to_string()),
        Err(error) => errors.push(format!("同花顺：{error}")),
    }
    points = sort_dedup_quarterly_eps(points);

    if points.len() < COMPLETE_QUARTERLY_EPS_POINTS {
        match fetch_sina_quarterly_eps(&client, code).await {
            Ok(rows) if !rows.is_empty() => {
                points.extend(rows);
                sources.push("新浪财经财务指标".to_string());
            }
            Ok(_) => errors.push("新浪财经：返回空 EPS 明细".to_string()),
            Err(error) => errors.push(format!("新浪财经：{error}")),
        }
        points = sort_dedup_quarterly_eps(points);
    }

    if points.is_empty() {
        errors.push("东财财报源已禁用；仅保留同花顺/新浪与本地缓存参与财报补全。".to_string());
    }

    QuarterlyEpsFetchResult {
        points,
        sources,
        errors,
    }
}

pub(crate) async fn fetch_ths_quarterly_eps(
    client: &reqwest::Client,
    code: &str,
) -> Result<Vec<Value>, String> {
    let normalized =
        normalize_stock_code(code).ok_or_else(|| format!("无法识别同花顺财报代码：{code}"))?;
    let digits = normalized
        .get(..6)
        .ok_or_else(|| format!("无法识别同花顺财报代码：{code}"))?;
    let market =
        ths_market_code(&normalized).ok_or_else(|| format!("同花顺暂不支持该市场：{code}"))?;
    let market_string = market.to_string();
    let url = reqwest::Url::parse_with_params(
        THS_FINANCIAL_ENDPOINT,
        &[
            ("code", digits),
            ("id", "client_stock_importance"),
            ("market", market_string.as_str()),
            ("type", "stock"),
            ("page", "1"),
            ("size", "50"),
            ("period", "0"),
        ],
    )
    .map_err(|error| error.to_string())?;
    let url_text = url.to_string();
    let text = match client.get(url.clone()).send().await {
        Ok(response) if response.status().is_success() => {
            response.text().await.map_err(|error| error.to_string())?
        }
        Ok(response) => {
            let primary_error = format!("HTTP {}", response.status().as_u16());
            let bytes = powershell_http_get_bytes(&url_text, FINANCIAL_REQUEST_TIMEOUT_SECS)
                .map_err(|fallback_error| {
                    format!("{primary_error}; PowerShell fallback: {fallback_error}")
                })?;
            decode_utf8_lossy(bytes)
        }
        Err(error) => {
            let primary_error = error.to_string();
            let bytes = powershell_http_get_bytes(&url_text, FINANCIAL_REQUEST_TIMEOUT_SECS)
                .map_err(|fallback_error| {
                    format!("{primary_error}; PowerShell fallback: {fallback_error}")
                })?;
            decode_utf8_lossy(bytes)
        }
    };
    parse_ths_quarterly_eps_json(&text)
}

pub(crate) fn parse_ths_quarterly_eps_json(text: &str) -> Result<Vec<Value>, String> {
    let value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let rows = value
        .get("data")
        .and_then(|data| data.get("data"))
        .and_then(Value::as_array)
        .ok_or_else(|| "响应缺少 data.data".to_string())?;
    let mut points = Vec::new();
    for row in rows {
        let Some(object) = row.as_object() else {
            continue;
        };
        let period = object_string_any(
            object,
            &[
                "date",
                "report_date",
                "report",
                "report_name",
                "quarter_name",
            ],
        )
        .and_then(|raw| financial_period_from_text(&raw));
        let Some(period) = period else {
            continue;
        };
        let Some(index_list) = object.get("index_list").and_then(Value::as_object) else {
            continue;
        };
        for (metric_name, metric_values) in index_list {
            if !financial_metric_is_eps(metric_name) {
                continue;
            }
            if let Some(eps) = eps_metric_value(metric_values) {
                points.push(quarterly_eps_value(&period, eps, "同花顺财务摘要"));
                break;
            }
        }
    }
    Ok(sort_dedup_quarterly_eps(points))
}

pub(crate) async fn fetch_sina_quarterly_eps(
    client: &reqwest::Client,
    code: &str,
) -> Result<Vec<Value>, String> {
    let normalized =
        normalize_stock_code(code).ok_or_else(|| format!("无法识别新浪财报代码：{code}"))?;
    let digits = normalized
        .get(..6)
        .ok_or_else(|| format!("无法识别新浪财报代码：{code}"))?;
    let current_year = current_calendar_year_utc();
    let mut points = Vec::new();
    let mut errors = Vec::new();
    for year in [current_year, current_year - 1, current_year - 2] {
        let url = format!(
            "{SINA_FINANCIAL_GUIDELINE_ENDPOINT}/stockid/{digits}/ctrl/{year}/displaytype/4.phtml"
        );
        let bytes_result = match client.get(&url).send().await {
            Ok(response) if response.status().is_success() => response
                .bytes()
                .await
                .map(|bytes| bytes.to_vec())
                .map_err(|error| error.to_string()),
            Ok(response) => Err(format!("HTTP {}", response.status().as_u16())),
            Err(error) => Err(error.to_string()),
        };
        let bytes = match bytes_result {
            Ok(bytes) => bytes,
            Err(primary_error) => {
                match powershell_http_get_bytes(&url, FINANCIAL_REQUEST_TIMEOUT_SECS) {
                    Ok(bytes) => bytes,
                    Err(fallback_error) => {
                        errors.push(format!(
                            "{} 年 {}; PowerShell fallback: {}",
                            year, primary_error, fallback_error
                        ));
                        continue;
                    }
                }
            }
        };
        let (text, _, _) = encoding_rs::GBK.decode(&bytes);
        points.extend(parse_sina_quarterly_eps_html(&text));
        points = sort_dedup_quarterly_eps(points);
        if points.len() >= COMPLETE_QUARTERLY_EPS_POINTS {
            break;
        }
    }
    if points.is_empty() && !errors.is_empty() {
        return Err(errors.join("；"));
    }
    Ok(points)
}

pub(crate) fn parse_sina_quarterly_eps_html(text: &str) -> Vec<Value> {
    let html = text
        .replace("<TR", "<tr")
        .replace("</TR", "</tr")
        .replace("<TD", "<td")
        .replace("</TD", "</td")
        .replace("<TH", "<th")
        .replace("</TH", "</th");
    let mut header_periods: Vec<String> = Vec::new();
    let mut points = Vec::new();
    for row in html.split("<tr") {
        let cells = extract_html_cells(row);
        if cells.is_empty() {
            continue;
        }
        let periods = cells
            .iter()
            .filter_map(|cell| financial_period_from_text(cell))
            .collect::<Vec<_>>();
        if periods.len() >= 2 {
            header_periods = periods;
        }
        let first = cells.first().map(String::as_str).unwrap_or("");
        if !financial_metric_is_eps(first) || header_periods.is_empty() {
            continue;
        }
        for (period, value_cell) in header_periods.iter().zip(cells.iter().skip(1)) {
            if let Some(eps) = parse_f64_str(value_cell) {
                points.push(quarterly_eps_value(period, eps, "新浪财经财务指标"));
            }
        }
    }
    sort_dedup_quarterly_eps(points)
}

pub(crate) fn extract_html_cells(row: &str) -> Vec<String> {
    let lower = row.to_ascii_lowercase();
    let mut cells = Vec::new();
    let mut index = 0;
    while index < lower.len() {
        let next_td = lower[index..].find("<td").map(|pos| index + pos);
        let next_th = lower[index..].find("<th").map(|pos| index + pos);
        let Some(start) = (match (next_td, next_th) {
            (Some(td), Some(th)) => Some(td.min(th)),
            (Some(td), None) => Some(td),
            (None, Some(th)) => Some(th),
            (None, None) => None,
        }) else {
            break;
        };
        let tag = if lower[start..].starts_with("<th") {
            "th"
        } else {
            "td"
        };
        let Some(content_start) = lower[start..].find('>').map(|pos| start + pos + 1) else {
            break;
        };
        let close_tag = format!("</{tag}>");
        let Some(content_end) = lower[content_start..]
            .find(&close_tag)
            .map(|pos| content_start + pos)
        else {
            break;
        };
        let cell = strip_html_tags(&row[content_start..content_end]);
        if !cell.trim().is_empty() {
            cells.push(cell);
        }
        index = content_end + close_tag.len();
    }
    cells
}

pub(crate) fn strip_html_tags(value: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in value.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .replace("&amp;", "&")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
