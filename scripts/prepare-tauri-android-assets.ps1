param()

$ErrorActionPreference = "Stop"

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$SourceDir = Join-Path $Root "app\static"
$OutputDir = Join-Path $Root "desktop\mobile-dist"
$StaticOutputDir = Join-Path $OutputDir "static"
$MobileDataScript = Join-Path $PSScriptRoot "build-mobile-tdx-dataset.py"
$MobileDataOutput = Join-Path $StaticOutputDir "mobile-market-data.json"

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

Write-Host "Prepared Tauri Android assets at: $OutputDir"
