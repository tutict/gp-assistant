param(
    [switch] $InitOnly,
    [switch] $Debug,
    [switch] $Aab,
    [switch] $SplitPerAbi,
    [string[]] $Target = @("aarch64")
)

$ErrorActionPreference = "Stop"

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$DesktopDir = Join-Path $Root "desktop"
$AndroidProjectDir = Join-Path $Root "desktop\src-tauri\gen\android"
$PrepareAssetsScript = Join-Path $PSScriptRoot "prepare-tauri-android-assets.ps1"

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

    Remove-Item -LiteralPath $childFullPath -Recurse -Force
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

function Update-AndroidProjectForLanImport {
    $AndroidManifest = Join-Path $AndroidProjectDir "app\src\main\AndroidManifest.xml"
    $AndroidBuildGradle = Join-Path $AndroidProjectDir "app\build.gradle.kts"

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

Initialize-AndroidEnvironment

Assert-EnvPath "ANDROID_HOME" "Install Android SDK and set ANDROID_HOME to the SDK directory."
Assert-EnvPath "NDK_HOME" "Install Android NDK and set NDK_HOME to the NDK directory."

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
} finally {
    Pop-Location
}
