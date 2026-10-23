use crate::market_domain::{CapabilityId, CapabilityStatus, CoverageSummary, CurrencyCode, MarketCapability, MarketId};
use calamine::{open_workbook_from_rs, Data, Reader, Xlsx};
use serde::{Deserialize, Serialize};
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
const TENCENT_HK_DAILY_KLINE_ENDPOINT: &str =
    "https://web.ifzq.gtimg.cn/appstock/app/fqkline/get";
const HKEX_MAX_WORKBOOK_BYTES: usize = 16 * 1024 * 1024;
const HK_REQUEST_TIMEOUT_SECS: u64 = 8;
const TENCENT_HK_SUPPORTED_QUOTE_CURRENCIES: [&str; 1] = ["HKD"];

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct HkSecurity {
    pub code: String,
    pub name: String,
    pub currency: String,
    pub board_lot: Option<u32>,
    pub security_type: String,
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
        return HkObservationMergeResult { notes: vec!["invalid HK stock code".to_string()], ..Default::default() };
    };
    let mut result = HkObservationMergeResult::default();
    match quote {
        Ok(mut value) => {
            value.code = code.clone();
            value.fetched_at = Some(fetched_at.to_string());
            value.last_error = None;
            cache.quotes.insert(code.clone(), value);
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
            cache.histories.insert(code.clone(), HkHistoryRecord {
                bars,
                source: "Tencent Finance".to_string(),
                fetched_at: fetched_at.to_string(),
                last_error: None,
            });
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

fn quote_capability_for_security(
    security: &HkSecurity,
    quote_currency: Option<&str>,
    quote_error: Option<&str>,
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
    let line = text.split(';').find(|line| line.trim_start().starts_with(&format!("v_{symbol}=")))
        .ok_or_else(|| format!("Tencent returned no quote for {code}"))?;
    let fields = line.split_once('=')
        .and_then(|(_, value)| value.trim().strip_prefix('"')?.strip_suffix('"'))
        .unwrap_or_default().split('~').collect::<Vec<_>>();
    if fields.len() < 76 { return Err(format!("Tencent HK quote payload is incomplete for {code}")); }
    let name = fields[1].trim();
    let price = parse_f64(fields.get(3).copied()).filter(|value| *value > 0.0)
        .ok_or_else(|| format!("Tencent HK quote has no valid price for {code}"))?;
    let quote_time = fields.get(30).map(|value| value.trim().to_string()).filter(|value| !value.is_empty());
    if name.is_empty() { return Err(format!("Tencent HK quote has no valid name for {code}")); }
    let currency = fields.get(75).and_then(|value| normalize_hk_currency(value))
        .ok_or_else(|| format!("Tencent HK quote has no valid currency for {code}"))?;
    if currency != expected_currency.as_str() {
        return Err(format!("Tencent HK quote currency mismatch for {code}: expected {}, received {currency}", expected_currency.as_str()));
    }
    Ok(HkQuote {
        code, name: name.to_string(), price,
        change_pct: parse_f64(fields.get(33).copied()).map(|value| value / 100.0),
        volume_shares: parse_f64(fields.get(6).copied()).and_then(number_to_u64),
        turnover_hkd: parse_f64(fields.get(37).copied()).filter(|value| *value >= 0.0),
        currency, quote_time, source: "Tencent Finance".to_string(), fetched_at: None, last_error: None,
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
        let Some(fields) = row.as_array() else { continue };
        if fields.len() < 6 { continue; }
        let Some(date) = fields[0].as_str().filter(|value| value.len() >= 10) else { continue };
        let Some(open) = json_number(&fields[1]) else { continue };
        let Some(close) = json_number(&fields[2]) else { continue };
        let Some(high) = json_number(&fields[3]) else { continue };
        let Some(low) = json_number(&fields[4]) else { continue };
        if [open, high, low, close].iter().any(|value| !value.is_finite() || *value <= 0.0)
            || high < open.max(close) || low > open.min(close) || high < low { continue; }
        bars.push(HkDailyBar {
            date: date.chars().take(10).collect(),
            open, high, low, close,
            volume_shares: json_number(&fields[5]).and_then(number_to_u64),
        });
    }
    if bars.is_empty() { return Err(format!("Tencent returned no valid daily bars for {code}")); }
    bars.sort_by(|left, right| left.date.cmp(&right.date));
    bars.dedup_by(|left, right| left.date == right.date);
    Ok(bars)
}

pub(crate) async fn fetch_tencent_hk_quote(
    client: &reqwest::Client,
    code: &str,
    expected_currency: &CurrencyCode,
) -> Result<HkQuote, String> {
    if !tencent_hk_quote_supports_currency(expected_currency) {
        return Err(format!("Tencent HK quote adapter does not support {} for {code}", expected_currency.as_str()));
    }
    let canonical = canonical_hk_code(code).ok_or_else(|| format!("invalid HK stock code: {code}"))?;
    let url = format!("{TENCENT_HK_QUOTE_ENDPOINT}hk{canonical}");
    let text = client.get(url).timeout(std::time::Duration::from_secs(HK_REQUEST_TIMEOUT_SECS))
        .send().await.map_err(|error| format!("Tencent HK quote request failed: {error}"))?
        .error_for_status().map_err(|error| format!("Tencent HK quote HTTP error: {error}"))?
        .text().await.map_err(|error| format!("read Tencent HK quote failed: {error}"))?;
    parse_tencent_hk_quote(&text, &canonical, expected_currency)
}

pub(crate) async fn fetch_tencent_hk_history(client: &reqwest::Client, code: &str, start: &str, end: &str) -> Result<Vec<HkDailyBar>, String> {
    let canonical = canonical_hk_code(code).ok_or_else(|| format!("invalid HK stock code: {code}"))?;
    let start = compact_date(start).ok_or_else(|| "invalid HK history start date".to_string())?;
    let end = compact_date(end).ok_or_else(|| "invalid HK history end date".to_string())?;
    if start > end { return Err("HK history start date is after end date".to_string()); }
    let param = format!("hk{canonical},day,{start},{end},2000,");
    let encoded = percent_encoding::utf8_percent_encode(
        &param,
        percent_encoding::NON_ALPHANUMERIC,
    );
    let url = format!("{TENCENT_HK_DAILY_KLINE_ENDPOINT}?param={encoded}");
    let text = client.get(url).timeout(std::time::Duration::from_secs(HK_REQUEST_TIMEOUT_SECS))
        .send().await.map_err(|error| format!("Tencent HK history request failed: {error}"))?
        .error_for_status().map_err(|error| format!("Tencent HK history HTTP error: {error}"))?
        .text().await.map_err(|error| format!("read Tencent HK history failed: {error}"))?;
    parse_tencent_hk_history(&text, &canonical)
}

fn canonical_hk_code(value: &str) -> Option<String> {
    let value = value.trim().to_ascii_uppercase();
    let digits = value.strip_suffix(".HK").or_else(|| value.strip_prefix("HK"))?;
    if digits.is_empty() || digits.len() > 5 || !digits.chars().all(|ch| ch.is_ascii_digit()) { return None; }
    Some(format!("{digits:0>5}"))
}

fn parse_f64(value: Option<&str>) -> Option<f64> { value?.trim().parse::<f64>().ok().filter(|value| value.is_finite()) }
fn json_number(value: &serde_json::Value) -> Option<f64> { value.as_f64().or_else(|| value.as_str()?.parse().ok()).filter(|value: &f64| value.is_finite()) }
fn number_to_u64(value: f64) -> Option<u64> { (value >= 0.0 && value <= u64::MAX as f64).then_some(value.round() as u64) }
fn compact_date(value: &str) -> Option<String> {
    let digits = value.chars().filter(|ch| ch.is_ascii_digit()).collect::<String>();
    (digits.len() == 8).then_some(digits)
}

pub(crate) fn parse_hkex_security_master(bytes: &[u8]) -> Result<Vec<HkSecurity>, String> {
    if bytes.is_empty() || bytes.len() > HKEX_MAX_WORKBOOK_BYTES {
        return Err("HKEX securities workbook is empty or exceeds the 16 MiB limit".to_string());
    }

    let mut workbook: Xlsx<Cursor<&[u8]>> =
        open_workbook_from_rs(Cursor::new(bytes)).map_err(|error| {
            format!("open HKEX securities workbook failed: {error}")
        })?;
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
            let name = header_column(
                &columns,
                &["nameofsecurities", "securitiesname", "name"],
            )?;
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

pub(crate) fn write_hk_market_cache(path: &Path, cache: &HkMarketCache) -> Result<(), String> {
    if cache.schema_version != 1 {
        return Err(format!(
            "unsupported HK market cache schema version: {}",
            cache.schema_version
        ));
    }
    let bytes = serde_json::to_vec(cache)
        .map_err(|error| format!("encode HK market cache failed: {error}"))?;
    crate::durability::atomic_write_json(path, &bytes)
}

pub(crate) fn update_hkex_security_cache(
    path: &Path,
    workbook_bytes: &[u8],
    updated_at: &str,
) -> Result<HkMarketCache, String> {
    let securities = parse_hkex_security_master(workbook_bytes)?;
    let mut cache = read_hk_market_cache(path)?;
    cache.securities = securities;
    cache.securities_source = HKEX_SECURITIES_SOURCE.to_string();
    cache.securities_updated_at = Some(updated_at.to_string());
    cache.securities_last_attempt_at = Some(updated_at.to_string());
    cache.securities_last_error = None;
    write_hk_market_cache(path, &cache)?;
    Ok(cache)
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
        "stale": cache.securities.is_empty() || cache.securities_last_error.is_some() || !cache.quote_errors.is_empty() || !cache.history_errors.is_empty(),
    }))
}

#[tauri::command]
pub(crate) async fn api_hk_market_refresh(app: AppHandle) -> Result<serde_json::Value, String> {
    let path = hk_market_data_path(&app)?;
    let timeout = std::time::Duration::from_secs(HK_REQUEST_TIMEOUT_SECS);
    let client = crate::market::build_tencent_http_client("Mozilla/5.0 GuXuanYou HK market", timeout)?;
    let attempt_at = current_epoch_millis();
    let response = match client
        .get(HKEX_SECURITIES_SOURCE)
        .timeout(timeout)
        .send()
        .await {
        Ok(response) => match response.error_for_status() {
            Ok(response) => response,
            Err(error) => {
                let message = format!("HKEX securities HTTP error: {error}");
                record_hkex_refresh_error(&path, &attempt_at, &message)?;
                return Err(message);
            }
        },
        Err(error) => {
            let message = format!("HKEX securities request failed: {error}");
            record_hkex_refresh_error(&path, &attempt_at, &message)?;
            return Err(message);
        }
    };
    let bytes = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(error) => {
            let message = format!("read HKEX securities workbook failed: {error}");
            record_hkex_refresh_error(&path, &attempt_at, &message)?;
            return Err(message);
        }
    };
    if bytes.len() > HKEX_MAX_WORKBOOK_BYTES {
        let message = "HKEX securities workbook exceeds the 16 MiB limit".to_string();
        record_hkex_refresh_error(&path, &attempt_at, &message)?;
        return Err(message);
    }
    let cache = match update_hkex_security_cache(&path, &bytes, &attempt_at) {
        Ok(cache) => cache,
        Err(error) => {
            record_hkex_refresh_error(&path, &attempt_at, &error)?;
            return Err(error);
        }
    };
    Ok(serde_json::json!({
        "market": "hk",
        "security_count": cache.securities.len(),
        "securities_source": cache.securities_source,
        "securities_updated_at": cache.securities_updated_at,
        "stale": false,
    }))
}

