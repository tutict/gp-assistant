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
    $node = Resolve-CommandPath "node" "Install Node.js and retry."
    Invoke-Checked "Frontend JavaScript syntax" $node @("--check", "app/static/app.js")
}

if (-not $SkipPrepare) {
    $prepareScript = Join-Path $Root "scripts/prepare-tauri-android-assets.ps1"
    Invoke-Checked "Prepare Tauri frontend assets" "powershell.exe" @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $prepareScript)
}

if (-not $SkipRust) {
    $cargo = Resolve-CommandPath "cargo" "Install the Rust stable toolchain and retry."
    Invoke-Checked "Rust gp-core tests" $cargo @("test", "--manifest-path", "native/gp-core/Cargo.toml")
    Invoke-Checked "Tauri cargo check" $cargo @("check", "--manifest-path", "desktop/src-tauri/Cargo.toml")
}

if (-not $SkipAndroidPreflight) {
    $androidScript = Join-Path $Root "scripts/build-android.ps1"
    Invoke-Checked "Android build environment preflight" "powershell.exe" @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $androidScript, "-PreflightOnly")
}

Write-Host ""
Write-Step "Release checks completed without Python/FastAPI requirements."
