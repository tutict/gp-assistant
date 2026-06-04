[CmdletBinding()]
param(
    [ValidateSet("mock", "akshare", "eastmoney", "astock")]
    [string]$Provider = "mock",

    [int]$Port = 8010,

    [switch]$NoInstall,
    [switch]$NoOpen,
    [switch]$NoSmoke,
    [switch]$NoWait
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
$HostName = "127.0.0.1"
$BaseUrl = "http://${HostName}:$Port"
$HealthUrl = "$BaseUrl/health"
$LogDir = Join-Path $RepoRoot "logs"
$RunLog = Join-Path $LogDir "test-server.launch.log"

function Write-Step {
    param([string]$Message)
    Write-Host "[gp-test] $Message" -ForegroundColor Cyan
}

function Write-Warn {
    param([string]$Message)
    Write-Host "[gp-test] WARN: $Message" -ForegroundColor Yellow
}

function Test-Healthy {
    param([string]$Url)
    try {
        $response = Invoke-WebRequest -UseBasicParsing -Uri $Url -TimeoutSec 2
        return $response.StatusCode -eq 200
    } catch {
        return $false
    }
}

function Get-PythonPath {
    if ($env:GP_ASSISTANT_PYTHON -and (Test-Path -LiteralPath $env:GP_ASSISTANT_PYTHON)) {
        return (Resolve-Path -LiteralPath $env:GP_ASSISTANT_PYTHON).Path
    }

    $cpythonVenv = Join-Path $RepoRoot ".venv-cpython\Scripts\python.exe"
    if (Test-Path -LiteralPath $cpythonVenv) {
        return $cpythonVenv
    }

    $venvPython = Join-Path $RepoRoot ".venv\Scripts\python.exe"
    if (Test-Path -LiteralPath $venvPython) {
        return $venvPython
    }

    $systemPython = Get-Command python -ErrorAction SilentlyContinue
    if (-not $systemPython) {
        throw "Python was not found. Install Python 3.11+ or set GP_ASSISTANT_PYTHON."
    }

    Write-Step "Creating .venv with system Python..."
    & $systemPython.Source -m venv (Join-Path $RepoRoot ".venv")
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to create .venv."
    }

    return $venvPython
}

function Ensure-Dependencies {
    param([string]$Python)

    & $Python -c "import fastapi, uvicorn" *> $null
    if ($LASTEXITCODE -eq 0) {
        Write-Step "Python dependencies are available."
        return
    }

    if ($NoInstall) {
        throw "Python dependencies are missing. Re-run without -NoInstall."
    }

    Write-Step "Installing Python dependencies from requirements.txt..."
    & $Python -m pip install -r (Join-Path $RepoRoot "requirements.txt")
    if ($LASTEXITCODE -ne 0) {
        throw "pip install failed."
    }
}

function Run-SmokeTests {
    param([string]$Url, [string]$Source)

    Write-Step "Running smoke tests..."
    Invoke-WebRequest -UseBasicParsing -Uri "$Url/" -TimeoutSec 5 | Out-Null
    Invoke-RestMethod -Uri "$Url/api/data-sources" -Headers @{ "X-Stock-Provider" = $Source } -TimeoutSec 5 | Out-Null

    $screenBody = @{
        min_roe = 0.05
        max_pe = 40
        limit = 10
        sort_by = "score"
        sort_dir = "desc"
    } | ConvertTo-Json

    $screen = Invoke-RestMethod `
        -Method Post `
        -Uri "$Url/api/screen" `
        -Headers @{ "X-Stock-Provider" = $Source } `
        -ContentType "application/json" `
        -Body $screenBody `
        -TimeoutSec 10

    $sectorBody = @{
        criteria = @{
            min_roe = 0.05
            max_pe = 40
            limit = 50
            sort_by = "score"
            sort_dir = "desc"
        }
        max_sectors = 4
        per_sector_limit = 2
        min_sector_candidates = 1
    } | ConvertTo-Json -Depth 4

    $sectorScreen = Invoke-RestMethod `
        -Method Post `
        -Uri "$Url/api/sector-screen" `
        -Headers @{ "X-Stock-Provider" = $Source } `
        -ContentType "application/json" `
        -Body $sectorBody `
        -TimeoutSec 10

    if ($sectorScreen.sector_count -lt 1) {
        throw "Sector screen smoke test returned no sectors."
    }

    Write-Step "Smoke tests passed. Screen result: $($screen.returned)/$($screen.total); sector result: $($sectorScreen.returned)/$($sectorScreen.sector_count)."
}

