use serde_json::json;
use serde_json::Value;
use sha2::Digest;
use sha2::Sha256;

pub(crate) fn payload_usize_field(
    value: &Value,
    key: &str,
    default: usize,
    min: usize,
    max: usize,
) -> usize {
    let parsed = value.get(key).and_then(|field| {
        field
            .as_u64()
            .and_then(|number| usize::try_from(number).ok())
            .or_else(|| {
                field
                    .as_str()
                    .and_then(|text| text.trim().parse::<usize>().ok())
            })
    });
    parsed.unwrap_or(default).clamp(min, max)
}

pub(crate) fn truncate_for_note(value: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for ch in value.chars().take(max_chars) {
        output.push(ch);
    }
    if value.chars().count() > max_chars {
        output.push_str("...");
    }
    output
}

pub(crate) fn parse_number(value: Option<&&str>) -> Option<f64> {
    let raw = value?.trim();
    if raw.is_empty() || matches!(raw, "-" | "None" | "nan") {
        return None;
    }
    raw.parse::<f64>().ok().filter(|value| value.is_finite())
}

pub(crate) fn object_f64(object: &serde_json::Map<String, Value>, key: &str) -> Option<f64> {
    object
        .get(key)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

pub(crate) fn object_string(object: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    object.get(key).and_then(Value::as_str).map(str::to_string)
}

pub(crate) fn append_result_notes(result: &mut Value, extra_notes: Vec<String>) {
    if extra_notes.is_empty() || !result.is_object() {
        return;
    }
    let notes = result
        .as_object_mut()
        .expect("result object checked")
        .entry("notes".to_string())
        .or_insert_with(|| json!([]));
    if let Some(items) = notes.as_array_mut() {
        for note in extra_notes {
            if !note.trim().is_empty() {
                items.push(Value::String(note));
            }
        }
    }
}

pub(crate) fn extract_json_after_var(html: &str, var_name: &str) -> Option<String> {
    let marker = format!("var {var_name}=");
    let start = html.find(&marker)? + marker.len();
    extract_balanced_json(&html[start..])
}

pub(crate) fn extract_balanced_json(raw: &str) -> Option<String> {
    let mut start = None;
    for (index, ch) in raw.char_indices() {
        if ch == '{' || ch == '[' {
            start = Some((index, ch));
            break;
        }
    }
    let (start_index, open) = start?;
    let close = if open == '{' { '}' } else { ']' };
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, ch) in raw[start_index..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
        } else if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return Some(raw[start_index..start_index + offset + ch.len_utf8()].to_string());
            }
        }
    }
    None
}

pub(crate) fn clean_html_text(value: &str) -> String {
    value
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

pub(crate) fn score_sentiment_label(score: f64) -> &'static str {
    if score >= 60.0 {
        "positive"
    } else if score <= 40.0 {
        "negative"
    } else {
        "uncertain"
    }
}

pub(crate) fn round2_value(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

pub(crate) fn format_amount_wan(value: f64) -> String {
    if value.abs() >= 100_000_000.0 {
        format!("{:.2} 亿", value / 100_000_000.0)
    } else if value.abs() >= 10_000.0 {
        format!("{:.2} 万", value / 10_000.0)
    } else {
        format_number_like(value)
    }
}

pub(crate) fn institution_buy_sell_ratio(buy: f64, sell: f64) -> String {
    if sell.abs() > f64::EPSILON {
        format_number_like(buy / sell)
    } else if buy > 0.0 {
        "∞".to_string()
    } else {
        "-".to_string()
    }
}

pub(crate) fn format_number_like(value: f64) -> String {
    if value.abs() >= 100.0 {
        format!("{value:.0}")
    } else if value.abs() >= 10.0 {
        format!("{value:.2}")
    } else {
        format!("{value:.3}")
    }
}

pub(crate) fn compact_count(value: f64) -> String {
    if value >= 10_000.0 {
        format!("{:.1}万", value / 10_000.0)
    } else {
        format_number_like(value)
    }
}

pub(crate) fn json_f64(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64().filter(|value| value.is_finite()),
        Value::String(raw) => parse_f64_str(raw),
        _ => None,
    }
}

pub(crate) fn parse_f64_str(raw: &str) -> Option<f64> {
    let value = raw.trim().replace(',', "");
    if value.is_empty() || matches!(value.as_str(), "-" | "None" | "nan") {
        return None;
    }
    value.parse::<f64>().ok().filter(|value| value.is_finite())
}

pub(crate) fn optional_trimmed_string(value: Option<&Value>) -> Option<String> {
    let trimmed = value?.as_str()?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn sanitize_path_part(value: &str) -> String {
    let mut part: String = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
        .take(120)
        .collect();
    if part.is_empty() {
        part.push_str("unknown");
    } else if part.chars().all(|ch| ch == '.') {
        part = format!("~{}", part.len());
    }
    part
}