pub(crate) fn record_hkex_refresh_error(path: &Path, attempt_at: &str, error: &str) -> Result<(), String> {
    let mut cache = read_hk_market_cache(path)?;
    cache.securities_last_attempt_at = Some(attempt_at.to_string());
    cache.securities_last_error = Some(error.to_string());
    write_hk_market_cache(path, &cache)
}

#[tauri::command]
pub(crate) fn api_hk_stock_search(app: AppHandle, payload: serde_json::Value) -> Result<serde_json::Value, String> {
    let cache = read_hk_market_cache(&hk_market_data_path(&app)?)?;
    let query = payload.get("query").and_then(serde_json::Value::as_str).unwrap_or("").trim().to_ascii_lowercase();
    let limit = payload.get("limit").and_then(serde_json::Value::as_u64).unwrap_or(20).clamp(1, 100) as usize;
    let items = cache.securities.into_iter().filter(|security| {
        query.is_empty() || security.code.contains(&query) || security.name.to_ascii_lowercase().contains(&query)
    }).take(limit).collect::<Vec<_>>();
    Ok(serde_json::json!({ "market": "hk", "items": items, "source": cache.securities_source, "updated_at": cache.securities_updated_at, "last_attempt_at": cache.securities_last_attempt_at, "last_error": cache.securities_last_error }))
}

