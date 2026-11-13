use calamine::{open_workbook_from_rs, Reader, Xlsx};
use futures::{stream, StreamExt};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::io::Cursor;
use std::time::Duration;

const FINANCIAL_SOURCE: &str = "eastmoney_cn_financial";
const COMPANY_PROFILE_TTL_MS: u128 = 7 * 24 * 60 * 60 * 1000;
const COMPANY_PROFILE_MAX_BYTES: u64 = 256 * 1024;

#[derive(Default)]
struct CompanyProfileCacheState {
    published: Option<PublishedCompanyProfile>,
}

struct PublishedCompanyProfile {
    generation: u64,
    profile: Value,
    cache_warning: Option<String>,
}

type CompanyProfileStateHandle = std::sync::Arc<std::sync::Mutex<CompanyProfileCacheState>>;
type CompanyProfileRequestRegistry = std::collections::HashMap<
    std::path::PathBuf,
    std::sync::Weak<std::sync::Mutex<CompanyProfileCacheState>>,
>;

fn company_profile_request(
    path: &std::path::Path,
) -> Result<(CompanyProfileStateHandle, u64), String> {
    use std::sync::{Arc, Mutex, OnceLock, Weak};
    static REQUESTS: OnceLock<Mutex<CompanyProfileRequestRegistry>> = OnceLock::new();
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let mut requests = REQUESTS
        .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
        .lock()
        .map_err(|_| "公司资料缓存状态暂不可用，请刷新重试。".to_owned())?;
    // Keep only in-flight state; completed stock lookups must not grow a permanent registry.
    requests.retain(|_, state| state.strong_count() > 0);
    let state = requests
        .get(path)
        .and_then(Weak::upgrade)
        .unwrap_or_else(|| {
            let state = Arc::new(Mutex::new(CompanyProfileCacheState::default()));
            requests.insert(path.to_path_buf(), Arc::downgrade(&state));
            state
        });
    let generation = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok((state, generation))
}

fn cn_company_profile_code(raw: &str) -> Result<String, String> {
    crate::market::normalize_stock_code(raw)
        .filter(|code| {
            code.split_once('.').is_some_and(|(digits, market)| {
                crate::market::valid_digits(digits) && matches!(market, "SH" | "SZ" | "BJ")
            })
        })
        .ok_or_else(|| "请输入有效 A 股代码。".to_owned())
}

fn cn_company_profile_text(value: &Value, field: &str) -> Option<String> {
    text(value, field).filter(|value| !matches!(value.as_str(), "--" | "—"))
}

fn valid_cn_company_profile(profile: &Value, code: &str) -> bool {
    profile.get("code").and_then(Value::as_str) == Some(code)
        && [
            "name",
            "industry",
            "business_summary",
            "company_summary",
            "source",
            "fetched_at",
        ]
        .iter()
        .all(|key| profile.get(key).is_some_and(Value::is_string))
        && cn_company_profile_text(profile, "name").is_some()
        && cn_company_profile_text(profile, "source").is_some()
        && ["business_summary", "company_summary"]
            .iter()
            .any(|key| cn_company_profile_text(profile, key).is_some())
        && text(profile, "fetched_at")
            .and_then(|value| value.parse::<u128>().ok())
            .is_some_and(|time| time > 0)
}

fn parse_cn_company_profile(
    payload: &Value,
    code: &str,
    fetched_at: &str,
) -> Result<Value, String> {
    let code = cn_company_profile_code(code)?;
    if payload.get("success").and_then(Value::as_bool) != Some(true) {
        return Err(format!(
            "公司资料来源返回错误：{}",
            text(payload, "message").unwrap_or_else(|| "未返回成功状态".to_owned())
        ));
    }
    let row = payload
        .pointer("/result/data")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find(|row| {
                text(row, "SECUCODE")
                    .and_then(|value| cn_company_profile_code(&value).ok())
                    .as_deref()
                    == Some(code.as_str())
            })
        })
        .ok_or_else(|| "公司资料未返回匹配当前股票的数据。".to_owned())?;
    let profile = json!({"code":code,"name":cn_company_profile_text(row,"ORG_NAME").unwrap_or_default(),
        "industry":cn_company_profile_text(row,"EM2016").unwrap_or_default(),
        "business_summary":cn_company_profile_text(row,"MAIN_BUSINESS").unwrap_or_default(),
        "company_summary":cn_company_profile_text(row,"ORG_PROFILE").unwrap_or_default(),
        "source":"东方财富公司资料","fetched_at":fetched_at});
    if !valid_cn_company_profile(&profile, &code) {
        return Err("公司资料暂未提供公司名称或业务介绍。".to_owned());
    }
    Ok(profile)
}

fn read_cn_company_profile(path: &std::path::Path, code: &str) -> Result<Option<Value>, String> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("公司资料缓存读取失败：{error}")),
    };
    if !metadata.is_file() || metadata.len() > COMPANY_PROFILE_MAX_BYTES {
        return Err("公司资料缓存无效，请刷新公司资料。".to_owned());
    }
    let bytes = std::fs::read(path).map_err(|error| format!("公司资料缓存读取失败：{error}"))?;
    let profile: Value = serde_json::from_slice(&bytes)
        .map_err(|_| "公司资料缓存无效，请刷新公司资料。".to_owned())?;
    if !valid_cn_company_profile(&profile, code) {
        return Err("公司资料缓存与当前股票不匹配，请刷新公司资料。".to_owned());
    }
    Ok(Some(profile))
}

async fn load_cn_company_profile<F>(
    path: &std::path::Path,
    code: &str,
    now: &str,
    refresh: bool,
    fetch: F,
) -> Result<Value, String>
where
    F: std::future::Future<Output = Result<Value, String>>,
{
    load_cn_company_profile_with_writer(
        path,
        code,
        now,
        refresh,
        fetch,
        crate::durability::atomic_write_json_with_status,
    )
    .await
}

async fn load_cn_company_profile_with_writer<F>(
    path: &std::path::Path,
    code: &str,
    now: &str,
    refresh: bool,
    fetch: F,
    write: impl FnOnce(&std::path::Path, &[u8]) -> Result<(), crate::durability::AtomicWriteError>,
) -> Result<Value, String>
where
    F: std::future::Future<Output = Result<Value, String>>,
{
    let code = cn_company_profile_code(code)?;
    let (request_state, generation) = company_profile_request(path)?;
    let cached = {
        let _guard = request_state
            .lock()
            .map_err(|_| "公司资料缓存状态暂不可用，请刷新重试。".to_owned())?;
        match read_cn_company_profile(path, &code) {
            Ok(cached) => cached,
            Err(_) if refresh => None,
            Err(error) => return Err(error),
        }
    };
    let now_ms = now
        .parse::<u128>()
        .map_err(|_| "公司资料抓取时间无效。".to_owned())?;
    let fresh = cached
        .as_ref()
        .and_then(|profile| text(profile, "fetched_at"))
        .and_then(|time| time.parse::<u128>().ok())
        .and_then(|time| now_ms.checked_sub(time))
        .is_some_and(|age| age < COMPANY_PROFILE_TTL_MS);
    if fresh && !refresh {
        return Ok(json!({"code":code,"profile":cached,"stale":false,"error":null}));
    }
    let fetched = fetch.await;
    let mut state = request_state
        .lock()
        .map_err(|_| "公司资料缓存状态暂不可用，请刷新重试。".to_owned())?;
    // Network requests stay concurrent. File replacement advances the generation,
    // including visible commits whose directory sync needs a retry.
    if let Some(published) = &state.published {
        if published.generation > generation {
            return Ok(
                json!({"code":code,"profile":published.profile,"stale":false,"error":published.cache_warning}),
            );
        }
    }
    match fetched {
        Ok(profile) => {
            if !valid_cn_company_profile(&profile, &code) {
                return Err("未返回匹配当前股票的公司资料".to_owned());
            }
            let bytes = serde_json::to_vec(&profile).map_err(|error| error.to_string())?;
            let (replaced, error) = if bytes.len() as u64 > COMPANY_PROFILE_MAX_BYTES {
                (
                    false,
                    Some("资料已读取，但超过本地缓存大小限制，本次未保存。".to_owned()),
                )
            } else {
                match write(path, &bytes) {
                    Ok(()) => (true, None),
                    Err(error) if error.was_replaced() => (
                        true,
                        Some("资料已读取并更新缓存，但存储同步未确认，可刷新重试。".to_owned()),
                    ),
                    Err(_) => (
                        false,
                        Some("资料已读取，本地缓存保存失败，可刷新重试。".to_owned()),
                    ),
                }
            };
            if replaced {
                state.published = Some(PublishedCompanyProfile {
                    generation,
                    profile: profile.clone(),
                    cache_warning: error.clone(),
                });
            }
            Ok(json!({"code":code,"profile":profile,"stale":false,"error":error}))
        }
        Err(error) => {
            let cached = state
                .published
                .as_ref()
                .map(|published| published.profile.clone())
                .or(cached);
            Ok(json!({"code":code,"stale":cached.is_some(),"profile":cached,"error":error}))
        }
    }
}

