[CmdletBinding()]
param(
    [ValidateSet("tdx")]
    [string]$Provider = "tdx",

    [ValidateRange(1, 65535)]
    [int]$Port = 8010,

    [ValidateSet("critical", "error", "warning", "info", "debug", "trace")]
    [string]$LogLevel = "info",

    [switch]$Reload,
    [switch]$NoReload,
    [switch]$NoInstall,
    [switch]$Open,
    [switch]$SkipSyntaxCheck,
    [switch]$DiagnosticsOnly
)

# Foreground dev launcher: runs the FastAPI backend in THIS console so all
# logs, tracebacks and request lines stream live to the screen. Use this when
# you want to watch for errors. Press Ctrl+C to stop.

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
$HostName = "127.0.0.1"
$BaseUrl = "http://${HostName}:$Port"
$HealthUrl = "$BaseUrl/health"
$ScriptStartedAt = Get-Date

if ($Reload -and $NoReload) {
    Write-Host "[gp-dev] ERROR: Use either -Reload or -NoReload, not both." -ForegroundColor Red
    exit 2
}

# Reload is the default for local development. Keep -Reload as a backwards
# compatible explicit flag, and use -NoReload when debugging single-process
# startup behavior.
$ReloadEnabled = -not [bool]$NoReload

function Write-Step {
    param([string]$Message)
    Write-Host "[gp-dev] $Message" -ForegroundColor Cyan
}

function Write-Warn {
    param([string]$Message)
    Write-Host "[gp-dev] WARN: $Message" -ForegroundColor Yellow
}

function Write-Info {
    param([string]$Message)
    Write-Host "[gp-dev] INFO: $Message" -ForegroundColor DarkGray
}

function Write-FailHint {
    param([string]$Message)
    Write-Host "[gp-dev] HINT: $Message" -ForegroundColor Magenta
}

function Write-Section {
    param([string]$Title)
    Write-Host ""
    Write-Step $Title
}

function Get-RedactedValue {
    param([object]$Value)
    $text = [string]$Value
    if ([string]::IsNullOrWhiteSpace($text)) { return "-" }
    $text = $text -replace "(?i)(api[_-]?key|token|password|passwd|secret)=([^;&\s]+)", '$1=[redacted]'
    $text = $text -replace "(?i)(https?://)([^:@/\s]+):([^@/\s]+)@", '$1$2:[redacted]@'
    return Get-ShortValue $text
}

function Invoke-TextCommand {
    param(
        [string]$FilePath,
        [string[]]$Arguments
    )
    try {
        $output = & $FilePath @Arguments 2>&1
        if ($LASTEXITCODE -eq 0) {
            return ($output -join "`n").Trim()
        }
        return "failed(exit=$LASTEXITCODE): $($output -join ' ')"
    } catch {
        return "failed: $($_.Exception.Message)"
    }
}

function Write-CommandProbe {
    param([string]$Name)
    $command = Get-Command $Name -ErrorAction SilentlyContinue
    if ($command) {
        Write-Host ("  {0}: {1}" -f $Name, $command.Source)
    } else {
        Write-Host ("  {0}: not found" -f $Name)
    }
}

function Write-SystemDiagnostics {
    Write-Section "System diagnostics:"
    Write-Host "  Started: $($ScriptStartedAt.ToString('o'))"
    Write-Host "  PowerShell: $($PSVersionTable.PSVersion) $($PSVersionTable.PSEdition)"
    Write-Host "  Execution policy: process $(Get-ExecutionPolicy -Scope Process), current-user $(Get-ExecutionPolicy -Scope CurrentUser)"
    Write-Host "  Process: PID=$PID 64-bit=$([Environment]::Is64BitProcess)"
    Write-Host "  User: $env:USERNAME@$env:COMPUTERNAME"
    Write-Host "  Working directory: $(Get-Location)"
    Write-Host "  Console encoding: input=$([Console]::InputEncoding.WebName) output=$([Console]::OutputEncoding.WebName)"
    Write-Host "  PATH length: $($env:PATH.Length)"
    Write-Host "  Commands:"
    Write-CommandProbe "python"
    Write-CommandProbe "py"
    Write-CommandProbe "node"
    Write-CommandProbe "git"
}

function Write-FileProbe {
    param(
        [string]$Label,
        [string]$Path
    )
    if (Test-Path -LiteralPath $Path) {
        $item = Get-Item -LiteralPath $Path
        $kind = if ($item.PSIsContainer) { "dir" } else { "file" }
        $size = if ($item.PSIsContainer) { "-" } else { $item.Length }
        Write-Host "  ${Label}: $kind path=$Path size=$size modified=$($item.LastWriteTime.ToString('o'))"
    } else {
        Write-Host "  ${Label}: missing path=$Path"
    }
}

