[CmdletBinding()]
param(
    [switch] $SkipAndroidPreflight,
    [switch] $SkipRust,
    [switch] $SkipNode
)

$ErrorActionPreference = "Stop"
$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

function Write-Step {
    param([string] $Message)
    Write-Host "[release-check] $Message" -ForegroundColor Cyan
}

function Resolve-Python {
    $candidates = @(
        (Join-Path $Root ".venv-cpython/Scripts/python.exe"),
        (Join-Path $Root ".venv/Scripts/python.exe")
    )
    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate) {
            return $candidate
        }
    }

    $python = Get-Command python -ErrorAction SilentlyContinue
    if ($python) {
        return $python.Source
    }

    throw "未找到 Python。请先创建 .venv-cpython 或将 Python 加入 PATH。"
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
        throw "$Name 不可用。$InstallHint"
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

$python = Resolve-Python
Invoke-Checked "Python compileall" $python @("-m", "compileall", "-q", "app", "scripts", "tests")
Invoke-Checked "Python pytest" $python @("-m", "pytest", "-q", "-p", "no:cacheprovider")

if (-not $SkipNode) {
    $node = Resolve-CommandPath "node" "请安装 Node.js，然后重试。"
    Invoke-Checked "Frontend JavaScript syntax" $node @("--check", "app/static/app.js")
}

if (-not $SkipRust) {
    $cargo = Resolve-CommandPath "cargo" "请安装 Rust stable toolchain，然后重试。"
    Invoke-Checked "Rust gp-core tests" $cargo @("test", "--manifest-path", "native/gp-core/Cargo.toml")
    Invoke-Checked "Tauri cargo check" $cargo @("check", "--manifest-path", "desktop/src-tauri/Cargo.toml")
}

if (-not $SkipAndroidPreflight) {
    $androidScript = Join-Path $Root "scripts/build-android.ps1"
    Invoke-Checked "Android build environment preflight" "powershell.exe" @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $androidScript, "-PreflightOnly")
}

Write-Host ""
Write-Step "Release checks completed."
