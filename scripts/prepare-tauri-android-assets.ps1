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
$AndroidResDir = Join-Path $Root "desktop\src-tauri\gen\android\app\src\main\res"
$AndroidIconSource = Join-Path $Root "desktop\src-tauri\icons\icon.png"
$AndroidAppName = -join @("A", [char]0x80A1, [char]0x9009, [char]0x80A1, [char]0x667A, [char]0x80FD, [char]0x4F53)

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

function Resize-Png {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Source,
        [Parameter(Mandatory = $true)]
        [string] $Destination,
        [Parameter(Mandatory = $true)]
        [int] $Size
    )

    Add-Type -AssemblyName System.Drawing
    $sourceImage = [System.Drawing.Image]::FromFile($Source)
    try {
        $bitmap = New-Object System.Drawing.Bitmap $Size, $Size
        try {
            $bitmap.SetResolution($sourceImage.HorizontalResolution, $sourceImage.VerticalResolution)
            $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
            try {
                $graphics.Clear([System.Drawing.Color]::Transparent)
                $graphics.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceOver
                $graphics.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
                $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
                $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
                $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
                $graphics.DrawImage($sourceImage, 0, 0, $Size, $Size)
            } finally {
                $graphics.Dispose()
            }

            $directory = Split-Path -Parent $Destination
            New-Item -ItemType Directory -Path $directory -Force | Out-Null
            $bitmap.Save($Destination, [System.Drawing.Imaging.ImageFormat]::Png)
        } finally {
            $bitmap.Dispose()
        }
    } finally {
        $sourceImage.Dispose()
    }
}

function Update-AndroidProjectBranding {
    if (-not (Test-Path -LiteralPath $AndroidResDir)) {
        return
    }
    if (-not (Test-Path -LiteralPath $AndroidIconSource)) {
        throw "Android icon source does not exist: $AndroidIconSource"
    }

    $valuesDir = Join-Path $AndroidResDir "values"
    New-Item -ItemType Directory -Path $valuesDir -Force | Out-Null
    $escapedAppName = [System.Security.SecurityElement]::Escape($AndroidAppName)
    $stringsXml = @(
        "<resources>",
        "    <string name=`"app_name`">$escapedAppName</string>",
        "    <string name=`"main_activity_title`">$escapedAppName</string>",
        "</resources>"
    ) -join "`r`n"
    Set-Content -LiteralPath (Join-Path $valuesDir "strings.xml") -Value $stringsXml -Encoding UTF8

    $colorsPath = Join-Path $valuesDir "colors.xml"
    if (Test-Path -LiteralPath $colorsPath) {
        $colorsXml = Get-Content -LiteralPath $colorsPath -Raw
        if ($colorsXml -notmatch 'name="ic_launcher_background"') {
            $launcherBackgroundColor = '    <color name="ic_launcher_background">#121A22</color>' + "`r`n</resources>"
            $colorsXml = $colorsXml -replace '</resources>', $launcherBackgroundColor
            Set-Content -LiteralPath $colorsPath -Value $colorsXml -Encoding UTF8
        }
    } else {
        $colorsXml = @(
            "<?xml version=`"1.0`" encoding=`"utf-8`"?>",
            "<resources>",
            "    <color name=`"ic_launcher_background`">#121A22</color>",
            "</resources>"
        ) -join "`r`n"
        Set-Content -LiteralPath $colorsPath -Value $colorsXml -Encoding UTF8
    }

    $launcherSizes = @{
        "mipmap-mdpi" = 48
        "mipmap-hdpi" = 72
        "mipmap-xhdpi" = 96
        "mipmap-xxhdpi" = 144
        "mipmap-xxxhdpi" = 192
    }
    foreach ($entry in $launcherSizes.GetEnumerator()) {
        $directory = Join-Path $AndroidResDir $entry.Key
        foreach ($fileName in @("ic_launcher.png", "ic_launcher_round.png", "ic_launcher_foreground.png")) {
            Resize-Png $AndroidIconSource (Join-Path $directory $fileName) $entry.Value
        }
    }

    $adaptiveIconDir = Join-Path $AndroidResDir "mipmap-anydpi-v26"
    New-Item -ItemType Directory -Path $adaptiveIconDir -Force | Out-Null
    $adaptiveIconXml = @(
        "<?xml version=`"1.0`" encoding=`"utf-8`"?>",
        "<adaptive-icon xmlns:android=`"http://schemas.android.com/apk/res/android`">",
        "    <background android:drawable=`"@color/ic_launcher_background`" />",
        "    <foreground android:drawable=`"@mipmap/ic_launcher_foreground`" />",
        "</adaptive-icon>"
    ) -join "`r`n"
    Set-Content -LiteralPath (Join-Path $adaptiveIconDir "ic_launcher.xml") -Value $adaptiveIconXml -Encoding UTF8
    Set-Content -LiteralPath (Join-Path $adaptiveIconDir "ic_launcher_round.xml") -Value $adaptiveIconXml -Encoding UTF8
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
Update-AndroidProjectBranding

Write-Host "Prepared Tauri Android assets at: $OutputDir"
