use crate::market_domain::codes::normalize_stock_code;
use crate::market_domain::datasets::apply_market_data_patch;
use crate::market_domain::datasets::market_data_patch_for_codes;
use crate::market_domain::dates::epoch_millis;
use crate::wire_values::sanitize_path_part;
use serde_json::json;
use serde_json::Value;
use sha2::Digest;
use sha2::Sha256;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use tauri::Manager;

pub(crate) const MOBILE_MARKET_DATA_FILE: &str = "mobile-market-data.json";

pub(crate) const MOBILE_MARKET_PATCH_DIR: &str = "mobile-market-data-patches";

pub(crate) const MOBILE_MARKET_WRITE_RETRY_ATTEMPTS: usize = 3;

pub(crate) const MOBILE_MARKET_WRITE_RETRY_DELAY_MS: u64 = 50;

pub(crate) fn retry_with_attempts<T, F>(
    attempts: usize,
    delay_ms: u64,
    mut op: F,
) -> Result<T, String>
where
    F: FnMut(usize) -> Result<T, String>,
{
    let attempts = attempts.max(1);
    let mut last_error: Option<String> = None;
    for attempt in 0..attempts {
        match op(attempt) {
            Ok(value) => return Ok(value),
            Err(error) => {
                last_error = Some(error);
                if attempt + 1 < attempts {
                    std::thread::sleep(Duration::from_millis(delay_ms));
                }
            }
        }
    }
    Err(last_error.unwrap_or_else(|| "operation failed".to_string()))
}

pub(crate) fn write_mobile_market_data_with_retry(
    tmp_path: &Path,
    path: &Path,
    bytes: &[u8],
) -> Result<(), String> {
    retry_with_attempts(
        MOBILE_MARKET_WRITE_RETRY_ATTEMPTS,
        MOBILE_MARKET_WRITE_RETRY_DELAY_MS,
        |_| write_mobile_market_data_once(tmp_path, path, bytes),
    )
}

pub(crate) fn write_mobile_market_data_once(
    _tmp_path: &Path,
    path: &Path,
    bytes: &[u8],
) -> Result<(), String> {
    crate::durability::atomic_write_json(path, bytes)
}

pub(crate) fn mobile_market_patch_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let path = mobile_market_data_path(app)?;
    let root = path
        .parent()
        .ok_or_else(|| "mobile market cache path has no parent".to_string())?;
    Ok(root.join(MOBILE_MARKET_PATCH_DIR))
}

pub(crate) fn apply_persisted_market_data_patches(
    app: &tauri::AppHandle,
    data: &mut Value,
) -> Result<usize, String> {
    let root = mobile_market_patch_dir(app)?;
    if !root.exists() {
        return Ok(0);
    }
    let mut paths = fs::read_dir(&root)
        .map_err(|error| format!("read mobile market patches failed: {error}"))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    paths.sort();
    let mut applied = 0usize;
    for path in paths {
        let patch = read_json_file(&path)?;
        apply_market_data_patch(data, &patch);
        applied += 1;
    }
    Ok(applied)
}

pub(crate) fn clear_mobile_market_data_patches(app: &tauri::AppHandle) -> Result<(), String> {
    let root = mobile_market_patch_dir(app)?;
    if root.exists() {
        fs::remove_dir_all(&root)
            .map_err(|error| format!("clear mobile market patches failed: {error}"))?;
    }
    Ok(())
}

pub(crate) fn market_data_patch_codes(patch: &Value) -> Vec<String> {
    let mut codes = HashSet::new();
    if let Some(stocks) = patch.get("stocks").and_then(Value::as_array) {
        codes.extend(stocks.iter().filter_map(|stock| {
            stock
                .get("code")
                .and_then(Value::as_str)
                .and_then(normalize_stock_code)
        }));
    }
    for section in [
        "histories",
        "financials",
        "factor_snapshots",
        "capital_evidence",
    ] {
        if let Some(items) = patch.get(section).and_then(Value::as_object) {
            codes.extend(items.keys().filter_map(|code| normalize_stock_code(code)));
        }
    }
    if let Some(ranges) = patch
        .pointer("/cn_reference/history_ranges")
        .and_then(Value::as_object)
    {
        codes.extend(ranges.keys().filter_map(|code| normalize_stock_code(code)));
    }
    let mut codes = codes.into_iter().collect::<Vec<_>>();
    codes.sort();
    codes
}

pub(crate) fn persist_market_data_patch_sync(
    app: &tauri::AppHandle,
    patch: &Value,
) -> Result<usize, String> {
    let root = mobile_market_patch_dir(app)?;
    fs::create_dir_all(&root)
        .map_err(|error| format!("create mobile market patch dir failed: {error}"))?;
    let mut written = 0usize;
    for code in market_data_patch_codes(patch) {
        let safe_code = sanitize_path_part(&code);
        let path = root.join(format!("{safe_code}.json"));
        let mut merged_patch = if path.exists() {
            read_json_file(&path)?
        } else {
            json!({})
        };
        let code_patch = market_data_patch_for_codes(patch, std::slice::from_ref(&code));
        apply_market_data_patch(&mut merged_patch, &code_patch);
        let bytes = serde_json::to_vec(&merged_patch)
            .map_err(|error| format!("serialize mobile market patch failed: {error}"))?;
        let tmp_path = root.join(format!("{safe_code}.tmp-{}", epoch_millis()));
        write_mobile_market_data_once(&tmp_path, &path, &bytes)?;
        written += 1;
    }
    Ok(written)
}

pub(crate) fn mobile_market_data_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("get app data dir failed: {error}"))?;
    root.push("market");
    root.push(MOBILE_MARKET_DATA_FILE);
    Ok(root)
}

pub(in crate::market) fn sha256_file(path: &Path) -> Result<String, std::io::Error> {
    use std::io::Read;

    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub(crate) fn read_json_file(path: &Path) -> Result<Value, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("读取 JSON 失败：{}：{error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("解析 JSON 失败：{}：{error}", path.display()))
}

pub(crate) fn find_first_current_manifest(root: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path().join("current_manifest.json");
        if path.exists() {
            return Some(path);
        }
    }
    None
}
