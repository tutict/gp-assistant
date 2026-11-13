param(
    [switch] $Install,
    [switch] $SkipDownload
)

$ErrorActionPreference = "Stop"

# Mobile screenshot baselines use Noto Sans SC. Without it, hosted Windows
# falls back to Microsoft YaHei UI and changes text rasterization and layout.
# This official font is licensed under the SIL Open Font License 1.1.
$workspaceRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$fontDirectory = Join-Path $workspaceRoot "artifacts\ui-fonts"
$fontPath = Join-Path $fontDirectory "NotoSansSC-VF.ttf"
$fontRevision = "165c01b46ea533872e002e0785ff17e44f6d97d8"
$fontUrl = "https://raw.githubusercontent.com/notofonts/noto-cjk/$fontRevision/Sans/Variable/TTF/Subset/NotoSansSC-VF.ttf"
$fontSha256 = "D68BAFCB48A2707749396AA12BBBD833CB70401F3A9A689FD2902C7E0D295964"

New-Item -ItemType Directory -Path $fontDirectory -Force | Out-Null
$valid = (Test-Path -LiteralPath $fontPath) -and (
    (Get-FileHash -LiteralPath $fontPath -Algorithm SHA256).Hash -eq $fontSha256
)
if (-not $valid) {
    if ($SkipDownload) {
        throw "UI baseline font is missing or invalid: $fontPath"
    }
    $temporary = "$fontPath.download"
    try {
        Invoke-WebRequest -Uri $fontUrl -OutFile $temporary
        if ((Get-FileHash -LiteralPath $temporary -Algorithm SHA256).Hash -ne $fontSha256) {
            throw "UI baseline font checksum mismatch"
        }
        Move-Item -LiteralPath $temporary -Destination $fontPath -Force
    } finally {
        if (Test-Path -LiteralPath $temporary) {
            Remove-Item -LiteralPath $temporary -Force
        }
    }
}
Write-Output "Verified Noto Sans SC font: $fontPath (SHA-256 $fontSha256)"

if ($Install) {
    if ($env:OS -ne "Windows_NT") {
        throw "Installing the UI baseline font requires Windows"
    }
    $userFontDirectory = Join-Path $env:LOCALAPPDATA "Microsoft\Windows\Fonts"
    New-Item -ItemType Directory -Path $userFontDirectory -Force | Out-Null
    $installedFontPath = Join-Path $userFontDirectory "GpAssistant-NotoSansSC-VF.ttf"
    Copy-Item -LiteralPath $fontPath -Destination $installedFontPath -Force
    $fontRegistry = "HKCU:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts"
    New-Item -Path $fontRegistry -Force | Out-Null
    New-ItemProperty -Path $fontRegistry -Name "GpAssistant UI baseline Noto Sans SC (TrueType)" `
        -Value $installedFontPath -PropertyType String -Force | Out-Null
    Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
namespace GpAssistant {
    public static class UIBaselineFonts {
        [DllImport("gdi32.dll", CharSet = CharSet.Unicode, ExactSpelling = true)]
        public static extern int AddFontResourceExW(string file, uint flags, IntPtr reserved);
    }
}
"@
    $registeredFaces = [GpAssistant.UIBaselineFonts]::AddFontResourceExW($installedFontPath, 0, [IntPtr]::Zero)
    if ($registeredFaces -le 0) {
        throw "Windows failed to register the verified UI baseline font"
    }
    Write-Output "Registered Noto Sans SC for the current Windows user: $registeredFaces font faces"
}
