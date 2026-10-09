[CmdletBinding()]
param(
    [switch] $SkipAndroidPreflight,
    [switch] $SkipRust,
    [switch] $SkipNode,
    [switch] $SkipPrepare,
    [switch] $SkipPackageBuild,
    [switch] $AllowUnsignedAndroid,
    [switch] $SkipReliability,
    [switch] $EvaluateBaseline,
    [string] $BaselinePath,
    [string] $CandidatePath,
    [string] $ExemptionsPath
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

# Measurement gates never synthesize missing device data or silently ignore supplied paths.
if (-not $EvaluateBaseline -and ($BaselinePath -or $CandidatePath -or $ExemptionsPath)) {
    Write-Error 'BaselinePath/CandidatePath/ExemptionsPath require -EvaluateBaseline.'
    exit 2
}
if ($EvaluateBaseline) {
    if (-not $BaselinePath -or -not $CandidatePath) {
        Write-Error 'requires_user: -EvaluateBaseline requires -BaselinePath and -CandidatePath with actual device readings.'
        exit 2
    }
    $node = Resolve-CommandPath 'node' 'Install Node.js and retry.'
    $baselineArgs = @((Join-Path $Root 'scripts/reliability-benchmark.mjs'), '--baseline', $BaselinePath, '--candidate', $CandidatePath)
    if ($ExemptionsPath) { $baselineArgs += @('--exemptions', $ExemptionsPath) }
    Invoke-Checked 'Measured reliability performance gate' $node $baselineArgs
} else {
    Write-Step 'Performance NOT EVALUATED (requires_user): actual same-device baselines and -EvaluateBaseline are required for performance sign-off.'
}

if ($SkipReliability) {
    Write-Step 'SKIPPED local reliability suites by explicit request; not a reliability sign-off.'
} else {
    $node = Resolve-CommandPath 'node' 'Install Node.js and retry.'
    Invoke-Checked 'Reliability deterministic benchmark/gate fixtures' $node @('--test', (Join-Path $Root 'scripts/reliability-benchmark.test.mjs'))
    if (-not $SkipRust) {
        Invoke-Checked 'Reliability native privacy, quota, lifecycle and feature fixtures' 'powershell.exe' @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $Root 'scripts/reliability-native.test.ps1'), '-Offline')
    } else {
        Write-Step 'SKIPPED native reliability suites (-SkipRust).'
    }
    if ($SkipNode) { Write-Step 'SKIPPED frontend reliability suites (-SkipNode).' }
}
if (-not $SkipNode) {
    $npm = Resolve-CommandPath "npm.cmd" "Install Node.js/npm and retry."
    $frontendDir = Join-Path $Root "desktop/frontend"
    if (-not (Test-Path -LiteralPath (Join-Path $frontendDir "node_modules"))) {
        Invoke-Checked "Install frontend dependencies" $npm @("ci") $frontendDir
    }
    Invoke-Checked "Release version metadata consistency" $npm @("run", "test:version") $frontendDir
    Invoke-Checked "Mobile industry exporter contract" $npm @("run", "test:industry-export") $frontendDir
    Invoke-Checked "Frontend UI density guard" $npm @("run", "test:density") $frontendDir
    Invoke-Checked "Frontend UI density contract tests" $npm @("run", "test:density-contract") $frontendDir
    Invoke-Checked "Frontend Agent replay CSS contract tests" $npm @("run", "test:agent-replay-css") $frontendDir
    Invoke-Checked "Frontend unstyled class guard" $npm @("run", "test:unstyled") $frontendDir
    Invoke-Checked "Frontend CSS architecture guard" $npm @("run", "test:architecture") $frontendDir
    Invoke-Checked "Frontend theme parity contract tests" $npm @("run", "test:theme-parity-contract") $frontendDir
    Invoke-Checked "Frontend theme parity guard" $npm @("run", "test:theme-parity") $frontendDir
    if (-not $SkipReliability) {
        Invoke-Checked "Frontend reliability controls and credential-reference fixtures" $npm @("run", "test:unit", "--", "src/components/settings/ReliabilityPanel.test.tsx", "src/components/panels/GepaLabPanel.test.tsx", "src/components/settings/BackupPanel.test.tsx") $frontendDir
    }
    Invoke-Checked "Frontend unit tests" $npm @("run", "test:unit") $frontendDir
    Invoke-Checked "Frontend React/TypeScript build" $npm @("run", "build") $frontendDir
    Invoke-Checked "Frontend UI contrast audit (fail mode)" $npm @("run", "test:contrast:built") $frontendDir
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
    Invoke-Checked "Tauri GEPA Rust tests" $cargo @("test", "--locked", "--features", "gepa-lab", "--manifest-path", "desktop/src-tauri/Cargo.toml")
    Invoke-Checked "Tauri cargo check" $cargo @("check", "--locked", "--manifest-path", "desktop/src-tauri/Cargo.toml")
    Invoke-Checked "Tauri GEPA cargo check" $cargo @("check", "--locked", "--features", "gepa-lab", "--manifest-path", "desktop/src-tauri/Cargo.toml")
}

if (-not $SkipAndroidPreflight) {
    $androidScript = Join-Path $Root "scripts/build-android.ps1"
    Invoke-Checked "Android build environment preflight" "powershell.exe" @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $androidScript, "-PreflightOnly")
}

if (-not $SkipPackageBuild) {
    $desktopDir = Join-Path $Root "desktop"
    $androidArgs = @(
        "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", (Join-Path $Root "scripts/build-android.ps1"),
        "-Target", "aarch64"
    )
    if (-not $AllowUnsignedAndroid) {
        $androidArgs += "-Signed"
    }
    Invoke-Checked "Build Android release package" "powershell.exe" $androidArgs
    $androidOutputRoot = Join-Path $Root "desktop/src-tauri/gen/android/app/build/outputs/apk"
    $androidArtifacts = @(Get-ChildItem -LiteralPath $androidOutputRoot -Recurse -File -Filter "*.apk" -ErrorAction SilentlyContinue |
        Where-Object {
            $_.Name -match "release" -and ($AllowUnsignedAndroid -or $_.Name -match "signed")
        })
    if ($androidArtifacts.Count -eq 0) {
        throw "Android release build completed without a release APK under $androidOutputRoot."
    }
    Write-Step "Android release artifact verified: $($androidArtifacts[-1].FullName)"

    $npm = Resolve-CommandPath "npm.cmd" "Install Node.js/npm and retry."
    Invoke-Checked "Build Windows NSIS installer" $npm @("run", "build:windows") $desktopDir
    $windowsOutputRoot = Join-Path $Root "desktop/src-tauri/target/release/bundle/nsis"
    $windowsArtifacts = @(Get-ChildItem -LiteralPath $windowsOutputRoot -Recurse -File -Filter "*.exe" -ErrorAction SilentlyContinue)
    if ($windowsArtifacts.Count -eq 0) {
        throw "Windows release build completed without an NSIS installer under $windowsOutputRoot."
    }
    Write-Step "Windows NSIS artifact verified: $($windowsArtifacts[-1].FullName)"
}

Write-Host ""
Write-Step "Requested release checks completed for the Tauri/Rust runtime. Skipped suites and unevaluated device performance remain unverified."
