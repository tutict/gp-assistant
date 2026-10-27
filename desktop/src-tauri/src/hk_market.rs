use crate::market_domain::{
    CapabilityId, CapabilityStatus, CoverageSummary, CurrencyCode, MarketCapability, MarketId,
    SecurityId, SecurityKind,
};
use calamine::{open_workbook_from_rs, Data, Reader, Xlsx};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager};

pub(crate) const HK_MARKET_DATA_FILE: &str = "hk-market-data.json";
const HKEX_SECURITIES_SOURCE: &str =
    "https://www.hkex.com.hk/eng/services/trading/securities/securitieslists/ListOfSecurities.xlsx";
const TENCENT_HK_QUOTE_ENDPOINT: &str = "https://qt.gtimg.cn/q=";
const TENCENT_HK_DAILY_KLINE_ENDPOINT: &str = "https://web.ifzq.gtimg.cn/appstock/app/fqkline/get";
const HKEX_MAX_WORKBOOK_BYTES: usize = 16 * 1024 * 1024;
const HK_REQUEST_TIMEOUT_SECS: u64 = 8;
const HK_FINANCIAL_ENDPOINT: &str = "https://datacenter.eastmoney.com/securities/api/data/v1/get";
const HK_FX_ENDPOINT: &str = "https://api.frankfurter.dev/v2/providers/ecb/rate/HKD/CNY";
const HKEX_STOCK_SEARCH_ENDPOINT: &str = "https://www1.hkexnews.hk/search/partial.do";
const TENCENT_HK_SUPPORTED_QUOTE_CURRENCIES: [&str; 1] = ["HKD"];
static HK_CACHE_WRITE_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HkClassificationStatus {
    #[default]
    Unverified,
    VerifiedOrdinaryShare,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct HkSecurity {
    pub code: String,
    pub name: String,
    pub currency: String,
    pub board_lot: Option<u32>,
    pub security_type: String,
    #[serde(default)]
    pub classification_status: HkClassificationStatus,
}

impl HkSecurity {
    pub(crate) fn cached_quote_matches_security_master(&self, quote: &HkQuote) -> bool {
        normalize_hk_currency(&quote.currency)
            .zip(normalize_hk_currency(&self.currency))
            .is_some_and(|(quote_currency, listing_currency)| quote_currency == listing_currency)
    }
}

fn observation_security_id(security: &HkSecurity) -> Result<SecurityId, String> {
    if security.classification_status != HkClassificationStatus::VerifiedOrdinaryShare {
        return Err(format!(
            "HK security {} has unverified instrument classification; ordinary-share observation is disabled",
            security.code
        ));
    }
    SecurityId::new(MarketId::Hk, &security.code, SecurityKind::OrdinaryShare)
}

fn ensure_hk_provider_ready(provider_id: &str) -> Result<(), String> {
    match provider_id {
        "frankfurter_ecb" => Ok(()),
        "hkex" => Err("HKEX security-directory refresh is disabled until written permission for automated access is documented".to_owned()),
        "tencent" => Err("Tencent HK quote/history refresh is disabled until provider-specific usage terms are documented".to_owned()),
        "eastmoney_hk" => Err("Eastmoney HK financial refresh is disabled until provider-specific usage and redistribution terms are documented".to_owned()),
        "hkexnews" => Err("HKEXnews metadata refresh is disabled until written permission for automated access is documented".to_owned()),
        _ => Err(format!("unknown HK market-data provider {provider_id}; refresh is disabled until readiness is reviewed")),
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct HkMarketCache {
    pub schema_version: u32,
    pub securities: Vec<HkSecurity>,
    pub securities_source: String,
    pub securities_updated_at: Option<String>,
    #[serde(default)]
    pub securities_last_attempt_at: Option<String>,
    #[serde(default)]
    pub securities_last_error: Option<String>,
    #[serde(default)]
    pub quotes: HashMap<String, HkQuote>,
    #[serde(default)]
    pub histories: HashMap<String, HkHistoryRecord>,
    #[serde(default)]
    pub quote_errors: HashMap<String, String>,
    #[serde(default)]
    pub history_errors: HashMap<String, String>,
    #[serde(default)]
    pub financials: HashMap<String, HkFinancialSnapshot>,
    #[serde(default)]
    pub fx_rate: Option<HkFxRate>,
    #[serde(default)]
    pub fx_error: Option<String>,
    #[serde(default)]
    pub disclosures: HashMap<String, HkDisclosureMetadata>,
    #[serde(default)]
    pub financial_errors: HashMap<String, String>,
    #[serde(default)]
    pub disclosure_errors: HashMap<String, String>,
}

impl Default for HkMarketCache {
    fn default() -> Self {
        Self {
            schema_version: 1,
            securities: Vec::new(),
            securities_source: String::new(),
            securities_updated_at: None,
            securities_last_attempt_at: None,
            securities_last_error: None,
            quotes: HashMap::new(),
            histories: HashMap::new(),
            quote_errors: HashMap::new(),
            history_errors: HashMap::new(),
            financials: HashMap::new(),
            fx_rate: None,
            fx_error: None,
            disclosures: HashMap::new(),
            financial_errors: HashMap::new(),
            disclosure_errors: HashMap::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct HkQuote {
    pub code: String,
    pub name: String,
    pub price: f64,
    pub change_pct: Option<f64>,
    pub volume_shares: Option<u64>,
    pub turnover_hkd: Option<f64>,
    #[serde(default)]
    pub pe: Option<f64>,
    #[serde(default)]
    pub pb: Option<f64>,
    #[serde(default)]
    pub dividend_yield_percent: Option<f64>,
    #[serde(default)]
    pub market_cap_hkd: Option<f64>,
    #[serde(default)]
    pub total_shares: Option<u64>,
    #[serde(default)]
    pub circulating_shares: Option<u64>,
    pub currency: String,
    pub quote_time: Option<String>,
    pub source: String,
    #[serde(default)]
    pub fetched_at: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct HkDailyBar {
    pub date: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume_shares: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct HkHistoryRecord {
    pub bars: Vec<HkDailyBar>,
    pub source: String,
    pub fetched_at: String,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct HkFinancialMetric {
    pub value: Option<f64>,
    pub currency: String,
    pub unit: String,
    pub source: String,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct HkFinancialSnapshot {
    pub code: String,
    pub currency: String,
    pub raw_report_period: String,
    pub normalized_period: String,
    pub report_date: Option<String>,
    pub disclosed_at: Option<String>,
    pub candidate_source: String,
    pub candidate_fetched_at: String,
    pub official_source_url: Option<String>,
    pub official_source_name: Option<String>,
    pub verification_status: String,
    pub revenue: HkFinancialMetric,
    pub net_profit: HkFinancialMetric,
    pub eps: HkFinancialMetric,
    pub roe: HkFinancialMetric,
    pub pe: HkFinancialMetric,
    pub pb: HkFinancialMetric,
    pub dividend_yield: HkFinancialMetric,
    pub market_cap: HkFinancialMetric,
    pub total_shares: HkFinancialMetric,
    pub circulating_shares: HkFinancialMetric,
    #[serde(default)]
    pub official_values: HashMap<String, HkFinancialMetric>,
    #[serde(default)]
    pub verification_conflicts: Vec<String>,
    #[serde(default)]
    pub field_errors: HashMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct HkFinancialResult {
    pub market: String,
    pub code: String,
    pub snapshot: Option<HkFinancialSnapshot>,
    pub fx_rate: Option<HkFxRate>,
    pub official_disclosure: Option<HkDisclosureMetadata>,
    pub candidate_updated_at: Option<String>,
    pub fx_updated_at: Option<String>,
    pub official_checked_at: Option<String>,
    pub notes: Vec<String>,
    pub stale: bool,
    pub field_errors: HashMap<String, String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[cfg(test)]
pub(crate) struct HkFinancialCurrencyPair {
    pub hkd: Option<f64>,
    pub cny: Option<f64>,
    pub fx_observed_at: Option<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct HkFinancialRefreshMergeResult {
    pub snapshot_updated: bool,
    pub fx_updated: bool,
    pub disclosure_updated: bool,
    pub notes: Vec<String>,
    pub field_errors: HashMap<String, String>,
}

pub(crate) trait HkFinancialProvider: Send + Sync {
    fn endpoint(&self, code: &str) -> Result<String, String>;
    fn parse(
        &self,
        text: &str,
        code: &str,
        fetched_at: &str,
    ) -> Result<HkFinancialSnapshot, String>;
}

struct EastmoneyHkFinancialProvider;

impl HkFinancialProvider for EastmoneyHkFinancialProvider {
    fn endpoint(&self, code: &str) -> Result<String, String> {
        let canonical =
            canonical_hk_code(code).ok_or_else(|| format!("invalid HK stock code: {code}"))?;
        Ok(format!("{HK_FINANCIAL_ENDPOINT}?reportName=RPT_HKF10_FN_GMAININDICATOR&columns=ALL&filter=(SECUCODE%3D%22{canonical}.HK%22)&pageNumber=1&pageSize=1&sortColumns=REPORT_DATE&sortTypes=-1&source=HSF10&client=PC"))
    }

    fn parse(
        &self,
        text: &str,
        code: &str,
        fetched_at: &str,
    ) -> Result<HkFinancialSnapshot, String> {
        parse_hk_financial_snapshot(text, code, fetched_at)
    }
}

pub(crate) trait HkDisclosureProvider: Send + Sync {
    fn endpoint(&self, code: &str) -> Result<String, String>;
    fn parse(
        &self,
        text: &str,
        code: &str,
        checked_at: &str,
    ) -> Result<HkDisclosureMetadata, String>;
}

struct HkexNewsDisclosureProvider;

impl HkDisclosureProvider for HkexNewsDisclosureProvider {
    fn endpoint(&self, code: &str) -> Result<String, String> {
        let canonical =
            canonical_hk_code(code).ok_or_else(|| format!("invalid HK stock code: {code}"))?;
        Ok(format!("{HKEX_STOCK_SEARCH_ENDPOINT}?lang=EN&type=A&name={canonical}&market=SEHK&callback=callback"))
    }

    fn parse(
        &self,
        text: &str,
        code: &str,
        checked_at: &str,
    ) -> Result<HkDisclosureMetadata, String> {
        parse_hk_disclosure_lookup(text, code, checked_at)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct HkFxRate {
    pub pair: String,
    pub rate: f64,
    pub source: String,
    pub fetched_at: String,
    pub observed_at: String,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct HkDisclosureMetadata {
    pub source_url: Option<String>,
    pub source_name: String,
    pub report_period: Option<String>,
    pub disclosed_at: Option<String>,
    pub checked_at: String,
    pub status: String,
    pub error: Option<String>,
}

pub(crate) fn parse_hk_financial_snapshot(
    text: &str,
    code: &str,
    fetched_at: &str,
) -> Result<HkFinancialSnapshot, String> {
    let canonical =
        canonical_hk_code(code).ok_or_else(|| format!("invalid HK stock code: {code}"))?;
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| format!("decode HK financial payload failed: {error}"))?;
    let row = value
        .get("result")
        .and_then(|result| result.get("data"))
        .and_then(serde_json::Value::as_array)
        .and_then(|rows| rows.first())
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "HK financial payload contains no rows".to_string())?;
    let raw_period = string_value(row, &["REPORT_DATA_TYPE", "DATE_TYPE", "REPORT_DATE_NAME"])
        .unwrap_or_else(|| "未知报告期".to_string());
    let normalized_period = normalize_hk_period(
        &raw_period,
        row.get("REPORT_DATE").and_then(serde_json::Value::as_str),
    );
    let raw_currency = string_value(row, &["CURRENCY_ABBR", "CURRENCY"])
        .ok_or_else(|| "HK financial payload is missing the source currency".to_string())?;
    let currency = normalize_hk_financial_currency(&raw_currency).ok_or_else(|| {
        format!("HK financial payload has an unsupported currency: {raw_currency}")
    })?;
    let source = "Eastmoney HK financial indicators";
    let metric =
        |value: Option<f64>, metric_currency: &str, unit: &str, reason: &str| HkFinancialMetric {
            value,
            currency: metric_currency.to_string(),
            unit: unit.to_string(),
            source: source.to_string(),
            reason: value.is_none().then(|| reason.to_string()),
        };
    let metric_currency = currency.clone();
    let amount = |value| {
        metric(
            value,
            &metric_currency,
            "currency_total",
            "Provider did not return this financial field",
        )
    };
    let per_share = |value| {
        metric(
            value,
            &metric_currency,
            "currency/share",
            "Provider did not return this per-share field",
        )
    };
    let percent = |value| {
        metric(
            value,
            "",
            "percent",
            "Provider did not return this ratio field",
        )
    };
    let mut snapshot = HkFinancialSnapshot {
        code: canonical,
        currency: currency.clone(),
        raw_report_period: raw_period,
        normalized_period,
        report_date: normalize_date_string(string_value(row, &["REPORT_DATE", "STD_REPORT_DATE"])),
        disclosed_at: normalize_date_string(string_value(row, &["NOTICE_DATE"])),
        candidate_source: "Eastmoney HK financial indicators".to_string(),
        candidate_fetched_at: fetched_at.to_string(),
        official_source_url: None,
        official_source_name: None,
        verification_status: "unverified".to_string(),
        revenue: amount(number_value(row, &["OPERATE_INCOME"])),
        net_profit: amount(number_value(row, &["HOLDER_PROFIT", "HOLDER_PROFITACTUAL"])),
        eps: per_share(number_value(row, &["BASIC_EPS"])),
        roe: percent(number_value(row, &["ROE", "ROE_AVG"])),
        pe: metric(None, "", "ratio", "Awaiting Tencent quote data"),
        pb: metric(None, "", "ratio", "Awaiting Tencent quote data"),
        dividend_yield: metric(None, "", "percent", "Awaiting Tencent quote data"),
        market_cap: metric(None, "HKD", "currency_total", "Awaiting Tencent quote data"),
        total_shares: metric(None, "", "shares", "Awaiting Tencent quote data"),
        circulating_shares: metric(None, "", "shares", "Awaiting Tencent quote data"),
        official_values: HashMap::new(),
        verification_conflicts: Vec::new(),
        field_errors: HashMap::new(),
    };
    sync_hk_financial_field_errors(&mut snapshot);
    Ok(snapshot)
}

fn normalize_hk_financial_currency(value: &str) -> Option<String> {
    match value.trim().to_ascii_uppercase().as_str() {
        "人民币" | "CNY" | "RMB" => Some("CNY".to_string()),
        "港币" | "港元" | "HKD" => Some("HKD".to_string()),
        "美元" | "USD" => Some("USD".to_string()),
        "欧元" | "EUR" => Some("EUR".to_string()),
        "英镑" | "GBP" => Some("GBP".to_string()),
        other if other.len() == 3 && other.chars().all(|ch| ch.is_ascii_alphabetic()) => {
            Some(other.to_string())
        }
        _ => None,
    }
}

fn sync_hk_financial_field_errors(snapshot: &mut HkFinancialSnapshot) {
    for (name, metric) in [
        ("revenue", &snapshot.revenue),
        ("net_profit", &snapshot.net_profit),
        ("eps", &snapshot.eps),
        ("roe", &snapshot.roe),
        ("pe", &snapshot.pe),
        ("pb", &snapshot.pb),
        ("dividend_yield", &snapshot.dividend_yield),
        ("market_cap", &snapshot.market_cap),
        ("total_shares", &snapshot.total_shares),
        ("circulating_shares", &snapshot.circulating_shares),
    ] {
        if let Some(reason) = metric.reason.as_ref() {
            snapshot
                .field_errors
                .insert(name.to_string(), reason.clone());
        } else {
            snapshot.field_errors.remove(name);
        }
    }
}

pub(crate) fn parse_hk_fx_rate(text: &str, fetched_at: &str) -> Result<HkFxRate, String> {
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| format!("decode HKD/CNY rate failed: {error}"))?;
    let base = value.get("base").and_then(serde_json::Value::as_str);
    let quote = value.get("quote").and_then(serde_json::Value::as_str);
    if base != Some("HKD") || quote != Some("CNY") {
        return Err(format!(
            "HKD/CNY provider response has an unexpected currency pair: {}/{}",
            base.unwrap_or("missing"),
            quote.unwrap_or("missing")
        ));
    }
    let rate = value
        .get("rate")
        .and_then(serde_json::Value::as_f64)
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| "HKD/CNY rate is missing or invalid".to_string())?;
    let observed_at = value
        .get("date")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "HKD/CNY rate date is missing".to_string())?;
    Ok(HkFxRate {
        pair: "HKD/CNY".to_string(),
        rate,
        source: "European Central Bank (via Frankfurter)".to_string(),
        fetched_at: fetched_at.to_string(),
        observed_at: observed_at.to_string(),
        last_error: None,
    })
}

#[cfg(test)]
pub(crate) fn hk_financial_currency_pair(
    metric: &HkFinancialMetric,
    fx: Option<&HkFxRate>,
) -> HkFinancialCurrencyPair {
    let Some(value) = metric.value else {
        return HkFinancialCurrencyPair::default();
    };
    let Some(rate) = fx.filter(|rate| rate.rate.is_finite() && rate.rate > 0.0) else {
        return if metric.currency == "CNY" {
            HkFinancialCurrencyPair {
                hkd: None,
                cny: Some(value),
                fx_observed_at: None,
            }
        } else {
            HkFinancialCurrencyPair {
                hkd: Some(value),
                cny: None,
                fx_observed_at: None,
            }
        };
    };
    if metric.currency == "CNY" {
        HkFinancialCurrencyPair {
            hkd: Some(value / rate.rate),
            cny: Some(value),
            fx_observed_at: Some(rate.observed_at.clone()),
        }
    } else if metric.currency == "HKD" {
        HkFinancialCurrencyPair {
            hkd: Some(value),
            cny: Some(value * rate.rate),
            fx_observed_at: Some(rate.observed_at.clone()),
        }
    } else {
        HkFinancialCurrencyPair {
            hkd: None,
            cny: Some(value),
            fx_observed_at: None,
        }
    }
}

pub(crate) fn verify_hk_financial_snapshot(
    snapshot: &mut HkFinancialSnapshot,
    official: HashMap<String, HkFinancialMetric>,
) {
    let mut conflicts = Vec::new();
    let mut comparable_count = 0usize;
    for (key, official_value) in &official {
        let candidate = match key.as_str() {
            "revenue" => &snapshot.revenue,
            "net_profit" => &snapshot.net_profit,
            "eps" => &snapshot.eps,
            "roe" => &snapshot.roe,
            _ => continue,
        };
        if let (Some(left), Some(right)) = (candidate.value, official_value.value) {
            if candidate.currency != official_value.currency
                || candidate.unit != official_value.unit
            {
                continue;
            }
            comparable_count += 1;
            let denominator = left.abs().max(right.abs()).max(1.0);
            if (left - right).abs() / denominator > 0.01 {
                conflicts.push(key.clone());
            }
        }
    }
    snapshot.official_values = official;
    snapshot.verification_conflicts = conflicts.clone();
    snapshot.verification_status = if !conflicts.is_empty() {
        "conflict".to_string()
    } else if comparable_count > 0 {
        "verified".to_string()
    } else if snapshot.official_source_url.is_some() {
        "metadata_only".to_string()
    } else {
        "unverified".to_string()
    };
}

pub(crate) fn parse_hk_disclosure_lookup(
    text: &str,
    code: &str,
    checked_at: &str,
) -> Result<HkDisclosureMetadata, String> {
    let canonical =
        canonical_hk_code(code).ok_or_else(|| format!("invalid HK stock code: {code}"))?;
    let json_text = text
        .trim()
        .strip_prefix("callback(")
        .and_then(|value| value.strip_suffix(");"))
        .unwrap_or(text.trim());
    let value: serde_json::Value = serde_json::from_str(json_text)
        .map_err(|error| format!("decode HKEXnews lookup failed: {error}"))?;
    let stock = value
        .get("stockInfo")
        .and_then(serde_json::Value::as_array)
        .and_then(|items| {
            items.iter().find(|item| {
                item.get("code").and_then(serde_json::Value::as_str) == Some(canonical.as_str())
            })
        });
    let Some(stock_id) = stock
        .and_then(|item| item.get("stockId"))
        .and_then(serde_json::Value::as_i64)
    else {
        return Ok(build_hk_disclosure_metadata(&canonical, checked_at));
    };
    Ok(HkDisclosureMetadata { source_url: Some(format!("https://www1.hkexnews.hk/search/titlesearch.xhtml?lang=EN&market=SEHK&stockId={stock_id}")), source_name: "HKEXnews".to_string(), report_period: None, disclosed_at: None, checked_at: checked_at.to_string(), status: "metadata_only".to_string(), error: None })
}

#[cfg(test)]
pub(crate) fn hkex_disclosure_endpoint(code: &str) -> Result<String, String> {
    HkexNewsDisclosureProvider.endpoint(code)
}

#[cfg(test)]
pub(crate) fn eastmoney_hk_financial_endpoint(code: &str) -> Result<String, String> {
    EastmoneyHkFinancialProvider.endpoint(code)
}

pub(crate) fn merge_hk_financial_refresh(
    cache: &mut HkMarketCache,
    code: &str,
    snapshot: Result<HkFinancialSnapshot, String>,
    fx: Result<HkFxRate, String>,
    disclosure: Result<HkDisclosureMetadata, String>,
) -> HkFinancialRefreshMergeResult {
    let Some(code) = canonical_hk_code(code) else {
        return HkFinancialRefreshMergeResult {
            notes: vec!["invalid HK stock code".to_string()],
            ..Default::default()
        };
    };
    let mut result = HkFinancialRefreshMergeResult::default();
    match snapshot {
        Ok(mut value) => {
            if let Some(quote) = cache.quotes.get(&code) {
                enrich_hk_financial_snapshot_from_quote(&mut value, quote);
            }
            cache.financials.insert(code.clone(), value);
            cache.financial_errors.remove(&code);
            result.snapshot_updated = true;
        }
        Err(error) => {
            cache.financial_errors.insert(code.clone(), error.clone());
            result
                .field_errors
                .insert("financial".to_string(), error.clone());
            result.notes.push(format!("财务：{error}"));
        }
    }
    match fx {
        Ok(value) => {
            cache.fx_rate = Some(value);
            cache.fx_error = None;
            result.fx_updated = true;
        }
        Err(error) => {
            cache.fx_error = Some(error.clone());
            if let Some(value) = cache.fx_rate.as_mut() {
                value.last_error = Some(error.clone());
            }
            result
                .field_errors
                .insert("fx_rate".to_string(), error.clone());
            result.notes.push(format!("汇率：{error}"));
        }
    }
    match disclosure {
        Ok(value) => {
            if value.status == "unavailable" {
                let error = value
                    .error
                    .clone()
                    .unwrap_or_else(|| "HKEXnews issuer metadata is unavailable".to_string());
                result
                    .field_errors
                    .insert("official_disclosure".to_string(), error.clone());
                result.notes.push(format!("官方披露：{error}"));
            }
            cache.disclosures.insert(code.clone(), value);
            cache.disclosure_errors.remove(&code);
            result.disclosure_updated = true;
        }
        Err(error) => {
            cache.disclosure_errors.insert(code.clone(), error.clone());
            result
                .field_errors
                .insert("official_disclosure".to_string(), error.clone());
            result.notes.push(format!("官方披露：{error}"));
        }
    }
    let disclosure = cache.disclosures.get(&code).cloned();
    if let Some(snapshot) = cache.financials.get_mut(&code) {
        if let Some(disclosure) = disclosure {
            if disclosure.status == "metadata_only" {
                snapshot.official_source_url = disclosure.source_url;
                snapshot.official_source_name = Some(disclosure.source_name);
            } else {
                snapshot.official_source_url = None;
                snapshot.official_source_name = None;
            }
        }
        let official_values = snapshot.official_values.clone();
        verify_hk_financial_snapshot(snapshot, official_values);
        sync_hk_financial_field_errors(snapshot);
    }
    result
}

pub(crate) fn hk_observation_should_refresh(data_policy: &str) -> bool {
    data_policy == "refresh"
}

pub(crate) fn build_hk_financial_result(cache: &HkMarketCache, code: &str) -> HkFinancialResult {
    let code = canonical_hk_code(code).unwrap_or_else(|| code.to_string());
    let snapshot = cache.financials.get(&code).cloned();
    let fx_rate = cache.fx_rate.clone();
    let official_disclosure = cache.disclosures.get(&code).cloned();
    let mut field_errors = snapshot
        .as_ref()
        .map(|value| value.field_errors.clone())
        .unwrap_or_default();

    if let Some(error) = cache.financial_errors.get(&code) {
        field_errors.insert("financial".to_string(), error.clone());
    } else if snapshot.is_none() {
        field_errors.insert(
            "financial".to_string(),
            "尚无财务缓存，请点击刷新财务。".to_string(),
        );
    }
    if let Some(error) = cache
        .fx_error
        .as_ref()
        .or_else(|| fx_rate.as_ref().and_then(|rate| rate.last_error.as_ref()))
    {
        field_errors.insert("fx_rate".to_string(), error.clone());
    } else if fx_rate.is_none() {
        field_errors.insert("fx_rate".to_string(), "尚无 HKD/CNY 汇率缓存。".to_string());
    }
    if let Some(error) = cache.disclosure_errors.get(&code) {
        field_errors.insert("official_disclosure".to_string(), error.clone());
    } else if let Some(disclosure) = official_disclosure.as_ref() {
        if disclosure.status == "unavailable" {
            field_errors.insert(
                "official_disclosure".to_string(),
                disclosure
                    .error
                    .clone()
                    .unwrap_or_else(|| "官方披露元数据不可用。".to_string()),
            );
        }
    } else {
        field_errors.insert(
            "official_disclosure".to_string(),
            "尚无官方披露元数据缓存。".to_string(),
        );
    }

    let notes = ["financial", "fx_rate", "official_disclosure"]
        .iter()
        .filter_map(|field| {
            field_errors
                .get(*field)
                .map(|error| format!("{field}：{error}"))
        })
        .collect::<Vec<_>>();
    HkFinancialResult {
        market: "hk".to_string(),
        code,
        candidate_updated_at: snapshot
            .as_ref()
            .map(|value| value.candidate_fetched_at.clone()),
        fx_updated_at: fx_rate.as_ref().map(|value| value.fetched_at.clone()),
        official_checked_at: official_disclosure
            .as_ref()
            .map(|value| value.checked_at.clone()),
        stale: !field_errors.is_empty(),
        snapshot,
        fx_rate,
        official_disclosure,
        notes,
        field_errors,
    }
}

pub(crate) async fn fetch_hk_financial_snapshot(
    client: &reqwest::Client,
    code: &str,
    fetched_at: &str,
) -> Result<HkFinancialSnapshot, String> {
    ensure_hk_provider_ready("eastmoney_hk")?;
    fetch_hk_financial_snapshot_from(client, &EastmoneyHkFinancialProvider, code, fetched_at).await
}

async fn fetch_hk_financial_snapshot_from(
    client: &reqwest::Client,
    provider: &dyn HkFinancialProvider,
    code: &str,
    fetched_at: &str,
) -> Result<HkFinancialSnapshot, String> {
    let url = provider.endpoint(code)?;
    let timeout = std::time::Duration::from_secs(HK_REQUEST_TIMEOUT_SECS);
    let text = fetch_hk_provider_text(
        client,
        &url,
        timeout,
        8 * 1024 * 1024,
        &format!("hk-financial-{code}"),
    )
    .await?;
    provider.parse(&text, code, fetched_at)
}

pub(crate) async fn fetch_hk_fx_rate(
    client: &reqwest::Client,
    fetched_at: &str,
) -> Result<HkFxRate, String> {
    ensure_hk_provider_ready("frankfurter_ecb")?;
    let timeout = std::time::Duration::from_secs(HK_REQUEST_TIMEOUT_SECS);
    let text =
        fetch_hk_provider_text(client, HK_FX_ENDPOINT, timeout, 256 * 1024, "hkd-cny-fx").await?;
    parse_hk_fx_rate(&text, fetched_at)
}

pub(crate) async fn fetch_hk_disclosure_metadata(
    client: &reqwest::Client,
    code: &str,
    checked_at: &str,
) -> Result<HkDisclosureMetadata, String> {
    ensure_hk_provider_ready("hkexnews")?;
    fetch_hk_disclosure_metadata_from(client, &HkexNewsDisclosureProvider, code, checked_at).await
}

async fn fetch_hk_disclosure_metadata_from(
    client: &reqwest::Client,
    provider: &dyn HkDisclosureProvider,
    code: &str,
    checked_at: &str,
) -> Result<HkDisclosureMetadata, String> {
    let url = provider.endpoint(code)?;
    let timeout = std::time::Duration::from_secs(HK_REQUEST_TIMEOUT_SECS);
    let text = fetch_hk_provider_text(
        client,
        &url,
        timeout,
        2 * 1024 * 1024,
        &format!("hk-disclosure-{code}"),
    )
    .await?;
    provider.parse(&text, code, checked_at)
}

pub(crate) fn build_hk_disclosure_metadata(code: &str, checked_at: &str) -> HkDisclosureMetadata {
    let canonical = canonical_hk_code(code).unwrap_or_else(|| code.to_string());
    HkDisclosureMetadata {
        source_url: None,
        source_name: "HKEXnews".to_string(),
        report_period: None,
        disclosed_at: None,
        checked_at: checked_at.to_string(),
        status: "unavailable".to_string(),
        error: Some(format!("No exact HKEXnews issuer match for {canonical}")),
    }
}

pub(crate) fn enrich_hk_financial_snapshot_from_quote(
    snapshot: &mut HkFinancialSnapshot,
    quote: &HkQuote,
) {
    snapshot.pe = quote_metric(quote.pe, "", "ratio", "Tencent Finance");
    snapshot.pb = quote_metric(quote.pb, "", "ratio", "Tencent Finance");
    snapshot.dividend_yield = quote_metric(
        quote.dividend_yield_percent,
        "",
        "percent",
        "Tencent Finance",
    );
    snapshot.market_cap = quote_metric(
        quote.market_cap_hkd,
        "HKD",
        "currency_total",
        "Tencent Finance",
    );
    snapshot.total_shares = quote_metric(
        quote.total_shares.map(|value| value as f64),
        "",
        "shares",
        "Tencent Finance",
    );
    snapshot.circulating_shares = quote_metric(
        quote.circulating_shares.map(|value| value as f64),
        "",
        "shares",
        "Tencent Finance",
    );
    for (key, metric) in [
        ("pe", &snapshot.pe),
        ("pb", &snapshot.pb),
        ("dividend_yield", &snapshot.dividend_yield),
        ("market_cap", &snapshot.market_cap),
        ("total_shares", &snapshot.total_shares),
        ("circulating_shares", &snapshot.circulating_shares),
    ] {
        if let Some(reason) = metric.reason.as_ref() {
            snapshot
                .field_errors
                .insert(key.to_string(), reason.clone());
        } else {
            snapshot.field_errors.remove(key);
        }
    }
}

fn quote_metric(value: Option<f64>, currency: &str, unit: &str, source: &str) -> HkFinancialMetric {
    HkFinancialMetric {
        value,
        currency: currency.to_string(),
        unit: unit.to_string(),
        source: source.to_string(),
        reason: value
            .is_none()
            .then(|| "Tencent quote did not return this field".to_string()),
    }
}

fn string_value(row: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        row.get(*key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    })
}
fn number_value(row: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| {
        row.get(*key)
            .and_then(|value| value.as_f64().or_else(|| value.as_str()?.parse().ok()))
            .filter(|value: &f64| value.is_finite())
    })
}
fn normalize_date_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.chars().take(10).collect())
        .filter(|value: &String| value.len() == 10)
}
fn normalize_hk_period(raw: &str, report_date: Option<&str>) -> String {
    let date = report_date
        .unwrap_or("")
        .chars()
        .take(10)
        .collect::<String>();
    let year = date.get(0..4).unwrap_or("----");
    let lower = raw.to_ascii_lowercase();
    if lower.contains("年报") || lower.contains("annual") {
        return format!("{year}FY");
    }
    if lower.contains("中报") || lower.contains("半年") || lower.contains("interim") {
        return format!("{year}H1");
    }
    if lower.contains("一季") || lower.contains("q1") {
        return format!("{year}Q1");
    }
    if lower.contains("三季") || lower.contains("q3") {
        return format!("{year}Q3");
    }
    if date.ends_with("03-31") {
        return format!("{year}Q1");
    }
    if date.ends_with("09-30") {
        return format!("{year}Q3");
    }
    format!("{year}RAW")
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct HkObservationMergeResult {
    pub quote_updated: bool,
    pub history_updated: bool,
    pub notes: Vec<String>,
}

pub(crate) fn merge_hk_observation_update(
    cache: &mut HkMarketCache,
    code: &str,
    quote: Result<HkQuote, String>,
    history: Result<Vec<HkDailyBar>, String>,
    fetched_at: &str,
) -> HkObservationMergeResult {
    let Some(code) = canonical_hk_code(code) else {
        return HkObservationMergeResult {
            notes: vec!["invalid HK stock code".to_string()],
            ..Default::default()
        };
    };
    let mut result = HkObservationMergeResult::default();
    match quote {
        Ok(mut value) => {
            value.code = code.clone();
            value.fetched_at = Some(fetched_at.to_string());
            value.last_error = None;
            cache.quotes.insert(code.clone(), value.clone());
            if let Some(snapshot) = cache.financials.get_mut(&code) {
                enrich_hk_financial_snapshot_from_quote(snapshot, &value);
            }
            cache.quote_errors.remove(&code);
            result.quote_updated = true;
        }
        Err(error) => {
            if let Some(value) = cache.quotes.get_mut(&code) {
                value.last_error = Some(error.clone());
            }
            cache.quote_errors.insert(code.clone(), error.clone());
            result.notes.push(format!("报价：{error}"));
        }
    }
    match history {
        Ok(bars) => {
            cache.histories.insert(
                code.clone(),
                HkHistoryRecord {
                    bars,
                    source: "Tencent Finance".to_string(),
                    fetched_at: fetched_at.to_string(),
                    last_error: None,
                },
            );
            cache.history_errors.remove(&code);
            result.history_updated = true;
        }
        Err(error) => {
            if let Some(value) = cache.histories.get_mut(&code) {
                value.last_error = Some(error.clone());
            }
            cache.history_errors.insert(code.clone(), error.clone());
            result.notes.push(format!("日 K：{error}"));
        }
    }
    result
}

fn normalize_hk_currency(value: &str) -> Option<String> {
    let currency = if value.trim().eq_ignore_ascii_case("RMB") {
        "CNY"
    } else {
        value.trim()
    };
    CurrencyCode::new(currency)
        .ok()
        .map(|currency| currency.as_str().to_owned())
}

fn tencent_hk_quote_supports_currency(currency: &CurrencyCode) -> bool {
    TENCENT_HK_SUPPORTED_QUOTE_CURRENCIES.contains(&currency.as_str())
}

fn cached_quote_for_security(security: &HkSecurity, quote: Option<HkQuote>) -> Option<HkQuote> {
    quote.filter(|quote| security.cached_quote_matches_security_master(quote))
}

fn quote_capability_for_security(
    security: &HkSecurity,
    quote_currency: Option<&str>,
    quote_error: Option<&str>,
    provider_readiness_error: Option<&str>,
) -> MarketCapability {
    let expected_currency = normalize_hk_currency(&security.currency);
    let received_currency = quote_currency.and_then(normalize_hk_currency);
    let (status, reason, quote_available) = match expected_currency.as_deref() {
        None => (
            CapabilityStatus::Unavailable,
            Some(format!(
                "security master has an invalid trading currency: {}",
                security.currency
            )),
            false,
        ),
        Some(expected) if !TENCENT_HK_SUPPORTED_QUOTE_CURRENCIES.contains(&expected) => (
            CapabilityStatus::Unavailable,
            Some(format!(
                "Tencent HK quote adapter does not support {expected}"
            )),
            false,
        ),
        Some(expected)
            if received_currency
                .as_deref()
                .is_some_and(|value| value != expected) =>
        {
            (
                CapabilityStatus::Unavailable,
                Some(format!(
                    "cached quote currency {} does not match security-master currency {expected}",
                    received_currency.as_deref().unwrap_or_default()
                )),
                false,
            )
        }
        Some(_) if received_currency.is_some() && provider_readiness_error.is_some() => (
            CapabilityStatus::Limited,
            provider_readiness_error.map(str::to_owned),
            true,
        ),
        Some(_) if received_currency.is_some() => (CapabilityStatus::Available, None, true),
        Some(_)
            if quote_error.is_some_and(|error| error.to_ascii_lowercase().contains("currency")) =>
        {
            (
                CapabilityStatus::Unavailable,
                quote_error.map(str::to_owned),
                false,
            )
        }
        Some(_) if provider_readiness_error.is_some() => (
            CapabilityStatus::Unavailable,
            provider_readiness_error.map(str::to_owned),
            false,
        ),
        Some(_) => (
            CapabilityStatus::Limited,
            Some(
                quote_error
                    .map(str::to_owned)
                    .unwrap_or_else(|| "no cached Tencent quote is available".to_owned()),
            ),
            false,
        ),
    };
    MarketCapability {
        market: MarketId::Hk,
        id: CapabilityId::Observe,
        status,
        reason,
        coverage: Some(CoverageSummary {
            requested_count: 1,
            available_count: if quote_available { 1 } else { 0 },
            missing_count: if quote_available { 0 } else { 1 },
            start_date: None,
            end_date: None,
            missing_fields: if quote_available {
                Vec::new()
            } else {
                vec!["quote".to_owned()]
            },
        }),
    }
}

pub(crate) fn parse_tencent_hk_quote(
    text: &str,
    code: &str,
    expected_currency: &CurrencyCode,
) -> Result<HkQuote, String> {
    let code = canonical_hk_code(code).ok_or_else(|| format!("invalid HK stock code: {code}"))?;
    let symbol = format!("hk{code}");
    let line = text
        .split(';')
        .find(|line| line.trim_start().starts_with(&format!("v_{symbol}=")))
        .ok_or_else(|| format!("Tencent returned no quote for {code}"))?;
    let fields = line
        .split_once('=')
        .and_then(|(_, value)| value.trim().strip_prefix('"')?.strip_suffix('"'))
        .unwrap_or_default()
        .split('~')
        .collect::<Vec<_>>();
    if fields.len() < 76 {
        return Err(format!("Tencent HK quote payload is incomplete for {code}"));
    }
    let name = fields[1].trim();
    let price = parse_f64(fields.get(3).copied())
        .filter(|value| *value > 0.0)
        .ok_or_else(|| format!("Tencent HK quote has no valid price for {code}"))?;
    let quote_time = fields
        .get(30)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if name.is_empty() {
        return Err(format!("Tencent HK quote has no valid name for {code}"));
    }
    let currency = fields
        .get(75)
        .and_then(|value| normalize_hk_currency(value))
        .ok_or_else(|| format!("Tencent HK quote has no valid currency for {code}"))?;
    if currency != expected_currency.as_str() {
        return Err(format!(
            "Tencent HK quote currency mismatch for {code}: expected {}, received {currency}",
            expected_currency.as_str()
        ));
    }

    Ok(HkQuote {
        code,
        name: name.to_string(),
        price,
        change_pct: parse_f64(fields.get(32).copied()).map(|value| value / 100.0),
        volume_shares: parse_f64(fields.get(6).copied()).and_then(number_to_u64),
        turnover_hkd: parse_f64(fields.get(37).copied()).filter(|value| *value >= 0.0),
        pe: parse_f64(fields.get(39).copied()),
        pb: parse_f64(fields.get(43).copied()),
        dividend_yield_percent: parse_f64(fields.get(47).copied()),
        market_cap_hkd: parse_f64(fields.get(45).copied()).map(|value| value * 100_000_000.0),
        total_shares: parse_f64(fields.get(69).copied()).and_then(number_to_u64),
        circulating_shares: parse_f64(fields.get(70).copied()).and_then(number_to_u64),
        currency,
        quote_time,
        source: "Tencent Finance".to_string(),
        fetched_at: None,
        last_error: None,
    })
}

pub(crate) fn parse_tencent_hk_history(text: &str, code: &str) -> Result<Vec<HkDailyBar>, String> {
    let code = canonical_hk_code(code).ok_or_else(|| format!("invalid HK stock code: {code}"))?;
    let symbol = format!("hk{code}");
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| format!("decode Tencent HK history failed: {error}"))?;
    let rows = value
        .get("data")
        .and_then(|data| data.get(&symbol))
        .and_then(|stock| stock.get("day").or_else(|| stock.get("qfqday")))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| format!("Tencent returned no daily history for {code}"))?;
    let mut bars = Vec::with_capacity(rows.len());
    for row in rows {
        let Some(fields) = row.as_array() else {
            continue;
        };
        if fields.len() < 6 {
            continue;
        }
        let Some(date) = fields[0].as_str().filter(|value| value.len() >= 10) else {
            continue;
        };
        let Some(open) = json_number(&fields[1]) else {
            continue;
        };
        let Some(close) = json_number(&fields[2]) else {
            continue;
        };
        let Some(high) = json_number(&fields[3]) else {
            continue;
        };
        let Some(low) = json_number(&fields[4]) else {
            continue;
        };
        if [open, high, low, close]
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.0)
            || high < open.max(close)
            || low > open.min(close)
            || high < low
        {
            continue;
        }
        bars.push(HkDailyBar {
            date: date.chars().take(10).collect(),
            open,
            high,
            low,
            close,
            volume_shares: json_number(&fields[5]).and_then(number_to_u64),
        });
    }
    if bars.is_empty() {
        return Err(format!("Tencent returned no valid daily bars for {code}"));
    }
    bars.sort_by(|left, right| left.date.cmp(&right.date));
    bars.dedup_by(|left, right| left.date == right.date);
    Ok(bars)
}

struct TencentHkMarketProvider {
    client: reqwest::Client,
    capabilities: Vec<crate::external_data::provider::ProviderCapability>,
    expected_currencies: HashMap<String, CurrencyCode>,
}

impl TencentHkMarketProvider {
    fn new(client: reqwest::Client, securities: &[HkSecurity]) -> Self {
        use crate::external_data::provider::ProviderCapability;
        use crate::external_data::{DataDomain, Market};
        Self {
            client,
            expected_currencies: securities
                .iter()
                .filter_map(|security| {
                    CurrencyCode::new(&security.currency)
                        .ok()
                        .map(|currency| (security.code.clone(), currency))
                })
                .collect(),
            capabilities: vec![
                ProviderCapability::new(
                    "tencent",
                    DataDomain::Quote,
                    Market::HongKong,
                    "public",
                    10,
                    ["quote"],
                ),
                ProviderCapability::new(
                    "tencent",
                    DataDomain::DailyHistory,
                    Market::HongKong,
                    "public",
                    10,
                    ["history"],
                ),
            ],
        }
    }
}

struct HkexSecurityDirectoryProvider {
    client: reqwest::Client,
    capabilities: Vec<crate::external_data::provider::ProviderCapability>,
}

impl HkexSecurityDirectoryProvider {
    fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            capabilities: vec![crate::external_data::provider::ProviderCapability::new(
                "hkex",
                crate::external_data::DataDomain::SecurityMaster,
                crate::external_data::Market::HongKong,
                "official",
                10,
                ["directory"],
            )],
        }
    }
}

async fn fetch_hk_provider_text(
    client: &reqwest::Client,
    url: &str,
    timeout: std::time::Duration,
    max_bytes: usize,
    request_id: &str,
) -> Result<String, String> {
    let context = crate::external_data::provider::ProviderContext {
        max_response_bytes: max_bytes,
        ..crate::external_data::provider::ProviderContext::new(request_id, timeout)
    };
    crate::external_data::execute_with_retry(
        |_| async {
            let response = client
                .get(url)
                .timeout(context.timeout)
                .send()
                .await
                .map_err(crate::external_data::provider::ProviderError::from_reqwest)?;
            let bytes = crate::external_data::provider::read_bounded_response(
                response,
                context.max_response_bytes,
            )
            .await?;
            String::from_utf8(bytes).map_err(|error| {
                crate::external_data::provider::ProviderError::invalid_payload(format!(
                    "provider response is not UTF-8: {error}"
                ))
            })
        },
        crate::external_data::RetryPolicy::default(),
        context.cancellation.clone(),
    )
    .await
    .map_err(|error| format!("{}: {}", error.category(), error.message()))
}

impl crate::external_data::provider::DataProvider for HkexSecurityDirectoryProvider {
    fn source_id(&self) -> &str {
        "hkex"
    }

    fn capabilities(&self) -> &[crate::external_data::provider::ProviderCapability] {
        &self.capabilities
    }

    fn fetch<'a>(
        &'a self,
        request: &'a crate::external_data::RefreshRequest,
        context: &'a crate::external_data::provider::ProviderContext,
    ) -> crate::external_data::provider::ProviderFuture<'a> {
        Box::pin(async move {
            use crate::external_data::provider::{ProviderBatch, ProviderError};
            use crate::external_data::{DataDomain, Market, StoredRecord};
            if request.domain != DataDomain::SecurityMaster || request.market != Market::HongKong {
                return Err(ProviderError::quality(
                    "HKEX directory provider only supports HK security master",
                ));
            }
            let response = self
                .client
                .get(HKEX_SECURITIES_SOURCE)
                .timeout(context.timeout)
                .send()
                .await
                .map_err(ProviderError::from_reqwest)?;
            let body = crate::external_data::provider::read_bounded_response(
                response,
                context.max_response_bytes.min(HKEX_MAX_WORKBOOK_BYTES),
            )
            .await?;
            let securities =
                parse_hkex_security_master(&body).map_err(ProviderError::invalid_payload)?;
            let fetched_at = current_epoch_millis();
            let record = StoredRecord::new(
                DataDomain::SecurityMaster,
                Market::HongKong,
                "hk",
                "directory",
                "hkex",
                "official",
                &fetched_at,
                None,
                None,
                "verified",
                vec![],
                serde_json::json!({
                    "items": securities,
                    "source": HKEX_SECURITIES_SOURCE,
                    "updated_at": fetched_at,
                    "last_attempt_at": fetched_at,
                    "last_error": Value::Null,
                }),
            );
            Ok(ProviderBatch::new("hkex", vec![record], body.len()))
        })
    }
}

impl crate::external_data::provider::DataProvider for TencentHkMarketProvider {
    fn source_id(&self) -> &str {
        "tencent"
    }

    fn capabilities(&self) -> &[crate::external_data::provider::ProviderCapability] {
        &self.capabilities
    }

    fn fetch<'a>(
        &'a self,
        request: &'a crate::external_data::RefreshRequest,
        context: &'a crate::external_data::provider::ProviderContext,
    ) -> crate::external_data::provider::ProviderFuture<'a> {
        Box::pin(async move {
            use crate::external_data::provider::{ProviderBatch, ProviderError};
            use crate::external_data::{DataDomain, Market, StoredRecord};
            let crate::external_data::RefreshScope::Entities { codes } = &request.scope else {
                return Err(ProviderError::quality(
                    "Tencent HK observation provider requires explicit entity codes",
                ));
            };
            let fetched_at = current_epoch_millis();
            let mut response_bytes = 0_usize;
            let mut records = Vec::with_capacity(codes.len());
            for code in codes {
                if context.cancellation.is_cancelled() {
                    return Err(ProviderError::cancelled());
                }
                let canonical = canonical_hk_code(code)
                    .ok_or_else(|| ProviderError::quality("invalid HK security code"))?;
                match request.domain {
                    DataDomain::Quote if request.market == Market::HongKong => {
                        let expected_currency =
                            self.expected_currencies.get(&canonical).ok_or_else(|| {
                                ProviderError::quality(format!(
                                    "HKEX security master has no valid currency for {canonical}"
                                ))
                            })?;
                        if !tencent_hk_quote_supports_currency(expected_currency) {
                            return Err(ProviderError::quality(format!(
                                "Tencent HK quote adapter does not support {} for {canonical}",
                                expected_currency.as_str()
                            )));
                        }
                        let url = format!("{TENCENT_HK_QUOTE_ENDPOINT}hk{canonical}");
                        let response = self
                            .client
                            .get(url)
                            .timeout(context.timeout)
                            .send()
                            .await
                            .map_err(ProviderError::from_reqwest)?;
                        let body = crate::external_data::provider::read_bounded_response(
                            response,
                            context.max_response_bytes.saturating_sub(response_bytes),
                        )
                        .await?;
                        response_bytes = response_bytes.saturating_add(body.len());
                        let text = String::from_utf8_lossy(&body);
                        let mut quote =
                            parse_tencent_hk_quote(&text, &canonical, expected_currency)
                                .map_err(ProviderError::invalid_payload)?;
                        quote.source = "Tencent Finance".into();
                        quote.fetched_at = Some(fetched_at.clone());
                        let quote_time = quote.quote_time.clone();
                        let payload = serde_json::to_value(quote).map_err(|error| {
                            ProviderError::invalid_payload(format!("encode HK quote: {error}"))
                        })?;
                        records.push(StoredRecord::new(
                            DataDomain::Quote,
                            Market::HongKong,
                            &canonical,
                            "quote",
                            "tencent",
                            "public",
                            &fetched_at,
                            quote_time.as_deref(),
                            None,
                            "unverified",
                            vec![],
                            payload,
                        ));
                    }
                    DataDomain::DailyHistory if request.market == Market::HongKong => {
                        let param = format!("hk{canonical},day,20200101,20501231,2000,");
                        let encoded = percent_encoding::utf8_percent_encode(
                            &param,
                            percent_encoding::NON_ALPHANUMERIC,
                        );
                        let url = format!("{TENCENT_HK_DAILY_KLINE_ENDPOINT}?param={encoded}");
                        let response = self
                            .client
                            .get(url)
                            .timeout(context.timeout)
                            .send()
                            .await
                            .map_err(ProviderError::from_reqwest)?;
                        let body = crate::external_data::provider::read_bounded_response(
                            response,
                            context.max_response_bytes.saturating_sub(response_bytes),
                        )
                        .await?;
                        response_bytes = response_bytes.saturating_add(body.len());
                        let text = String::from_utf8_lossy(&body);
                        let bars = parse_tencent_hk_history(&text, &canonical)
                            .map_err(ProviderError::invalid_payload)?;
                        let history = HkHistoryRecord {
                            bars,
                            source: "Tencent Finance".into(),
                            fetched_at: fetched_at.clone(),
                            last_error: None,
                        };
                        let observed_at = history.bars.last().map(|bar| bar.date.clone());
                        let payload = serde_json::to_value(history).map_err(|error| {
                            ProviderError::invalid_payload(format!("encode HK history: {error}"))
                        })?;
                        records.push(StoredRecord::new(
                            DataDomain::DailyHistory,
                            Market::HongKong,
                            &canonical,
                            "history",
                            "tencent",
                            "public",
                            &fetched_at,
                            observed_at.as_deref(),
                            None,
                            "unverified",
                            vec![],
                            payload,
                        ));
                    }
                    _ => {
                        return Err(ProviderError::quality(
                            "Tencent HK provider does not cover requested domain or market",
                        ));
                    }
                }
            }
            Ok(ProviderBatch::new("tencent", records, response_bytes))
        })
    }
}

