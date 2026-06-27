[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [string] $Mode,
    [switch] $Web,
    [switch] $Tauri,
    [switch] $PreflightOnly,
    [switch] $RunPreflight,
    [switch] $SkipPrepare,
    [switch] $SkipCargoCheck,
    [switch] $VerboseTauri,
    [switch] $NoWatch,
    [switch] $Watch,
    [switch] $NoTranscript,
    [switch] $NoBuild,
    [switch] $NoKill,
    [switch] $OpenBrowser,
    [int] $Port = 8010,
    [Alias('Host', 'ListenHost')]
    [string] $BindHost = '127.0.0.1',
    [string] $Python,
    [string] $LogFile,
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]] $ForwardArgs
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $PSScriptRoot
$FrontendRoot = Join-Path $RepoRoot 'desktop\frontend'
$FrontendDist = Join-Path $RepoRoot 'desktop\mobile-dist'
$ScriptStartedAt = Get-Date
$LogRoot = Join-Path $RepoRoot 'logs\dev'
$SessionLogTimestamp = $ScriptStartedAt.ToString('yyyyMMdd-HHmmss')
$LoggingEnabled = $false
$SessionLogPath = $null
$LatestLogPath = $null
$ScriptExitCode = 0

function Write-LogLine {
    param(
        [AllowEmptyString()][string] $Message = '',
        [ConsoleColor] $Color = [ConsoleColor]::Gray
    )

    if ([string]::IsNullOrEmpty($Message)) {
        Write-Host ''
    } else {
        Write-Host $Message -ForegroundColor $Color
    }

    if ($script:LoggingEnabled -and $script:SessionLogPath) {
        Add-Content -LiteralPath $script:SessionLogPath -Value $Message -Encoding UTF8
        if ($script:LatestLogPath -and ($script:LatestLogPath -ne $script:SessionLogPath)) {
            Add-Content -LiteralPath $script:LatestLogPath -Value $Message -Encoding UTF8
        }
    }
}

function Write-Step {
    param([string] $Message)
    Write-LogLine "[gp-dev] $Message" ([ConsoleColor]::Cyan)
}

function Write-Info {
    param([string] $Message)
    Write-LogLine "[gp-dev] INFO: $Message" ([ConsoleColor]::DarkGray)
}

function Write-Warn {
    param([string] $Message)
    Write-LogLine "[gp-dev] WARN: $Message" ([ConsoleColor]::Yellow)
}

function Write-Hint {
    param([string] $Message)
    Write-LogLine "[gp-dev] HINT: $Message" ([ConsoleColor]::Magenta)
}

function Resolve-StartMode {
    if ($Web -and $Tauri) {
        throw 'Choose either -Web or -Tauri, not both.'
    }
    if ($Web) {
        return 'web'
    }
    if ($Tauri) {
        return 'tauri'
    }
    if ([string]::IsNullOrWhiteSpace($Mode)) {
        return 'tauri'
    }

    switch ($Mode.Trim().ToLowerInvariant()) {
        'web' { return 'web' }
        'python' { return 'web' }
        'fastapi' { return 'web' }
        'tauri' { return 'tauri' }
        'desktop' { return 'tauri' }
        default { throw "Unknown start-dev mode '$Mode'. Use 'tauri' or 'web'." }
    }
}

function Resolve-SessionLogPath {
    param([string] $Prefix)

    if ($NoTranscript) {
        return $null
    }
    if ([string]::IsNullOrWhiteSpace($LogFile)) {
        return (Join-Path $LogRoot "$Prefix-dev-$SessionLogTimestamp.log")
    }
    if ([System.IO.Path]::IsPathRooted($LogFile)) {
        return [System.IO.Path]::GetFullPath($LogFile)
    }
    return [System.IO.Path]::GetFullPath((Join-Path $RepoRoot $LogFile))
}

