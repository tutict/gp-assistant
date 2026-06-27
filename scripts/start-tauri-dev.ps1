[CmdletBinding()]
param(
    [switch]$PreflightOnly,
    [switch]$RunPreflight,
    [switch]$SkipPrepare,
    [switch]$SkipCargoCheck,
    [switch]$VerboseTauri,
    [switch]$NoWatch,
    [switch]$NoTranscript,
    [string]$LogFile
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $PSScriptRoot
$DesktopRoot = Join-Path $RepoRoot 'desktop'
$ScriptStartedAt = Get-Date
$LogRoot = Join-Path $RepoRoot 'logs\dev'
$SessionLogTimestamp = $ScriptStartedAt.ToString('yyyyMMdd-HHmmss')
$LoggingEnabled = $false
$SessionLogPath = $null
$LatestLogPath = Join-Path $LogRoot 'tauri-dev.latest.log'
$ScriptExitCode = 0

function Write-LogLine {
    param(
        [AllowEmptyString()][string]$Message = '',
        [ConsoleColor]$Color = [ConsoleColor]::Gray
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
    param([string]$Message)
    Write-LogLine "[gp-tauri] $Message" ([ConsoleColor]::Cyan)
}

function Write-Warn {
    param([string]$Message)
    Write-LogLine "[gp-tauri] WARN: $Message" ([ConsoleColor]::Yellow)
}

function Write-Info {
    param([string]$Message)
    Write-LogLine "[gp-tauri] INFO: $Message" ([ConsoleColor]::DarkGray)
}

function Write-FailHint {
    param([string]$Message)
    Write-LogLine "[gp-tauri] HINT: $Message" ([ConsoleColor]::Magenta)
}

function Resolve-SessionLogPath {
    if ($NoTranscript) {
        return $null
    }
    if ([string]::IsNullOrWhiteSpace($LogFile)) {
        return (Join-Path $LogRoot "tauri-dev-$SessionLogTimestamp.log")
    }
    if ([System.IO.Path]::IsPathRooted($LogFile)) {
        return [System.IO.Path]::GetFullPath($LogFile)
    }
    return [System.IO.Path]::GetFullPath((Join-Path $RepoRoot $LogFile))
}

function Initialize-SessionLogging {
    if ($NoTranscript) {
        Write-Warn 'Session logging disabled by -NoTranscript.'
        return
    }

    $script:SessionLogPath = Resolve-SessionLogPath
    if ([string]::IsNullOrWhiteSpace($script:SessionLogPath)) {
        return
    }

    $logDir = Split-Path -Parent $script:SessionLogPath
    if ($logDir) {
        New-Item -ItemType Directory -Force -Path $logDir | Out-Null
    }
    New-Item -ItemType Directory -Force -Path $LogRoot | Out-Null
    Set-Content -LiteralPath $script:SessionLogPath -Value @() -Encoding UTF8
    Set-Content -LiteralPath $script:LatestLogPath -Value @() -Encoding UTF8
    $script:LoggingEnabled = $true

    Write-Step "Session log: $script:SessionLogPath"
    Write-Info "Latest log alias: $LatestLogPath"
    Write-Info 'Child npm/cargo output is mirrored into the session log.'
}

function Finalize-SessionLogging {
    if (-not $script:LoggingEnabled) {
        return
    }

    if ($script:SessionLogPath -and (Test-Path -LiteralPath $script:SessionLogPath)) {
        Copy-Item -LiteralPath $script:SessionLogPath -Destination $LatestLogPath -Force -ErrorAction SilentlyContinue
    }
}

function Invoke-TextCommand {
    param(
        [string]$FilePath,
        [string[]]$Arguments
    )

    $previousErrorActionPreference = $ErrorActionPreference
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
        $ErrorActionPreference = $previousErrorActionPreference
    }
}

function Write-CommandProbe {
    param(
        [string]$Label,
        [string]$CommandName = $Label
    )

    $command = Get-Command $CommandName -ErrorAction SilentlyContinue
    if ($command) {
        Write-LogLine ("  {0}: {1}" -f $Label, $command.Source)
    } else {
        Write-LogLine ("  {0}: not found" -f $Label)
    }
}

function Assert-Command {
    param([string]$Name)
    $command = Get-Command $Name -ErrorAction SilentlyContinue
    if (-not $command) {
        throw "$Name was not found in PATH."
    }
    return $command.Source
}

function Invoke-LoggedCommand {
    param(
        [string]$FilePath,
        [string[]]$Arguments
    )

    $previousErrorActionPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $output = & $FilePath @Arguments 2>&1
        $exitCode = if ($null -eq $LASTEXITCODE) { 0 } else { $LASTEXITCODE }
        foreach ($entry in @($output)) {
            $line = if ($null -eq $entry) { '' } else { $entry.ToString() }
            Write-LogLine $line
        }
        return $exitCode
    } finally {
        $ErrorActionPreference = $previousErrorActionPreference
    }
}

