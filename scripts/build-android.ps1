param(
    [switch] $InitOnly,
    [switch] $PreflightOnly,
    [switch] $Debug,
    [switch] $Aab,
    [switch] $SplitPerAbi,
    [switch] $Signed,
    [switch] $CreateSigningKey,
    [string[]] $Target = @("aarch64")
)

$ErrorActionPreference = "Stop"

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$DesktopDir = Join-Path $Root "desktop"
$AndroidProjectDir = Join-Path $Root "desktop\src-tauri\gen\android"
$PrepareAssetsScript = Join-Path $PSScriptRoot "prepare-tauri-android-assets.ps1"
$SigningDir = Join-Path $Root "desktop\src-tauri\keys"
$SigningConfigPath = Join-Path $SigningDir "android-signing.local.json"
$DefaultReleaseKeystore = Join-Path $SigningDir "guxuanyou-release.jks"
$DefaultReleaseKeyAlias = "guxuanyou-release"

function Use-EnvPathFallback {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Name,
        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    if (-not [Environment]::GetEnvironmentVariable($Name) -and (Test-Path -LiteralPath $Path)) {
        [Environment]::SetEnvironmentVariable($Name, $Path, "Process")
    }
}

function Initialize-AndroidEnvironment {
    $DefaultSdk = "C:\tmp\android-sdk"
    Use-EnvPathFallback "ANDROID_HOME" $DefaultSdk
    Use-EnvPathFallback "ANDROID_SDK_ROOT" $DefaultSdk

    $SdkRoot = [Environment]::GetEnvironmentVariable("ANDROID_HOME")
    if (-not [Environment]::GetEnvironmentVariable("NDK_HOME") -and $SdkRoot) {
        $NdkRoot = Join-Path $SdkRoot "ndk"
        if (Test-Path -LiteralPath $NdkRoot) {
            $LatestNdk = Get-ChildItem -LiteralPath $NdkRoot -Directory |
                Sort-Object Name -Descending |
                Select-Object -First 1
            if ($LatestNdk) {
                [Environment]::SetEnvironmentVariable("NDK_HOME", $LatestNdk.FullName, "Process")
            }
        }
    }

    $AndroidJdk = "C:\Program Files\Android\openjdk\jdk-21.0.8"
    $AndroidJdkOverride = [Environment]::GetEnvironmentVariable("GP_ANDROID_JAVA_HOME")
    if ($AndroidJdkOverride) {
        [Environment]::SetEnvironmentVariable("JAVA_HOME", $AndroidJdkOverride, "Process")
        $env:JAVA_HOME = $AndroidJdkOverride
    } elseif (Test-Path -LiteralPath $AndroidJdk) {
        [Environment]::SetEnvironmentVariable("JAVA_HOME", $AndroidJdk, "Process")
        $env:JAVA_HOME = $AndroidJdk
    } else {
        Use-EnvPathFallback "JAVA_HOME" $AndroidJdk
    }

    $JavaHome = [Environment]::GetEnvironmentVariable("JAVA_HOME")
    if ($JavaHome) {
        $env:PATH = "$JavaHome\bin;$env:PATH"
    }
    if ($SdkRoot) {
        $env:PATH = "$SdkRoot\cmdline-tools\latest\bin;$SdkRoot\platform-tools;$env:PATH"
    }
}

function Assert-EnvPath {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Name,
        [Parameter(Mandatory = $true)]
        [string] $InstallHint
    )

    $value = [Environment]::GetEnvironmentVariable($Name)
    if (-not $value -or -not (Test-Path -LiteralPath $value)) {
        throw "$Name is not set or does not exist. $InstallHint"
    }
}


function Resolve-JavaExecutable {
    $JavaHome = [Environment]::GetEnvironmentVariable("JAVA_HOME")
    if ($JavaHome) {
        $candidate = Join-Path $JavaHome "bin\java.exe"
        if (Test-Path -LiteralPath $candidate) {
            return $candidate
        }
    }

    $javaCommand = Get-Command java -ErrorAction SilentlyContinue
    if ($javaCommand) {
        return $javaCommand.Source
    }

    throw "java.exe was not found. Install JDK 17/21 or set GP_ANDROID_JAVA_HOME to the matching JDK."
}