function Initialize-SessionLogging {
    param([string] $Prefix)

    if ($NoTranscript) {
        Write-Warn 'Session logging disabled by -NoTranscript.'
        return
    }

    $script:SessionLogPath = Resolve-SessionLogPath -Prefix $Prefix
    if ([string]::IsNullOrWhiteSpace($script:SessionLogPath)) {
        return
    }

    $logDir = Split-Path -Parent $script:SessionLogPath
    if ($logDir) {
        New-Item -ItemType Directory -Force -Path $logDir | Out-Null
    }
    New-Item -ItemType Directory -Force -Path $LogRoot | Out-Null
    $script:LatestLogPath = Join-Path $LogRoot "$Prefix-dev.latest.log"
    Set-Content -LiteralPath $script:SessionLogPath -Value @() -Encoding UTF8
    Set-Content -LiteralPath $script:LatestLogPath -Value @() -Encoding UTF8
    $script:LoggingEnabled = $true

    Write-Step "Session log: $script:SessionLogPath"
    Write-Info "Latest log alias: $script:LatestLogPath"
}

function Finalize-SessionLogging {
    if (-not $script:LoggingEnabled) {
        return
    }
    if ($script:SessionLogPath -and $script:LatestLogPath -and (Test-Path -LiteralPath $script:SessionLogPath)) {
        Copy-Item -LiteralPath $script:SessionLogPath -Destination $script:LatestLogPath -Force -ErrorAction SilentlyContinue
    }
}

function Invoke-TextCommand {
    param(
        [string] $FilePath,
        [string[]] $Arguments,
        [string] $WorkingDirectory = $RepoRoot
    )

    $previousErrorActionPreference = $ErrorActionPreference
    Push-Location -LiteralPath $WorkingDirectory
    try {
        $ErrorActionPreference = 'Continue'
        $output = & $FilePath @Arguments 2>&1
        if ($LASTEXITCODE -eq 0) {
            return ($output -join "`n").Trim()
        }
        return "failed(exit=$LASTEXITCODE): $($output -join ' ')"
    } catch {
        return "failed: $($_.Exception.Message)"
    } finally {
        Pop-Location
        $ErrorActionPreference = $previousErrorActionPreference
    }
}

function Invoke-LoggedCommand {
    param(
        [string] $FilePath,
        [string[]] $Arguments,
        [string] $WorkingDirectory = $RepoRoot
    )

    Write-Info "cwd: $WorkingDirectory"
    Write-Info ("cmd: {0} {1}" -f $FilePath, ($Arguments -join ' '))

    $previousErrorActionPreference = $ErrorActionPreference
    Push-Location -LiteralPath $WorkingDirectory
    try {
        $ErrorActionPreference = 'Continue'
        & $FilePath @Arguments 2>&1 | ForEach-Object {
            $line = if ($null -eq $_) { '' } else { $_.ToString() }
            Write-LogLine $line
        }
        return $(if ($null -eq $LASTEXITCODE) { 0 } else { $LASTEXITCODE })
    } finally {
        Pop-Location
        $ErrorActionPreference = $previousErrorActionPreference
    }
}

function Resolve-CommandPath {
    param(
        [string] $Name,
        [string] $InstallHint
    )

    $command = Get-Command $Name -ErrorAction SilentlyContinue
    if (-not $command) {
        throw "$Name was not found. $InstallHint"
    }
    return $command.Source
}

function Resolve-PythonPath {
    if (-not [string]::IsNullOrWhiteSpace($Python)) {
        $candidate = if ([System.IO.Path]::IsPathRooted($Python)) { $Python } else { Join-Path $RepoRoot $Python }
        if (-not (Test-Path -LiteralPath $candidate)) {
            throw "Python executable was not found: $candidate"
        }
        return [System.IO.Path]::GetFullPath($candidate)
    }

    $candidates = @(
        (Join-Path $RepoRoot '.venv-cpython\Scripts\python.exe'),
        (Join-Path $RepoRoot '.venv\Scripts\python.exe')
    )
    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate) {
            return [System.IO.Path]::GetFullPath($candidate)
        }
    }

    $command = Get-Command 'python.exe' -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }
    $launcher = Get-Command 'py.exe' -ErrorAction SilentlyContinue
    if ($launcher) {
        return $launcher.Source
    }
    throw 'No Python runtime was found. Install dependencies into .venv-cpython or pass -Python <path>.'
}

