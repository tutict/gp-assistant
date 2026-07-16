param(
    [switch] $SkipAndroidProjectUpdates,
    [string] $FinancialSourcePath = ""
)

$ErrorActionPreference = "Stop"

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$FrontendDir = Join-Path $Root "desktop\frontend"
$OutputDir = Join-Path $Root "desktop\mobile-dist"
$AndroidManifest = Join-Path $Root "desktop\src-tauri\gen\android\app\src\main\AndroidManifest.xml"
$AndroidBuildGradle = Join-Path $Root "desktop\src-tauri\gen\android\app\build.gradle.kts"
$AndroidMainActivity = Join-Path $Root "desktop\src-tauri\gen\android\app\src\main\java\com\tutict\stockoptimizer\MainActivity.kt"
$AndroidResDir = Join-Path $Root "desktop\src-tauri\gen\android\app\src\main\res"
$AndroidIconSource = Join-Path $Root "desktop\src-tauri\icons\icon-v2.png"
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

function Invoke-Checked {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Description,
        [Parameter(Mandatory = $true)]
        [string] $FilePath,
        [Parameter(Mandatory = $true)]
        [string[]] $Arguments,
        [Parameter(Mandatory = $true)]
        [string] $WorkingDirectory
    )

    Write-Host $Description
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

function Build-ReactFrontend {
    if (-not (Test-Path -LiteralPath $FrontendDir)) {
        throw "React frontend directory does not exist: $FrontendDir"
    }

    if (-not (Test-Path -LiteralPath (Join-Path $FrontendDir "node_modules"))) {
        Invoke-Checked "Installing React frontend dependencies" "npm.cmd" @("--prefix", "desktop/frontend", "ci") $Root
    }

    if ($env:GP_SKIP_FRONTEND_BUILD -eq "1") {
        Write-Host "Skipping React/TypeScript frontend build; using existing mobile-dist"
    } else {
        Invoke-Checked "Building React/TypeScript frontend" "npm.cmd" @("run", "build") $FrontendDir
    }

    $indexPath = Join-Path $OutputDir "index.html"
    if (-not (Test-Path -LiteralPath $indexPath)) {
        throw "React frontend build did not produce expected file: $indexPath"
    }
}

