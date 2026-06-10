use base64::{engine::general_purpose, Engine as _};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::Manager;

#[cfg(not(mobile))]
use std::{
    env,
    net::{SocketAddr, TcpStream},
    process::{Child, Command, Stdio},
    sync::Mutex,
    thread,
    time::Instant,
};

#[cfg(not(mobile))]
use tauri::{webview::PageLoadEvent, WebviewUrl, WebviewWindowBuilder};
#[cfg(not(mobile))]
use tauri_plugin_shell::{process::CommandChild, ShellExt};

#[cfg(not(mobile))]
const APP_HOST: &str = "127.0.0.1";
#[cfg(not(mobile))]
const DEFAULT_PORT: u16 = 8010;
#[cfg(not(mobile))]
const BACKEND_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

const MOBILE_MARKET_DATA_FILE: &str = "mobile-market-data.json";
const TENCENT_QUOTE_ENDPOINT: &str = "https://qt.gtimg.cn/q=";
const TENCENT_BATCH_SIZE: usize = 180;
const TENCENT_DEFAULT_MAX_CANDIDATES: usize = 16_000;

#[cfg(not(mobile))]
struct BackendState(Mutex<Option<BackendProcess>>);

#[cfg(not(mobile))]
enum BackendProcess {
    Python(Child),
    Sidecar(Option<CommandChild>),
}

#[tauri::command]
fn core_screen(payload: Value) -> Result<Value, String> {
    gp_core::screen_value(payload).map_err(|error| error.to_string())
}

#[tauri::command]
fn core_screen_with_data(payload: Value) -> Result<Value, String> {
    gp_core::screen_with_data_value(payload).map_err(|error| error.to_string())
}

#[tauri::command]
fn core_graph_screen(payload: Value) -> Result<Value, String> {
    gp_core::graph_screen_value(payload).map_err(|error| error.to_string())
}

#[tauri::command]
fn core_graph_screen_with_data(payload: Value) -> Result<Value, String> {
    gp_core::graph_screen_with_data_value(payload).map_err(|error| error.to_string())
}

#[tauri::command]
fn core_backtest(payload: Value) -> Result<Value, String> {
    gp_core::backtest_value(payload).map_err(|error| error.to_string())
}

#[tauri::command]
fn core_backtest_with_data(payload: Value) -> Result<Value, String> {
    gp_core::backtest_with_data_value(payload).map_err(|error| error.to_string())
}

#[tauri::command]
fn core_trend(payload: Value) -> Result<Value, String> {
    gp_core::trend_value(payload).map_err(|error| error.to_string())
}

#[tauri::command]
fn core_trend_with_data(payload: Value) -> Result<Value, String> {
    gp_core::trend_with_data_value(payload).map_err(|error| error.to_string())
}

#[tauri::command]
fn core_trend_screen(payload: Value) -> Result<Value, String> {
    gp_core::trend_screen_value(payload).map_err(|error| error.to_string())
}

#[tauri::command]
fn core_trend_screen_with_data(payload: Value) -> Result<Value, String> {
    gp_core::trend_screen_with_data_value(payload).map_err(|error| error.to_string())
}

#[tauri::command]
fn core_agent(payload: Value) -> Result<Value, String> {
    gp_core::agent_value(payload).map_err(|error| error.to_string())
}

#[tauri::command]
fn core_agent_with_data(payload: Value) -> Result<Value, String> {
    gp_core::agent_with_data_value(payload).map_err(|error| error.to_string())
}

#[tauri::command]
fn core_mobile_stock_skill(payload: Value) -> Result<Value, String> {
    gp_core::mobile_stock_skill_value(payload).map_err(|error| error.to_string())
}

#[tauri::command]
fn core_validate_data_source(payload: Value) -> Result<Value, String> {
    gp_core::validate_data_source_value(payload).map_err(|error| error.to_string())
}

#[tauri::command]
fn core_mobile_market_data_read(app: tauri::AppHandle) -> Result<Value, String> {
    read_mobile_market_data(&app)
}

#[tauri::command]
fn core_mobile_market_data_write(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    write_mobile_market_data(&app, payload)
}

#[tauri::command]
fn core_mobile_market_data_clear(app: tauri::AppHandle) -> Result<Value, String> {
    let path = mobile_market_data_path(&app)?;
    if !path.exists() {
        return Ok(json!({
            "removed": false,
            "removed_bytes": 0,
            "notes": ["mobile market cache is already empty"]
        }));
    }
    let removed_bytes = fs::metadata(&path).map(|metadata| metadata.len()).unwrap_or(0);
    fs::remove_file(&path).map_err(|error| {
        format!(
            "remove mobile market cache failed: {}: {error}",
            path.display()
        )
    })?;
    Ok(json!({
        "removed": true,
        "removed_bytes": removed_bytes,
        "notes": ["mobile market cache removed"]
    }))
}