#[tauri::command]
pub(crate) async fn api_cn_company_profile(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    use tauri::Manager;
    let code = cn_company_profile_code(
        payload
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )?;
    let path = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("cn-company-profiles")
        .join(format!("{code}.json"));
    let fetched_at = crate::market::epoch_millis().to_string();
    let refresh = payload
        .get("refresh")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let fetch = async {
        let work = crate::runtime::with_heavy_network_permit("api_cn_company_profile", async {
            let preferred = crate::market::build_http_client_with_proxy(
                "Mozilla/5.0 GuXuanYou cn-company-profile",
                Duration::from_secs(6),
                Some(&payload),
            )?;
            let direct = crate::market::build_direct_http_client(
                "Mozilla/5.0 GuXuanYou cn-company-profile",
                Duration::from_secs(6),
            )?;
            let mut url = reqwest::Url::parse(crate::market::EASTMONEY_SECURITIES_ENDPOINT)
                .expect("fixed public endpoint");
            url.query_pairs_mut()
                .append_pair("reportName", "RPT_F10_ORG_BASICINFO")
                .append_pair(
                    "columns",
                    "SECUCODE,ORG_NAME,EM2016,MAIN_BUSINESS,ORG_PROFILE",
                )
                .append_pair("filter", &format!("(SECUCODE=\"{code}\")"))
                .append_pair("pageNumber", "1")
                .append_pair("pageSize", "1")
                .append_pair("source", "HSF10")
                .append_pair("client", "PC");
            let response = fetch_json(
                &preferred,
                &direct,
                url.as_str(),
                "https://emweb.securities.eastmoney.com/",
            )
            .await?;
            parse_cn_company_profile(&response, &code, &fetched_at)
        });
        tokio::time::timeout(Duration::from_secs(12), work)
            .await
            .map_err(|_| "公司资料连接超时，请刷新重试。".to_owned())?
    };
    load_cn_company_profile(&path, &code, &fetched_at, refresh, fetch).await
}

fn supplement_report_metrics(reports: &mut Map<String, Value>, rows: &[Value]) -> usize {
    let mut supplemented = 0;
    for row in rows {
        let Some(code) = text(row, "SECUCODE") else {
            continue;
        };
        let Some(report) = reports.get_mut(&code).and_then(Value::as_object_mut) else {
            continue;
        };
        let period = text(row, "REPORTDATE")
            .and_then(|date| crate::market::financial_period_from_text(&date));
        let notice = text(row, "NOTICE_DATE").map(|date| date.chars().take(10).collect::<String>());
        if report.get("currency").and_then(Value::as_str) != Some("CNY")
            || period.as_deref() != report.get("period").and_then(Value::as_str)
            || notice.as_deref() != report.get("report_notice_date").and_then(Value::as_str)
        {
            continue;
        }
        let mut fields = Vec::new();
        for (target, source) in [("latest_eps", "BASIC_EPS"), ("roe", "WEIGHTAVG_ROE")] {
            if crate::market::json_f64(report.get(target)).is_none() {
                if let Some(value) = row
                    .get(source)
                    .and_then(|value| crate::market::json_f64(Some(value)))
                {
                    report.insert(target.to_string(), json!(value));
                    fields.push(target);
                }
            }
        }
        if !fields.is_empty() {
            supplemented += 1;
            let note = format!(
                "{} 采用东方财富同报告期、同披露日期的业绩报告数据。",
                fields.join("、")
            );
            let notes = report.entry("notes").or_insert_with(|| json!([]));
            if let Some(notes) = notes.as_array_mut() {
                if !notes
                    .iter()
                    .any(|value| value.as_str() == Some(note.as_str()))
                {
                    notes.push(json!(note));
                }
            }
        }
    }
    supplemented
}

pub(crate) fn parse_session_calendar(
    script: &str,
    rows: &[Value],
    as_of: &str,
    fetched_at: &str,
) -> Result<Value, String> {
    // Read the exchange's named literals as data. Never execute the downloaded script.
    let literal = |name: &str| {
        script
            .split_once(&format!("var {name}"))
            .and_then(|(_, tail)| tail.split_once('='))
            .and_then(|(_, tail)| tail.split_once(';'))
            .map(|(value, _)| value.trim().trim_matches(['\"', '\'']))
    };
    let date = |value: &str| {
        chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .ok()
            .map(|date| date.format("%Y%m%d").to_string())
    };
    let system_date = literal("systemDate_global")
        .and_then(date)
        .ok_or("交易所日期响应无效")?;
    let last_date = literal("lastTradeDate_global")
        .and_then(date)
        .ok_or("交易所最近交易日响应无效")?;
    let trading = match literal("whetherTradeDate_global") {
        Some("true") => true,
        Some("false") => false,
        _ => return Err("交易所交易状态无效".to_string()),
    };
    if system_date != as_of || last_date.as_str() > as_of {
        return Err("交易所日期信息未覆盖当前日期".to_string());
    }
    let mut dates = std::collections::BTreeSet::new();
    for row in rows {
        let value = row
            .as_array()
            .and_then(|row| row.first())
            .and_then(Value::as_str)
            .and_then(date)
            .ok_or("交易日序列包含无效日期")?;
        if value.as_str() > as_of {
            return Err("交易日序列包含未来行情日期".to_string());
        }
        dates.insert(value);
    }
    if dates.is_empty()
        || (!trading && (!dates.contains(&last_date) || dates.contains(as_of)))
        || (trading && last_date != as_of && !dates.contains(&last_date))
    {
        return Err("实际交易日序列与交易所状态不一致".to_string());
    }
    if trading {
        dates.insert(as_of.to_string());
    }
    Ok(
        json!({"checked":true,"source":"sse_calendar_tencent_sessions","fetched_at":fetched_at,
        "coverage_start":dates.first(),"coverage_end":as_of,"last_trade_date":last_date,"is_trading_day":trading,
        "trading_dates":dates.into_iter().collect::<Vec<_>>()}),
    )
}

fn text(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "-")
        .map(str::to_string)
}

fn listed_on(listing_date: Option<&str>, as_of: &str) -> bool {
    listing_date
        .and_then(crate::market::compact_date_key)
        .is_some_and(|date| date.as_str() <= as_of)
}

fn normalize_name(name: &str) -> String {
    name.chars()
        .filter(|ch| !ch.is_whitespace())
        .map(|ch| match ch {
            'Ａ' => 'A',
            'Ｂ' => 'B',
            _ => ch,
        })
        .collect()
}

pub(crate) fn parse_sse_directory(value: &Value, as_of: &str) -> Result<Vec<Value>, String> {
    let rows = value
        .get("result")
        .and_then(Value::as_array)
        .ok_or("上交所证券目录缺少 result")?;
    let expected = value
        .pointer("/pageHelp/total")
        .and_then(Value::as_u64)
        .ok_or("上交所证券目录缺少总数")?;
    if expected == 0 || expected as usize != rows.len() {
        return Err("上交所证券目录分页尚未完整返回，保留已有目录".to_string());
    }
    let mut stocks = BTreeMap::new();
    for row in rows {
        if !matches!(text(row, "STOCK_TYPE").as_deref(), Some("1" | "8"))
            || text(row, "DELIST_DATE").is_some_and(|date| listed_on(Some(&date), as_of))
            || !listed_on(text(row, "LIST_DATE").as_deref(), as_of)
        {
            continue;
        }
        let code = text(row, "A_STOCK_CODE")
            .filter(|code| crate::market::valid_digits(code))
            .ok_or("上交所 A 股目录包含无效代码")?;
        let name = text(row, "SEC_NAME_CN")
            .or_else(|| text(row, "COMPANY_ABBR"))
            .ok_or("上交所 A 股目录包含空名称")?;
        let code = format!("{code}.SH");
        stocks.insert(
            code.clone(),
            json!({"code":code,"name":normalize_name(&name),
            "industry":text(row,"CSRC_CODE_DESC").unwrap_or_default(),
            "listing_date":text(row,"LIST_DATE"),"directory_source":"sse"}),
        );
    }
    Ok(stocks.into_values().collect())
}

