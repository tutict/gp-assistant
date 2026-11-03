use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Flow {
    Observe,
    Screening,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Stage {
    CacheSnapshotPrepare,
    FinancialSupplement,
    HistorySupplement,
    NetworkSupplement,
    CandidateGeneration,
    RustCalculation,
    ResultPersistence,
    CommandTotal,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CacheStrategy {
    CacheOnly,
    Refresh,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Outcome {
    Ok,
    Error,
    Timeout,
    CacheHit,
    CacheMiss,
    Partial,
}

#[derive(Serialize)]
pub(crate) struct PerfEvent {
    flow: Flow,
    stage: Stage,
    elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_strategy: Option<CacheStrategy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stock_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    candidate_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    history_rows: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    financial_records: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<Outcome>,
}

impl PerfEvent {
    pub(crate) fn new(flow: Flow, stage: Stage, elapsed: Duration) -> Self {
        Self {
            flow,
            stage,
            elapsed_ms: elapsed.as_millis().min(u64::MAX as u128) as u64,
            cache_strategy: None,
            stock_count: None,
            candidate_count: None,
            history_rows: None,
            financial_records: None,
            request_bytes: None,
            response_bytes: None,
            outcome: None,
        }
    }

    pub(crate) fn with_cache_strategy(mut self, cache_strategy: CacheStrategy) -> Self {
        self.cache_strategy = Some(cache_strategy);
        self
    }

    pub(crate) fn with_counts(
        mut self,
        stock_count: usize,
        history_rows: usize,
        financial_records: usize,
    ) -> Self {
        self.stock_count = Some(stock_count);
        self.history_rows = Some(history_rows);
        self.financial_records = Some(financial_records);
        self
    }

    pub(crate) fn with_candidate_count(mut self, candidate_count: usize) -> Self {
        self.candidate_count = Some(candidate_count);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_bytes(mut self, request_bytes: usize, response_bytes: usize) -> Self {
        self.request_bytes = Some(request_bytes);
        self.response_bytes = Some(response_bytes);
        self
    }

    pub(crate) fn with_outcome(mut self, outcome: Outcome) -> Self {
        self.outcome = Some(outcome);
        self
    }

    #[cfg(any(test, all(target_os = "android", debug_assertions)))]
    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct DataCounts {
    pub(crate) stock_count: usize,
    pub(crate) history_rows: usize,
    pub(crate) financial_records: usize,
}

pub(crate) fn data_counts(value: &Value) -> DataCounts {
    let data = value.get("data").unwrap_or(value);
    let stock_count = data
        .get("stocks")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let history_rows = data
        .get("histories")
        .and_then(Value::as_object)
        .map(|histories| {
            histories
                .values()
                .filter_map(Value::as_array)
                .map(Vec::len)
                .sum()
        })
        .unwrap_or_default();
    let financial_records = data
        .get("financials")
        .and_then(Value::as_object)
        .map_or(0, serde_json::Map::len);
    DataCounts {
        stock_count,
        history_rows,
        financial_records,
    }
}

#[inline]
pub(crate) fn requested(payload: &Value) -> bool {
    #[cfg(debug_assertions)]
    {
        payload
            .get("perf_diagnostics")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = payload;
        false
    }
}

#[cfg(all(target_os = "android", debug_assertions))]
#[link(name = "log")]
unsafe extern "C" {
    fn __android_log_write(
        priority: i32,
        tag: *const std::ffi::c_char,
        text: *const std::ffi::c_char,
    ) -> i32;
}

#[inline(always)]
pub(crate) fn record(enabled: bool, event: PerfEvent) {
    #[cfg(all(target_os = "android", debug_assertions))]
    if enabled {
        let line = event.to_json();
        if let Ok(line) = std::ffi::CString::new(line) {
            // Android logcat is the only sink; diagnostic records are never persisted or uploaded.
            unsafe {
                __android_log_write(4, b"gp-perf\0".as_ptr().cast(), line.as_ptr());
            }
        }
    }

    #[cfg(not(all(target_os = "android", debug_assertions)))]
    {
        let _ = (enabled, event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn diagnostic_record_contains_only_categorical_stages_and_counts() {
        let event = PerfEvent::new(
            Flow::Observe,
            Stage::RustCalculation,
            Duration::from_millis(43),
        )
        .with_cache_strategy(CacheStrategy::Refresh)
        .with_counts(1, 750, 8)
        .with_candidate_count(80)
        .with_bytes(1_200, 6_400)
        .with_outcome(Outcome::Ok);
        let line: serde_json::Value = serde_json::from_str(&event.to_json()).unwrap();

        assert_eq!(
            line,
            json!({
                "flow":"observe",
                "stage":"rust_calculation",
                "elapsed_ms":43,
                "cache_strategy":"refresh",
                "stock_count":1,
                "candidate_count":80,
                "history_rows":750,
                "financial_records":8,
                "request_bytes":1200,
                "response_bytes":6400,
                "outcome":"ok"
            })
        );
        assert!(!event.to_json().contains("600000"));
        assert!(!event.to_json().contains("secret"));
    }

    #[test]
    fn data_counts_report_only_collection_sizes() {
        let counts = data_counts(&json!({
            "stocks": [{"code":"600000.SH", "price":12.34}],
            "histories": {"600000.SH":[{"close":12.34},{"close":12.35}]},
            "financials": {"600000.SH":{"period":"2026Q2","eps":[1,2]}}
        }));

        assert_eq!(counts.stock_count, 1);
        assert_eq!(counts.history_rows, 2);
        assert_eq!(counts.financial_records, 1);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn diagnostics_require_an_explicit_request_flag() {
        assert!(requested(&json!({"perf_diagnostics":true})));
        assert!(!requested(&json!({"perf_diagnostics":false})));
        assert!(!requested(&json!({})));
    }
}