function Get-ListeningProcessIds {
    param([int] $LocalPort)

    $ids = @()
    $getNetTcp = Get-Command 'Get-NetTCPConnection' -ErrorAction SilentlyContinue
    if ($getNetTcp) {
        try {
            $ids += Get-NetTCPConnection -LocalPort $LocalPort -State Listen -ErrorAction Stop |
                Select-Object -ExpandProperty OwningProcess
        } catch {
            Write-Info "Get-NetTCPConnection did not find a listener on port $LocalPort."
        }
    }

    if (-not $ids -or $ids.Count -eq 0) {
        $netstat = Invoke-TextCommand -FilePath 'netstat.exe' -Arguments @('-ano', '-p', 'tcp')
        foreach ($line in ($netstat -split "`n")) {
            if ($line -match "^\s*TCP\s+\S+:$LocalPort\s+\S+\s+LISTENING\s+(\d+)\s*$") {
                $ids += [int] $Matches[1]
            }
        }
    }

    return @($ids | Where-Object { $_ -and $_ -ne $PID } | Sort-Object -Unique)
}

function Stop-PortListeners {
    param([int] $LocalPort)

    $ids = Get-ListeningProcessIds -LocalPort $LocalPort
    if (-not $ids -or $ids.Count -eq 0) {
        Write-Info "No existing listener found on port $LocalPort."
        return
    }

    Write-Warn "Port $LocalPort is already in use by PID(s): $($ids -join ', ')."
    if ($NoKill) {
        throw "Port $LocalPort is occupied and -NoKill was supplied. Stop the old process or choose another -Port."
    }

    foreach ($id in $ids) {
        $process = Get-Process -Id $id -ErrorAction SilentlyContinue
        $name = if ($process) { $process.ProcessName } else { 'unknown' }
        Write-Warn "Stopping old listener PID $id ($name) before starting the web server."
        Stop-Process -Id $id -Force -ErrorAction Stop
    }
}

function Assert-ReactBuild {
    $indexPath = Join-Path $FrontendDist 'index.html'
    if ($NoBuild) {
        if (-not (Test-Path -LiteralPath $indexPath)) {
            throw "React build output is missing: $indexPath. Run without -NoBuild once."
        }
        Write-Warn 'Skipping React frontend build by -NoBuild.'
        return
    }

    $npm = Resolve-CommandPath 'npm.cmd' 'Install Node.js/npm and retry.'
    Write-LogLine ''
    Write-Step 'Building React frontend for Python Web...'
    $exitCode = Invoke-LoggedCommand -FilePath $npm -Arguments @('--prefix', 'desktop/frontend', 'run', 'build') -WorkingDirectory $RepoRoot
    if ($exitCode -ne 0) {
        throw "React frontend build failed with exit code $exitCode."
    }
    if (-not (Test-Path -LiteralPath $indexPath)) {
        throw "React build completed but index.html was not found: $indexPath"
    }
}

function Assert-PythonWebImports {
    param([string] $PythonPath)

    Write-LogLine ''
    Write-Step 'Checking Python Web runtime imports...'
    $probe = 'import fastapi, uvicorn, app.main; print("python web imports ok")'
    $exitCode = Invoke-LoggedCommand -FilePath $PythonPath -Arguments @('-c', $probe) -WorkingDirectory $RepoRoot
    if ($exitCode -ne 0) {
        throw "Python Web import check failed with exit code $exitCode. Install requirements.legacy-python.txt into the selected environment."
    }
}