function Get-JavaMajorVersion {
    param(
        [Parameter(Mandatory = $true)]
        [string] $JavaExe
    )

    $previousErrorActionPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = "Continue"
        $versionOutput = & $JavaExe -version 2>&1
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previousErrorActionPreference
    }
    if ($exitCode -ne 0) {
        throw "Java version check failed: $($versionOutput -join ' ')"
    }

    $versionText = ($versionOutput | ForEach-Object { [string]$_ }) -join "`n"
    $versionText = $versionText.Trim()
    $firstLine = [string]($versionOutput | Select-Object -First 1)
    if ($firstLine -match '"(?<major>[0-9]+)(?:\.(?<minor>[0-9]+))?') {
        $major = [int]$Matches.major
        if ($major -eq 1 -and $Matches.minor) {
            return [int]$Matches.minor
        }
        return $major
    }

    throw "Unable to detect Java version: $versionText"
}

function Assert-AndroidJavaVersion {
    $javaExe = Resolve-JavaExecutable
    $major = Get-JavaMajorVersion $javaExe
    if ($major -ne 17 -and $major -ne 21) {
        throw "Android build requires JDK 17 or 21; detected JDK ${major}: ${javaExe}. Install JDK 17/21 or set GP_ANDROID_JAVA_HOME to the matching JDK."
    }

    Write-Host "Android Java OK: JDK $major ($javaExe)"
}

function Invoke-Checked {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Description,
        [Parameter(Mandatory = $true)]
        [string] $FilePath,
        [Parameter(Mandatory = $true)]
        [string[]] $Arguments
    )

    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Description failed with exit code $LASTEXITCODE."
    }
}

function Get-RandomHexPassword {
    $bytes = New-Object byte[] 24
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    try {
        $rng.GetBytes($bytes)
    } finally {
        $rng.Dispose()
    }
    return ([System.BitConverter]::ToString($bytes) -replace "-", "").ToLowerInvariant()
}

function Resolve-KeytoolExecutable {
    $JavaHome = [Environment]::GetEnvironmentVariable("JAVA_HOME")
    if ($JavaHome) {
        $candidate = Join-Path $JavaHome "bin\keytool.exe"
        if (Test-Path -LiteralPath $candidate) {
            return $candidate
        }
    }

    $command = Get-Command keytool -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }

    throw "Android release signing requires keytool.exe. Set GP_ANDROID_JAVA_HOME to JDK 17/21 and retry."
}

function Resolve-AndroidBuildToolExecutable {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Name
    )

    $SdkRoot = [Environment]::GetEnvironmentVariable("ANDROID_HOME")
    if (-not $SdkRoot) {
        $SdkRoot = [Environment]::GetEnvironmentVariable("ANDROID_SDK_ROOT")
    }
    if (-not $SdkRoot) {
        throw "ANDROID_HOME is not set; cannot locate Android build-tools for $Name."
    }

    $BuildToolsRoot = Join-Path $SdkRoot "build-tools"
    if (-not (Test-Path -LiteralPath $BuildToolsRoot)) {
        throw "Android build-tools not found under $BuildToolsRoot. Install build-tools with sdkmanager."
    }

    $tool = Get-ChildItem -LiteralPath $BuildToolsRoot -Directory |
        Sort-Object Name -Descending |
        ForEach-Object { Join-Path $_.FullName $Name } |
        Where-Object { Test-Path -LiteralPath $_ } |
        Select-Object -First 1
    if (-not $tool) {
        throw "Android build-tool $Name was not found under $BuildToolsRoot."
    }
    return $tool
}

function New-AndroidReleaseKeystore {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Keystore,
        [Parameter(Mandatory = $true)]
        [string] $Alias,
        [Parameter(Mandatory = $true)]
        [string] $StorePassword,
        [Parameter(Mandatory = $true)]
        [string] $KeyPassword
    )

    if (Test-Path -LiteralPath $Keystore) {
        return
    }

    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Keystore) | Out-Null
    $keytool = Resolve-KeytoolExecutable
    Invoke-Checked "Create Android release keystore" $keytool @(
        "-genkeypair",
        "-v",
        "-keystore", $Keystore,
        "-alias", $Alias,
        "-keyalg", "RSA",
        "-keysize", "2048",
        "-validity", "10000",
        "-storepass", $StorePassword,
        "-keypass", $KeyPassword,
        "-dname", "CN=GuXuanYou, OU=Release, O=GuXuanYou, L=Shanghai, ST=Shanghai, C=CN"
    )
}

