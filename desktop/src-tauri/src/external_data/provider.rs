use super::{
    refresh::CancellationToken, DataDomain, Market, RefreshRequest, RefreshScope, StoredRecord,
};
use futures::StreamExt;
use reqwest::Response;
use std::{
    collections::BTreeSet,
    future::Future,
    pin::Pin,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub(crate) type ProviderFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ProviderBatch, ProviderError>> + Send + 'a>>;

pub(crate) trait DataProvider: Send + Sync {
    fn source_id(&self) -> &str;
    fn capabilities(&self) -> &[ProviderCapability];
    fn fetch<'a>(
        &'a self,
        request: &'a RefreshRequest,
        context: &'a ProviderContext,
    ) -> ProviderFuture<'a>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProviderCapability {
    pub source_id: String,
    pub domain: DataDomain,
    pub market: Market,
    pub source_tier: String,
    /// Lower values are preferred. A fallback must have a numerically higher priority.
    pub priority: u16,
    pub fields: BTreeSet<String>,
}

impl ProviderCapability {
    pub(crate) fn new<I, S>(
        source_id: impl Into<String>,
        domain: DataDomain,
        market: Market,
        source_tier: impl Into<String>,
        priority: u16,
        fields: I,
    ) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            source_id: source_id.into(),
            domain,
            market,
            source_tier: source_tier.into(),
            priority,
            fields: fields.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProviderBatch {
    pub source_id: String,
    pub records: Vec<StoredRecord>,
    pub response_bytes: usize,
}

impl ProviderBatch {
    pub(crate) fn new(
        source_id: impl Into<String>,
        records: Vec<StoredRecord>,
        response_bytes: usize,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            records,
            response_bytes,
        }
    }

