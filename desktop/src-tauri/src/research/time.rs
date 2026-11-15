use serde_json::Value;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering as AtomicOrdering;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

pub(in crate::research) fn normalized_research_timestamp(
    value: &str,
    end_of_date: bool,
) -> Option<String> {
    let timestamp = if value.len() == 10 {
        let date = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()?;
        if end_of_date {
            date.and_hms_nano_opt(23, 59, 59, 999_999_999)?
        } else {
            date.and_hms_opt(0, 0, 0)?
        }
    } else if let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(value) {
        timestamp.naive_utc()
    } else {
        chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f")
            .or_else(|_| chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f"))
            .ok()?
    };
    Some(timestamp.format("%Y-%m-%d %H:%M:%S%.9f").to_string())
}

pub(crate) fn normalize_research_as_of(value: Option<&Value>) -> Result<Option<String>, String> {
    let value = match value {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(value)) if value.trim().is_empty() => {
            return Err("as_of cannot be empty".to_string())
        }
        Some(Value::String(value)) if value.len() <= 40 => value.trim(),
        Some(_) => return Err("as_of must be an ISO 8601 date or timestamp".to_string()),
    };
    normalized_research_timestamp(value, true)
        .map(Some)
        .ok_or_else(|| "as_of must be a valid ISO 8601 date or timestamp".to_string())
}

pub(crate) fn research_evidence_available_as_of(published_at: Option<&str>, cutoff: &str) -> bool {
    published_at
        .and_then(|value| normalized_research_timestamp(value, false))
        .is_some_and(|published| published.as_str() <= cutoff)
}

pub(in crate::research) fn epoch_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

pub(in crate::research) fn unique_suffix() -> u64 {
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_ID.fetch_add(1, AtomicOrdering::Relaxed)
}