function Assert-AndroidProjectChildPath {
    param([Parameter(Mandatory = $true)][string] $Path)

    $androidProjectRoot = [System.IO.Path]::GetFullPath((Join-Path $Root "desktop\src-tauri\gen\android")).TrimEnd("\", "/")
    $childFullPath = [System.IO.Path]::GetFullPath($Path).TrimEnd("\", "/")
    $prefix = $androidProjectRoot + [System.IO.Path]::DirectorySeparatorChar
    if (-not $childFullPath.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to write outside Android project: $childFullPath"
    }
}

function Write-AndroidNetworkSecurityConfig {
    $androidProjectRoot = Join-Path $Root "desktop\src-tauri\gen\android"
    if (-not (Test-Path -LiteralPath $androidProjectRoot)) {
        return
    }

    $xmlDir = Join-Path $AndroidResDir "xml"
    $configPath = Join-Path $xmlDir "guxuanyou_network_security_config.xml"
    Assert-AndroidProjectChildPath $configPath
    New-Item -ItemType Directory -Path $xmlDir -Force | Out-Null
    $config = @(
        '<?xml version="1.0" encoding="utf-8"?>',
        '<network-security-config>',
        '    <base-config cleartextTrafficPermitted="false" />',
        '    <domain-config cleartextTrafficPermitted="true">',
        '        <domain includeSubdomains="false">localhost</domain>',
        '        <domain includeSubdomains="false">127.0.0.1</domain>',
        '        <domain includeSubdomains="false">10.0.2.2</domain>',
        '    </domain-config>',
        '</network-security-config>',
        ''
    ) -join "`r`n"
    [System.IO.File]::WriteAllText($configPath, $config, [System.Text.UTF8Encoding]::new($false))
}

function Apply-AndroidApplicationNetworkSecurityConfig {
    param([Parameter(Mandatory = $true)][string] $Manifest)

    $updated = $Manifest -replace 'android:usesCleartextTraffic="true"', 'android:usesCleartextTraffic="false"'
    if ($updated -notmatch 'android:usesCleartextTraffic=') {
        $updated = $updated -replace '<application\b', '<application android:usesCleartextTraffic="false"'
    }
    if ($updated -notmatch 'android:networkSecurityConfig=') {
        $updated = $updated -replace '<application\b', '<application android:networkSecurityConfig="@xml/guxuanyou_network_security_config"'
    }
    return $updated
}

function Disable-AndroidCleartextPlaceholder {
    param([Parameter(Mandatory = $true)][string] $Gradle)

    $updated = $Gradle -replace 'manifestPlaceholders\["usesCleartextTraffic"\]\s*=\s*"true"', 'manifestPlaceholders["usesCleartextTraffic"] = "false"'
    $updated = $updated -replace 'manifestPlaceholders\["usesCleartextTraffic"\]\s*=\s*"false"', 'manifestPlaceholders["usesCleartextTraffic"] = "false"'
    return $updated
}

function Update-AndroidProjectForLanImport {
    Write-AndroidNetworkSecurityConfig

    if (Test-Path -LiteralPath $AndroidManifest) {
        $manifest = Get-Content -LiteralPath $AndroidManifest -Raw
        $manifestUpdated = $false
        $internetPermissionPattern = [regex]::Escape('    <uses-permission android:name="android.permission.INTERNET" />')
        $networkStatePermission = "    <uses-permission android:name=`"android.permission.ACCESS_NETWORK_STATE`" />"

        if ($manifest -notmatch "android\.permission\.CAMERA") {
            $lanImportPermissions = @(
                "    <uses-permission android:name=`"android.permission.INTERNET`" />",
                $networkStatePermission,
                "    <uses-permission android:name=`"android.permission.CAMERA`" />",
                "    <uses-feature android:name=`"android.hardware.camera`" android:required=`"false`" />"
            ) -join "`r`n"
            $manifest = $manifest -replace $internetPermissionPattern, $lanImportPermissions
            if ($manifest -notmatch "android\.permission\.CAMERA") {
                $manifestRootPattern = "<manifest([^>]*)>"
                $manifest = $manifest -replace $manifestRootPattern, "<manifest`$1>`r`n$lanImportPermissions"
            }
            $manifestUpdated = $true
        }

        if ($manifest -notmatch "android\.permission\.ACCESS_NETWORK_STATE") {
            if ($manifest -match $internetPermissionPattern) {
                $manifest = $manifest -replace $internetPermissionPattern, "    <uses-permission android:name=`"android.permission.INTERNET`" />`r`n$networkStatePermission"
            } else {
                $manifestRootPattern = "<manifest([^>]*)>"
                $manifest = $manifest -replace $manifestRootPattern, "<manifest`$1>`r`n$networkStatePermission"
            }
            $manifestUpdated = $true
        }

        $securedManifest = Apply-AndroidApplicationNetworkSecurityConfig $manifest
        if ($securedManifest -ne $manifest) {
            $manifest = $securedManifest
            $manifestUpdated = $true
        }

        if ($manifestUpdated) {
            Set-Content -LiteralPath $AndroidManifest -Value $manifest -Encoding UTF8
        }
    }

    if (Test-Path -LiteralPath $AndroidBuildGradle) {
        $gradle = Get-Content -LiteralPath $AndroidBuildGradle -Raw
        $updated = Disable-AndroidCleartextPlaceholder $gradle
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

function Update-AndroidManifestBranding {
    if (-not (Test-Path -LiteralPath $AndroidManifest)) {
        return
    }

    $manifest = Get-Content -LiteralPath $AndroidManifest -Raw
    if ($manifest -match "<application(\s|>)") {
        if ($manifest -match "android:icon=`"[^`"]+`"") {
            $manifest = [regex]::Replace($manifest, "android:icon=`"[^`"]+`"", "android:icon=`"@mipmap/ic_launcher`"", 1)
        } else {
            $manifest = $manifest -replace "<application(\s|>)", "<application android:icon=`"@mipmap/ic_launcher`"`$1"
        }

        if ($manifest -match "android:roundIcon=`"[^`"]+`"") {
            $manifest = [regex]::Replace($manifest, "android:roundIcon=`"[^`"]+`"", "android:roundIcon=`"@mipmap/ic_launcher_round`"", 1)
        } else {
            $manifest = $manifest -replace "<application(\s|>)", "<application android:roundIcon=`"@mipmap/ic_launcher_round`"`$1"
        }
    }

    Set-Content -LiteralPath $AndroidManifest -Value $manifest -Encoding UTF8
}
function Update-AndroidSafeAreaInsets {
    $mainActivityDir = Split-Path -Parent $AndroidMainActivity
    if (-not (Test-Path -LiteralPath $mainActivityDir)) {
        return
    }

    $mainActivity = @(
        "package com.tutict.stockoptimizer",
        "",
        "import android.os.Bundle",
        "import android.view.View",
        "import androidx.activity.enableEdgeToEdge",
        "import androidx.core.view.ViewCompat",
        "import androidx.core.view.WindowInsetsCompat",
        "",
        "class MainActivity : TauriActivity() {",
        "  override fun onCreate(savedInstanceState: Bundle?) {",
        "    enableEdgeToEdge()",
        "    super.onCreate(savedInstanceState)",
        "    applyTopSystemBarInset()",
        "  }",
        "",
        "  private fun applyTopSystemBarInset() {",
        "    val contentView = findViewById<View>(android.R.id.content) ?: return",
        "    ViewCompat.setOnApplyWindowInsetsListener(contentView) { view, insets ->",
        "      val systemBars = insets.getInsets(WindowInsetsCompat.Type.systemBars())",
        "      val cutout = insets.getInsets(WindowInsetsCompat.Type.displayCutout())",
        "      val topInset = maxOf(systemBars.top, cutout.top)",
        "      view.setPadding(view.paddingLeft, topInset, view.paddingRight, view.paddingBottom)",
        "      insets",
        "    }",
        "    ViewCompat.requestApplyInsets(contentView)",
        "  }",
        "}",
        ""
    ) -join "`r`n"

    Set-Content -LiteralPath $AndroidMainActivity -Value $mainActivity -Encoding UTF8
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

    $foregroundBitmap = @(
        "<?xml version=`"1.0`" encoding=`"utf-8`"?>",
        "<bitmap xmlns:android=`"http://schemas.android.com/apk/res/android`"",
        "    android:gravity=`"center`"",
        "    android:src=`"@mipmap/ic_launcher_foreground`" />"
    ) -join "`r`n"
    Set-Content -LiteralPath (Join-Path $drawableDir "ic_launcher_foreground.xml") -Value $foregroundBitmap -Encoding UTF8
    Set-Content -LiteralPath (Join-Path $drawableV24Dir "ic_launcher_foreground.xml") -Value $foregroundBitmap -Encoding UTF8
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
    Update-AndroidManifestBranding

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


function ConvertTo-MobileSnapshotNumber {
    param([object] $Value)

    if ($null -eq $Value) {
        return $null
    }

    $text = ([string] $Value).Trim()
    if ([string]::IsNullOrWhiteSpace($text) -or $text -eq "-") {
        return $null
    }

    $style = [System.Globalization.NumberStyles]::Float -bor [System.Globalization.NumberStyles]::AllowThousands
    $culture = [System.Globalization.CultureInfo]::InvariantCulture
    [double] $number = 0
    if ([double]::TryParse($text, $style, $culture, [ref] $number)) {
        if ([double]::IsNaN($number) -or [double]::IsInfinity($number)) {
            return $null
        }
        return $number
    }

    return $null
}

function ConvertTo-MobileSnapshotBillion {
    param([object] $Value)

    $number = ConvertTo-MobileSnapshotNumber $Value
    if ($null -eq $number) {
        return $null
    }
    # tdx_fundamentals.csv stores monetary statement fields in CNY yuan.
    # The mobile snapshot names use the existing *_billion convention but
    # display these values in Chinese 亿, so conversion must always be / 1e8.
    return $number / 100000000
}

function Normalize-MobileSnapshotCode {
    param([object] $Value)

    if ($null -eq $Value) {
        return $null
    }
    $raw = ([string] $Value).Trim().ToUpperInvariant()
    if (-not $raw) {
        return $null
    }
    if ($raw -match '^(?<digits>\d{6})\.(?<market>SH|SZ|BJ)$') {
        return "$($Matches.digits).$($Matches.market)"
    }
    if ($raw -match '^(?<market>SH|SZ|BJ)(?<digits>\d{6})$') {
        return "$($Matches.digits).$($Matches.market)"
    }
    if ($raw -match '(?<digits>\d{6})') {
        $digits = $Matches.digits
        $market = "SZ"
        if ($digits -match '^[695]') {
            $market = "SH"
        } elseif ($digits -match '^[48]') {
            $market = "BJ"
        }
        return "$digits.$market"
    }
    return $null
}

function ConvertTo-MobileSnapshotPeriodKey {
    param(
        [object] $ReportDate,
        [object] $ReportType
    )

    $text = ([string] $ReportDate).Trim()
    $year = $null
    $month = $null
    if ($text -match '(?<year>20\d{2})(?:[-/.\s]|\u5e74)*(?<month>0?[1-9]|1[0-2])') {
        $year = [int] $Matches.year
        $month = [int] $Matches.month
    }

    if ($null -eq $year -or $null -eq $month) {
        $typeText = ([string] $ReportType).Trim()
        if ($typeText -match '(?<year>20\d{2})') {
            $year = [int] $Matches.year
        }
        if ($typeText -match 'Q\s*(?<quarter>[1-4])') {
            return ('{0:D4}Q{1}' -f $year, [int] $Matches.quarter)
        }
        return $null
    }

    $quarter = switch ($month) {
        3 { 1 }
        6 { 2 }
        9 { 3 }
        12 { 4 }
        default { $null }
    }
    if ($null -eq $quarter) {
        return $null
    }
    return ('{0:D4}Q{1}' -f $year, $quarter)
}

function New-MobileFinancialEntry {
    param([string] $Code)

    return [ordered]@{
        code = $Code
        latest_period = $null
        latest_eps = $null
        latest_eps_period = $null
        latest_bps = $null
        latest_bps_period = $null
        latest_financial_period = $null
        deducted_net_profit_billion = $null
        deducted_net_profit_margin = $null
        deducted_net_profit_growth_rate = $null
        operating_revenue_billion = $null
        operating_revenue_yoy = $null
        parent_net_profit_billion = $null
        parent_net_profit_yoy = $null
        gross_margin = $null
        net_margin = $null
        roe = $null
        asset_liability_ratio = $null
        periods = @{}
        inferred_periods = @{}
    }
}

function Export-MobileFinancialSnapshot {
    $snapshotPath = Join-Path $OutputDir "mobile-financial-snapshot.json"
    $fundamentalsPath = if ([string]::IsNullOrWhiteSpace($FinancialSourcePath)) {
        Join-Path $Root "data\cache\tdx_fundamentals.csv"
    } else {
        [System.IO.Path]::GetFullPath($FinancialSourcePath)
    }
    $financials = [ordered]@{}
    $byCode = @{}
    $source = $null
    $notes = New-Object System.Collections.Generic.List[string]

    if (Test-Path -LiteralPath $fundamentalsPath) {
        $source = "data/cache/tdx_fundamentals.csv"
        $rows = Import-Csv -LiteralPath $fundamentalsPath -Encoding UTF8
        foreach ($row in $rows) {
            $code = Normalize-MobileSnapshotCode $row.SECUCODE
            if (-not $code) {
                $code = Normalize-MobileSnapshotCode $row.SECURITY_CODE
            }
            if (-not $code) {
                continue
            }

            if (-not $byCode.ContainsKey($code)) {
                $byCode[$code] = New-MobileFinancialEntry $code
            }
            $entry = $byCode[$code]
            $periodKey = ConvertTo-MobileSnapshotPeriodKey $row.REPORT_DATE $row.REPORT_DATE_NAME
            $eps = ConvertTo-MobileSnapshotNumber $row.EPSJB
            $bps = ConvertTo-MobileSnapshotNumber $row.BPS
            $profitBillion = ConvertTo-MobileSnapshotBillion $row.KCFJCXSYJLR
            $revenueBillion = ConvertTo-MobileSnapshotBillion $row.TOTALOPERATEREVE
            $margin = $null
            if ($null -ne $profitBillion -and $null -ne $revenueBillion -and $revenueBillion -gt 0) {
                $margin = ($profitBillion / $revenueBillion) * 100
            }
            $growth = ConvertTo-MobileSnapshotNumber $row.KCFJCXSYJLRTZ
            $epsGrowth = ConvertTo-MobileSnapshotNumber $row.EPSJBTZ
            $operatingRevenueYoy = ConvertTo-MobileSnapshotNumber $row.TOTALOPERATEREVETZ
            $parentNetProfitBillion = ConvertTo-MobileSnapshotBillion $row.PARENTNETPROFIT
            $parentNetProfitYoy = ConvertTo-MobileSnapshotNumber $row.PARENTNETPROFITTZ
            $grossMargin = ConvertTo-MobileSnapshotNumber $row.XSMLL
            $netMargin = ConvertTo-MobileSnapshotNumber $row.XSJLL
            $roe = ConvertTo-MobileSnapshotNumber $row.ROEJQ
            $assetLiabilityRatio = ConvertTo-MobileSnapshotNumber $row.ZCFZL

            if ($periodKey) {
                if ($null -eq $entry.latest_period -or [string] $periodKey -gt [string] $entry.latest_period) {
                    $entry.latest_period = $periodKey
                }
                if ($null -ne $eps -and -not $entry.periods.ContainsKey($periodKey)) {
                    $entry.periods[$periodKey] = $eps
                }
                if ($null -ne $eps -and $null -ne $epsGrowth -and $periodKey -match '^([0-9]{4})Q([1-4])$') {
                    $previousYear = [int] $Matches[1] - 1
                    $previousPeriod = ('{0:D4}Q{1}' -f $previousYear, [int] $Matches[2])
                    $denominator = 1.0 + ([double] $epsGrowth / 100.0)
                    if ([Math]::Abs($denominator) -gt 0.000001 -and -not $entry.periods.ContainsKey($previousPeriod) -and -not $entry.inferred_periods.ContainsKey($previousPeriod)) {
                        $entry.inferred_periods[$previousPeriod] = [double] $eps / $denominator
                    }
                }
                if ($null -ne $eps -and ($null -eq $entry.latest_eps_period -or [string] $periodKey -gt [string] $entry.latest_eps_period)) {
                    $entry.latest_eps = $eps
                    $entry.latest_eps_period = $periodKey
                }
                if ($null -ne $bps -and ($null -eq $entry.latest_bps_period -or [string] $periodKey -gt [string] $entry.latest_bps_period)) {
                    $entry.latest_bps = $bps
                    $entry.latest_bps_period = $periodKey
                }
                if (($null -ne $profitBillion -or
                    $null -ne $margin -or
                    $null -ne $growth -or
                    $null -ne $revenueBillion -or
                    $null -ne $operatingRevenueYoy -or
                    $null -ne $parentNetProfitBillion -or
                    $null -ne $parentNetProfitYoy -or
                    $null -ne $grossMargin -or
                    $null -ne $netMargin -or
                    $null -ne $roe -or
                    $null -ne $assetLiabilityRatio) -and
                    ($null -eq $entry.latest_financial_period -or [string] $periodKey -gt [string] $entry.latest_financial_period)) {
                    $entry.deducted_net_profit_billion = $profitBillion
                    $entry.deducted_net_profit_margin = $margin
                    $entry.deducted_net_profit_growth_rate = $growth
                    $entry.operating_revenue_billion = $revenueBillion
                    $entry.operating_revenue_yoy = $operatingRevenueYoy
                    $entry.parent_net_profit_billion = $parentNetProfitBillion
                    $entry.parent_net_profit_yoy = $parentNetProfitYoy
                    $entry.gross_margin = $grossMargin
                    $entry.net_margin = $netMargin
                    $entry.roe = $roe
                    $entry.asset_liability_ratio = $assetLiabilityRatio
                    $entry.latest_financial_period = $periodKey
                }
            }
        }

        foreach ($code in ($byCode.Keys | Sort-Object)) {
            $entry = $byCode[$code]
            $quarterly = New-Object System.Collections.Generic.List[object]
            $periodRows = New-Object System.Collections.Generic.List[object]
            foreach ($period in $entry.periods.Keys) {
                $periodRows.Add([pscustomobject]@{ period = $period; value = $entry.periods[$period]; source = $source; inferred = $false }) | Out-Null
            }
            foreach ($period in $entry.inferred_periods.Keys) {
                if (-not $entry.periods.ContainsKey($period)) {
                    $periodRows.Add([pscustomobject]@{ period = $period; value = $entry.inferred_periods[$period]; source = ($source + ' / inferred EPS YoY'); inferred = $true }) | Out-Null
                }
            }
            foreach ($row in ($periodRows | Sort-Object -Property period -Descending | Select-Object -First 12)) {
                if ($null -eq $row.value) {
                    continue
                }
                $quarterlyItem = [ordered]@{
                    period = $row.period
                    value = [Math]::Round([double] $row.value, 6)
                    source = $row.source
                }
                if ($row.inferred) {
                    $quarterlyItem.inferred = $true
                    $quarterlyItem.note = 'inferred_from_eps_yoy'
                }
                $quarterly.Add([pscustomobject]$quarterlyItem) | Out-Null
            }
            if ($null -eq $entry.deducted_net_profit_billion -and
                $null -eq $entry.deducted_net_profit_margin -and
                $null -eq $entry.deducted_net_profit_growth_rate -and
                $null -eq $entry.operating_revenue_billion -and
                $null -eq $entry.parent_net_profit_billion -and
                $null -eq $entry.gross_margin -and
                $null -eq $entry.net_margin -and
                $null -eq $entry.roe -and
                $null -eq $entry.asset_liability_ratio -and
                $null -eq $entry.latest_eps -and
                $null -eq $entry.latest_bps -and
                $quarterly.Count -eq 0) {
                continue
            }

            $period = $entry.latest_eps_period
            if (-not $period) { $period = $entry.latest_bps_period }
            if (-not $period) { $period = $entry.latest_period }
            $stock = [ordered]@{
                code = $code
                deducted_net_profit_billion = if ($null -eq $entry.deducted_net_profit_billion) { $null } else { [Math]::Round([double] $entry.deducted_net_profit_billion, 6) }
                deducted_net_profit_margin = if ($null -eq $entry.deducted_net_profit_margin) { $null } else { [Math]::Round([double] $entry.deducted_net_profit_margin, 6) }
                deducted_net_profit_growth_rate = if ($null -eq $entry.deducted_net_profit_growth_rate) { $null } else { [Math]::Round([double] $entry.deducted_net_profit_growth_rate, 6) }
                operating_revenue_billion = if ($null -eq $entry.operating_revenue_billion) { $null } else { [Math]::Round([double] $entry.operating_revenue_billion, 6) }
                operating_revenue_yoy = if ($null -eq $entry.operating_revenue_yoy) { $null } else { [Math]::Round([double] $entry.operating_revenue_yoy, 6) }
                parent_net_profit_billion = if ($null -eq $entry.parent_net_profit_billion) { $null } else { [Math]::Round([double] $entry.parent_net_profit_billion, 6) }
                parent_net_profit_yoy = if ($null -eq $entry.parent_net_profit_yoy) { $null } else { [Math]::Round([double] $entry.parent_net_profit_yoy, 6) }
                gross_margin = if ($null -eq $entry.gross_margin) { $null } else { [Math]::Round([double] $entry.gross_margin, 6) }
                net_margin = if ($null -eq $entry.net_margin) { $null } else { [Math]::Round([double] $entry.net_margin, 6) }
                roe = if ($null -eq $entry.roe) { $null } else { [Math]::Round([double] $entry.roe, 6) }
                asset_liability_ratio = if ($null -eq $entry.asset_liability_ratio) { $null } else { [Math]::Round([double] $entry.asset_liability_ratio, 6) }
                latest_eps = if ($null -eq $entry.latest_eps) { $null } else { [Math]::Round([double] $entry.latest_eps, 6) }
                latest_bps = if ($null -eq $entry.latest_bps) { $null } else { [Math]::Round([double] $entry.latest_bps, 6) }
                period = $period
                source = $source
                quarterly_eps = $quarterly
            }
            $financials[$code] = [pscustomobject][ordered]@{
                deducted_net_profit_billion = $stock.deducted_net_profit_billion
                deducted_net_profit_margin = $stock.deducted_net_profit_margin
                deducted_net_profit_growth_rate = $stock.deducted_net_profit_growth_rate
                latest_eps = $stock.latest_eps
                latest_bps = $stock.latest_bps
                operating_revenue_billion = $stock.operating_revenue_billion
                operating_revenue_yoy = $stock.operating_revenue_yoy
                parent_net_profit_billion = $stock.parent_net_profit_billion
                parent_net_profit_yoy = $stock.parent_net_profit_yoy
                gross_margin = $stock.gross_margin
                net_margin = $stock.net_margin
                roe = $stock.roe
                asset_liability_ratio = $stock.asset_liability_ratio
                period = $stock.period
                source = $stock.source
                quarterly_eps = $stock.quarterly_eps
            }
        }
        $notes.Add("tdx fundamentals snapshot exported for mobile deducted filters and EPS indicators") | Out-Null
    } else {
        if (Test-Path -LiteralPath $snapshotPath) {
            try {
                $existingSnapshot = Get-Content -LiteralPath $snapshotPath -Raw -Encoding UTF8 | ConvertFrom-Json
                $existingFinancialCount = @($existingSnapshot.financials.psobject.Properties).Count
                if ($existingFinancialCount -gt 0) {
                    Write-Host "Reused bundled financial snapshot with $existingFinancialCount entries at $snapshotPath"
                    return
                }
            } catch {
                Write-Warning "Bundled financial snapshot is unreadable: $($_.Exception.Message)"
            }
        }
        throw "Cannot prepare a financial snapshot: data/cache/tdx_fundamentals.csv is missing and no non-empty bundled baseline was copied by the frontend build."
    }

    $snapshot = [pscustomobject][ordered]@{
        schema_version = "mobile-financial-snapshot/v3"
        generated_at = (Get-Date).ToUniversalTime().ToString("o")
        source = $source
        stock_count = 0
        financial_count = $financials.Count
        financials = $financials
        notes = $notes
    }
    $json = $snapshot | ConvertTo-Json -Depth 10 -Compress
    [System.IO.File]::WriteAllText($snapshotPath, $json, [System.Text.UTF8Encoding]::new($false))
    Write-Host "Prepared bundled financial snapshot: $($financials.Count) entries at $snapshotPath"
}
Assert-WorkspaceChildPath $OutputDir $Root

Build-ReactFrontend

Export-MobileFinancialSnapshot

if (-not $SkipAndroidProjectUpdates) {
    Update-AndroidProjectForLanImport
    Update-AndroidProjectBranding
    Update-AndroidStartupTheme
    Update-AndroidSafeAreaInsets
}

Write-Host "Prepared Tauri React assets at: $OutputDir"
