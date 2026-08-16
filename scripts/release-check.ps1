[CmdletBinding()]
param(
    [switch] $SkipAndroidPreflight,
    [switch] $SkipRust,
    [switch] $SkipNode,
    [switch] $SkipPrepare
)

$ErrorActionPreference = "Stop"
$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

function Write-Step {
    param([string] $Message)
    Write-Host "[release-check] $Message" -ForegroundColor Cyan
}

function Resolve-CommandPath {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Name,
        [Parameter(Mandatory = $true)]
        [string] $InstallHint
    )

    $command = Get-Command $Name -ErrorAction SilentlyContinue
    if (-not $command) {
        throw "$Name is unavailable. $InstallHint"
    }
    return $command.Source
}

function Invoke-Checked {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Description,
        [Parameter(Mandatory = $true)]
        [string] $FilePath,
        [Parameter(Mandatory = $true)]
        [string[]] $Arguments,
        [string] $WorkingDirectory = $Root
    )

    Write-Step $Description
    Write-Host "  cwd: $WorkingDirectory"
    Write-Host "  cmd: $FilePath $($Arguments -join ' ')"
    Push-Location $WorkingDirectory
    try {
        & $FilePath @Arguments
        if ($LASTEXITCODE -ne 0) {
            throw "$Description failed with exit code $LASTEXITCODE."
        }
    } finally {
        Pop-Location
    }
}

if (-not $SkipNode) {
    $npm = Resolve-CommandPath "npm.cmd" "Install Node.js/npm and retry."
    $frontendDir = Join-Path $Root "desktop/frontend"
    if (-not (Test-Path -LiteralPath (Join-Path $frontendDir "node_modules"))) {
        Invoke-Checked "Install frontend dependencies" $npm @("ci") $frontendDir
    }
    Invoke-Checked "Release version metadata consistency" $npm @("run", "test:version") $frontendDir
    Invoke-Checked "Frontend UI density guard" $npm @("run", "test:density") $frontendDir
    Invoke-Checked "Frontend UI density contract tests" $npm @("run", "test:density-contract") $frontendDir
    Invoke-Checked "Frontend Agent replay CSS contract tests" $npm @("run", "test:agent-replay-css") $frontendDir
    Invoke-Checked "Frontend CSS architecture guard" $npm @("run", "test:architecture") $frontendDir
    Invoke-Checked "Frontend unit tests" $npm @("run", "test:unit") $frontendDir
    Invoke-Checked "Frontend React/TypeScript build" $npm @("run", "build") $frontendDir
    Invoke-Checked "Frontend desktop visual and shortcut harness" $npm @("run", "test:desktop:built") $frontendDir
}

if (-not $SkipPrepare) {
    $prepareScript = Join-Path $Root "scripts/prepare-tauri-android-assets.ps1"
    Invoke-Checked "Prepare Tauri frontend assets" "powershell.exe" @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $prepareScript)
}

if (-not $SkipRust) {
    $cargo = Resolve-CommandPath "cargo" "Install the Rust stable toolchain and retry."
    Invoke-Checked "Rust gp-core format check" $cargo @("fmt", "--manifest-path", "native/gp-core/Cargo.toml", "--", "--check")
    Invoke-Checked "Tauri Rust format check" $cargo @("fmt", "--manifest-path", "desktop/src-tauri/Cargo.toml", "--", "--check")
    Invoke-Checked "Rust gp-core tests" $cargo @("test", "--locked", "--manifest-path", "native/gp-core/Cargo.toml")
    Invoke-Checked "Tauri Rust tests" $cargo @("test", "--locked", "--manifest-path", "desktop/src-tauri/Cargo.toml")
    Invoke-Checked "Tauri cargo check" $cargo @("check", "--locked", "--manifest-path", "desktop/src-tauri/Cargo.toml")
}

if (-not $SkipAndroidPreflight) {
    $androidScript = Join-Path $Root "scripts/build-android.ps1"
    Invoke-Checked "Android build environment preflight" "powershell.exe" @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $androidScript, "-PreflightOnly")
}

Write-Host ""
Write-Step "Release checks completed for the Tauri/Rust runtime."