#[tauri::command]
pub(crate) async fn api_hk_observe(app: AppHandle, payload: serde_json::Value) -> Result<serde_json::Value, String> {
    let path = hk_market_data_path(&app)?;
    let code = payload.get("code").and_then(serde_json::Value::as_str).and_then(canonical_hk_code)
        .ok_or_else(|| "请输入有效港股代码。".to_string())?;
    let policy = payload.get("data_policy").and_then(serde_json::Value::as_str).unwrap_or("cache_only");
    let mut cache = read_hk_market_cache(&path)?;
    let security = cache.securities.iter().find(|item| item.code == code).cloned()
        .ok_or_else(|| format!("港股证券列表中没有找到 {code}。"))?;
    let mut notes = Vec::new();
    let expected_currency = CurrencyCode::new(&security.currency)?;
    if policy == "refresh" || !cache.quotes.contains_key(&code) || !cache.histories.contains_key(&code) {
        let timeout = std::time::Duration::from_secs(HK_REQUEST_TIMEOUT_SECS);
        let client = crate::market::build_tencent_http_client("Mozilla/5.0 GuXuanYou HK observe", timeout)?;
        let fetched_at = current_epoch_millis();
        let result = merge_hk_observation_update(
            &mut cache,
            &code,
            fetch_tencent_hk_quote(&client, &code, &expected_currency).await,
            fetch_tencent_hk_history(&client, &code, "2020-01-01", "2050-12-31").await,
            &fetched_at,
        );
        notes.extend(result.notes);
        if result.quote_updated || result.history_updated || !notes.is_empty() {
            write_hk_market_cache(&path, &cache)?;
        }
    }
    let cached_quote = cache.quotes.get(&code).cloned();
    let quote = cached_quote.clone().filter(|value| value.currency == security.currency);
    let history = cache.histories.get(&code).cloned();
    let quote_capability = quote_capability_for_security(&security, cached_quote.as_ref().map(|value| value.currency.as_str()), cache.quote_errors.get(&code).map(String::as_str));
    if let Some(error) = cache.quote_errors.get(&code) {
        if !notes.iter().any(|note| note.contains(error)) { notes.push(format!("报价：{error}")); }
    }
    if let Some(error) = cache.history_errors.get(&code) {
        if !notes.iter().any(|note| note.contains(error)) { notes.push(format!("日 K：{error}")); }
    }
    let stale = quote.is_none() || history.is_none() || !notes.is_empty();
    Ok(serde_json::json!({
        "market": "hk",
        "security": security,
        "quote": quote,
        "quote_capability": quote_capability,
        "history": history.as_ref().map(|record| record.bars.clone()).unwrap_or_default(),
        "quote_updated_at": cache.quotes.get(&code).and_then(|value| value.fetched_at.clone()),
        "history_updated_at": history.as_ref().map(|record| record.fetched_at.clone()),
        "history_error": history.as_ref().and_then(|record| record.last_error.clone()).or_else(|| cache.history_errors.get(&code).cloned()),
        "quote_error": cache.quotes.get(&code).and_then(|value| value.last_error.clone()).or_else(|| cache.quote_errors.get(&code).cloned()),
        "notes": notes,
        "stale": stale,
    }))
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
    fn quote_capability_is_unavailable_for_an_unsupported_listing_currency() {
        let security = super::HkSecurity {
            code: "00012".to_owned(),
            name: "Dual Counter".to_owned(),
            currency: "CNY".to_owned(),
            board_lot: Some(100),
            security_type: "Equity".to_owned(),
        };

        let capability = super::quote_capability_for_security(&security, None, None);

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
        };

        let capability = super::quote_capability_for_security(&security, Some("HKD"), None);

        assert_eq!(
            capability.status,
            crate::market_domain::CapabilityStatus::Available
        );
        assert_eq!(capability.coverage.as_ref().unwrap().available_count, 1);
    }
}