function Get-AndroidSigningConfig {
    $EnvKeystore = [Environment]::GetEnvironmentVariable("GP_ANDROID_KEYSTORE")
    $EnvAlias = [Environment]::GetEnvironmentVariable("GP_ANDROID_KEY_ALIAS")
    $EnvStorePassword = [Environment]::GetEnvironmentVariable("GP_ANDROID_KEYSTORE_PASSWORD")
    $EnvKeyPassword = [Environment]::GetEnvironmentVariable("GP_ANDROID_KEY_PASSWORD")

    if ($EnvKeystore -or $EnvAlias -or $EnvStorePassword -or $EnvKeyPassword) {
        if (-not $EnvKeystore -or -not $EnvAlias -or -not $EnvStorePassword) {
            throw "GP_ANDROID_KEYSTORE, GP_ANDROID_KEY_ALIAS, and GP_ANDROID_KEYSTORE_PASSWORD must all be set for env-based signing."
        }
        if (-not $EnvKeyPassword) {
            $EnvKeyPassword = $EnvStorePassword
        }
        return [pscustomobject]@{
            Keystore = $EnvKeystore
            Alias = $EnvAlias
            StorePassword = $EnvStorePassword
            KeyPassword = $EnvKeyPassword
            Source = "environment"
        }
    }

    if (Test-Path -LiteralPath $SigningConfigPath) {
        $config = Get-Content -LiteralPath $SigningConfigPath -Raw | ConvertFrom-Json
        if (-not $config.keystore -or -not $config.alias -or -not $config.store_password) {
            throw "Local Android signing config is incomplete: $SigningConfigPath"
        }
        if ($CreateSigningKey) {
            New-AndroidReleaseKeystore $config.keystore $config.alias $config.store_password $config.store_password
        }
        return [pscustomobject]@{
            Keystore = $config.keystore
            Alias = $config.alias
            StorePassword = $config.store_password
            KeyPassword = $config.store_password
            Source = "local"
        }
    }

    if (-not $CreateSigningKey) {
        throw "Signed Android build requested, but no signing config was found. Re-run with -CreateSigningKey, or set GP_ANDROID_KEYSTORE / GP_ANDROID_KEY_ALIAS / GP_ANDROID_KEYSTORE_PASSWORD / GP_ANDROID_KEY_PASSWORD."
    }

    New-Item -ItemType Directory -Force -Path $SigningDir | Out-Null
    $storePassword = Get-RandomHexPassword
    $keyPassword = $storePassword
    New-AndroidReleaseKeystore $DefaultReleaseKeystore $DefaultReleaseKeyAlias $storePassword $keyPassword
    $localConfig = [ordered]@{
        keystore = $DefaultReleaseKeystore
        alias = $DefaultReleaseKeyAlias
        store_password = $storePassword
        key_password = $storePassword
    }
    $localConfig | ConvertTo-Json | Set-Content -LiteralPath $SigningConfigPath -Encoding UTF8
    Write-Host "Created local Android release signing config: $SigningConfigPath"
    Write-Host "Keep this keystore and config. Future Android upgrades need the same signing key."

    return [pscustomobject]@{
        Keystore = $DefaultReleaseKeystore
        Alias = $DefaultReleaseKeyAlias
        StorePassword = $storePassword
        KeyPassword = $storePassword
        Source = "generated"
    }
}

function Find-UnsignedReleaseApk {
    $ApkOutputRoot = Join-Path $AndroidProjectDir "app\build\outputs\apk"
    if (-not (Test-Path -LiteralPath $ApkOutputRoot)) {
        throw "Android APK output directory not found: $ApkOutputRoot"
    }

    $apk = Get-ChildItem -LiteralPath $ApkOutputRoot -Recurse -File -Filter "*.apk" |
        Where-Object { $_.Name -match "release" -and $_.Name -match "unsigned" } |
        Sort-Object LastWriteTimeUtc -Descending |
        Select-Object -First 1
    if (-not $apk) {
        throw "No unsigned release APK was found under $ApkOutputRoot."
    }
    return $apk
}

