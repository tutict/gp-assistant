[CmdletBinding()]
param(
    [ValidateSet('status', 'prune')]
    [string]$Action = 'status',
    [int]$MaxMb = 200,
    [string]$AppDataDir
)

$ErrorActionPreference = 'Stop'

function Resolve-AppDataDir {
    param([string]$Override)
    if (-not [string]::IsNullOrWhiteSpace($Override)) {
        return [System.IO.Path]::GetFullPath($Override)
    }
    if ([string]::IsNullOrWhiteSpace($env:APPDATA)) {
        throw 'APPDATA is not set; pass -AppDataDir explicitly.'
    }
    return (Join-Path $env:APPDATA 'com.tutict.stockoptimizer')
}

function Get-FileInfoJson {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return @{ exists = $false; path = $Path; bytes = 0 }
    }
    $item = Get-Item -LiteralPath $Path
    return @{
        exists = $true
        path = $item.FullName
        bytes = [int64]$item.Length
        updated_at = $item.LastWriteTime.ToString('o')
    }
}

function Measure-TreeBytes {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { return 0 }
    return [int64]((Get-ChildItem -LiteralPath $Path -Recurse -File -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum).Sum)
}

function Remove-IfExists {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { return $false }
    Remove-Item -LiteralPath $Path -Recurse -Force
    return $true
}

$root = Resolve-AppDataDir $AppDataDir
$marketPath = Join-Path $root 'market\mobile-market-data.json'
$newsPath = Join-Path $root 'news\news-cache.json'
$ragPath = Join-Path $root 'rag\rag-pack.json'
$upstreamDesktopPath = Join-Path $root 'upstream_rag_desktop'
$upstreamMobilePath = Join-Path $root 'upstream_rag_mobile'

if ($Action -eq 'prune') {
    $removed = @()
    foreach ($path in @($marketPath, $newsPath, $ragPath, $upstreamDesktopPath)) {
        if (Remove-IfExists $path) { $removed += $path }
    }
}

$market = Get-FileInfoJson $marketPath
$news = Get-FileInfoJson $newsPath
$rag = Get-FileInfoJson $ragPath
$result = @{
    runtime = 'tauri-appdata'
    action = $Action
    app_data_dir = $root
    policy = @{ mode = 'light'; max_bytes = [int64]$MaxMb * 1024 * 1024 }
    caches = @{
        market = $market
        news = $news
        rag_pack = $rag
        upstream_rag_desktop = @{ exists = (Test-Path -LiteralPath $upstreamDesktopPath); path = $upstreamDesktopPath; bytes = Measure-TreeBytes $upstreamDesktopPath }
        upstream_rag_mobile = @{ exists = (Test-Path -LiteralPath $upstreamMobilePath); path = $upstreamMobilePath; bytes = Measure-TreeBytes $upstreamMobilePath }
    }
    cache_bytes = [int64]$market.bytes + [int64]$news.bytes + [int64]$rag.bytes + (Measure-TreeBytes $upstreamDesktopPath) + (Measure-TreeBytes $upstreamMobilePath)
    notes = @(
        'This script inspects and prunes the Tauri app-data caches without the retired service stack.',
        'Run .\start-tauri-dev.bat and use the app data-maintenance panel for online Tencent refresh, which requires Tauri AppHandle progress events.'
    )
}
if ($Action -eq 'prune') { $result.removed = $removed }

$result | ConvertTo-Json -Depth 8
