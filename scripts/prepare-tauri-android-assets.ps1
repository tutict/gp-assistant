param()

$ErrorActionPreference = "Stop"

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$SourceDir = Join-Path $Root "app\static"
$OutputDir = Join-Path $Root "desktop\mobile-dist"
$StaticOutputDir = Join-Path $OutputDir "static"
$AndroidManifest = Join-Path $Root "desktop\src-tauri\gen\android\app\src\main\AndroidManifest.xml"
$AndroidBuildGradle = Join-Path $Root "desktop\src-tauri\gen\android\app\build.gradle.kts"
$AndroidResDir = Join-Path $Root "desktop\src-tauri\gen\android\app\src\main\res"
$AndroidIconSource = Join-Path $Root "desktop\src-tauri\icons\icon.png"
$AndroidAppName = -join @([char]0x80A1, [char]0x9009, [char]0x4F18)
$AndroidThemeName = "GpAssistantTheme"
$AndroidBootColor = "#120E0D"
$AndroidLauncherColor = "#D9251D"

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
            $internetPermissionPattern = [regex]::Escape('    <uses-permission android:name="android.permission.INTERNET" />')
            $lanImportPermissions = @(
                "    <uses-permission android:name=`"android.permission.INTERNET`" />",
                "    <uses-permission android:name=`"android.permission.CAMERA`" />",
                "    <uses-feature android:name=`"android.hardware.camera`" android:required=`"false`" />"
            ) -join "`r`n"
            $manifest = $manifest -replace $internetPermissionPattern, $lanImportPermissions
            if ($manifest -notmatch "android\.permission\.CAMERA") {
                $manifestRootPattern = "<manifest([^>]*)>"
                $manifest = $manifest -replace $manifestRootPattern, "<manifest`$1>`r`n$lanImportPermissions"
            }
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
        [int] $Size,
        [double] $ContentScale = 1.0
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
                $contentSize = [Math]::Max(1, [int][Math]::Round($Size * $ContentScale))
                $offset = [int][Math]::Round(($Size - $contentSize) / 2)
                $graphics.DrawImage($sourceImage, $offset, $offset, $contentSize, $contentSize)
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

function Set-AndroidColorResource {
    param(
        [Parameter(Mandatory = $true)]
        [string] $ValuesDir,
        [Parameter(Mandatory = $true)]
        [string] $Name,
        [Parameter(Mandatory = $true)]
        [string] $Value
    )

    New-Item -ItemType Directory -Path $ValuesDir -Force | Out-Null
    $colorsPath = Join-Path $ValuesDir "colors.xml"
    if (Test-Path -LiteralPath $colorsPath) {
        $colorsXml = Get-Content -LiteralPath $colorsPath -Raw
    } else {
        $colorsXml = @(
            "<?xml version=`"1.0`" encoding=`"utf-8`"?>",
            "<resources>",
            "</resources>"
        ) -join "`r`n"
    }

    $colorLine = "    <color name=`"$Name`">$Value</color>"
    $colorPattern = "<color\s+name=`"$([regex]::Escape($Name))`">[^<]*</color>"
    if ($colorsXml -match $colorPattern) {
        $colorsXml = [regex]::Replace($colorsXml, $colorPattern, $colorLine)
    } else {
        $colorsXml = $colorsXml -replace "</resources>", "$colorLine`r`n</resources>"
    }

    Set-Content -LiteralPath $colorsPath -Value $colorsXml -Encoding UTF8
}

function Resolve-AndroidThemeParent {
    $themesPath = Join-Path (Join-Path $AndroidResDir "values") "themes.xml"
    if (Test-Path -LiteralPath $themesPath) {
        $themesXml = Get-Content -LiteralPath $themesPath -Raw
        $match = [regex]::Match($themesXml, '<style\s+name="([^"]+)"\s+parent="([^"]+)"')
        if ($match.Success -and $match.Groups[1].Value -ne $AndroidThemeName) {
            return "@style/$($match.Groups[1].Value)"
        }
    }

    return "Theme.MaterialComponents.DayNight.NoActionBar"
}