function Sign-AndroidReleaseApk {
    if (-not $Signed) {
        return $null
    }
    if ($Debug -or $Aab) {
        throw "-Signed currently supports release APK builds only. Remove -Debug/-Aab or sign those artifacts with a dedicated flow."
    }

    $config = Get-AndroidSigningConfig
    if (-not (Test-Path -LiteralPath $config.Keystore)) {
        throw "Android release keystore does not exist: $($config.Keystore)"
    }

    $unsignedApk = Find-UnsignedReleaseApk
    $zipalign = Resolve-AndroidBuildToolExecutable "zipalign.exe"
    $apksigner = Resolve-AndroidBuildToolExecutable "apksigner.bat"
    $tauriConfigPath = Join-Path $Root "desktop\src-tauri\tauri.conf.json"
    $appVersion = [string]((Get-Content -LiteralPath $tauriConfigPath -Raw | ConvertFrom-Json).version)
    if ($appVersion -notmatch '^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$') {
        throw "Invalid Tauri application version in ${tauriConfigPath}: $appVersion"
    }
    $targetLabel = ($Target | Where-Object { $_ } | ForEach-Object { $_ -replace '[^0-9A-Za-z_-]', '-' }) -join '-'
    if (-not $targetLabel) {
        $targetLabel = "universal"
    }
    $alignedApk = Join-Path $unsignedApk.DirectoryName "guxuanyou-release-aligned.apk"
    $signedApk = Join-Path $unsignedApk.DirectoryName "guxuanyou_${appVersion}_android_${targetLabel}_release_signed.apk"

    if (Test-Path -LiteralPath $alignedApk) {
        Remove-Item -LiteralPath $alignedApk -Force
    }
    if (Test-Path -LiteralPath $signedApk) {
        Remove-Item -LiteralPath $signedApk -Force
    }

    Invoke-Checked "zipalign Android release APK" $zipalign @(
        "-f",
        "-p",
        "4",
        $unsignedApk.FullName,
        $alignedApk
    )
    Invoke-Checked "Sign Android release APK" $apksigner @(
        "sign",
        "--ks", $config.Keystore,
        "--ks-key-alias", $config.Alias,
        "--ks-pass", "pass:$($config.StorePassword)",
        "--key-pass", "pass:$($config.KeyPassword)",
        "--out", $signedApk,
        $alignedApk
    )
    Invoke-Checked "Verify Android signed APK" $apksigner @(
        "verify",
        "--verbose",
        "--print-certs",
        $signedApk
    )
    Write-Host "Signed Android APK: $signedApk"
    return $signedApk
}


function Use-LocalGradleDistribution {
    $GradleZip = "C:\tmp\gradle-8.14.3-bin.zip"
    $WrapperProperties = Join-Path $AndroidProjectDir "gradle\wrapper\gradle-wrapper.properties"
    if (-not (Test-Path -LiteralPath $GradleZip) -or -not (Test-Path -LiteralPath $WrapperProperties)) {
        return
    }

    $GradleUrl = "file:///" + ($GradleZip -replace "\\", "/")
    $Updated = Get-Content -LiteralPath $WrapperProperties | ForEach-Object {
        if ($_ -like "distributionUrl=*") {
            "distributionUrl=$GradleUrl"
        } else {
            $_
        }
    }
    Set-Content -LiteralPath $WrapperProperties -Value $Updated -Encoding ASCII
}

