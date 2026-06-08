$ErrorActionPreference = "Stop"

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$BinariesDir = Join-Path $Root "desktop\src-tauri\binaries"
$DistDir = Join-Path $Root "dist"
$EntryPoint = Join-Path $Root "app\desktop_server.py"
$Requirements = Join-Path $Root "requirements.txt"
$StaticDir = Join-Path $Root "app\static"
$PromptDir = Join-Path $Root "app\prompts"
$ModelDir = Join-Path $Root "models\bge-small-zh-v1.5-int8"

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

function Resolve-RustHost {
    $version = rustc -Vv
    $hostLine = $version | Where-Object { $_ -like "host:*" } | Select-Object -First 1
    if (-not $hostLine) {
        throw "Unable to detect Rust host triple from rustc -Vv."
    }

    return ($hostLine -replace "^host:\s*", "").Trim()
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

$Python = Resolve-Python
$HostTriple = Resolve-RustHost

New-Item -ItemType Directory -Force -Path $BinariesDir | Out-Null

Invoke-Checked "Install Python requirements" $Python @("-m", "pip", "install", "-r", $Requirements)
Invoke-Checked "Install PyInstaller" $Python @("-m", "pip", "install", "pyinstaller")

$PyInstallerArgs = @(
    "-m",
    "PyInstaller",
    "--clean",
    "--onefile",
    "--name",
    "gp-assistant-backend",
    "--collect-all",
    "onnxruntime",
    "--collect-all",
    "tokenizers",
    "--collect-all",
    "pytdx",
    "--distpath",
    $DistDir,
    "--workpath",
    (Join-Path $Root "build\pyinstaller"),
    "--specpath",
    (Join-Path $Root "build\pyinstaller"),
    "--add-data",
    "$StaticDir;app\static",
    "--add-data",
    "$PromptDir;app\prompts",
    $EntryPoint
)

if (Test-Path -LiteralPath $ModelDir) {
    $PyInstallerArgs = @(
        $PyInstallerArgs[0..($PyInstallerArgs.Length - 2)]
        "--add-data"
        "$ModelDir;models\bge-small-zh-v1.5-int8"
        $PyInstallerArgs[-1]
    )
}

Invoke-Checked "Build Tauri sidecar" $Python $PyInstallerArgs

$Extension = ""
if ($IsWindows -or $env:OS -eq "Windows_NT") {
    $Extension = ".exe"
}

$Source = Join-Path $DistDir "gp-assistant-backend$Extension"
$Target = Join-Path $BinariesDir "gp-assistant-backend-$HostTriple$Extension"
Copy-Item -LiteralPath $Source -Destination $Target -Force

Write-Host "Created Tauri sidecar: $Target"
