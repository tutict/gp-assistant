use crate::market_domain::{
    CapabilityId, CapabilityStatus, CoverageSummary, CurrencyCode, MarketCapability, MarketId,
    SecurityId, SecurityKind,
};
use calamine::{open_workbook_from_rs, Data, Reader, Xlsx};
use lopdf::content::{Content, Operation};
use lopdf::{Document, LoadOptions};
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
const SINA_HK_QUOTE_ENDPOINT: &str = "https://hq.sinajs.cn/list=rt_hk";
const HK_QUOTE_PRICE_SEMANTICS_VERSION: u8 = 1;
const TENCENT_HK_DAILY_KLINE_ENDPOINT: &str =
    "https://web.ifzq.gtimg.cn/appstock/app/hkfqkline/get?_var=kline_dayqfq&param=";
const HKEX_MAX_WORKBOOK_BYTES: usize = 16 * 1024 * 1024;
const HK_REQUEST_TIMEOUT_SECS: u64 = 8;
const HK_QUOTE_BATCH_SIZE: usize = 100;
const HK_QUOTE_BATCH_RESPONSE_LIMIT_BYTES: usize = 512 * 1024;
const HK_FINANCIAL_ENDPOINT: &str = "https://datacenter.eastmoney.com/securities/api/data/v1/get";
const HK_FX_ENDPOINT: &str = "https://api.frankfurter.dev/v2/providers/ecb/rate/HKD/CNY";
const HKEX_STOCK_SEARCH_ENDPOINT: &str = "https://www1.hkexnews.hk/search/partial.do";
const HKEX_TITLE_SEARCH_ENDPOINT: &str = "https://www1.hkexnews.hk/search/titleSearchServlet.do";
const TENCENT_HK_SUPPORTED_QUOTE_CURRENCIES: [&str; 1] = ["HKD"];
const HKEX_UPDATE_MAX_HTML_BYTES: usize = 4 * 1024 * 1024;
const HKEX_UPDATE_MAX_PDF_BYTES: usize = 12 * 1024 * 1024;
static HK_CACHE_WRITE_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HkClassificationStatus {
    #[default]
    Unverified,
    ExchangeListedEquity,
    VerifiedOrdinaryShare,
    UserConfirmedOrdinaryShare,
}

impl HkClassificationStatus {
    pub(crate) fn allows_observation(&self) -> bool {
        matches!(
            self,
            Self::ExchangeListedEquity
                | Self::VerifiedOrdinaryShare
                | Self::UserConfirmedOrdinaryShare
        )
    }
}

fn hkex_equity_subcategory(security_type: &str) -> bool {
    let Some((category, subcategory)) = security_type.split_once('/') else {
        return false;
    };
    category.trim().eq_ignore_ascii_case("equity")
        && subcategory
            .trim()
            .to_ascii_lowercase()
            .starts_with("equity securities (")
}

fn normalize_cached_hk_classifications(securities: &mut [HkSecurity]) {
    for security in securities {
        if security.classification_status == HkClassificationStatus::Unverified
            && hkex_equity_subcategory(&security.security_type)
        {
            security.classification_status = HkClassificationStatus::ExchangeListedEquity;
        }
    }
}

fn preserve_local_hk_classifications(previous: &[HkSecurity], refreshed: &mut [HkSecurity]) {
    let user_confirmed = previous
        .iter()
        .filter(|security| {
            security.classification_status == HkClassificationStatus::UserConfirmedOrdinaryShare
        })
        .map(|security| security.code.as_str())
        .collect::<HashSet<_>>();
    for security in refreshed {
        if user_confirmed.contains(security.code.as_str())
            && matches!(
                security.classification_status,
                HkClassificationStatus::Unverified | HkClassificationStatus::ExchangeListedEquity
            )
        {
            security.classification_status = HkClassificationStatus::UserConfirmedOrdinaryShare;
        }
    }
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
    pub(crate) fn supports_hkd_research(&self) -> bool {
        self.classification_status.allows_observation()
            && normalize_hk_currency(&self.currency).as_deref() == Some("HKD")
    }

    pub(crate) fn cached_quote_matches_security_master(&self, quote: &HkQuote) -> bool {
        quote.has_current_price_semantics()
            && normalize_hk_currency(&quote.currency)
                .zip(normalize_hk_currency(&self.currency))
                .is_some_and(|(quote_currency, listing_currency)| {
                    quote_currency == listing_currency
                })
    }
}

pub(crate) fn hk_chinese_display_name(quote: Option<&HkQuote>) -> Option<&str> {
    quote.and_then(|quote| {
        quote
            .name_zh
            .as_deref()
            .filter(|name| !name.is_empty() && name.chars().any(is_han_character))
            .or_else(|| {
                let name = quote.name.trim();
                (!name.is_empty() && name.chars().any(is_han_character)).then_some(name)
            })
    })
}

pub(crate) fn hk_financial_chinese_name(snapshot: Option<&HkFinancialSnapshot>) -> Option<&str> {
    let snapshot = snapshot?;
    snapshot
        .company_profile
        .as_ref()
        .filter(|profile| profile.code == snapshot.code)
        .and_then(|profile| profile.name_zh.as_deref())
        .filter(|name| name.chars().any(is_han_character))
}

fn hk_quote_needs_sina_name_enrichment(quote: &HkQuote) -> bool {
    hk_chinese_display_name(Some(quote)).is_none()
}

fn enrich_hk_quote_chinese_name(primary: &mut HkQuote, sina: &HkQuote) -> bool {
    if primary.name_zh.is_some() {
        return false;
    }
    let Some(name) = hk_chinese_display_name(Some(sina)) else {
        return false;
    };
    primary.name_zh = Some(name.to_owned());
    true
}

async fn enrich_tencent_hk_quote_name_from_sina<F, Fut>(
    primary: &mut HkQuote,
    fetch_sina: F,
) -> usize
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(HkQuote, usize), String>>,
{
    if primary.source != "Tencent Finance" || !hk_quote_needs_sina_name_enrichment(primary) {
        return 0;
    }
    let Ok((sina, response_bytes)) = fetch_sina().await else {
        return 0;
    };
    let _ = enrich_hk_quote_chinese_name(primary, &sina);
    response_bytes
}

fn preserve_hk_chinese_name(previous: Option<&HkQuote>, refreshed: &mut HkQuote) {
    if refreshed.name_zh.is_some() {
        return;
    }
    if let Some(name) = hk_chinese_display_name(previous) {
        refreshed.name_zh = Some(name.to_owned());
    }
}

fn hk_security_matches_query(security: &HkSecurity, quote: Option<&HkQuote>, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || security.code.contains(&query)
        || security.name.to_lowercase().contains(&query)
        || quote.is_some_and(|quote| {
            quote.name.to_lowercase().contains(&query)
                || quote
                    .name_zh
                    .as_deref()
                    .is_some_and(|name| name.to_lowercase().contains(&query))
        })
}

fn is_han_character(character: char) -> bool {
    matches!(
        character as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x3134F
    )
}

fn observation_security_id(security: &HkSecurity) -> Result<SecurityId, String> {
    if !security.classification_status.allows_observation() {
        return Err(format!(
            "HK security {} has unverified instrument classification; observation is disabled",
            security.code
        ));
    }
    let kind = if security.classification_status == HkClassificationStatus::ExchangeListedEquity {
        SecurityKind::ListedEquity
    } else {
        SecurityKind::OrdinaryShare
    };
    SecurityId::new(MarketId::Hk, &security.code, kind)
}

fn confirm_hk_ordinary_share(cache: &mut HkMarketCache, raw_code: &str) -> Result<(), String> {
    let code = canonical_hk_code(raw_code)
        .ok_or_else(|| format!("invalid HK security code: {raw_code}"))?;
    let security = cache
        .securities
        .iter_mut()
        .find(|security| security.code == code)
        .ok_or_else(|| format!("HK security {code} is not present in the local directory"))?;
    if security.classification_status == HkClassificationStatus::Unverified {
        security.classification_status = HkClassificationStatus::UserConfirmedOrdinaryShare;
    }
    Ok(())
}