#[tauri::command]
async fn core_mobile_market_data_refresh_tencent(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let seed = payload.get("seed").cloned().unwrap_or_else(|| json!({}));
    let scan_candidates = payload
        .get("scan_candidates")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let use_previous_close = payload
        .get("use_previous_close")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let max_candidates = payload
        .get("max_candidates")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or(TENCENT_DEFAULT_MAX_CANDIDATES);

    let refresh =
        refresh_tencent_market_data(seed, scan_candidates, max_candidates, use_previous_close)
            .await?;
    let cache = write_mobile_market_data(&app, refresh.dataset)?;
    Ok(json!({
        "refreshed": true,
        "source": "tencent",
        "requested": refresh.requested,
        "fetched": refresh.fetched,
        "failed_batches": refresh.failed_batches,
        "status": cache,
        "notes": [
            format!(
                "Tencent quote refresh finished: fetched {} of {} candidates",
                refresh.fetched, refresh.requested
            )
        ]
    }))
}

#[tauri::command]
fn core_upstream_rag_import(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let manifest_value = payload
        .get("manifest")
        .cloned()
        .ok_or_else(|| "缺少 manifest。".to_string())?;
    let mut manifest = match manifest_value {
        Value::Object(map) => map,
        _ => return Err("manifest 必须是 JSON 对象。".to_string()),
    };
    let pack_base64 = payload
        .get("pack_base64")
        .and_then(Value::as_str)
        .ok_or_else(|| "缺少 pack_base64。".to_string())?;
    let pack_bytes = general_purpose::STANDARD
        .decode(pack_base64)
        .map_err(|error| format!("RAG 包解码失败：{error}"))?;

    let expected_sha256 = manifest
        .get("sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| "manifest 缺少 sha256。".to_string())?;
    let actual_sha256 = sha256_hex(&pack_bytes);
    if !expected_sha256.eq_ignore_ascii_case(&actual_sha256) {
        return Err(format!(
            "RAG 包 sha256 校验失败：预期 {expected_sha256}，实际 {actual_sha256}。"
        ));
    }
    if let Some(expected_size) = manifest.get("file_size").and_then(Value::as_u64) {
        if expected_size != pack_bytes.len() as u64 {
            return Err(format!(
                "RAG 包大小校验失败：预期 {expected_size} 字节，实际 {} 字节。",
                pack_bytes.len()
            ));
        }
    }

    let stock_code = manifest
        .get("target_stock_code")
        .and_then(Value::as_str)
        .ok_or_else(|| "manifest 缺少 target_stock_code。".to_string())?;
    let pack_version = manifest
        .get("pack_version")
        .and_then(Value::as_str)
        .ok_or_else(|| "manifest 缺少 pack_version。".to_string())?;
    let stock_code_owned = stock_code.to_string();
    let pack_version_owned = pack_version.to_string();

    let root = upstream_rag_mobile_root(&app)?;
    let stock_dir = root.join(sanitize_path_part(&stock_code_owned));
    fs::create_dir_all(&stock_dir).map_err(|error| format!("创建 RAG 目录失败：{error}"))?;

    let version_dir = stock_dir.join(sanitize_path_part(&pack_version_owned));
    let tmp_dir = stock_dir.join(format!(
        "{}.tmp-{}",
        sanitize_path_part(&pack_version_owned),
        epoch_millis()
    ));
    if tmp_dir.exists() {
        fs::remove_dir_all(&tmp_dir).map_err(|error| format!("清理临时目录失败：{error}"))?;
    }
    fs::create_dir_all(&tmp_dir).map_err(|error| format!("创建临时目录失败：{error}"))?;

    let tmp_pack_path = tmp_dir.join("rag_pack.sqlite");
    let tmp_manifest_path = tmp_dir.join("manifest.json");
    fs::write(&tmp_pack_path, &pack_bytes).map_err(|error| format!("写入 RAG 包失败：{error}"))?;
    manifest.insert(
        "_local_pack_path".to_string(),
        json!(version_dir.join("rag_pack.sqlite").display().to_string()),
    );
    manifest.insert(
        "_local_manifest_path".to_string(),
        json!(version_dir.join("manifest.json").display().to_string()),
    );
    manifest.insert("_imported_at_epoch_ms".to_string(), json!(epoch_millis()));
    fs::write(
        &tmp_manifest_path,
        serde_json::to_vec_pretty(&Value::Object(manifest.clone()))
            .map_err(|error| format!("序列化 manifest 失败：{error}"))?,
    )
    .map_err(|error| format!("写入 manifest 失败：{error}"))?;

    if version_dir.exists() {
        fs::remove_dir_all(&version_dir).map_err(|error| format!("替换旧版本目录失败：{error}"))?;
    }
    fs::rename(&tmp_dir, &version_dir).map_err(|error| format!("提交 RAG 包失败：{error}"))?;

    let current_manifest_path = stock_dir.join("current_manifest.json");
    let previous_manifest_path = stock_dir.join("previous_manifest.json");
    if current_manifest_path.exists() {
        fs::copy(&current_manifest_path, &previous_manifest_path)
            .map_err(|error| format!("保存回滚 manifest 失败：{error}"))?;
    }
    fs::write(
        &current_manifest_path,
        serde_json::to_vec_pretty(&Value::Object(manifest.clone()))
            .map_err(|error| format!("序列化 current manifest 失败：{error}"))?,
    )
    .map_err(|error| format!("更新 current manifest 失败：{error}"))?;

    Ok(json!({
        "imported": true,
        "root": root.display().to_string(),
        "stock_code": stock_code_owned,
        "pack_version": pack_version_owned,
        "manifest": Value::Object(manifest),
        "notes": ["已校验 sha256 并完成原子替换。"]
    }))
}

#[tauri::command]
fn core_upstream_rag_list(app: tauri::AppHandle) -> Result<Value, String> {
    let root = upstream_rag_mobile_root(&app)?;
    let mut packs = Vec::new();
    if root.exists() {
        for stock_entry in
            fs::read_dir(&root).map_err(|error| format!("读取 RAG 目录失败：{error}"))?
        {
            let stock_entry =
                stock_entry.map_err(|error| format!("读取 RAG 子目录失败：{error}"))?;
            let stock_dir = stock_entry.path();
            if !stock_dir.is_dir() {
                continue;
            }
            let current_version = read_json_file(&stock_dir.join("current_manifest.json"))
                .ok()
                .and_then(|value| {
                    value
                        .get("pack_version")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
            for version_entry in fs::read_dir(&stock_dir)
                .map_err(|error| format!("读取 RAG 版本目录失败：{error}"))?
            {
                let version_entry =
                    version_entry.map_err(|error| format!("读取 RAG 版本失败：{error}"))?;
                let version_dir = version_entry.path();
                if !version_dir.is_dir() {
                    continue;
                }
                if let Ok(mut manifest) = read_json_file(&version_dir.join("manifest.json")) {
                    let pack_version = manifest
                        .get("pack_version")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    if let Value::Object(ref mut map) = manifest {
                        map.insert(
                            "current".to_string(),
                            json!(Some(pack_version.clone()) == current_version),
                        );
                    }
                    packs.push(manifest);
                }
            }
        }
    }
    let notes = if packs.is_empty() {
        vec!["安卓端尚未导入上下游 RAG 包。"]
    } else {
        Vec::<&str>::new()
    };
    Ok(json!({
        "root": root.display().to_string(),
        "packs": packs,
        "notes": notes
    }))
}

#[tauri::command]
fn core_upstream_rag_detail(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let root = upstream_rag_mobile_root(&app)?;
    let stock_code = payload
        .get("stock_code")
        .and_then(Value::as_str)
        .unwrap_or("");
    let pack_version = payload
        .get("pack_version")
        .and_then(Value::as_str)
        .unwrap_or("");
    let manifest_path = if !stock_code.is_empty() && !pack_version.is_empty() {
        root.join(sanitize_path_part(stock_code))
            .join(sanitize_path_part(pack_version))
            .join("manifest.json")
    } else if !stock_code.is_empty() {
        root.join(sanitize_path_part(stock_code))
            .join("current_manifest.json")
    } else {
        find_first_current_manifest(&root)
            .ok_or_else(|| "安卓端尚未导入上下游 RAG 包。".to_string())?
    };
    let manifest = read_json_file(&manifest_path)?;
    Ok(json!({
        "manifest": manifest,
        "notes": []
    }))
}

#[tauri::command]
fn core_upstream_rag_rollback(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let root = upstream_rag_mobile_root(&app)?;
    let stock_code = payload
        .get("stock_code")
        .and_then(Value::as_str)
        .ok_or_else(|| "缺少 stock_code。".to_string())?;
    let stock_dir = root.join(sanitize_path_part(stock_code));
    let current_manifest_path = stock_dir.join("current_manifest.json");
    let previous_manifest_path = stock_dir.join("previous_manifest.json");
    if !previous_manifest_path.exists() {
        return Err("没有可回滚的上一个 RAG 包。".to_string());
    }
    let current_bytes = fs::read(&current_manifest_path).ok();
    let previous_bytes = fs::read(&previous_manifest_path)
        .map_err(|error| format!("读取回滚 manifest 失败：{error}"))?;
    fs::write(&current_manifest_path, previous_bytes)
        .map_err(|error| format!("恢复 current manifest 失败：{error}"))?;
    if let Some(bytes) = current_bytes {
        fs::write(&previous_manifest_path, bytes)
            .map_err(|error| format!("更新 previous manifest 失败：{error}"))?;
    }
    let manifest = read_json_file(&current_manifest_path)?;
    Ok(json!({
        "rolled_back": true,
        "manifest": manifest,
        "notes": ["已切换到上一个 RAG 包。"]
    }))
}

struct TencentRefreshResult {
    dataset: Value,
    requested: usize,
    fetched: usize,
    failed_batches: usize,
}

async fn refresh_tencent_market_data(
    seed: Value,
    scan_candidates: bool,
    max_candidates: usize,
    use_previous_close: bool,
) -> Result<TencentRefreshResult, String> {
    let (seed_stocks, seed_codes) = seed_stock_maps(&seed);
    let mut candidate_codes = seed_codes;
    if scan_candidates {
        append_tencent_candidate_codes(&mut candidate_codes);
    }
    dedupe_stock_codes(&mut candidate_codes);
    if candidate_codes.len() > max_candidates {
        candidate_codes.truncate(max_candidates);
    }
    if candidate_codes.is_empty() {
        return Err("mobile market refresh has no candidate stock codes".to_string());
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("Mozilla/5.0 GP-Assistant/0.2 mobile")
        .build()
        .map_err(|error| format!("create Tencent HTTP client failed: {error}"))?;

    let mut stocks = Vec::new();
    let mut seen = HashSet::new();
    let mut failed_batches = 0usize;
    for batch in candidate_codes.chunks(TENCENT_BATCH_SIZE) {
        match fetch_tencent_quotes(&client, batch).await {
            Ok(text) => {
                for mut stock in parse_tencent_quotes(&text, &seed_stocks, use_previous_close) {
                    let Some(code) = stock
                        .get("code")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                    else {
                        continue;
                    };
                    if seen.insert(code) {
                        stocks.push(Value::Object(std::mem::take(&mut stock)));
                    }
                }
            }
            Err(_) => {
                failed_batches += 1;
            }
        }
    }
    if stocks.is_empty() {
        return Err("Tencent quote refresh returned no valid stocks".to_string());
    }
    stocks.sort_by(|left, right| {
        left.get("code")
            .and_then(Value::as_str)
            .unwrap_or("")
            .cmp(right.get("code").and_then(Value::as_str).unwrap_or(""))
    });
    let valid_codes: HashSet<String> = stocks
        .iter()
        .filter_map(|stock| stock.get("code").and_then(Value::as_str).map(str::to_string))
        .collect();

    let dataset = json!({
        "source": "tencent",
        "generated_at": epoch_millis().to_string(),
        "generated_at_epoch_ms": epoch_millis(),
        "notes": [
            "mobile online refresh via Tencent quote",
            "industry and slow-changing metrics are merged from the previous local dataset",
            if use_previous_close {
                "price policy: previous close before market close"
            } else {
                "price policy: latest Tencent quote"
            }
        ],
        "stocks": stocks,
        "relations": filter_seed_relations(&seed, &valid_codes),
        "histories": filter_seed_histories(&seed, &valid_codes)
    });

    Ok(TencentRefreshResult {
        requested: candidate_codes.len(),
        fetched: valid_codes.len(),
        failed_batches,
        dataset,
    })
}

async fn fetch_tencent_quotes(client: &reqwest::Client, codes: &[String]) -> Result<String, String> {
    let symbols: Vec<String> = codes
        .iter()
        .filter_map(|code| tencent_symbol(code))
        .collect();
    if symbols.is_empty() {
        return Ok(String::new());
    }
    let url = format!("{TENCENT_QUOTE_ENDPOINT}{}", symbols.join(","));
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("Tencent quote request failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("Tencent quote HTTP {}", response.status()));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("Tencent quote body read failed: {error}"))?;
    let (text, _, _) = encoding_rs::GBK.decode(&bytes);
    Ok(text.into_owned())
}

fn parse_tencent_quotes(
    text: &str,
    seed_stocks: &HashMap<String, serde_json::Map<String, Value>>,
    use_previous_close: bool,
) -> Vec<serde_json::Map<String, Value>> {
    let mut stocks = Vec::new();
    for raw_line in text.split(';') {
        let line = raw_line.trim();
        if line.is_empty() || !line.contains('=') || !line.contains('"') {
            continue;
        }
        let Some(left) = line.split('=').next() else {
            continue;
        };
        let key = left.rsplit('_').next().unwrap_or("").trim();
        let Some(code) = normalize_stock_code(key) else {
            continue;
        };
        let values: Vec<&str> = line
            .split('"')
            .nth(1)
            .unwrap_or("")
            .split('~')
            .collect();
        if values.len() < 53 {
            continue;
        }
        let name = values.get(1).map(|value| value.trim()).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let price = if use_previous_close {
            parse_number(values.get(4))
                .filter(|value| *value > 0.0)
                .or_else(|| parse_number(values.get(3)).filter(|value| *value > 0.0))
        } else {
            parse_number(values.get(3))
                .filter(|value| *value > 0.0)
                .or_else(|| parse_number(values.get(4)).filter(|value| *value > 0.0))
        };
        let Some(price) = price else {
            continue;
        };

        let existing = seed_stocks.get(&code);
        let mut stock = existing.cloned().unwrap_or_default();
        let pe = first_positive_number(&[values.get(39), values.get(52)])
            .or_else(|| existing.and_then(|object| object_f64(object, "pe")));
        let pb = parse_number(values.get(46))
            .filter(|value| *value > 0.0)
            .or_else(|| existing.and_then(|object| object_f64(object, "pb")));
        let market_cap = parse_number(values.get(44))
            .filter(|value| *value > 0.0)
            .or_else(|| existing.and_then(|object| object_f64(object, "market_cap_billion")));
        let roe = existing
            .and_then(|object| object_f64(object, "roe"))
            .or_else(|| estimate_roe(pe, pb));
        let industry = existing
            .and_then(|object| object_string(object, "industry"))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| board_label(&code).to_string());
        let is_st = name.to_ascii_uppercase().contains("ST")
            || existing
                .and_then(|object| object.get("is_st"))
                .and_then(Value::as_bool)
                .unwrap_or(false);

        stock.insert("code".to_string(), json!(code));
        stock.insert("name".to_string(), json!(name));
        stock.insert("industry".to_string(), json!(industry));
        stock.insert("is_st".to_string(), json!(is_st));
        stock.insert("price".to_string(), json!(price));
        stock.insert("pe".to_string(), json!(pe));
        stock.insert("pb".to_string(), json!(pb));
        stock.insert("roe".to_string(), json!(roe));
        stock.insert("market_cap_billion".to_string(), json!(market_cap));
        stock.insert(
            "dividend_yield".to_string(),
            json!(existing.and_then(|object| object_f64(object, "dividend_yield"))),
        );
        stocks.push(stock);
    }
    stocks
}

fn seed_stock_maps(seed: &Value) -> (HashMap<String, serde_json::Map<String, Value>>, Vec<String>) {
    let mut stocks = HashMap::new();
    let mut codes = Vec::new();
    if let Some(items) = seed.get("stocks").and_then(Value::as_array) {
        for item in items {
            let Some(object) = item.as_object() else {
                continue;
            };
            let Some(code) = object
                .get("code")
                .and_then(Value::as_str)
                .and_then(normalize_stock_code)
            else {
                continue;
            };
            codes.push(code.clone());
            stocks.insert(code, object.clone());
        }
    }
    (stocks, codes)
}

fn append_tencent_candidate_codes(codes: &mut Vec<String>) {
    append_range(codes, "SZ", 1, 3999);
    append_range(codes, "SZ", 300000, 301999);
    append_range(codes, "SH", 600000, 605999);
    append_range(codes, "SH", 688000, 689999);
    append_range(codes, "BJ", 920000, 920999);
}

fn append_range(codes: &mut Vec<String>, market: &str, start: u32, end: u32) {
    for value in start..=end {
        codes.push(format!("{value:06}.{market}"));
    }
}

fn dedupe_stock_codes(codes: &mut Vec<String>) {
    let mut seen = HashSet::new();
    codes.retain(|code| seen.insert(code.clone()));
}

fn tencent_symbol(code: &str) -> Option<String> {
    let normalized = normalize_stock_code(code)?;
    let digits = &normalized[..6];
    if normalized.ends_with(".SH") {
        Some(format!("sh{digits}"))
    } else if normalized.ends_with(".BJ") {
        Some(format!("bj{digits}"))
    } else {
        Some(format!("sz{digits}"))
    }
}

fn normalize_stock_code(value: &str) -> Option<String> {
    let raw = value.trim().to_ascii_uppercase();
    if raw.is_empty() {
        return None;
    }
    if let Some(digits) = raw.strip_prefix("SH").filter(|digits| valid_digits(digits)) {
        return Some(format!("{digits}.SH"));
    }
    if let Some(digits) = raw.strip_prefix("SZ").filter(|digits| valid_digits(digits)) {
        return Some(format!("{digits}.SZ"));
    }
    if let Some(digits) = raw.strip_prefix("BJ").filter(|digits| valid_digits(digits)) {
        return Some(format!("{digits}.BJ"));
    }
    if let Some((digits, market)) = raw.split_once('.') {
        if valid_digits(digits) && matches!(market, "SH" | "SZ" | "BJ") {
            return Some(format!("{digits}.{market}"));
        }
    }
    let digits: String = raw.chars().filter(|ch| ch.is_ascii_digit()).take(6).collect();
    if !valid_digits(&digits) {
        return None;
    }
    Some(format!("{}.{}", digits, infer_market(&digits)))
}

fn valid_digits(value: &str) -> bool {
    value.len() == 6 && value.chars().all(|ch| ch.is_ascii_digit())
}

fn infer_market(digits: &str) -> &'static str {
    if digits.starts_with('6') || digits.starts_with('9') || digits.starts_with('5') {
        "SH"
    } else if digits.starts_with('4') || digits.starts_with('8') {
        "BJ"
    } else {
        "SZ"
    }
}

fn board_label(code: &str) -> &'static str {
    let digits = &code[..6];
    if code.ends_with(".BJ") {
        "Beijing Stock Exchange"
    } else if digits.starts_with("688") {
        "STAR Market"
    } else if digits.starts_with("300") || digits.starts_with("301") {
        "ChiNext"
    } else if code.ends_with(".SH") {
        "Shanghai A Share"
    } else {
        "Shenzhen A Share"
    }
}

fn parse_number(value: Option<&&str>) -> Option<f64> {
    let raw = value?.trim();
    if raw.is_empty() || matches!(raw, "-" | "None" | "nan") {
        return None;
    }
    raw.parse::<f64>().ok().filter(|value| value.is_finite())
}

fn first_positive_number(values: &[Option<&&str>]) -> Option<f64> {
    values
        .iter()
        .find_map(|value| parse_number(*value).filter(|number| *number > 0.0))
}

fn estimate_roe(pe: Option<f64>, pb: Option<f64>) -> Option<f64> {
    let pe = pe.filter(|value| *value > 0.0)?;
    let pb = pb.filter(|value| *value > 0.0)?;
    Some(pb / pe)
}

fn object_f64(object: &serde_json::Map<String, Value>, key: &str) -> Option<f64> {
    object
        .get(key)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

fn object_string(object: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn filter_seed_relations(seed: &Value, valid_codes: &HashSet<String>) -> Value {
    let mut relations = Vec::new();
    if let Some(items) = seed.get("relations").and_then(Value::as_array) {
        for item in items {
            let source = item
                .get("source_code")
                .and_then(Value::as_str)
                .and_then(normalize_stock_code);
            let target = item
                .get("target_code")
                .and_then(Value::as_str)
                .and_then(normalize_stock_code);
            if source
                .as_ref()
                .map(|code| valid_codes.contains(code))
                .unwrap_or(false)
                && target
                    .as_ref()
                    .map(|code| valid_codes.contains(code))
                    .unwrap_or(false)
            {
                relations.push(item.clone());
            }
        }
    }
    Value::Array(relations)
}

fn filter_seed_histories(seed: &Value, valid_codes: &HashSet<String>) -> Value {
    let mut histories = serde_json::Map::new();
    if let Some(items) = seed.get("histories").and_then(Value::as_object) {
        for (raw_code, history) in items {
            let Some(code) = normalize_stock_code(raw_code) else {
                continue;
            };
            if valid_codes.contains(&code) {
                histories.insert(code, history.clone());
            }
        }
    }
    Value::Object(histories)
}

fn read_mobile_market_data(app: &tauri::AppHandle) -> Result<Value, String> {
    let path = mobile_market_data_path(app)?;
    if !path.exists() {
        return Ok(json!({
            "exists": false,
            "bytes": 0,
            "path": path.display().to_string(),
            "notes": ["mobile market cache is empty"]
        }));
    }
    let data = read_json_file(&path)?;
    let summary = gp_core::validate_data_source_value(data.clone())
        .map_err(|error| format!("cached mobile market data is invalid: {error}"))?;
    let bytes = fs::metadata(&path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    Ok(json!({
        "exists": true,
        "bytes": bytes,
        "path": path.display().to_string(),
        "updated_at_epoch_ms": file_modified_millis(&path),
        "summary": summary,
        "data": data,
        "notes": ["mobile market cache loaded"]
    }))
}

fn write_mobile_market_data(app: &tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let summary = gp_core::validate_data_source_value(payload.clone())
        .map_err(|error| format!("mobile market data validation failed: {error}"))?;
    let path = mobile_market_data_path(app)?;
    let root = path
        .parent()
        .ok_or_else(|| "mobile market cache path has no parent".to_string())?;
    fs::create_dir_all(root)
        .map_err(|error| format!("create mobile market cache dir failed: {error}"))?;
    let tmp_path = root.join(format!("{}.tmp-{}", MOBILE_MARKET_DATA_FILE, epoch_millis()));
    let bytes = serde_json::to_vec(&payload)
        .map_err(|error| format!("serialize mobile market data failed: {error}"))?;
    fs::write(&tmp_path, &bytes)
        .map_err(|error| format!("write mobile market cache temp failed: {error}"))?;
    if path.exists() {
        fs::remove_file(&path)
            .map_err(|error| format!("replace old mobile market cache failed: {error}"))?;
    }
    fs::rename(&tmp_path, &path)
        .map_err(|error| format!("commit mobile market cache failed: {error}"))?;
    Ok(json!({
        "exists": true,
        "bytes": bytes.len(),
        "path": path.display().to_string(),
        "updated_at_epoch_ms": file_modified_millis(&path),
        "summary": summary,
        "data": payload,
        "notes": ["mobile market cache written"]
    }))
}

fn mobile_market_data_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("get app data dir failed: {error}"))?;
    root.push("market");
    root.push(MOBILE_MARKET_DATA_FILE);
    Ok(root)
}

fn file_modified_millis(path: &Path) -> Option<u128> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis())
}

fn upstream_rag_mobile_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("获取应用数据目录失败：{error}"))?;
    root.push("upstream_rag");
    Ok(root)
}