function Write-AndroidStartupTheme {
    param(
        [Parameter(Mandatory = $true)]
        [string] $ValuesDir,
        [Parameter(Mandatory = $true)]
        [string] $ThemeParent,
        [switch] $UseAndroid12Splash
    )

    New-Item -ItemType Directory -Path $ValuesDir -Force | Out-Null

    $themeItems = @(
        "        <item name=`"android:windowNoTitle`">true</item>",
        "        <item name=`"android:windowActionBar`">false</item>",
        "        <item name=`"android:windowBackground`">@color/gp_boot_background</item>",
        "        <item name=`"android:colorBackground`">@color/gp_boot_background</item>",
        "        <item name=`"android:forceDarkAllowed`">false</item>",
        "        <item name=`"android:statusBarColor`">@color/gp_boot_background</item>",
        "        <item name=`"android:navigationBarColor`">@color/gp_boot_background</item>",
        "        <item name=`"android:windowLightStatusBar`">false</item>",
        "        <item name=`"android:windowLightNavigationBar`">false</item>"
    )

    if ($UseAndroid12Splash) {
        $themeItems += @(
            "        <item name=`"android:windowSplashScreenBackground`">@color/gp_boot_background</item>",
            "        <item name=`"android:windowSplashScreenAnimatedIcon`">@mipmap/gp_splash_icon</item>"
        )
    }

    $styleLines = @(
        "<?xml version=`"1.0`" encoding=`"utf-8`"?>",
        "<resources>",
        "    <style name=`"$AndroidThemeName`" parent=`"$ThemeParent`">"
    )
    $styleLines += $themeItems
    $styleLines += @(
        "    </style>",
        "</resources>"
    )
    $stylesXml = $styleLines -join "`r`n"

    Set-Content -LiteralPath (Join-Path $ValuesDir "styles.xml") -Value $stylesXml -Encoding UTF8
}

function Update-AndroidManifestStartupTheme {
    if (-not (Test-Path -LiteralPath $AndroidManifest)) {
        return
    }

    $manifest = Get-Content -LiteralPath $AndroidManifest -Raw
    $themeAttribute = "android:theme=`"@style/$AndroidThemeName`""
    if ($manifest -match "android:theme=`"[^`"]+`"") {
        $manifest = [regex]::Replace($manifest, "android:theme=`"[^`"]+`"", $themeAttribute)
    } elseif ($manifest -match "<application(\s|>)") {
        $manifest = $manifest -replace "<application(\s|>)", "<application $themeAttribute`$1"
    }

    Set-Content -LiteralPath $AndroidManifest -Value $manifest -Encoding UTF8
}

function Update-AndroidStartupTheme {
    if (-not (Test-Path -LiteralPath $AndroidResDir)) {
        return
    }

    $valuesDirs = @(
        (Join-Path $AndroidResDir "values"),
        (Join-Path $AndroidResDir "values-night"),
        (Join-Path $AndroidResDir "values-v31")
    )

    foreach ($valuesDir in $valuesDirs) {
        Set-AndroidColorResource $valuesDir "ic_launcher_background" $AndroidLauncherColor
        Set-AndroidColorResource $valuesDir "gp_boot_background" $AndroidBootColor
    }

    $themeParent = Resolve-AndroidThemeParent
    Write-AndroidStartupTheme (Join-Path $AndroidResDir "values") $themeParent
    Write-AndroidStartupTheme (Join-Path $AndroidResDir "values-night") $themeParent
    Write-AndroidStartupTheme (Join-Path $AndroidResDir "values-v31") $themeParent -UseAndroid12Splash
    Update-AndroidManifestStartupTheme
}

