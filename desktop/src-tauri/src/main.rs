#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    env,
    net::{SocketAddr, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_shell::{process::CommandChild, ShellExt};

const APP_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 8010;

struct BackendState(Mutex<Option<BackendProcess>>);

enum BackendProcess {
    Python(Child),
    Sidecar(Option<CommandChild>),
}

#[tauri::command]
fn core_screen(payload: serde_json::Value) -> Result<serde_json::Value, String> {
    gp_core::screen_value(payload).map_err(|error| error.to_string())
}

#[tauri::command]
fn core_screen_with_data(payload: serde_json::Value) -> Result<serde_json::Value, String> {
    gp_core::screen_with_data_value(payload).map_err(|error| error.to_string())
}

#[tauri::command]
fn core_graph_screen(payload: serde_json::Value) -> Result<serde_json::Value, String> {
    gp_core::graph_screen_value(payload).map_err(|error| error.to_string())
}

#[tauri::command]
fn core_graph_screen_with_data(payload: serde_json::Value) -> Result<serde_json::Value, String> {
    gp_core::graph_screen_with_data_value(payload).map_err(|error| error.to_string())
}

#[tauri::command]
fn core_backtest(payload: serde_json::Value) -> Result<serde_json::Value, String> {
    gp_core::backtest_value(payload).map_err(|error| error.to_string())
}

#[tauri::command]
fn core_backtest_with_data(payload: serde_json::Value) -> Result<serde_json::Value, String> {
    gp_core::backtest_with_data_value(payload).map_err(|error| error.to_string())
}

#[tauri::command]
fn core_agent(payload: serde_json::Value) -> Result<serde_json::Value, String> {
    gp_core::agent_value(payload).map_err(|error| error.to_string())
}

#[tauri::command]
fn core_agent_with_data(payload: serde_json::Value) -> Result<serde_json::Value, String> {
    gp_core::agent_with_data_value(payload).map_err(|error| error.to_string())
}

#[tauri::command]
fn core_validate_data_source(payload: serde_json::Value) -> Result<serde_json::Value, String> {
    gp_core::validate_data_source_value(payload).map_err(|error| error.to_string())
}

impl BackendProcess {
    fn kill(&mut self) {
        match self {
            BackendProcess::Python(child) => {
                let _ = child.kill();
            }
            BackendProcess::Sidecar(child) => {
                if let Some(child) = child.take() {
                    let _ = child.kill();
                }
            }
        }
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(BackendState(Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![
            core_screen,
            core_screen_with_data,
            core_graph_screen,
            core_graph_screen_with_data,
            core_backtest,
            core_backtest_with_data,
            core_agent,
            core_agent_with_data,
            core_validate_data_source
        ])
        .setup(|app| {
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
        })
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                stop_backend(&window.app_handle());
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running GP Assistant");
}

fn backend_port() -> u16 {
    env::var("GP_ASSISTANT_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT)
}

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
                env::var("STOCK_PROVIDER").unwrap_or_else(|_| "mock".to_string()),
            )
            .spawn()
            .map_err(shell_error)?
            .1;
        Ok(BackendProcess::Sidecar(Some(child)))
    } else {
        start_python_backend(port).map(BackendProcess::Python)
    }
}

fn shell_error(error: tauri_plugin_shell::Error) -> tauri::Error {
    tauri::Error::Anyhow(anyhow::Error::new(error))
}

fn should_use_sidecar() -> bool {
    env::var("GP_ASSISTANT_USE_SIDECAR")
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(!cfg!(debug_assertions))
}

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
            env::var("STOCK_PROVIDER").unwrap_or_else(|_| "mock".to_string()),
        )
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    command.spawn().map_err(Into::into)
}

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