fn read_json_file(path: &Path) -> Result<Value, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("读取 JSON 失败：{}：{error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("解析 JSON 失败：{}：{error}", path.display()))
}

fn find_first_current_manifest(root: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path().join("current_manifest.json");
        if path.exists() {
            return Some(path);
        }
    }
    None
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn sanitize_path_part(value: &str) -> String {
    let mut part: String = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
        .take(120)
        .collect();
    if part.is_empty() {
        part.push_str("unknown");
    }
    part
}

#[cfg(not(mobile))]
impl BackendProcess {
    fn kill(&mut self) {
        match self {
            BackendProcess::Python(child) => {
                let _ = child.kill();
                let _ = child.wait();
            }
            BackendProcess::Sidecar(child) => {
                if let Some(child) = child.take() {
                    let pid = child.pid();
                    let _ = child.kill();
                    wait_for_process_exit(pid, BACKEND_SHUTDOWN_TIMEOUT);
                }
            }
        }
    }
}

#[cfg(not(mobile))]
fn wait_for_process_exit(pid: u32, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !process_is_running(pid) {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(all(not(mobile), windows))]
fn process_is_running(pid: u32) -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .any(|part| part == pid.to_string())
        })
        .unwrap_or(false)
}

#[cfg(all(not(mobile), unix))]
fn process_is_running(pid: u32) -> bool {
    let proc_path = PathBuf::from(format!("/proc/{pid}"));
    proc_path.exists()
        || Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
}

