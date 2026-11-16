#[cfg(windows)]
use crate::wire_values::truncate_for_note;
#[cfg(windows)]
use base64::engine::general_purpose;
#[cfg(windows)]
use base64::Engine;
#[cfg(not(mobile))]
use std::process::Command;

#[cfg(windows)]
pub(crate) fn powershell_http_get_bytes(url: &str, timeout_secs: u64) -> Result<Vec<u8>, String> {
    let timeout_ms = timeout_secs.saturating_mul(1000).max(1000).to_string();
    let script = r#"& {
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$u = $env:GP_HTTP_URL
$timeoutMs = [int]$env:GP_HTTP_TIMEOUT_MS
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$request = [System.Net.WebRequest]::Create($u)
$request.Method = 'GET'
$request.Timeout = $timeoutMs
$request.ReadWriteTimeout = $timeoutMs
if ($request -is [System.Net.HttpWebRequest]) {
  $request.UserAgent = 'Mozilla/5.0 GuXuanYou/0.3 financial'
  $request.Accept = '*/*'
  $request.AutomaticDecompression = [Net.DecompressionMethods]::GZip -bor [Net.DecompressionMethods]::Deflate
}
$response = $request.GetResponse()
try {
  $stream = $response.GetResponseStream()
  $memory = New-Object System.IO.MemoryStream
  $stream.CopyTo($memory)
  [Convert]::ToBase64String($memory.ToArray())
} finally {
  if ($stream) { $stream.Dispose() }
  if ($response) { $response.Dispose() }
}
}"#;
    let mut command = Command::new("powershell.exe");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command
        .env("GP_HTTP_URL", url)
        .env("GP_HTTP_TIMEOUT_MS", timeout_ms.as_str())
        .args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output()
        .map_err(|error| format!("PowerShell HTTP 启动失败：{error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if !stderr.trim().is_empty() {
            stderr.trim()
        } else {
            stdout.trim()
        };
        return Err(format!(
            "PowerShell HTTP 失败：{}",
            truncate_for_note(detail, 240)
        ));
    }
    let encoded = String::from_utf8_lossy(&output.stdout);
    let encoded = encoded.trim();
    if encoded.is_empty() {
        return Err("PowerShell HTTP 返回空响应".to_string());
    }
    general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| format!("PowerShell HTTP 响应解码失败：{error}"))
}

#[cfg(not(windows))]
pub(crate) fn powershell_http_get_bytes(_url: &str, _timeout_secs: u64) -> Result<Vec<u8>, String> {
    Err("PowerShell HTTP fallback only runs on Windows".to_string())
}

#[cfg(windows)]
pub(crate) fn powershell_http_get_bytes_with_headers(
    url: &str,
    timeout_secs: u64,
    user_agent: &str,
    referer: &str,
) -> Result<Vec<u8>, String> {
    let timeout_ms = timeout_secs.saturating_mul(1000).max(1000).to_string();
    let script = r#"& {
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$u = $env:GP_HTTP_URL
$timeoutMs = [int]$env:GP_HTTP_TIMEOUT_MS
$ua = $env:GP_HTTP_UA
$referer = $env:GP_HTTP_REFERER
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$request = [System.Net.WebRequest]::Create($u)
$request.Method = 'GET'
$request.Timeout = $timeoutMs
$request.ReadWriteTimeout = $timeoutMs
if ($request -is [System.Net.HttpWebRequest]) {
  $request.UserAgent = $ua
  $request.Accept = 'text/html,application/json,text/plain,*/*'
  if ($referer) { $request.Referer = $referer }
  $request.AutomaticDecompression = [Net.DecompressionMethods]::GZip -bor [Net.DecompressionMethods]::Deflate
}
$response = $request.GetResponse()
try {
  $stream = $response.GetResponseStream()
  $memory = New-Object System.IO.MemoryStream
  $stream.CopyTo($memory)
  [Convert]::ToBase64String($memory.ToArray())
} finally {
  if ($stream) { $stream.Dispose() }
  if ($response) { $response.Dispose() }
}
}"#;
    let mut command = Command::new("powershell.exe");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command
        .env("GP_HTTP_URL", url)
        .env("GP_HTTP_TIMEOUT_MS", timeout_ms.as_str())
        .env("GP_HTTP_UA", user_agent)
        .env("GP_HTTP_REFERER", referer)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output()
        .map_err(|error| format!("PowerShell HTTP 启动失败：{error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if !stderr.trim().is_empty() {
            stderr.trim()
        } else {
            stdout.trim()
        };
        return Err(format!(
            "PowerShell HTTP 失败：{}",
            truncate_for_note(detail, 240)
        ));
    }
    let encoded = String::from_utf8_lossy(&output.stdout);
    let encoded = encoded.trim();
    if encoded.is_empty() {
        return Err("PowerShell HTTP 返回空响应".to_string());
    }
    general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| format!("PowerShell HTTP 响应解码失败：{error}"))
}

#[cfg(not(windows))]
pub(crate) fn powershell_http_get_bytes_with_headers(
    _url: &str,
    _timeout_secs: u64,
    _user_agent: &str,
    _referer: &str,
) -> Result<Vec<u8>, String> {
    Err("PowerShell HTTP fallback only runs on Windows".to_string())
}