async fn fetch_explicit_hk_batch<P: crate::external_data::provider::DataProvider>(
    app: &AppHandle,
    provider: &P,
    request: &crate::external_data::RefreshRequest,
    context: &crate::external_data::provider::ProviderContext,
    at: &str,
    foreground_recovery: bool,
) -> Result<Vec<crate::external_data::StoredRecord>, String> {
    ensure_hk_provider_ready(provider.source_id())?;
    let request_id = request
        .request_id
        .as_deref()
        .ok_or_else(|| "explicit HK refresh request id is required".to_string())?;
    let idempotency_key =
        hk_refresh_idempotency_key(provider.source_id(), request.domain, request_id);
    let mut store = crate::external_data::open_app_store(app)?;
    let (run, is_new) =
        store.create_refresh_run(request, &idempotency_key, at.parse().unwrap_or(0))?;
    let retryable_run_can_start = run.state == crate::external_data::RefreshRunState::Retryable
        && (!foreground_recovery || run.recovery_count == 0);
    if !is_new
        && !retryable_run_can_start
        && run.state != crate::external_data::RefreshRunState::NeedsUserRetry
        && run.state != crate::external_data::RefreshRunState::Queued
    {
        let records = match &request.scope {
            crate::external_data::RefreshScope::Entities { codes } => {
                let mut records = Vec::new();
                for code in codes {
                    records.extend(store.read_records(request.domain, request.market, code)?);
                }
                records
            }
            crate::external_data::RefreshScope::MarketWide => {
                store.read_domain_records(request.domain, request.market)?
            }
            crate::external_data::RefreshScope::DateRange { .. } => Vec::new(),
        };
        return Ok(records
            .into_iter()
            .filter(|record| record.record_key != "error")
            .collect());
    }
    store.start_refresh_run(&run.run_id, foreground_recovery, at.parse().unwrap_or(0))?;
    store.record_source_attempt(provider.source_id(), request.domain, request.market, at)?;
    drop(store);

    let fetched = crate::external_data::shared_refresh_coordinator()
        .fetch(provider, request, context)
        .await;
    let mut store = crate::external_data::open_app_store(app)?;
    match fetched {
        Ok(batch) => {
            {
                let _guard = HK_CACHE_WRITE_LOCK
                    .get_or_init(|| std::sync::Mutex::new(()))
                    .lock()
                    .map_err(|_| "HK cache write lock is unavailable".to_string())?;
                store.upsert_batch(&batch.records)?;
            }
            store.record_source_success(
                provider.source_id(),
                request.domain,
                request.market,
                at,
                batch.records.len(),
            )?;
            store.finish_refresh_run(
                &run.run_id,
                crate::external_data::RefreshRunState::Completed,
                at.parse().unwrap_or(0),
                None,
                None,
            )?;
            Ok(batch.records)
        }
        Err(error) => {
            store.record_source_failure(
                provider.source_id(),
                request.domain,
                request.market,
                at,
                error.category(),
                error.message(),
            )?;
            let state = if error.retryable() {
                crate::external_data::RefreshRunState::Retryable
            } else {
                crate::external_data::RefreshRunState::Failed
            };
            store.finish_refresh_run(
                &run.run_id,
                state,
                at.parse().unwrap_or(0),
                Some(error.category()),
                Some(error.message()),
            )?;
            Err(format!("{}: {}", error.category(), error.message()))
        }
    }
}

