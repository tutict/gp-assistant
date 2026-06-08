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
    Use-EnvPathFallback "JAVA_HOME" $AndroidJdk

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
    Use-LocalGradleDistribution

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