#[cfg(all(not(mobile), not(any(windows, unix))))]
fn process_is_running(_pid: u32) -> bool {
    false
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().invoke_handler(tauri::generate_handler![
        core_screen,
        core_screen_with_data,
        core_graph_screen,
        core_graph_screen_with_data,
        core_backtest,
        core_backtest_with_data,
        core_trend,
        core_trend_with_data,
        core_trend_screen,
        core_trend_screen_with_data,
        core_agent,
        core_agent_with_data,
        core_mobile_stock_skill,
        core_validate_data_source,
        core_mobile_market_data_read,
        core_mobile_market_data_write,
        core_mobile_market_data_clear,
        core_mobile_market_data_refresh_tencent,
        core_upstream_rag_import,
        core_upstream_rag_list,
        core_upstream_rag_detail,
        core_upstream_rag_rollback
    ]);

    #[cfg(not(mobile))]
    let builder = builder
        .plugin(tauri_plugin_shell::init())
        .manage(BackendState(Mutex::new(None)))
        .setup(|app| setup_desktop(app).map_err(Into::into))
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                stop_backend(&window.app_handle());
            }
        });

    #[cfg(mobile)]
    let builder = builder.setup(|_| Ok(()));

    builder
        .run(tauri::generate_context!())
        .expect("error while running A股选股智能体");
}