function Write-SystemDiagnostics {
    Write-LogLine ''
    Write-Step 'Starting Tauri desktop diagnostics...'
    Write-LogLine "  Started: $($ScriptStartedAt.ToString('o'))"
    Write-LogLine "  Repo root: $RepoRoot"
    Write-LogLine "  Desktop root: $DesktopRoot"
    Write-LogLine "  PowerShell: $($PSVersionTable.PSVersion) $($PSVersionTable.PSEdition)"
    Write-LogLine "  User: $env:USERNAME@$env:COMPUTERNAME"
    Write-LogLine '  Commands:'
    Write-CommandProbe 'node'
    Write-CommandProbe 'npm.cmd'
    Write-CommandProbe 'cargo'
    Write-CommandProbe 'rustc'
}

function Run-PrepareAssets {
    param([string]$NpmPath)
    Write-LogLine ''
    Write-Step 'Preparing Tauri frontend assets...'
    $exitCode = Invoke-LoggedCommand -FilePath $NpmPath -Arguments @('run', 'prepare:android')
    if ($exitCode -ne 0) {
        throw "npm run prepare:android failed with exit code $exitCode."
    }
}

function Run-CargoCheck {
    param([string]$CargoPath)
    Write-LogLine ''
    Write-Step 'Checking Rust desktop crate...'
    $exitCode = Invoke-LoggedCommand -FilePath $CargoPath -Arguments @('check', '--manifest-path', (Join-Path $DesktopRoot 'src-tauri\Cargo.toml'))
    if ($exitCode -ne 0) {
        throw "cargo check failed with exit code $exitCode."
    }
}

try {
    Set-Location -LiteralPath $DesktopRoot
    Initialize-SessionLogging
    Write-SystemDiagnostics

    $node = Assert-Command 'node'
    $npm = Assert-Command 'npm.cmd'
    $cargo = Assert-Command 'cargo'
    $rustc = Assert-Command 'rustc'

    Write-Info "node version: $(Invoke-TextCommand -FilePath $node -Arguments @('--version'))"
    Write-Info "npm version: $(Invoke-TextCommand -FilePath $npm -Arguments @('--version'))"
    Write-Info "cargo version: $(Invoke-TextCommand -FilePath $cargo -Arguments @('--version'))"
    Write-Info "rustc version: $(Invoke-TextCommand -FilePath $rustc -Arguments @('--version'))"

    if ($PreflightOnly -or $RunPreflight) {
        if (-not $SkipPrepare) {
            Run-PrepareAssets -NpmPath $npm
        } else {
            Write-Warn 'Skipping prepare:android by -SkipPrepare.'
        }

        if (-not $SkipCargoCheck) {
            Run-CargoCheck -CargoPath $cargo
        } else {
            Write-Warn 'Skipping cargo check by -SkipCargoCheck.'
        }
    }

    if ($PreflightOnly) {
        Write-LogLine ''
        Write-Step 'Preflight completed; tauri dev was not started.'
    } else {
        $devArgs = @('run', 'dev', '--')
        if ($VerboseTauri) {
            $devArgs += '--verbose'
        }
        if ($NoWatch) {
            $devArgs += '--no-watch'
        }

        Write-LogLine ''
        Write-Step 'Starting tauri dev...'
        Write-Info ("Command: npm.cmd {0}" -f ($devArgs -join ' '))
        Write-Info 'Tauri dev is a foreground process. This window stays open while the desktop app is running.'
        Write-Info 'Close the desktop window or press Ctrl+C here to stop it.'
        if ($NoWatch) {
            Write-Info 'Watch mode is disabled by -NoWatch for a quieter startup log.'
        } else {
            Write-Warn 'Watch mode is enabled. File changes can trigger rebuilds and repeated log output.'
        }
        if (-not $VerboseTauri) {
            Write-Info 'Tip: add -VerboseTauri only when you need deep CLI logs; it can flood the console with html parser debug output.'
        }

        $ScriptExitCode = Invoke-LoggedCommand -FilePath $npm -Arguments $devArgs
        Write-LogLine ''
        Write-Step "tauri dev exited with code $ScriptExitCode after $([int]((Get-Date) - $ScriptStartedAt).TotalSeconds)s."
    }
} catch {
    $ScriptExitCode = 1
    Write-LogLine ''
    Write-Warn 'Tauri desktop startup failed.'
    Write-LogLine "  Error: $($_.Exception.Message)" ([ConsoleColor]::Red)
    Write-FailHint 'Run: .\start-tauri-dev.bat'
    Write-FailHint 'Run: .\start-tauri-dev.bat -PreflightOnly -NoTranscript'
    Write-FailHint 'Run: .\start-tauri-dev.bat -RunPreflight -SkipPrepare -NoWatch -NoTranscript'
    Write-FailHint 'Run: .\start-tauri-dev.bat -VerboseTauri -NoWatch -NoTranscript'
    Write-FailHint 'Run: npm.cmd run dev -- --no-watch'
} finally {
    if ($script:SessionLogPath) {
        Write-LogLine ''
        Write-Step "Session log saved to: $script:SessionLogPath"
    }
    Finalize-SessionLogging
}

exit $ScriptExitCode