fn hk_refresh_idempotency_key(
    source_id: &str,
    domain: crate::external_data::DataDomain,
    request_id: &str,
) -> String {
    format!(
        "{}-{}-{}",
        source_id,
        domain.as_str(),
        Sha256::digest(request_id.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

pub(crate) async fn resume_explicit_hk_refreshes(app: AppHandle) -> Result<Value, String> {
    let readiness_error = ensure_hk_provider_ready("tencent").err();
    let mut store = crate::external_data::open_app_store(&app)?;
    let runs = store.foreground_recoveries()?;
    if let Some(reason) = readiness_error {
        let failed = runs
            .iter()
            .filter(|run| {
                run.request.market == crate::external_data::Market::HongKong
                    && matches!(
                        run.request.domain,
                        crate::external_data::DataDomain::Quote
                            | crate::external_data::DataDomain::DailyHistory
                    )
            })
            .map(|run| {
                let at = current_epoch_millis();
                store.start_refresh_run(&run.run_id, true, at.parse().unwrap_or(0))?;
                store.record_source_failure(
                    "tencent",
                    run.request.domain,
                    run.request.market,
                    &at,
                    "provider_readiness",
                    &reason,
                )?;
                store.finish_refresh_run(
                    &run.run_id,
                    crate::external_data::RefreshRunState::NeedsUserRetry,
                    at.parse().unwrap_or(0),
                    Some("provider_readiness"),
                    Some(&reason),
                )?;
                Ok(serde_json::json!({"run_id": run.run_id, "error": reason}))
            })
            .collect::<Result<Vec<_>, String>>()?;
        return Ok(serde_json::json!({
            "resumed": 0,
            "failed": failed,
            "blocked_reason": reason,
        }));
    }
    drop(store);
    let securities = read_hk_market_cache(&hk_market_data_path(&app)?)?.securities;
    let mut resumed = 0_usize;
    let mut failed = Vec::new();
    for run in runs {
        if run.request.market != crate::external_data::Market::HongKong
            || !matches!(
                run.request.domain,
                crate::external_data::DataDomain::Quote
                    | crate::external_data::DataDomain::DailyHistory
            )
        {
            continue;
        }
        let timeout = std::time::Duration::from_secs(HK_REQUEST_TIMEOUT_SECS);
        let client = match crate::market::build_tencent_http_client(
            "Mozilla/5.0 GuXuanYou HK recovery",
            timeout,
        ) {
            Ok(client) => client,
            Err(error) => {
                failed.push(serde_json::json!({"run_id": run.run_id, "error": error}));
                continue;
            }
        };
        let provider = TencentHkMarketProvider::new(client, &securities);
        let request_id = run
            .request
            .request_id
            .clone()
            .unwrap_or_else(|| run.run_id.clone());
        let context = crate::external_data::provider::ProviderContext::new(request_id, timeout);
        match fetch_explicit_hk_batch(
            &app,
            &provider,
            &run.request,
            &context,
            &current_epoch_millis(),
            true,
        )
        .await
        {
            Ok(_) => resumed += 1,
            Err(error) => failed.push(serde_json::json!({"run_id": run.run_id, "error": error})),
        }
    }
    Ok(serde_json::json!({"resumed": resumed, "failed": failed}))
}

#[tauri::command]
pub(crate) async fn api_explicit_hk_refresh(
    app: AppHandle,
    payload: Value,
) -> Result<Value, String> {
    use crate::external_data::{DataDomain, Market, RefreshRequest, RefreshScope};
    let domain = match payload.get("domain").and_then(Value::as_str) {
        Some("quote") => DataDomain::Quote,
        Some("daily_history") => DataDomain::DailyHistory,
        Some(other) => return Err(format!("HK refresh does not support domain {other}")),
        None => return Err("refresh domain is required".into()),
    };
    if payload.get("market").and_then(Value::as_str) != Some("hk") {
        return Err("HK provider refresh requires market=hk".into());
    }
    ensure_hk_provider_ready("tencent")?;
    let codes = payload
        .get("entity_codes")
        .and_then(Value::as_array)
        .ok_or_else(|| "entity_codes must be an array".to_string())?
        .iter()
        .map(|value| {
            value
                .as_str()
                .and_then(canonical_hk_code)
                .ok_or_else(|| "entity_codes contains an invalid HK code".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if codes.is_empty() || codes.len() > 50 {
        return Err("HK refresh must include between 1 and 50 security codes".into());
    }
    let fetched_at = current_epoch_millis();
    let raw_request_id = payload
        .get("request_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("hk-refresh-{fetched_at}"));
    let request = RefreshRequest {
        domain,
        market: Market::HongKong,
        scope: RefreshScope::Entities { codes },
        request_id: Some(raw_request_id.clone()),
    };
    let timeout = std::time::Duration::from_secs(HK_REQUEST_TIMEOUT_SECS);
    let client = crate::market::build_http_client_with_proxy(
        "Mozilla/5.0 GuXuanYou HK explicit refresh",
        timeout,
        Some(&payload),
    )?;
    let securities = read_hk_market_cache(&hk_market_data_path(&app)?)?.securities;
    let provider = TencentHkMarketProvider::new(client, &securities);
    let mut context =
        crate::external_data::provider::ProviderContext::new(&raw_request_id, timeout);
    context.network_policy = crate::external_data::provider::ProviderNetworkPolicy {
        proxy_mode: crate::market::proxy_mode_from_payload(Some(&payload)),
        proxy_url: crate::market::proxy_from_payload(Some(&payload)),
        allow_direct_fallback: false,
    };
    let records =
        fetch_explicit_hk_batch(&app, &provider, &request, &context, &fetched_at, false).await?;
    let idempotency_key = hk_refresh_idempotency_key("tencent", domain, &raw_request_id);
    let run = crate::external_data::open_app_store(&app)?
        .refresh_run_by_idempotency_key(&idempotency_key)?
        .ok_or_else(|| "HK refresh run status is unavailable".to_string())?;
    Ok(serde_json::json!({
        "market": "hk",
        "domain": domain.as_str(),
        "request_id": raw_request_id,
        "run_id": run.run_id,
        "state": run.state,
        "refreshed_records": records.len(),
        "record_keys": records.iter().map(|record| record.entity_key.clone()).collect::<Vec<_>>(),
    }))
}

trait RefreshScopeEntity {
    fn entity_key(&self) -> Result<String, String>;
}

impl RefreshScopeEntity for crate::external_data::RefreshScope {
    fn entity_key(&self) -> Result<String, String> {
        match self {
            Self::Entities { codes } if codes.len() == 1 => Ok(codes[0].clone()),
            _ => Err("expected one entity code in HK observation refresh".into()),
        }
    }
}

fn canonical_hk_code(value: &str) -> Option<String> {
    let value = value.trim().to_ascii_uppercase();
    let digits = value
        .strip_suffix(".HK")
        .or_else(|| value.strip_prefix("HK"))?;
    if digits.is_empty() || digits.len() > 5 || !digits.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    Some(format!("{digits:0>5}"))
}

fn parse_f64(value: Option<&str>) -> Option<f64> {
    value?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}
fn json_number(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|value: &f64| value.is_finite())
}
fn number_to_u64(value: f64) -> Option<u64> {
    (value >= 0.0 && value <= u64::MAX as f64).then_some(value.round() as u64)
}
pub(crate) fn parse_hkex_security_master(bytes: &[u8]) -> Result<Vec<HkSecurity>, String> {
    if bytes.is_empty() || bytes.len() > HKEX_MAX_WORKBOOK_BYTES {
        return Err("HKEX securities workbook is empty or exceeds the 16 MiB limit".to_string());
    }

    let mut workbook: Xlsx<Cursor<&[u8]>> = open_workbook_from_rs(Cursor::new(bytes))
        .map_err(|error| format!("open HKEX securities workbook failed: {error}"))?;
    let sheet_name = workbook
        .sheet_names()
        .first()
        .cloned()
        .ok_or_else(|| "HKEX securities workbook has no worksheets".to_string())?;
    let sheet = workbook
        .worksheet_range(&sheet_name)
        .map_err(|error| format!("read HKEX securities sheet failed: {error}"))?;
    let rows = sheet
        .rows()
        .map(|row| row.iter().map(cell_text).collect::<Vec<_>>())
        .collect::<Vec<_>>();

    parse_hkex_security_rows(&rows)
}

fn parse_hkex_security_rows(rows: &[Vec<String>]) -> Result<Vec<HkSecurity>, String> {
    let headers = rows
        .iter()
        .enumerate()
        .find_map(|(index, row)| {
            let columns = row
                .iter()
                .enumerate()
                .map(|(column, header)| (normalize_header(header), column))
                .collect::<std::collections::HashMap<_, _>>();
            let code = header_column(&columns, &["stockcode", "securitycode", "code"])?;
            let name = header_column(&columns, &["nameofsecurities", "securitiesname", "name"])?;
            let category = header_column(&columns, &["category"])?;
            let subcategory = header_column(&columns, &["subcategory"])?;
            let board_lot = header_column(&columns, &["boardlot", "boardlotsize"])?;
            let currency = header_column(&columns, &["tradingcurrency", "currency"])?;
            Some((
                index,
                SecurityColumns {
                    code,
                    name,
                    category,
                    subcategory,
                    board_lot,
                    currency,
                },
            ))
        })
        .ok_or_else(|| "HKEX securities workbook is missing required column headers".to_string())?;

    let (header_index, columns) = headers;
    let mut securities = Vec::new();
    let mut seen_codes = HashSet::new();
    for row in rows.iter().skip(header_index + 1) {
        let code = row
            .get(columns.code)
            .map(|value| normalize_hkex_code(value))
            .unwrap_or_default();
        let name = row
            .get(columns.name)
            .map(|value| value.trim().to_string())
            .unwrap_or_default();
        let category = row
            .get(columns.category)
            .map(|value| value.trim())
            .unwrap_or_default();
        let subcategory = row
            .get(columns.subcategory)
            .map(|value| value.trim())
            .unwrap_or_default();
        let currency = row
            .get(columns.currency)
            .and_then(|value| normalize_hk_currency(value))
            .unwrap_or_default();

        if code.is_empty()
            || name.is_empty()
            || !category.eq_ignore_ascii_case("Equity")
            || !subcategory
                .to_ascii_lowercase()
                .starts_with("equity securities (")
            || currency.is_empty()
            || !seen_codes.insert(code.clone())
        {
            continue;
        }

        let board_lot = row
            .get(columns.board_lot)
            .and_then(|value| parse_positive_u32(value));
        securities.push(HkSecurity {
            code,
            name,
            currency,
            board_lot,
            security_type: format!("{category} / {subcategory}"),
            classification_status: HkClassificationStatus::Unverified,
        });
    }

    if securities.is_empty() {
        return Err("HKEX securities workbook contains no supported ordinary shares".to_string());
    }
    securities.sort_by(|left, right| left.code.cmp(&right.code));
    Ok(securities)
}

struct SecurityColumns {
    code: usize,
    name: usize,
    category: usize,
    subcategory: usize,
    board_lot: usize,
    currency: usize,
}

fn header_column(
    columns: &std::collections::HashMap<String, usize>,
    aliases: &[&str],
) -> Option<usize> {
    aliases
        .iter()
        .find_map(|alias| columns.get(*alias).copied())
}

fn normalize_header(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect()
}

fn cell_text(value: &Data) -> String {
    match value {
        Data::Empty => String::new(),
        Data::Int(number) => number.to_string(),
        Data::Float(number) if number.fract() == 0.0 => format!("{number:.0}"),
        _ => value.to_string(),
    }
}

fn normalize_hkex_code(value: &str) -> String {
    let digits = value.trim().replace(',', "");
    if digits.is_empty() || digits.len() > 5 || !digits.chars().all(|ch| ch.is_ascii_digit()) {
        return String::new();
    }
    format!("{digits:0>5}")
}

fn parse_positive_u32(value: &str) -> Option<u32> {
    let digits = value.trim().replace(',', "");
    let parsed = digits.parse::<u32>().ok()?;
    (parsed > 0).then_some(parsed)
}

pub(crate) fn read_hk_market_cache(path: &Path) -> Result<HkMarketCache, String> {
    let database_path = external_hk_store_path(path)?;
    let mut store = crate::external_data::DataStore::open(&database_path)?;
    if path.exists() {
        let promoted = import_hk_legacy_domains(&mut store, path)?;
        let mut cache = read_hk_cache_from_store(&store)?;
        if promoted.len() != hk_cache_domains().len() {
            merge_unpromoted_hk_legacy_domains(
                &mut cache,
                read_hk_market_cache_legacy(path)?,
                &promoted,
            );
        }
        return Ok(cache);
    }
    read_hk_cache_from_store(&store)
}

fn read_hk_market_cache_legacy(path: &Path) -> Result<HkMarketCache, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(HkMarketCache::default());
        }
        Err(error) => return Err(format!("read HK market cache failed: {error}")),
    };
    let cache: HkMarketCache = serde_json::from_slice(&bytes)
        .map_err(|error| format!("decode HK market cache failed: {error}"))?;
    if cache.schema_version != 1 {
        return Err(format!(
            "unsupported HK market cache schema version: {}",
            cache.schema_version
        ));
    }
    Ok(cache)
}

#[cfg(test)]
pub(crate) fn write_hk_market_cache(path: &Path, cache: &HkMarketCache) -> Result<(), String> {
    let _guard = HK_CACHE_WRITE_LOCK
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .map_err(|_| "HK cache write lock is unavailable".to_string())?;
    write_hk_market_cache_inner(path, cache)
}

fn write_hk_market_cache_inner(path: &Path, cache: &HkMarketCache) -> Result<(), String> {
    if cache.schema_version != 1 {
        return Err(format!(
            "unsupported HK market cache schema version: {}",
            cache.schema_version
        ));
    }
    let database_path = external_hk_store_path(path)?;
    if database_path.exists() || !path.exists() {
        let mut store = crate::external_data::DataStore::open(&database_path)?;
        let promoted = if path.exists() {
            import_hk_legacy_domains(&mut store, path)?
        } else {
            hk_cache_domains().into_iter().collect()
        };
        if promoted.is_empty() {
            return write_hk_market_cache_legacy(path, cache);
        }
        replace_hk_cache_domains_in_store(&mut store, cache, &promoted)?;
        if promoted.len() != hk_cache_domains().len() {
            return write_hk_market_cache_legacy(path, cache);
        }
        return Ok(());
    }
    write_hk_market_cache_legacy(path, cache)
}

pub(crate) fn update_hk_market_cache<F, T>(
    path: &Path,
    update: F,
) -> Result<(HkMarketCache, T), String>
where
    F: FnOnce(&mut HkMarketCache) -> Result<T, String>,
{
    // Re-read after provider I/O, then serialize the cache read-modify-write with provider commits.
    let _guard = HK_CACHE_WRITE_LOCK
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .map_err(|_| "HK cache write lock is unavailable".to_string())?;
    let mut cache = read_hk_market_cache(path)?;
    let result = update(&mut cache)?;
    write_hk_market_cache_inner(path, &cache)?;
    Ok((cache, result))
}

fn write_hk_market_cache_legacy(path: &Path, cache: &HkMarketCache) -> Result<(), String> {
    let bytes = serde_json::to_vec(cache)
        .map_err(|error| format!("encode HK market cache failed: {error}"))?;
    crate::durability::atomic_write_json(path, &bytes)
}

fn external_hk_store_path(path: &Path) -> Result<PathBuf, String> {
    let parent = path
        .parent()
        .ok_or_else(|| "HK cache path has no parent".to_string())?;
    Ok(crate::external_data::store_path(parent))
}

fn hk_cache_domains() -> [(
    crate::external_data::DataDomain,
    crate::external_data::Market,
); 6] {
    use crate::external_data::{DataDomain, Market};
    [
        (DataDomain::SecurityMaster, Market::HongKong),
        (DataDomain::Quote, Market::HongKong),
        (DataDomain::DailyHistory, Market::HongKong),
        (DataDomain::FinancialSnapshot, Market::HongKong),
        (DataDomain::FxRate, Market::Global),
        (DataDomain::Disclosure, Market::HongKong),
    ]
}

fn import_hk_legacy_domains(
    store: &mut crate::external_data::DataStore,
    path: &Path,
) -> Result<
    HashSet<(
        crate::external_data::DataDomain,
        crate::external_data::Market,
    )>,
    String,
> {
    use crate::external_data::import_legacy_json;
    let mut promoted = HashSet::new();
    for (domain, market) in hk_cache_domains() {
        let report = import_legacy_json(store, path, domain, market, move |value, _, _| {
            let cache: HkMarketCache = serde_json::from_value(value)
                .map_err(|error| format!("decode HK cache for {domain:?}: {error}"))?;
            if cache.schema_version != 1 {
                return Err(format!(
                    "unsupported HK market cache schema version: {}",
                    cache.schema_version
                ));
            }
            let mut records = hk_cache_records(&cache);
            Ok(records.remove(&(domain, market)).unwrap_or_default())
        })?;
        if report.promoted {
            promoted.insert((domain, market));
        }
    }
    Ok(promoted)
}

fn merge_unpromoted_hk_legacy_domains(
    current: &mut HkMarketCache,
    legacy: HkMarketCache,
    promoted: &HashSet<(
        crate::external_data::DataDomain,
        crate::external_data::Market,
    )>,
) {
    use crate::external_data::{DataDomain, Market};
    if !promoted.contains(&(DataDomain::SecurityMaster, Market::HongKong)) {
        current.securities = legacy.securities;
        current.securities_source = legacy.securities_source;
        current.securities_updated_at = legacy.securities_updated_at;
        current.securities_last_attempt_at = legacy.securities_last_attempt_at;
        current.securities_last_error = legacy.securities_last_error;
    }
    if !promoted.contains(&(DataDomain::Quote, Market::HongKong)) {
        current.quotes = legacy.quotes;
        current.quote_errors = legacy.quote_errors;
    }
    if !promoted.contains(&(DataDomain::DailyHistory, Market::HongKong)) {
        current.histories = legacy.histories;
        current.history_errors = legacy.history_errors;
    }
    if !promoted.contains(&(DataDomain::FinancialSnapshot, Market::HongKong)) {
        current.financials = legacy.financials;
        current.financial_errors = legacy.financial_errors;
    }
    if !promoted.contains(&(DataDomain::FxRate, Market::Global)) {
        current.fx_rate = legacy.fx_rate;
        current.fx_error = legacy.fx_error;
    }
    if !promoted.contains(&(DataDomain::Disclosure, Market::HongKong)) {
        current.disclosures = legacy.disclosures;
        current.disclosure_errors = legacy.disclosure_errors;
    }
}

fn hk_cache_records(
    cache: &HkMarketCache,
) -> HashMap<
    (
        crate::external_data::DataDomain,
        crate::external_data::Market,
    ),
    Vec<crate::external_data::StoredRecord>,
> {
    use crate::external_data::{DataDomain, Market, StoredRecord};
    let mut grouped: HashMap<_, Vec<StoredRecord>> = HashMap::new();
    let default_at = cache
        .securities_updated_at
        .clone()
        .unwrap_or_else(current_epoch_millis);

    if !cache.securities.is_empty()
        || cache.securities_last_attempt_at.is_some()
        || cache.securities_updated_at.is_some()
        || !cache.securities_source.is_empty()
        || cache.securities_last_error.is_some()
    {
        grouped
            .entry((DataDomain::SecurityMaster, Market::HongKong))
            .or_default()
            .push(StoredRecord::new(
                DataDomain::SecurityMaster,
                Market::HongKong,
                "hk",
                "directory",
                nonempty_source(&cache.securities_source, "HKEX"),
                "official",
                cache
                    .securities_updated_at
                    .as_deref()
                    .unwrap_or(&default_at),
                None,
                None,
                if cache.securities.is_empty() {
                    "unavailable"
                } else {
                    "verified"
                },
                vec![],
                serde_json::json!({
                    "items": cache.securities,
                    "source": cache.securities_source,
                    "updated_at": cache.securities_updated_at,
                    "last_attempt_at": cache.securities_last_attempt_at,
                    "last_error": cache.securities_last_error,
                }),
            ));
    }
    for (code, quote) in &cache.quotes {
        let fetched_at = quote.fetched_at.as_deref().unwrap_or(&default_at);
        grouped
            .entry((DataDomain::Quote, Market::HongKong))
            .or_default()
            .push(StoredRecord::new(
                DataDomain::Quote,
                Market::HongKong,
                code,
                "quote",
                &quote.source,
                "public",
                fetched_at,
                quote.quote_time.as_deref(),
                None,
                "unverified",
                vec![],
                serde_json::to_value(quote).unwrap_or(Value::Null),
            ));
    }
    for (code, error) in &cache.quote_errors {
        grouped
            .entry((DataDomain::Quote, Market::HongKong))
            .or_default()
            .push(StoredRecord::new(
                DataDomain::Quote,
                Market::HongKong,
                code,
                "error",
                "tencent",
                "public",
                &default_at,
                None,
                None,
                "unavailable",
                vec![],
                serde_json::json!({"error": error}),
            ));
    }
    for (code, history) in &cache.histories {
        grouped
            .entry((DataDomain::DailyHistory, Market::HongKong))
            .or_default()
            .push(StoredRecord::new(
                DataDomain::DailyHistory,
                Market::HongKong,
                code,
                "history",
                &history.source,
                "public",
                &history.fetched_at,
                history.bars.last().map(|bar| bar.date.as_str()),
                None,
                "unverified",
                vec![],
                serde_json::to_value(history).unwrap_or(Value::Null),
            ));
    }
    for (code, error) in &cache.history_errors {
        grouped
            .entry((DataDomain::DailyHistory, Market::HongKong))
            .or_default()
            .push(StoredRecord::new(
                DataDomain::DailyHistory,
                Market::HongKong,
                code,
                "error",
                "tencent",
                "public",
                &default_at,
                None,
                None,
                "unavailable",
                vec![],
                serde_json::json!({"error": error}),
            ));
    }
    for (code, snapshot) in &cache.financials {
        grouped
            .entry((DataDomain::FinancialSnapshot, Market::HongKong))
            .or_default()
            .push(StoredRecord::new(
                DataDomain::FinancialSnapshot,
                Market::HongKong,
                code,
                "snapshot",
                &snapshot.candidate_source,
                "public",
                &snapshot.candidate_fetched_at,
                None,
                snapshot.disclosed_at.as_deref(),
                &snapshot.verification_status,
                vec![],
                serde_json::to_value(snapshot).unwrap_or(Value::Null),
            ));
    }
    for (code, error) in &cache.financial_errors {
        grouped
            .entry((DataDomain::FinancialSnapshot, Market::HongKong))
            .or_default()
            .push(StoredRecord::new(
                DataDomain::FinancialSnapshot,
                Market::HongKong,
                code,
                "error",
                "eastmoney",
                "public",
                &default_at,
                None,
                None,
                "unavailable",
                vec![],
                serde_json::json!({"error": error}),
            ));
    }
    if let Some(rate) = &cache.fx_rate {
        grouped
            .entry((DataDomain::FxRate, Market::Global))
            .or_default()
            .push(StoredRecord::new(
                DataDomain::FxRate,
                Market::Global,
                "HKD/CNY",
                "rate",
                &rate.source,
                "public",
                &rate.fetched_at,
                Some(&rate.observed_at),
                None,
                "unverified",
                vec![],
                serde_json::to_value(rate).unwrap_or(Value::Null),
            ));
    }
    if let Some(error) = &cache.fx_error {
        grouped
            .entry((DataDomain::FxRate, Market::Global))
            .or_default()
            .push(StoredRecord::new(
                DataDomain::FxRate,
                Market::Global,
                "HKD/CNY",
                "error",
                "frankfurter",
                "public",
                &default_at,
                None,
                None,
                "unavailable",
                vec![],
                serde_json::json!({"error": error}),
            ));
    }
    for (code, disclosure) in &cache.disclosures {
        grouped
            .entry((DataDomain::Disclosure, Market::HongKong))
            .or_default()
            .push(StoredRecord::new(
                DataDomain::Disclosure,
                Market::HongKong,
                code,
                "metadata",
                &disclosure.source_name,
                "official",
                &disclosure.checked_at,
                None,
                disclosure.disclosed_at.as_deref(),
                &disclosure.status,
                vec![],
                serde_json::to_value(disclosure).unwrap_or(Value::Null),
            ));
    }
    for (code, error) in &cache.disclosure_errors {
        grouped
            .entry((DataDomain::Disclosure, Market::HongKong))
            .or_default()
            .push(StoredRecord::new(
                DataDomain::Disclosure,
                Market::HongKong,
                code,
                "error",
                "hkexnews",
                "official",
                &default_at,
                None,
                None,
                "unavailable",
                vec![],
                serde_json::json!({"error": error}),
            ));
    }
    grouped
}

fn replace_hk_cache_domains_in_store(
    store: &mut crate::external_data::DataStore,
    cache: &HkMarketCache,
    domains_to_replace: &HashSet<(
        crate::external_data::DataDomain,
        crate::external_data::Market,
    )>,
) -> Result<(), String> {
    let mut grouped = hk_cache_records(cache);
    for (domain, market) in hk_cache_domains() {
        if !domains_to_replace.contains(&(domain, market)) {
            continue;
        }
        let records = grouped.remove(&(domain, market)).unwrap_or_default();
        store.replace_domain_batch(domain, market, &records)?;
    }
    Ok(())
}

fn read_hk_cache_from_store(
    store: &crate::external_data::DataStore,
) -> Result<HkMarketCache, String> {
    use crate::external_data::{DataDomain, Market};
    let mut cache = HkMarketCache::default();
    for record in store.read_records(DataDomain::SecurityMaster, Market::HongKong, "hk")? {
        if record.record_key == "directory" {
            cache.securities = serde_json::from_value(record.payload["items"].clone())
                .map_err(|error| format!("decode HK security directory record: {error}"))?;
            cache.securities_source = record.payload["source"]
                .as_str()
                .unwrap_or(&record.metadata.source_id)
                .to_string();
            cache.securities_updated_at = record.payload["updated_at"].as_str().map(str::to_string);
            cache.securities_last_attempt_at = record.payload["last_attempt_at"]
                .as_str()
                .map(str::to_string);
            cache.securities_last_error = record.payload["last_error"].as_str().map(str::to_string);
        }
    }
    for record in store.read_domain_records(DataDomain::Quote, Market::HongKong)? {
        match record.record_key.as_str() {
            "quote" => {
                let quote: HkQuote = serde_json::from_value(record.payload)
                    .map_err(|error| format!("decode HK quote record: {error}"))?;
                cache.quotes.insert(record.entity_key, quote);
            }
            "error" => {
                if let Some(error) = record.payload["error"].as_str() {
                    cache
                        .quote_errors
                        .insert(record.entity_key, error.to_string());
                }
            }
            _ => {}
        }
    }
    for record in store.read_domain_records(DataDomain::DailyHistory, Market::HongKong)? {
        match record.record_key.as_str() {
            "history" => {
                let history: HkHistoryRecord = serde_json::from_value(record.payload)
                    .map_err(|error| format!("decode HK history record: {error}"))?;
                cache.histories.insert(record.entity_key, history);
            }
            "error" => {
                if let Some(error) = record.payload["error"].as_str() {
                    cache
                        .history_errors
                        .insert(record.entity_key, error.to_string());
                }
            }
            _ => {}
        }
    }
    for record in store.read_domain_records(DataDomain::FinancialSnapshot, Market::HongKong)? {
        match record.record_key.as_str() {
            "snapshot" => {
                let snapshot: HkFinancialSnapshot = serde_json::from_value(record.payload)
                    .map_err(|error| format!("decode HK financial snapshot: {error}"))?;
                cache.financials.insert(record.entity_key, snapshot);
            }
            "error" => {
                if let Some(error) = record.payload["error"].as_str() {
                    cache
                        .financial_errors
                        .insert(record.entity_key, error.to_string());
                }
            }
            _ => {}
        }
    }
    for record in store.read_domain_records(DataDomain::FxRate, Market::Global)? {
        if record.record_key == "rate" {
            cache.fx_rate = Some(
                serde_json::from_value(record.payload)
                    .map_err(|error| format!("decode HK FX rate: {error}"))?,
            );
        } else if record.record_key == "error" {
            cache.fx_error = record.payload["error"].as_str().map(str::to_string);
        }
    }
    for record in store.read_domain_records(DataDomain::Disclosure, Market::HongKong)? {
        match record.record_key.as_str() {
            "metadata" => {
                let disclosure: HkDisclosureMetadata = serde_json::from_value(record.payload)
                    .map_err(|error| format!("decode HK disclosure metadata: {error}"))?;
                cache.disclosures.insert(record.entity_key, disclosure);
            }
            "error" => {
                if let Some(error) = record.payload["error"].as_str() {
                    cache
                        .disclosure_errors
                        .insert(record.entity_key, error.to_string());
                }
            }
            _ => {}
        }
    }
    Ok(cache)
}

fn nonempty_source<'a>(source: &'a str, fallback: &'a str) -> &'a str {
    if source.trim().is_empty() {
        fallback
    } else {
        source
    }
}

pub(crate) fn update_hkex_security_cache(
    path: &Path,
    workbook_bytes: &[u8],
    updated_at: &str,
) -> Result<HkMarketCache, String> {
    let securities = parse_hkex_security_master(workbook_bytes)?;
    update_hk_market_cache(path, move |cache| {
        cache.securities = securities;
        cache.securities_source = HKEX_SECURITIES_SOURCE.to_string();
        cache.securities_updated_at = Some(updated_at.to_string());
        cache.securities_last_attempt_at = Some(updated_at.to_string());
        cache.securities_last_error = None;
        Ok(())
    })
    .map(|(cache, ())| cache)
}

pub(crate) fn current_epoch_millis() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

pub(crate) fn hk_market_data_path(app: &AppHandle) -> Result<PathBuf, String> {
    let mut path = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("resolve HK market data directory failed: {error}"))?;
    path.push(HK_MARKET_DATA_FILE);
    Ok(path)
}

pub(crate) fn hk_market_cache_path(root: &Path) -> PathBuf {
    root.join(HK_MARKET_DATA_FILE)
}

#[tauri::command]
pub(crate) fn api_hk_market_status(app: AppHandle) -> Result<serde_json::Value, String> {
    let path = hk_market_data_path(&app)?;
    let cache = read_hk_market_cache(&path)?;
    let external_data = crate::external_data::status_for_app(&app)?;
    Ok(serde_json::json!({
        "market": "hk",
        "cache_path": path.to_string_lossy(),
        "security_count": cache.securities.len(),
        "securities_source": cache.securities_source,
        "securities_updated_at": cache.securities_updated_at,
        "securities_last_attempt_at": cache.securities_last_attempt_at,
        "securities_last_error": cache.securities_last_error,
        "quote_count": cache.quotes.len(),
        "history_count": cache.histories.len(),
        "quote_updated_at": cache.quotes.values().filter_map(|quote| quote.fetched_at.as_ref()).max(),
        "history_updated_at": cache.histories.values().map(|history| history.fetched_at.as_str()).max(),
        "quote_errors": cache.quote_errors,
        "history_errors": cache.history_errors,
        "external_data": external_data,
        "stale": cache.securities.is_empty() || cache.securities_last_error.is_some() || !cache.quote_errors.is_empty() || !cache.history_errors.is_empty(),
    }))
}

#[tauri::command]
pub(crate) async fn api_hk_market_refresh(app: AppHandle) -> Result<serde_json::Value, String> {
    ensure_hk_provider_ready("hkex")?;
    let path = hk_market_data_path(&app)?;
    let timeout = std::time::Duration::from_secs(HK_REQUEST_TIMEOUT_SECS);
    let attempt_at = current_epoch_millis();
    let request_id = format!("hk-directory-{attempt_at}");
    let request = crate::external_data::RefreshRequest {
        domain: crate::external_data::DataDomain::SecurityMaster,
        market: crate::external_data::Market::HongKong,
        scope: crate::external_data::RefreshScope::MarketWide,
        request_id: Some(request_id.clone()),
    };
    let client =
        crate::market::build_tencent_http_client("Mozilla/5.0 GuXuanYou HK market", timeout)?;
    let provider = HkexSecurityDirectoryProvider::new(client);
    let context = crate::external_data::provider::ProviderContext {
        max_response_bytes: HKEX_MAX_WORKBOOK_BYTES,
        ..crate::external_data::provider::ProviderContext::new(&request_id, timeout)
    };
    let records = match fetch_explicit_hk_batch(
        &app,
        &provider,
        &request,
        &context,
        &attempt_at,
        false,
    )
    .await
    {
        Ok(records) => records,
        Err(error) => {
            record_hkex_refresh_error(&path, &attempt_at, &error)?;
            return Err(error);
        }
    };
    let cache = read_hk_market_cache(&path)?;
    let run = crate::external_data::open_app_store(&app)?
        .refresh_run_by_idempotency_key(&hk_refresh_idempotency_key(
            "hkex",
            request.domain,
            &request_id,
        ))?
        .ok_or_else(|| "HKEX refresh run status is unavailable".to_string())?;
    Ok(serde_json::json!({
        "market": "hk",
        "security_count": cache.securities.len(),
        "securities_source": cache.securities_source,
        "securities_updated_at": cache.securities_updated_at,
        "run_id": run.run_id,
        "state": run.state,
        "refreshed_records": records.len(),
        "stale": false,
    }))
}

pub(crate) fn record_hkex_refresh_error(
    path: &Path,
    attempt_at: &str,
    error: &str,
) -> Result<(), String> {
    update_hk_market_cache(path, |cache| {
        cache.securities_last_attempt_at = Some(attempt_at.to_string());
        cache.securities_last_error = Some(error.to_string());
        Ok(())
    })
    .map(|_| ())
}

#[tauri::command]
pub(crate) fn api_hk_stock_search(
    app: AppHandle,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let cache = read_hk_market_cache(&hk_market_data_path(&app)?)?;
    let query = payload
        .get("query")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let limit = payload
        .get("limit")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(20)
        .clamp(1, 100) as usize;
    let items = cache
        .securities
        .into_iter()
        .filter(|security| {
            query.is_empty()
                || security.code.contains(&query)
                || security.name.to_ascii_lowercase().contains(&query)
        })
        .take(limit)
        .collect::<Vec<_>>();
    let refresh_readiness = ensure_hk_provider_ready("hkex");
    Ok(
        serde_json::json!({ "market": "hk", "items": items, "source": cache.securities_source, "updated_at": cache.securities_updated_at, "last_attempt_at": cache.securities_last_attempt_at, "last_error": cache.securities_last_error, "refresh_enabled": refresh_readiness.is_ok(), "refresh_reason": refresh_readiness.err() }),
    )
}

#[tauri::command]
pub(crate) async fn api_hk_observe(
    app: AppHandle,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let path = hk_market_data_path(&app)?;
    let code = payload
        .get("code")
        .and_then(serde_json::Value::as_str)
        .and_then(canonical_hk_code)
        .ok_or_else(|| "请输入有效港股代码。".to_string())?;
    let policy = payload
        .get("data_policy")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("cache_only");
    let mut cache = read_hk_market_cache(&path)?;
    let security = cache
        .securities
        .iter()
        .find(|item| item.code == code)
        .cloned()
        .ok_or_else(|| format!("港股证券列表中没有找到 {code}。"))?;
    observation_security_id(&security)?;
    let provider_readiness_error = ensure_hk_provider_ready("tencent").err();
    let mut notes = Vec::new();
    if hk_observation_should_refresh(policy) {
        if let Some(error) = provider_readiness_error.as_ref() {
            return Err(error.clone());
        }
        let timeout = std::time::Duration::from_secs(HK_REQUEST_TIMEOUT_SECS);
        let fetched_at = current_epoch_millis();
        let client = crate::market::build_http_client_with_proxy(
            "Mozilla/5.0 GuXuanYou HK observe",
            timeout,
            Some(&payload),
        )?;
        let provider = TencentHkMarketProvider::new(client, &cache.securities);
        let request_id = payload
            .get("request_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("hk-observe-{code}-{fetched_at}"));
        let quote_request = crate::external_data::RefreshRequest {
            domain: crate::external_data::DataDomain::Quote,
            market: crate::external_data::Market::HongKong,
            scope: crate::external_data::RefreshScope::Entities {
                codes: vec![code.clone()],
            },
            request_id: Some(format!("{request_id}-quote")),
        };
        let history_request = crate::external_data::RefreshRequest {
            domain: crate::external_data::DataDomain::DailyHistory,
            market: crate::external_data::Market::HongKong,
            scope: crate::external_data::RefreshScope::Entities {
                codes: vec![code.clone()],
            },
            request_id: Some(format!("{request_id}-history")),
        };
        let mut quote_context = crate::external_data::provider::ProviderContext::new(
            format!("{request_id}-quote"),
            timeout,
        );
        let mut history_context = crate::external_data::provider::ProviderContext::new(
            format!("{request_id}-history"),
            timeout,
        );
        let proxy_mode = crate::market::proxy_mode_from_payload(Some(&payload));
        let proxy_url = crate::market::proxy_from_payload(Some(&payload));
        let network_policy = crate::external_data::provider::ProviderNetworkPolicy {
            proxy_mode,
            proxy_url,
            allow_direct_fallback: false,
        };
        quote_context.network_policy = crate::external_data::provider::ProviderNetworkPolicy {
            proxy_mode: network_policy.proxy_mode.clone(),
            proxy_url: network_policy.proxy_url.clone(),
            allow_direct_fallback: network_policy.allow_direct_fallback,
        };
        history_context.network_policy = network_policy;

        let (quote_batch, history_batch) = tokio::join!(
            fetch_explicit_hk_batch(
                &app,
                &provider,
                &quote_request,
                &quote_context,
                &fetched_at,
                false
            ),
            fetch_explicit_hk_batch(
                &app,
                &provider,
                &history_request,
                &history_context,
                &fetched_at,
                false
            ),
        );
        let quote = match quote_batch {
            Ok(records) => records
                .into_iter()
                .next()
                .map(|record| {
                    serde_json::from_value(record.payload)
                        .map_err(|error| format!("decode refreshed HK quote: {error}"))
                })
                .transpose()?
                .ok_or_else(|| "Tencent returned no HK quote record".to_string()),
            Err(error) => Err(error),
        };
        let history = match history_batch {
            Ok(records) => records
                .into_iter()
                .next()
                .map(|record| {
                    serde_json::from_value::<HkHistoryRecord>(record.payload)
                        .map(|history| history.bars)
                        .map_err(|error| format!("decode refreshed HK history: {error}"))
                })
                .transpose()?
                .ok_or_else(|| "Tencent returned no HK history record".to_string()),
            Err(error) => Err(error),
        };
        let (updated_cache, result) = update_hk_market_cache(&path, |latest| {
            Ok(merge_hk_observation_update(
                latest,
                &code,
                quote,
                history,
                &fetched_at,
            ))
        })?;
        cache = updated_cache;
        notes.extend(result.notes);
    }
    let cached_quote = cache.quotes.get(&code).cloned();
    let quote = cached_quote_for_security(&security, cached_quote.clone());
    let history = cache.histories.get(&code).cloned();
    let quote_currency_error = cached_quote
        .as_ref()
        .filter(|_| quote.is_none())
        .map(|value| {
            format!(
                "cached quote currency {} does not match security-master currency {}",
                value.currency, security.currency
            )
        });
    let quote_error = quote_currency_error.or_else(|| {
        cache
            .quotes
            .get(&code)
            .and_then(|value| value.last_error.clone())
            .or_else(|| cache.quote_errors.get(&code).cloned())
    });
    let history_error = history
        .as_ref()
        .and_then(|record| record.last_error.clone())
        .or_else(|| cache.history_errors.get(&code).cloned());
    let quote_capability = quote_capability_for_security(
        &security,
        cached_quote.as_ref().map(|value| value.currency.as_str()),
        quote_error.as_deref(),
        provider_readiness_error.as_deref(),
    );
    if let Some(error) = quote_error.as_ref() {
        if !notes.iter().any(|note| note.contains(error)) {
            notes.push(format!("报价：{error}"));
        }
    }
    if let Some(error) = history_error.as_ref() {
        if !notes.iter().any(|note| note.contains(error)) {
            notes.push(format!("日 K：{error}"));
        }
    }
    let stale = quote.is_none() || history.is_none() || !notes.is_empty();
    let financial = build_hk_financial_result(&cache, &code);
    Ok(serde_json::json!({
        "market": "hk",
        "security": security,
        "quote": quote,
        "history": history.as_ref().map(|record| record.bars.clone()).unwrap_or_default(),
        "quote_capability": quote_capability,
        "provider_readiness_error": provider_readiness_error,
        "quote_updated_at": quote.as_ref().and_then(|value| value.fetched_at.clone()),
        "history_source": history.as_ref().map(|value| value.source.clone()),
        "history_updated_at": history.as_ref().map(|record| record.fetched_at.clone()),
        "history_error": history_error,
        "quote_error": quote_error,
        "notes": notes,
        "stale": stale,
        "financial": financial.snapshot,
        "fx_rate": financial.fx_rate,
        "official_disclosure": financial.official_disclosure,
        "candidate_updated_at": financial.candidate_updated_at,
        "fx_updated_at": financial.fx_updated_at,
        "official_checked_at": financial.official_checked_at,
        "financial_notes": financial.notes,
        "financial_field_errors": financial.field_errors,
        "financial_stale": financial.stale,
    }))
}

#[tauri::command]
pub(crate) fn api_hk_financial_status(
    app: AppHandle,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let code = payload
        .get("code")
        .and_then(serde_json::Value::as_str)
        .and_then(canonical_hk_code)
        .ok_or_else(|| "请输入有效港股代码。".to_string())?;
    let cache = read_hk_market_cache(&hk_market_data_path(&app)?)?;
    serde_json::to_value(build_hk_financial_result(&cache, &code))
        .map_err(|error| format!("encode HK financial status failed: {error}"))
}

#[tauri::command]
pub(crate) async fn api_hk_financial_refresh(
    app: AppHandle,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let code = payload
        .get("code")
        .and_then(serde_json::Value::as_str)
        .and_then(canonical_hk_code)
        .ok_or_else(|| "请输入有效港股代码。".to_string())?;
    let path = hk_market_data_path(&app)?;
    let fetched_at = current_epoch_millis();
    let timeout = std::time::Duration::from_secs(HK_REQUEST_TIMEOUT_SECS);
    {
        let store = crate::external_data::open_app_store(&app)?;
        store.record_source_attempt(
            "eastmoney_hk",
            crate::external_data::DataDomain::FinancialSnapshot,
            crate::external_data::Market::HongKong,
            &fetched_at,
        )?;
        store.record_source_attempt(
            "frankfurter",
            crate::external_data::DataDomain::FxRate,
            crate::external_data::Market::Global,
            &fetched_at,
        )?;
        store.record_source_attempt(
            "hkexnews",
            crate::external_data::DataDomain::Disclosure,
            crate::external_data::Market::HongKong,
            &fetched_at,
        )?;
    }
    let (financial, fx, disclosure) = match crate::market::build_tencent_http_client(
        "Mozilla/5.0 GuXuanYou HK financial",
        timeout,
    ) {
        Ok(client) => tokio::join!(
            fetch_hk_financial_snapshot(&client, &code, &fetched_at),
            fetch_hk_fx_rate(&client, &fetched_at),
            fetch_hk_disclosure_metadata(&client, &code, &fetched_at),
        ),
        Err(error) => (Err(error.clone()), Err(error.clone()), Err(error)),
    };
    {
        let store = crate::external_data::open_app_store(&app)?;
        record_hk_source_result(
            &store,
            "eastmoney_hk",
            crate::external_data::DataDomain::FinancialSnapshot,
            crate::external_data::Market::HongKong,
            &fetched_at,
            financial.is_ok(),
            financial.as_ref().err().map(String::as_str),
        )?;
        record_hk_source_result(
            &store,
            "frankfurter",
            crate::external_data::DataDomain::FxRate,
            crate::external_data::Market::Global,
            &fetched_at,
            fx.is_ok(),
            fx.as_ref().err().map(String::as_str),
        )?;
        record_hk_source_result(
            &store,
            "hkexnews",
            crate::external_data::DataDomain::Disclosure,
            crate::external_data::Market::HongKong,
            &fetched_at,
            disclosure.is_ok(),
            disclosure.as_ref().err().map(String::as_str),
        )?;
    }
    let (cache, _) = update_hk_market_cache(&path, |latest| {
        Ok(merge_hk_financial_refresh(
            latest, &code, financial, fx, disclosure,
        ))
    })?;
    serde_json::to_value(build_hk_financial_result(&cache, &code))
        .map_err(|error| format!("encode HK financial refresh result failed: {error}"))
}

fn record_hk_source_result(
    store: &crate::external_data::DataStore,
    source_id: &str,
    domain: crate::external_data::DataDomain,
    market: crate::external_data::Market,
    at: &str,
    success: bool,
    error: Option<&str>,
) -> Result<(), String> {
    if success {
        store.record_source_success(source_id, domain, market, at, 1)
    } else {
        let error = error.unwrap_or("provider request failed");
        let lower = error.to_ascii_lowercase();
        let category = if lower.contains("timeout") || lower.contains("timed out") {
            "timeout"
        } else if lower.contains("429") {
            "http_429"
        } else if lower.contains("http 5") {
            "http_5xx"
        } else if lower.contains("connection") || lower.contains("request failed") {
            "connection"
        } else {
            "provider_error"
        };
        store.record_source_failure(source_id, domain, market, at, category, error)
    }
}

#[cfg(test)]
mod currency_contract_tests {
    use super::{parse_hkex_security_rows, parse_tencent_hk_quote};
    use crate::market_domain::CurrencyCode;

    #[test]
    fn hkex_directory_keeps_ordinary_shares_in_each_listed_currency() {
        let rows = vec![
            vec![
                "Stock Code".into(),
                "Name of Securities".into(),
                "Category".into(),
                "Subcategory".into(),
                "Board Lot".into(),
                "Trading Currency".into(),
            ],
            vec![
                "00011".into(),
                "Bank".into(),
                "Equity".into(),
                "Equity Securities (Main Board)".into(),
                "100".into(),
                "HKD".into(),
            ],
            vec![
                "00012".into(),
                "Dual Counter".into(),
                "Equity".into(),
                "Equity Securities (Main Board)".into(),
                "100".into(),
                "RMB".into(),
            ],
            vec![
                "03188".into(),
                "ETF".into(),
                "Equity".into(),
                "Exchange Traded Funds".into(),
                "100".into(),
                "HKD".into(),
            ],
        ];

        let securities = parse_hkex_security_rows(&rows).unwrap();

        assert_eq!(
            securities
                .iter()
                .map(|item| (item.code.as_str(), item.currency.as_str()))
                .collect::<Vec<_>>(),
            [("00011", "HKD"), ("00012", "CNY")]
        );
        assert_eq!(
            serde_json::to_value(&securities[0]).unwrap()["classification_status"],
            "unverified"
        );
    }

    #[test]
    fn hk_quote_currency_must_match_security_master() {
        let cny = CurrencyCode::new("CNY").unwrap();
        let quote = parse_tencent_hk_quote(
            include_str!("../testdata/tencent-hk-quote.txt"),
            "00700.HK",
            &cny,
        );

        assert!(quote.unwrap_err().contains("currency"));
    }

    #[test]
    fn unverified_hk_classification_cannot_produce_an_observation_identity() {
        let security = super::HkSecurity {
            code: "00011".to_owned(),
            name: "Bank".to_owned(),
            currency: "HKD".to_owned(),
            board_lot: Some(100),
            security_type: "Equity".to_owned(),
            classification_status: super::HkClassificationStatus::Unverified,
        };

        assert!(super::observation_security_id(&security).is_err());
    }

    #[test]
    fn unresolved_provider_terms_fail_closed_except_for_the_attributed_ecb_fx_source() {
        for provider in ["hkex", "tencent", "eastmoney_hk", "hkexnews"] {
            assert!(super::ensure_hk_provider_ready(provider)
                .unwrap_err()
                .contains("disabled"));
        }
        assert!(super::ensure_hk_provider_ready("hkex").is_err());
        assert!(super::ensure_hk_provider_ready("frankfurter_ecb").is_ok());
    }

    #[test]
    fn hk_quote_keeps_the_currency_matching_the_security_master() {
        let hkd = CurrencyCode::new("HKD").unwrap();
        let quote = parse_tencent_hk_quote(
            include_str!("../testdata/tencent-hk-quote.txt"),
            "00700.HK",
            &hkd,
        )
        .unwrap();

        assert_eq!(quote.code, "00700");
        assert_eq!(quote.currency, "HKD");
    }

    #[test]
    fn cached_quote_is_not_exposed_when_its_currency_conflicts_with_the_security_master() {
        let security = super::HkSecurity {
            code: "00012".to_owned(),
            name: "Dual Counter".to_owned(),
            currency: "CNY".to_owned(),
            board_lot: Some(100),
            security_type: "Equity".to_owned(),
            classification_status: super::HkClassificationStatus::Unverified,
        };
        let quote = super::HkQuote {
            code: "00012".to_owned(),
            name: "Dual Counter".to_owned(),
            price: 10.0,
            change_pct: None,
            volume_shares: None,
            turnover_hkd: None,
            pe: None,
            pb: None,
            dividend_yield_percent: None,
            market_cap_hkd: None,
            total_shares: None,
            circulating_shares: None,
            currency: "HKD".to_owned(),
            quote_time: None,
            source: "Tencent Finance".to_owned(),
            fetched_at: None,
            last_error: None,
        };

        assert!(!security.cached_quote_matches_security_master(&quote));
        assert!(super::cached_quote_for_security(&security, Some(quote)).is_none());
    }

    #[test]
    fn quote_capability_is_unavailable_for_an_unsupported_listing_currency() {
        let security = super::HkSecurity {
            code: "00012".to_owned(),
            name: "Dual Counter".to_owned(),
            currency: "CNY".to_owned(),
            board_lot: Some(100),
            security_type: "Equity".to_owned(),
            classification_status: super::HkClassificationStatus::Unverified,
        };

        let capability = super::quote_capability_for_security(&security, None, None, None);

        assert_eq!(
            capability.status,
            crate::market_domain::CapabilityStatus::Unavailable
        );
        assert!(capability.reason.as_deref().unwrap().contains("CNY"));
    }

    #[test]
    fn quote_capability_is_available_when_the_provider_matches_the_listing_currency() {
        let security = super::HkSecurity {
            code: "00700".to_owned(),
            name: "Tencent".to_owned(),
            currency: "HKD".to_owned(),
            board_lot: Some(100),
            security_type: "Equity".to_owned(),
            classification_status: super::HkClassificationStatus::VerifiedOrdinaryShare,
        };

        let capability = super::quote_capability_for_security(&security, Some("HKD"), None, None);

        assert_eq!(
            capability.status,
            crate::market_domain::CapabilityStatus::Available
        );
        assert_eq!(capability.coverage.as_ref().unwrap().available_count, 1);
    }

    #[test]
    fn cached_quote_is_limited_while_provider_readiness_is_closed() {
        let security = super::HkSecurity {
            code: "00700".to_owned(),
            name: "Tencent".to_owned(),
            currency: "HKD".to_owned(),
            board_lot: Some(100),
            security_type: "Equity".to_owned(),
            classification_status: super::HkClassificationStatus::VerifiedOrdinaryShare,
        };

        let capability = super::quote_capability_for_security(
            &security,
            Some("HKD"),
            None,
            Some("Tencent provider terms are unresolved"),
        );

        assert_eq!(
            capability.status,
            crate::market_domain::CapabilityStatus::Limited
        );
        assert_eq!(
            capability.reason.as_deref(),
            Some("Tencent provider terms are unresolved")
        );
    }
}