pub(crate) fn parse_szse_directory(bytes: &[u8], as_of: &str) -> Result<Vec<Value>, String> {
    let mut workbook: Xlsx<Cursor<&[u8]>> = open_workbook_from_rs(Cursor::new(bytes))
        .map_err(|error| format!("深交所证券目录工作簿解析失败：{error}"))?;
    let name = workbook
        .sheet_names()
        .first()
        .cloned()
        .ok_or("深交所证券目录工作簿为空")?;
    let sheet = workbook
        .worksheet_range(&name)
        .map_err(|error| error.to_string())?;
    let mut rows = sheet.rows();
    let headers = rows.next().ok_or("深交所证券目录缺少表头")?;
    let columns: Vec<String> = headers
        .iter()
        .map(|cell| normalize_name(&cell.to_string()))
        .collect();
    let code_col = columns
        .iter()
        .position(|column| column == "A股代码")
        .ok_or("深交所目录缺少 A 股代码列")?;
    let name_col = columns
        .iter()
        .position(|column| column == "A股简称")
        .ok_or("深交所目录缺少 A 股简称列")?;
    let date_col = columns
        .iter()
        .position(|column| column == "A股上市日期")
        .ok_or("深交所目录缺少上市日期列")?;
    let industry_col = columns.iter().position(|column| column == "所属行业");
    let mut stocks = BTreeMap::new();
    for row in rows {
        let cell = |column: usize| row.get(column).map(ToString::to_string).unwrap_or_default();
        let raw_code = cell(code_col);
        let code = raw_code
            .parse::<u32>()
            .ok()
            .map(|code| format!("{code:06}"));
        let Some(code) = code.filter(|code| code.starts_with('0') || code.starts_with('3')) else {
            continue;
        };
        let listing_date = cell(date_col);
        if !listed_on(Some(&listing_date), as_of) {
            continue;
        }
        let code = format!("{code}.SZ");
        let name = normalize_name(&cell(name_col));
        if name.is_empty() {
            return Err("深交所证券目录包含空名称".to_string());
        }
        stocks.insert(
            code.clone(),
            json!({"code":code,"name":name,
            "industry":industry_col.map(cell).unwrap_or_default(),
            "listing_date":listing_date,"directory_source":"szse"}),
        );
    }
    if stocks.is_empty() {
        return Err("深交所证券目录没有已上市 A 股".to_string());
    }
    Ok(stocks.into_values().collect())
}

pub(crate) fn listed_beijing_directory(rows: &[Value], as_of: &str) -> Vec<Value> {
    rows.iter()
        .filter(|row| {
            text(row, "code").is_some_and(|code| code.ends_with(".BJ"))
                && listed_on(text(row, "listing_date").as_deref(), as_of)
                && (!text(row, "name").is_some_and(|name| name.contains('退'))
                    || row
                        .get("price")
                        .and_then(Value::as_f64)
                        .is_some_and(|price| price > 0.0))
        })
        .cloned()
        .collect()
}

pub(crate) fn parse_financial_reports(
    rows: &[Value],
    codes: &HashSet<String>,
    as_of: &str,
    fetched_at: &str,
) -> Map<String, Value> {
    let mut reports = BTreeMap::<String, &Value>::new();
    for row in rows {
        let Some(code) = text(row, "SECUCODE").filter(|code| codes.contains(code)) else {
            continue;
        };
        if !code.split_once('.').is_some_and(|(digits, exchange)| {
            crate::market::valid_digits(digits) && matches!(exchange, "SH" | "SZ" | "BJ")
        }) {
            continue;
        }
        let Some(report_date) =
            text(row, "REPORT_DATE").and_then(|date| crate::market::compact_date_key(&date))
        else {
            continue;
        };
        let Some(notice_date) =
            text(row, "NOTICE_DATE").and_then(|date| crate::market::compact_date_key(&date))
        else {
            continue;
        };
        if report_date.as_str() > as_of || notice_date.as_str() > as_of {
            continue;
        }
        let rank = (report_date, notice_date);
        let replace = reports.get(&code).is_none_or(|current| {
            rank > (
                text(current, "REPORT_DATE")
                    .and_then(|date| crate::market::compact_date_key(&date))
                    .unwrap_or_default(),
                text(current, "NOTICE_DATE")
                    .and_then(|date| crate::market::compact_date_key(&date))
                    .unwrap_or_default(),
            )
        });
        if replace {
            reports.insert(code, row);
        }
    }
    reports
        .into_iter()
        .filter_map(|(code, row)| {
            let wrapper = json!({"result":{"data":[row]}});
            let mut fields =
                crate::observe::parse_latest_observe_financial_snapshot(&wrapper, &code).ok()?;
            let period = text(row, "REPORT_DATE")
                .and_then(|date| crate::market::financial_period_from_text(&date))?;
            fields.insert("period".into(), json!(period));
            fields.insert("source".into(), json!(FINANCIAL_SOURCE));
            fields.insert("report_fetched_at".into(), json!(fetched_at));
            fields.insert(
                "report_notice_date".into(),
                json!(
                    text(row, "NOTICE_DATE").map(|date| date.chars().take(10).collect::<String>())
                ),
            );
            Some((code, Value::Object(fields)))
        })
        .collect()
}

pub(crate) fn apply_reference_data(
    seed: &mut Value,
    directory: &[Value],
    financials: Map<String, Value>,
    fetched_at: &str,
) {
    let (mut stocks, _) = crate::market::seed_stock_maps(seed);
    if !directory.is_empty() {
        let current: HashSet<String> = directory
            .iter()
            .filter_map(|row| text(row, "code"))
            .collect();
        let mut archive = seed
            .get("archived_stocks")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        for (code, stock) in &stocks {
            if !current.contains(code) {
                archive.insert(code.clone(), Value::Object(stock.clone()));
            }
        }
        stocks.retain(|code, _| current.contains(code));
        for item in directory {
            let Some(code) = text(item, "code") else {
                continue;
            };
            let stock = stocks.entry(code.clone()).or_insert_with(|| {
                Map::from_iter([
                    ("code".to_string(), json!(code)),
                    ("price".to_string(), json!(0.0)),
                ])
            });
            for field in ["name", "listing_date", "directory_source"] {
                if let Some(value) = item.get(field) {
                    stock.insert(field.into(), value.clone());
                }
            }
            if stock
                .get("industry")
                .and_then(Value::as_str)
                .is_none_or(|industry| industry.is_empty())
            {
                stock.insert(
                    "industry".into(),
                    item.get("industry")
                        .cloned()
                        .unwrap_or_else(|| json!(crate::market::board_label(&code))),
                );
            }
            if stock
                .get("price")
                .and_then(Value::as_f64)
                .is_none_or(|price| price <= 0.0)
            {
                if let Some(price) = item
                    .get("price")
                    .and_then(Value::as_f64)
                    .filter(|price| *price > 0.0)
                {
                    stock.insert("price".into(), json!(price));
                }
            }
            stock.insert(
                "is_st".into(),
                json!(
                    text(item, "name").is_some_and(|name| name.to_ascii_uppercase().contains("ST"))
                ),
            );
        }
        seed["archived_stocks"] = Value::Object(archive);
        let mut codes: Vec<String> = current.into_iter().collect();
        codes.sort();
        if !seed.get("cn_reference").is_some_and(Value::is_object) {
            seed["cn_reference"] = json!({});
        }
        seed["cn_reference"]["directory"] = json!({"checked":true,"fetched_at":fetched_at,"count":codes.len(),"codes":codes,
            "scope":"sse_szse_listed_bse_quote","sources":["sse","szse","eastmoney_cn_directory"]});
    }
    let mut entries = seed
        .get("financials")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for (code, mut entry) in financials {
        entry["report_fetched_at"] = json!(fetched_at);
        crate::market::merge_financial_entry(&mut entries, &code, &entry);
    }
    for (code, stock) in &mut stocks {
        let Some(entry) = entries
            .get(code)
            .and_then(Value::as_object)
            .filter(|entry| entry.get("report_fetched_at").is_some())
        else {
            continue;
        };
        for field in [
            "latest_eps",
            "latest_bps",
            "deducted_net_profit_billion",
            "deducted_net_profit_margin",
            "deducted_net_profit_growth_rate",
        ] {
            stock.insert(
                field.into(),
                entry.get(field).cloned().unwrap_or(Value::Null),
            );
        }
        stock.insert(
            "roe".into(),
            json!(entry
                .get("roe")
                .and_then(Value::as_f64)
                .map(|value| value / 100.0)),
        );
        for (target, source) in [
            ("period", "period"),
            ("financial_source", "source"),
            ("financial_report_notice_date", "report_notice_date"),
        ] {
            if let Some(value) = entry.get(source) {
                stock.insert(target.into(), value.clone());
            }
        }
    }
    let sorted: BTreeMap<_, _> = stocks.into_iter().collect();
    seed["stocks"] = Value::Array(sorted.into_values().map(Value::Object).collect());
    seed["financials"] = Value::Object(entries);
}

pub(crate) fn directory_candidate_codes(seed: &Value) -> Option<Vec<String>> {
    let directory = seed.pointer("/cn_reference/directory")?;
    if directory.get("checked").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let codes = directory
        .get("codes")?
        .as_array()?
        .iter()
        .map(|value| value.as_str().map(str::to_string))
        .collect::<Option<Vec<_>>>()?;
    (!codes.is_empty()).then_some(codes)
}

const PAGE_SIZE: usize = 500;
const FETCH_CONCURRENCY: usize = 6;
const FINANCIAL_COLUMNS: &str = "SECUCODE,SECURITY_CODE,SECURITY_NAME_ABBR,ORG_CODE,REPORT_DATE,REPORT_DATE_NAME,NOTICE_DATE,CURRENCY,EPSJB,BPS,TOTALOPERATEREVE,TOTALOPERATEREVETZ,PARENTNETPROFIT,PARENTNETPROFITTZ,KCFJCXSYJLR,KCFJCXSYJLRTZ,XSMLL,XSJLL,ROEJQ,ZCFZL,SECURITY_TYPE_CODE";

