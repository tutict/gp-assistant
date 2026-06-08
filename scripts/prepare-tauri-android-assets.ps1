param()

$ErrorActionPreference = "Stop"

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$SourceDir = Join-Path $Root "app\static"
$OutputDir = Join-Path $Root "desktop\mobile-dist"
$StaticOutputDir = Join-Path $OutputDir "static"
$MobileDataScript = Join-Path $PSScriptRoot "build-mobile-tdx-dataset.py"
$MobileDataOutput = Join-Path $StaticOutputDir "mobile-market-data.json"
$AndroidManifest = Join-Path $Root "desktop\src-tauri\gen\android\app\src\main\AndroidManifest.xml"
$AndroidBuildGradle = Join-Path $Root "desktop\src-tauri\gen\android\app\build.gradle.kts"

function Resolve-Python {
    $candidates = @(
        (Join-Path $Root ".venv-cpython\Scripts\python.exe"),
        (Join-Path $Root ".venv\Scripts\python.exe"),
        "python"
    )

    foreach ($candidate in $candidates) {
        $command = Get-Command $candidate -ErrorAction SilentlyContinue
        if ($command) {
            return $command.Source
        }
    }

    throw "Python was not found. Create .venv-cpython or .venv, or add python to PATH."
}

function Assert-WorkspaceChildPath {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path,
        [Parameter(Mandatory = $true)]
        [string] $Parent
    )

    $parentFullPath = [System.IO.Path]::GetFullPath($Parent).TrimEnd("\", "/")
    $childFullPath = [System.IO.Path]::GetFullPath($Path).TrimEnd("\", "/")
    $prefix = $parentFullPath + [System.IO.Path]::DirectorySeparatorChar

    if (-not $childFullPath.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to write outside workspace: $childFullPath"
    }
}

function Update-AndroidProjectForLanImport {
    if (Test-Path -LiteralPath $AndroidManifest) {
        $manifest = Get-Content -LiteralPath $AndroidManifest -Raw
        if ($manifest -notmatch "android\.permission\.CAMERA") {
            $manifest = $manifest -replace (
                [regex]::Escape('    <uses-permission android:name="android.permission.INTERNET" />'),
                "    <uses-permission android:name=`"android.permission.INTERNET`" />`r`n" +
                "    <uses-permission android:name=`"android.permission.CAMERA`" />`r`n" +
                "    <uses-feature android:name=`"android.hardware.camera`" android:required=`"false`" />"
            )
            Set-Content -LiteralPath $AndroidManifest -Value $manifest -Encoding UTF8
        }
    }

    if (Test-Path -LiteralPath $AndroidBuildGradle) {
        $gradle = Get-Content -LiteralPath $AndroidBuildGradle -Raw
        $updated = $gradle -replace 'manifestPlaceholders\["usesCleartextTraffic"\]\s*=\s*"false"', 'manifestPlaceholders["usesCleartextTraffic"] = "true"'
        if ($updated -ne $gradle) {
            Set-Content -LiteralPath $AndroidBuildGradle -Value $updated -Encoding UTF8
        }
    }
}

if (-not (Test-Path -LiteralPath $SourceDir)) {
    throw "Frontend source directory does not exist: $SourceDir"
}

Assert-WorkspaceChildPath $OutputDir $Root

if (Test-Path -LiteralPath $OutputDir) {
    Remove-Item -LiteralPath $OutputDir -Recurse -Force
}

New-Item -ItemType Directory -Path $StaticOutputDir -Force | Out-Null

Copy-Item -LiteralPath (Join-Path $SourceDir "index.html") -Destination (Join-Path $OutputDir "index.html") -Force

Get-ChildItem -LiteralPath $SourceDir -Force |
    Where-Object { $_.Name -ne "index.html" } |
    ForEach-Object {
        Copy-Item -LiteralPath $_.FullName -Destination $StaticOutputDir -Recurse -Force
    }

$Python = Resolve-Python
& $Python $MobileDataScript --output $MobileDataOutput
if ($LASTEXITCODE -ne 0) {
    throw "Build mobile TDX data set failed with exit code $LASTEXITCODE."
}

Update-AndroidProjectForLanImport

Write-Host "Prepared Tauri Android assets at: $OutputDir"
