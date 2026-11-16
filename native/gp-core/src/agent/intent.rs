use crate::screening::ScreenCriteria;
use regex::Regex;
use std::collections::HashSet;

pub(super) fn agent_intent_kind(action: &str) -> &'static str {
    match action {
        "trend_screen" => "trend_analysis",
        "graph_screen" => "sector_analysis",
        "backtest" => "portfolio_simulation",
        "screen" => "stock_screen",
        "observe_stock" => "stock_snapshot",
        "news_rag" => "stock_news",
        "watchlist_action" => "watchlist_action",
        _ => "clarify",
    }
}

pub(super) fn agent_depth_for_mode(mode: &str) -> &'static str {
    match mode {
        "research" => "research",
        "expert" => "expert",
        _ => "quick",
    }
}

pub(super) fn agent_window_hint(message: &str) -> Option<String> {
    let lower = message.to_lowercase();
    if lower.contains("recent") || message.contains("近期") || message.contains("最近") {
        Some("recent".to_string())
    } else if lower.contains("today") || message.contains("今天") || message.contains("今日") {
        Some("today".to_string())
    } else {
        None
    }
}

pub(crate) fn heuristic_criteria(message: &str) -> ScreenCriteria {
    let mut criteria = ScreenCriteria::default();
    criteria.max_pe = extract_number_after(message, &["PE", "pe", "市盈率"]);
    criteria.max_pb = extract_number_after(message, &["PB", "pb", "市净率"]);
    criteria.min_roe = extract_percent_after(message, &["ROE", "roe", "净资产收益率"]);
    criteria.industry = extract_industry(message);
    criteria
}

fn extract_number_after(message: &str, labels: &[&str]) -> Option<f64> {
    for label in labels {
        let pattern = format!(
            r"{}\s*(?:低于|小于|<=|<|不高于|少于|在)?\s*(\d+(?:\.\d+)?)",
            regex::escape(label)
        );
        if let Ok(regex) = Regex::new(&pattern) {
            if let Some(match_result) = regex.captures(message) {
                return match_result.get(1)?.as_str().parse().ok();
            }
        }
    }
    None
}

fn extract_percent_after(message: &str, labels: &[&str]) -> Option<f64> {
    for label in labels {
        let pattern = format!(
            r"{}\s*(?:高于|大于|>=|>|不低于|超过|在)?\s*(\d+(?:\.\d+)?)\s*(%)?",
            regex::escape(label)
        );
        if let Ok(regex) = Regex::new(&pattern) {
            if let Some(match_result) = regex.captures(message) {
                let value: f64 = match_result.get(1)?.as_str().parse().ok()?;
                return Some(if match_result.get(2).is_some() {
                    value / 100.0
                } else {
                    value
                });
            }
        }
    }
    None
}

fn extract_industry(message: &str) -> Option<String> {
    let known = [
        ("银行", "银行"),
        ("白酒", "白酒"),
        ("饮料", "白酒"),
        ("食品饮料", "食品饮料"),
        ("电池", "动力电池"),
        ("动力电池", "动力电池"),
        ("新能源", "动力电池"),
        ("汽车", "汽车"),
        ("电子", "电子制造"),
        ("半导体", "电子制造"),
        ("芯片", "电子制造"),
        ("电子制造", "电子制造"),
        ("光伏", "光伏"),
        ("化工", "化工"),
    ];
    known
        .iter()
        .find(|(keyword, _industry)| message.contains(keyword))
        .map(|(_keyword, industry)| (*industry).to_string())
}

pub(super) fn extract_codes(message: &str) -> Vec<String> {
    let Ok(regex) = Regex::new(r"\b(\d{6})(?:\.(SH|SZ|BJ))?\b") else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    regex
        .captures_iter(&message.to_uppercase())
        .filter_map(|captures| {
            let code = captures.get(1)?.as_str();
            let market = captures.get(2).map(|m| m.as_str()).unwrap_or_else(|| {
                if code.starts_with('6') {
                    "SH"
                } else if code.starts_with('8') || code.starts_with('4') {
                    "BJ"
                } else {
                    "SZ"
                }
            });
            let normalized = format!("{code}.{market}");
            if seen.insert(normalized.clone()) {
                Some(normalized)
            } else {
                None
            }
        })
        .collect()
}

pub(super) fn extract_date(message: &str, default: &str, first: bool) -> String {
    let Ok(date_regex) = Regex::new(r"\b(20\d{2})(?:[-/.年])?(\d{1,2})(?:[-/.月])?(\d{1,2})日?\b")
    else {
        return default.to_string();
    };
    let dates: Vec<(String, String, String)> = date_regex
        .captures_iter(message)
        .filter_map(|captures| {
            Some((
                captures.get(1)?.as_str().to_string(),
                captures.get(2)?.as_str().to_string(),
                captures.get(3)?.as_str().to_string(),
            ))
        })
        .collect();
    if let Some((year, month, day)) = if first { dates.first() } else { dates.last() } {
        return format!("{year}{:0>2}{:0>2}", month, day);
    }

    let Ok(year_regex) = Regex::new(r"\b(20\d{2})\b") else {
        return default.to_string();
    };
    let years: Vec<String> = year_regex
        .captures_iter(message)
        .filter_map(|captures| captures.get(1).map(|matched| matched.as_str().to_string()))
        .collect();
    if let Some(year) = if first { years.first() } else { years.last() } {
        if first {
            format!("{year}0101")
        } else {
            format!("{year}1231")
        }
    } else {
        default.to_string()
    }
}
