[CmdletBinding()]
param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]] $ForwardArgs
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $PSScriptRoot
$TauriScript = Join-Path $PSScriptRoot 'start-tauri-dev.ps1'

if (-not (Test-Path -LiteralPath $TauriScript)) {
    throw "Tauri launcher was not found: $TauriScript"
}

Write-Host '[gp-dev] Tauri is the default dev path; forwarding to scripts/start-tauri-dev.ps1.' -ForegroundColor Cyan
if ($ForwardArgs -and $ForwardArgs.Count -gt 0) {
    & $TauriScript @ForwardArgs
} else {
    & $TauriScript -RunPreflight -NoWatch
}
exit $LASTEXITCODE