function Write-ProjectFileDiagnostics {
    Write-Section "Project file diagnostics:"
    Write-FileProbe "requirements" (Join-Path $RepoRoot "requirements.txt")
    Write-FileProbe "env file" (Join-Path $RepoRoot ".env")
    Write-FileProbe "static index" (Join-Path $RepoRoot "app\static\index.html")
    Write-FileProbe "frontend app" (Join-Path $RepoRoot "app\static\app.js")
    Write-FileProbe "cache dir" (Join-Path $RepoRoot "data\cache")
    $cacheRoot = Join-Path $RepoRoot "data\cache"
    if (Test-Path -LiteralPath $cacheRoot) {
        $files = @(Get-ChildItem -LiteralPath $cacheRoot -File -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 8)
        if ($files.Count) {
            Write-Host "  recent cache files:"
            foreach ($file in $files) {
                Write-Host "    - $($file.Name) size=$($file.Length) modified=$($file.LastWriteTime.ToString('o'))"
            }
        }
    }
}

function Write-NodeDiagnostics {
    Write-Section "Frontend diagnostics:"
    $node = Get-Command node -ErrorAction SilentlyContinue
    if (-not $node) {
        Write-Warn "Node.js was not found; frontend syntax checks will be skipped."
        return
    }
    Write-Host "  node: $($node.Source)"
    Write-Host "  node version: $(Invoke-TextCommand -FilePath $node.Source -Arguments @('--version'))"
    $appJs = Join-Path $RepoRoot "app\static\app.js"
    if (Test-Path -LiteralPath $appJs) {
        $item = Get-Item -LiteralPath $appJs
        Write-Host "  app.js: $($item.FullName) size=$($item.Length) modified=$($item.LastWriteTime.ToString('o'))"
    }
}

function Get-ShortValue {
    param([object]$Value)
    $text = [string]$Value
    if ([string]::IsNullOrWhiteSpace($text)) { return "-" }
    if ($text.Length -le 140) { return $text }
    return $text.Substring(0, 137) + "..."
}

