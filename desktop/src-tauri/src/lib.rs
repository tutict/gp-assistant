use base64::{engine::general_purpose, Engine as _};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::Manager;

#[cfg(not(mobile))]
use std::{
    env,
    net::{SocketAddr, TcpStream},
    process::{Child, Command, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

#[cfg(not(mobile))]
use tauri::{WebviewUrl, WebviewWindowBuilder};
#[cfg(not(mobile))]
use tauri_plugin_shell::{process::CommandChild, ShellExt};

#[cfg(not(mobile))]
const APP_HOST: &str = "127.0.0.1";
#[cfg(not(mobile))]
const DEFAULT_PORT: u16 = 8010;
#[cfg(not(mobile))]
const BACKEND_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

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
        .expect("error while running GP Assistant");
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
    .title("GP Assistant")
    .inner_size(1280.0, 860.0)
    .min_inner_size(960.0, 680.0)
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