#[cfg(not(mobile))]
fn setup_desktop(app: &mut tauri::App) -> tauri::Result<()> {
    let port = backend_port();
    let backend_url = format!("http://{APP_HOST}:{port}");
    let process = start_backend(app, port)?;
    *app.state::<BackendState>()
        .0
        .lock()
        .expect("backend lock poisoned") = Some(process);

    wait_for_backend(port)?;

    let window = WebviewWindowBuilder::new(
        app,
        "main",
        WebviewUrl::External(backend_url.parse().expect("valid backend URL")),
    )
    .title("A股选股智能体")
    .inner_size(1280.0, 860.0)
    .min_inner_size(960.0, 680.0)
    .visible(false)
    .on_page_load(|window, payload| {
        if matches!(payload.event(), PageLoadEvent::Finished) {
            let _ = window.show();
            let _ = window.set_focus();
        }
    })
    .build()?;

    let app_handle = app.handle().clone();
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
            stop_backend(&app_handle);
        }
    });

    Ok(())
}

#[cfg(not(mobile))]
fn backend_port() -> u16 {
    env::var("GP_ASSISTANT_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT)
}

#[cfg(not(mobile))]
fn start_backend(app: &tauri::App, port: u16) -> tauri::Result<BackendProcess> {
    if should_use_sidecar() {
        let child = app
            .shell()
            .sidecar("gp-assistant-backend")
            .map_err(shell_error)?
            .env("GP_ASSISTANT_HOST", APP_HOST)
            .env("GP_ASSISTANT_PORT", port.to_string())
            .env(
                "STOCK_PROVIDER",
                env::var("STOCK_PROVIDER").unwrap_or_else(|_| "tdx".to_string()),
            )
            .spawn()
            .map_err(shell_error)?
            .1;
        Ok(BackendProcess::Sidecar(Some(child)))
    } else {
        start_python_backend(port).map(BackendProcess::Python)
    }
}

#[cfg(not(mobile))]
fn shell_error(error: tauri_plugin_shell::Error) -> tauri::Error {
    tauri::Error::Anyhow(anyhow::Error::new(error))
}

#[cfg(not(mobile))]
fn should_use_sidecar() -> bool {
    env::var("GP_ASSISTANT_USE_SIDECAR")
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(!cfg!(debug_assertions))
}

#[cfg(not(mobile))]
fn start_python_backend(port: u16) -> tauri::Result<Child> {
    let root = repo_root();
    let python = python_path(&root);

    let mut command = Command::new(python);
    command
        .current_dir(root)
        .arg("-m")
        .arg("uvicorn")
        .arg("app.main:app")
        .arg("--host")
        .arg(APP_HOST)
        .arg("--port")
        .arg(port.to_string())
        .env("GP_ASSISTANT_PORT", port.to_string())
        .env(
            "STOCK_PROVIDER",
            env::var("STOCK_PROVIDER").unwrap_or_else(|_| "tdx".to_string()),
        )
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    command.spawn().map_err(Into::into)
}

#[cfg(not(mobile))]
fn repo_root() -> PathBuf {
    if let Ok(root) = env::var("GP_ASSISTANT_ROOT") {
        return PathBuf::from(root);
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|desktop| desktop.parent())
        .map(PathBuf::from)
        .expect("src-tauri should live under desktop")
}

#[cfg(not(mobile))]
fn python_path(root: &PathBuf) -> PathBuf {
    if let Ok(python) = env::var("GP_ASSISTANT_PYTHON") {
        return PathBuf::from(python);
    }

    let candidates = [
        root.join(".venv-cpython")
            .join("Scripts")
            .join("python.exe"),
        root.join(".venv").join("Scripts").join("python.exe"),
    ];

    candidates
        .into_iter()
        .find(|candidate| candidate.exists())
        .unwrap_or_else(|| PathBuf::from("python"))
}

#[cfg(not(mobile))]
fn wait_for_backend(port: u16) -> tauri::Result<()> {
    let address: SocketAddr = format!("{APP_HOST}:{port}")
        .parse()
        .expect("valid backend socket address");
    let deadline = Instant::now() + Duration::from_secs(30);

    while Instant::now() < deadline {
        if TcpStream::connect_timeout(&address, Duration::from_millis(250)).is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(250));
    }

    Err(tauri::Error::Anyhow(anyhow::anyhow!(
        "Timed out waiting for backend at {address}"
    )))
}

#[cfg(not(mobile))]
fn stop_backend(app: &tauri::AppHandle) {
    if let Some(mut process) = app
        .state::<BackendState>()
        .0
        .lock()
        .expect("backend lock poisoned")
        .take()
    {
        process.kill();
    }
}