    pub(crate) fn validate(
        &self,
        request: &RefreshRequest,
        capability: &ProviderCapability,
        max_response_bytes: usize,
        max_records: usize,
    ) -> Result<(), ProviderError> {
        if self.source_id != capability.source_id {
            return Err(ProviderError::quality(
                "provider batch source does not match its declared capability",
            ));
        }
        if request.domain != capability.domain || request.market != capability.market {
            return Err(ProviderError::quality(
                "provider capability does not cover the requested domain and market",
            ));
        }
        if self.response_bytes > max_response_bytes {
            return Err(ProviderError::invalid_payload(format!(
                "provider response exceeded {max_response_bytes} bytes"
            )));
        }
        if self.records.len() > max_records {
            return Err(ProviderError::invalid_payload(format!(
                "provider batch exceeded {max_records} records"
            )));
        }
        let requested_entities = match &request.scope {
            RefreshScope::Entities { codes } => {
                if codes.is_empty() {
                    return Err(ProviderError::quality(
                        "entity-scoped refresh must include at least one entity",
                    ));
                }
                Some(codes.iter().cloned().collect::<BTreeSet<_>>())
            }
            RefreshScope::DateRange {
                start_date,
                end_date,
            } => {
                let start = normalize_date(start_date)
                    .ok_or_else(|| ProviderError::quality("refresh start_date is invalid"))?;
                let end = normalize_date(end_date)
                    .ok_or_else(|| ProviderError::quality("refresh end_date is invalid"))?;
                if start > end {
                    return Err(ProviderError::quality(
                        "refresh date window ends before it starts",
                    ));
                }
                Some(BTreeSet::from([start, end]))
            }
            RefreshScope::MarketWide => None,
        };
        let mut unique_keys = BTreeSet::new();
        for record in &self.records {
            if record.domain != request.domain || record.market != request.market {
                return Err(ProviderError::quality(
                    "provider batch contains a record from another domain or market",
                ));
            }
            if record.metadata.source_id != self.source_id {
                return Err(ProviderError::quality(
                    "provider record source does not match the batch source",
                ));
            }
            if !capability.fields.contains(&record.record_key) {
                return Err(ProviderError::quality(format!(
                    "provider does not declare field {}",
                    record.record_key
                )));
            }
            match (&request.scope, &requested_entities) {
                (RefreshScope::Entities { .. }, Some(codes))
                    if !codes.contains(&record.entity_key) =>
                {
                    return Err(ProviderError::quality(
                        "provider batch contains an entity outside the requested scope",
                    ));
                }
                (RefreshScope::DateRange { .. }, Some(bounds)) => {
                    let date = record
                        .metadata
                        .observed_at
                        .as_deref()
                        .and_then(normalize_date)
                        .ok_or_else(|| {
                            ProviderError::quality(
                                "date-scoped provider record has no valid observed date",
                            )
                        })?;
                    let start = bounds.first().expect("date bounds have two entries");
                    let end = bounds.last().expect("date bounds have two entries");
                    if date.as_str() < start.as_str() || date.as_str() > end.as_str() {
                        return Err(ProviderError::quality(
                            "provider batch contains an observation outside the requested date window",
                        ));
                    }
                }
                _ => {}
            }
            let key = (
                record.entity_key.as_str(),
                record.record_key.as_str(),
                record.metadata.source_id.as_str(),
            );
            if !unique_keys.insert(key) {
                return Err(ProviderError::quality(
                    "provider batch contains duplicate record identities",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ProviderError {
    Connection {
        message: String,
    },
    Timeout {
        message: String,
    },
    HttpStatus {
        status: u16,
        retry_after: Option<Duration>,
        message: String,
    },
    InvalidPayload {
        message: String,
    },
    Quality {
        message: String,
    },
    Cancelled {
        message: String,
    },
    Other {
        message: String,
    },
}

impl ProviderError {
    pub(crate) fn connection(message: impl Into<String>) -> Self {
        Self::Connection {
            message: message.into(),
        }
    }

    pub(crate) fn timeout(message: impl Into<String>) -> Self {
        Self::Timeout {
            message: message.into(),
        }
    }

    pub(crate) fn invalid_payload(message: impl Into<String>) -> Self {
        Self::InvalidPayload {
            message: message.into(),
        }
    }

    pub(crate) fn quality(message: impl Into<String>) -> Self {
        Self::Quality {
            message: message.into(),
        }
    }

    pub(crate) fn cancelled() -> Self {
        Self::Cancelled {
            message: "provider request was cancelled".into(),
        }
    }

    pub(crate) fn http(status: u16, retry_after: Option<&str>, now: SystemTime) -> Self {
        Self::HttpStatus {
            status,
            retry_after: retry_after.and_then(|value| parse_retry_after(value, now)),
            message: format!("provider returned HTTP {status}"),
        }
    }

    pub(crate) fn from_reqwest(error: reqwest::Error) -> Self {
        let message = error.to_string();
        if error.is_timeout() {
            Self::timeout(message)
        } else if error.is_connect() {
            Self::connection(message)
        } else {
            Self::Other { message }
        }
    }

    pub(crate) fn retryable(&self) -> bool {
        match self {
            Self::Connection { .. } | Self::Timeout { .. } => true,
            Self::HttpStatus { status, .. } => {
                *status == 408 || *status == 429 || (500..=599).contains(status)
            }
            Self::InvalidPayload { .. }
            | Self::Quality { .. }
            | Self::Cancelled { .. }
            | Self::Other { .. } => false,
        }
    }

    pub(crate) fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::HttpStatus { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    pub(crate) fn category(&self) -> &'static str {
        match self {
            Self::Connection { .. } => "connection",
            Self::Timeout { .. } => "timeout",
            Self::HttpStatus { status, .. } if *status == 408 => "http_408",
            Self::HttpStatus { status, .. } if *status == 429 => "http_429",
            Self::HttpStatus { status, .. } if (500..=599).contains(status) => "http_5xx",
            Self::HttpStatus { .. } => "http_4xx",
            Self::InvalidPayload { .. } => "invalid_payload",
            Self::Quality { .. } => "quality_validation",
            Self::Cancelled { .. } => "cancelled",
            Self::Other { .. } => "other",
        }
    }

    pub(crate) fn message(&self) -> &str {
        match self {
            Self::Connection { message }
            | Self::Timeout { message }
            | Self::InvalidPayload { message }
            | Self::Quality { message }
            | Self::Cancelled { message }
            | Self::Other { message }
            | Self::HttpStatus { message, .. } => message,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FallbackRejection {
    CapabilityMismatch,
    FieldMismatch,
    EntityMismatch,
    SourceMismatch,
    PriorityMismatch,
    PeriodMismatch,
    CurrencyMismatch,
    UnitMismatch,
}

pub(crate) fn fallback_compatibility(
    preferred: &ProviderCapability,
    fallback: &ProviderCapability,
    field: &str,
    preferred_record: &StoredRecord,
    fallback_record: &StoredRecord,
) -> Result<(), FallbackRejection> {
    if preferred.domain != fallback.domain
        || preferred.market != fallback.market
        || preferred_record.domain != fallback_record.domain
        || preferred_record.market != fallback_record.market
    {
        return Err(FallbackRejection::CapabilityMismatch);
    }
    if !preferred.fields.contains(field)
        || !fallback.fields.contains(field)
        || preferred_record.record_key != field
        || fallback_record.record_key != field
    {
        return Err(FallbackRejection::FieldMismatch);
    }
    if preferred_record.entity_key != fallback_record.entity_key {
        return Err(FallbackRejection::EntityMismatch);
    }
    if preferred_record.metadata.source_id != preferred.source_id
        || fallback_record.metadata.source_id != fallback.source_id
        || preferred.source_id == fallback.source_id
    {
        return Err(FallbackRejection::SourceMismatch);
    }
    if fallback.priority <= preferred.priority {
        return Err(FallbackRejection::PriorityMismatch);
    }
    for (keys, rejection) in [
        (
            &["period", "report_period", "raw_report_period"][..],
            FallbackRejection::PeriodMismatch,
        ),
        (&["currency"][..], FallbackRejection::CurrencyMismatch),
        (&["unit"][..], FallbackRejection::UnitMismatch),
    ] {
        let left = dimension_value_any(&preferred_record.payload, keys);
        let right = dimension_value_any(&fallback_record.payload, keys);
        if left.is_none() || right.is_none() || left != right {
            return Err(rejection.clone());
        }
    }
    Ok(())
}

fn dimension_value(payload: &serde_json::Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .filter(|value| !value.is_null())
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string())
        })
}

fn dimension_value_any(payload: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| dimension_value(payload, key))
}

fn normalize_date(value: &str) -> Option<String> {
    let compact = value
        .trim()
        .chars()
        .filter(|character| character.is_ascii_digit())
        .take(8)
        .collect::<String>();
    (compact.len() == 8).then_some(compact)
}

pub(crate) struct ProviderNetworkPolicy {
    pub proxy_mode: String,
    pub proxy_url: Option<String>,
    pub allow_direct_fallback: bool,
}

impl Default for ProviderNetworkPolicy {
    fn default() -> Self {
        Self {
            proxy_mode: "system".into(),
            proxy_url: None,
            allow_direct_fallback: false,
        }
    }
}

pub(crate) struct ProviderContext {
    pub request_id: String,
    pub timeout: Duration,
    pub max_response_bytes: usize,
    pub max_records: usize,
    pub cancellation: CancellationToken,
    pub network_policy: ProviderNetworkPolicy,
}

impl ProviderContext {
    pub(crate) fn new(request_id: impl Into<String>, timeout: Duration) -> Self {
        Self {
            request_id: request_id.into(),
            timeout,
            max_response_bytes: 8 * 1024 * 1024,
            max_records: 50_000,
            cancellation: CancellationToken::new(),
            network_policy: ProviderNetworkPolicy::default(),
        }
    }
}

pub(crate) async fn read_bounded_response(
    response: Response,
    max_bytes: usize,
) -> Result<Vec<u8>, ProviderError> {
    let status = response.status();
    if !status.is_success() {
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok());
        return Err(ProviderError::http(
            status.as_u16(),
            retry_after,
            SystemTime::now(),
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(ProviderError::invalid_payload(format!(
            "provider response exceeded {max_bytes} bytes"
        )));
    }
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(ProviderError::from_reqwest)?;
        if body.len().saturating_add(chunk.len()) > max_bytes {
            return Err(ProviderError::invalid_payload(format!(
                "provider response exceeded {max_bytes} bytes"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn parse_retry_after(value: &str, now: SystemTime) -> Option<Duration> {
    let value = value.trim();
    let raw = if let Ok(seconds) = value.parse::<u64>() {
        Duration::from_secs(seconds)
    } else {
        let timestamp = parse_http_date(value)?;
        let now = now.duration_since(UNIX_EPOCH).ok()?.as_secs();
        Duration::from_secs(timestamp.saturating_sub(now))
    };
    Some(raw.min(Duration::from_secs(300)))
}

fn parse_http_date(value: &str) -> Option<u64> {
    let parts = value.split_ascii_whitespace().collect::<Vec<_>>();
    if parts.len() != 6 || !parts[0].ends_with(',') || parts[5] != "GMT" {
        return None;
    }
    let day = parts[1].parse::<i64>().ok()?;
    let month = match parts[2] {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    let year = parts[3].parse::<i64>().ok()?;
    let mut time = parts[4].split(':');
    let hour = time.next()?.parse::<i64>().ok()?;
    let minute = time.next()?.parse::<i64>().ok()?;
    let second = time.next()?.parse::<i64>().ok()?;
    if time.next().is_some()
        || !(1..=31).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=60).contains(&second)
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    let seconds = days
        .checked_mul(86_400)?
        .checked_add(hour * 3_600 + minute * 60 + second)?;
    u64::try_from(seconds).ok()
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external_data::{DataDomain, Market, RefreshRequest, RefreshScope, StoredRecord};
    use serde_json::json;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::{Duration, UNIX_EPOCH},
    };

    fn local_http_server(response: &'static [u8]) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2_048];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(response);
            let _ = stream.flush();
        });
        (format!("http://{address}/fixture"), worker)
    }

    #[test]
    fn only_transient_http_statuses_are_retryable() {
        for status in [408, 429, 500, 503, 599] {
            assert!(ProviderError::http(status, None, UNIX_EPOCH).retryable());
        }
        for status in [400, 401, 404, 422] {
            assert!(!ProviderError::http(status, None, UNIX_EPOCH).retryable());
        }
        assert!(!ProviderError::invalid_payload("bad json").retryable());
        assert!(!ProviderError::quality("wrong currency").retryable());
    }

    #[test]
    fn retry_after_accepts_delta_and_http_date_but_caps_at_five_minutes() {
        let now = UNIX_EPOCH + Duration::from_secs(1_445_412_480); // 2015-10-21 07:28:00 UTC.
        let delta = ProviderError::http(429, Some("120"), now);
        assert_eq!(delta.retry_after(), Some(Duration::from_secs(120)));

        let date = ProviderError::http(503, Some("Wed, 21 Oct 2015 07:30:00 GMT"), now);
        assert_eq!(date.retry_after(), Some(Duration::from_secs(120)));

        let capped = ProviderError::http(429, Some("900"), now);
        assert_eq!(capped.retry_after(), Some(Duration::from_secs(300)));
        let malformed = ProviderError::http(429, Some("tomorrow"), now);
        assert_eq!(malformed.retry_after(), None);
    }

    fn capability(source: &str, priority: u16) -> ProviderCapability {
        ProviderCapability::new(
            source,
            DataDomain::FinancialSnapshot,
            Market::HongKong,
            "public",
            priority,
            ["revenue"],
        )
    }

    fn financial_record(source: &str, period: &str, currency: &str, unit: &str) -> StoredRecord {
        StoredRecord::new(
            DataDomain::FinancialSnapshot,
            Market::HongKong,
            "00700.HK",
            "revenue",
            source,
            "public",
            "2026-10-20T00:00:00Z",
            None,
            None,
            "unverified",
            vec![],
            json!({"period": period, "currency": currency, "unit": unit, "value": 100}),
        )
    }

    #[test]
    fn fallback_requires_same_field_entity_period_currency_and_unit() {
        let primary_capability = capability("preferred", 10);
        let fallback_capability = capability("fallback", 20);
        let primary = financial_record("preferred", "FY2025", "HKD", "million");
        let compatible = financial_record("fallback", "FY2025", "HKD", "million");
        assert!(fallback_compatibility(
            &primary_capability,
            &fallback_capability,
            "revenue",
            &primary,
            &compatible,
        )
        .is_ok());

        for (period, currency, unit) in [
            ("FY2024", "HKD", "million"),
            ("FY2025", "CNY", "million"),
            ("FY2025", "HKD", "thousand"),
        ] {
            assert!(fallback_compatibility(
                &primary_capability,
                &fallback_capability,
                "revenue",
                &primary,
                &financial_record("fallback", period, currency, unit),
            )
            .is_err());
        }
        assert!(fallback_compatibility(
            &primary_capability,
            &fallback_capability,
            "eps",
            &primary,
            &compatible,
        )
        .is_err());
    }

    #[test]
    fn provider_batch_rejects_wrong_scope_source_and_oversized_response() {
        let request = RefreshRequest {
            domain: DataDomain::FinancialSnapshot,
            market: Market::HongKong,
            scope: RefreshScope::Entities {
                codes: vec!["00700.HK".into()],
            },
            request_id: Some("request-1".into()),
        };
        let capability = capability("eastmoney", 10);
        let valid = ProviderBatch::new(
            "eastmoney",
            vec![financial_record("eastmoney", "FY2025", "HKD", "million")],
            128,
        );
        assert!(valid.validate(&request, &capability, 1024, 10).is_ok());

        let oversized = ProviderBatch::new(
            "eastmoney",
            vec![financial_record("eastmoney", "FY2025", "HKD", "million")],
            1025,
        );
        assert!(oversized.validate(&request, &capability, 1024, 10).is_err());

        let wrong_source = ProviderBatch::new(
            "other-source",
            vec![financial_record("other-source", "FY2025", "HKD", "million")],
            128,
        );
        assert!(wrong_source
            .validate(&request, &capability, 1024, 10)
            .is_err());

        let wrong_scope = RefreshRequest {
            scope: RefreshScope::Entities {
                codes: vec!["00005.HK".into()],
            },
            ..request
        };
        assert!(valid.validate(&wrong_scope, &capability, 1024, 10).is_err());

        let empty_scope = RefreshRequest {
            scope: RefreshScope::Entities { codes: vec![] },
            ..wrong_scope
        };
        assert!(valid.validate(&empty_scope, &capability, 1024, 10).is_err());
    }

    #[test]
    fn bounded_response_reader_accepts_local_fixture_and_enforces_body_limit() {
        let (url, worker) = local_http_server(
            b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
        );
        let body = tauri::async_runtime::block_on(async {
            let client = reqwest::Client::builder().no_proxy().build().unwrap();
            let response = client.get(url).send().await.unwrap();
            read_bounded_response(response, 5).await.unwrap()
        });
        worker.join().unwrap();
        assert_eq!(body, b"hello");

        let (url, worker) = local_http_server(
            b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n0123456789",
        );
        let error = tauri::async_runtime::block_on(async {
            let client = reqwest::Client::builder().no_proxy().build().unwrap();
            let response = client.get(url).send().await.unwrap();
            read_bounded_response(response, 5).await.unwrap_err()
        });
        worker.join().unwrap();
        assert!(matches!(error, ProviderError::InvalidPayload { .. }));
    }

    #[test]
    fn bounded_response_reader_classifies_retry_after_from_local_http_response() {
        let (url, worker) = local_http_server(
            b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 60\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        let error = tauri::async_runtime::block_on(async {
            let client = reqwest::Client::builder().no_proxy().build().unwrap();
            let response = client.get(url).send().await.unwrap();
            read_bounded_response(response, 128).await.unwrap_err()
        });
        worker.join().unwrap();
        assert_eq!(error.retry_after(), Some(Duration::from_secs(60)));
    }
}