function Value-OrDefault {
    param(
        [object]$Value,
        [string]$Default
    )
    if ([string]::IsNullOrWhiteSpace([string]$Value)) { return $Default }
    return $Value
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

function Get-PortOwners {
    param([int]$TargetPort)
    try {
        return @(Get-NetTCPConnection -LocalPort $TargetPort -ErrorAction Stop | Select-Object -Property LocalAddress,LocalPort,State,OwningProcess)
    } catch {
        return @()
    }
}

function Write-PortDiagnostics {
    param([int]$TargetPort)

    if (Test-Healthy -Url $HealthUrl) {
        Write-Info "Health endpoint is already responding at $HealthUrl."
    } else {
        Write-Info "Health endpoint is not responding yet at $HealthUrl."
    }

    $owners = Get-PortOwners -TargetPort $TargetPort
    if (-not $owners.Count) {
        Write-Info "Port $TargetPort is currently free."
        return
    }

    Write-Warn "Port $TargetPort is already in use:"
    foreach ($owner in $owners) {
        $processName = "unknown"
        $processPath = "-"
        try {
            $process = Get-Process -Id $owner.OwningProcess -ErrorAction Stop
            $processName = $process.ProcessName
            $processPath = Value-OrDefault $process.Path "-"
        } catch { }
        Write-Host ("  - {0}:{1} {2} PID={3} ({4})" -f $owner.LocalAddress, $owner.LocalPort, $owner.State, $owner.OwningProcess, $processName)
        Write-Host ("    path: {0}" -f (Get-ShortValue $processPath))
    }
    Write-FailHint "Use -Port <free-port>, or stop the owning process if it is stale."
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

function Invoke-PythonText {
    param(
        [string]$Python,
        [string[]]$Arguments
    )
    try {
        $output = & $Python @Arguments 2>&1
        if ($LASTEXITCODE -eq 0) {
            return ($output -join "`n").Trim()
        }
        return "failed: $($output -join ' ')"
    } catch {
        return "failed: $($_.Exception.Message)"
    }
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

function Write-PythonDiagnostics {
    param([string]$Python)

    $diagnosticCode = "import importlib.util, platform, sys; print('executable=' + sys.executable); print('version=' + ' '.join(sys.version.splitlines())); print('prefix=' + sys.prefix); print('base_prefix=' + sys.base_prefix); print('platform=' + platform.platform()); names=('fastapi','uvicorn','akshare','pandas','numpy','pytdx'); [print(name + '=' + ('missing' if importlib.util.find_spec(name) is None else 'present:' + str(importlib.util.find_spec(name).origin))) for name in names]"

    Write-Section "Python diagnostics:"
    Write-Host "  Selected Python: $Python"
    foreach ($line in (Invoke-PythonText -Python $Python -Arguments @('-c', $diagnosticCode)) -split "`n") {
        if (-not [string]::IsNullOrWhiteSpace($line)) { Write-Host "  $line" }
    }
    Write-Host "  pip=$(Invoke-PythonText -Python $Python -Arguments @('-m','pip','--version'))"
}

function Write-GitDiagnostics {
    $git = Get-Command git -ErrorAction SilentlyContinue
    if (-not $git) {
        Write-Warn "git was not found; skipping repository status."
        return
    }
    try {
        $branch = (& $git.Source rev-parse --abbrev-ref HEAD 2>$null).Trim()
        if ($LASTEXITCODE -eq 0 -and $branch) { Write-Info "Git branch: $branch" }
        $status = @(& $git.Source status --short 2>$null)
        if ($status.Count) {
            Write-Warn "Git working tree has $($status.Count) changed/untracked entries."
            foreach ($line in $status | Select-Object -First 8) {
                Write-Host "  $line"
            }
            if ($status.Count -gt 8) { Write-Host "  ... $($status.Count - 8) more" }
        } else {
            Write-Info "Git working tree is clean."
        }
    } catch {
        Write-Warn "Could not read git status: $($_.Exception.Message)"
    }
}

function Write-ConfigDiagnostics {
    Write-Section "Runtime configuration:"
    Write-Host "  Repo root: $RepoRoot"
    Write-Host "  URL: $BaseUrl"
    Write-Host "  Health: $HealthUrl"
    Write-Host "  Provider: $Provider"
    Write-Host "  Log level: $LogLevel"
    Write-Host "  Reload: $(if ($ReloadEnabled) { 'enabled' } else { 'disabled' })"
    Write-Host "  Reload flag: $([bool]$Reload)"
    Write-Host "  No reload: $([bool]$NoReload)"
    Write-Host "  Open browser: $([bool]$Open)"
    Write-Host "  No install: $([bool]$NoInstall)"
    Write-Host "  Skip syntax check: $([bool]$SkipSyntaxCheck)"
    Write-Host "  Diagnostics only: $([bool]$DiagnosticsOnly)"
    Write-Host "  GP_ASSISTANT_PYTHON: $(Get-ShortValue $env:GP_ASSISTANT_PYTHON)"
    Write-Host "  GP_CAPITAL_CACHE: $(Get-ShortValue (Value-OrDefault $env:GP_CAPITAL_CACHE "data/cache/capital_evidence.sqlite"))"
    Write-Host "  GP_CAPITAL_ENABLE_EXTERNAL: $(Get-ShortValue (Value-OrDefault $env:GP_CAPITAL_ENABLE_EXTERNAL "<default:true>"))"
    Write-Host "  GP_CAPITAL_LHB_FAST_TIMEOUT: $(Get-ShortValue (Value-OrDefault $env:GP_CAPITAL_LHB_FAST_TIMEOUT "<default:false>"))"
    Write-Host "  GP_CAPITAL_FUND_FLOW_TIMEOUT: $(Get-ShortValue (Value-OrDefault $env:GP_CAPITAL_FUND_FLOW_TIMEOUT "<default:8>"))"
    Write-Host "  GP_CAPITAL_LHB_TIMEOUT: $(Get-ShortValue (Value-OrDefault $env:GP_CAPITAL_LHB_TIMEOUT "<default:12>"))"
    Write-Host "  TDX_HOSTS: $(Get-ShortValue $env:TDX_HOSTS)"
    Write-Host "  ASTOCK_TDX_HOSTS: $(Get-ShortValue $env:ASTOCK_TDX_HOSTS)"
    Write-Host "  HTTP_PROXY: $(Get-RedactedValue $env:HTTP_PROXY)"
    Write-Host "  HTTPS_PROXY: $(Get-RedactedValue $env:HTTPS_PROXY)"
    Write-Host "  NO_PROXY: $(Get-ShortValue $env:NO_PROXY)"
}

function Invoke-SyntaxCheck {
    param([string]$Python)

    if ($SkipSyntaxCheck) {
        Write-Warn "Skipping syntax checks."
        return
    }

    Write-Step "Checking Python import path..."
    & $Python -c "import app.main; print('app.main import ok')"
    if ($LASTEXITCODE -ne 0) {
        throw "Python app import check failed."
    }

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
}

function Start-OpenBrowserJob {
    param([string]$Url)

    Write-Step "Browser will open once the server is healthy."
    Start-Job -ScriptBlock {
        param($HealthUrl, $OpenUrl)
        for ($i = 0; $i -lt 90; $i++) {
            Start-Sleep -Milliseconds 500
            try {
                $r = Invoke-WebRequest -UseBasicParsing -Uri $HealthUrl -TimeoutSec 2
                if ($r.StatusCode -eq 200) {
                    Start-Process $OpenUrl
                    break
                }
            } catch { }
        }
    } -ArgumentList $HealthUrl, $Url | Out-Null
}

function Write-FailureDiagnostics {
    param([object]$ErrorRecord)

    Write-Host ""
    Write-Warn "Dev server failed before entering uvicorn."
    if ($ErrorRecord) {
        Write-Host "  Error: $($ErrorRecord.Exception.Message)"
        if ($ErrorRecord.Exception.InnerException) {
            Write-Host "  Inner: $($ErrorRecord.Exception.InnerException.Message)"
        }
        if ($ErrorRecord.InvocationInfo) {
            Write-Host "  At: $($ErrorRecord.InvocationInfo.PositionMessage)"
        }
        if ($ErrorRecord.ScriptStackTrace) {
            Write-Host "  Stack: $($ErrorRecord.ScriptStackTrace)"
        }
    }
    Write-PortDiagnostics -TargetPort $Port
    Write-FailHint "Run: .\start-dev.bat -DiagnosticsOnly -LogLevel debug"
    Write-FailHint "Run: .\start-dev.bat -Port 8011 -LogLevel debug"
    Write-FailHint "Run: .\.venv-cpython\Scripts\python.exe -m pytest tests/test_observation.py"
    Write-FailHint "If AkShare or external evidence hangs, temporarily set GP_CAPITAL_ENABLE_EXTERNAL=false for local UI debugging."
}

try {
    Set-Location -LiteralPath $RepoRoot

    Write-Host ""
    Write-Step "Starting GP Assistant dev server diagnostics..."
    Write-SystemDiagnostics
    Write-ConfigDiagnostics
    Write-PortDiagnostics -TargetPort $Port
    Write-GitDiagnostics
    Write-ProjectFileDiagnostics

    $Python = Get-PythonPath
    Write-PythonDiagnostics -Python $Python
    Write-NodeDiagnostics
    Ensure-Dependencies -Python $Python
    Invoke-SyntaxCheck -Python $Python

    # Backend env. Force UTF-8 so Chinese log lines and tracebacks render cleanly.
    $env:STOCK_PROVIDER = $Provider
    $env:GP_ASSISTANT_HOST = $HostName
    $env:GP_ASSISTANT_PORT = [string]$Port
    $env:GP_ASSISTANT_LOG_LEVEL = $LogLevel
    $env:PYTHONUTF8 = "1"
    $env:PYTHONIOENCODING = "utf-8"

    if ($DiagnosticsOnly) {
        Write-Host ""
        Write-Step "Diagnostics-only mode completed; server was not started."
        exit 0
    }

    if ($Open) {
        Start-OpenBrowserJob "$BaseUrl/#sectionAgent"
    }

    Write-Host ""
    Write-Step "Starting FastAPI on $BaseUrl  (provider=$Provider, log-level=$LogLevel)"
    Write-Step "Health: $HealthUrl"
    Write-Step "UI: $BaseUrl/#sectionAgent   |   Press Ctrl+C to stop."
    $ReloadDir = Join-Path $RepoRoot "app"
    if ($ReloadEnabled) {
        Write-Info "Hot reload: enabled; watching $ReloadDir"
        Write-Info "Command: $Python -m uvicorn app.main:app --host $HostName --port $Port --log-level $LogLevel --reload --reload-dir `"$ReloadDir`""
    } else {
        Write-Info "Hot reload: disabled by -NoReload."
        Write-Info "Command: $Python -m uvicorn app.main:app --host $HostName --port $Port --log-level $LogLevel"
    }
    Write-Host ""

    # Run uvicorn in the FOREGROUND so all output streams to this console.
    if ($ReloadEnabled) {
        # Reload needs the import-string form; serves the same app object.
        & $Python -m uvicorn app.main:app `
            --host $HostName `
            --port $Port `
            --log-level $LogLevel `
            --reload `
            --reload-dir $ReloadDir
    } else {
        & $Python -m uvicorn app.main:app `
            --host $HostName `
            --port $Port `
            --log-level $LogLevel
    }

    $exitCode = if ($null -eq $LASTEXITCODE) { 0 } else { $LASTEXITCODE }
    Write-Host ""
    Write-Step "Dev server process exited with code $exitCode after $([int]((Get-Date) - $ScriptStartedAt).TotalSeconds)s."
    if ($exitCode -ne 0) {
        Write-Warn "Server process returned a non-zero exit code."
        Write-PortDiagnostics -TargetPort $Port
        Write-FailHint "Re-run with: .\start-dev.bat -LogLevel debug"
    }
    exit $exitCode
} catch {
    Write-FailureDiagnostics -ErrorRecord $_
    exit 1
}