function Remove-CheckedDirectory {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path,
        [Parameter(Mandatory = $true)]
        [string] $Parent
    )

    if (-not (Test-Path -LiteralPath $Path)) {
        return
    }

    $parentFullPath = [System.IO.Path]::GetFullPath($Parent).TrimEnd("\", "/")
    $childFullPath = [System.IO.Path]::GetFullPath($Path).TrimEnd("\", "/")
    $prefix = $parentFullPath + [System.IO.Path]::DirectorySeparatorChar
    if (-not $childFullPath.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to remove path outside expected parent: $childFullPath"
    }

    try {
        Remove-Item -LiteralPath $childFullPath -Recurse -Force
    } catch {
        Write-Warning "Skipping locked cleanup path: $childFullPath ($($_.Exception.Message))"
    }
}

function Clear-TauriAndroidPluginCache {
    $CargoHome = [Environment]::GetEnvironmentVariable("CARGO_HOME")
    if (-not $CargoHome) {
        $CargoHome = Join-Path $env:USERPROFILE ".cargo"
    }

    $RegistrySrc = Join-Path $CargoHome "registry\src"
    if (-not (Test-Path -LiteralPath $RegistrySrc)) {
        return
    }

    Get-ChildItem -LiteralPath $RegistrySrc -Directory | ForEach-Object {
        Get-ChildItem -LiteralPath $_.FullName -Directory -Filter "tauri-plugin-*" | ForEach-Object {
            $TauriApiCache = Join-Path $_.FullName "android\.tauri\tauri-api"
            Remove-CheckedDirectory $TauriApiCache $RegistrySrc
        }
    }
}

function Assert-AndroidProjectChildPath {
    param([Parameter(Mandatory = $true)][string] $Path)

    $androidProjectRoot = [System.IO.Path]::GetFullPath($AndroidProjectDir).TrimEnd("\", "/")
    $childFullPath = [System.IO.Path]::GetFullPath($Path).TrimEnd("\", "/")
    $prefix = $androidProjectRoot + [System.IO.Path]::DirectorySeparatorChar
    if (-not $childFullPath.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to write outside Android project: $childFullPath"
    }
}

function Write-AndroidNetworkSecurityConfig {
    if (-not (Test-Path -LiteralPath $AndroidProjectDir)) {
        return
    }

    $xmlDir = Join-Path $AndroidProjectDir "app\src\main\res\xml"
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
    $AndroidManifest = Join-Path $AndroidProjectDir "app\src\main\AndroidManifest.xml"
    $AndroidBuildGradle = Join-Path $AndroidProjectDir "app\build.gradle.kts"

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
Initialize-AndroidEnvironment

Assert-EnvPath "ANDROID_HOME" "Install Android SDK and set ANDROID_HOME to the SDK directory."
Assert-EnvPath "NDK_HOME" "Install Android NDK and set NDK_HOME to the NDK directory."
Assert-AndroidJavaVersion

if ($PreflightOnly) {
    Write-Host "Android preflight completed; build was not started."
    return
}

Push-Location $DesktopDir
try {
    if (-not (Test-Path -LiteralPath $AndroidProjectDir)) {
        Invoke-Checked "Initialize Tauri Android project" "npm.cmd" @(
            "run",
            "android:init"
        )
    }
} finally {
    Pop-Location
}

Invoke-Checked "Prepare Tauri Android frontend assets" "powershell.exe" @(
    "-NoProfile",
    "-ExecutionPolicy",
    "Bypass",
    "-File",
    $PrepareAssetsScript
)

Push-Location $DesktopDir
try {
    Update-AndroidProjectForLanImport
    Use-LocalGradleDistribution
    Clear-TauriAndroidPluginCache
    $GradleProblemsReportDir = Join-Path $AndroidProjectDir "build\reports\problems"
    Remove-CheckedDirectory $GradleProblemsReportDir $AndroidProjectDir

    if ($InitOnly) {
        Write-Host "Android project is initialized at: $AndroidProjectDir"
        return
    }

    $BuildArgs = @("exec", "tauri", "android", "build", "--")
    if ($Debug) {
        $BuildArgs += "--debug"
    }
    if ($Aab) {
        $BuildArgs += "--aab"
    } else {
        $BuildArgs += "--apk"
    }
    if ($SplitPerAbi) {
        $BuildArgs += "--split-per-abi"
    }
    foreach ($item in $Target) {
        if ($item) {
            $BuildArgs += @("--target", $item)
        }
    }
    $BuildArgs += "--ci"

    Invoke-Checked "Build Tauri Android package" "npm.cmd" $BuildArgs
    Sign-AndroidReleaseApk | Out-Null
} finally {
    Pop-Location
}
