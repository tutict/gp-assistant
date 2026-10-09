//! Local-first request policy and pure coverage checks. No I/O or network.
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DataPolicy {
    CacheOnly,
    Refresh,
}
impl DataPolicy {
    pub(crate) fn parse(payload: &Value) -> Result<Self, String> {
        let policy = match payload.get("data_policy") {
            None => Self::CacheOnly,
            Some(Value::String(value)) if value == "cache_only" => Self::CacheOnly,
            Some(Value::String(value)) if value == "refresh" => Self::Refresh,
            _ => return Err(json!({"code":"INVALID_DATA_POLICY","message":"data_policy must be cache_only or refresh"}).to_string()),
        };
        if payload
            .get("internal_release_validation_cold_start")
            .and_then(Value::as_bool)
            == Some(true)
            && policy != Self::Refresh
        {
            return Err(json!({"code":"REFRESH_REQUIRED","message":"release validation cold-start requires explicit data_policy=refresh","action":"refresh"}).to_string());
        }
        Ok(policy)
    }
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::CacheOnly => "cache_only",
            Self::Refresh => "refresh",
        }
    }
}

pub(crate) fn core_payload(mut payload: Value) -> Value {
    if let Some(object) = payload.as_object_mut() {
        for key in ["data_policy", "request_id", "job_id", "financial_snapshot"] {
            object.remove(key);
        }
    }
    payload
}

pub(crate) fn missing_error(operation: &str, missing: &[Value]) -> String {
    json!({"code":"LOCAL_DATA_MISSING", "operation":operation, "missing_count":missing.len(),
        "missing":missing.iter().take(20).collect::<Vec<_>>(), "truncated":missing.len() > 20,
        "message":"Required local data is missing; no substitute algorithm was used.",
        "action":"refresh"})
    .to_string()
}

// Input dates have already passed the market/core parser. Ignore malformed dates defensively.
fn date_key(date: &str) -> Option<String> {
    let key: String = date.chars().filter(|c| *c != '-').take(8).collect();
    (key.len() == 8 && key.bytes().all(|c| c.is_ascii_digit())).then_some(key)
}
pub(crate) fn history_coverage<'a>(
    dates: impl Iterator<Item = &'a str>,
    start: &str,
    end: &str,
    required: usize,
) -> Value {
    let start = date_key(start).unwrap_or_else(|| "99999999".into());
    let end = date_key(end).unwrap_or_default();
    let mut dates: Vec<_> = dates
        .filter_map(date_key)
        .filter(|d| *d >= start && *d <= end)
        .collect();
    dates.sort_unstable();
    dates.dedup();
    json!({"kind":"history", "start_date":start, "end_date":end, "required_bars":required,
        "available_bars":dates.len(), "first_date":dates.first(), "as_of":dates.last(), "sufficient":dates.len() >= required})
}

pub(crate) fn require_coverage(operation: &str, coverage: &[Value]) -> Result<(), String> {
    let missing: Vec<_> = coverage
        .iter()
        .filter(|item| item.get("sufficient").and_then(Value::as_bool) != Some(true))
        .cloned()
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(missing_error(operation, &missing))
    }
}

pub(crate) fn metadata(
    policy: DataPolicy,
    quote_as_of: Option<String>,
    coverage: &[Value],
) -> Value {
    let history_as_of = coverage
        .iter()
        .filter_map(|item| item.get("as_of").and_then(Value::as_str))
        .min();
    json!({"data_policy":policy.as_str(), "source": if policy == DataPolicy::CacheOnly { "local_cache" } else { "cache_with_explicit_refresh" },
        "quote_as_of":quote_as_of, "history_as_of":history_as_of, "coverage_count":coverage.len(),
        "coverage":coverage.iter().take(20).collect::<Vec<_>>(), "coverage_truncated":coverage.len() > 20,
        "freshness_note":"Cache age does not invalidate readable data; dates describe the actual inputs, not live-market freshness."})
}
pub(crate) fn attach_metadata(result: &mut Value, metadata: Value, payload: &Value) {
    if let Some(object) = result.as_object_mut() {
        object.insert("data_metadata".into(), metadata);
        for key in ["request_id", "job_id"] {
            if let Some(value) = payload.get(key) {
                object.insert(key.into(), value.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn defaults_closed_and_requires_explicit_refresh() {
        assert_eq!(
            DataPolicy::parse(&json!({})).unwrap(),
            DataPolicy::CacheOnly
        );
        assert_eq!(
            DataPolicy::parse(&json!({"data_policy":"refresh"})).unwrap(),
            DataPolicy::Refresh
        );
        assert!(DataPolicy::parse(&json!({"data_policy":"auto"})).is_err());
        assert!(DataPolicy::parse(&json!({"data_policy":null})).is_err());
    }
    #[test]
    fn cold_start_requires_refresh_instead_of_silently_becoming_warm() {
        assert!(
            DataPolicy::parse(&json!({"internal_release_validation_cold_start":true})).is_err()
        );
        assert!(DataPolicy::parse(
            &json!({"internal_release_validation_cold_start":true,"data_policy":"refresh"})
        )
        .is_ok());
    }
    #[test]
    fn missing_details_are_bounded_and_keep_total_count() {
        let missing = (0..100)
            .map(|i| json!({"kind":"history","code":i.to_string()}))
            .collect::<Vec<_>>();
        let error: serde_json::Value =
            serde_json::from_str(&missing_error("screen", &missing)).unwrap();
        assert_eq!(error["code"], "LOCAL_DATA_MISSING");
        assert_eq!(error["missing_count"], 100);
        assert_eq!(error["missing"].as_array().unwrap().len(), 20);
        assert_eq!(error["action"], "refresh");
    }
    #[test]
    fn old_cache_is_readable_but_missing_or_future_history_is_not() {
        let rows = vec![json!({"date":"20200102"}), json!({"date":"20200103"})];
        assert_eq!(
            history_coverage(
                rows.iter().map(|r| r["date"].as_str().unwrap()),
                "19900101",
                "20261006",
                2
            )["sufficient"],
            true
        );
        assert_eq!(
            history_coverage(
                rows.iter().map(|r| r["date"].as_str().unwrap()),
                "19900101",
                "20200102",
                2
            )["sufficient"],
            false
        );
        assert_eq!(
            history_coverage(std::iter::empty(), "19900101", "20501231", 2)["sufficient"],
            false
        );
    }
    #[test]
    fn transport_ids_are_not_core_algorithm_fields() {
        let core = core_payload(
            json!({"data_policy":"cache_only","request_id":"r","job_id":"j","as_of_date":"20200103"}),
        );
        assert_eq!(core, json!({"as_of_date":"20200103"}));
    }
    #[test]
    fn metadata_reports_old_dates_and_preserves_transport_ids() {
        let coverage = vec![history_coverage(
            ["20200102", "20200103"].into_iter(),
            "19900101",
            "20501231",
            2,
        )];
        let meta = metadata(DataPolicy::CacheOnly, Some("20200103".into()), &coverage);
        assert_eq!(meta["history_as_of"], "20200103");
        assert_eq!(meta["source"], "local_cache");
        let mut result = json!({"items":[]});
        attach_metadata(&mut result, meta, &json!({"request_id":"r", "job_id":"j"}));
        assert_eq!(result["job_id"], "j");
        assert!(require_coverage("screen", &coverage).is_ok());
    }
}