pub(crate) fn ensure_hk_provider_ready(provider_id: &str) -> Result<(), String> {
    match provider_id {
        "frankfurter_ecb" | "hkex" | "tencent" | "eastmoney_hk" | "sina_hk_financial" | "hkexnews" => Ok(()),
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
    pub quarterly_updates: HashMap<String, HkQuarterlyUpdateRecord>,
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
            quarterly_updates: HashMap::new(),
            financial_errors: HashMap::new(),
            disclosure_errors: HashMap::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct HkQuote {
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub name_zh: Option<String>,
    pub price: f64,
    #[serde(default)]
    pub price_semantics_version: u8,
    #[serde(default)]
    pub previous_close: Option<f64>,
    #[serde(default)]
    pub open: Option<f64>,
    #[serde(default)]
    pub high: Option<f64>,
    #[serde(default)]
    pub low: Option<f64>,
    #[serde(default)]
    pub change: Option<f64>,
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

impl HkQuote {
    fn has_current_price_semantics(&self) -> bool {
        !self.source.eq_ignore_ascii_case("Sina Finance")
            || self.price_semantics_version >= HK_QUOTE_PRICE_SEMANTICS_VERSION
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(crate) struct HkHistoryRecord {
    pub bars: Vec<HkDailyBar>,
    pub source: String,
    pub fetched_at: String,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_range: Option<HkHistoryRequestRange>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub request_ranges: Vec<HkHistoryRequestRange>,
    #[serde(default)]
    pub price_adjustment: HkHistoryPriceAdjustment,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub bar_fetched_at: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suspended_as_of: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum HkHistoryPriceAdjustment {
    #[default]
    #[serde(rename = "qfq")]
    Forward,
    #[serde(rename = "hfq")]
    Backward,
}

impl HkHistoryPriceAdjustment {
    fn query_value(self) -> &'static str {
        match self {
            Self::Forward => "qfq",
            Self::Backward => "hfq",
        }
    }
    fn alternate(self) -> Self {
        match self {
            Self::Forward => Self::Backward,
            Self::Backward => Self::Forward,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct HkFetchedHistory {
    pub bars: Vec<HkDailyBar>,
    pub price_adjustment: HkHistoryPriceAdjustment,
    #[serde(default)]
    pub suspended_as_of: Option<String>,
}

#[derive(Debug)]
struct HkHistoryPage {
    bars: Vec<HkDailyBar>,
    suspended_as_of: Option<String>,
}

impl From<Vec<HkDailyBar>> for HkHistoryPage {
    fn from(bars: Vec<HkDailyBar>) -> Self {
        Self {
            bars,
            suspended_as_of: None,
        }
    }
}

#[derive(Debug)]
enum HkHistoryFetchError {
    InvalidPrices(String),
    Provider(String),
}

impl std::fmt::Display for HkHistoryFetchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPrices(message) | Self::Provider(message) => formatter.write_str(message),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct HkHistoryRequestRange {
    pub start_date: String,
    pub end_date: String,
}

pub(crate) fn merge_hk_history_record(
    previous: Option<&HkHistoryRecord>,
    mut incoming: HkHistoryRecord,
) -> HkHistoryRecord {
    let mut ranges = incoming.request_ranges.clone();
    ranges.extend(incoming.request_range.clone());
    let incoming_rows = incoming
        .bars
        .iter()
        .filter_map(|bar| {
            crate::market_domain::dates::compact_date_key(&bar.date).map(|key| (key, bar.clone()))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut bars = std::collections::BTreeMap::new();
    let mut bar_fetched_at = HashMap::new();
    let mut incoming_is_newer = true;
    let mut invalidate_archive = false;
    if let Some(previous) = previous {
        incoming_is_newer = hk_history_timestamp(&incoming.fetched_at)
            >= hk_history_timestamp(&previous.fetched_at);
        // Adjustment types cannot share one price series.
        if previous.price_adjustment != incoming.price_adjustment {
            return if incoming_is_newer {
                incoming
            } else {
                previous.clone()
            };
        }
        for bar in &previous.bars {
            if let Some(key) = crate::market_domain::dates::compact_date_key(&bar.date) {
                bar_fetched_at.insert(
                    key.clone(),
                    previous
                        .bar_fetched_at
                        .get(&key)
                        .cloned()
                        .unwrap_or_else(|| previous.fetched_at.clone()),
                );
                bars.insert(key, bar.clone());
            }
        }
        let local_now =
            chrono::Utc::now().with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap());
        let closed_date = if local_now.time() < chrono::NaiveTime::from_hms_opt(16, 15, 0).unwrap()
        {
            local_now.date_naive().pred_opt().unwrap()
        } else {
            local_now.date_naive()
        }
        .format("%Y%m%d")
        .to_string();
        let changed_dates = bars
            .iter()
            .filter_map(|(date, old)| {
                let new = incoming_rows.get(date)?;
                (date <= &closed_date && !hk_history_prices_equal(old, new)).then_some(date)
            })
            .collect::<Vec<_>>();
        if changed_dates.iter().any(|date| {
            hk_history_timestamp(
                incoming
                    .bar_fetched_at
                    .get(*date)
                    .unwrap_or(&incoming.fetched_at),
            ) < hk_history_timestamp(bar_fetched_at.get(*date).unwrap_or(&previous.fetched_at))
        }) {
            let mut kept = previous.clone();
            kept.revision_reason =
                Some("较旧的复权价格版本已丢弃，缺少的历史区间需重新补齐。".to_owned());
            return kept;
        }
        invalidate_archive =
            !changed_dates.is_empty() && bars.keys().any(|date| !incoming_rows.contains_key(date));
        if !invalidate_archive {
            ranges.extend(previous.request_ranges.clone());
            ranges.extend(previous.request_range.clone());
        }
    }
    for (key, bar) in &incoming_rows {
        let fetched_at = incoming
            .bar_fetched_at
            .get(key)
            .unwrap_or(&incoming.fetched_at);
        let previous_at = bar_fetched_at.get(key).map(String::as_str).unwrap_or("0");
        if !bars.contains_key(key)
            || hk_history_timestamp(fetched_at) >= hk_history_timestamp(previous_at)
        {
            bars.insert(key.clone(), bar.clone());
            bar_fetched_at.insert(key.clone(), fetched_at.clone());
        }
    }
    let revision_reason = invalidate_archive.then(|| {
        "数据源重算了复权历史，未重新获取的旧区间已暂停使用，回测时会按需重新补齐。".to_owned()
    });
    if invalidate_archive && ranges.is_empty() {
        if let Some((first, last)) = incoming_rows
            .keys()
            .next()
            .zip(incoming_rows.keys().next_back())
        {
            ranges.push(HkHistoryRequestRange {
                start_date: first.clone(),
                end_date: last.clone(),
            });
        }
    }
    if !incoming_is_newer {
        incoming = previous.unwrap().clone();
    }
    ranges.sort_by(|left, right| left.start_date.cmp(&right.start_date));
    let mut merged_ranges: Vec<HkHistoryRequestRange> = Vec::new();
    for range in ranges {
        if let Some(last) = merged_ranges
            .last_mut()
            .filter(|last| range.start_date <= last.end_date)
        {
            last.end_date = last.end_date.clone().max(range.end_date);
        } else {
            merged_ranges.push(range);
        }
    }
    incoming.request_range = (merged_ranges.len() == 1).then(|| merged_ranges[0].clone());
    incoming.request_ranges = merged_ranges;
    incoming.bar_fetched_at = bar_fetched_at;
    if revision_reason.is_some() {
        incoming.revision_reason = revision_reason;
    }
    incoming.bars = bars.into_values().collect();
    incoming
}

fn hk_history_timestamp(raw: &str) -> u128 {
    raw.parse().unwrap_or_else(|_| {
        chrono::DateTime::parse_from_rfc3339(raw)
            .map(|date| date.timestamp_millis().max(0) as u128)
            .unwrap_or(0)
    })
}

fn hk_history_prices_equal(left: &HkDailyBar, right: &HkDailyBar) -> bool {
    [left.open, left.high, left.low, left.close]
        .into_iter()
        .zip([right.open, right.high, right.low, right.close])
        .all(|(left, right)| (left - right).abs() <= left.abs().max(right.abs()).max(1.0) * 1e-8)
}

pub(crate) fn hk_history_bar_is_prepared(history: &HkHistoryRecord, bar: &HkDailyBar) -> bool {
    if history.request_ranges.is_empty() && history.request_range.is_none() {
        return true;
    }
    let Some(date) = crate::market_domain::dates::compact_date_key(&bar.date) else {
        return false;
    };
    history
        .request_ranges
        .iter()
        .chain(history.request_range.iter())
        .any(|range| range.start_date <= date && date <= range.end_date)
}

pub(crate) fn hk_history_bars_for_quote(
    history: &HkHistoryRecord,
    quote: Option<&HkQuote>,
) -> Vec<HkDailyBar> {
    let mut bars = history
        .bars
        .iter()
        .filter(|bar| hk_history_bar_is_prepared(history, bar))
        .cloned()
        .collect::<Vec<_>>();
    if history.price_adjustment != HkHistoryPriceAdjustment::Backward {
        return bars;
    }
    let Some(last) = bars.last() else { return bars };
    let Some(quote) = quote.filter(|quote| {
        quote.price.is_finite()
            && quote.price > 0.0
            && (crate::market_domain::dates::compact_date_key(
                quote.quote_time.as_deref().unwrap_or_default(),
            ) == crate::market_domain::dates::compact_date_key(&last.date)
                || hk_history_suspension_matches_quote(history, quote))
    }) else {
        return bars;
    };
    let scale = quote.price / last.close;
    if !scale.is_finite() || scale <= 0.0 {
        return bars;
    }
    for bar in &mut bars {
        bar.open *= scale;
        bar.high *= scale;
        bar.low *= scale;
        bar.close *= scale;
    }
    bars
}

pub(crate) fn hk_history_suspension_matches_quote(
    history: &HkHistoryRecord,
    quote: &HkQuote,
) -> bool {
    quote.volume_shares == Some(0)
        && history.suspended_as_of.as_ref().is_some_and(|date| {
            crate::market_domain::dates::compact_date_key(
                quote.quote_time.as_deref().unwrap_or_default(),
            )
            .as_ref()
                == Some(date)
                && history
                    .request_ranges
                    .iter()
                    .chain(history.request_range.iter())
                    .any(|range| range.start_date <= *date && *date <= range.end_date)
        })
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
pub(crate) struct HkCompanyProfile {
    pub code: String,
    pub name: String,
    pub name_zh: Option<String>,
    pub industry: String,
    pub business_summary: String,
    pub source: String,
    pub fetched_at: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub company_profile: Option<HkCompanyProfile>,
    #[serde(default)]
    pub supplemental_source: Option<String>,
    #[serde(default)]
    pub supplemental_fetched_at: Option<String>,
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
    pub additional_metrics: HashMap<String, HkFinancialMetric>,
    #[serde(default)]
    pub official_values: HashMap<String, HkFinancialMetric>,
    #[serde(default)]
    pub verification_conflicts: Vec<String>,
    #[serde(default)]
    pub field_errors: HashMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct HkQuarterlyUpdate {
    pub code: String,
    pub period: String,
    pub period_end: String,
    pub disclosed_at: String,
    pub title: String,
    pub source_name: String,
    pub source_url: String,
    pub currency: String,
    pub unit: String,
    pub audit_status: String,
    pub fetched_at: String,
    pub quarterly_revenue: Option<f64>,
    pub quarterly_revenue_yoy_percent: Option<f64>,
    pub quarterly_revenue_qoq_percent: Option<f64>,
    pub nine_month_revenue: Option<f64>,
    pub nine_month_revenue_yoy_percent: Option<f64>,
    pub quarterly_net_profit: Option<f64>,
    pub quarterly_net_profit_yoy_percent: Option<f64>,
    pub quarterly_net_profit_qoq_percent: Option<f64>,
    pub nine_month_net_profit: Option<f64>,
    pub nine_month_net_profit_yoy_percent: Option<f64>,
    pub quarterly_total_comprehensive_income: Option<f64>,
    pub quarterly_total_comprehensive_income_yoy_percent: Option<f64>,
    pub quarterly_total_comprehensive_income_qoq_percent: Option<f64>,
    pub nine_month_total_comprehensive_income: Option<f64>,
    pub nine_month_total_comprehensive_income_yoy_percent: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct HkQuarterlyUpdateRecord {
    pub update: Option<HkQuarterlyUpdate>,
    pub checked_at: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct HkFinancialResult {
    pub market: String,
    pub code: String,
    pub snapshot: Option<HkFinancialSnapshot>,
    pub fx_rate: Option<HkFxRate>,
    pub official_disclosure: Option<HkDisclosureMetadata>,
    pub quarterly_update: Option<HkQuarterlyUpdate>,
    pub quarterly_update_checked_at: Option<String>,
    pub quarterly_update_error: Option<String>,
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

pub(crate) trait HkSupplementalFinancialProvider: Send + Sync {
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
        company_profile: None,
        supplemental_source: None,
        supplemental_fetched_at: None,
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
        additional_metrics: [
            ("revenue_yoy", ("OPERATE_INCOME_YOY", "percent")),
            ("net_profit_yoy", ("HOLDER_PROFIT_YOY", "percent")),
            ("gross_margin", ("GROSS_PROFIT_RATIO", "percent")),
            ("net_margin", ("NET_PROFIT_RATIO", "percent")),
            ("asset_liability_ratio", ("DEBT_ASSET_RATIO", "percent")),
            ("bps", ("BPS", "currency/share")),
            ("diluted_eps", ("DILUTED_EPS", "currency/share")),
            (
                "operating_cash_flow_per_share",
                ("PER_NETCASH_OPERATE", "currency/share"),
            ),
            (
                "operating_cash_flow",
                ("OPERATE_CASH_FLOW", "currency_total"),
            ),
            ("current_ratio", ("CURRENT_RATIO", "ratio")),
        ]
        .into_iter()
        .filter_map(|(key, (field, unit))| {
            let value = number_value(row, &[field]);
            value.map(|value| {
                let metric_currency = if unit == "percent" || unit == "ratio" {
                    ""
                } else if unit == "currency/share" && field == "BPS" {
                    &currency
                } else {
                    &currency
                };
                (
                    key.to_string(),
                    HkFinancialMetric {
                        value: Some(value),
                        currency: metric_currency.to_string(),
                        unit: unit.to_string(),
                        source: source.to_string(),
                        reason: None,
                    },
                )
            })
        })
        .collect(),
        official_values: HashMap::new(),
        verification_conflicts: Vec::new(),
        field_errors: HashMap::new(),
    };
    sync_hk_financial_field_errors(&mut snapshot);
    Ok(snapshot)
}

struct SinaHkFinancialProvider;

fn parse_hk_company_profile(
    text: &str,
    code: &str,
    fetched_at: &str,
) -> Result<HkCompanyProfile, String> {
    let canonical =
        canonical_hk_code(code).ok_or_else(|| format!("invalid HK stock code: {code}"))?;
    let payload: Value = serde_json::from_str(text)
        .map_err(|error| format!("decode HK company profile: {error}"))?;
    let row = payload
        .get("result")
        .and_then(|result| result.get("data"))
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                let row = row.as_object()?;
                let received = string_value(row, &["SECUCODE", "SECURITY_CODE"])?;
                (canonical_hk_code(&received).as_deref() == Some(canonical.as_str())).then_some(row)
            })
        })
        .ok_or_else(|| format!("HK company profile contains no matching issuer for {canonical}"))?;
    let name = string_value(row, &["ORG_NAME", "SECURITY_NAME_ABBR"])
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| "HK company profile contains no company name".to_owned())?;
    let short_name = string_value(row, &["SECURITY_NAME_ABBR"]);
    let name_zh = short_name
        .filter(|name| name.chars().any(is_han_character))
        .or_else(|| name.chars().any(is_han_character).then(|| name.clone()));
    Ok(HkCompanyProfile {
        code: canonical,
        name,
        name_zh,
        industry: string_value(row, &["BELONG_INDUSTRY"]).unwrap_or_default(),
        business_summary: string_value(row, &["MAIN_BUSINESS"]).unwrap_or_default(),
        source: "Eastmoney HK company profile".to_owned(),
        fetched_at: fetched_at.to_owned(),
    })
}

async fn fetch_hk_company_profile(
    client: &reqwest::Client,
    code: &str,
    fetched_at: &str,
) -> Result<HkCompanyProfile, String> {
    let canonical =
        canonical_hk_code(code).ok_or_else(|| format!("invalid HK stock code: {code}"))?;
    let url = format!("{HK_FINANCIAL_ENDPOINT}?reportName=RPT_HKF10_INFO_ORGPROFILE&columns=SECUCODE,SECURITY_CODE,SECURITY_NAME_ABBR,ORG_NAME,BELONG_INDUSTRY,MAIN_BUSINESS&filter=(SECUCODE%3D%22{canonical}.HK%22)&pageNumber=1&pageSize=1&source=HSF10&client=PC");
    let text = fetch_hk_provider_text(
        client,
        &url,
        std::time::Duration::from_secs(HK_REQUEST_TIMEOUT_SECS),
        256 * 1024,
        "hk-company-profile",
    )
    .await?;
    parse_hk_company_profile(&text, &canonical, fetched_at)
}

impl HkSupplementalFinancialProvider for SinaHkFinancialProvider {
    fn endpoint(&self, code: &str) -> Result<String, String> {
        let canonical =
            canonical_hk_code(code).ok_or_else(|| format!("invalid HK security code: {code}"))?;
        Ok(format!(
            "https://stock.finance.sina.com.cn/hkstock/finance/{canonical}.html"
        ))
    }

    fn parse(
        &self,
        text: &str,
        code: &str,
        fetched_at: &str,
    ) -> Result<HkFinancialSnapshot, String> {
        parse_sina_hk_financial_snapshot(text, code, fetched_at)
    }
}

pub(crate) fn merge_hk_financial_candidates(
    primary: Result<HkFinancialSnapshot, String>,
    supplemental: Result<HkFinancialSnapshot, String>,
) -> Result<HkFinancialSnapshot, String> {
    let mut primary = match primary {
        Ok(primary) => primary,
        Err(primary_error) => {
            return match supplemental {
                Ok(mut supplemental) => {
                    supplemental.field_errors.insert("eastmoney_supplement".to_string(), primary_error);
                    Ok(supplemental)
                }
                Err(supplemental_error) => Err(format!(
                    "Eastmoney financial indicators failed ({primary_error}); Sina HK statements failed ({supplemental_error})"
                )),
            };
        }
    };
    let supplemental = match supplemental {
        Ok(supplemental) => supplemental,
        Err(error) => {
            primary
                .field_errors
                .insert("sina_supplement".to_string(), error);
            return Ok(primary);
        }
    };
    primary.supplemental_source = Some("Sina Finance HK statements".to_string());
    primary.supplemental_fetched_at = Some(supplemental.candidate_fetched_at.clone());
    if primary.code != supplemental.code
        || primary.normalized_period != supplemental.normalized_period
        || primary.report_date != supplemental.report_date
    {
        primary.field_errors.insert(
            "sina_supplement".to_string(),
            format!(
                "新浪报告期 {}（{}）与主来源报告期 {}（{}）不一致，未混合跨期数据",
                supplemental.normalized_period,
                supplemental.report_date.as_deref().unwrap_or("未知"),
                primary.normalized_period,
                primary.report_date.as_deref().unwrap_or("未知")
            ),
        );
        return Ok(primary);
    }

    let mut sina_used = false;
    for (primary_metric, supplemental_metric) in [
        (&mut primary.revenue, &supplemental.revenue),
        (&mut primary.net_profit, &supplemental.net_profit),
        (&mut primary.eps, &supplemental.eps),
        (&mut primary.roe, &supplemental.roe),
    ] {
        if primary_metric.value.is_none() && supplemental_metric.value.is_some() {
            *primary_metric = supplemental_metric.clone();
            sina_used = true;
        }
    }
    for (key, metric) in supplemental.additional_metrics {
        let should_fill = primary
            .additional_metrics
            .get(&key)
            .is_none_or(|existing| existing.value.is_none());
        if should_fill && metric.value.is_some() {
            primary.additional_metrics.insert(key, metric);
            sina_used = true;
        }
    }
    if sina_used {
        if !primary.candidate_source.contains("Sina") {
            primary
                .candidate_source
                .push_str(" + Sina Finance HK statements");
        }
    }
    sync_hk_financial_field_errors(&mut primary);
    Ok(primary)
}

pub(crate) fn preserve_same_period_hk_financials(
    previous: &HkFinancialSnapshot,
    refreshed: &mut HkFinancialSnapshot,
) {
    if previous.code != refreshed.code
        || previous.normalized_period != refreshed.normalized_period
        || previous.report_date != refreshed.report_date
    {
        return;
    }
    if refreshed.company_profile.is_none() {
        refreshed.company_profile = previous.company_profile.clone();
    }
    for (old, new) in [
        (&previous.revenue, &mut refreshed.revenue),
        (&previous.net_profit, &mut refreshed.net_profit),
        (&previous.eps, &mut refreshed.eps),
        (&previous.roe, &mut refreshed.roe),
    ] {
        if new.value.is_none() && old.value.is_some() {
            *new = old.clone();
        }
    }
    for (key, old) in &previous.additional_metrics {
        let should_preserve = refreshed
            .additional_metrics
            .get(key)
            .is_none_or(|new| new.value.is_none());
        if should_preserve && old.value.is_some() {
            refreshed
                .additional_metrics
                .insert(key.clone(), old.clone());
        }
    }
    sync_hk_financial_field_errors(refreshed);
}

pub(crate) fn parse_sina_hk_financial_snapshot(
    text: &str,
    code: &str,
    fetched_at: &str,
) -> Result<HkFinancialSnapshot, String> {
    use scraper::{Html, Selector};

    let canonical =
        canonical_hk_code(code).ok_or_else(|| format!("invalid HK security code: {code}"))?;
    let document = Html::parse_document(text);
    let table_selector = Selector::parse("table").map_err(|error| error.to_string())?;
    let row_selector = Selector::parse("tr").map_err(|error| error.to_string())?;
    let cell_selector = Selector::parse("th, td").map_err(|error| error.to_string())?;
    // A unit elsewhere in a statement must not become the page default.
    let page_unit = document
        .root_element()
        .descendants()
        .filter(|node| {
            !node.ancestors().any(|ancestor| {
                ancestor
                    .value()
                    .as_element()
                    .is_some_and(|element| matches!(element.name(), "table" | "script" | "style"))
            })
        })
        .filter_map(|node| node.value().as_text())
        .find_map(|text| sina_hk_unit_declaration(text))
        .map(|unit| parse_sina_hk_amount_unit(&unit))
        .unwrap_or_else(|| Err("Sina HK statement is missing an amount unit".to_string()));
    let mut periods: HashMap<String, SinaHkPeriodValues> = HashMap::new();

    for table in document.select(&table_selector) {
        let rows = table
            .select(&row_selector)
            .map(|row| {
                row.select(&cell_selector)
                    .map(|cell| {
                        normalized_hk_metric_text(&cell.text().collect::<String>())
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ")
                            .trim()
                            .to_string()
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|row| row.len() > 1)
            .collect::<Vec<_>>();
        let date_row = rows
            .iter()
            .find(|row| {
                let label = row
                    .first()
                    .map(|value| compact_hk_metric_label(value))
                    .unwrap_or_default();
                label.contains("截止日期") || label == "报告期" || label == "報告期"
            })
            .or_else(|| {
                rows.iter().find(|row| {
                    row.iter()
                        .skip(1)
                        .any(|value| parse_hk_financial_date(value).is_some())
                })
            });
        let Some(date_row) = date_row else { continue };
        let date_columns = date_row
            .iter()
            .enumerate()
            .skip(1)
            .filter_map(|(index, value)| parse_hk_financial_date(value).map(|date| (index, date)))
            .collect::<Vec<_>>();
        if date_columns.is_empty() {
            continue;
        }
        let announcement_row = rows.iter().find(|row| {
            row.first()
                .is_some_and(|value| compact_hk_metric_label(value).contains("公告日期"))
        });
        let currency_row = rows.iter().find(|row| {
            row.first().is_some_and(|value| {
                let label = compact_hk_metric_label(value);
                label == "币种" || label == "幣種" || label == "货币" || label == "貨幣"
            })
        });
        let unit_row = rows.iter().find(|row| {
            row.first().is_some_and(|value| {
                let label = compact_hk_metric_label(value);
                label == "单位" || label == "單位"
            })
        });
        let caption_unit = table
            .select(&Selector::parse("caption").map_err(|error| error.to_string())?)
            .find_map(|caption| sina_hk_unit_declaration(&caption.text().collect::<String>()))
            .map(|unit| parse_sina_hk_amount_unit(&unit));
        let report_type_row = rows.iter().find(|row| {
            row.first().is_some_and(|value| {
                let label = compact_hk_metric_label(value);
                label == "报表类型" || label == "報表類型"
            })
        });

        for (index, report_date) in date_columns {
            let raw_currency = currency_row.and_then(|row| row.get(index)).ok_or_else(|| {
                format!("Sina HK statement {report_date} is missing the source currency")
            })?;
            let table_currency = parse_sina_hk_statement_currency(raw_currency).ok_or_else(|| {
                format!("Sina HK statement {report_date} has an unsupported currency: {raw_currency}")
            })?;
            let amount_unit = if let Some(row) = unit_row {
                row.get(index)
                    .ok_or_else(|| {
                        format!("Sina HK statement {report_date} is missing an amount unit")
                    })
                    .and_then(|unit| parse_sina_hk_amount_unit(unit))
            } else {
                caption_unit.clone().unwrap_or_else(|| page_unit.clone())
            };
            let report_type = report_type_row
                .and_then(|row| row.get(index))
                .map(String::as_str)
                .unwrap_or("");
            let normalized_period = normalize_hk_period(report_type, Some(&report_date));
            let entry = periods
                .entry(report_date.clone())
                .or_insert_with(|| SinaHkPeriodValues {
                    raw_period: if report_type.is_empty() {
                        report_date.clone()
                    } else {
                        format!("{report_date} {report_type}")
                    },
                    normalized_period,
                    report_date: report_date.clone(),
                    disclosed_at: announcement_row
                        .and_then(|row| row.get(index))
                        .and_then(|value| parse_hk_financial_date(value)),
                    currency: table_currency.clone(),
                    metrics: HashMap::new(),
                });
            for row in &rows {
                let Some(label) = row.first() else { continue };
                let Some(raw_value) = row.get(index) else {
                    continue;
                };
                let Some((key, metric)) =
                    parse_sina_hk_financial_row(label, raw_value, &table_currency, &amount_unit)
                else {
                    continue;
                };
                entry.metrics.insert(key, metric);
            }
        }
    }
    let (_, values) = periods
        .into_iter()
        .max_by(|(left, _), (right, _)| left.cmp(right))
        .ok_or_else(|| "Sina HK financial page contains no dated statement tables".to_string())?;
    if values.metrics.is_empty() {
        return Err(
            "Sina HK financial page contains no recognized metrics for its latest report date"
                .to_string(),
        );
    }
    let metric = |key: &str, unit: &str, currency: &str| {
        values
            .metrics
            .get(key)
            .cloned()
            .unwrap_or_else(|| HkFinancialMetric {
                value: None,
                currency: currency.to_string(),
                unit: unit.to_string(),
                source: "Sina Finance HK statements".to_string(),
                reason: Some("新浪财务表未返回此字段".to_string()),
            })
    };
    let mut snapshot = HkFinancialSnapshot {
        code: canonical,
        currency: values.currency.clone(),
        raw_report_period: values.raw_period,
        normalized_period: values.normalized_period,
        report_date: Some(values.report_date),
        disclosed_at: values.disclosed_at,
        candidate_source: "Sina Finance HK statements".to_string(),
        candidate_fetched_at: fetched_at.to_string(),
        company_profile: None,
        supplemental_source: None,
        supplemental_fetched_at: None,
        official_source_url: None,
        official_source_name: None,
        verification_status: "unverified".to_string(),
        revenue: metric("revenue", "currency_total", &values.currency),
        net_profit: metric("net_profit", "currency_total", &values.currency),
        eps: metric("eps", "currency/share", &values.currency),
        roe: metric("roe", "percent", ""),
        pe: HkFinancialMetric {
            value: None,
            currency: String::new(),
            unit: "ratio".to_string(),
            source: "Tencent Finance".to_string(),
            reason: Some("等待腾讯行情字段".to_string()),
        },
        pb: HkFinancialMetric {
            value: None,
            currency: String::new(),
            unit: "ratio".to_string(),
            source: "Tencent Finance".to_string(),
            reason: Some("等待腾讯行情字段".to_string()),
        },
        dividend_yield: HkFinancialMetric {
            value: None,
            currency: String::new(),
            unit: "percent".to_string(),
            source: "Tencent Finance".to_string(),
            reason: Some("等待腾讯行情字段".to_string()),
        },
        market_cap: HkFinancialMetric {
            value: None,
            currency: "HKD".to_string(),
            unit: "currency_total".to_string(),
            source: "Tencent Finance".to_string(),
            reason: Some("等待腾讯行情字段".to_string()),
        },
        total_shares: HkFinancialMetric {
            value: None,
            currency: String::new(),
            unit: "shares".to_string(),
            source: "Tencent Finance".to_string(),
            reason: Some("等待腾讯行情字段".to_string()),
        },
        circulating_shares: HkFinancialMetric {
            value: None,
            currency: String::new(),
            unit: "shares".to_string(),
            source: "Tencent Finance".to_string(),
            reason: Some("等待腾讯行情字段".to_string()),
        },
        additional_metrics: values
            .metrics
            .into_iter()
            .filter(|(key, _)| !["revenue", "net_profit", "eps", "roe"].contains(&key.as_str()))
            .collect(),
        official_values: HashMap::new(),
        verification_conflicts: Vec::new(),
        field_errors: HashMap::new(),
    };
    sync_hk_financial_field_errors(&mut snapshot);
    Ok(snapshot)
}

#[derive(Clone)]
struct SinaHkPeriodValues {
    raw_period: String,
    normalized_period: String,
    report_date: String,
    disclosed_at: Option<String>,
    currency: String,
    metrics: HashMap<String, HkFinancialMetric>,
}

#[derive(Clone)]
struct SinaHkDenomination {
    multiplier: f64,
    currency: Option<String>,
}

fn parse_sina_hk_statement_currency(value: &str) -> Option<String> {
    let currency = normalize_hk_financial_currency(&normalized_hk_metric_text(value))?;
    // Do not treat an arbitrary three-letter placeholder as a currency.
    [
        "HKD", "CNY", "USD", "EUR", "GBP", "JPY", "AUD", "CAD", "CHF", "SGD", "NZD", "TWD", "KRW",
        "INR",
    ]
    .contains(&currency.as_str())
    .then_some(currency)
}

fn sina_hk_unit_declaration(value: &str) -> Option<String> {
    let value = normalized_hk_metric_text(value).replace('單', "单");
    let (_, unit) = value.split_once("单位")?;
    let unit = unit.trim_start_matches([' ', ':', '：', '(', '（']);
    Some(unit.split([')', '）', '\n']).next()?.trim().to_string())
}

fn parse_sina_hk_amount_unit(value: &str) -> Result<SinaHkDenomination, String> {
    let unit = compact_hk_metric_label(&normalized_hk_metric_text(value))
        .trim_end_matches(')')
        .to_string();
    let (multiplier, denomination) = [
        ("十亿", 1_000_000_000.0),
        ("千万", 10_000_000.0),
        ("百万", 1_000_000.0),
        ("亿", 100_000_000.0),
        ("万", 10_000.0),
        ("千", 1_000.0),
        ("billion", 1_000_000_000.0),
        ("million", 1_000_000.0),
        ("thousand", 1_000.0),
    ]
    .into_iter()
    .find_map(|(prefix, multiplier)| unit.strip_prefix(prefix).map(|rest| (multiplier, rest)))
    .unwrap_or((1.0, unit.as_str()));
    let currency =
        match denomination {
            "元" => None,
            "" if multiplier != 1.0 && unit.is_ascii() => None,
            other => Some(parse_sina_hk_statement_currency(other).ok_or_else(|| {
                format!("Sina HK statement has an unsupported amount unit: {value}")
            })?),
        };
    Ok(SinaHkDenomination {
        multiplier,
        currency,
    })
}

fn sina_hk_row_unit(label: &str) -> Option<&str> {
    let (_, suffix) = label.split_once('(').or_else(|| label.split_once('（'))?;
    let unit = suffix.trim_end_matches([')', '）']).trim();
    // Parenthetical loss labels describe the metric, rather than its denomination.
    (!matches!(unit, "虧損" | "亏损" | "損失" | "损失")).then_some(unit)
}

fn parse_sina_hk_per_share_unit(label: &str) -> Result<SinaHkDenomination, String> {
    let Some(unit) = sina_hk_row_unit(label) else {
        return Ok(SinaHkDenomination {
            multiplier: 1.0,
            currency: None,
        });
    };
    let unit = compact_hk_metric_label(&normalized_hk_metric_text(unit));
    let denomination = unit.trim_end_matches("/股").trim_end_matches("/share");
    let currency = match denomination {
        "仙" | "分" | "cent" | "cents" => None,
        "港仙" => Some("HKD".to_string()),
        "美分" => Some("USD".to_string()),
        _ => {
            return parse_sina_hk_amount_unit(denomination).and_then(|unit| {
                if unit.multiplier == 1.0 {
                    Ok(unit)
                } else {
                    Err(format!(
                        "Sina HK statement has an unsupported per-share unit: {denomination}"
                    ))
                }
            })
        }
    };
    Ok(SinaHkDenomination {
        multiplier: 0.01,
        currency,
    })
}

fn parse_sina_hk_financial_row(
    label: &str,
    raw_value: &str,
    table_currency: &str,
    amount_unit: &Result<SinaHkDenomination, String>,
) -> Option<(String, HkFinancialMetric)> {
    let normalized_label = normalized_hk_metric_text(label);
    let label = compact_hk_metric_label(&normalized_label);
    let base_label = compact_hk_metric_label(normalized_label.split(['(', '（']).next()?);
    let raw_value = raw_value.trim();
    if raw_value.is_empty() || raw_value == "--" || raw_value == "-" {
        return None;
    }
    let value = parse_f64(Some(raw_value.trim_end_matches('%').trim()))?;
    let (key, value, unit, currency) = if label.contains("营業额同比")
        || label.contains("營業額同比")
        || label.contains("营业额同比")
        || label.contains("营业收入同比")
        || label.contains("營業收入同比")
    {
        ("revenue_yoy", value, "percent", String::new())
    } else if label.contains("股东应占盈利同比")
        || label.contains("股東應佔盈利同比")
        || label.contains("股东应占溢利同比")
        || label.contains("股東應佔溢利同比")
        || label.contains("归母净利润同比")
    {
        ("net_profit_yoy", value, "percent", String::new())
    } else if label.contains("基本每股盈利") || label.contains("基本每股收益") {
        ("eps", value, "currency/share", table_currency.to_string())
    } else if label.contains("摊薄每股盈利")
        || label.contains("攤薄每股盈利")
        || label.contains("摊薄每股收益")
        || label.contains("攤薄每股收益")
    {
        (
            "diluted_eps",
            value,
            "currency/share",
            table_currency.to_string(),
        )
    } else if label.contains("每股资产净值")
        || label.contains("每股資產淨值")
        || label.contains("每股净资产")
    {
        ("bps", value, "currency/share", table_currency.to_string())
    } else if label.contains("股东应占盈利")
        || label.contains("股東應佔盈利")
        || label.contains("股东应占溢利")
        || label.contains("股東應佔溢利")
        || label.contains("归母净利润")
    {
        (
            "net_profit",
            value,
            "currency_total",
            table_currency.to_string(),
        )
    } else if base_label == "营业额" || base_label == "营业收入" || base_label == "营业总收入"
    {
        (
            "revenue",
            value,
            "currency_total",
            table_currency.to_string(),
        )
    } else if label.contains("毛利率") {
        ("gross_margin", value, "percent", String::new())
    } else if label.contains("净利率") || label.contains("淨利率") {
        ("net_margin", value, "percent", String::new())
    } else if label.contains("股东权益回报率")
        || label.contains("股東權益回報率")
        || label.contains("净资产收益率")
        || label.contains("淨資產收益率")
        || label.eq_ignore_ascii_case("roe")
    {
        ("roe", value, "percent", String::new())
    } else if label.contains("资产负债率") || label.contains("資產負債率") {
        ("asset_liability_ratio", value, "percent", String::new())
    } else if label.contains("经营业务所得之现金流入净额")
        || label.contains("經營業務所得之現金流入淨額")
        || label.contains("经营现金流")
    {
        (
            "operating_cash_flow",
            value,
            "currency_total",
            table_currency.to_string(),
        )
    } else if label.contains("流动比率") || label.contains("流動比率") {
        ("current_ratio", value, "ratio", String::new())
    } else {
        return None;
    };
    let denomination = match unit {
        "currency_total" => Some(
            sina_hk_row_unit(&normalized_label)
                .map(parse_sina_hk_amount_unit)
                .unwrap_or_else(|| amount_unit.clone()),
        ),
        "currency/share" => Some(parse_sina_hk_per_share_unit(&normalized_label)),
        _ => None,
    };
    let mut metric = HkFinancialMetric {
        value: Some(value),
        currency,
        unit: unit.to_string(),
        source: "Sina Finance HK statements".to_string(),
        reason: None,
    };
    if let Some(denomination) = denomination {
        match denomination {
            Ok(denomination) => {
                metric.value = Some(value * denomination.multiplier);
                if let Some(currency) = denomination.currency {
                    metric.currency = currency;
                }
            }
            Err(error) => {
                metric.value = None;
                metric.reason = Some(error);
            }
        }
    }
    Some((key.to_string(), metric))
}

fn compact_hk_metric_label(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            !character.is_whitespace() && !matches!(character, ':' | '：' | '（' | '(')
        })
        .collect::<String>()
        .replace("）", ")")
        .to_lowercase()
}

fn normalized_hk_metric_text(value: &str) -> String {
    value
        .replace('營', "营")
        .replace('業', "业")
        .replace('報', "报")
        .replace('幣', "币")
        .replace('種', "种")
        .replace('貨', "货")
        .replace('額', "额")
        .replace('東', "东")
        .replace('應', "应")
        .replace('佔', "占")
        .replace('潤', "润")
        .replace('淨', "净")
        .replace('資', "资")
        .replace('產', "产")
        .replace('萬', "万")
        .replace('億', "亿")
        .replace('負', "负")
        .replace('債', "债")
        .replace('率', "率")
}

fn parse_hk_financial_date(value: &str) -> Option<String> {
    let value = value.trim();
    let digits = value
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<String>();
    if digits.len() >= 4 {
        let year = &digits[..4];
        if value.contains("中報") || value.contains("中报") || value.contains("半年") {
            return Some(format!("{year}-06-30"));
        }
        if value.contains("年報") || value.contains("年报") || value.contains("年度") {
            return Some(format!("{year}-12-31"));
        }
        if value.contains("一季") || value.contains("一季度") {
            return Some(format!("{year}-03-31"));
        }
        if value.contains("三季") || value.contains("三季度") {
            return Some(format!("{year}-09-30"));
        }
    }
    if digits.len() == 8 {
        Some(format!(
            "{}-{}-{}",
            &digits[..4],
            &digits[4..6],
            &digits[6..8]
        ))
    } else if digits.len() == 6 {
        let year = format!("20{}", &digits[..2]);
        Some(format!("{year}-{}-{}", &digits[2..4], &digits[4..6]))
    } else {
        None
    }
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
    for (name, metric) in &snapshot.additional_metrics {
        if let Some(reason) = metric.reason.as_ref() {
            snapshot.field_errors.insert(name.clone(), reason.clone());
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HkexQuarterlyAnnouncement {
    pub title: String,
    pub disclosed_at: String,
    pub source_url: String,
}

pub(crate) fn parse_hkex_quarterly_update_text(
    text: &str,
    code: &str,
    title: &str,
    disclosed_at: &str,
    source_url: &str,
    fetched_at: &str,
) -> Result<HkQuarterlyUpdate, String> {
    let code = canonical_hk_code(code).ok_or_else(|| format!("invalid HK stock code: {code}"))?;
    let body_period = parse_hk_quarter_period_end(text);
    let title_period = parse_hk_quarter_period_end(title).filter(|date| date.ends_with("-09-30"));
    if let (Some(body), Some(title)) = (&body_period, &title_period) {
        if body.ends_with("-09-30") && body != title {
            return Err(
                "HKEX update title and financial text disagree on the reporting period".to_string(),
            );
        }
    }
    let period_end = title_period
        .or(body_period)
        .ok_or_else(|| "HKEX update does not contain a recognized quarter-end date".to_string())?;
    if !period_end.ends_with("-09-30") {
        return Err("HKEX update is not a third-quarter update".to_string());
    }

    let lines = text.lines().map(str::trim).collect::<Vec<_>>();
    let revenue_values = parse_hk_update_metric_row(&lines, "revenue").unwrap_or_default();
    let profit_values = parse_hk_update_metric_row(&lines, "profit for the period")
        .or_else(|| parse_hk_update_metric_row(&lines, "net profit"))
        .or_else(|| parse_hk_update_metric_row(&lines, "profit attributable"))
        .unwrap_or_default();
    let comprehensive_income_values =
        parse_hk_update_metric_row(&lines, "total comprehensive income").unwrap_or_default();
    if revenue_values.is_empty()
        && profit_values.is_empty()
        && comprehensive_income_values.is_empty()
    {
        return Err("HKEX update contains no recognized financial values".to_string());
    }

    let currency = if text.to_ascii_lowercase().contains("rmb")
        || text.to_ascii_lowercase().contains("renminbi")
        || text.to_ascii_lowercase().contains("cny")
    {
        "CNY"
    } else if text.to_ascii_lowercase().contains("hkd")
        || text.to_ascii_lowercase().contains("hong kong dollar")
    {
        "HKD"
    } else if text.to_ascii_lowercase().contains("us$") || text.to_ascii_lowercase().contains("usd")
    {
        "USD"
    } else {
        ""
    };
    if currency.is_empty() {
        return Err("HKEX update does not state its reporting currency".to_string());
    }
    let lower = text.to_ascii_lowercase();
    let unit = if lower.contains("billion") {
        "billion"
    } else if lower.contains("million") {
        "million"
    } else if lower.contains("thousand") || lower.contains("'000") || lower.contains("’000") {
        "thousand"
    } else {
        "currency"
    };
    let audit_status = if lower.contains("unaudited") {
        "unaudited"
    } else if lower.contains("audited") {
        "audited"
    } else {
        "unspecified"
    };

    Ok(HkQuarterlyUpdate {
        code,
        period: period_end[..4].to_string() + "Q3",
        period_end,
        disclosed_at: disclosed_at.to_string(),
        title: title.trim().to_string(),
        source_name: "HKEXnews".to_string(),
        source_url: source_url.to_string(),
        currency: currency.to_string(),
        unit: unit.to_string(),
        audit_status: audit_status.to_string(),
        fetched_at: fetched_at.to_string(),
        quarterly_revenue: revenue_values.first().copied(),
        quarterly_revenue_yoy_percent: revenue_values.get(4).copied(),
        quarterly_revenue_qoq_percent: revenue_values.get(2).copied(),
        nine_month_revenue: revenue_values.get(5).copied(),
        nine_month_revenue_yoy_percent: revenue_values.get(7).copied(),
        quarterly_net_profit: profit_values.first().copied(),
        quarterly_net_profit_yoy_percent: profit_values.get(4).copied(),
        quarterly_net_profit_qoq_percent: profit_values.get(2).copied(),
        nine_month_net_profit: profit_values.get(5).copied(),
        nine_month_net_profit_yoy_percent: profit_values.get(7).copied(),
        quarterly_total_comprehensive_income: comprehensive_income_values.first().copied(),
        quarterly_total_comprehensive_income_yoy_percent: comprehensive_income_values
            .get(4)
            .copied(),
        quarterly_total_comprehensive_income_qoq_percent: comprehensive_income_values
            .get(2)
            .copied(),
        nine_month_total_comprehensive_income: comprehensive_income_values.get(5).copied(),
        nine_month_total_comprehensive_income_yoy_percent: comprehensive_income_values
            .get(7)
            .copied(),
    })
}

fn parse_hk_update_metric_row(lines: &[&str], metric: &str) -> Option<Vec<f64>> {
    let start = lines
        .iter()
        .position(|line| line.to_ascii_lowercase().starts_with(metric))?;
    let row = lines
        .iter()
        .skip(start)
        .take(64)
        .enumerate()
        .take_while(|(index, line)| *index == 0 || !starts_hk_update_metric_label(line))
        .map(|(_, line)| *line)
        .collect::<Vec<_>>()
        .join(" ");
    let values = parse_hk_update_numbers(&row);
    // The comparative table has five monetary cells and three percentage cells.
    // Incomplete or differently shaped rows must not shift another value into them.
    (values.len() >= 8).then(|| values.into_iter().take(8).collect())
}

fn starts_hk_update_metric_label(line: &str) -> bool {
    let lower = line.trim().to_ascii_lowercase();
    [
        "revenue",
        "corporate client",
        "self-branded",
        "profit before tax",
        "profit for the period",
        "net profit",
        "profit attributable",
        "total comprehensive income",
        "adjusted profit",
        "add:",
        "subtract:",
    ]
    .iter()
    .any(|label| lower.starts_with(label))
}

fn parse_hk_update_numbers(text: &str) -> Vec<f64> {
    let mut negative_next = false;
    text.replace('–', "-")
        .replace('−', "-")
        .replace('—', "-")
        .split_whitespace()
        .filter_map(|token| {
            if token == "-" {
                negative_next = true;
                return None;
            }
            let accounting_negative = token.starts_with('(') && token.ends_with(')');
            let value_text = token
                .trim_matches(|character: char| {
                    matches!(character, '(' | ')' | '[' | ']' | ',' | ';' | ':')
                })
                .trim_end_matches('%')
                .replace(',', "");
            let parsed = value_text.parse::<f64>();
            let negative = std::mem::take(&mut negative_next);
            let mut value = parsed.ok().filter(|value| value.is_finite())?;
            if accounting_negative || negative {
                value = -value.abs();
            }
            Some(value)
        })
        .collect()
}

fn parse_hk_quarter_period_end(text: &str) -> Option<String> {
    for line in text.lines() {
        let normalized = line
            .replace(',', " ")
            .replace('.', " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let tokens = normalized.split_whitespace().collect::<Vec<_>>();
        for window in tokens.windows(3) {
            for format in ["%d %B %Y", "%d %b %Y"] {
                if let Ok(date) = chrono::NaiveDate::parse_from_str(&window.join(" "), format) {
                    return Some(date.format("%Y-%m-%d").to_string());
                }
            }
        }
        for token in tokens {
            for format in ["%Y-%m-%d", "%d/%m/%Y", "%d-%m-%Y"] {
                if let Ok(date) = chrono::NaiveDate::parse_from_str(token, format) {
                    return Some(date.format("%Y-%m-%d").to_string());
                }
            }
        }
    }
    None
}

pub(crate) fn parse_hkex_quarterly_announcements(
    html: &str,
    requested_code: &str,
) -> Vec<HkexQuarterlyAnnouncement> {
    let Some(requested_code) = canonical_hk_code(requested_code) else {
        return Vec::new();
    };
    let Ok(outer) = serde_json::from_str::<Value>(html) else {
        return Vec::new();
    };
    let inner = match outer.get("result") {
        Some(Value::String(result)) => {
            let Ok(result) = serde_json::from_str::<Value>(result) else {
                return Vec::new();
            };
            result
        }
        Some(result) => result.clone(),
        None => outer,
    };
    let rows = inner
        .as_array()
        .or_else(|| inner.get("data").and_then(Value::as_array))
        .or_else(|| inner.get("results").and_then(Value::as_array))
        .or_else(|| inner.get("rows").and_then(Value::as_array))
        .or_else(|| inner.get("searchResultList").and_then(Value::as_array));
    let Some(rows) = rows else {
        return Vec::new();
    };
    let base = reqwest::Url::parse("https://www1.hkexnews.hk/").expect("constant HKEX URL");
    let mut announcements = Vec::new();
    for row in rows {
        if let Some(row_codes) = row
            .get("STOCK_CODE")
            .or_else(|| row.get("stock_code"))
            .and_then(Value::as_str)
        {
            let matches_requested = row_codes
                .split(|character: char| !character.is_ascii_digit())
                .filter(|value| !value.is_empty())
                .any(|value| canonical_hk_code(value).as_deref() == Some(requested_code.as_str()));
            if !matches_requested {
                continue;
            }
        }
        let title = row
            .get("TITLE")
            .or_else(|| row.get("title"))
            .or_else(|| row.get("TITLE_EN"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if !hkex_quarterly_title_candidate(&title) {
            continue;
        }
        let Some(file_link) = row
            .get("FILE_LINK")
            .or_else(|| row.get("file_link"))
            .or_else(|| row.get("FILELINK"))
            .or_else(|| row.get("fileLink"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let Ok(source_url) = base.join(file_link) else {
            continue;
        };
        if !matches!(
            source_url.host_str(),
            Some("www1.hkexnews.hk" | "www.hkexnews.hk")
        ) || !source_url.path().to_ascii_lowercase().ends_with(".pdf")
        {
            continue;
        }
        let Some(disclosed_at) = row
            .get("DATE_TIME")
            .or_else(|| row.get("date_time"))
            .or_else(|| row.get("DATE"))
            .or_else(|| row.get("date"))
            .and_then(Value::as_str)
            .and_then(|value| parse_hkex_display_date(value))
        else {
            continue;
        };
        announcements.push(HkexQuarterlyAnnouncement {
            title: title.to_string(),
            disclosed_at,
            source_url: source_url.to_string(),
        });
    }
    announcements.sort_by(|left, right| right.disclosed_at.cmp(&left.disclosed_at));
    announcements
}

fn hkex_quarterly_title_candidate(title: &str) -> bool {
    let lower = title.to_ascii_lowercase();
    let is_financial_update = lower.contains("financial update")
        || lower.contains("quarterly update")
        || lower.contains("quarterly results")
        || lower.contains("quarterly report")
        || lower.contains("nine months ended");
    let is_q3_period = lower.contains("third quarter")
        || lower.contains("quarter 3")
        || lower.contains("q3")
        || lower.contains("nine months")
        || lower.contains("september")
        || lower.contains("sep ");
    is_financial_update && is_q3_period
}

fn parse_hkex_display_date(text: &str) -> Option<String> {
    let normalized = text.replace(',', " ");
    let tokens = normalized
        .split_whitespace()
        .map(|token| token.trim_matches(|character: char| "()[]{}".contains(character)))
        .collect::<Vec<_>>();
    for token in &tokens {
        for format in ["%Y-%m-%d", "%Y/%m/%d", "%Y%m%d", "%d/%m/%Y", "%d-%m-%Y"] {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(token, format) {
                return Some(date.format("%Y-%m-%d").to_string());
            }
        }
    }
    for window in tokens.windows(3) {
        for format in ["%d %B %Y", "%d %b %Y"] {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(&window.join(" "), format) {
                return Some(date.format("%Y-%m-%d").to_string());
            }
        }
    }
    None
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
            if let Some(previous) = cache.financials.get(&code) {
                preserve_same_period_hk_financials(previous, &mut value);
            }
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

pub(crate) fn merge_hk_quarterly_update_refresh(
    cache: &mut HkMarketCache,
    code: &str,
    update: Result<Option<HkQuarterlyUpdate>, String>,
    checked_at: &str,
) {
    let Some(code) = canonical_hk_code(code) else {
        return;
    };
    let existing = cache.quarterly_updates.get(&code).cloned();
    let mut record = HkQuarterlyUpdateRecord {
        update: existing.as_ref().and_then(|value| value.update.clone()),
        checked_at: checked_at.to_string(),
        error: None,
    };
    match update {
        Ok(Some(value)) => {
            if value.code == code
                && record
                    .update
                    .as_ref()
                    .is_none_or(|previous| previous.period_end <= value.period_end)
            {
                record.update = Some(value);
            }
        }
        Ok(None) => {}
        Err(error) => record.error = Some(error),
    }
    cache.quarterly_updates.insert(code, record);
}

pub(crate) fn hk_observation_should_refresh(data_policy: &str) -> bool {
    data_policy == "refresh"
}

pub(crate) fn build_hk_financial_result(cache: &HkMarketCache, code: &str) -> HkFinancialResult {
    let code = canonical_hk_code(code).unwrap_or_else(|| code.to_string());
    let snapshot = cache.financials.get(&code).cloned();
    let fx_rate = cache.fx_rate.clone();
    let official_disclosure = cache.disclosures.get(&code).cloned();
    let quarterly_update_record = cache.quarterly_updates.get(&code);
    let quarterly_update = quarterly_update_record.and_then(|value| value.update.clone());
    let quarterly_update_checked_at = quarterly_update_record.map(|value| value.checked_at.clone());
    let quarterly_update_error = quarterly_update_record.and_then(|value| value.error.clone());
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
        quarterly_update,
        quarterly_update_checked_at,
        quarterly_update_error,
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
    ensure_hk_provider_ready("sina_hk_financial")?;
    let (eastmoney, sina, profile) = tokio::join!(
        fetch_hk_financial_snapshot_from(client, &EastmoneyHkFinancialProvider, code, fetched_at),
        fetch_sina_hk_financial_snapshot_from(client, &SinaHkFinancialProvider, code, fetched_at),
        fetch_hk_company_profile(client, code, fetched_at),
    );
    let mut snapshot = merge_hk_financial_candidates(eastmoney, sina)?;
    match profile {
        Ok(profile) => snapshot.company_profile = Some(profile),
        Err(error) => {
            snapshot
                .field_errors
                .insert("company_profile".to_owned(), error);
        }
    }
    Ok(snapshot)
}

async fn fetch_sina_hk_financial_snapshot_from(
    client: &reqwest::Client,
    provider: &dyn HkSupplementalFinancialProvider,
    code: &str,
    fetched_at: &str,
) -> Result<HkFinancialSnapshot, String> {
    let url = provider.endpoint(code)?;
    let timeout = std::time::Duration::from_secs(HK_REQUEST_TIMEOUT_SECS);
    let response = client
        .get(&url)
        .header(reqwest::header::REFERER, "https://finance.sina.com.cn/")
        .header(
            reqwest::header::USER_AGENT,
            "Mozilla/5.0 GuXuanYou HK financial supplement",
        )
        .timeout(timeout)
        .send()
        .await
        .map_err(|error| format!("Sina HK financial request failed: {error}"))?;
    let body = crate::external_data::provider::read_bounded_response(response, 6 * 1024 * 1024)
        .await
        .map_err(|error| {
            format!(
                "Sina HK financial response failed: {}: {}",
                error.category(),
                error.message()
            )
        })?;
    let (utf8, _, had_utf8_errors) = encoding_rs::UTF_8.decode(&body);
    let html = if had_utf8_errors {
        let (gbk, _, had_gbk_errors) = encoding_rs::GBK.decode(&body);
        if had_gbk_errors {
            return Err("Sina HK financial response has unsupported text encoding".to_string());
        }
        gbk.into_owned()
    } else {
        utf8.into_owned()
    };
    provider.parse(&html, code, fetched_at)
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

fn hkex_quarterly_update_search_url(
    disclosure: &HkDisclosureMetadata,
    from: &str,
    to: &str,
) -> Result<String, String> {
    let source_url = disclosure
        .source_url
        .as_deref()
        .ok_or_else(|| "HKEX issuer identifier is unavailable".to_string())?;
    let source = reqwest::Url::parse(source_url)
        .map_err(|error| format!("invalid HKEX issuer search URL: {error}"))?;
    if source.host_str() != Some("www1.hkexnews.hk") {
        return Err("HKEX issuer search URL has an unexpected host".to_string());
    }
    let stock_id = source
        .query_pairs()
        .find_map(|(key, value)| (key == "stockId").then(|| value.into_owned()))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "HKEX issuer identifier is missing from its search URL".to_string())?;
    let mut url = reqwest::Url::parse(HKEX_TITLE_SEARCH_ENDPOINT)
        .expect("constant HKEX title search endpoint should be valid");
    url.query_pairs_mut()
        .append_pair("lang", "E")
        .append_pair("stockId", &stock_id)
        .append_pair("fromDate", from)
        .append_pair("toDate", to)
        .append_pair("category", "0")
        .append_pair("market", "SEHK")
        .append_pair("sortDir", "1")
        .append_pair("sortByOptions", "DateTime")
        .append_pair("documentType", "-1")
        .append_pair("title", "")
        .append_pair("searchType", "0")
        .append_pair("t1code", "-2")
        .append_pair("t2Gcode", "-2")
        .append_pair("t2code", "-2")
        .append_pair("rowRange", "100");
    Ok(url.to_string())
}

fn hkex_quarterly_update_date_range() -> (String, String) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default();
    let today = chrono::DateTime::<chrono::Utc>::from_timestamp(now, 0)
        .map(|value| value.date_naive())
        .unwrap_or_else(|| chrono::NaiveDate::from_ymd_opt(2026, 11, 7).expect("valid fallback"));
    let from = today
        .checked_sub_signed(chrono::Duration::days(370))
        .unwrap_or(today);
    (
        from.format("%Y%m%d").to_string(),
        today.format("%Y%m%d").to_string(),
    )
}

pub(crate) async fn fetch_hkex_quarterly_update(
    client: &reqwest::Client,
    code: &str,
    disclosure: &HkDisclosureMetadata,
    fetched_at: &str,
) -> Result<Option<HkQuarterlyUpdate>, String> {
    ensure_hk_provider_ready("hkexnews")?;
    let (from, to) = hkex_quarterly_update_date_range();
    let url = hkex_quarterly_update_search_url(disclosure, &from, &to)?;
    let response = client
        .get(&url)
        .header(reqwest::header::ACCEPT, "application/json, text/plain, */*")
        .header(
            reqwest::header::REFERER,
            "https://www1.hkexnews.hk/search/titlesearch.xhtml?lang=en",
        )
        .timeout(std::time::Duration::from_secs(HK_REQUEST_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|error| format!("HKEX quarterly search request failed: {error}"))?;
    let body =
        crate::external_data::provider::read_bounded_response(response, HKEX_UPDATE_MAX_HTML_BYTES)
            .await
            .map_err(|error| {
                format!(
                    "HKEX quarterly search response failed: {}: {}",
                    error.category(),
                    error.message()
                )
            })?;
    let html = String::from_utf8(body)
        .map_err(|error| format!("HKEX quarterly search response is not UTF-8: {error}"))?;
    let Some(announcement) = parse_hkex_quarterly_announcements(&html, code)
        .into_iter()
        .next()
    else {
        return Ok(None);
    };
    let response = client
        .get(&announcement.source_url)
        .header(reqwest::header::REFERER, "https://www1.hkexnews.hk/")
        .timeout(std::time::Duration::from_secs(HK_REQUEST_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|error| format!("HKEX quarterly update request failed: {error}"))?;
    let bytes =
        crate::external_data::provider::read_bounded_response(response, HKEX_UPDATE_MAX_PDF_BYTES)
            .await
            .map_err(|error| {
                format!(
                    "HKEX quarterly update PDF failed: {}: {}",
                    error.category(),
                    error.message()
                )
            })?;
    let text = extract_hkex_pdf_text(&bytes)?;
    match parse_hkex_quarterly_update_text(
        &text,
        code,
        &announcement.title,
        &announcement.disclosed_at,
        &announcement.source_url,
        fetched_at,
    ) {
        Ok(update) => Ok(Some(update)),
        Err(error) if error.contains("not a third-quarter update") => Ok(None),
        Err(error) => Err(error),
    }
}

fn extract_hkex_pdf_text(bytes: &[u8]) -> Result<String, String> {
    if !bytes.starts_with(b"%PDF-") {
        return Err("HKEX quarterly update response is not a PDF".to_string());
    }
    let mut document = Document::load_mem_with_options(
        bytes,
        LoadOptions::with_max_decompressed_size(8 * 1024 * 1024),
    )
    .map_err(|error| format!("open HKEX quarterly update PDF failed: {error}"))?;
    let pages = document.get_pages();
    if pages.is_empty() || pages.len() > 100 {
        return Err(format!(
            "HKEX quarterly update PDF has an unsupported page count: {}",
            pages.len()
        ));
    }
    for page_id in pages.values().copied() {
        let content = document
            .get_page_content_with_limit(page_id, 8 * 1024 * 1024)
            .map_err(|error| format!("read HKEX quarterly PDF page failed: {error}"))?;
        let content = Content::decode(&content)
            .map_err(|error| format!("decode HKEX quarterly PDF page failed: {error}"))?;
        let content = preserve_hk_pdf_text_spacing(content);
        document
            .change_page_content(
                page_id,
                content
                    .encode()
                    .map_err(|error| format!("prepare HKEX quarterly PDF text failed: {error}"))?,
            )
            .map_err(|error| format!("prepare HKEX quarterly PDF page failed: {error}"))?;
    }
    let mut text = String::new();
    for page in pages.keys().copied() {
        let page_text = document
            .extract_text_with_limit(&[page], 512 * 1024)
            .map_err(|error| format!("extract HKEX quarterly update PDF failed: {error}"))?;
        text.push_str(&page_text);
        text.push('\n');
    }
    if text.trim().is_empty() {
        return Err("HKEX quarterly update PDF contains no extractable text".to_string());
    }
    Ok(text)
}

fn preserve_hk_pdf_text_spacing(content: Content<Vec<Operation>>) -> Content<Vec<Operation>> {
    // lopdf's text collector handles integer kerning, but loses real-valued gaps
    // and text-position changes. Restore those separators in the extraction copy.
    fn preserve_kerning(value: &mut lopdf::Object) {
        match value {
            lopdf::Object::Real(gap) if *gap < -100.0 => *value = lopdf::Object::Integer(-101),
            lopdf::Object::Array(values) => values.iter_mut().for_each(preserve_kerning),
            _ => {}
        }
    }
    let mut operations = Vec::with_capacity(content.operations.len());
    let mut line_y: Option<f32> = None;
    for mut operation in content.operations {
        let separator = match operation.operator.as_str() {
            "BT" => {
                line_y = None;
                None
            }
            "Tm" => {
                let next_y = operation
                    .operands
                    .get(5)
                    .and_then(|value| value.as_float().ok());
                let newline = line_y
                    .zip(next_y)
                    .map(|(previous, next)| (next - previous).abs() > 1.0);
                line_y = next_y;
                newline
            }
            "Td" | "TD" => operation
                .operands
                .get(1)
                .and_then(|value| value.as_float().ok())
                .map(|dy| {
                    line_y = line_y.map(|previous| previous + dy);
                    dy.abs() > 1.0
                }),
            "TJ" => {
                operation.operands.iter_mut().for_each(preserve_kerning);
                None
            }
            _ => None,
        };
        operations.push(operation);
        if let Some(newline) = separator {
            operations.push(if newline {
                Operation::new("T*", vec![])
            } else {
                Operation::new(
                    "TJ",
                    vec![lopdf::Object::Array(vec![lopdf::Object::Integer(-101)])],
                )
            });
        }
    }
    Content { operations }
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
    if date.ends_with("06-30") {
        return format!("{year}H1");
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
    history: Result<HkHistoryRecord, String>,
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
            preserve_hk_chinese_name(cache.quotes.get(&code), &mut value);
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
        Ok(history) => {
            let record = merge_hk_history_record(cache.histories.get(&code), history);
            if let Some(reason) = &record.revision_reason {
                result.notes.push(reason.clone());
            }
            cache.histories.insert(code.clone(), record);
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
        quote_scope: None,
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
        sources: Vec::new(),
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
    if fields
        .get(2)
        .map(|value| normalize_hkex_code(value))
        .as_deref()
        != Some(code.as_str())
    {
        return Err(format!("Tencent HK quote code does not match {code}"));
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
        name_zh: name_zh_candidate(name),
        price,
        price_semantics_version: HK_QUOTE_PRICE_SEMANTICS_VERSION,
        previous_close: parse_f64(fields.get(4).copied()).filter(|value| *value > 0.0),
        open: parse_f64(fields.get(5).copied()).filter(|value| *value > 0.0),
        high: parse_f64(fields.get(33).copied()).filter(|value| *value > 0.0),
        low: parse_f64(fields.get(34).copied()).filter(|value| *value > 0.0),
        change: parse_f64(fields.get(31).copied()),
        change_pct: parse_f64(fields.get(32).copied()).map(|value| value / 100.0),
        volume_shares: parse_f64(fields.get(6).copied()).and_then(number_to_u64),
        turnover_hkd: parse_f64(fields.get(37).copied()).filter(|value| *value >= 0.0),
        pe: parse_f64(fields.get(39).copied()),
        pb: parse_f64(fields.get(58).copied()),
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

pub(crate) fn parse_sina_hk_quote(
    text: &str,
    code: &str,
    expected_currency: &CurrencyCode,
) -> Result<HkQuote, String> {
    let code = canonical_hk_code(code).ok_or_else(|| format!("invalid HK stock code: {code}"))?;
    if expected_currency.as_str() != "HKD" {
        return Err(format!(
            "Sina HK quote currency cannot be verified for {code}; only HKD listings are supported"
        ));
    }

    let symbol = format!("rt_hk{code}");
    let line = text
        .split(';')
        .find(|line| {
            line.trim_start()
                .starts_with(&format!("var hq_str_{symbol}="))
        })
        .ok_or_else(|| format!("Sina returned no quote for {code}"))?;
    let raw_fields = line
        .split_once('=')
        .map(|(_, value)| value.trim())
        .and_then(|value| value.strip_prefix('"')?.strip_suffix('"'))
        .ok_or_else(|| format!("Sina HK quote payload is malformed for {code}"))?;
    let fields = parse_sina_csv_fields(raw_fields)?;
    if fields.len() < 19 {
        return Err(format!("Sina HK quote payload is incomplete for {code}"));
    }
    let name = fields[1].trim();
    if name.is_empty() {
        return Err(format!("Sina HK quote has no valid name for {code}"));
    }
    let price = parse_f64(fields.get(6).map(String::as_str))
        .filter(|value| *value > 0.0)
        .ok_or_else(|| format!("Sina HK quote has no valid price for {code}"))?;
    let date = fields.get(17).map(String::as_str).unwrap_or_default();
    let time = fields.get(18).map(String::as_str).unwrap_or_default();
    let quote_time = if date.len() == 10
        && date.as_bytes().get(4) == Some(&b'/')
        && date.as_bytes().get(7) == Some(&b'/')
        && time.len() == 8
        && time.as_bytes().get(2) == Some(&b':')
        && time.as_bytes().get(5) == Some(&b':')
    {
        Some(format!(
            "{}-{}-{} {time}",
            &date[..4],
            &date[5..7],
            &date[8..10]
        ))
    } else {
        None
    };
    if quote_time.is_none() {
        return Err(format!(
            "Sina HK quote has no valid observation time for {code}"
        ));
    }

    Ok(HkQuote {
        code,
        name: name.to_owned(),
        name_zh: name_zh_candidate(name),
        price,
        price_semantics_version: HK_QUOTE_PRICE_SEMANTICS_VERSION,
        previous_close: parse_f64(fields.get(3).map(String::as_str)).filter(|value| *value > 0.0),
        open: parse_f64(fields.get(2).map(String::as_str)).filter(|value| *value > 0.0),
        high: parse_f64(fields.get(4).map(String::as_str)).filter(|value| *value > 0.0),
        low: parse_f64(fields.get(5).map(String::as_str)).filter(|value| *value > 0.0),
        change: parse_f64(fields.get(7).map(String::as_str)),
        change_pct: parse_f64(fields.get(8).map(String::as_str)).map(|value| value / 100.0),
        volume_shares: parse_f64(fields.get(12).map(String::as_str)).and_then(number_to_u64),
        turnover_hkd: parse_f64(fields.get(11).map(String::as_str)).filter(|value| *value >= 0.0),
        pe: None,
        pb: None,
        dividend_yield_percent: None,
        market_cap_hkd: None,
        total_shares: None,
        circulating_shares: None,
        currency: "HKD".to_owned(),
        quote_time,
        source: "Sina Finance".to_owned(),
        fetched_at: None,
        last_error: None,
    })
}

fn name_zh_candidate(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty() && name.chars().any(is_han_character)).then(|| name.to_owned())
}

fn parse_sina_csv_fields(value: &str) -> Result<Vec<String>, String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut chars = value.chars().peekable();
    let mut in_quotes = false;
    while let Some(ch) = chars.next() {
        match ch {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                current.push('"');
                chars.next();
            }
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => fields.push(std::mem::take(&mut current)),
            _ => current.push(ch),
        }
    }
    if in_quotes {
        return Err("Sina HK quote contains an unterminated CSV field".to_owned());
    }
    fields.push(current);
    Ok(fields)
}

fn choose_hk_quote(
    tencent: Result<HkQuote, String>,
    sina: Result<HkQuote, String>,
    expected_currency: &CurrencyCode,
) -> Result<HkQuote, String> {
    match tencent {
        Ok(quote)
            if normalize_hk_currency(&quote.currency).as_deref()
                == Some(expected_currency.as_str()) =>
        {
            Ok(quote)
        }
        tencent_result => {
            let tencent_error = match tencent_result {
                Ok(quote) => format!("Tencent quote currency mismatch: {}", quote.currency),
                Err(error) => error,
            };
            match sina {
                Ok(quote)
                    if normalize_hk_currency(&quote.currency).as_deref()
                        == Some(expected_currency.as_str()) =>
                {
                    Ok(quote)
                }
                Ok(quote) => Err(format!(
                    "Tencent quote failed ({tencent_error}); Sina quote currency mismatch: {}",
                    quote.currency
                )),
                Err(sina_error) => Err(format!(
                    "Tencent quote failed ({tencent_error}); Sina quote failed ({sina_error})"
                )),
            }
        }
    }
}

pub(crate) fn parse_tencent_hk_history(text: &str, code: &str) -> Result<Vec<HkDailyBar>, String> {
    let code = canonical_hk_code(code).ok_or_else(|| format!("invalid HK stock code: {code}"))?;
    let symbol = format!("hk{code}");
    let response = text.trim();
    let json = response
        .split_once('=')
        .map(|(_, payload)| payload.trim().trim_end_matches(';').trim())
        .filter(|payload| payload.starts_with('{'))
        .unwrap_or(response);
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|error| format!("decode Tencent HK history failed: {error}"))?;
    let rows = value
        .get("data")
        .and_then(|data| data.get(&symbol))
        .and_then(|stock| {
            stock
                .get("hfqday")
                .or_else(|| stock.get("qfqday"))
                .or_else(|| stock.get("day"))
        })
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

fn tencent_hk_daily_history_url(code: &str) -> Result<String, String> {
    tencent_hk_daily_history_url_with_adjustment(code, HkHistoryPriceAdjustment::Forward)
}

fn tencent_hk_daily_history_url_with_adjustment(
    code: &str,
    adjustment: HkHistoryPriceAdjustment,
) -> Result<String, String> {
    let canonical = canonical_hk_code(code)
        .ok_or_else(|| format!("invalid HK stock code for daily history: {code}"))?;
    let param = format!("hk{canonical},day,,,2000,{}", adjustment.query_value());
    let encoded = percent_encoding::utf8_percent_encode(&param, percent_encoding::NON_ALPHANUMERIC);
    Ok(format!("{TENCENT_HK_DAILY_KLINE_ENDPOINT}{encoded}"))
}

fn parse_tencent_hk_price_page(
    text: &str,
    code: &str,
    adjustment: HkHistoryPriceAdjustment,
) -> Result<HkHistoryPage, HkHistoryFetchError> {
    let raw = text
        .trim()
        .split_once('=')
        .map(|(_, raw)| raw)
        .unwrap_or(text)
        .trim()
        .trim_end_matches(';');
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|error| HkHistoryFetchError::Provider(error.to_string()))?;
    if value["code"].as_i64() != Some(0) {
        return Err(HkHistoryFetchError::Provider(format!(
            "{code} 港股日线数据源错误：{}",
            value["msg"].as_str().unwrap_or("无效响应")
        )));
    }
    let stock = value["data"][format!("hk{code}")]
        .as_object()
        .ok_or_else(|| HkHistoryFetchError::Provider(format!("{code} 港股日线响应缺少证券数据")))?;
    let quote = stock
        .get("qt")
        .and_then(|quotes| quotes.get(format!("hk{code}")))
        .and_then(serde_json::Value::as_array);
    let suspended_as_of = quote
        .filter(|fields| fields.get(40).and_then(serde_json::Value::as_str) == Some("S"))
        .and_then(|fields| fields.get(30))
        .and_then(serde_json::Value::as_str)
        .and_then(crate::market_domain::dates::compact_date_key);
    let requested_key = format!("{}day", adjustment.query_value());
    // Tencent uses `day` when the requested adjustment has no corporate actions to apply.
    let Some(rows) = stock
        .get(&requested_key)
        .or_else(|| stock.get("day"))
        .and_then(serde_json::Value::as_array)
    else {
        if ["day", "qfqday", "hfqday"]
            .iter()
            .all(|key| !stock.contains_key(*key))
        {
            return Ok(HkHistoryPage {
                bars: Vec::new(),
                suspended_as_of,
            });
        }
        return Err(HkHistoryFetchError::Provider(format!(
            "{code} 数据源没有返回所请求的 {} 复权日线",
            adjustment.query_value()
        )));
    };
    if rows.is_empty() {
        return Ok(HkHistoryPage {
            bars: Vec::new(),
            suspended_as_of,
        });
    }
    let isolated = serde_json::json!({"data":{format!("hk{code}"):{"day":rows}}});
    let bars = parse_tencent_hk_history(&isolated.to_string(), code).map_err(|_| {
        HkHistoryFetchError::InvalidPrices(format!(
            "{code} {} 日线包含非正或无效价格，所选区间未补齐",
            adjustment.query_value()
        ))
    })?;
    if bars.len() != rows.len() {
        return Err(HkHistoryFetchError::InvalidPrices(format!(
            "{code} {} 日线含无效或重复价格，所选区间未补齐",
            adjustment.query_value()
        )));
    }
    Ok(HkHistoryPage {
        bars,
        suspended_as_of,
    })
}

async fn collect_tencent_hk_history_range<F, Fut>(
    code: &str,
    start: chrono::NaiveDate,
    mut cursor: chrono::NaiveDate,
    mut fetch_page: F,
) -> Result<HkHistoryPage, HkHistoryFetchError>
where
    F: FnMut(chrono::NaiveDate) -> Fut,
    Fut: std::future::Future<Output = Result<HkHistoryPage, HkHistoryFetchError>>,
{
    let mut merged = std::collections::BTreeMap::new();
    let mut previous_first = None;
    let mut suspended_as_of = None;
    for page_index in 0..8 {
        let fetched = fetch_page(cursor).await?;
        if page_index == 0 {
            suspended_as_of = fetched.suspended_as_of;
        }
        let page = fetched.bars;
        if page.is_empty() {
            return if merged.is_empty() {
                Err(HkHistoryFetchError::Provider(format!(
                    "{code} 在所选区间没有可用港股日线"
                )))
            } else {
                Ok(HkHistoryPage {
                    bars: merged.into_values().collect(),
                    suspended_as_of,
                })
            };
        }
        let first = chrono::NaiveDate::parse_from_str(&page[0].date, "%Y-%m-%d")
            .map_err(|error| HkHistoryFetchError::Provider(error.to_string()))?;
        if previous_first == Some(first) && first == cursor && page.len() == 1 {
            if merged
                .get(&page[0].date)
                .is_some_and(|bar| !hk_history_prices_equal(bar, &page[0]))
            {
                return Err(HkHistoryFetchError::InvalidPrices(format!(
                    "{code} 分页期间复权价格版本发生变化，请重新补齐区间"
                )));
            }
            cursor = first.pred_opt().ok_or_else(|| {
                HkHistoryFetchError::Provider("港股日线日期超出可查询范围".to_owned())
            })?;
            continue;
        }
        if first > cursor || previous_first.is_some_and(|previous| first >= previous) {
            return Err(HkHistoryFetchError::Provider(format!(
                "{code} 日线分页未向前推进，无法补齐所选区间"
            )));
        }
        for bar in page {
            let day = chrono::NaiveDate::parse_from_str(&bar.date, "%Y-%m-%d")
                .map_err(|error| HkHistoryFetchError::Provider(error.to_string()))?;
            if day <= cursor {
                if merged
                    .get(&bar.date)
                    .is_some_and(|previous| !hk_history_prices_equal(previous, &bar))
                {
                    return Err(HkHistoryFetchError::InvalidPrices(format!(
                        "{code} 分页期间复权价格版本发生变化，请重新补齐区间"
                    )));
                }
                merged.insert(bar.date.clone(), bar);
            }
        }
        if first <= start {
            return Ok(HkHistoryPage {
                bars: merged.into_values().collect(),
                suspended_as_of,
            });
        }
        previous_first = Some(first);
        cursor = first;
    }
    Err(HkHistoryFetchError::Provider(format!(
        "{code} 日线分页达到上限，所选区间未补齐，请缩短回测日期"
    )))
}

async fn fetch_tencent_hk_adjusted_range(
    client: &reqwest::Client,
    code: &str,
    start: chrono::NaiveDate,
    end: chrono::NaiveDate,
    adjustment: HkHistoryPriceAdjustment,
) -> Result<HkHistoryPage, HkHistoryFetchError> {
    let canonical = code.to_owned();
    collect_tencent_hk_history_range(&canonical, start, end, |cursor| {
        let canonical = canonical.clone();
        let client = client.clone();
        async move {
            let param = format!(
                "hk{canonical},day,,{},640,{}",
                cursor.format("%Y-%m-%d"),
                adjustment.query_value()
            );
            let encoded =
                percent_encoding::utf8_percent_encode(&param, percent_encoding::NON_ALPHANUMERIC);
            let response = client
                .get(format!("{TENCENT_HK_DAILY_KLINE_ENDPOINT}{encoded}"))
                .header(reqwest::header::REFERER, "https://gu.qq.com/")
                .timeout(std::time::Duration::from_secs(HK_REQUEST_TIMEOUT_SECS))
                .send()
                .await
                .map_err(|error| {
                    HkHistoryFetchError::Provider(format!("{canonical} 港股日线请求失败：{error}"))
                })?;
            let bytes =
                crate::external_data::provider::read_bounded_response(response, 2 * 1024 * 1024)
                    .await
                    .map_err(|error| {
                        HkHistoryFetchError::Provider(format!(
                            "{canonical} 港股日线响应失败：{}",
                            error.message()
                        ))
                    })?;
            let text = String::from_utf8_lossy(&bytes);
            parse_tencent_hk_price_page(&text, &canonical, adjustment)
        }
    })
    .await
}

pub(crate) async fn fetch_tencent_hk_history_range(
    client: &reqwest::Client,
    code: &str,
    start: &str,
    end: &str,
    preferred_adjustment: HkHistoryPriceAdjustment,
) -> Result<HkFetchedHistory, String> {
    let canonical = canonical_hk_code(code).ok_or_else(|| "无效港股代码".to_owned())?;
    let parse_date = |raw: &str| {
        let compact = crate::market_domain::dates::compact_date_key(raw)
            .ok_or_else(|| format!("无效日线日期：{raw}"))?;
        chrono::NaiveDate::parse_from_str(&compact, "%Y%m%d").map_err(|error| error.to_string())
    };
    let start = parse_date(start)?;
    let end = parse_date(end)?;
    if start > end {
        return Err("日线开始日期不能晚于结束日期".to_owned());
    }
    fetch_valid_hk_adjustment_range(preferred_adjustment, |adjustment| {
        fetch_tencent_hk_adjusted_range(client, &canonical, start, end, adjustment)
    })
    .await
}

async fn fetch_valid_hk_adjustment_range<F, Fut>(
    preferred_adjustment: HkHistoryPriceAdjustment,
    fetcher: F,
) -> Result<HkFetchedHistory, String>
where
    F: Fn(HkHistoryPriceAdjustment) -> Fut,
    Fut: std::future::Future<Output = Result<HkHistoryPage, HkHistoryFetchError>>,
{
    match fetcher(preferred_adjustment).await {
        Ok(page) => Ok(HkFetchedHistory {
            bars: page.bars,
            price_adjustment: preferred_adjustment,
            suspended_as_of: page.suspended_as_of,
        }),
        Err(HkHistoryFetchError::InvalidPrices(primary)) => {
            let alternate = preferred_adjustment.alternate();
            let page = fetcher(alternate)
                .await
                .map_err(|error| format!("{primary}；另一复权口径也不可用：{error}"))?;
            Ok(HkFetchedHistory {
                bars: page.bars,
                price_adjustment: alternate,
                suspended_as_of: page.suspended_as_of,
            })
        }
        Err(error) => Err(error.to_string()),
    }
}

struct TencentHkMarketProvider {
    client: reqwest::Client,
    capabilities: Vec<crate::external_data::provider::ProviderCapability>,
    expected_currencies: HashMap<String, CurrencyCode>,
    history_adjustments: HashMap<String, HkHistoryPriceAdjustment>,
}

impl TencentHkMarketProvider {
    fn new(client: reqwest::Client, securities: &[HkSecurity]) -> Self {
        use crate::external_data::provider::ProviderCapability;
        use crate::external_data::{DataDomain, Market};
        Self {
            client,
            history_adjustments: HashMap::new(),
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

    fn with_history_adjustments(mut self, histories: &HashMap<String, HkHistoryRecord>) -> Self {
        self.history_adjustments = histories
            .iter()
            .map(|(code, history)| (code.clone(), history.price_adjustment))
            .collect();
        self
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

async fn fetch_sina_hk_quote(
    client: &reqwest::Client,
    code: &str,
    expected_currency: &CurrencyCode,
    timeout: std::time::Duration,
    max_bytes: usize,
) -> Result<(HkQuote, usize), String> {
    let canonical =
        canonical_hk_code(code).ok_or_else(|| format!("invalid HK stock code: {code}"))?;
    let response = client
        .get(format!("{SINA_HK_QUOTE_ENDPOINT}{canonical}"))
        .header(reqwest::header::REFERER, "https://finance.sina.com.cn/")
        .header(
            reqwest::header::USER_AGENT,
            "Mozilla/5.0 GuXuanYou HK quote",
        )
        .timeout(timeout)
        .send()
        .await
        .map_err(|error| format!("Sina HK quote request failed: {error}"))?;
    let body = crate::external_data::provider::read_bounded_response(response, max_bytes)
        .await
        .map_err(|error| {
            format!(
                "Sina HK quote response failed: {}: {}",
                error.category(),
                error.message()
            )
        })?;
    let (text, _, had_errors) = encoding_rs::GB18030.decode(&body);
    if had_errors {
        return Err("Sina HK quote response is not valid GB18030 text".to_owned());
    }
    let quote = parse_sina_hk_quote(&text, &format!("{canonical}.HK"), expected_currency)?;
    Ok((quote, body.len()))
}

fn tencent_hk_quote_batch_url(codes: &[String]) -> Result<String, String> {
    let symbols = codes
        .iter()
        .map(|code| {
            canonical_hk_quote_batch_code(code)
                .map(|code| format!("hk{code}"))
                .ok_or_else(|| format!("invalid HK quote batch code: {code}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if symbols.is_empty() {
        return Err("HK quote batch cannot be empty".to_owned());
    }
    Ok(format!("{TENCENT_HK_QUOTE_ENDPOINT}{}", symbols.join(",")))
}

fn sina_hk_quote_batch_url(codes: &[String]) -> Result<String, String> {
    let symbols = codes
        .iter()
        .map(|code| {
            canonical_hk_quote_batch_code(code)
                .map(|code| format!("rt_hk{code}"))
                .ok_or_else(|| format!("invalid HK Sina quote batch code: {code}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if symbols.is_empty() {
        return Err("HK Sina quote batch cannot be empty".to_owned());
    }
    Ok(format!("https://hq.sinajs.cn/list={}", symbols.join(",")))
}

fn decode_tencent_hk_quote_response(body: &[u8]) -> Result<String, String> {
    if let Ok(text) = std::str::from_utf8(body) {
        return Ok(text.to_owned());
    }
    let (text, _, had_errors) = encoding_rs::GB18030.decode(body);
    if had_errors {
        return Err("Tencent HK quote response is not valid UTF-8 or GB18030 text".to_owned());
    }
    Ok(text.into_owned())
}

async fn fetch_tencent_hk_quote_batch_text(
    client: &reqwest::Client,
    codes: &[String],
    timeout: std::time::Duration,
) -> Result<(String, usize), String> {
    let url = tencent_hk_quote_batch_url(codes)?;
    let response = client
        .get(url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|error| format!("Tencent HK batch quote request failed: {error}"))?;
    let body = crate::external_data::provider::read_bounded_response(
        response,
        HK_QUOTE_BATCH_RESPONSE_LIMIT_BYTES,
    )
    .await
    .map_err(|error| {
        format!(
            "Tencent HK batch quote response failed: {}: {}",
            error.category(),
            error.message()
        )
    })?;
    let text = decode_tencent_hk_quote_response(&body)?;
    Ok((text, body.len()))
}

async fn fetch_sina_hk_quote_batch_text(
    client: &reqwest::Client,
    codes: &[String],
    timeout: std::time::Duration,
) -> Result<(String, usize), String> {
    let url = sina_hk_quote_batch_url(codes)?;
    let response = client
        .get(url)
        .header(reqwest::header::REFERER, "https://finance.sina.com.cn/")
        .header(
            reqwest::header::USER_AGENT,
            "Mozilla/5.0 GuXuanYou HK batch quote",
        )
        .timeout(timeout)
        .send()
        .await
        .map_err(|error| format!("Sina HK batch quote request failed: {error}"))?;
    let body = crate::external_data::provider::read_bounded_response(
        response,
        HK_QUOTE_BATCH_RESPONSE_LIMIT_BYTES,
    )
    .await
    .map_err(|error| {
        format!(
            "Sina HK batch quote response failed: {}: {}",
            error.category(),
            error.message()
        )
    })?;
    let (text, _, had_errors) = encoding_rs::GB18030.decode(&body);
    if had_errors {
        return Err("Sina HK batch quote response is not valid GB18030 text".to_owned());
    }
    Ok((text.into_owned(), body.len()))
}

struct HkQuoteRefreshOutcome {
    requested_count: usize,
    unsupported_currency_count: usize,
    quotes: Vec<HkQuote>,
    errors: HashMap<String, String>,
    response_bytes: usize,
}

async fn fetch_hk_quote_refresh(
    client: &reqwest::Client,
    securities: &[HkSecurity],
    fetched_at: &str,
    timeout: std::time::Duration,
) -> HkQuoteRefreshOutcome {
    use futures::StreamExt;

    let supported = securities
        .iter()
        .filter_map(|security| {
            let currency = CurrencyCode::new(&security.currency).ok()?;
            tencent_hk_quote_supports_currency(&currency).then_some((security.clone(), currency))
        })
        .collect::<Vec<_>>();
    let mut outcome = HkQuoteRefreshOutcome {
        requested_count: securities.len(),
        unsupported_currency_count: securities.len().saturating_sub(supported.len()),
        quotes: Vec::new(),
        errors: HashMap::new(),
        response_bytes: 0,
    };
    let batches = supported
        .chunks(HK_QUOTE_BATCH_SIZE)
        .map(|chunk| chunk.to_vec())
        .collect::<Vec<_>>();
    let client = client.clone();
    let outcomes = futures::stream::iter(batches.into_iter().map(|chunk| {
        let client = client.clone();
        let fetched_at = fetched_at.to_owned();
        async move { fetch_hk_quote_batch_chunk(&client, &chunk, &fetched_at, timeout).await }
    }))
    .buffer_unordered(4)
    .collect::<Vec<_>>()
    .await;
    for batch in outcomes {
        outcome.response_bytes = outcome.response_bytes.saturating_add(batch.response_bytes);
        outcome.quotes.extend(batch.quotes);
        outcome.errors.extend(batch.errors);
    }
    outcome
}

async fn fetch_hk_quote_batch_chunk(
    client: &reqwest::Client,
    chunk: &[(HkSecurity, CurrencyCode)],
    fetched_at: &str,
    timeout: std::time::Duration,
) -> HkQuoteRefreshOutcome {
    let codes = chunk
        .iter()
        .map(|(security, _)| security.code.clone())
        .collect::<Vec<_>>();
    let tencent_response = fetch_tencent_hk_quote_batch_text(client, &codes, timeout).await;
    let mut response_bytes = tencent_response
        .as_ref()
        .map(|(_, bytes)| *bytes)
        .unwrap_or_default();
    let tencent_text = tencent_response
        .as_ref()
        .ok()
        .map(|(text, _)| text.as_str());
    let mut needs_sina = Vec::new();
    let mut quotes = Vec::new();

    for (security, expected_currency) in chunk {
        let quote = tencent_text
            .ok_or_else(|| {
                tencent_response
                    .as_ref()
                    .err()
                    .cloned()
                    .unwrap_or_else(|| "Tencent returned no batch quote response".to_owned())
            })
            .and_then(|text| {
                parse_tencent_hk_quote(text, &format!("{}.HK", security.code), expected_currency)
            });
        match quote {
            Ok(mut quote) => {
                quote.fetched_at = Some(fetched_at.to_owned());
                if hk_quote_needs_sina_name_enrichment(&quote) {
                    needs_sina.push((security.code.clone(), expected_currency.clone(), None));
                }
                quotes.push(quote);
            }
            Err(error) => needs_sina.push((
                security.code.clone(),
                expected_currency.clone(),
                Some(error),
            )),
        }
    }

    let mut errors = HashMap::new();
    if !needs_sina.is_empty() {
        let missing_codes = needs_sina
            .iter()
            .map(|(code, _, _)| code.clone())
            .collect::<Vec<_>>();
        let sina_response = fetch_sina_hk_quote_batch_text(client, &missing_codes, timeout).await;
        if let Ok((_, bytes)) = &sina_response {
            response_bytes = response_bytes.saturating_add(*bytes);
        }
        for (code, expected_currency, tencent_error) in needs_sina {
            let quote = sina_response
                .as_ref()
                .map_err(|error| error.clone())
                .and_then(|(text, _)| {
                    parse_sina_hk_quote(text, &format!("{code}.HK"), &expected_currency)
                });
            match (tencent_error, quote) {
                (Some(tencent_error), Ok(sina_quote)) => {
                    match choose_hk_quote(Err(tencent_error), Ok(sina_quote), &expected_currency) {
                        Ok(mut quote) => {
                            quote.fetched_at = Some(fetched_at.to_owned());
                            quotes.push(quote);
                        }
                        Err(error) => {
                            errors.insert(code, error);
                        }
                    }
                }
                (Some(tencent_error), Err(sina_error)) => {
                    errors.insert(
                        code,
                        format!("Tencent quote failed ({tencent_error}); Sina quote failed ({sina_error})"),
                    );
                }
                (None, Ok(sina_quote)) => {
                    if let Some(primary) = quotes.iter_mut().find(|quote| quote.code == code) {
                        enrich_hk_quote_chinese_name(primary, &sina_quote);
                    }
                }
                (None, Err(_)) => {}
            }
        }
    }
    HkQuoteRefreshOutcome {
        requested_count: chunk.len(),
        unsupported_currency_count: 0,
        quotes,
        errors,
        response_bytes,
    }
}

#[derive(Debug, Default, Serialize)]
struct HkQuoteRefreshSummary {
    requested_count: usize,
    attempted_count: usize,
    updated_count: usize,
    failed_count: usize,
    unsupported_currency_count: usize,
    response_bytes: usize,
    error_samples: Vec<String>,
}

impl HkQuoteRefreshSummary {
    fn is_stale(&self) -> bool {
        self.attempted_count == 0
            || self.failed_count > 0
            || self.updated_count < self.attempted_count
    }
}

fn merge_hk_quote_refresh(
    cache: &mut HkMarketCache,
    outcome: HkQuoteRefreshOutcome,
) -> HkQuoteRefreshSummary {
    let mut summary = HkQuoteRefreshSummary {
        requested_count: outcome.requested_count,
        attempted_count: outcome
            .requested_count
            .saturating_sub(outcome.unsupported_currency_count),
        failed_count: outcome.errors.len(),
        unsupported_currency_count: outcome.unsupported_currency_count,
        response_bytes: outcome.response_bytes,
        error_samples: outcome
            .errors
            .iter()
            .take(3)
            .map(|(code, error)| format!("{code}: {error}"))
            .collect(),
        ..HkQuoteRefreshSummary::default()
    };
    for mut quote in outcome.quotes {
        let code = quote.code.clone();
        let Some(security) = cache.securities.iter().find(|item| item.code == code) else {
            continue;
        };
        if !security.cached_quote_matches_security_master(&quote)
            || !quote.price.is_finite()
            || quote.price <= 0.0
        {
            continue;
        }
        preserve_hk_chinese_name(cache.quotes.get(&code), &mut quote);
        quote.last_error = None;
        cache.quotes.insert(code.clone(), quote);
        cache.quote_errors.remove(&code);
        summary.updated_count += 1;
    }
    for (code, error) in outcome.errors {
        if let Some(quote) = cache.quotes.get_mut(&code) {
            quote.last_error = Some(error.clone());
        }
        cache.quote_errors.insert(code, error);
    }
    summary
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
                        let tencent_quote = async {
                            let url = format!("{TENCENT_HK_QUOTE_ENDPOINT}hk{canonical}");
                            let response = self
                                .client
                                .get(url)
                                .timeout(context.timeout)
                                .send()
                                .await
                                .map_err(|error| format!("Tencent request failed: {error}"))?;
                            let body = crate::external_data::provider::read_bounded_response(
                                response,
                                context.max_response_bytes.saturating_sub(response_bytes),
                            )
                            .await
                            .map_err(|error| {
                                format!(
                                    "Tencent response failed: {}: {}",
                                    error.category(),
                                    error.message()
                                )
                            })?;
                            let bytes = body.len();
                            let text = decode_tencent_hk_quote_response(&body)?;
                            let quote = parse_tencent_hk_quote(
                                &text,
                                &format!("{canonical}.HK"),
                                expected_currency,
                            )?;
                            Ok::<_, String>((quote, bytes))
                        }
                        .await;
                        let (mut quote, mut quote_bytes) = match tencent_quote {
                            Ok((quote, bytes)) => (quote, bytes),
                            Err(tencent_error) => {
                                let (quote, bytes) = fetch_sina_hk_quote(
                                    &self.client,
                                    &canonical,
                                    expected_currency,
                                    context.timeout,
                                    context.max_response_bytes.saturating_sub(response_bytes),
                                )
                                .await
                                .map_err(|sina_error| {
                                    ProviderError::invalid_payload(format!(
                                        "Tencent quote failed ({tencent_error}); Sina quote failed ({sina_error})"
                                    ))
                                })?;
                                let quote = choose_hk_quote(
                                    Err(tencent_error),
                                    Ok(quote),
                                    expected_currency,
                                )
                                .map_err(ProviderError::invalid_payload)?;
                                (quote, bytes)
                            }
                        };
                        let name_bytes =
                            enrich_tencent_hk_quote_name_from_sina(&mut quote, || async {
                                fetch_sina_hk_quote(
                                    &self.client,
                                    &canonical,
                                    expected_currency,
                                    context.timeout,
                                    context
                                        .max_response_bytes
                                        .saturating_sub(response_bytes.saturating_add(quote_bytes)),
                                )
                                .await
                            })
                            .await;
                        quote_bytes = quote_bytes.saturating_add(name_bytes);
                        response_bytes = response_bytes.saturating_add(quote_bytes);
                        if quote.source.is_empty() {
                            quote.source = "Tencent Finance".into();
                        }
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
                        let adjustment = self
                            .history_adjustments
                            .get(&canonical)
                            .copied()
                            .unwrap_or_default();
                        let url =
                            tencent_hk_daily_history_url_with_adjustment(&canonical, adjustment)
                                .map_err(ProviderError::quality)?;
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
                        let page = parse_tencent_hk_price_page(&text, &canonical, adjustment)
                            .map_err(|error| ProviderError::invalid_payload(error.to_string()))?;
                        let bars = page.bars;
                        let request_range =
                            bars.first().zip(bars.last()).and_then(|(first, last)| {
                                Some(HkHistoryRequestRange {
                                    start_date: crate::market_domain::dates::compact_date_key(
                                        &first.date,
                                    )?,
                                    end_date: crate::market_domain::dates::compact_date_key(
                                        &last.date,
                                    )?
                                    .max(page.suspended_as_of.clone().unwrap_or_default()),
                                })
                            });
                        let history = HkHistoryRecord {
                            bars,
                            request_range,
                            request_ranges: Vec::new(),
                            price_adjustment: adjustment,
                            bar_fetched_at: HashMap::new(),
                            revision_reason: None,
                            suspended_as_of: page.suspended_as_of,
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
        Ok(mut batch) => {
            if request.domain == crate::external_data::DataDomain::SecurityMaster
                && request.market == crate::external_data::Market::HongKong
            {
                let previous = store
                    .read_records(
                        crate::external_data::DataDomain::SecurityMaster,
                        crate::external_data::Market::HongKong,
                        "hk",
                    )?
                    .into_iter()
                    .find(|record| record.record_key == "directory")
                    .and_then(|record| {
                        serde_json::from_value::<Vec<HkSecurity>>(record.payload["items"].clone())
                            .ok()
                    })
                    .unwrap_or_default();
                for record in &mut batch.records {
                    if record.record_key != "directory" {
                        continue;
                    }
                    let mut refreshed =
                        serde_json::from_value::<Vec<HkSecurity>>(record.payload["items"].clone())
                            .map_err(|error| {
                                format!(
                            "decode refreshed HK directory for local classification merge: {error}"
                        )
                            })?;
                    preserve_local_hk_classifications(&previous, &mut refreshed);
                    record.payload["items"] = serde_json::to_value(refreshed)
                        .map_err(|error| format!("encode refreshed HK directory: {error}"))?;
                }
            }
            {
                let raw_records = batch.records.clone();
                let _guard = HK_CACHE_WRITE_LOCK
                    .get_or_init(|| std::sync::Mutex::new(()))
                    .lock()
                    .map_err(|_| "HK cache write lock is unavailable".to_string())?;
                merge_hk_history_batch(&store, &mut batch.records)?;
                store.upsert_batch(&batch.records)?;
                batch.records = raw_records;
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

fn merge_hk_history_batch(
    store: &crate::external_data::DataStore,
    records: &mut [crate::external_data::StoredRecord],
) -> Result<(), String> {
    use crate::external_data::{DataDomain, Market};
    for record in records {
        if record.domain != DataDomain::DailyHistory
            || record.market != Market::HongKong
            || record.record_key != "history"
        {
            continue;
        }
        let previous = store
            .read_records(record.domain, record.market, &record.entity_key)?
            .into_iter()
            .find(|record| record.record_key == "history")
            .map(|record| serde_json::from_value::<HkHistoryRecord>(record.payload))
            .transpose()
            .map_err(|error| error.to_string())?;
        let incoming = serde_json::from_value::<HkHistoryRecord>(record.payload.clone())
            .map_err(|error| error.to_string())?;
        let merged = merge_hk_history_record(previous.as_ref(), incoming);
        record.metadata.fetched_at = merged.fetched_at.clone();
        record.metadata.observed_at = merged.bars.last().map(|bar| bar.date.clone());
        record.metadata.payload_hash = None;
        record.payload = serde_json::to_value(merged).map_err(|error| error.to_string())?;
    }
    Ok(())
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
        let client = match crate::network::http::build_tencent_http_client(
            "Mozilla/5.0 GuXuanYou HK recovery",
            timeout,
        ) {
            Ok(client) => client,
            Err(error) => {
                failed.push(serde_json::json!({"run_id": run.run_id, "error": error}));
                continue;
            }
        };
        let provider = TencentHkMarketProvider::new(client, &securities).with_history_adjustments(
            &read_hk_market_cache(&hk_market_data_path(&app)?)?.histories,
        );
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
    let client = crate::network::http::build_http_client_with_proxy(
        "Mozilla/5.0 GuXuanYou HK explicit refresh",
        timeout,
        Some(&payload),
    )?;
    let securities = read_hk_market_cache(&hk_market_data_path(&app)?)?.securities;
    let provider = TencentHkMarketProvider::new(client, &securities)
        .with_history_adjustments(&read_hk_market_cache(&hk_market_data_path(&app)?)?.histories);
    let mut context =
        crate::external_data::provider::ProviderContext::new(&raw_request_id, timeout);
    context.network_policy = crate::external_data::provider::ProviderNetworkPolicy {
        proxy_mode: crate::network::proxy::proxy_mode_from_payload(Some(&payload)),
        proxy_url: crate::network::proxy::proxy_from_payload(Some(&payload)),
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

pub(crate) fn canonical_hk_code(value: &str) -> Option<String> {
    let value = value.trim().to_ascii_uppercase();
    let digits = value
        .strip_suffix(".HK")
        .or_else(|| value.strip_prefix("HK"))
        .unwrap_or(value.as_str());
    if digits.is_empty() || digits.len() > 5 || !digits.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    Some(format!("{digits:0>5}"))
}

fn canonical_hk_quote_batch_code(value: &str) -> Option<String> {
    canonical_hk_code(value).or_else(|| {
        let code = normalize_hkex_code(value);
        (!code.is_empty()).then_some(code)
    })
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
            classification_status: HkClassificationStatus::ExchangeListedEquity,
        });
    }

    if securities.is_empty() {
        return Err("HKEX securities workbook contains no supported equity securities".to_string());
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
            normalize_cached_hk_classifications(&mut cache.securities);
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
        current.quarterly_updates = legacy.quarterly_updates;
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
    for (code, record) in &cache.quarterly_updates {
        grouped
            .entry((DataDomain::Disclosure, Market::HongKong))
            .or_default()
            .push(StoredRecord::new(
                DataDomain::Disclosure,
                Market::HongKong,
                code,
                "quarterly_update",
                "HKEXnews",
                "official",
                &record.checked_at,
                None,
                record
                    .update
                    .as_ref()
                    .map(|update| update.disclosed_at.as_str()),
                if record.error.is_some() {
                    "unavailable"
                } else if record.update.is_some() {
                    "unverified"
                } else {
                    "no_update_found"
                },
                vec![],
                serde_json::to_value(record).unwrap_or(Value::Null),
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
            "quarterly_update" => {
                let update: HkQuarterlyUpdateRecord = serde_json::from_value(record.payload)
                    .map_err(|error| format!("decode HK quarterly update: {error}"))?;
                cache.quarterly_updates.insert(record.entity_key, update);
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
    normalize_cached_hk_classifications(&mut cache.securities);
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

#[cfg(test)]
#[path = "hk_market/api/tests/market_status_tests.rs"]
mod market_status_tests;

#[cfg(test)]
#[path = "hk_market/api/tests/sina_financial_boundary_tests.rs"]
mod sina_financial_boundary_tests;

#[cfg(test)]
#[path = "hk_market/api/tests/currency_contract_tests.rs"]
mod currency_contract_tests;

pub(crate) mod api;
// Retain the existing crate-level HK entrypoints for source compatibility.
#[allow(unused_imports)]
pub(crate) use api::{
    api_hk_confirm_ordinary_share, api_hk_financial_refresh, api_hk_financial_status,
    api_hk_market_refresh, api_hk_market_status, api_hk_network_probe, api_hk_observe,
    api_hk_stock_search, record_hkex_refresh_error,
};
#[cfg(test)]
use api::{hk_market_status_value, probe_hk_source};
