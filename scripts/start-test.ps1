[CmdletBinding()]
param(
    [switch] $SkipPrepare,
    [switch] $SkipCargoCheck,
    [switch] $NoTranscript
)

$ErrorActionPreference = 'Stop'
$TauriScript = Join-Path $PSScriptRoot 'start-tauri-dev.ps1'

if (-not (Test-Path -LiteralPath $TauriScript)) {
    throw "Tauri launcher was not found: $TauriScript"
}

$args = @('-PreflightOnly')
if ($SkipPrepare) { $args += '-SkipPrepare' }
if ($SkipCargoCheck) { $args += '-SkipCargoCheck' }
if ($NoTranscript) { $args += '-NoTranscript' }

Write-Host '[gp-test] Python/FastAPI test server is retired; running Tauri preflight instead.' -ForegroundColor Cyan
& $TauriScript @args
exit $LASTEXITCODE