function Write-AndroidLauncherFallbackVectors {
    $drawableDir = Join-Path $AndroidResDir "drawable"
    $drawableV24Dir = Join-Path $AndroidResDir "drawable-v24"
    New-Item -ItemType Directory -Path $drawableDir -Force | Out-Null
    New-Item -ItemType Directory -Path $drawableV24Dir -Force | Out-Null

    $backgroundVector = @(
        "<?xml version=`"1.0`" encoding=`"utf-8`"?>",
        "<vector xmlns:android=`"http://schemas.android.com/apk/res/android`"",
        "    android:width=`"108dp`"",
        "    android:height=`"108dp`"",
        "    android:viewportWidth=`"108`"",
        "    android:viewportHeight=`"108`">",
        "    <path android:fillColor=`"$AndroidLauncherColor`" android:pathData=`"M0,0h108v108h-108z`" />",
        "    <path android:fillColor=`"#00000000`" android:pathData=`"M12,0L12,108`" android:strokeWidth=`"0.8`" android:strokeColor=`"#24FFF7F3`" />",
        "    <path android:fillColor=`"#00000000`" android:pathData=`"M36,0L36,108`" android:strokeWidth=`"0.8`" android:strokeColor=`"#20FFF7F3`" />",
        "    <path android:fillColor=`"#00000000`" android:pathData=`"M60,0L60,108`" android:strokeWidth=`"0.8`" android:strokeColor=`"#20FFF7F3`" />",
        "    <path android:fillColor=`"#00000000`" android:pathData=`"M84,0L84,108`" android:strokeWidth=`"0.8`" android:strokeColor=`"#24FFF7F3`" />",
        "</vector>"
    ) -join "`r`n"
    Set-Content -LiteralPath (Join-Path $drawableDir "ic_launcher_background.xml") -Value $backgroundVector -Encoding UTF8

    $foregroundVector = @(
        "<?xml version=`"1.0`" encoding=`"utf-8`"?>",
        "<vector xmlns:android=`"http://schemas.android.com/apk/res/android`"",
        "    android:width=`"108dp`"",
        "    android:height=`"108dp`"",
        "    android:viewportWidth=`"108`"",
        "    android:viewportHeight=`"108`">",
        "    <path android:fillColor=`"#FFF7F3`" android:pathData=`"M54,18L83,76H70L64,64H44L38,76H25L54,18zM49,52h10l-5,-12z`" />",
        "    <path android:fillColor=`"#FFF7F3`" android:pathData=`"M28,84h52v8h-52z`" />",
        "</vector>"
    ) -join "`r`n"
    Set-Content -LiteralPath (Join-Path $drawableV24Dir "ic_launcher_foreground.xml") -Value $foregroundVector -Encoding UTF8
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

    Set-AndroidColorResource $valuesDir "ic_launcher_background" $AndroidLauncherColor
    Write-AndroidLauncherFallbackVectors

    $launcherSizes = @{
        "mipmap-mdpi" = 48
        "mipmap-hdpi" = 72
        "mipmap-xhdpi" = 96
        "mipmap-xxhdpi" = 144
        "mipmap-xxxhdpi" = 192
    }
    foreach ($entry in $launcherSizes.GetEnumerator()) {
        $directory = Join-Path $AndroidResDir $entry.Key
        foreach ($fileName in @("ic_launcher.png", "ic_launcher_round.png")) {
            Resize-Png $AndroidIconSource (Join-Path $directory $fileName) $entry.Value
        }
        Resize-Png $AndroidIconSource (Join-Path $directory "ic_launcher_foreground.png") $entry.Value 0.76
        Resize-Png $AndroidIconSource (Join-Path $directory "gp_splash_icon.png") $entry.Value 0.68
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

Update-AndroidProjectForLanImport
Update-AndroidProjectBranding
Update-AndroidStartupTheme

Write-Host "Prepared Tauri Android assets at: $OutputDir"