function Start-WebMode {
    Initialize-SessionLogging -Prefix 'web'
    Write-LogLine ''
    Write-Step 'Starting Python Web diagnostics...'
    Write-LogLine "  Started: $($ScriptStartedAt.ToString('o'))"
    Write-LogLine "  Repo root: $RepoRoot"
    Write-LogLine "  Frontend source: $FrontendRoot"
    Write-LogLine "  Frontend dist: $FrontendDist"
    Write-LogLine "  URL: http://${BindHost}:$Port/"
    Write-LogLine "  PowerShell: $($PSVersionTable.PSVersion) $($PSVersionTable.PSEdition)"

    $pythonPath = Resolve-PythonPath
    Write-Info "Python: $pythonPath"
    Write-Info "Python version: $(Invoke-TextCommand -FilePath $pythonPath -Arguments @('--version'))"

    Assert-ReactBuild
    Assert-PythonWebImports -PythonPath $pythonPath

    if ($PreflightOnly) {
        $listeners = Get-ListeningProcessIds -LocalPort $Port
        if ($listeners -and $listeners.Count -gt 0) {
            Write-Warn "Preflight found existing listener(s) on port ${Port}: $($listeners -join ', ')."
            Write-Hint 'Run without -PreflightOnly to stop them automatically, or use -NoKill to fail fast.'
        } else {
            Write-Info "Preflight found port $Port free."
        }
        Write-LogLine ''
        Write-Step 'Python Web preflight completed; server was not started.'
        return 0
    }

    Stop-PortListeners -LocalPort $Port

    $url = "http://${BindHost}:$Port/"
    $env:NO_PROXY = (($env:NO_PROXY, '127.0.0.1', 'localhost', $BindHost) -join ',')
    $env:no_proxy = $env:NO_PROXY

    Write-LogLine ''
    Write-Step 'Starting Python Web server...'
    Write-Info "Command: $pythonPath -m uvicorn app.main:app --host $BindHost --port $Port --log-level info"
    Write-Info 'This window stays open while uvicorn is running; that is normal.'
    Write-Info 'Press Ctrl+C here to stop the web server.'
    Write-Info "Open: $url"
    if ($OpenBrowser) {
        Start-Process $url | Out-Null
    }

    $exitCode = Invoke-LoggedCommand -FilePath $pythonPath -Arguments @('-m', 'uvicorn', 'app.main:app', '--host', $BindHost, '--port', [string] $Port, '--log-level', 'info') -WorkingDirectory $RepoRoot
    Write-LogLine ''
    Write-Step "Python Web server exited with code $exitCode after $([int]((Get-Date) - $ScriptStartedAt).TotalSeconds)s."
    return $exitCode
}

function Start-TauriMode {
    $TauriScript = Join-Path $PSScriptRoot 'start-tauri-dev.ps1'

    if (-not (Test-Path -LiteralPath $TauriScript)) {
        throw "Tauri launcher was not found: $TauriScript"
    }

    $tauriArgs = @()
    if ($PreflightOnly) {
        $tauriArgs += '-PreflightOnly'
    } else {
        $tauriArgs += '-RunPreflight'
    }
    # Tauri dev runs prepare:android via beforeDevCommand; only preflight-only needs the script-side prepare.
    if ($SkipPrepare -or -not $PreflightOnly) { $tauriArgs += '-SkipPrepare' }
    if ($SkipCargoCheck) { $tauriArgs += '-SkipCargoCheck' }
    if ($VerboseTauri) { $tauriArgs += '-VerboseTauri' }
    if ($NoTranscript) { $tauriArgs += '-NoTranscript' }
    if (-not [string]::IsNullOrWhiteSpace($LogFile)) { $tauriArgs += @('-LogFile', $LogFile) }
    if ($NoWatch -or -not $Watch) { $tauriArgs += '-NoWatch' }
    if ($ForwardArgs -and $ForwardArgs.Count -gt 0) { $tauriArgs += $ForwardArgs }

    Write-Host '[gp-dev] Default mode is Tauri desktop; forwarding to scripts/start-tauri-dev.ps1.' -ForegroundColor Cyan
    Write-Host "[gp-dev] Tauri args: $($tauriArgs -join ' ')" -ForegroundColor DarkGray
    & $TauriScript @tauriArgs
    return $(if ($null -eq $LASTEXITCODE) { 0 } else { $LASTEXITCODE })
}

try {
    $resolvedMode = Resolve-StartMode
    switch ($resolvedMode) {
        'web' { $ScriptExitCode = Start-WebMode }
        'tauri' { $ScriptExitCode = Start-TauriMode }
        default { throw "Unhandled start-dev mode: $resolvedMode" }
    }
} catch {
    $ScriptExitCode = 1
    Write-LogLine ''
    Write-Warn 'Startup failed.'
    Write-LogLine "  Error: $($_.Exception.Message)" ([ConsoleColor]::Red)
    Write-Hint 'Default desktop retry: .\start-dev.bat'
    Write-Hint 'Tauri preflight only: .\start-dev.bat -PreflightOnly -NoTranscript'
    Write-Hint 'Python Web retry: .\start-dev.bat web'
    Write-Hint 'Python Web preflight only: .\start-dev.bat web -PreflightOnly -NoTranscript'
    Write-Hint 'Python Web without rebuilding frontend: .\start-dev.bat web -NoBuild'
} finally {
    if ($script:SessionLogPath) {
        Write-LogLine ''
        Write-Step "Session log saved to: $script:SessionLogPath"
    }
    Finalize-SessionLogging
}

exit $ScriptExitCode