async fn reference_bytes(
    client: &reqwest::Client,
    url: &str,
    referer: &str,
) -> Result<Vec<u8>, String> {
    let response = client
        .get(url)
        .header(reqwest::header::REFERER, referer)
        .send()
        .await
        .map_err(|error| format!("公开数据请求失败：{error}"))?;
    if !response.status().is_success() {
        return Err(format!("公开数据接口返回 HTTP {}", response.status()));
    }
    const LIMIT: usize = 10 * 1024 * 1024;
    if response
        .content_length()
        .is_some_and(|length| length > LIMIT as u64)
    {
        return Err("公开数据单页超过读取上限".to_string());
    }
    let mut body = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|error| error.to_string())?;
        if bytes.len().saturating_add(chunk.len()) > LIMIT {
            return Err("公开数据单页超过读取上限".to_string());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn fetch_bytes(
    preferred: &reqwest::Client,
    direct: &reqwest::Client,
    url: &str,
    referer: &str,
) -> Result<Vec<u8>, String> {
    match tokio::time::timeout(
        Duration::from_secs(3),
        reference_bytes(preferred, url, referer),
    )
    .await
    {
        Ok(Ok(bytes)) => Ok(bytes),
        _ => match reference_bytes(direct, url, referer).await {
            Ok(bytes) => Ok(bytes),
            Err(first) => reference_bytes(direct, url, referer)
                .await
                .map_err(|last| format!("{first}; retry failed: {last}")),
        },
    }
}

async fn fetch_json(
    preferred: &reqwest::Client,
    direct: &reqwest::Client,
    url: &str,
    referer: &str,
) -> Result<Value, String> {
    let bytes = fetch_bytes(preferred, direct, url, referer).await?;
    serde_json::from_slice(&bytes).map_err(|error| format!("公开数据 JSON 解析失败：{error}"))
}

fn financial_url(
    filter: &str,
    page: usize,
    page_size: usize,
    columns: &str,
    latest: bool,
) -> String {
    let mut url = reqwest::Url::parse(crate::market::EASTMONEY_SECURITIES_ENDPOINT)
        .expect("fixed public endpoint");
    url.query_pairs_mut()
        .append_pair("reportName", "RPT_F10_FINANCE_MAINFINADATA")
        .append_pair("columns", columns)
        .append_pair("filter", filter)
        .append_pair("pageNumber", &page.to_string())
        .append_pair("pageSize", &page_size.to_string())
        .append_pair(
            "sortColumns",
            if latest { "REPORT_DATE" } else { "SECUCODE" },
        )
        .append_pair("sortTypes", if latest { "-1" } else { "1" })
        .append_pair("source", "HSF10")
        .append_pair("client", "PC");
    url.to_string()
}

async fn fetch_report_supplements(
    preferred: &reqwest::Client,
    direct: &reqwest::Client,
    reports: &mut Map<String, Value>,
) -> Vec<String> {
    let mut groups = BTreeMap::<String, Vec<String>>::new();
    for (code, report) in reports.iter() {
        if report.get("latest_eps").is_none_or(Value::is_null)
            || (report.get("roe").is_none_or(Value::is_null)
                && report
                    .get("latest_bps")
                    .and_then(Value::as_f64)
                    .is_some_and(|bps| bps > 0.0))
        {
            if let Some(period) = text(report, "period") {
                groups.entry(period).or_default().push(code.clone());
            }
        }
    }
    let mut errors = Vec::new();
    for (period, codes) in groups {
        let Some((year, quarter)) = period.split_once('Q') else {
            continue;
        };
        let end = match quarter {
            "1" => "03-31",
            "2" => "06-30",
            "3" => "09-30",
            "4" => "12-31",
            _ => continue,
        };
        for codes in codes.chunks(128) {
            let codes = codes
                .iter()
                .map(|code| format!("\"{code}\""))
                .collect::<Vec<_>>()
                .join(",");
            let filter = format!("(REPORTDATE='{year}-{end}')(SECUCODE in ({codes}))");
            let mut url = reqwest::Url::parse(crate::market::EASTMONEY_DATACENTER_ENDPOINT)
                .expect("fixed public endpoint");
            url.query_pairs_mut()
                .append_pair("reportName", "RPT_LICO_FN_CPD")
                .append_pair(
                    "columns",
                    "SECUCODE,REPORTDATE,NOTICE_DATE,BASIC_EPS,WEIGHTAVG_ROE",
                )
                .append_pair("filter", &filter)
                .append_pair("pageNumber", "1")
                .append_pair("pageSize", "500")
                .append_pair("source", "WEB")
                .append_pair("client", "WEB");
            match fetch_json(
                preferred,
                direct,
                url.as_str(),
                "https://data.eastmoney.com/",
            )
            .await
            .and_then(|value| {
                crate::market::normalize_eastmoney_public_json(value, "同报告期业绩报告", true)
            }) {
                Ok(value) => {
                    supplement_report_metrics(
                        reports,
                        crate::market::eastmoney_result_rows(&value),
                    );
                }
                Err(error) => errors.push(error),
            }
        }
    }
    errors
}

fn sse_url(stock_type: &str) -> String {
    let mut url = reqwest::Url::parse("https://query.sse.com.cn/sseQuery/commonQuery.do")
        .expect("fixed SSE endpoint");
    url.query_pairs_mut()
        .append_pair("STOCK_TYPE", stock_type)
        .append_pair("sqlId", "COMMON_SSE_CP_GPJCTPZ_GPLB_GP_L")
        .append_pair("COMPANY_STATUS", "2,4,5,7,8")
        .append_pair("type", "inParams")
        .append_pair("isPagination", "true")
        .append_pair("pageHelp.cacheSize", "1")
        .append_pair("pageHelp.beginPage", "1")
        .append_pair("pageHelp.pageSize", "10000")
        .append_pair("pageHelp.pageNo", "1")
        .append_pair("pageHelp.endPage", "1");
    url.to_string()
}

fn beijing_url(page: usize) -> String {
    let mut url = reqwest::Url::parse("https://push2.eastmoney.com/api/qt/clist/get")
        .expect("fixed quote directory endpoint");
    url.query_pairs_mut()
        .append_pair("pn", &page.to_string())
        .append_pair("pz", "100")
        .append_pair("po", "0")
        .append_pair("np", "1")
        .append_pair("fltt", "2")
        .append_pair("invt", "2")
        .append_pair("fid", "f12")
        .append_pair("fs", "m:0+t:81+s:2048")
        .append_pair("fields", "f12,f13,f14,f2,f9,f23,f26,f18,f124");
    url.to_string()
}

async fn fetch_beijing_directory(
    preferred: &reqwest::Client,
    direct: &reqwest::Client,
    as_of: &str,
) -> Result<Vec<Value>, String> {
    let first = fetch_json(
        preferred,
        direct,
        &beijing_url(1),
        "https://quote.eastmoney.com/",
    )
    .await?;
    let count = first
        .pointer("/data/total")
        .and_then(Value::as_u64)
        .filter(|count| *count > 0 && *count <= 1500)
        .ok_or("北交所行情目录缺少有效总数")? as usize;
    let mut rows = first
        .pointer("/data/diff")
        .and_then(Value::as_array)
        .cloned()
        .ok_or("北交所行情目录缺少记录")?;
    let pages = count.div_ceil(100);
    let results = stream::iter(2..=pages)
        .map(|page| async move {
            fetch_json(
                preferred,
                direct,
                &beijing_url(page),
                "https://quote.eastmoney.com/",
            )
            .await
        })
        .buffer_unordered(FETCH_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
    for result in results {
        let value = result?;
        if value.pointer("/data/total").and_then(Value::as_u64) != Some(count as u64) {
            return Err("北交所行情目录在分页期间发生变化，保留已有目录".to_string());
        }
        rows.extend(
            value
                .pointer("/data/diff")
                .and_then(Value::as_array)
                .ok_or("北交所行情目录分页缺少记录")?
                .clone(),
        );
    }
    let unique: HashSet<_> = rows.iter().filter_map(|row| text(row, "f12")).collect();
    if rows.len() != count || unique.len() != count {
        return Err("北交所行情目录分页不完整，保留已有目录".to_string());
    }
    let directory: Vec<Value> = rows.into_iter().filter_map(|row| {
        let code = text(&row,"f12").filter(|code| code.starts_with("92") && crate::market::valid_digits(code))?;
        let listing_date = row.get("f26").and_then(Value::as_u64).map(|date| date.to_string())
            .or_else(|| text(&row,"f26"));
        Some(json!({"code":format!("{code}.BJ"),"name":text(&row,"f14"),"industry":"北交所",
            "listing_date":listing_date,"directory_source":"eastmoney_cn_directory",
            "price":row.get("f2").and_then(Value::as_f64),"previous_close":row.get("f18").and_then(Value::as_f64),
            "pe":row.get("f9").and_then(Value::as_f64),"pb":row.get("f23").and_then(Value::as_f64)}))
    }).collect();
    Ok(listed_beijing_directory(&directory, as_of))
}

async fn fetch_directory(
    preferred: &reqwest::Client,
    direct: &reqwest::Client,
    as_of: &str,
) -> Result<Vec<Value>, String> {
    let szse_url =
        "https://www.szse.cn/api/report/ShowReport?SHOWTYPE=xlsx&CATALOGID=1110&TABKEY=tab1";
    let main_url = sse_url("1");
    let star_url = sse_url("8");
    let (main, star, szse, beijing) = tokio::join!(
        fetch_json(preferred, direct, &main_url, "https://www.sse.com.cn/"),
        fetch_json(preferred, direct, &star_url, "https://www.sse.com.cn/"),
        fetch_bytes(preferred, direct, szse_url, "https://www.szse.cn/"),
        fetch_beijing_directory(preferred, direct, as_of),
    );
    let mut rows = parse_sse_directory(&main?, as_of)?;
    rows.extend(parse_sse_directory(&star?, as_of)?);
    rows.extend(parse_szse_directory(&szse?, as_of)?);
    rows.extend(beijing?);
    let unique: HashSet<_> = rows.iter().filter_map(|row| text(row, "code")).collect();
    if unique.len() != rows.len() {
        return Err("沪深北证券目录存在重复代码，保留已有目录".to_string());
    }
    Ok(rows)
}

async fn fetch_session_calendar(
    preferred: &reqwest::Client,
    direct: &reqwest::Client,
    as_of: &str,
    fetched_at: &str,
) -> Result<Value, String> {
    let script = fetch_bytes(
        preferred,
        direct,
        &format!("https://www.sse.com.cn/js/common/systemDate_global.js?v={as_of}"),
        "https://www.sse.com.cn/",
    )
    .await?;
    let script = String::from_utf8(script).map_err(|error| error.to_string())?;
    let mut url = reqwest::Url::parse(crate::market::TENCENT_DAILY_KLINE_ENDPOINT)
        .expect("fixed Tencent history endpoint");
    url.query_pairs_mut()
        .append_pair("param", "sh000001,day,,,180,qfq");
    let sessions = fetch_json(preferred, direct, url.as_str(), "https://gu.qq.com/").await?;
    let rows = sessions
        .pointer("/data/sh000001/qfqday")
        .or_else(|| sessions.pointer("/data/sh000001/day"))
        .and_then(Value::as_array)
        .ok_or("行情来源没有返回交易日序列")?;
    parse_session_calendar(&script, rows, as_of, fetched_at)
}

struct FinancialBatch {
    period: String,
    rows: Vec<Value>,
    errors: Vec<String>,
}

async fn fetch_financial_batch(
    preferred: &reqwest::Client,
    direct: &reqwest::Client,
    as_of: &str,
) -> Result<FinancialBatch, String> {
    let date = format!("{}-{}-{}", &as_of[..4], &as_of[4..6], &as_of[6..8]);
    let latest_filter = format!("(REPORT_DATE<='{date}')(NOTICE_DATE<='{date} 23:59:59')");
    let latest = fetch_json(
        preferred,
        direct,
        &financial_url(&latest_filter, 1, 1, "REPORT_DATE", true),
        "https://emweb.securities.eastmoney.com/",
    )
    .await?;
    let latest = crate::market::normalize_eastmoney_public_json(latest, "A 股最新报告期", false)?;
    let report_date = crate::market::eastmoney_result_rows(&latest)
        .first()
        .and_then(|row| text(row, "REPORT_DATE"))
        .and_then(|date| crate::market::normalize_history_date(&date))
        .ok_or("A 股财报接口没有已公开的报告期")?;
    let period = crate::market::financial_period_from_text(&report_date)
        .ok_or("A 股财报接口报告期无法识别")?;
    let filter = format!("(REPORT_DATE='{report_date}')");
    let first = fetch_json(
        preferred,
        direct,
        &financial_url(&filter, 1, PAGE_SIZE, FINANCIAL_COLUMNS, false),
        "https://emweb.securities.eastmoney.com/",
    )
    .await?;
    let first = crate::market::normalize_eastmoney_public_json(first, "A 股全市场财报", false)?;
    let count = first
        .pointer("/result/count")
        .and_then(Value::as_u64)
        .filter(|count| *count > 0 && *count <= 50_000)
        .ok_or("A 股财报接口缺少有效记录总数")? as usize;
    let pages = first
        .pointer("/result/pages")
        .and_then(Value::as_u64)
        .filter(|pages| *pages as usize == count.div_ceil(PAGE_SIZE))
        .ok_or("A 股财报接口分页信息不一致")? as usize;
    let mut rows = crate::market::eastmoney_result_rows(&first).to_vec();
    let results = stream::iter(2..=pages)
        .map(|page| {
            let filter = &filter;
            async move {
                (
                    page,
                    fetch_json(
                        preferred,
                        direct,
                        &financial_url(filter, page, PAGE_SIZE, FINANCIAL_COLUMNS, false),
                        "https://emweb.securities.eastmoney.com/",
                    )
                    .await,
                )
            }
        })
        .buffer_unordered(FETCH_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
    let mut errors = Vec::new();
    for (page, result) in results {
        match result.and_then(|value| {
            crate::market::normalize_eastmoney_public_json(value, "A 股财报分页", false)
        }) {
            Ok(value)
                if value.pointer("/result/count").and_then(Value::as_u64) == Some(count as u64) =>
            {
                rows.extend(crate::market::eastmoney_result_rows(&value).to_vec())
            }
            Ok(_) => errors.push(format!("财报第 {page} 页总数变化，稍后重试")),
            Err(error) => errors.push(format!("财报第 {page} 页：{error}")),
        }
    }
    if rows.len() != count && errors.is_empty() {
        errors.push(format!("财报分页返回 {}/{} 条", rows.len(), count));
    }
    Ok(FinancialBatch {
        period,
        rows,
        errors,
    })
}

pub(crate) async fn refresh_reference_data(
    app: &tauri::AppHandle,
    seed: &mut Value,
    payload: &Value,
) -> Result<Value, String> {
    prepare_reference_data(Some(app), seed, payload).await
}

async fn prepare_reference_data(
    app: Option<&tauri::AppHandle>,
    seed: &mut Value,
    payload: &Value,
) -> Result<Value, String> {
    let fetched_at = crate::market::epoch_millis().to_string();
    let as_of = crate::market::local_yyyymmdd_from_epoch_ms(crate::market::epoch_millis())
        .ok_or("无法读取当前日期")?;
    let preferred = crate::market::build_http_client_with_proxy(
        "Mozilla/5.0 GuXuanYou cn-financial-refresh",
        Duration::from_secs(8),
        Some(payload),
    )?;
    let direct = crate::market::build_direct_http_client(
        "Mozilla/5.0 GuXuanYou cn-financial-refresh",
        Duration::from_secs(8),
    )?;
    if let Some(app) = app {
        crate::market::emit_market_refresh_log(app, "cn_reference_start", "info", json!({}));
    }
    let work = async {
        let (directory, batch, calendar) = tokio::join!(
            fetch_directory(&preferred, &direct, &as_of),
            fetch_financial_batch(&preferred, &direct, &as_of),
            fetch_session_calendar(&preferred, &direct, &as_of, &fetched_at)
        );
        let mut errors = Vec::new();
        let directory = match directory {
            Ok(rows) => rows,
            Err(error) => {
                errors.push(error);
                Vec::new()
            }
        };
        let codes: HashSet<String> = if directory.is_empty() {
            directory_candidate_codes(seed)
                .unwrap_or_else(|| crate::market::seed_stock_maps(seed).1)
                .into_iter()
                .collect()
        } else {
            directory
                .iter()
                .filter_map(|row| text(row, "code"))
                .collect()
        };
        let (period, mut reports) = match batch {
            Ok(batch) => {
                errors.extend(batch.errors);
                (
                    Some(batch.period),
                    parse_financial_reports(&batch.rows, &codes, &as_of, &fetched_at),
                )
            }
            Err(error) => {
                errors.push(error);
                (None, Map::new())
            }
        };
        let mut missing: Vec<_> = codes
            .iter()
            .filter(|code| !reports.contains_key(*code))
            .cloned()
            .collect();
        missing.sort();
        // Missing market-wide rows include newly listed companies and issuers whose
        // latest public report has an older period. Request their actual latest report.
        if missing.len() > 128 {
            errors.push(format!("{} 只证券的财报需后续补齐", missing.len()));
            missing.truncate(128);
        }
        let results = stream::iter(missing)
            .map(|code| {
                let preferred = &preferred;
                let direct = &direct;
                async move {
                    let filter = format!("(SECUCODE=\"{code}\")");
                    (
                        code,
                        fetch_json(
                            preferred,
                            direct,
                            &financial_url(&filter, 1, 4, FINANCIAL_COLUMNS, true),
                            "https://emweb.securities.eastmoney.com/",
                        )
                        .await,
                    )
                }
            })
            .buffer_unordered(FETCH_CONCURRENCY)
            .collect::<Vec<_>>()
            .await;
        for (code, result) in results {
            match result.and_then(|value| {
                crate::market::normalize_eastmoney_public_json(value, "A 股单证券财报", true)
            }) {
                Ok(value) => reports.extend(parse_financial_reports(
                    crate::market::eastmoney_result_rows(&value),
                    &HashSet::from([code]),
                    &as_of,
                    &fetched_at,
                )),
                Err(error) => errors.push(error),
            }
        }
        errors.extend(fetch_report_supplements(&preferred, &direct, &mut reports).await);
        let fetched = reports.len();
        apply_reference_data(seed, &directory, reports, &fetched_at);
        if !seed.get("cn_reference").is_some_and(Value::is_object) {
            seed["cn_reference"] = json!({});
        }
        match calendar {
            Ok(calendar) => seed["cn_reference"]["calendar"] = calendar,
            Err(error) => errors.push(error),
        }
        if !directory.is_empty() {
            seed["cn_reference"]["directory"]["as_of"] = json!(as_of);
        }
        if fetched > 0 {
            seed["cn_reference"]["financial"] = json!({"fetched_at":fetched_at,"latest_report_period":period,
                "fetched_count":fetched,"requested_count":codes.len(),"missing_count":codes.len().saturating_sub(fetched),
                "source":FINANCIAL_SOURCE,"errors":errors});
        } else {
            seed["cn_reference"]["financial_refresh_error"] = json!(errors);
        }
        let result = json!({"directory_count":directory.len(),"financial_fetched":fetched,"financial_requested":codes.len(),
            "report_period":period,"failed":!errors.is_empty(),"errors":errors});
        if let Some(app) = app {
            crate::market::emit_market_refresh_log(
                app,
                "cn_reference_complete",
                if errors.is_empty() { "ok" } else { "warn" },
                result.clone(),
            );
        }
        Ok::<Value, String>(result)
    };
    match tokio::time::timeout(Duration::from_secs(50), work).await {
        Ok(result) => result,
        Err(_) => {
            let error = "A 股目录与财报更新超时，保留已有缓存";
            if let Some(app) = app {
                crate::market::emit_market_refresh_log(
                    app,
                    "cn_reference_complete",
                    "warn",
                    json!({"error":error}),
                );
            }
            Ok(json!({"failed":true,"errors":[error]}))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cn_company_profile_selects_the_requested_issuer_and_keeps_business_context() {
        let payload = json!({"success":true,"result":{"data":[
            {"SECUCODE":"000001.SZ","ORG_NAME":"另一家公司","MAIN_BUSINESS":"银行业务"},
            {"SECUCODE":"600909.SH","ORG_NAME":"华安证券股份有限公司","EM2016":"金融-非银行金融-证券",
             "MAIN_BUSINESS":"证券自营业务投资股票、基金、债券等金融资产",
             "ORG_PROFILE":"业务涵盖证券经纪、资产管理、私募投资、并购重组等领域。"}
        ]}});
        let profile = parse_cn_company_profile(&payload, "600909.SH", "1000").unwrap();
        assert_eq!(profile["code"], "600909.SH");
        assert_eq!(profile["name"], "华安证券股份有限公司");
        assert_eq!(
            profile["business_summary"],
            "证券自营业务投资股票、基金、债券等金融资产"
        );
        assert_eq!(
            profile["company_summary"],
            "业务涵盖证券经纪、资产管理、私募投资、并购重组等领域。"
        );
        assert_eq!(profile["source"], "东方财富公司资料");
        assert_eq!(profile["fetched_at"], "1000");
    }

    #[test]
    fn cn_company_profile_rejects_wrong_issuers_and_missing_business_descriptions() {
        let other = json!({"success":true,"result":{"data":[{"SECUCODE":"000001.SZ","ORG_NAME":"银行","MAIN_BUSINESS":"银行业务"}]}});
        assert!(parse_cn_company_profile(&other, "600909.SH", "1000").is_err());
        assert!(parse_cn_company_profile(&other, "00700.HK", "1000").is_err());
        let empty = json!({"success":true,"result":{"data":[{"SECUCODE":"600909.SH","ORG_NAME":"华安证券","MAIN_BUSINESS":"-","ORG_PROFILE":null}]}});
        assert!(parse_cn_company_profile(&empty, "600909.SH", "1000").is_err());
    }

    #[test]
    fn cn_company_profile_rejects_a_failed_provider_response_even_with_an_embedded_row() {
        let failed = json!({"success":false,"message":"provider unavailable","result":{"data":[
            {"SECUCODE":"600909.SH","ORG_NAME":"华安证券","MAIN_BUSINESS":"证券业务"}
        ]}});
        assert!(parse_cn_company_profile(&failed, "600909.SH", "1000").is_err());
    }

    #[test]
    fn cn_company_profile_keeps_the_company_overview_when_business_is_a_missing_value_marker() {
        let payload = json!({"success":true,"result":{"data":[
            {"SECUCODE":"600909.SH","ORG_NAME":"华安证券","MAIN_BUSINESS":"--","ORG_PROFILE":"提供证券经纪和资产管理服务。"}
        ]}});
        let profile = parse_cn_company_profile(&payload, "600909.SH", "1000").unwrap();
        assert_eq!(profile["business_summary"], "");
        assert_eq!(profile["company_summary"], "提供证券经纪和资产管理服务。");
    }

    fn company_profile_fixture_path() -> std::path::PathBuf {
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "gp-company-profile-{}-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ))
    }

    fn company_profile_fixture() -> Value {
        json!({"code":"600909.SH","name":"华安证券股份有限公司","industry":"证券",
            "business_summary":"证券经纪和资产管理","company_summary":"综合类证券公司",
            "source":"东方财富公司资料","fetched_at":"1000"})
    }

    #[tokio::test]
    async fn cn_company_profile_uses_a_fresh_cache_without_fetching() {
        let path = company_profile_fixture_path();
        std::fs::write(
            &path,
            serde_json::to_vec(&company_profile_fixture()).unwrap(),
        )
        .unwrap();
        let result = load_cn_company_profile(&path, "600909.SH", "2000", false, async {
            panic!("fresh cache must not make a provider request")
        })
        .await
        .unwrap();
        assert_eq!(result["profile"]["business_summary"], "证券经纪和资产管理");
        assert_eq!(result["stale"], false);
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn cn_company_profile_saves_successful_first_reads_and_cache_refreshes() {
        for (has_cache, refresh, now_ms) in [
            (false, false, 2000),
            (true, false, 1000 + COMPANY_PROFILE_TTL_MS),
            (true, true, 2000),
        ] {
            let path = company_profile_fixture_path();
            if has_cache {
                std::fs::write(
                    &path,
                    serde_json::to_vec(&company_profile_fixture()).unwrap(),
                )
                .unwrap();
            }
            let now = now_ms.to_string();
            let mut updated = company_profile_fixture();
            updated["business_summary"] = json!("最新证券经纪和资产管理业务");
            updated["fetched_at"] = json!(now);
            let result = load_cn_company_profile(&path, "600909.SH", &now, refresh, async {
                Ok(updated.clone())
            })
            .await
            .unwrap();
            assert_eq!(result["profile"], updated);
            assert_eq!(result["stale"], false);
            assert_eq!(result["error"], Value::Null);
            assert_eq!(
                read_cn_company_profile(&path, "600909.SH").unwrap(),
                Some(updated)
            );
            std::fs::remove_file(path).unwrap();
        }
    }

    #[tokio::test]
    async fn cn_company_profile_failed_refresh_retains_the_previous_sourced_profile() {
        let path = company_profile_fixture_path();
        let bytes = serde_json::to_vec(&company_profile_fixture()).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let result = load_cn_company_profile(&path, "600909.SH", "2000", true, async {
            Err("公司资料连接超时".to_owned())
        })
        .await
        .unwrap();
        assert_eq!(result["profile"]["business_summary"], "证券经纪和资产管理");
        assert_eq!(result["profile"]["fetched_at"], "1000");
        assert_eq!(result["stale"], true);
        assert_eq!(result["error"], "公司资料连接超时");
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn cn_company_profile_older_completion_cannot_overwrite_newer_refresh() {
        for newer_time in ["1000", "2000"] {
            let path = company_profile_fixture_path();
            let (older_started_tx, older_started_rx) = tokio::sync::oneshot::channel();
            let (finish_older_tx, finish_older_rx) = tokio::sync::oneshot::channel();
            let mut newer = company_profile_fixture();
            newer["business_summary"] = json!("最新证券业务资料");
            newer["fetched_at"] = json!(newer_time);
            let older = load_cn_company_profile(&path, "600909.SH", "1000", true, async {
                older_started_tx.send(()).unwrap();
                finish_older_rx.await.unwrap();
                Ok(company_profile_fixture())
            });
            let later = async {
                older_started_rx.await.unwrap();
                let result = load_cn_company_profile(&path, "600909.SH", newer_time, true, async {
                    Ok(newer.clone())
                })
                .await
                .unwrap();
                finish_older_tx.send(()).unwrap();
                result
            };
            let (older, later) = tokio::join!(older, later);
            let stored = read_cn_company_profile(&path, "600909.SH").unwrap();
            let normal = load_cn_company_profile(&path, "600909.SH", "3000", false, async {
                panic!("the newer cached profile must remain readable")
            })
            .await
            .unwrap();
            std::fs::remove_file(&path).unwrap();
            assert_eq!(stored, Some(newer.clone()));
            assert_eq!(normal["profile"], newer);
            assert_eq!(later["profile"], newer);
            assert_eq!(older.unwrap()["profile"], newer);
        }
    }

    #[tokio::test]
    async fn cn_company_profile_oversized_refresh_preserves_readable_cache_and_reports_error() {
        let path = company_profile_fixture_path();
        let original = company_profile_fixture();
        let before = serde_json::to_vec(&original).unwrap();
        std::fs::write(&path, &before).unwrap();
        let mut large = original.clone();
        large["business_summary"] = json!("x".repeat(COMPANY_PROFILE_MAX_BYTES as usize));
        large["fetched_at"] = json!("2000");
        let result = load_cn_company_profile(&path, "600909.SH", "2000", true, async {
            Ok(large.clone())
        })
        .await
        .unwrap();
        let after = std::fs::read(&path).unwrap();
        let cached = read_cn_company_profile(&path, "600909.SH");
        std::fs::remove_file(path).unwrap();
        assert!(
            result["error"].is_string(),
            "oversized cache writes must report a save error"
        );
        assert_eq!(result["profile"], large);
        assert_eq!(before, after);
        assert_eq!(cached.unwrap(), Some(original));
    }

    #[tokio::test]
    async fn cn_company_profile_replaced_cache_survives_sync_failure_and_older_completion() {
        use crate::durability::CommitPhase;
        let path = company_profile_fixture_path();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
        let mut newer = company_profile_fixture();
        newer["business_summary"] = json!("最新证券业务资料");
        newer["fetched_at"] = json!("2000");
        let older = load_cn_company_profile(&path, "600909.SH", "1000", true, async {
            started_tx.send(()).unwrap();
            finish_rx.await.unwrap();
            Ok(company_profile_fixture())
        });
        let later = async {
            started_rx.await.unwrap();
            let result = load_cn_company_profile_with_writer(
                &path,
                "600909.SH",
                "2000",
                true,
                async { Ok(newer.clone()) },
                |path, bytes| {
                    crate::durability::atomic_write_with_status_and_hook(path, bytes, |phase| {
                        if phase == CommitPhase::Replaced {
                            Err("injected directory sync failure".to_owned())
                        } else {
                            Ok(())
                        }
                    })
                },
            )
            .await
            .unwrap();
            assert_eq!(
                read_cn_company_profile(&path, "600909.SH").unwrap(),
                Some(newer.clone())
            );
            finish_tx.send(()).unwrap();
            result
        };
        let (older, later) = tokio::join!(older, later);
        let older = older.unwrap();
        let stored = read_cn_company_profile(&path, "600909.SH").unwrap();
        assert_eq!(
            stored,
            Some(newer.clone()),
            "an already replaced file must keep its publication generation"
        );
        assert_eq!(older["profile"], newer);
        assert!(
            later["error"].is_string(),
            "an unconfirmed directory sync must stay visible"
        );
        assert_eq!(
            older["error"], later["error"],
            "earlier completions must retain the durability warning"
        );

        let retry = load_cn_company_profile(&path, "600909.SH", "3000", true, async {
            Ok(newer.clone())
        })
        .await
        .unwrap();
        assert_eq!(retry["error"], Value::Null);
        assert_eq!(
            read_cn_company_profile(&path, "600909.SH").unwrap(),
            Some(newer)
        );
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn cn_company_profile_failed_write_does_not_block_older_completion() {
        use crate::durability::CommitPhase;
        let path = company_profile_fixture_path();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
        let older = load_cn_company_profile(&path, "600909.SH", "1000", true, async {
            started_tx.send(()).unwrap();
            finish_rx.await.unwrap();
            Ok(company_profile_fixture())
        });
        let later = async {
            started_rx.await.unwrap();
            let mut newer = company_profile_fixture();
            newer["fetched_at"] = json!("2000");
            let result = load_cn_company_profile_with_writer(
                &path,
                "600909.SH",
                "2000",
                true,
                async { Ok(newer) },
                |path, bytes| {
                    crate::durability::atomic_write_with_status_and_hook(path, bytes, |phase| {
                        if phase == CommitPhase::Synced {
                            Err("injected replacement failure".to_owned())
                        } else {
                            Ok(())
                        }
                    })
                },
            )
            .await
            .unwrap();
            assert!(
                !path.exists(),
                "a pre-replacement failure must preserve the original missing file"
            );
            finish_tx.send(()).unwrap();
            result
        };
        let (older, later) = tokio::join!(older, later);
        assert!(later["error"].is_string());
        assert_eq!(older.unwrap()["profile"], company_profile_fixture());
        assert_eq!(
            read_cn_company_profile(&path, "600909.SH").unwrap(),
            Some(company_profile_fixture())
        );
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn cn_company_profile_cache_byte_limit_accepts_boundary_and_refuses_one_more_byte() {
        for extra in [0, 1] {
            let path = company_profile_fixture_path();
            let mut profile = company_profile_fixture();
            profile["business_summary"] = json!("");
            let overhead = serde_json::to_vec(&profile).unwrap().len();
            profile["business_summary"] =
                json!("x".repeat(COMPANY_PROFILE_MAX_BYTES as usize - overhead + extra));
            assert_eq!(
                serde_json::to_vec(&profile).unwrap().len(),
                COMPANY_PROFILE_MAX_BYTES as usize + extra
            );
            let result = load_cn_company_profile(&path, "600909.SH", "1000", false, async {
                Ok(profile.clone())
            })
            .await
            .unwrap();
            let cached = read_cn_company_profile(&path, "600909.SH");
            let exists = path.exists();
            if exists {
                std::fs::remove_file(path).unwrap();
            }
            assert_eq!(exists, extra == 0);
            assert_eq!(result["error"].is_null(), extra == 0);
            assert_eq!(cached.unwrap(), (extra == 0).then_some(profile));
        }
    }

    fn read_reference_request(stream: &mut std::net::TcpStream) {
        use std::io::Read;
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        while !request.ends_with(b"\r\n\r\n") {
            let count = stream.read(&mut buffer).unwrap();
            assert_ne!(count, 0);
            request.extend_from_slice(&buffer[..count]);
        }
    }

    #[tokio::test]
    async fn cn_reference_read_limit_rejects_oversized_content_length_before_body() {
        use std::io::Write;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut connection, _) = listener.accept().unwrap();
            read_reference_request(&mut connection);
            write!(
                connection,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                10 * 1024 * 1024 + 1
            )
            .unwrap();
        });
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let error = reference_bytes(
            &client,
            &format!("http://{address}"),
            "https://example.com/",
        )
        .await
        .unwrap_err();
        server.join().unwrap();
        assert_eq!(error, "公开数据单页超过读取上限");
    }

    #[tokio::test]
    async fn cn_reference_read_limit_rejects_chunked_body_without_waiting_for_end() {
        use std::io::Write;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (body_sent_tx, body_sent_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut connection, _) = listener.accept().unwrap();
            read_reference_request(&mut connection);
            connection
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
            let length = 10 * 1024 * 1024 + 1;
            write!(connection, "{length:x}\r\n").unwrap();
            let chunk = [b'x'; 16 * 1024];
            let mut sent = 0;
            while sent < length {
                let count = chunk.len().min(length - sent);
                if connection.write_all(&chunk[..count]).is_err() {
                    break;
                }
                sent += count;
            }
            let _ = connection.write_all(b"\r\n");
            let _ = body_sent_tx.send(());
            let _ = finish_rx.recv_timeout(Duration::from_secs(5));
            let _ = connection.write_all(b"0\r\n\r\n");
        });
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let mut reader = tokio::spawn(async move {
            reference_bytes(
                &client,
                &format!("http://{address}"),
                "https://example.com/",
            )
            .await
        });
        body_sent_rx.await.unwrap();
        let result = tokio::time::timeout(Duration::from_secs(2), &mut reader).await;
        let _ = finish_tx.send(());
        server.join().unwrap();
        if result.is_err() {
            reader.abort();
        }
        assert!(
            result.is_ok(),
            "the byte limit must be enforced before the response terminates"
        );
        assert_eq!(
            result.unwrap().unwrap().unwrap_err(),
            "公开数据单页超过读取上限"
        );
    }
    use serde_json::json;

    #[test]
    fn cn_data_performance_supplement_accepts_reported_zero_only_for_the_same_report() {
        let mut reports = Map::from_iter([(
            "002389.SZ".to_string(),
            json!({"period":"2026Q3","currency":"CNY","report_notice_date":"2026-10-29","roe":-0.02}),
        )]);
        let wrong = vec![
            json!({"SECUCODE":"002389.SZ","REPORTDATE":"2026-03-31","NOTICE_DATE":"2026-04-29","BASIC_EPS":0.1026}),
            json!({"SECUCODE":"002389.SZ","REPORTDATE":"2026-09-30","NOTICE_DATE":"2026-10-28","BASIC_EPS":0.5}),
        ];
        assert_eq!(supplement_report_metrics(&mut reports, &wrong), 0);
        assert!(reports["002389.SZ"].get("latest_eps").is_none());
        let actual = vec![
            json!({"SECUCODE":"002389.SZ","REPORTDATE":"2026-09-30","NOTICE_DATE":"2026-10-29","BASIC_EPS":0.0,"WEIGHTAVG_ROE":null}),
        ];
        assert_eq!(supplement_report_metrics(&mut reports, &actual), 1);
        assert_eq!(reports["002389.SZ"]["latest_eps"], 0.0);
        assert_eq!(reports["002389.SZ"]["roe"], -0.02);
    }

    #[test]
    fn cn_data_calendar_uses_exchange_status_and_real_sessions_without_executing_scripts() {
        let script="function get_systemDate_global(){var systemDate_global=\"2026-10-06\";return systemDate_global;} function get_whetherTradeDate_global(){var whetherTradeDate_global=false;return whetherTradeDate_global;} function get_lastTradeDate_global(){var lastTradeDate_global=\"2026-09-30\";return lastTradeDate_global;}";
        let rows = vec![json!(["2026-09-29", "10"]), json!(["2026-09-30", "10"])];
        let calendar = parse_session_calendar(script, &rows, "20261006", "123").unwrap();
        assert_eq!(calendar["coverage_end"], "20261006");
        assert_eq!(calendar["last_trade_date"], "20260930");
        assert_eq!(calendar["trading_dates"], json!(["20260929", "20260930"]));
        assert!(parse_session_calendar(script, &rows, "20261007", "123").is_err());
        assert!(
            parse_session_calendar(script, &[json!(["2026-09-29", "10"])], "20261006", "123")
                .is_err()
        );
    }

    #[test]
    #[ignore = "manual live public-source probe against a copied cache"]
    fn cn_live_reference_probe_uses_real_public_reports_and_only_writes_a_test_artifact() {
        let input = std::env::var("GP_CN_PROBE_INPUT").expect("copied-cache JSON path");
        let output = std::env::var("GP_CN_PROBE_OUTPUT").expect("diagnostic artifact path");
        let mut seed: Value = serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
        let summary =
            tauri::async_runtime::block_on(prepare_reference_data(None, &mut seed, &json!({})))
                .unwrap();
        assert!(
            summary["directory_count"].as_u64().unwrap_or(0) > 5000,
            "{summary}"
        );
        assert!(
            summary["financial_fetched"].as_u64().unwrap_or(0) > 5000,
            "{summary}"
        );
        let (typed, _) =
            crate::market::parse_mobile_market_data_snapshot(&seed, "CN live reference probe")
                .unwrap();
        let count = typed.stocks.len();
        let financial_count = typed
            .stocks
            .iter()
            .filter(|stock| {
                typed
                    .financials
                    .get(&stock.code)
                    .is_some_and(|report| report.report_fetched_at.is_some())
            })
            .count();
        let report = json!({"summary":summary,"stock_count":count,"financial_count":financial_count,
            "roe_available":typed.stocks.iter().filter(|stock|stock.roe.is_some()).count(),
            "examples":(["000002.SZ","600519.SH","688981.SH","920085.BJ"].map(|code|json!({"code":code,"financial":seed["financials"][code],"stock":seed["stocks"].as_array().unwrap().iter().find(|stock|stock["code"]==code)})))});
        std::fs::write(&output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        std::fs::write(
            format!("{output}.seed.json"),
            serde_json::to_vec(&seed).unwrap(),
        )
        .unwrap();
        println!("{report}");
        assert!(financial_count > 5000);
    }

    #[test]
    fn cn_reference_szse_workbook_reads_a_share_columns_and_preserves_leading_zeroes() {
        let rows = parse_szse_directory(
            include_bytes!("../testdata/szse-a-share-sample.xlsx"),
            "20261108",
        )
        .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["code"], "000002.SZ");
        assert_eq!(rows[0]["name"], "万科A");
        assert_eq!(rows[1]["code"], "002129.SZ");
        assert!(parse_szse_directory(b"not a workbook", "20261108").is_err());
    }

    #[test]
    fn cn_reference_sse_directory_excludes_b_shares_delistings_and_future_listings() {
        let value = json!({"pageHelp":{"total":4},"result":[
            {"A_STOCK_CODE":"600000","SEC_NAME_CN":"浦发银行","STOCK_TYPE":"1","LIST_DATE":"19991110","DELIST_DATE":"-"},
            {"A_STOCK_CODE":"688981","SEC_NAME_CN":"中芯国际","STOCK_TYPE":"8","LIST_DATE":"20200716","DELIST_DATE":"-"},
            {"A_STOCK_CODE":"600001","SEC_NAME_CN":"历史公司","STOCK_TYPE":"1","LIST_DATE":"19991110","DELIST_DATE":"20060101"},
            {"A_STOCK_CODE":"603000","SEC_NAME_CN":"未来上市","STOCK_TYPE":"1","LIST_DATE":"20261110","DELIST_DATE":"-"}
        ]});
        let rows = parse_sse_directory(&value, "20261108").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1]["code"], "688981.SH");
        assert!(
            parse_sse_directory(&json!({"pageHelp":{"total":5},"result":[]}), "20261108").is_err()
        );
    }

    #[test]
    fn cn_reference_beijing_directory_retains_suspensions_and_excludes_old_or_future_codes() {
        let rows = vec![
            json!({"code":"920001.BJ","name":"正常公司","price":12.0,"listing_date":"20200101"}),
            json!({"code":"920002.BJ","name":"暂停交易","price":null,"listing_date":"20200101"}),
            json!({"code":"920003.BJ","name":"旧公司退","price":null,"listing_date":"20200101"}),
            json!({"code":"920004.BJ","name":"未上市","price":null,"listing_date":"20261110"}),
        ];
        assert_eq!(listed_beijing_directory(&rows, "20261108").len(), 2);
    }

    #[test]
    fn cn_reference_financials_use_security_identity_publication_date_and_explicit_units() {
        let rows = vec![
            json!({"SECUCODE":"000002.SZ","REPORT_DATE":"2026-09-30","REPORT_DATE_NAME":"2026三季报","NOTICE_DATE":"2026-10-30","CURRENCY":"CNY","EPSJB":-2.01,"BPS":7.8,"ROEJQ":-22.8,"TOTALOPERATEREVE":104998998452.2,"KCFJCXSYJLR":-21537523659.32}),
            json!({"SECUCODE":"000002.NQ","REPORT_DATE":"2026-09-30","NOTICE_DATE":"2026-10-30","EPSJB":999.0}),
            json!({"SECUCODE":"600000.SH","REPORT_DATE":"2026-09-30","NOTICE_DATE":"2026-11-10","EPSJB":999.0}),
        ];
        let codes = HashSet::from(["000002.SZ".to_string(), "600000.SH".to_string()]);
        let financials = parse_financial_reports(&rows, &codes, "20261108", "1794105983398");
        assert_eq!(financials.len(), 1);
        assert_eq!(financials["000002.SZ"]["period"], "2026Q3");
        assert_eq!(financials["000002.SZ"]["roe"], -22.8);
        assert_eq!(financials["000002.SZ"]["report_notice_date"], "2026-10-30");
        assert_eq!(
            financials["000002.SZ"]["report_fetched_at"],
            "1794105983398"
        );
        assert!(
            (financials["000002.SZ"]["deducted_net_profit_billion"]
                .as_f64()
                .unwrap()
                + 215.3752365932)
                .abs()
                < 1e-8
        );
    }

    #[test]
    fn cn_reference_projection_updates_reported_factors_and_archives_old_directory_rows() {
        let mut seed = json!({
            "stocks":[{"code":"000002.SZ","name":"万科A","industry":"房地产","price":6.0,"roe":0.15,"latest_eps":-0.5},
                {"code":"000003.SZ","name":"旧代码","industry":"历史","price":9.0}],
            "histories":{"000003.SZ":[{"date":"20000101","close":9.0}]},
            "financials":{"000002.SZ":{"period":"2026Q1","latest_eps":-0.5}}
        });
        let directory = vec![
            json!({"code":"000002.SZ","name":"万科A","industry":"房地产","price":6.08,"listing_date":"19910129"}),
        ];
        let financials = Map::from_iter([(
            "000002.SZ".to_string(),
            json!({"period":"2026Q3","latest_eps":-2.01,"roe":-22.8,"source":"eastmoney_cn_financial","report_notice_date":"2026-10-30"}),
        )]);
        apply_reference_data(&mut seed, &directory, financials, "1794105983398");
        assert_eq!(seed["stocks"].as_array().unwrap().len(), 1);
        assert_eq!(seed["stocks"][0]["roe"], -0.228);
        assert_eq!(seed["stocks"][0]["latest_eps"], -2.01);
        assert_eq!(seed["stocks"][0]["period"], "2026Q3");
        assert_eq!(seed["archived_stocks"]["000003.SZ"]["price"], 9.0);
        assert_eq!(seed["histories"]["000003.SZ"][0]["close"], 9.0);
        let mut codes = directory_candidate_codes(&seed).unwrap();
        seed["stocks"]
            .as_array_mut()
            .unwrap()
            .push(json!({"code":"000004.SZ"}));
        assert_eq!(codes, directory_candidate_codes(&seed).unwrap());
        codes.sort();
        assert_eq!(codes, vec!["000002.SZ"]);
    }
}