function Start-TestServer {
    param(
        [string]$Python,
        [string]$WorkingDirectory
    )

    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $Python
    $psi.Arguments = "-m app.desktop_server"
    $psi.WorkingDirectory = $WorkingDirectory
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true

    return [System.Diagnostics.Process]::Start($psi)
}

function Open-TestUrl {
    param([string]$Url)

    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $Url
    $psi.UseShellExecute = $true
    [System.Diagnostics.Process]::Start($psi) | Out-Null
}

Set-Location -LiteralPath $RepoRoot
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null

$Python = Get-PythonPath
Write-Step "Using Python: $Python"
Ensure-Dependencies -Python $Python

$node = Get-Command node -ErrorAction SilentlyContinue
if ($node) {
    Write-Step "Checking frontend JavaScript syntax..."
    & $node.Source --check (Join-Path $RepoRoot "app\static\app.js")
    if ($LASTEXITCODE -ne 0) {
        throw "Frontend JavaScript syntax check failed."
    }
} else {
    Write-Warn "Node.js was not found; skipping frontend syntax check."
}

$serverProcess = $null
$startedHere = $false

if (Test-Healthy -Url $HealthUrl) {
    Write-Step "Server is already running at $BaseUrl."
} else {
    Write-Step "Starting FastAPI test server on $BaseUrl with provider=$Provider..."
    @(
        "Started: $(Get-Date -Format o)"
        "Python: $Python"
        "URL: $BaseUrl"
        "Provider: $Provider"
    ) | Set-Content -LiteralPath $RunLog -Encoding UTF8

    $env:STOCK_PROVIDER = $Provider
    $env:GP_ASSISTANT_HOST = $HostName
    $env:GP_ASSISTANT_PORT = [string]$Port
    $env:PYTHONUTF8 = "1"
    $env:PYTHONIOENCODING = "utf-8"

    $serverProcess = Start-TestServer -Python $Python -WorkingDirectory $RepoRoot
    $startedHere = $true

    $ready = $false
    for ($i = 0; $i -lt 45; $i++) {
        Start-Sleep -Milliseconds 500
        if ($serverProcess.HasExited) {
            break
        }
        if (Test-Healthy -Url $HealthUrl) {
            $ready = $true
            break
        }
    }

    if (-not $ready) {
        if ($serverProcess -and -not $serverProcess.HasExited) {
            Stop-Process -Id $serverProcess.Id -Force
        }
        Write-Host ""
        Write-Warn "Server failed to become healthy. Check whether port $Port is occupied."
        throw "Test server startup failed."
    }

    Write-Step "Server is healthy. PID=$($serverProcess.Id)."
}

if (-not $NoSmoke) {
    Run-SmokeTests -Url $BaseUrl -Source $Provider
}

Write-Step "Test UI: $BaseUrl/#sectionAgent"
Write-Step "Launch log: $RunLog"

if (-not $NoOpen) {
    Write-Step "Opening browser..."
    Open-TestUrl "$BaseUrl/#sectionAgent"
}

if ($NoWait) {
    if ($startedHere -and $serverProcess -and -not $serverProcess.HasExited) {
        Stop-Process -Id $serverProcess.Id
        Write-Step "NoWait enabled. Stopped test server after smoke tests."
    } else {
        Write-Step "NoWait enabled. Exiting."
    }
    exit 0
}

if ($startedHere -and $serverProcess) {
    Write-Host ""
    Read-Host "Press Enter to stop the test server"
    if (-not $serverProcess.HasExited) {
        Stop-Process -Id $serverProcess.Id
        Write-Step "Stopped test server PID=$($serverProcess.Id)."
    }
} else {
    Write-Host ""
    Read-Host "Press Enter to exit"
}
